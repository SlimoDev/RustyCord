//! Explicit, confirmed edits to the current account's global profile.
use super::*;

fn confirmed_bio(value: &Value) -> Option<String> {
    let value = value
        .get("user_profile")
        .and_then(|v| v.get("bio"))
        .or_else(|| value.get("bio"))
        .or_else(|| value.get("user").and_then(|v| v.get("bio")))?;
    if value.is_null() {
        Some(String::new())
    } else {
        value.as_str().map(str::to_owned)
    }
}
fn confirmed_status(value: &Value) -> Option<Value> {
    let status = value.get("custom_status")?;
    if status.is_null() {
        Some(Value::Null)
    } else if status.is_object()
        && status
            .get("text")
            .is_none_or(|v| v.is_string() || v.is_null())
        && (status.get("text").is_some()
            || status.get("emoji_id").is_some()
            || status.get("emoji_name").is_some())
    {
        Some(status.clone())
    } else {
        None
    }
}
fn status_body(text: &str, extra: &str) -> Result<Value, String> {
    if text.chars().count() > 128 {
        return Err("El estado supera 128 caracteres.".into());
    }
    let extra: Value = serde_json::from_str(extra).unwrap_or(Value::Null);
    let mut body = json!({"text":text,"expires_at":null});
    if let Some(id) = extra
        .get("emoji_id")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
    {
        if !parsing::valid_id(id) {
            return Err("Emoji de estado no válido.".into());
        }
        body["emoji_id"] = id.into();
    }
    if let Some(name) = extra
        .get("emoji_name")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
    {
        if name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err("Emoji de estado no válido.".into());
        }
        body["emoji_name"] = name.into();
    }
    Ok(
        if text.is_empty() && body.get("emoji_id").is_none() && body.get("emoji_name").is_none() {
            Value::Null
        } else {
            body
        },
    )
}

async fn execute_edit<F, R>(
    owner: &str,
    bio: bool,
    text: String,
    extra: String,
    mut request: F,
) -> Reply
where
    F: FnMut(Method, String, Option<Value>) -> R,
    R: std::future::Future<Output = Reply>,
{
    let body = if bio {
        if text.encode_utf16().count() > 190 {
            return Err("La biografía supera 190 caracteres.".into());
        }
        json!({"bio":text})
    } else {
        json!({"custom_status":status_body(&text, &extra)?})
    };
    let path = if bio {
        "users/@me/profile"
    } else {
        "users/@me/settings"
    };
    let mut response = request(Method::PATCH, path.into(), Some(body)).await?;
    let complete = if bio {
        confirmed_bio(&response).is_some()
    } else {
        confirmed_status(&response).is_some()
    };
    if !complete {
        response = request(Method::GET, if bio {format!("users/{owner}/profile")} else {path.into()}, None).await.map_err(|error|format!("El cambio se envió, pero no se pudo confirmar. Recarga antes de reintentar. {error}"))?;
    }
    if bio {
        confirmed_bio(&response)
            .map(|bio| json!({"bio":bio}))
            .ok_or_else(|| {
                "El servicio no confirmó la biografía. Recarga antes de reintentar.".into()
            })
    } else {
        confirmed_status(&response)
            .map(|status| json!({"custom_status":status}))
            .ok_or_else(|| "El servicio no confirmó el estado. Recarga antes de reintentar.".into())
    }
}

