use super::*;
use notdiscord_core::guild_settings::{self, GuildSettings, GuildVoiceMember};

/// Kept generic so offline tests can assert that revoked permissions never send a write.
async fn editor_request<F, R>(
    guild: &str,
    me: &str,
    edit: Option<(&str, &str, &Value)>,
    mut request: F,
) -> Value
where
    F: FnMut(Method, String, Option<Value>) -> R,
    R: std::future::Future<Output = Reply>,
{
    let mut confirmed = false;
    let result: Reply = async {
        let mut raw = request(Method::GET, format!("guilds/{guild}"), None).await?;
        if s(&raw,"id")!=guild{return Err("El servicio respondió con otro servidor. Vuelve a cargar la configuración.".into());}
        let mut member = request(Method::GET, format!("guilds/{guild}/members/{me}"), None).await?;
        let mut channels = request(Method::GET, format!("guilds/{guild}/channels"), None).await?;
        guild_settings::snapshot(&raw, &member, &channels, me)?;
        let mut updated_member = Value::Null;
        if let Some((kind, target, body)) = edit {
            let other = if kind == "nickname" && target != me {
                Some(
                    request(
                        Method::GET,
                        format!("guilds/{guild}/members/{target}"),
                        None,
                    )
                    .await?,
                )
            } else {
                None
            };
            let write = guild_settings::prepare_write(
                &raw,
                &member,
                &channels,
                me,
                kind,
                target,
                body,
                other.as_ref(),
            )?;
            let method = Method::from_bytes(write.method.as_bytes())
                .map_err(|_| "Método de servidor no válido.")?;
            request(method, write.path, write.body).await?;
            confirmed = true;
            // Confirmed values come from the service, never from the submitted form.
            raw = request(Method::GET, format!("guilds/{guild}"), None).await?;
            if s(&raw,"id")!=guild{return Err("El guardado se confirmó, pero la recarga no corresponde al servidor editado.".into());}
            member = request(Method::GET, format!("guilds/{guild}/members/{me}"), None).await?;
            channels = request(Method::GET, format!("guilds/{guild}/channels"), None).await?;
            if kind == "nickname" {
                updated_member = if target == me {
                    member.clone()
                } else {
                    request(
                        Method::GET,
                        format!("guilds/{guild}/members/{target}"),
                        None,
                    )
                    .await?
                };
            }
            guild_settings::snapshot(&raw, &member, &channels, me)?;
        }
        Ok(json!({"guild":raw,"member":member,"channels":channels,"updated_member":updated_member}))
    }
    .await;
    match result {
        Ok(value) => json!({"ok":true,"confirmed":confirmed,"value":value}),
        Err(error) => json!({"ok":false,"confirmed":confirmed,"error":error}),
    }
}

