use super::*;
use notdiscord_core::{
    search::{self, SearchContext},
    settings_proto,
};
async fn execute_guild_notification_patch<F, R>(id: &str, patch: Value, request: F) -> Reply
where F: FnOnce(Method, String, Option<Value>) -> R, R: std::future::Future<Output=Reply> {
    let mut validation = notdiscord_core::notifications::SettingsStore::default();
    validation.begin_patch(id,patch.clone())?;
    Ok(match request(Method::PATCH,format!("users/@me/guilds/{id}/settings"),Some(patch)).await {
        Ok(_) => json!({"success":true}),
        Err(error) => json!({"success":false,"error":error}),
    })
}
impl Controller {
    fn known_profile_person(&self,id:&str)->Person {
        let mut known=Person {id:id.into(),..Default::default()};
        for person in self.state.messages.iter().rev().map(|message|&message.author)
            .chain(self.people.get(id)).chain(self.state.current_user.iter())
            .chain(self.state.guild_members.iter().map(|member|&member.user))
            .chain(self.state.relationships.iter().map(|relation|&relation.user))
            .chain(self.state.call_participants.iter().map(|person|&person.user))
            .chain(self.state.messages.iter().filter_map(|message|message.reply_preview.as_ref().and_then(|reply|reply.author.as_ref())))
            .filter(|person|person.id==id) {
            if known.name.is_empty() || known.name=="Usuario" {known.name=person.name.clone();}
            if known.username.is_empty() {known.username=person.username.clone();}
            if known.display_name.is_empty() {known.display_name=person.display_name.clone();}
            if known.avatar_url.is_none() {known.avatar_url=person.avatar_url.clone();}
            known.is_bot|=person.is_bot;
        }
        if known.name.is_empty() {known.name="Usuario".into();}
        known
    }
    fn known_webhook_author(&self,id:&str)->bool {
        self.state.messages.iter().any(|message|message.author.id==id && !message.webhook_id.is_empty())
    }
    fn dm_profile_target(&self, channel: &str, id: &str) -> bool {
        self.state.view == "chat"
            && self.state.selected_guild_id.is_empty()
            && self.state.selected_channel_id == channel
            && self.state.selected_channel_kind == "dm"
            && self
                .state
                .direct_messages
                .iter()
                .any(|item| item.id == channel && item.kind == "dm" && item.recipient_id == id)
    }
    fn dm_profile_fallback(&self, id: &str) -> ProfileDetails {
        let presence = self
            .state
            .user_presences
            .get(id)
            .cloned()
            .unwrap_or_default();
        ProfileDetails {
            user: self.known_profile_person(id),
            created_at: parsing::account_created_at(id),
            friends_since: self
                .state
                .relationships
                .iter()
                .find(|relationship| relationship.kind == 1 && relationship.user.id == id)
                .and_then(|relationship| relationship.since),
            presence: presence.status,
            custom_status: presence.custom_status,
            custom_status_emoji: presence.custom_status_emoji,
            activities: presence.activities,
            ..Default::default()
        }
    }
    fn open_profile_action(&mut self, mut action: UiAction) -> Result<bool, String> {
        let channel=self.state.selected_channel_id.clone();
        self.cancel_view_requests();
        let request_version = self.next_request("profile");
        let id = Self::valid(&action.id)?.to_owned();
        let person = self.known_profile_person(&id);
        let is_bot=person.is_bot; let is_webhook=self.known_webhook_author(&id);
        self.remember(person.clone());
        if self.state.view!="profile"{self.selected_before_profile = self.state.view.clone();}
        self.state.profile_user = Some(person.clone());
        let presence = self
            .state
            .user_presences
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let member = self.state.guild_members.iter().find(|m| m.user.id == id);
        let context = !self.state.selected_guild_id.is_empty()
            && self.state.channel_access.can_view
            && (member.is_some()
                || self
                    .state
                    .messages
                    .iter()
                    .any(|m| m.channel_id == channel && m.author.id == id)
                || id == self.me());
        let fallback = ProfileDetails {
            user: person,
            is_webhook,
            created_at: parsing::account_created_at(&id),
            friends_since: self
                .state
                .relationships
                .iter()
                .find(|r| r.kind == 1 && r.user.id == id)
                .and_then(|r| r.since),
            presence: presence.status,
            custom_status: presence.custom_status,
            custom_status_emoji: presence.custom_status_emoji,
            activities: presence.activities,
            guild_id: if context {
                self.state.selected_guild_id.clone()
            } else {
                String::new()
            },
            guild_name: if context {
                self.state
                    .guilds
                    .iter()
                    .find(|g| g.id == self.state.selected_guild_id)
                    .map(|g| g.name.clone())
                    .unwrap_or_default()
            } else {
                String::new()
            },
            guild_roles: if context {
                member.map(|m| m.roles.clone()).unwrap_or_default()
            } else {
                Vec::new()
            },
            nickname: if context {
                member.map(|m| m.nickname.clone()).unwrap_or_default()
            } else {
                String::new()
            },
            mutual_friends_available: false,
            mutual_guilds_available: false,
            ..Default::default()
        };
        self.state.profile_subtitle=if is_webhook {"Autor de webhook · identidad observada en este chat; no es un perfil de usuario.".into()} else {String::new()};
        self.state.profile_details = Some(fallback);
        self.state.view = "profile".into();
        self.publish();
        let guild = if context {
            self.state.selected_guild_id.clone()
        } else {
            String::new()
        };
        if is_webhook {return Ok(true);}
        let Some(api)=self.api.clone() else {
            self.state.profile_subtitle="Perfil local · no se consultó el servicio.".into();self.publish();return Ok(true);
        };
        action.kind = "__profile".into();
        action.extra = request_version.to_string();
        self.launch(action,Some(self.nav),async move {
            let suffix=if guild.is_empty(){String::new()}else{format!("&guild_id={guild}")};
            let profile=match api.get(&format!("users/{id}/profile?with_mutual_guilds=true&with_mutual_friends=true{suffix}")).await {
                Ok(profile)=>profile,
                Err(error) if matches!(error.status,Some(403|404)) && matches!(error.kind,notdiscord_core::api::ErrorKind::Http|notdiscord_core::api::ErrorKind::PermissionDenied) =>return Ok(json!({"profile":null,"unavailable":true})),
                Err(error)=>return Err(error.to_string()),
            };
            let friends=if is_bot || profile.get("mutual_friends").is_some() {None}
                else {api.get(&format!("users/{id}/relationships")).await.ok()};
            Ok(json!({"profile":profile,"friends":friends}))
        });
        Ok(true)
    }
    pub(super) async fn social_action(&mut self, mut action: UiAction) -> Result<bool, String> {
        if self.profile_editor_action(action.clone())? {return Ok(true);}
        // Reviewing/joining an invite is delegated to Discord's website. These
        // local actions must also work while authenticated network work is paused.
        if matches!(action.kind.as_str(), "invite-preview" | "invite-accept") {
            let value = if action.id.is_empty() { &action.text } else { &action.id };
            let url = crate::platform::official_invite_url(value)?;
            if !action.id.is_empty() && !action.text.is_empty()
                && crate::platform::official_invite_url(&action.text)? != url
            {
                return Err("La invitación cambió. Vuelve a abrirla.".into());
            }
            if action.kind == "invite-preview" {
                self.data("invite-preview", json!({"code":invite_code(&url)?,"delegated":true}));
            } else {
                self.data("open-official-invite", json!({"url":url}));
            }
            return Ok(true);
        }
        if action.kind=="profile" {return self.open_profile_action(action);}
        if action.kind == "guild-hide-muted" {
            let id = Self::valid(&action.id)?.to_owned();
            if !self.state.guilds.iter().any(|guild|guild.id==id) { return Err("Servidor no disponible.".into()); }
            let hidden = action.text.parse::<bool>().map_err(|_|"Preferencia local no válida.")?;
            let account = self.me(); Self::valid(&account)?;
            self.load_local_notification_views();
            let mut ids = self.notification_settings.hidden_ids();
            ids.retain(|guild|guild!=&id); if hidden { ids.push(id.clone()); }
            ids.sort(); ids.dedup(); if ids.len()>500 { return Err("Demasiados ajustes locales.".into()); }
            storage::save_json(&self.data.join("notification-views").join(format!("{account}.json")), &ids)?;
            self.notification_settings.set_hidden(&id, hidden)?;
            self.refresh_notification_nav(); self.publish();
            return Ok(true);
        }
        if matches!(action.kind.as_str(),"open-dm"|"add-friend") && !action.id.is_empty() && self.known_webhook_author(&action.id) {
            return Err("Los autores de webhook no admiten mensajes directos ni solicitudes de amistad.".into());
        }
        let api = self.api()?;
        let channel = self.state.selected_channel_id.clone();
        let nav = Some(self.nav);
        match action.kind.as_str() {
            "ui-settings" => {}
            "local-error" => return Err(action.text),
            "typing" => {
                if self.send_typing && self.state.channel_access.can_send
                    && !channel.is_empty()
                    && self
                        .last_typing_sent
                        .is_none_or(|at| at.elapsed() >= Duration::from_secs(8))
                {
                    self.last_typing_sent = Some(Instant::now());
                    action.kind = "__typing".into();
                    self.launch(action, Some(self.nav), async move {
                        api.request(Method::POST, &format!("channels/{channel}/typing"), None)
                            .await
                            .map_err(|e| e.to_string())
                    });
                }
            }
            "custom-activity" => {
                let activity = action.text.trim().to_owned();
                if activity.chars().count() > 128 || activity.chars().any(char::is_control) {
                    return Err("La actividad admite 128 caracteres sin controles.".into());
                }
                self.set_manual_game_activity(activity).await;
            }
            "dm-sidebar-profile" => {
                let id=Self::valid(&action.id)?.to_owned();
                if !self.dm_profile_target(&action.channel_id,&id) {
                    return Err("El perfil no corresponde al mensaje directo abierto.".into());
                }
                action.kind="__dm_sidebar_profile".into();
                self.launch(action,Some(self.nav),async move {
                    api.get(&format!("users/{id}/profile?with_mutual_guilds=true&with_mutual_friends=true"))
                        .await.map_err(|error|error.to_string())
                });
            }
            "add-friend" => {
                let id = action.id.clone();
                if !id.is_empty() {
                    if id == self.me() || self.known_profile_person(&id).is_bot {
                        return Err("No puedes enviar esa solicitud.".into());
                    }
                    if let Some(r) = self.state.relationships.iter().find(|r| r.user.id == id) {
                        if r.kind == 3 {
                            return self.social_action_friend_accept(id).await.map(|_| true);
                        }
                        if matches!(r.kind, 1 | 2 | 4) {
                            return Err(
                                "La relación actual no permite enviar otra solicitud.".into()
                            );
                        }
                    }
                }
                let username = if action.text.is_empty() {
                    self.people
                        .get(&id)
                        .map(|p| p.username.clone())
                        .unwrap_or_default()
                } else {
                    action.text.trim().trim_start_matches('@').to_owned()
                };
                if username.is_empty() || username.encode_utf16().count() > 128 {
                    return Err("Espera a que cargue el nombre de usuario.".into());
                }
                action.kind = "__relations".into();
                let request_key = if id.is_empty() {
                    username.to_lowercase()
                } else {
                    id
                };
                if self.friend_pending.len() >= 16
                    || !self.friend_pending.insert(request_key.clone())
                {
                    return Err("Espera a que termine la solicitud de amistad en curso.".into());
                }
                action.extra = request_key;
                self.launch(action, None, async move {
                    api.request(
                        Method::POST,
                        "users/@me/relationships",
                        Some(json!({"username":username,"discriminator":null})),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    api.get("users/@me/relationships")
                        .await
                        .map_err(|e| e.to_string())
                });
            }
            "accept-friend" => {
                self.social_action_friend_accept(action.id).await?;
            }
            "remove-friend" => {
                let id = Self::valid(&action.id)?.to_owned();
                action.kind = "__relations".into();
                self.launch(action, None, async move {
                    api.request(
                        Method::DELETE,
                        &format!("users/@me/relationships/{id}"),
                        None,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    api.get("users/@me/relationships")
                        .await
                        .map_err(|e| e.to_string())
                });
            }
            "open-dm" => {
                let id = Self::valid(&action.id)?.to_owned();
                if let Some(channel)=self.state.direct_messages.iter().find(|channel|channel.kind=="dm"&&channel.recipient_id==id).map(|channel|channel.id.clone()){
                    self.select_channel(&channel)?;return Ok(true);
                }
                action.kind = "__open-dm".into();
                self.launch(action, None, async move {
                    api.request(
                        Method::POST,
                        "users/@me/channels",
                        Some(json!({"recipients":[id]})),
                    )
                    .await
                    .map_err(|e| e.to_string())
                });
            }
            "add-group" | "group-add" => {
                if self.state.selected_channel_kind != "group" {
                    return Err("No es un grupo.".into());
                }
                let id = Self::valid(&action.id)?.to_owned();
                if !self
                    .state
                    .group_recipient_candidates
                    .iter()
                    .any(|p| p.id == id)
                    || self.state.group_members.len() >= 10
                {
                    return Err("No se puede añadir a esa persona al grupo.".into());
                }
                action = self.group_refresh_action(&channel);
                self.launch(action, nav, async move {
                    api.request(
                        Method::PUT,
                        &format!("channels/{channel}/recipients/{id}"),
                        Some(json!({})),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    api.get(&format!("channels/{channel}"))
                        .await
                        .map_err(|e| e.to_string())
                });
            }
            "pin-message" | "unpin-message" => {
                if !self.state.can_pin_messages {
                    return Err("No tienes permiso para fijar mensajes.".into());
                }
                let id = Self::valid(&action.id)?.to_owned();
                if !self.state.messages.iter().chain(&self.state.pinned_messages).any(|message|message.id==id&&message.channel_id==channel&&!message.deleted) {
                    return Err("El mensaje ya no está disponible en esta conversación.".into());
                }
                let pinned=action.kind=="pin-message";
                let method = if action.kind == "pin-message" {
                    Method::PUT
                } else {
                    Method::DELETE
                };
                action.kind = "__message-pin".into();
                action.channel_id=channel.clone();
                action.text=if pinned {"true"}else{"false"}.into();
                self.launch(action, None, async move {
                    api.request(
                        method,
                        &format!("channels/{channel}/messages/pins/{id}"),
                        Some(json!({})),
                    )
                    .await
                    .map_err(|e| e.to_string())
                });
            }
            "react" | "react-toggle" => {
                self.require("history")?;
                let id = Self::valid(&action.id)?.to_owned();
                let message = self
                    .state
                    .messages
                    .iter()
                    .find(|m| m.id == id)
                    .ok_or("Mensaje no disponible.")?;
                let emoji = reaction_key(&action.text)?;
                let existing = message
                    .reactions
                    .iter()
                    .find(|r| r.emoji.reaction_key() == emoji);
                if existing.is_none() && !self.state.channel_access.can_react {
                    return Err("No puedes crear una reacción nueva aquí.".into());
                }
                let remove = action.kind == "react-toggle" && existing.is_some_and(|r| r.me);
                if !remove && self.guild_acl.as_ref().is_some_and(|a| a.timed_out) {
                    return Err(
                        "No puedes reaccionar mientras se aplica la restricción temporal.".into(),
                    );
                }
                let burst = existing.is_some_and(|r| r.burst_me && !r.normal_me.unwrap_or(r.me));
                let route = format!(
                    "channels/{channel}/messages/{id}/reactions/{}/{}",
                    url::form_urlencoded::byte_serialize(emoji.as_bytes()).collect::<String>(),
                    if remove && burst {
                        "1/@me?burst=true"
                    } else {
                        "@me"
                    }
                );
                action.kind = "__reaction".into();
                self.launch(action, nav, async move {
                    api.request(
                        if remove { Method::DELETE } else { Method::PUT },
                        &route,
                        None,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    api.get(&format!("channels/{channel}/messages/{id}"))
                        .await
                        .map_err(|e| e.to_string())
                });
            }
            "search" => {
                self.require("history")?;
                self.cancel_view_requests();
                let request_version = self.next_request("search");
                let parsed = search::parse(
                    &action.text,
                    &SearchContext {
                        current_user_id: self.me(),
                        people: self.state.search_people.clone(),
                        channels: self.state.channels.clone(),
                    },
                )
                .map_err(|e| e.to_string())?;
                let path = if self.state.selected_guild_id.is_empty() {
                    format!(
                        "channels/{channel}/messages/search?{}",
                        parsed.query_string()
                    )
                } else {
                    format!(
                        "guilds/{}/messages/search?{}",
                        self.state.selected_guild_id,
                        parsed.query_string()
                    )
                };
                action.kind = "__search".into();
                action.extra =
                    json!({"authorType":parsed.author_type,"requestVersion":request_version})
                        .to_string();
                self.state.view = "search".into();
                if parsed.filters.is_empty() && parsed.content.is_empty() {
                    self.state.messages = self
                        .cache
                        .get(&channel)
                        .into_iter()
                        .filter(|m| parsed.matches(m))
                        .take(100)
                        .collect();
                    self.publish();
                    return Ok(true);
                }
                self.state.messages.clear();
                self.publish();
                self.launch(action, Some(self.nav), async move {
                    api.get(&path).await.map_err(|e| e.to_string())
                });
            }
            "forward-channels" => {
                let guild = action.id.clone();
                if guild.is_empty() {
                    self.data(
                        "forward-channels",
                        json!({"guild_id":"","channels":self.state.direct_messages}),
                    );
                } else {
                    Self::valid(&guild)?;
                    if !self.state.guilds.iter().any(|g| g.id == guild) {
                        return Err("Servidor no disponible.".into());
                    }
                    let me = self.me();
                    action.kind = "__forward-channels".into();
                    self.launch(action, None, async move {
                        let g = api
                            .get(&format!("guilds/{guild}"))
                            .await
                            .map_err(|e| e.to_string())?;
                        let member = api
                            .get(&format!("guilds/{guild}/members/{me}"))
                            .await
                            .map_err(|e| e.to_string())?;
                        let channels = api
                            .get(&format!("guilds/{guild}/channels"))
                            .await
                            .map_err(|e| e.to_string())?;
                        let acl = GuildAccess::from_values(&g, &member, &me);
                        let catalog = items(&channels)
                            .iter()
                            .map(|v| (s(v, "id"), v.clone()))
                            .collect::<HashMap<_, _>>();
                        let result = catalog
                            .values()
                            .map(|v| acl.decorate(v, catalog.get(&s(v, "parent_id"))))
                            .filter(|n| {
                                matches!(n.kind.as_str(), "text" | "thread")
                                    && n.access.as_ref().is_some_and(|a| a.can_view)
                            })
                            .take(2000)
                            .collect::<Vec<_>>();
                        Ok(json!({"guild_id":guild,"channels":result}))
                    });
                }
            }
            "forward" | "forward-message" | "forward-many" => {
                self.require("history")?;
                if action.channel_id != channel || !matches!(self.state.view.as_str(),"chat"|"pins") {
                    return Err("La conversación cambió. Abre el mensaje y vuelve a elegir los destinos.".into());
                }
                let id = Self::valid(&action.id)?.to_owned();
                let source=self.cache.get(&channel).into_iter()
                    .chain(self.state.messages.iter().cloned()).chain(self.state.pinned_messages.iter().cloned())
                    .find(|message|message.id==id&&message.channel_id==channel)
                    .ok_or("El mensaje ya no está en la vista. No se ha reenviado nada.")?;
                if source.deleted||source.forwarded||!matches!(source.message_type,0|19){return Err("Sólo se pueden reenviar mensajes originales disponibles.".into());}
                let targets=forward_targets(&action)?;
                let me = self.me();
                let dm_ids = self
                    .state
                    .direct_messages
                    .iter()
                    .filter(|item|matches!(item.kind.as_str(),"dm"|"group"))
                    .map(|n| n.id.clone())
                    .collect::<std::collections::HashSet<_>>();
                let guild_ids=self.state.guilds.iter().map(|guild|guild.id.clone()).collect::<HashSet<_>>();
                let origin_guild=self.state.selected_guild_id.clone();
                action.kind = "__forwarded".into();
                action.channel_id=channel;
                self.launch(action,None,async move{
                    execute_forward(&source,&origin_guild,&targets,&dm_ids,&guild_ids,&me,move |method,path,body|{
                        let api=api.clone();async move{api.request(method,&path,body).await.map_err(|error|error.to_string())}
                    }).await
                });
            }
            "mark-read" => {
                let id = if action.id.is_empty() {
                    channel
                } else {
                    action.id.clone()
                };
                Self::valid(&id)?;
                let last = self
                    .latest
                    .get(&id)
                    .cloned()
                    .or_else(|| {
                        self.state
                            .direct_messages
                            .iter()
                            .find(|n| n.id == id)
                            .map(|n| n.last_message_id.clone())
                    })
                    .unwrap_or_default();
                action.kind="__marked-read".into();action.id=id.clone();
                self.launch(action,None,async move{let last=if parsing::valid_id(&last){last}else{let page=api.get(&format!("channels/{id}/messages?limit=1")).await.map_err(|e|e.to_string())?;items(&page).first().map(|m|s(m,"id")).unwrap_or_default()};if parsing::valid_id(&last){api.request(Method::POST,&format!("channels/{id}/messages/{last}/ack"),Some(json!({"token":null,"manual":true}))).await.map_err(|e|e.to_string())?;}Ok(json!({"last":last}))});
            }
            "close-dm" => {
                let id = Self::valid(&action.id)?.to_owned();
                if !self.state.direct_messages.iter().any(|n|n.id==id&&n.kind=="dm"){return Err("Esta acción sólo cierra conversaciones individuales. No abandona grupos.".into())}
                action.kind = "__closed-dm".into();
                self.launch(action, None, async move {
                    api.request(Method::DELETE, &format!("channels/{id}"), None)
                        .await
                        .map_err(|e| e.to_string())
                });
            }
            "mute-channel" | "mute-dm" => {
                let id = Self::valid(&action.id)?.to_owned();
                let muted = action
                    .text
                    .parse::<bool>()
                    .unwrap_or(!self.muted.get(&id).copied().unwrap_or(false));
                let guild = self
                    .state
                    .channels
                    .iter()
                    .any(|n| n.id == id)
                    .then(|| self.state.selected_guild_id.clone())
                    .filter(|g| !g.is_empty())
                    .unwrap_or_else(|| "@me".into());
                action.kind = "__muted".into();
                action.text = muted.to_string();
                self.launch(action, None, async move {
                    api.request(
                        Method::PATCH,
                        &format!("users/@me/guilds/{guild}/settings"),
                        Some(json!({"channel_overrides":{id:{"muted":muted}}})),
                    )
                    .await
                    .map_err(|e| e.to_string())
                });
            }
            "block-user"=>{let id=Self::valid(&action.id)?.to_owned();if id==self.me(){return Err("No puedes bloquear tu propia cuenta.".into())}action.kind="__relations".into();self.launch(action,None,async move{api.request(Method::PUT,&format!("users/@me/relationships/{id}"),Some(json!({"type":2}))).await.map_err(|e|e.to_string())?;api.get("users/@me/relationships").await.map_err(|e|e.to_string())});},
            "guild-mute"=>{let id=Self::valid(&action.id)?.to_owned();let guild=self.state.guilds.iter().find(|g|g.id==id).ok_or("Servidor no disponible.")?;let muted=!guild.is_muted;action.kind="__guild-muted".into();action.text=muted.to_string();self.launch(action,None,async move{api.request(Method::PATCH,&format!("users/@me/guilds/{id}/settings"),Some(json!({"muted":muted}))).await.map_err(|e|e.to_string())});},
            "guild-notifications" => {
                let id = Self::valid(&action.id)?.to_owned();
                if !self.state.guilds.iter().any(|guild|guild.id==id) { return Err("Servidor no disponible.".into()); }
                if action.text.len()>256 { return Err("Cambio de notificación demasiado grande.".into()); }
                let patch: Value = serde_json::from_str(&action.text).map_err(|_|"Cambio de notificación no válido.")?;
                let token = self.notification_settings.begin_patch(&id,patch.clone())?;
                action.kind = "__guild-notifications".into(); action.extra = token.to_string();
                self.refresh_notification_nav(); self.publish();
                self.launch(action,None,async move { execute_guild_notification_patch(&id,patch,move |method,path,body| async move {
                    api.request(method,&path,body).await.map_err(|error|error.to_string())
                }).await });
            },
            "guild-leave"=>{let id=Self::valid(&action.id)?.to_owned();if !self.state.guilds.iter().any(|g|g.id==id){return Err("Servidor no disponible.".into())}action.kind="__guild-left".into();self.launch(action,None,async move{api.request(Method::DELETE,&format!("users/@me/guilds/{id}"),None).await.map_err(|e|e.to_string())});},
            "guild-settings"=>self.status("El editor de roles y ajustes del servidor todavía no está implementado. No se ha cambiado ningún ajuste."),
            "guild-mark-read"=>{let id=Self::valid(&action.id)?.to_owned();if !self.state.guilds.iter().any(|g|g.id==id){return Err("Servidor no disponible.".into())}let me=self.me();action.kind="__guild-read".into();self.launch(action,None,async move{let guild=api.get(&format!("guilds/{id}")).await.map_err(|e|e.to_string())?;let member=api.get(&format!("guilds/{id}/members/{me}")).await.map_err(|e|e.to_string())?;let channels=api.get(&format!("guilds/{id}/channels")).await.map_err(|e|e.to_string())?;let acl=GuildAccess::from_values(&guild,&member,&me);let catalog=items(&channels).iter().take(2000).map(|c|(s(c,"id"),c.clone())).collect::<HashMap<_,_>>();let targets=catalog.values().filter(|c|acl.evaluate(c,catalog.get(&s(c,"parent_id"))).can_view&&parsing::valid_id(&s(c,"last_message_id"))).map(|c|json!({"channel_id":s(c,"id"),"message_id":s(c,"last_message_id"),"read_state_type":0})).collect::<Vec<_>>();let mut completed=Vec::new();for chunk in targets.chunks(100){if api.request(Method::POST,"read-states/ack-bulk",Some(json!({"read_states":chunk}))).await.is_err(){return Ok(json!({"completed":completed,"partial":true}))}completed.extend_from_slice(chunk)}Ok(json!({"completed":completed,"partial":false}))});},
            "guild-invite"=>{let guild=Self::valid(&action.id)?.to_owned();if !self.state.guilds.iter().any(|g|g.id==guild){return Err("Servidor no disponible.".into())}let me=self.me();action.kind="__guild-invite-channels".into();self.launch(action,None,async move{let g=api.get(&format!("guilds/{guild}")).await.map_err(|e|e.to_string())?;let m=api.get(&format!("guilds/{guild}/members/{me}")).await.map_err(|e|e.to_string())?;let channels=api.get(&format!("guilds/{guild}/channels")).await.map_err(|e|e.to_string())?;let acl=GuildAccess::from_values(&g,&m,&me);let options=items(&channels).iter().take(2000).map(|c|acl.decorate(c,None)).filter(|n|n.can_invite&&matches!(n.kind.as_str(),"text"|"voice")).take(500).collect::<Vec<_>>();if options.is_empty(){return Err("No tienes permiso para crear invitaciones en los canales disponibles.".into())}Ok(json!({"guildId":guild,"channels":options}))});},
            "create-guild-invite"=>{let channel=Self::valid(&action.id)?.to_owned();let guild=Self::valid(&action.text)?.to_owned();if !self.state.guilds.iter().any(|g|g.id==guild){return Err("Servidor no disponible.".into())}let me=self.me();action.kind="__invite-created".into();self.launch(action,None,async move{let g=api.get(&format!("guilds/{guild}")).await.map_err(|e|e.to_string())?;let m=api.get(&format!("guilds/{guild}/members/{me}")).await.map_err(|e|e.to_string())?;let c=api.get(&format!("channels/{channel}")).await.map_err(|e|e.to_string())?;let acl=GuildAccess::from_values(&g,&m,&me);if s(&c,"guild_id")!=guild||acl.channel_permissions(&c)&1025!=1025||!matches!(i(&c,"type"),0|2|5){return Err("No se pudieron confirmar permisos para crear la invitación.".into())}api.request(Method::POST,&format!("channels/{channel}/invites"),Some(json!({"max_age":86400,"max_uses":0,"temporary":false,"unique":false}))).await.map_err(|e|e.to_string())});},
            "status" => {
                self.change_presence(action, api)?;
            }
            "activity-toggle" => {
                let enabled = !self.state.game_activity_enabled;
                action.kind = "__activity".into();
                action.text = enabled.to_string();
                self.launch(action, None, async move {
                    let encoded = settings_proto::visibility_delta(enabled);
                    let response = api
                        .request(
                            Method::PATCH,
                            "users/@me/settings-proto/1",
                            Some(json!({"settings":encoded})),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    if b(&response, "out_of_date") {
                        return Err(
                            "Los ajustes cambiaron en otro cliente. Vuelve a intentarlo.".into(),
                        );
                    }
                    api.get("users/@me/settings-proto/1")
                        .await
                        .map_err(|e| e.to_string())
                });
            }
            "create-invite" => {
                if self.guild_acl.as_ref().is_none_or(|a| {
                    self.channels
                        .get(&channel)
                        .is_none_or(|v| a.channel_permissions(v) & 1 == 0)
                }) {
                    return Err("No tienes permiso para crear invitaciones.".into());
                }
                action.kind = "__invite-created".into();
                self.launch(action, None, async move {
                    api.request(
                        Method::POST,
                        &format!("channels/{channel}/invites"),
                        Some(json!({"max_age":86400,"max_uses":0,"temporary":false})),
                    )
                    .await
                    .map_err(|e| e.to_string())
                });
            }
            "members-more" => {
                if self.member_demand.is_none(){return Ok(true)}
                self.member_range = (self.member_range + 100).min(1999);
                self.subscribe().await;
            }
            "emoji-recent" | "gif-recent" | "gif-favorite" | "gif-import" => {
                let gif = action.kind != "emoji-recent";
                let val: Value = serde_json::from_str(&action.extra).unwrap_or_default();
                let picker = self.picker.as_mut().ok_or("Cuenta no disponible.")?;
                let entry = storage::PickerValue {
                    value: action.text.clone(),
                    name: if action.id.is_empty() {
                        "GIF".into()
                    } else {
                        action.id.clone()
                    },
                    image_url: s(&val, "imageUrl"),
                };
                if action.kind == "gif-favorite" {
                    picker.favorite(entry)?;
                } else if action.kind == "gif-import" {
                    picker.import_gif_links(&action.text)?;
                } else {
                    picker.recent(entry, gif)?;
                }
                let history = serde_json::to_value(&picker.data).unwrap();
                self.data("picker-history", history);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    async fn social_action_friend_accept(&mut self, id: String) -> Result<(), String> {
        Self::valid(&id)?;
        let api = self.api()?;
        self.launch(UiAction::new("__relations"), None, async move {
            api.request(
                Method::PUT,
                &format!("users/@me/relationships/{id}"),
                Some(json!({})),
            )
            .await
            .map_err(|e| e.to_string())?;
            api.get("users/@me/relationships")
                .await
                .map_err(|e| e.to_string())
        });
        Ok(())
    }
    pub(super) async fn social_complete(
        &mut self,
        action: UiAction,
        value: Value,
    ) -> Result<(), String> {
        if self.profile_editor_complete(&action,&value) {return Ok(());}
        match action.kind.as_str() {
            "__message-pin" => {
                self.apply_message_pin(&action.channel_id, &action.id, action.text == "true");
                self.publish();
            }
            "__dm_sidebar_profile" => {
                if !self.dm_profile_target(&action.channel_id, &action.id) {
                    return Ok(());
                }
                let fallback = self.dm_profile_fallback(&action.id);
                let details = parsing::profile_details(&value, &fallback, &self.state.guilds, &[]);
                if details.user.id != action.id {
                    return Err("El servicio devolvió otro perfil.".into());
                }
                self.remember(details.user.clone());
                self.data(
                    "dm-sidebar-profile",
                    json!({"channel_id":action.channel_id,"profile":details}),
                );
            }
            "__profile" => {
                if self.state.view != "profile"
                    || self
                        .state
                        .profile_user
                        .as_ref()
                        .is_none_or(|p| p.id != action.id)
                    || !self.request_current("profile", action.extra.parse().unwrap_or(0))
                {
                    return Ok(());
                }
                let fallback = self
                    .state
                    .profile_details
                    .clone()
                    .ok_or("El perfil se cerró.")?;
                // Resolve only IDs actually returned by this profile. Cloning
                // the whole 3000-person cache on every open is unnecessary.
                let people = items(&value["profile"]["mutual_friends"]).iter().take(1000)
                    .filter_map(Value::as_str).filter_map(|id|self.people.get(id).cloned()).collect::<Vec<_>>();
                let mut details = parsing::profile_details(
                    &value["profile"],
                    &fallback,
                    &self.state.guilds,
                    &people,
                );
                if value["friends"].is_array() {
                    details.mutual_friends = items(&value["friends"])
                        .iter()
                        .take(100)
                        .map(parsing::person)
                        .collect();
                    details.mutual_friends_available = true;
                }
                self.remember(details.user.clone());
                if !details.guild_id.is_empty() && details.guild_id==self.state.selected_guild_id && !self.state.guild_nicknames.contains_key(&details.user.id) {
                    self.remember_guild_nickname(&details.guild_id,&details.user.id,&json!({"nick":details.nickname}));
                }
                self.state.profile_user = Some(details.user.clone());
                self.state.profile_details = Some(details);
                self.state.profile_subtitle=if b(&value,"unavailable") {"Discord no proporcionó un perfil remoto. Se conserva la identidad observada.".into()}else{String::new()};
                self.publish();
            }
            "__relations" => {
                self.friend_pending.remove(&action.extra);
                self.state.relationships = items(&value)
                    .iter()
                    .take(3000)
                    .map(parsing::relationship)
                    .collect();
                for r in self.state.relationships.clone() {
                    self.remember(r.user);
                }
                self.publish();
            }
            "__open-dm" => {
                self.recipients(&value);
                let dm = parsing::channel(&value);
                self.state.direct_messages.retain(|n| n.id != dm.id);
                self.state.direct_messages.insert(0, dm.clone());
                self.select_channel(&dm.id)?;
            }
            "__group" => {
                if !self.group_refresh_current(&action,&value) {return Ok(());}
                self.recipients(&value);
                self.refresh_group(&value);
                self.publish();
            }
            "__reaction" => {
                let message = parsing::message(&value);
                for pinned in &mut self.state.pinned_messages {
                    if pinned.id == message.id && pinned.channel_id == message.channel_id {
                        *pinned = message.clone()
                    }
                }
                self.cache.upsert(message);
                self.refresh_messages();
                self.publish();
            }
            "__search" => {
                let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
                if self.state.view != "search"
                    || !self
                        .request_current("search", extra["requestVersion"].as_u64().unwrap_or(0))
                {
                    return Ok(());
                }
                let parsed = search::ParsedSearch {
                    author_type: s(&extra, "authorType"),
                    ..Default::default()
                };
                let list = items(&value["messages"])
                    .iter()
                    .flat_map(|v| items(v).iter())
                    .filter(|v| b(v, "hit"))
                    .take(100)
                    .map(parsing::message)
                    .filter(|m| parsed.matches(m))
                    .collect::<Vec<_>>();
                self.state.messages = list.into();
                self.state.view = "search".into();
                self.state.status = if parsed.author_type.is_empty() {
                    "Resultados de búsqueda".into()
                } else {
                    "Tipo de autor aplicado a los resultados cargados.".into()
                };
                self.publish();
            }
            "__forward-channels" => self.data("forward-channels", value),
            "__forwarded" => {
                for response in items(&value["messages"]) {
                    let Ok(message) = serde_json::from_value::<ChatMessage>(response.clone())
                    else {
                        continue;
                    };
                    if !parsing::valid_id(&message.id) || !parsing::valid_id(&message.channel_id) {
                        continue;
                    }
                    let channel = message.channel_id.clone();
                    self.observe_message_delivery(&channel, &message.id, true);
                    self.observe_message_preview(&message);
                    if !self.cache.get(&channel).is_empty() {
                        self.cache.upsert(message);
                    }
                }
                self.refresh_messages();
                self.decorate_dms();
                if !self.state.selected_guild_id.is_empty() {
                    self.rebuild_channels();
                }
                let sent = i(&value, "sent");
                let total = i(&value, "total");
                self.state.status = if b(&value, "partial") {
                    format!(
                        "Reenvío parcial: {sent} de {total} destinos. No se repitió ningún envío."
                    )
                } else {
                    format!("Mensaje reenviado a {sent} destino(s)")
                };
                self.data("forward-result",json!({"channel_id":action.channel_id,"message_id":action.id,"sent":sent,"total":total,"partial":b(&value,"partial"),"error":s(&value,"error")}));
                self.publish();
            }
            "__closed-dm" => {
                self.state.direct_messages.retain(|n| n.id != action.id);
                if self.state.selected_channel_id == action.id {
                    self.state.selected_channel_id.clear();
                    self.state.view = "friends".into();
                    self.state.messages.clear();
                }
                self.publish();
            }
            "__marked-read" => {
                if action.extra=="automatic"{self.read_acks.complete(&action.id);}
                let last = s(&value, "last");
                if parsing::valid_id(&last) {
                    self.mark_read_through(&action.id, &last);
                    self.decorate_dms();
                    if !self.state.selected_guild_id.is_empty() {
                        self.rebuild_channels();
                    }
                    if action.extra != "automatic" {
                        self.state.status = "Lectura del canal actualizada".into();
                    }
                    self.publish();
                } else if action.extra != "automatic" {
                    self.state.status = "El canal no tiene mensajes para marcar.".into();
                    self.publish();
                }
            }
            "__guild-muted" => {
                self.notification_settings.apply(&json!({"guild_id":action.id,"muted":action.text=="true"}));
                for guild in &mut self.state.guilds {
                    if guild.id == action.id {
                        guild.is_muted = action.text == "true"
                    }
                }
                self.publish();
            }
            "__guild-notifications" => {
                let token = action.extra.parse::<u64>().unwrap_or(0);
                if let Some(success) = self.notification_settings.finish_patch(&action.id, token, b(&value,"success")) {
                    self.refresh_notification_nav();
                    if !success { self.error(if s(&value,"error").is_empty() { "Discord no confirmó el ajuste; se conserva el anterior.".into() } else { s(&value,"error") }); }
                    self.publish();
                }
            }
            "__guild-left" => {
                self.state.guilds.retain(|g| g.id != action.id);
                if self.state.selected_guild_id == action.id {
                    self.select_guild("")?;
                }
                self.status("Has salido del servidor");
            }
            "__guild-read" => {
                let mut all_confirmed =
                    !b(&value, "partial") && !items(&value["completed"]).is_empty();
                for read in items(&value["completed"]) {
                    let channel = s(read, "channel_id");
                    let last = s(read, "message_id");
                    self.mark_read_through(&channel, &last);
                    all_confirmed &= parsing::valid_id(&channel)
                        && parsing::valid_id(&last)
                        && self.latest.get(&channel).is_none_or(|id| {
                            id.len() < last.len() || id.len() == last.len() && *id <= last
                        });
                }
                if all_confirmed {
                    for guild in &mut self.state.guilds {
                        if guild.id == action.id {
                            guild.unread = false;
                            guild.mention_count = 0;
                        }
                    }
                }
                self.decorate_dms();
                self.rebuild_channels();
                self.state.status = if b(&value, "partial") {
                    "Se marcó parte del servidor como leído. Discord no confirmó los demás canales."
                } else {
                    "Canales del servidor marcados como leídos"
                }
                .into();
                self.publish();
            }
            "__guild-invite-channels" => self.data("guild-invite-channels", value),
            "__muted" => {
                self.muted.insert(action.id, action.text == "true");
                self.decorate_dms();
                self.publish();
            }
            "__status" => {
                self.finish_presence_change(&action, Some(&value))?;
            }
            "__activity" => {
                let proto = settings_proto::read(&s(&value, "settings"), false)
                    .map_err(|e| e.to_string())?;
                self.state.game_activity_enabled = proto.show_current_game.ok_or("Discord no confirmó la visibilidad de actividad; se conserva la configuración anterior.")?;
                self.state.game_activity_available = true;
                self.state.game_activity_configured = true;
                if self.state.game_activity_enabled != (action.text == "true") {
                    return Err("Discord no confirmó el cambio de visibilidad. Se conserva el estado leído del servidor.".into());
                }
                self.tick_game_activity().await;
                self.publish();
            }
            "__invite-created" => self.data(
                "invite-created",
                json!(format!("https://discord.gg/{}", s(&value, "code"))),
            ),
            _ => (),
        }
        Ok(())
    }
    pub(super) fn recipients(&mut self, channel: &Value) {
        for value in items(&channel["recipients"]).iter().take(25) {
            self.remember(parsing::person(value));
        }
    }
    pub(super) fn member(&self, user: Person, owner: &str) -> ChannelMember {
        let p = self
            .state
            .user_presences
            .get(&user.id)
            .cloned()
            .unwrap_or_default();
        let presence = if user.id == self.me() {
            self.state.self_presence.clone()
        } else {
            p.status
        };
        ChannelMember {
            is_owner: user.id == owner,
            user,
            presence,
            custom_status: p.custom_status,
            custom_status_emoji: p.custom_status_emoji,
            activities: p.activities,
            ..Default::default()
        }
    }
    pub(super) fn refresh_group(&mut self, channel: &Value) {
        let kind = i(channel, "type");
        self.state.selected_channel_kind = if kind == 3 {
            "group"
        } else if kind == 1 {
            "dm"
        } else {
            self.state.selected_channel_kind.as_str()
        }
        .into();
        if kind != 3 {
            self.state.group_owner_id.clear();
            self.state.group_member_removal_pending_ids.clear();
            return;
        }
        let old_owner=self.state.group_owner_id.clone();
        let old_ids:HashSet<_>=self.state.group_members.iter().map(|m|m.user.id.clone()).collect();
        let owner=if channel.get("owner_id").is_some(){s(channel,"owner_id")}else{old_owner.clone()};
        self.state.group_owner_id=if parsing::valid_id(&owner){owner.clone()}else{String::new()};
        let mut users=if channel.get("recipients").is_some(){items(&channel["recipients"]).iter().take(25).map(parsing::person).collect::<Vec<_>>()}
            else{self.state.group_members.iter().map(|member|member.user.clone()).collect()};
        if let Some(me) = self.state.current_user.clone() {
            users.push(me);
        }
        let mut seen = HashSet::new();
        users.retain(|p| !p.id.is_empty() && seen.insert(p.id.clone()));
        let new_ids:HashSet<_>=users.iter().map(|user|user.id.clone()).collect();
        if old_owner!=self.state.group_owner_id||old_ids!=new_ids {
            self.state.member_context_version=self.state.member_context_version.wrapping_add(1);
        }
        self.state.group_member_removal_pending_ids.retain(|id|new_ids.contains(id));
        self.state.group_members = users.into_iter().map(|p| self.member(p, &owner)).collect();
        self.state.group_members.sort_by(|a, b| {
            b.is_owner
                .cmp(&a.is_owner)
                .then(
                    (!matches!(a.presence.as_str(), "online" | "idle" | "dnd"))
                        .cmp(&(!matches!(b.presence.as_str(), "online" | "idle" | "dnd"))),
                )
                .then(a.user.name.to_lowercase().cmp(&b.user.name.to_lowercase()))
        });
        let ids = self
            .state
            .group_members
            .iter()
            .map(|m| m.user.id.clone())
            .collect::<std::collections::HashSet<_>>();
        self.state.group_recipient_candidates = self
            .state
            .relationships
            .iter()
            .filter(|r| r.kind == 1 && !ids.contains(&r.user.id))
            .map(|r| r.user.clone())
            .collect();
        self.search_people();
    }
}
fn forward_targets(action: &UiAction) -> Result<Vec<String>, String> {
    let raw = if action.kind == "forward-many" {
        &action.text
    } else if !action.extra.is_empty() {
        &action.extra
    } else {
        &action.text
    };
    let values: Value = if parsing::valid_id(raw) {
        json!([raw])
    } else {
        serde_json::from_str(raw).map_err(|_| "La lista de destinos no es válida.")?
    };
    let values = if values.is_array() {
        items(&values)
    } else {
        items(&values["destinations"])
    };
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for value in values {
        let id = value
            .as_str()
            .or_else(|| value["Id"].as_str())
            .or_else(|| value["id"].as_str())
            .ok_or("Un destino no es válido.")?;
        valid_forward_id(id)?;
        if seen.insert(id.to_owned()) {
            targets.push(id.to_owned());
        }
        if targets.len() > 10 {
            return Err("Selecciona entre uno y diez destinos.".into());
        }
    }
    if targets.is_empty() {
        return Err("Selecciona entre uno y diez destinos.".into());
    }
    Ok(targets)
}
// Local validation is shared by the executable helper and its isolated route fixtures.
fn valid_forward_id(id: &str) -> Result<(), String> {
    if parsing::valid_id(id) {
        Ok(())
    } else {
        Err("Un identificador de reenvío no es válido.".into())
    }
}
fn forward_payload(source: &ChatMessage, guild: &str) -> Value {
    json!({"message_reference":{"type":1,"message_id":source.id,"channel_id":source.channel_id,"guild_id":if guild.is_empty(){None}else{Some(guild)}},"allowed_mentions":{"parse":[],"replied_user":false}})
}
async fn execute_forward<F, Fut>(
    source: &ChatMessage,
    origin_guild: &str,
    targets: &[String],
    dm_ids: &HashSet<String>,
    guild_ids: &HashSet<String>,
    me: &str,
    mut request: F,
) -> Result<Value, String>
where
    F: FnMut(Method, String, Option<Value>) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    valid_forward_id(&source.id)?;
    valid_forward_id(&source.channel_id)?;
    if source.deleted || source.forwarded || !matches!(source.message_type, 0 | 19) {
        return Err("Sólo se pueden reenviar mensajes originales disponibles.".into());
    }
    if !origin_guild.is_empty() {
        valid_forward_id(origin_guild)?;
    }
    let has_files = source
        .attachments
        .iter()
        .any(|file| !file.id.starts_with("embed-"));
    let has_embeds = source
        .attachments
        .iter()
        .any(|file| file.id.starts_with("embed-"));
    let mut guild_access = HashMap::<String, GuildAccess>::new();
    // All destinations are checked before the first POST. Known private chats do
    // not need a redundant GET; they are already the authenticated sidebar list.
    for target in targets {
        valid_forward_id(target)?;
        if dm_ids.contains(target) {
            continue;
        }
        let channel = request(Method::GET, format!("channels/{target}"), None).await?;
        if s(&channel, "id") != *target || !matches!(i(&channel, "type"), 0 | 5 | 10 | 11 | 12) {
            return Err(
                "Un destino no es un canal de texto disponible. No se ha reenviado nada.".into(),
            );
        }
        let guild = s(&channel, "guild_id");
        if !parsing::valid_id(&guild) || !guild_ids.contains(&guild) {
            return Err(
                "Uno de los servidores de destino ya no está disponible. No se ha reenviado nada."
                    .into(),
            );
        }
        if !guild_access.contains_key(&guild) {
            let raw = request(Method::GET, format!("guilds/{guild}"), None).await?;
            let member = request(Method::GET, format!("guilds/{guild}/members/{me}"), None).await?;
            guild_access.insert(guild.clone(), GuildAccess::from_values(&raw, &member, me));
        }
        let parent = if matches!(i(&channel, "type"), 10 | 11 | 12) {
            let parent = s(&channel, "parent_id");
            valid_forward_id(&parent)?;
            Some(request(Method::GET, format!("channels/{parent}"), None).await?)
        } else {
            None
        };
        let access = guild_access[&guild].evaluate(&channel, parent.as_ref());
        if !access.can_view
            || !access.can_send
            || has_files && !access.can_attach
            || has_embeds && !access.can_embed
        {
            return Err(
                "Uno de los destinos no permite enviar este contenido. No se ha reenviado nada."
                    .into(),
            );
        }
    }
    let mut messages = Vec::new();
    let mut sent = 0;
    for target in targets {
        match request(
            Method::POST,
            format!("channels/{target}/messages"),
            Some(forward_payload(source, origin_guild)),
        )
        .await
        {
            Ok(value) => {
                sent += 1;
                let message = parsing::message(&value);
                if parsing::valid_id(&message.id) && message.channel_id == *target {
                    messages.push(
                        serde_json::to_value(message)
                            .map_err(|_| "No se pudo actualizar el mensaje reenviado.")?,
                    );
                }
            }
            Err(error) => {
                return Ok(
                    json!({"sent":sent,"total":targets.len(),"partial":true,"error":if sent==0{format!("Discord rechazó el reenvío: {error}")}else{format!("Reenvío parcial: {sent} de {} destinos. Los envíos confirmados no se reintentaron. {error}",targets.len())},"messages":messages}),
                );
            }
        }
    }
    Ok(json!({"sent":sent,"total":targets.len(),"partial":false,"error":"","messages":messages}))
}
fn invite_code(value: &str) -> Result<String, String> {
    let url = crate::platform::official_invite_url(value)?;
    Ok(url.rsplit('/').next().unwrap().to_owned())
}
pub(super) fn reaction_key(text: &str) -> Result<String, String> {
    let text = text.trim();
    let custom =
        regex::Regex::new(r"^<?(?:(?:a)?:)?(?P<name>[A-Za-z0-9_]{1,100}):(?P<id>[0-9]{1,24})>?$")
            .unwrap();
    if let Some(c) = custom.captures(text) {
        return Ok(format!("{}:{}", &c["name"], &c["id"]));
    }
    if text.is_empty()
        || text.encode_utf16().count() > 80
        || text.chars().any(char::is_control)
        || text.contains(['/', '<'])
    {
        return Err("Selecciona un emoji válido para reaccionar.".into());
    }
    Ok(text.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn notification_patch_is_single_manual_write_and_commits_only_its_own_field() {
        let (mut controller, _) = Controller::test_fixture();
        controller.state.guilds = vec![NavItem { id:"10".into(),kind:"guild".into(),..Default::default() }];
        controller.notification_settings.apply(&json!({"guild_id":"10","message_notifications":1,"suppress_roles":true}));
        let patch = json!({"message_notifications":0});
        let token = controller.notification_settings.begin_patch("10",patch.clone()).unwrap();
        let result = execute_guild_notification_patch("10",patch,|method,path,body|async move {
            assert_eq!(method,Method::PATCH); assert_eq!(path,"users/@me/guilds/10/settings");
            assert_eq!(body.unwrap(),json!({"message_notifications":0}));
            // Full REST payload is deliberately not passed to completion.
            Ok(json!({"message_notifications":0,"suppress_roles":false,"suppress_everyone":false}))
        }).await.unwrap();
        controller.notification_settings.apply(&json!({"guild_id":"10","suppress_everyone":true}));
        let action = UiAction { kind:"__guild-notifications".into(),id:"10".into(),extra:token.to_string(),..Default::default() };
        controller.social_complete(action.clone(),result).await.unwrap();
        let settings = &controller.state.guilds[0].guild_notifications;
        assert_eq!(settings.message_notifications,Some(0)); assert!(settings.suppress_roles && settings.suppress_everyone);
        let second = controller.notification_settings.begin_patch("10",json!({"suppress_roles":false})).unwrap();
        controller.social_complete(action,json!({"success":true})).await.unwrap();
        assert!(controller.notification_settings.get("10").pending,"An old completion must not finish the new patch");
        let failure = execute_guild_notification_patch("10",json!({"suppress_roles":false}),|_,_,_|async {Err("Offline rejection".into())}).await.unwrap();
        controller.social_complete(UiAction {kind:"__guild-notifications".into(),id:"10".into(),extra:second.to_string(),..Default::default()},failure).await.unwrap();
        assert!(controller.notification_settings.get("10").suppress_roles);
        assert!(!controller.notification_settings.get("10").pending);
    }
    #[tokio::test]
    async fn hidden_muted_channels_are_local_persistent_and_account_scoped_without_an_api() {
        let (mut controller, _) = Controller::test_fixture();
        controller.state.current_user=Some(Person {id:"1".into(),..Default::default()});
        controller.state.guilds=vec![NavItem {id:"10".into(),kind:"guild".into(),..Default::default()}];
        assert!(controller.api.is_none());
        controller.social_action(UiAction {kind:"guild-hide-muted".into(),id:"10".into(),text:"true".into(),..Default::default()}).await.unwrap();
        assert!(controller.state.guilds[0].guild_notifications.hide_muted_channels);
        assert_eq!(storage::read_json::<Vec<String>>(&controller.data.join("notification-views/1.json"),32768),vec!["10".to_owned()]);
        controller.notification_settings=Default::default(); controller.load_local_notification_views();
        assert!(controller.notification_settings.get("10").hide_muted_channels);
        controller.state.current_user.as_mut().unwrap().id="2".into(); controller.notification_settings=Default::default(); controller.load_local_notification_views();
        assert!(!controller.notification_settings.get("10").hide_muted_channels);
        let (mut failed, _) = Controller::test_fixture();
        failed.state.current_user=Some(Person {id:"1".into(),..Default::default()});
        failed.state.guilds=vec![NavItem {id:"10".into(),kind:"guild".into(),..Default::default()}];
        std::fs::write(failed.data.join("notification-views"), b"not a directory").unwrap();
        assert!(failed.social_action(UiAction {kind:"guild-hide-muted".into(),id:"10".into(),text:"true".into(),..Default::default()}).await.is_err());
        assert!(!failed.notification_settings.get("10").hide_muted_channels,"Failed persistence must keep the previous preference");
    }
    #[tokio::test]
    async fn observed_webhook_profile_is_available_without_user_endpoints_or_invalid_social_actions() {
        let (mut c,_,mut jobs)=Controller::test_fixture_with_replies();c.state.signed_in=true;c.state.view="chat".into();
        c.state.messages.push(ChatMessage {id:"300".into(),channel_id:"200".into(),author:Person{id:"400".into(),name:"Webhook visible".into(),is_bot:true,..Default::default()},webhook_id:"400".into(),..Default::default()});
        c.social_action(UiAction::id("profile","400")).await.unwrap();
        assert_eq!(c.state.view,"profile");assert_eq!(c.state.profile_user.as_ref().unwrap().name,"Webhook visible");
        assert!(c.state.profile_details.as_ref().unwrap().is_webhook);assert!(c.api.is_none());assert!(jobs.try_recv().is_err());
        for kind in ["open-dm","add-friend"] {assert!(c.social_action(UiAction::id(kind,"400")).await.unwrap_err().contains("webhook"));}
        assert!(jobs.try_recv().is_err());
    }
    #[tokio::test]
    async fn bot_profile_partial_or_unavailable_response_keeps_identity_from_the_visible_author() {
        let (mut c,_,mut jobs)=Controller::test_fixture_with_replies();c.state.signed_in=true;c.state.view="chat".into();
        let bot=Person{id:"400".into(),name:"Bot visible".into(),username:"bot_name".into(),display_name:"Bot visible".into(),is_bot:true,..Default::default()};
        c.state.messages.push(ChatMessage {id:"300".into(),channel_id:"200".into(),author:bot.clone(),..Default::default()});
        c.social_action(UiAction::id("profile","400")).await.unwrap();
        assert_eq!(c.state.profile_user,Some(bot.clone()));assert!(!c.state.profile_details.as_ref().unwrap().is_webhook);assert!(jobs.try_recv().is_err());
        let mut response=UiAction::id("__profile","400");response.extra=c.request_versions["profile"].to_string();
        c.social_complete(response.clone(),json!({"profile":{"user":{"id":"400"}},"friends":null})).await.unwrap();
        assert_eq!(c.state.profile_user,Some(bot.clone()));
        c.social_complete(response,json!({"profile":null,"unavailable":true})).await.unwrap();
        assert_eq!(c.state.profile_user,Some(bot));assert!(c.state.profile_subtitle.contains("conserva"));assert!(jobs.try_recv().is_err());
    }
    #[tokio::test]
    async fn invite_review_and_legacy_accept_delegate_without_any_api_or_http_job() {
        for legacy_text in [false, true] {
            let (mut c, updates, mut replies) = Controller::test_fixture_with_replies();
            c.state.signed_in = true;
            assert!(c.api.is_none(), "Fixture must not have an HTTP transport");
            let mut preview = UiAction::new("invite-preview");
            preview.text = "https://discord.com/invite/Abc_12-X".into();
            c.act(preview).await.unwrap();
            let preview = updates.try_iter().filter_map(|update| match update {
                UiUpdate::Data(kind, data) if kind == "invite-preview" => Some(data),
                _ => None,
            }).collect::<Vec<_>>();
            assert_eq!(preview.len(), 1);
            assert_eq!(preview[0]["code"], "Abc_12-X");
            assert_eq!(preview[0]["delegated"], true);
            assert!(replies.try_recv().is_err(), "Local review must not schedule HTTP work");

            let mut accept = UiAction::new("invite-accept");
            if legacy_text { accept.text = "Abc_12-X".into(); }
            else { accept.id = "Abc_12-X".into(); }
            c.act(accept).await.unwrap();
            let opened = updates.try_iter().filter_map(|update| match update {
                UiUpdate::Data(kind, data) if kind == "open-official-invite" => Some(data),
                _ => None,
            }).collect::<Vec<_>>();
            assert_eq!(opened, [json!({"url":"https://discord.gg/Abc_12-X"})]);
            assert!(replies.try_recv().is_err(), "Delegating must not enqueue a native join");
            assert!(c.api.is_none());
        }
    }

    #[tokio::test]
    async fn invalid_or_conflicting_invites_never_delegate_or_schedule_network_work() {
        for action in [
            UiAction::id("invite-accept", "https://evil.test/abc"),
            UiAction::id("invite-accept", "https://discord.gg/abc?token=secret"),
            UiAction { kind:"invite-accept".into(),id:"abc".into(),text:"different".into(),..Default::default() },
            UiAction { kind:"invite-preview".into(),text:"https://user@discord.gg/abc".into(),..Default::default() },
        ] {
            let (mut c, updates, mut replies) = Controller::test_fixture_with_replies();
            c.state.signed_in = true;
            assert!(c.act(action).await.is_err());
            assert!(!updates.try_iter().any(|update| matches!(update, UiUpdate::Data(_, _))));
            assert!(replies.try_recv().is_err());
        }
    }
    fn source_forward() -> ChatMessage {
        ChatMessage {
            id: "100".into(),
            channel_id: "20".into(),
            content: "Fixture source".into(),
            ..Default::default()
        }
    }
    #[test]
    fn forward_target_contract_preserves_selection_order_and_guild_origin() {
        let action = UiAction {
            kind: "forward-many".into(),
            text: r#"["30","31","30"]"#.into(),
            ..Default::default()
        };
        assert_eq!(forward_targets(&action).unwrap(), ["30", "31"]);
        assert!(
            forward_targets(&UiAction {
                kind: "forward-many".into(),
                text: r#"["30",null]"#.into(),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            forward_targets(&UiAction {
                kind: "forward-many".into(),
                text: serde_json::to_string(&(1..12).map(|id| id.to_string()).collect::<Vec<_>>())
                    .unwrap(),
                ..Default::default()
            })
            .is_err()
        );
        assert_eq!(
            forward_payload(&source_forward(), "10")["message_reference"]["guild_id"],
            "10"
        );
        assert!(forward_payload(&source_forward(), "")["message_reference"]["guild_id"].is_null());
    }
    #[tokio::test]
    async fn forwarding_to_known_private_chats_posts_native_reference_without_redundant_gets() {
        let calls = std::sync::Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let targets = vec!["30".into(), "31".into()];
        let private = targets.iter().cloned().collect();
        let result=execute_forward(&source_forward(),"10",&targets,&private,&HashSet::new(),"1",move |method,path,body|{let calls=calls.clone();async move{
            assert_eq!(method,Method::POST);let body=body.unwrap();assert_eq!(body["message_reference"],json!({"type":1,"message_id":"100","channel_id":"20","guild_id":"10"}));assert_eq!(body["allowed_mentions"]["parse"],json!([]));
            let target=path.split('/').nth(1).unwrap().to_owned();calls.lock().unwrap().push(path);
            Ok(json!({"id":format!("9{target}"),"channel_id":target,"message_reference":{"type":1},"message_snapshots":[{"message":{"content":"Fixture source"}}]}))
        }}).await.unwrap();
        assert_eq!(
            *observed.lock().unwrap(),
            ["channels/30/messages", "channels/31/messages"]
        );
        assert_eq!(result["sent"], 2);
        assert_eq!(result["partial"], false);
        let message: ChatMessage = serde_json::from_value(result["messages"][0].clone()).unwrap();
        assert!(message.forwarded);
        assert_eq!(message.content, "Fixture source");
    }
    #[tokio::test]
    async fn forwarding_checks_all_permissions_before_posting_and_rejects_denied_guild_destination()
    {
        let calls = std::sync::Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let targets = vec!["30".into(), "90".into()];
        let private = HashSet::from(["30".into()]);
        let guilds = HashSet::from(["10".into()]);
        let result = execute_forward(
            &source_forward(),
            "",
            &targets,
            &private,
            &guilds,
            "1",
            move |method, path, _| {
                let calls = calls.clone();
                async move {
                    calls.lock().unwrap().push((method.clone(), path.clone()));
                    assert_eq!(method, Method::GET);
                    Ok(match path.as_str() {
                        "channels/90" => json!({"id":"90","guild_id":"10","type":0}),
                        "guilds/10" => {
                            json!({"id":"10","roles":[{"id":"10","permissions":"1024"}]})
                        }
                        "guilds/10/members/1" => json!({"user":{"id":"1"},"roles":[]}),
                        _ => panic!("Unexpected route {path}"),
                    })
                }
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(observed.lock().unwrap().len(), 3);
    }
    #[tokio::test]
    async fn forwarding_partial_failure_reports_confirmed_destinations_and_never_retries() {
        let calls = std::sync::Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let targets = vec!["30".into(), "31".into(), "32".into()];
        let private = targets.iter().cloned().collect();
        let result = execute_forward(
            &source_forward(),
            "",
            &targets,
            &private,
            &HashSet::new(),
            "1",
            move |method, path, _| {
                let calls = calls.clone();
                async move {
                    assert_eq!(method, Method::POST);
                    calls.lock().unwrap().push(path.clone());
                    if path == "channels/31/messages" {
                        Err("Fixture rejection".into())
                    } else {
                        Ok(json!({"id":"900","channel_id":"30","message_reference":{"type":1}}))
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(result["sent"], 1);
        assert_eq!(result["total"], 3);
        assert_eq!(result["partial"], true);
        assert_eq!(items(&result["messages"]).len(), 1);
        assert_eq!(
            *observed.lock().unwrap(),
            ["channels/30/messages", "channels/31/messages"]
        );
    }
    #[tokio::test]
    async fn late_manual_and_automatic_read_completions_are_monotonic_and_keep_navigation_busy() {
        let (mut c, _) = Controller::test_fixture();
        c.state.busy = true;
        c.latest.insert("20".into(), "101".into());
        c.read.insert("20".into(), "100".into());
        c.mention.insert("20".into(), 1);
        let mut action = UiAction::id("__marked-read", "20");
        action.extra = "automatic".into();
        c.social_complete(action, json!({"last":"90"}))
            .await
            .unwrap();
        assert_eq!(c.read["20"], "100");
        assert_eq!(c.mention["20"], 1);
        assert!(c.state.busy);
        c.social_complete(UiAction::id("__marked-read", "20"), json!({"last":"100"}))
            .await
            .unwrap();
        assert_eq!(c.mention["20"], 1);
        assert!(c.state.busy);
    }
    #[tokio::test]
    async fn private_read_and_forward_completions_do_not_rebuild_a_duplicate_channel_catalogue() {
        let (mut c, _) = Controller::test_fixture();
        c.state.current_user = Some(Person {
            id: "1".into(),
            ..Default::default()
        });
        c.state.direct_messages.push(NavItem {
            id: "20".into(),
            kind: "dm".into(),
            last_message_id: "90".into(),
            ..Default::default()
        });
        // A leftover raw private channel must never become a guild sidebar row.
        c.channels.insert("20".into(), json!({"id":"20","type":1}));
        c.read.insert("20".into(), "90".into());
        c.social_complete(UiAction::id("__marked-read", "20"), json!({"last":"90"}))
            .await
            .unwrap();
        assert!(c.state.channels.is_empty());
        let forwarded = parsing::message(
            &json!({"id":"100","channel_id":"20","author":{"id":"1"},"message_reference":{"type":1}}),
        );
        c.social_complete(UiAction::id("__forwarded", "80"), json!({
            "sent":1,"total":1,"partial":false,"messages":[serde_json::to_value(forwarded).unwrap()]
        })).await.unwrap();
        assert!(c.state.channels.is_empty());
        assert_eq!(c.state.direct_messages.len(), 1);
        assert!(!c.state.direct_messages[0].unread);
        assert_eq!(c.state.direct_messages[0].last_message_id, "100");
        assert_eq!(c.read["20"], "90");
        c.dispatch("MESSAGE_CREATE", json!({"id":"100","channel_id":"20","author":{"id":"1"},"message_reference":{"type":1}})).await.unwrap();
        assert!(c.state.channels.is_empty());
        assert!(!c.state.direct_messages[0].unread);
    }
    #[tokio::test]
    async fn guild_read_completion_does_not_clear_a_newer_message_or_another_guild() {
        let (mut c, _) = Controller::test_fixture();
        c.state.guilds = vec![
            NavItem {
                id: "10".into(),
                unread: true,
                mention_count: 2,
                ..Default::default()
            },
            NavItem {
                id: "11".into(),
                unread: true,
                mention_count: 4,
                ..Default::default()
            },
        ];
        c.latest.insert("20".into(), "101".into());
        c.mention.insert("20".into(), 1);
        c.mention.insert("21".into(), 2);
        c.state.busy = true;
        c.social_complete(
            UiAction::id("__guild-read", "10"),
            json!({"completed":[{"channel_id":"20","message_id":"100"}],"partial":false}),
        )
        .await
        .unwrap();
        assert_eq!(c.read["20"], "100");
        assert_eq!(c.mention["20"], 1);
        assert_eq!(c.mention["21"], 2);
        assert!(c.state.guilds.iter().all(|g| g.unread));
        assert_eq!(c.state.guilds[1].mention_count, 4);
        assert!(c.state.busy);
    }
    #[test]
    fn invitation_origin_not_arbitrary_url_suffix() {
        assert_eq!(invite_code("https://discord.gg/abc").unwrap(), "abc");
        assert_eq!(invite_code("abc").unwrap(), "abc");
        assert!(invite_code("https://evil.test/abc").is_err());
        assert!(invite_code("http://discord.gg/abc").is_err());
    }
    #[test]
    fn reaction_custom_and_system_keys() {
        assert_eq!(reaction_key("<a:wave:123>").unwrap(), "wave:123");
        assert_eq!(reaction_key("😀").unwrap(), "😀");
        assert!(reaction_key("<@123>").is_err());
    }
    #[test]
    fn group_members_dedup_self_and_use_self_presence() {
        let (mut c, _) = Controller::test_fixture();
        c.state.current_user = Some(Person {
            id: "1".into(),
            name: "self".into(),
            ..Default::default()
        });
        c.state.self_presence = "idle".into();
        c.refresh_group(&json!({"type":3,"owner_id":"2","recipients":[{"id":"1","username":"self"},{"id":"2","username":"owner"},{"id":"2","username":"owner"}]}));
        assert_eq!(c.state.group_members.len(), 2);
        assert_eq!(c.state.group_members[0].user.id, "2");
        assert_eq!(
            c.state
                .group_members
                .iter()
                .find(|m| m.user.id == "1")
                .unwrap()
                .presence,
            "idle"
        );
    }
}