impl Controller {
    pub(super) fn profile_editor_action(&mut self, mut action: UiAction) -> Result<bool, String> {
        if !matches!(
            action.kind.as_str(),
            "profile-editor-load" | "save-profile-bio" | "custom-status"
        ) {
            return Ok(false);
        }
        let api = self.api()?;
        let owner = self.me();
        Self::valid(&owner)?;
        if action.kind == "profile-editor-load" {
            action.kind = "__profile-editor-load".into();
            self.launch(action, None, async move {
                let result = async {
                    let profile = api
                        .get(&format!("users/{owner}/profile"))
                        .await
                        .map_err(|e| e.to_string())?;
                    if profile
                        .get("user")
                        .and_then(|user| user.get("id"))
                        .and_then(Value::as_str)
                        .is_some_and(|id| id != owner)
                    {
                        return Err("El servicio devolvió otro perfil.".into());
                    }
                    let settings = api
                        .get("users/@me/settings")
                        .await
                        .map_err(|e| e.to_string())?;
                    let bio =
                        confirmed_bio(&profile).ok_or("No se pudo cargar la biografía actual.")?;
                    let status =
                        confirmed_status(&settings).ok_or("No se pudo cargar el estado actual.")?;
                    Ok::<_, String>(json!({"bio":bio,"custom_status":status}))
                }
                .await;
                Ok(match result {
                    Ok(value) => json!({"owner":owner,"value":value}),
                    Err(error) => json!({"owner":owner,"error":error}),
                })
            });
            return Ok(true);
        }
        let bio = action.kind == "save-profile-bio";
        if bio && action.text.encode_utf16().count() > 190 {
            return Err("La biografía supera 190 caracteres.".into());
        }
        if !bio {
            status_body(&action.text, &action.extra)?;
        }
        let pending = if bio {
            "profile-bio-pending"
        } else {
            "profile-status-pending"
        };
        if self.request_versions.contains_key(pending) {
            return Err("Ya hay un guardado pendiente. Espera a su confirmación.".into());
        }
        self.request_versions.insert(pending.into(), 1);
        let text = action.text.clone();
        let extra = action.extra.clone();
        action.kind = if bio {
            "__profile-bio"
        } else {
            "__profile-status"
        }
        .into();
        self.launch(action, None, async move {
            let result = execute_edit(&owner, bio, text, extra, move |method, path, body| {
                let api = api.clone();
                async move {
                    api.request(method, &path, body)
                        .await
                        .map_err(|e| e.to_string())
                }
            })
            .await;
            Ok(match result {
                Ok(value) => json!({"owner":owner,"value":value}),
                Err(error) => json!({"owner":owner,"error":error}),
            })
        });
        Ok(true)
    }
    pub(super) fn profile_editor_complete(&mut self, action: &UiAction, value: &Value) -> bool {
        if !matches!(
            action.kind.as_str(),
            "__profile-editor-load" | "__profile-bio" | "__profile-status"
        ) {
            return false;
        }
        let loading = action.kind == "__profile-editor-load";
        let bio = action.kind == "__profile-bio";
        if !loading {
            self.request_versions.remove(if bio {
                "profile-bio-pending"
            } else {
                "profile-status-pending"
            });
        }
        if s(value, "owner") != self.me() {
            return true;
        }
        if value.get("error").is_none() && !loading {
            let me = self.me();
            // A profile GET begun before this save must not restore old fields.
            self.next_request("profile");
            if bio {
                if let Some(profile) = self
                    .state
                    .profile_details
                    .as_mut()
                    .filter(|p| p.user.id == me)
                {
                    profile.bio = s(&value["value"], "bio");
                }
            } else {
                let status = &value["value"]["custom_status"];
                let mut presence = self
                    .state
                    .user_presences
                    .get(&me)
                    .cloned()
                    .unwrap_or_default();
                if presence.status.is_empty() {
                    presence.status = self.state.self_presence.clone();
                }
                presence.custom_status = s(status, "text");
                let emoji = StatusEmoji {
                    id: s(status, "emoji_id"),
                    name: s(status, "emoji_name"),
                    ..Default::default()
                };
                presence.custom_status_emoji =
                    (!emoji.id.is_empty() || !emoji.name.is_empty()).then_some(emoji.clone());
                presence.activities.retain(|a| a.kind != 4);
                if !presence.custom_status.is_empty() || presence.custom_status_emoji.is_some() {
                    presence.activities.push(PresenceActivity {
                        kind: 4,
                        name: "Custom Status".into(),
                        state: presence.custom_status.clone(),
                        emoji: presence.custom_status_emoji.clone(),
                        ..Default::default()
                    });
                }
                self.state
                    .user_presences
                    .insert(me.clone(), presence.clone());
                self.state.presence_version = self.state.presence_version.wrapping_add(1);
                for member in self
                    .state
                    .group_members
                    .iter_mut()
                    .chain(&mut self.state.guild_members)
                {
                    if member.user.id == me {
                        member.custom_status = presence.custom_status.clone();
                        member.custom_status_emoji = presence.custom_status_emoji.clone();
                        member.activities = presence.activities.clone();
                    }
                }
                if let Some(profile) = self
                    .state
                    .profile_details
                    .as_mut()
                    .filter(|p| p.user.id == me)
                {
                    profile.custom_status = presence.custom_status;
                    profile.custom_status_emoji = presence.custom_status_emoji;
                    profile.activities = presence.activities;
                }
                self.decorate_dms();
            }
            self.publish();
        }
        self.data(if loading {"profile-editor-loaded"} else {"profile-edit-result"},json!({"owner":self.me(),"token":action.id,"kind":if loading {"load"} else if bio {"bio"} else {"status"},"value":value["value"],"error":value["error"]}));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn partial_bio_write_reads_canonical_value_instead_of_echoing_request() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = calls.clone();
        let result = execute_edit(
            "10",
            true,
            "Requested".into(),
            String::new(),
            move |method, path, body| {
                seen.lock().unwrap().push((method.clone(), path, body));
                async move {
                    Ok(if method == Method::PATCH {
                        json!({})
                    } else {
                        json!({"user_profile":{"bio":"Canonical"}})
                    })
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(result, json!({"bio":"Canonical"}));
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, "users/@me/profile");
        assert_eq!(calls[0].2, Some(json!({"bio":"Requested"})));
        assert_eq!(calls[1].0, Method::GET);
        assert_eq!(calls[1].1, "users/10/profile");
    }
    #[tokio::test]
    async fn clear_status_is_confirmed_and_failed_reload_never_echoes_requested_text() {
        let result = execute_edit(
            "10",
            false,
            String::new(),
            String::new(),
            |_, _, body| async move {
                assert_eq!(body, Some(json!({"custom_status":null})));
                Ok(json!({"custom_status":null}))
            },
        )
        .await
        .unwrap();
        assert_eq!(result, json!({"custom_status":null}));
        assert!(
            execute_edit(
                "10",
                true,
                "Pending".into(),
                String::new(),
                |method, _, _| async move {
                    if method == Method::PATCH {
                        Ok(json!({}))
                    } else {
                        Err("offline".into())
                    }
                }
            )
            .await
            .unwrap_err()
            .contains("no se pudo confirmar")
        );
    }
    #[tokio::test]
    async fn confirmed_status_updates_self_profile_only_and_preserves_other_activities() {
        let (mut c, _) = Controller::test_fixture();
        c.state.current_user = Some(Person {
            id: "10".into(),
            ..Default::default()
        });
        c.state.self_presence = "dnd".into();
        c.state.profile_details = Some(ProfileDetails {
            user: Person {
                id: "10".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        c.state.user_presences.insert(
            "10".into(),
            UserPresence {
                activities: vec![PresenceActivity {
                    kind: 0,
                    name: "Game".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        assert!(c.profile_editor_complete(
            &UiAction::id("__profile-status", "fixture"),
            &json!({"owner":"10","value":{"custom_status":{"text":"Confirmed","emoji_name":"🙂"}}})
        ));
        assert_eq!(
            c.state.profile_details.as_ref().unwrap().custom_status,
            "Confirmed"
        );
        assert_eq!(c.state.user_presences["10"].activities.len(), 2);
        assert_eq!(c.state.self_presence, "dnd");
    }
    #[tokio::test]
    async fn validation_rejects_overlong_bio_before_any_request() {
        assert!(
            execute_edit(
                "10",
                true,
                "x".repeat(191),
                String::new(),
                |_, _, _| async {
                    panic!("must not request");
                    #[allow(unreachable_code)]
                    Ok(Value::Null)
                }
            )
            .await
            .is_err()
        );
        assert!(status_body(&"x".repeat(129), "").is_err());
    }
    #[tokio::test]
    async fn offline_action_rejection_releases_matching_editor_request_without_api() {
        let (mut controller, updates) = Controller::test_fixture();
        controller.state.current_user = Some(Person {
            id: "10".into(),
            ..Default::default()
        });
        let action = UiAction {
            id: "pending-token".into(),
            kind: "save-profile-bio".into(),
            text: "Draft".into(),
            ..Default::default()
        };
        let error = controller.social_action(action.clone()).await.unwrap_err();
        controller.action_error(&action, error);
        assert!(controller.api.is_none());
        let result = updates
            .try_iter()
            .find_map(|update| {
                if let UiUpdate::Data(kind, value) = update {
                    if kind == "profile-edit-result" {
                        Some(value)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(result["token"], "pending-token");
        assert_eq!(result["owner"], "10");
        assert_eq!(result["kind"], "bio");
        assert!(result["error"].is_string());
        assert!(
            !controller
                .request_versions
                .contains_key("profile-bio-pending")
        );
    }
}