impl Controller {
    pub(super) fn guild_settings_action(&mut self, action: &UiAction) -> Result<bool, String> {
        if !matches!(
            action.kind.as_str(),
            "guild-settings"
                | "guild-settings-refresh"
                | "guild-settings-save"
                | "guild-settings-close"
        ) {
            return Ok(false);
        }
        if action.kind == "guild-settings-close" {
            if !self
                .state
                .guild_settings
                .as_ref()
                .is_some_and(|state| state.saving)
            {
                self.next_request("guild-settings");
                self.state.guild_settings = None;
            }
            self.state.view = if self.state.selected_channel_id.is_empty() {
                "friends"
            } else {
                "chat"
            }
            .into();
            self.recover_after_guild_editor()?;
            self.publish();
            return Ok(true);
        }
        let guild = Self::valid(&action.id)?.to_owned();
        let me = self.me();
        Self::valid(&me)?;
        if !self.state.guilds.iter().any(|known| known.id == guild) {
            return Err("El servidor ya no está disponible.".into());
        }
        let saving = action.kind == "guild-settings-save";
        if self
            .state
            .guild_settings
            .as_ref()
            .is_some_and(|state| state.saving)
        {
            return Err("Espera a que se confirme la modificación pendiente.".into());
        }
        let edit = if saving {
            if self.state.view != "guild-settings"
                || self
                    .state
                    .guild_settings
                    .as_ref()
                    .is_none_or(|state| state.guild_id != guild || state.loading)
            {
                return Err("Vuelve a abrir la configuración de este servidor.".into());
            }
            if action.text.len() > 32 * 1024 {
                return Err("La modificación supera el límite permitido.".into());
            }
            let body: Value =
                serde_json::from_str(&action.text).map_err(|_| "Configuración no válida.")?;
            Some((action.extra.clone(), action.channel_id.clone(), body))
        } else {
            None
        };
        let api = self.api()?;
        api.safety().check().map_err(|error| error.to_string())?;
        let version = self.next_request("guild-settings");
        if !saving
            && self
                .state
                .guild_settings
                .as_ref()
                .is_none_or(|state| state.guild_id != guild)
        {
            self.state.guild_settings = Some(GuildSettings {
                guild_id: guild.clone(),
                name: self
                    .state
                    .guilds
                    .iter()
                    .find(|v| v.id == guild)
                    .map(|v| v.name.clone())
                    .unwrap_or_default(),
                ..Default::default()
            });
        }
        if let Some(state) = &mut self.state.guild_settings {
            state.loading = !saving;
            state.saving = saving;
            state.error.clear();
            state.note.clear();
        }
        self.state.view = "guild-settings".into();
        self.publish();
        let mut completed = UiAction::id("__guild-settings", &guild);
        completed.extra = version.to_string();
        if let Some((kind, target, body)) = &edit {
            completed.text = json!({"kind":kind,"target":target,"body":body}).to_string();
        }
        self.launch(completed, None, async move {
            let edit = edit
                .as_ref()
                .map(|(kind, target, body)| (kind.as_str(), target.as_str(), body));
            Ok(editor_request(&guild, &me, edit, |method, path, body| {
                let api = api.clone();
                async move {
                    api.request(method, &path, body)
                        .await
                        .map_err(|error| error.to_string())
                }
            })
            .await)
        });
        Ok(true)
    }
    pub(super) fn complete_guild_settings(
        &mut self,
        action: UiAction,
        value: Value,
    ) -> Result<(), String> {
        let version = action.extra.parse::<u64>().unwrap_or(0);
        if !self.request_current("guild-settings", version)
            || self
                .state
                .guild_settings
                .as_ref()
                .is_none_or(|state| state.guild_id != action.id)
        {
            return Ok(());
        }
        if value["ok"] != true {
            if let Some(state) = &mut self.state.guild_settings {
                state.loading = false;
                state.saving = false;
                state.error = s(&value, "error");
                if value["confirmed"] == true {
                    state.note="El servidor confirmó el guardado, pero no se pudo recargar la configuración. Actualiza antes de modificarla de nuevo.".into();
                }
            }
            self.publish();
            return Ok(());
        }
        let payload = &value["value"];
        let me = self.me();
        let mut settings = guild_settings::snapshot(
            &payload["guild"],
            &payload["member"],
            &payload["channels"],
            &me,
        )?;
        settings.revision = version;
        if value["confirmed"] == true {
            settings.note = "Cambios confirmados por el servidor.".into();
        }
        if let Some(nav) = self
            .state
            .guilds
            .iter_mut()
            .find(|item| item.id == action.id)
        {
            nav.name = settings.name.clone();
            nav.can_manage = settings.can_manage_guild
                || settings.can_manage_channels
                || settings.can_manage_roles;
        }
        self.remember_guild_nickname(&action.id, &me, &payload["member"]);
        if self.state.selected_guild_id == action.id {
            self.guild_raw = payload["guild"].clone();
            self.self_member = payload["member"].clone();
            self.guild_acl = Some(GuildAccess::from_values(
                &self.guild_raw,
                &self.self_member,
                &me,
            ));
            let fresh = items(&payload["channels"]);
            self.channels.retain(|id, raw| {
                matches!(i(raw, "type"), 10 | 11 | 12)
                    || fresh.iter().any(|raw| s(raw, "id") == *id)
            });
            for channel in fresh {
                self.channels.insert(s(channel, "id"), channel.clone());
            }
            self.rebuild_channels();
            self.revoke_access();
            if let Some(channel) = self
                .state
                .channels
                .iter()
                .find(|channel| channel.id == self.state.selected_channel_id)
            {
                self.state.channel_name = channel.name.clone();
            }
        }
        if value["confirmed"] == true && payload["updated_member"].is_object() {
            let target = s(&payload["updated_member"]["user"], "id");
            self.remember_guild_nickname(&action.id, &target, &payload["updated_member"]);
        }
        self.state.guild_settings = Some(settings);
        self.recover_after_guild_editor()?;
        self.publish();
        Ok(())
    }
    fn recover_after_guild_editor(&mut self)->Result<(),String>{
        if self.state.view!="guild-settings" && !self.state.selected_guild_id.is_empty()
            && !self.state.selected_channel_id.is_empty() && !self.state.channels.iter().any(|channel|channel.id==self.state.selected_channel_id && channel.access.as_ref().is_some_and(|access|access.can_view)) {
            self.recover_guild_navigation()?;
        }
        Ok(())
    }

    pub(super) fn remember_guild_nickname(&mut self, guild: &str, user: &str, member: &Value) {
        if !parsing::valid_id(guild) || !parsing::valid_id(user) {
            return;
        }
        if let Some(value) = member.get("nick") {
            let nickname = value
                .as_str()
                .map(|v| parsing::limited(v, 32))
                .unwrap_or_default();
            if guild == self.state.selected_guild_id {
                if self.state.guild_nicknames.len() < 3000
                    || self.state.guild_nicknames.contains_key(user)
                {
                    self.state
                        .guild_nicknames
                        .insert(user.into(), nickname.clone());
                }
                for person in &mut self.state.guild_members {
                    if person.user.id == user {
                        person.nickname = nickname.clone();
                    }
                }
                for person in self.guild_member_slots.iter_mut().flatten() {
                    if person.user.id == user {
                        person.nickname = nickname.clone();
                    }
                }
                if let Some(profile) = &mut self.state.profile_details
                    && profile.guild_id == guild
                    && profile.user.id == user
                {
                    profile.nickname = nickname.clone();
                }
            }
            for person in &mut self.state.guild_voice_members {
                if person.guild_id == guild && person.user.id == user {
                    person.nickname = nickname.clone();
                }
            }
        }
        if member["user"].is_object() && s(&member["user"], "id") == user {
            let raw = &member["user"];
            let parsed = parsing::person(raw);
            let mut person = self
                .people
                .get(user)
                .cloned()
                .unwrap_or_else(|| parsed.clone());
            if raw.get("username").is_some() {
                person.username = parsed.username;
            }
            if raw.get("global_name").is_some() {
                person.display_name = parsed.display_name;
            }
            if raw.get("avatar").is_some() {
                person.avatar_url = parsed.avatar_url;
            }
            if raw.get("bot").is_some() {
                person.is_bot = parsed.is_bot;
            }
            person.name = guild_settings::visible_name(&person, "").into();
            self.remember(person.clone());
            for member in &mut self.state.guild_members{if member.user.id==user{member.user=person.clone();}}
            for member in self.guild_member_slots.iter_mut().flatten(){if member.user.id==user{member.user=person.clone();}}
            for member in &mut self.state.guild_voice_members {
                if member.user.id == user {
                    member.user = person.clone();
                }
            }
        }
    }
    pub(super) fn remember_guild_voice_state(&mut self, value: &Value) {
        let guild = s(value, "guild_id");
        let id = s(value, "user_id");
        let channel = s(value, "channel_id");
        if !parsing::valid_id(&guild) || !parsing::valid_id(&id) {
            return;
        }
        self.remember_guild_nickname(&guild, &id, &value["member"]);
        let previous = self
            .state
            .guild_voice_members
            .iter()
            .find(|person| person.guild_id == guild && person.user.id == id)
            .cloned();
        if value.get("channel_id").is_some_and(Value::is_null) {
            self.state
                .guild_voice_members
                .retain(|person| person.guild_id != guild || person.user.id != id);
            self.publish();
            return;
        }
        let channel=if value.get("channel_id").is_none(){previous.as_ref().map(|member|member.channel_id.clone()).unwrap_or_default()}else{channel};
        if !parsing::valid_id(&channel) {
            return;
        }
        let raw_user = if value["member"]["user"].is_object() {
            &value["member"]["user"]
        } else {
            &value["user"]
        };
        let mut person = self.people
                .get(&id)
                .cloned()
                .or_else(|| previous.as_ref().map(|person| person.user.clone()))
                .unwrap_or_else(|| Person {
                    id: id.clone(),
                    name: "Usuario".into(),
                    ..Default::default()
                });
        if raw_user.is_object()&&s(raw_user,"id")==id {
            let parsed=parsing::person(raw_user);
            if raw_user.get("username").is_some(){person.username=parsed.username;}
            if raw_user.get("global_name").is_some(){person.display_name=parsed.display_name;}
            if raw_user.get("avatar").is_some(){person.avatar_url=parsed.avatar_url;}
            if raw_user.get("bot").is_some(){person.is_bot=parsed.is_bot;}
            person.name=guild_settings::visible_name(&person,"").into();
        }
        let nickname = if value["member"].get("nick").is_some() {
            s(&value["member"], "nick")
        } else {
            previous
                .as_ref()
                .map(|person| person.nickname.clone())
                .unwrap_or_default()
        };
        let self_mute=value["self_mute"].as_bool().unwrap_or_else(||previous.as_ref().is_some_and(|member|member.self_mute));
        let server_mute=value["mute"].as_bool().unwrap_or_else(||previous.as_ref().is_some_and(|member|member.server_mute));
        let self_deaf=value["self_deaf"].as_bool().unwrap_or_else(||previous.as_ref().is_some_and(|member|member.self_deaf));
        let server_deaf=value["deaf"].as_bool().unwrap_or_else(||previous.as_ref().is_some_and(|member|member.server_deaf));
        let member = GuildVoiceMember {
            guild_id: guild.clone(),
            channel_id: channel,
            user: person,
            nickname,
            muted: self_mute||server_mute,
            deafened: self_deaf||server_deaf,
            self_mute,server_mute,self_deaf,server_deaf,
            self_video:value["self_video"].as_bool().unwrap_or_else(||previous.as_ref().is_some_and(|member|member.self_video)),
            screen_sharing:value["self_stream"].as_bool().unwrap_or_else(||previous.as_ref().is_some_and(|member|member.screen_sharing)),
        };
        self.state
            .guild_voice_members
            .retain(|person| person.guild_id != guild || person.user.id != id);
        if self.state.guild_voice_members.len() < 1024 {
            self.state.guild_voice_members.push(member);
        }
        self.publish();
    }
    pub(super) fn reset_guild_voice_snapshot(&mut self, guild: &str) {
        self.state
            .guild_voice_members
            .retain(|person| person.guild_id != guild);
    }
    pub(super) fn seed_guild_nicknames(&mut self) {
        self.state.guild_nicknames.clear();
        let guild = self.state.selected_guild_id.clone();
        let me = self.me();
        let member = self.self_member.clone();
        self.remember_guild_nickname(&guild, &me, &member);
        for member in items(&self.guild_raw["members"])
            .iter()
            .take(3000)
            .cloned()
            .collect::<Vec<_>>()
        {
            let id = s(&member["user"], "id");
            self.remember_guild_nickname(&guild, &id, &member);
        }
        for member in self
            .state
            .guild_voice_members
            .clone()
            .iter()
            .filter(|member| member.guild_id == guild)
        {
            if !member.nickname.is_empty() {
                self.state
                    .guild_nicknames
                    .insert(member.user.id.clone(), member.nickname.clone());
            }
        }
    }
    pub(super) fn observe_message_nickname(&mut self, message: &Value) {
        let guild = if parsing::valid_id(&s(message, "guild_id")) {
            s(message, "guild_id")
        } else if self.state.selected_guild_id.is_empty() {
            return;
        } else if self.channels.contains_key(&s(message, "channel_id")) {
            self.state.selected_guild_id.clone()
        } else {
            return;
        };
        let user = s(&message["author"], "id");
        // A historical message cannot overwrite a later live nickname update,
        // including an explicit removal (the map retains an empty nickname).
        if guild != self.state.selected_guild_id || !self.state.guild_nicknames.contains_key(&user)
        {
            self.remember_guild_nickname(&guild, &user, &message["member"]);
        }
        if message["referenced_message"].is_object() {
            self.observe_message_nickname(&message["referenced_message"]);
        }
    }
    pub(super) fn invalidate_guild_editor(&mut self, guild: &str) {
        if let Some(settings) = &mut self.state.guild_settings
            && settings.guild_id == guild
            && !settings.saving
        {
            settings.can_manage_guild = false;
            settings.can_manage_roles = false;
            settings.can_manage_channels = false;
            settings.can_change_nickname = false;
            settings.can_manage_nicknames = false;
            for role in &mut settings.roles {
                role.editable = false;
            }
            for channel in &mut settings.channels {
                channel.editable = false;
                channel.permissions_editable = false;
                channel.can_invite = false;
            }
            settings.note =
                "Los permisos o la configuración cambiaron. Actualiza los valores antes de editar."
                    .into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn server_editor_uses_fresh_acl_before_any_write_and_only_returns_confirmed_data() {
        let guild = json!({"id":"10","owner_id":"9","name":"Before","roles":[{"id":"10","permissions":"1024"}]});
        let member = json!({"user":{"id":"1"},"roles":[]});
        let channels = json!([]);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let observed = sent.clone();
        let value = editor_request(
            "10",
            "1",
            Some(("general", "", &json!({"name":"Denied"}))),
            move |method, path, _| {
                observed
                    .lock()
                    .unwrap()
                    .push((method.clone(), path.clone()));
                let value = if path.ends_with("/members/1") {
                    member.clone()
                } else if path.ends_with("/channels") {
                    channels.clone()
                } else {
                    guild.clone()
                };
                async move { Ok(value) }
            },
        )
        .await;
        assert_eq!(value["ok"], false);
        assert_eq!(value["confirmed"], false);
        assert!(
            sent.lock()
                .unwrap()
                .iter()
                .all(|(method, _)| *method == Method::GET)
        );
    }
    #[tokio::test]
    async fn confirmed_nickname_is_reloaded_from_service_instead_of_echoing_the_form() {
        let guild = json!({"id":"10","owner_id":"1","name":"Synthetic","roles":[{"id":"10","position":0,"permissions":"1024"},{"id":"20","position":3,"permissions":"0"}]});
        let own = json!({"user":{"id":"1"},"roles":["20"]});
        let steps = Arc::new(Mutex::new(Vec::new()));
        let observed = steps.clone();
        let sent = Arc::new(AtomicBool::new(false));
        let wrote = sent.clone();
        let value=editor_request("10","1",Some(("nickname","2",&json!({"nick":"Requested"}))),move|method,path,body|{
            observed.lock().unwrap().push((method.clone(),path.clone(),body));
            let value=if method==Method::PATCH{wrote.store(true,Ordering::SeqCst);json!({"nick":"Server canonical"})}
                else if path.ends_with("/members/2"){json!({"user":{"id":"2"},"roles":[],"nick":if wrote.load(Ordering::SeqCst){"Server canonical"}else{"Before"}})}
                else if path.ends_with("/members/1"){own.clone()}else if path.ends_with("/channels"){json!([])}else{guild.clone()};async move{Ok(value)}
        }).await;
        assert_eq!(value["ok"], true);
        assert_eq!(value["confirmed"], true);
        assert_eq!(value["value"]["updated_member"]["nick"], "Server canonical");
        let steps = steps.lock().unwrap();
        assert_eq!(steps.len(), 9);
        assert!(steps[..4].iter().all(|step| step.0 == Method::GET));
        assert_eq!(steps[4].0, Method::PATCH);
        assert_eq!(steps[4].1, "guilds/10/members/2");
        assert_eq!(steps[4].2, Some(json!({"nick":"Requested"})));
        assert!(steps[5..].iter().all(|step| step.0 == Method::GET));
    }
    #[tokio::test]
    async fn successful_write_with_failed_reload_is_reported_as_confirmed_without_echoing_requested_data(){
        let guild=json!({"id":"10","owner_id":"1","name":"Before","roles":[]});
        let member=json!({"user":{"id":"1"},"roles":[]});let wrote=Arc::new(AtomicBool::new(false));let sent=wrote.clone();
        let response=editor_request("10","1",Some(("general","",&json!({"name":"Requested"}))),move|method,path,_|{
            let response=if method==Method::PATCH{sent.store(true,Ordering::SeqCst);Ok(json!({"name":"Confirmed"}))}
                else if sent.load(Ordering::SeqCst){Err("Recarga sintética no disponible".into())}
                else if path.ends_with("/members/1"){Ok(member.clone())}else if path.ends_with("/channels"){Ok(json!([]))}else{Ok(guild.clone())};
            async move{response}
        }).await;
        assert_eq!(response["ok"],false);assert_eq!(response["confirmed"],true);assert!(response.get("value").is_none());
        assert_eq!(response["error"],"Recarga sintética no disponible");
    }
    #[test]
    fn voice_sidebar_moves_leaves_and_clears_nicknames_without_touching_global_identity() {
        let (mut controller, _) = Controller::test_fixture();
        controller.state.selected_guild_id = "10".into();
        let first = json!({"guild_id":"10","user_id":"2","channel_id":"30","member":{"user":{"id":"2","username":"Account","global_name":"Global"},"nick":"Server"}});
        controller.remember_guild_voice_state(&first);
        assert_eq!(controller.state.guild_voice_members.len(), 1);
        assert_eq!(
            controller
                .state
                .guild_display_name(&controller.state.guild_voice_members[0].user),
            "Server"
        );
        assert_eq!(controller.people["2"].name, "Global");
        controller
            .remember_guild_voice_state(&json!({"guild_id":"10","user_id":"2","channel_id":"31"}));
        assert_eq!(controller.state.guild_voice_members.len(), 1);
        assert_eq!(controller.state.guild_voice_members[0].channel_id, "31");
        controller.remember_guild_nickname("10", "2", &json!({"nick":null}));
        assert_eq!(
            controller
                .state
                .guild_display_name(&controller.state.guild_voice_members[0].user),
            "Global"
        );
        controller
            .remember_guild_voice_state(&json!({"guild_id":"10","user_id":"2","channel_id":null}));
        assert!(controller.state.guild_voice_members.is_empty());
    }
    #[test]
    fn partial_voice_sidebar_metadata_keeps_channel_identity_and_omitted_flags() {
        let (mut controller,_)=Controller::test_fixture();
        controller.remember_guild_voice_state(&json!({"guild_id":"10","user_id":"2","channel_id":"30","self_mute":true,"self_deaf":true,
            "member":{"user":{"id":"2","username":"Account","global_name":"Visible","avatar":"hash"},"nick":"Server"}}));
        let avatar=controller.state.guild_voice_members[0].user.avatar_url.clone();
        controller.remember_guild_voice_state(&json!({"guild_id":"10","user_id":"2","member":{"user":{"id":"2"},"nick":"Renamed"}}));
        let member=&controller.state.guild_voice_members[0];assert_eq!(member.channel_id,"30");assert_eq!(member.user.name,"Visible");assert_eq!(member.user.avatar_url,avatar);
        assert!(member.muted&&member.deafened);assert_eq!(member.nickname,"Renamed");
        controller.remember_guild_voice_state(&json!({"guild_id":"10","user_id":"2","channel_id":"31","self_mute":false,"self_deaf":false}));
        assert_eq!(controller.state.guild_voice_members.len(),1);assert_eq!(controller.state.guild_voice_members[0].channel_id,"31");assert!(!controller.state.guild_voice_members[0].muted&&!controller.state.guild_voice_members[0].deafened);
    }
}
