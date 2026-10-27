use super::*;
fn update_member_roles(member:&mut ChannelMember,value:&Value,catalog:&[GuildRole]) {
    let Some(raw)=value["roles"].as_array()else{return;};
    let ids=raw.iter().take(500).filter_map(Value::as_str).collect::<HashSet<_>>();
    member.roles=catalog.iter().filter(|role|ids.contains(role.id.as_str())).cloned().collect();
    member.role_color=member.roles.iter().find(|role|role.color!=0).map(|role|role.color).unwrap_or(0);
    member.hoist_role_id=member.roles.iter().find(|role|role.hoist).map(|role|role.id.clone()).unwrap_or_default();
}

fn image_attachment(attachment: &ChatAttachment) -> bool {
    let content_type = attachment
        .content_type
        .as_deref()
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if content_type.starts_with("image/") && content_type.len() > "image/".len() {
        return true;
    }
    // Videos also have width/height. An explicit non-image MIME type takes
    // precedence over dimensions or a misleading filename.
    if !content_type.is_empty() && content_type != "application/octet-stream" {
        return false;
    }
    let extension = |name: &str| {
        name.rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
    };
    let suffix = extension(&attachment.name).or_else(|| {
        url::Url::parse(&attachment.url)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .and_then(|url| extension(url.path()))
    });
    if let Some(suffix) = suffix {
        return matches!(
            suffix.as_str(),
            "png"
                | "apng"
                | "jpg"
                | "jpeg"
                | "jpe"
                | "gif"
                | "webp"
                | "avif"
                | "bmp"
                | "dib"
                | "tif"
                | "tiff"
                | "ico"
                | "heic"
                | "heif"
                | "svg"
        );
    }
    attachment.width > 0 && attachment.height > 0
}

fn message_notice_body(message: &ChatMessage) -> String {
    if message.forwarded || !message.stickers.is_empty() {
        return parsing::message_preview(message)
            .chars()
            .take(160)
            .collect();
    }
    if !message.content.trim().is_empty() {
        return message.content.chars().take(50).collect();
    }
    if message.attachments.iter().any(image_attachment) {
        "Envió una imagen".into()
    } else if message.attachments.is_empty() {
        "Nuevo mensaje".into()
    } else {
        "Adjunto".into()
    }
}

#[cfg(test)]
mod notice_body_tests {
    use super::*;

    fn with_attachment(attachment: ChatAttachment) -> ChatMessage {
        ChatMessage {
            attachments: vec![attachment],
            ..Default::default()
        }
    }

    #[test]
    fn image_only_gateway_message_has_the_requested_notification_body() {
        let message = parsing::message(&serde_json::json!({
            "id":"100", "channel_id":"200", "content":"",
            "author":{"id":"300", "username":"Synthetic"},
            "attachments":[{"id":"400", "filename":"photo.PNG",
                "url":"https://cdn.discordapp.com/attachments/200/400/photo.PNG?ex=123",
                "content_type":"image/png", "width":640, "height":480}]
        }));
        assert_eq!(message_notice_body(&message), "Envió una imagen");
    }

    #[test]
    fn image_metadata_and_safe_suffix_fallbacks_cover_missing_fields() {
        for attachment in [
            ChatAttachment {
                content_type: Some("IMAGE/WEBP; charset=binary".into()),
                ..Default::default()
            },
            ChatAttachment {
                name: "image.JPEG".into(),
                ..Default::default()
            },
            ChatAttachment {
                url: "https://cdn.discordapp.com/photo.avif?ex=123&hm=456".into(),
                ..Default::default()
            },
            ChatAttachment {
                name: "upload".into(),
                width: 320,
                height: 240,
                content_type: Some("application/octet-stream".into()),
                ..Default::default()
            },
        ] {
            assert_eq!(
                message_notice_body(&with_attachment(attachment)),
                "Envió una imagen"
            );
        }
    }

    #[test]
    fn video_dimensions_and_misleading_names_do_not_label_arbitrary_files_as_images() {
        for attachment in [
            ChatAttachment {
                name: "clip.mp4".into(),
                width: 640,
                height: 480,
                ..Default::default()
            },
            ChatAttachment {
                name: "photo.png".into(),
                content_type: Some("video/mp4".into()),
                width: 640,
                height: 480,
                ..Default::default()
            },
            ChatAttachment {
                name: "report.pdf".into(),
                content_type: Some("application/pdf".into()),
                ..Default::default()
            },
            ChatAttachment {
                name: "photo.png.exe".into(),
                width: 640,
                height: 480,
                ..Default::default()
            },
            ChatAttachment {
                url: "https://cdn.discordapp.com/archive.zip?name=photo.png".into(),
                width: 640,
                height: 480,
                ..Default::default()
            },
            ChatAttachment {
                name: "archive.zip".into(),
                content_type: Some("application/octet-stream".into()),
                ..Default::default()
            },
            ChatAttachment::default(),
        ] {
            assert_eq!(message_notice_body(&with_attachment(attachment)), "Adjunto");
        }
    }

    #[test]
    fn real_text_keeps_its_original_content_and_existing_character_limit() {
        let mut message = with_attachment(ChatAttachment {
            name: "photo.png".into(),
            ..Default::default()
        });
        message.content = "  Mira esta imagen: café 🙂".into();
        assert_eq!(message_notice_body(&message), message.content);
        message.content = "🙂".repeat(60);
        assert_eq!(message_notice_body(&message), "🙂".repeat(50));
        message.content = " \n\t".into();
        assert_eq!(message_notice_body(&message), "Envió una imagen");
        assert_eq!(
            message_notice_body(&ChatMessage::default()),
            "Nuevo mensaje"
        );
    }
}

#[cfg(test)]
fn notice_with_origin(notice: &DesktopNotice, account: &str, generation: u64) -> Value {
    notice_with_epoch(notice, account, generation, 0)
}
fn notice_with_epoch(notice: &DesktopNotice, account: &str, generation: u64, epoch: u64) -> Value {
    let mut value = serde_json::to_value(notice).unwrap();
    value["accountId"] = json!(account);
    value["generation"] = json!(generation);
    value["notificationEpoch"] = json!(epoch);
    value
}

impl Controller {
    pub(super) async fn dispatch(&mut self, kind: &str, value: Value) -> Result<(), String> {
        self.moderation_gateway_delta(kind, &value);
        if self
            .api
            .as_ref()
            .is_some_and(|api| api.safety().state() != SafetyState::Allowed)
        {
            self.sync_network_safety().await;
            return Ok(());
        }
        if let Some(delta)=request_policy::catalog_delta(kind,&value){
            request_policy::overlay_sidebar(&mut self.sidebar_seed,kind,&delta);
            if request_policy::invalidate_partial_component(&mut self.sidebar_seed,kind,&delta)&&!self.sidebar_inflight{self.sidebar_wait=Some(Instant::now()+Duration::from_millis(250));}
            if self.sidebar_inflight {
                let bytes=delta.to_string().len();
                if self.sidebar_deltas.len()>=256||self.sidebar_delta_bytes.saturating_add(bytes)>512*1024 {
                    if let Some(api)=&self.api{api.safety().block(SafetyState::SynchronizationRequired);}
                    return Err("El catálogo cambió demasiado durante su carga. Vuelve a conectar para reconciliarlo.".into());
                }
                self.sidebar_delta_bytes+=bytes;self.sidebar_deltas.push_back((kind.into(),delta));
            }
        }
        let me=self.me();
        self.read_index.observe(kind,&value,&me);
        let known_channel=self.channels.contains_key(&s(&value,"id"));
        self.guild_load.observe(kind,&value,&me,known_channel);
        if matches!(kind, "MESSAGE_CREATE" | "MESSAGE_UPDATE" | "MESSAGE_DELETE") {
            let channel = s(&value, "channel_id");
            if self.features.log_is_purged(&channel, &s(&value, "id")) { return Ok(()); }
            self.observe_message_nickname(&value);
            if self.loading_channel == channel {
                let bytes = value.to_string().encode_utf16().count() * 2;
                if self.pending_deltas.len() >= 256
                    || self.pending_delta_bytes.saturating_add(bytes) > 1024 * 1024
                {
                    self.cancel_view_requests();
                    self.error("Demasiados cambios durante la carga. Vuelve a abrir el canal; los mensajes recibidos se conservan.".into());
                } else {
                    self.pending_deltas.push_back((kind.into(), value.clone()));
                    self.pending_delta_bytes += bytes;
                }
            }
            let cached_old = self.cache.find(&channel, &s(&value, "id"));
            let outside_window = cached_old.is_none() && matches!(self.state.view.as_str(),"pins"|"search");
            let old = cached_old.or_else(|| {
                // Pins/search can contain a fully received message outside the
                // small chat LRU. Its observed payload is still known locally.
                matches!(kind,"MESSAGE_UPDATE"|"MESSAGE_DELETE").then(|| {
                    self.state.pinned_messages.iter().chain(
                        self.state.messages.iter().filter(|_|matches!(self.state.view.as_str(),"chat"|"pins"|"search"))
                    ).find(|message|message.id==s(&value,"id")&&message.channel_id==channel&&!message.deleted).cloned()
                }).flatten()
            });
            if kind == "MESSAGE_UPDATE" || kind == "MESSAGE_DELETE" {
                if let Some(old) = &old {
                    let changed = !old.deleted && (kind == "MESSAGE_DELETE"
                        || value
                            .get("content")
                            .is_some_and(|v| v.as_str().is_some_and(|s| s != old.content)));
                    if changed {
                        if let Err(error) = self.features.record(old, kind == "MESSAGE_DELETE") {
                            self.error(error);
                        }
                    }
                }
            }
            let mut list = self.cache.get(&channel);
            if let Some(previous)=&old && !list.iter().any(|message|message.id==previous.id) {list.push(previous.clone());}
            apply_delta(&mut list, kind, &value);
            if kind == "MESSAGE_DELETE" {
                let id = s(&value, "id");
                for reply in &mut list {
                    if reply.reply_to.as_deref() == Some(&id)
                        && reply
                            .reply_preview
                            .as_ref()
                            .is_none_or(|p| p.channel_id == channel)
                    {
                        reply.reply_preview = Some(MessageReplyPreview {
                            message_id: id.clone(),
                            channel_id: channel.clone(),
                            deleted: true,
                            ..Default::default()
                        })
                    }
                }
            }
            let changed=list.iter().find(|message|message.id==s(&value,"id")).cloned();
            if outside_window && matches!(kind,"MESSAGE_UPDATE"|"MESSAGE_DELETE") {
                if let Some(changed)=&changed {for message in &mut self.state.messages {if message.id==changed.id&&message.channel_id==channel {*message=changed.clone();}}}
            } else {self.cache.replace(&channel, list);}
            let id = s(&value, "id");
            if kind == "MESSAGE_DELETE" {
                self.state
                    .pinned_messages
                    .retain(|m| m.id != id || m.channel_id != channel)
            } else if let Some(changed) = changed {
                for pinned in &mut self.state.pinned_messages {
                    if pinned.id == id && pinned.channel_id == channel {
                        *pinned = changed.clone()
                    }
                }
            }
            self.features.filter_purged(&mut self.state.pinned_messages);
            if self.state.view == "pins" && channel == self.state.selected_channel_id {
                self.state.messages = self.state.pinned_messages.clone().into();
            }
            self.features.filter_purged(&mut self.state.messages);
            if kind == "MESSAGE_UPDATE" {
                if let Some(pinned) = value.get("pinned").and_then(Value::as_bool) {
                    self.apply_message_pin(&channel, &id, pinned);
                }
            }
            if kind == "MESSAGE_CREATE" {
                let message = parsing::message(&value);
                self.remember(message.author.clone());
                let id = message.id.clone();
                if parsing::valid_id(&channel) && parsing::valid_id(&id) {
                    let duplicate = old.is_some()
                        || self.message_seen.iter().any(|(seen_channel, seen_id)| {
                            seen_channel == &channel && seen_id == &id
                        });
                    if !duplicate {
                        self.message_seen.push_back((channel.clone(), id.clone()));
                        while self.message_seen.len() > 4096 {
                            self.message_seen.pop_front();
                        }
                    }
                    let own = message.author.id == self.me();
                    self.observe_message_delivery(&channel, &id, own);
                    self.observe_message_preview(&message);
                    let already_read = self
                        .read
                        .get(&channel)
                        .is_some_and(|read| !snowflake_greater(&id, read));
                    let personal = !value
                        .get("guild_id")
                        .is_some_and(|v| v.is_string() && !v.as_str().unwrap_or("").is_empty());
                    if !duplicate && !own && !already_read {
                        let mentioned =
                            message.mentions.iter().any(|person| person.id == self.me())
                                || b(&value, "mention_everyone")
                                || self.guild_acl.as_ref().filter(|acl| acl.guild_id == s(&value,"guild_id"))
                                    .is_some_and(|acl| items(&value["mention_roles"]).iter()
                                        .filter_map(Value::as_str).any(|role| acl.has_role(role)));
                        let delivery = self.notification_delivery(&message, &value);
                        let sound_muted = self.state.self_presence == "dnd"
                            || self.notification_muted(&channel,&s(&value,"guild_id"));
                        if !sound_muted && delivery.notify {
                            let mut sound = json!({"event":if delivery.mention{"mention"}else{"message"},"channel_id":channel,"message_id":id,"account_id":self.me()});
                            if self.state.notification_epoch != 0 { sound["notification_epoch"] = json!(self.state.notification_epoch); }
                            if self.generation != 0 { sound["session_generation"] = json!(self.generation); }
                            self.data("sound", sound);
                        }
                        if personal || mentioned {
                            let count = self.mention.entry(channel.clone()).or_default();
                            *count = count.saturating_add(1);
                            self.unread_seen.push_back((channel.clone(), id.clone()));
                            while self.unread_seen.len() > 4096 {
                                self.unread_seen.pop_front();
                            }
                        }
                        if delivery.notify { self.notice_message(&message, &value, personal); }
                        if !personal {
                            for guild in &mut self.state.guilds {
                                if guild.id == s(&value, "guild_id") {
                                    guild.unread = true;
                                    if mentioned {
                                        guild.mention_count = guild.mention_count.saturating_add(1);
                                    }
                                }
                            }
                        }
                    }
                    if !self
                        .state
                        .direct_messages
                        .iter()
                        .any(|item| item.id == channel)
                        && personal
                    {
                        self.load_private_channel(&channel);
                    }
                }
            }
            self.refresh_messages();
            self.decorate_dms();
            if !self.state.selected_guild_id.is_empty() {
                self.rebuild_channels();
            }
            self.publish();
            return Ok(());
        }
        match kind {
            "PRESENCE_UPDATE" => {
                self.presence(&value);
                self.publish();
            }
            "SELF_PRESENCE" => {
                self.remote_presence(&s(&value, "status"));
                if let Some(show) = value["show_current_game"].as_bool() {
                    self.state.game_activity_enabled = show;
                    self.state.game_activity_available = true;
                    self.state.game_activity_configured = true;
                }
                self.publish();
            }
            "READY_CATALOG" => {
                self.sidebar_cancel.cancel();self.sidebar_cancel=tokio_util::sync::CancellationToken::new();
                self.sidebar_epoch=self.sidebar_epoch.wrapping_add(1);
                self.sidebar_seed=value;
                self.sidebar_deltas.clear();self.sidebar_delta_bytes=0;
                self.sidebar_inflight=false;
                self.sidebar();
            }
            "GUILD_CREATE"|"GUILD_UPDATE"|"GUILD_DELETE" => {
                let guild = s(&value, "id");
                self.invalidate_guild_editor(&guild);
                if kind=="GUILD_DELETE" {
                    self.emoji_cache.remove(&guild);
                    if guild==self.state.selected_guild_id {self.state.emojis.clear();}
                } else if value["emojis"].is_array() {
                    self.next_request(&format!("emoji-catalog/{guild}"));self.store_emoji_catalog(&guild,items(&value["emojis"]));
                }
                if kind=="GUILD_DELETE"{self.reset_guild_voice_snapshot(&guild);}
                if kind == "GUILD_DELETE" { self.thread_catalog.remove(&guild); }
                else if value["threads"].is_array() {
                    self.next_request(&format!("guild-threads/{guild}"));
                    let threads = super::channel_catalog::thread_response(&guild, &value, &self.me());
                    self.thread_catalog.replace(&guild, &threads);
                }
                if kind != "GUILD_DELETE" && guild == self.state.selected_guild_id {
                    for key in ["id","name","owner_id","roles","premium_tier"] {
                        if let Some(field)=value.get(key) {
                            if key!="roles"||field.is_array() {self.guild_raw[key]=field.clone();}
                        }
                    }
                    if !self.self_member.is_null() {
                        self.guild_acl=Some(GuildAccess::from_values(&self.guild_raw,&self.self_member,&self.me()));
                    }
                    for channel in items(&value["channels"]).iter().take(2000) {
                        let id = s(channel, "id");
                        if parsing::valid_id(&id) && (self.channels.len() < 2000 || self.channels.contains_key(&id)) {
                            self.channels.insert(id, channel.clone());
                        }
                    }
                    if value["threads"].is_array() {
                        if let Some(threads) = self.thread_catalog.get(&guild) {
                            for thread in threads {
                                let id = s(&thread, "id");
                                if self.channels.len() < 2000 || self.channels.contains_key(&id) { self.channels.insert(id, thread); }
                            }
                        }
                    }
                    self.rebuild_channels(); self.revoke_access();
                }
                if kind != "GUILD_DELETE" { self.observe_guild_voice_metadata(&value); }
                if kind=="GUILD_DELETE"&&b(&value,"unavailable") {
                    if s(&value,"id")==self.state.selected_guild_id{
                        self.state.channel_access=ChannelAccess::default();self.member_demand=None;self.queue_member_subscription();
                        self.state.status="El servidor no está disponible temporalmente.".into();
                    }
                    self.publish();return Ok(());
                }
                if self.sidebar_seed["guilds"].is_array(){self.state.guilds=self.sidebar_guilds(&self.sidebar_seed["guilds"]);}
                if kind=="GUILD_DELETE"&&s(&value,"id")==self.state.selected_guild_id{self.select_guild("")?;}
                self.publish();
            }
            "READY_METADATA" => {
                self.load_local_notification_views();
                self.observe_guild_voice_metadata(&value);
                if !s(&value, "self_status").is_empty() {
                    self.remote_presence(&s(&value, "self_status"));
                }
                for p in items(&value["presences"]).iter().take(2000) {
                    self.presence(p);
                }
                for setting in items(&value["settings"]).iter().take(500) {
                    self.notification_settings(setting);
                }
                for read in items(&value["read_states"]).iter().take(2000) {
                    self.read
                        .insert(s(read, "channel_id"), s(read, "last_message_id"));
                    self.mention
                        .insert(s(read, "channel_id"), i(read, "mention_count"));
                }
                self.decorate_dms();
                self.publish();
                self.subscribe().await;
            }
            "MESSAGE_ACK" => {
                let channel = s(&value, "channel_id");
                let last = s(&value, "message_id");
                self.mark_read_through(&channel, &last);
                self.decorate_dms();
                if !self.state.selected_guild_id.is_empty() {
                    self.rebuild_channels();
                }
                self.publish();
            }
            "USER_GUILD_SETTINGS_UPDATE" => {
                self.notification_settings(&value);
                self.decorate_dms();
                self.publish();
            }
            "TYPING_START" => {
                if s(&value, "channel_id") == self.state.selected_channel_id
                    && s(&value, "user_id") != self.me()
                {
                    if value["member"]["user"].is_object() {
                        self.remember(parsing::person(&value["member"]["user"]));
                    }
                    if self.typing.len() < 32 {
                        self.typing.insert(s(&value, "user_id"), Instant::now());
                    }
                    self.refresh_typing();
                    self.publish();
                }
            }
            "GUILD_EMOJIS_UPDATE" => {
                let guild=s(&value,"guild_id");
                if self.state.guilds.iter().any(|item|item.id==guild) && value["emojis"].is_array() {
                    self.next_request(&format!("emoji-catalog/{guild}"));let list=self.store_emoji_catalog(&guild,items(&value["emojis"]));
                    if guild==self.state.selected_guild_id {self.data("guild-emojis",serde_json::to_value(list).unwrap());self.publish();}
                }
            }
            "USER_UPDATE" => {
                let p = parsing::person(&value);
                self.remember(p.clone());
                if p.id == self.me() {
                    self.state.current_user = Some(p);
                    if let Some(entitlement)=value.get("premium_type") {
                        let premium=entitlement.as_i64().filter(|premium|(0..=3).contains(premium)).and_then(|premium|i32::try_from(premium).ok());
                        self.premium=premium.unwrap_or(0);self.state.upload_limit.premium_type=premium;self.refresh_emoji_access();
                    }
                }
                self.decorate_dms();
                self.publish();
            }
            "THREAD_LIST_SYNC" => {
                let guild = s(&value, "guild_id");
                if !self.state.guilds.iter().any(|item| item.id == guild) { return Ok(()); }
                self.next_request(&format!("guild-threads/{guild}"));
                let parents = value.get("channel_ids").and_then(Value::as_array);
                let in_scope = |thread: &Value| parents.is_none_or(|parents| parents.iter().any(|id| id.as_str() == Some(s(thread, "parent_id").as_str())));
                let cached = self.thread_catalog.get(&guild);
                let complete = parents.is_none() || cached.is_some();
                let mut threads = cached.unwrap_or_default();
                threads.retain(|thread| !in_scope(thread));
                threads.extend(super::channel_catalog::thread_response(&guild, &value, &self.me()));
                if complete { self.thread_catalog.replace(&guild, &threads); }
                if guild == self.state.selected_guild_id {
                    self.channels.retain(|_, channel| !matches!(i(channel, "type"), 10..=12) || !in_scope(channel));
                    for thread in threads.into_iter().take(2000) {
                        if self.channels.len() < 2000 || self.channels.contains_key(&s(&thread, "id")) { self.channels.insert(s(&thread, "id"), thread); }
                    }
                    self.rebuild_channels(); self.revoke_access(); self.publish();
                }
            }
            "THREAD_MEMBER_UPDATE" | "THREAD_MEMBERS_UPDATE" => {
                let guild = s(&value, "guild_id");
                let id = s(&value, "id");
                let me = self.me().to_owned();
                let removed = items(&value["removed_member_ids"]).iter().take(2000).any(|id| id.as_str() == Some(me.as_str()));
                let member = if kind == "THREAD_MEMBER_UPDATE" {
                    (s(&value, "user_id").is_empty() || s(&value, "user_id") == me).then_some(&value)
                } else {
                    items(&value["added_members"]).iter().take(2000).find(|member| s(member, "user_id") == me)
                };
                if !removed && member.is_none() { return Ok(()); }
                let patch = json!({"id":id,"guild_id":guild,"member":if removed { Value::Null } else { member.cloned().unwrap_or(Value::Null) }});
                self.next_request(&format!("guild-threads/{guild}"));
                self.thread_catalog.delta(&guild, &patch, false);
                if guild == self.state.selected_guild_id {
                    if let Some(previous) = self.channels.get(&id) {
                        if matches!(i(previous, "type"), 10..=12) {
                            let merged = merge_object(previous, &patch);
                            self.channels.insert(id, merged);
                            self.rebuild_channels(); self.revoke_access(); self.publish();
                        }
                    }
                }
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "THREAD_CREATE" | "THREAD_UPDATE" => {
                let id = s(&value, "id");
                self.invalidate_guild_editor(&s(&value,"guild_id"));
                if kind.starts_with("THREAD_") {
                    let guild = s(&value, "guild_id");
                    self.next_request(&format!("guild-threads/{guild}"));
                    self.thread_catalog.delta(&guild, &value, false);
                }
                if (s(&value, "guild_id") == self.state.selected_guild_id
                    || s(&value, "guild_id").is_empty() && self.channels.contains_key(&id))
                    && !self.state.selected_guild_id.is_empty()
                {
                    let previous = self.channels.get(&id).cloned().unwrap_or(Value::Null);
                    let merged = merge_object(&previous, &value);
                    let selected = id == self.state.selected_channel_id
                        || self
                            .channels
                            .get(&self.state.selected_channel_id)
                            .is_some_and(|c| s(c, "parent_id") == id);
                    if selected
                        && (value.get("permission_overwrites").is_some()
                            || value.get("member_list_id").is_some())
                    {
                        self.invalidate_member_context();
                    }
                    if self.channels.len() < 2000 || self.channels.contains_key(&id) { self.channels.insert(id.clone(), merged); }
                    self.rebuild_channels();
                    self.revoke_access();
                    if selected {
                        self.subscribe().await;
                    }
                } else if matches!(i(&value, "type"), 1 | 3) {
                    self.recipients(&value);
                    let dm = parsing::channel(&value);
                    self.state.direct_messages.retain(|n| n.id != id);
                    self.state.direct_messages.push(dm);
                    if self.state.selected_channel_id == id {
                        self.refresh_group(&value);
                    }
                }
                self.decorate_dms();
                self.publish();
            }
            "CHANNEL_DELETE" | "THREAD_DELETE" => {
                let id = s(&value, "id");
                if kind == "THREAD_DELETE" {
                    let guild = s(&value, "guild_id");
                    self.next_request(&format!("guild-threads/{guild}"));
                    self.thread_catalog.delta(&guild, &value, true);
                }
                self.channels.remove(&id);
                self.latest.remove(&id);
                self.incoming_latest.remove(&id);
                self.state.channels.retain(|c| c.id != id);
                self.state.direct_messages.retain(|c| c.id != id);
                if self.state.selected_channel_id == id {
                    if !self.state.selected_guild_id.is_empty() {
                        self.recover_guild_navigation()?;
                    } else {
                        self.reset_navigation();
                        self.state.selected_channel_id.clear();
                        self.state.messages.clear();
                        self.state.view = "friends".into();
                    }
                }
                self.publish();
            }
            "CHANNEL_RECIPIENT_ADD" | "CHANNEL_RECIPIENT_REMOVE" => {
                self.observe_private_recipient(kind,&value);
                let id = s(&value, "channel_id");
                if id == self.state.selected_channel_id {
                    let api = self.api()?;
                    let done=self.group_refresh_action(&id);
                    self.launch(done, Some(self.nav), async move {
                        api.get(&format!("channels/{id}"))
                            .await
                            .map_err(|e| e.to_string())
                    });
                }
            }
            "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" => {
                let relation = parsing::relationship(&value);
                self.remember(relation.user.clone());
                self.state
                    .relationships
                    .retain(|r| r.user.id != relation.user.id);
                self.state.relationships.push(relation);
                self.publish();
            }
            "RELATIONSHIP_REMOVE" => {
                let id = s(&value, "id");
                self.state.relationships.retain(|r| r.user.id != id);
                self.publish();
            }
            "MESSAGE_REACTION_ADD"
            | "MESSAGE_REACTION_REMOVE"
            | "MESSAGE_REACTION_REMOVE_ALL"
            | "MESSAGE_REACTION_REMOVE_EMOJI" => {
                self.invalidate_reaction_users(kind, &value);
                self.reaction_event(kind, &value);
                self.refresh_messages();
                self.publish();
            }
            "GUILD_MEMBER_LIST_UPDATE" => {
                if self.member_demand.is_some()&&self.state.channel_access.can_view_members{self.member_list(&value);self.publish();}
            }
            "GUILD_MEMBER_REMOVE" => {
                let guild=s(&value,"guild_id");let user=s(&value["user"],"id");
                self.state.guild_voice_members.retain(|member|member.guild_id!=guild||member.user.id!=user);
                if guild==self.state.selected_guild_id{
                    self.state.guild_nicknames.remove(&user);
                    self.state.guild_members.retain(|member|member.user.id!=user);
                    for member in &mut self.guild_member_slots{if member.as_ref().is_some_and(|member|member.user.id==user){*member=None;}}
                }
                self.invalidate_guild_editor(&guild);self.publish();
            }
            "GUILD_MEMBER_UPDATE" => {
                let guild = s(&value, "guild_id");
                let user = s(&value["user"], "id");
                if user==self.me() && (value.get("roles").is_some()||value.get("communication_disabled_until").is_some()){self.invalidate_guild_editor(&guild);}
                self.remember_guild_nickname(&guild, &user, &value);
                if guild == self.state.selected_guild_id {
                    let id = s(&value["user"], "id");
                    if id == self.me() {
                        if value.get("roles").is_some() || value.get("communication_disabled_until").is_some() {
                            self.invalidate_member_context();
                        }
                        for key in ["roles", "communication_disabled_until", "nick"] {
                            if let Some(v) = value.get(key) {
                                self.self_member[key] = v.clone();
                            }
                        }
                        self.guild_acl = Some(GuildAccess::from_values(
                            &self.guild_raw,
                            &self.self_member,
                            &self.me(),
                        ));
                        self.rebuild_channels();
                        self.revoke_access();
                    }
                    for slot in &mut self.guild_member_slots {
                        if let Some(m) = slot {
                            if m.user.id == id {
                                if value.get("nick").is_some() {
                                    m.nickname = s(&value, "nick");
                                }
                                update_member_roles(m,&value,&self.state.guild_roles);
                            }
                        }
                    }
                    for member in &mut self.state.guild_members {
                        if member.user.id==id {update_member_roles(member,&value,&self.state.guild_roles);}
                    }
                    self.publish();
                }
            }
            "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" | "GUILD_ROLE_DELETE" => {
                self.invalidate_guild_editor(&s(&value,"guild_id"));
                if s(&value, "guild_id") == self.state.selected_guild_id {
                    self.invalidate_member_context();
                    let role = value.get("role").unwrap_or(&Value::Null);
                    let id = if role.is_null() {
                        s(&value, "role_id")
                    } else {
                        s(role, "id")
                    };
                    if let Some(roles) = self.guild_raw["roles"].as_array_mut() {
                        roles.retain(|r| s(r, "id") != id);
                        if !role.is_null() {
                            roles.push(role.clone());
                        }
                    }
                    self.guild_acl = Some(GuildAccess::from_values(
                        &self.guild_raw,
                        &self.self_member,
                        &self.me(),
                    ));
                    self.rebuild_channels();
                    self.revoke_access();
                    self.publish();
                }
            }
            "CALL_CREATE" | "CALL_UPDATE" | "CALL_DELETE" => {
                self.incoming_call(kind, &value);
                self.voice_call_dispatch(kind, &value).await?;
                self.decorate_dms();
                self.publish();
            }
            "VOICE_STATE_UPDATE"
            | "VOICE_SERVER_UPDATE"
            | "STREAM_CREATE"
            | "STREAM_UPDATE"
            | "STREAM_SERVER_UPDATE"
            | "STREAM_DELETE" => self.voice_dispatch(kind, &value).await?,
            "USER_SETTINGS_UPDATE" => {
                if !s(&value, "status").is_empty() {
                    self.remote_presence(&s(&value, "status"));
                }
                self.publish();
            }
            "USER_SETTINGS_PROTO_UPDATE" => {
                if i(&value["settings"],"type")==2{self.gif_settings_update(&value);}
                if i(&value["settings"], "type") == 1 {
                    if let Ok(p) = notdiscord_core::settings_proto::read(
                        &s(&value["settings"], "proto"),
                        !b(&value, "partial"),
                    ) {
                        if let Some(show) = p.show_current_game {
                            self.state.game_activity_enabled = show;
                            self.state.game_activity_available = true;
                            self.state.game_activity_configured = true;
                        }
                        self.publish();
                    }
                }
            }
            "__incoming-dm" => (),
            _ => (),
        }
        Ok(())
    }
    pub(super) fn decorate_dms(&mut self) {
        let me = self.me();
        for n in &mut self.state.direct_messages {
            n.is_pinned = self.features.pins.contains(&n.id);
            n.is_muted = self.muted.get(&n.id).copied().unwrap_or(false);
            n.mention_count = self.mention.get(&n.id).copied().unwrap_or(0);
            let latest = self
                .latest
                .get(&n.id)
                .cloned()
                .unwrap_or_else(|| n.last_message_id.clone());
            n.unread = n.mention_count > 0
                || self.read.get(&n.id).is_some_and(|read| {
                    let incoming = self.incoming_latest.get(&n.id).unwrap_or(&latest);
                    snowflake_greater(incoming, read)
                });
            if !latest.is_empty() {
                n.last_message_id = latest;
            }
            if let Some(p) = self.people.get(&n.recipient_id) {
                n.name = p.name.clone();
                n.icon_url = p.avatar_url.clone();
            }
            if let Some(p) = self.state.user_presences.get(&n.recipient_id) {
                n.presence = p.status.clone();
                n.custom_status = p.custom_status.clone();
                n.custom_status_emoji = p.custom_status_emoji.clone();
                n.activities = p.activities.clone();
            }
            n.has_active_call = self
                .state
                .incoming_calls
                .iter()
                .any(|c| c.channel_id == n.id)
                || self.voice_channel == n.id;
            n.incoming_call_ringing = self
                .state
                .incoming_calls
                .iter()
                .any(|c| c.channel_id == n.id && c.ringing);
            let _ = &me;
        }
        let nav_mutes = self
            .state
            .direct_messages
            .iter()
            .map(|item| (item.id.clone(), item.is_muted))
            .collect::<HashMap<_, _>>();
        for call in &mut self.state.incoming_calls {
            call.muted = self
                .muted
                .get(&call.channel_id)
                .copied()
                .or_else(|| nav_mutes.get(&call.channel_id).copied())
                .unwrap_or(false);
        }
        parsing::order_dms(&mut self.state.direct_messages);
        self.state.unread_message_count = self
            .state
            .direct_messages
            .iter()
            .filter(|n| n.unread)
            .map(|n| n.mention_count.max(1))
            .sum();
    }
    /// Keep delivery order separate from the last message that can be unread.
    /// An outgoing delivery must not advance the read watermark: earlier
    /// incoming messages can still be waiting above the user's scroll position.
    pub(super) fn observe_message_delivery(&mut self, channel: &str, id: &str, own: bool) {
        if !parsing::valid_id(channel) || !parsing::valid_id(id) {
            return;
        }
        if own && !self.incoming_latest.contains_key(channel) {
            let previous = self
                .latest
                .get(channel)
                .cloned()
                .or_else(|| {
                    self.state
                        .direct_messages
                        .iter()
                        .chain(&self.state.channels)
                        .find(|item| item.id == channel)
                        .map(|item| item.last_message_id.clone())
                        .filter(|last| parsing::valid_id(last))
                })
                .or_else(|| {
                    self.channels
                        .get(channel)
                        .map(|raw| s(raw, "last_message_id"))
                })
                .filter(|last| parsing::valid_id(last))
                .or_else(|| self.read.get(channel).cloned())
                .unwrap_or_default();
            self.incoming_latest.insert(channel.into(), previous);
        } else if !own {
            if let Some(incoming) = self.incoming_latest.get_mut(channel) {
                if snowflake_greater(id, incoming) {
                    *incoming = id.into();
                }
            }
        }
        if self
            .latest
            .get(channel)
            .is_none_or(|last| snowflake_greater(id, last))
        {
            self.latest.insert(channel.into(), id.into());
        }
        if self.latest.len() > 2000 {
            if let Some(key) = self
                .latest
                .keys()
                .find(|key| key.as_str() != channel)
                .cloned()
            {
                self.latest.remove(&key);
            }
        }
        // Sidebar catalogues are bounded (500 private and 2000 guild channels).
        // Retain their classification; discard only inactive-channel metadata
        // if a session visits thousands of other channels.
        while self.incoming_latest.len() > 4096 {
            let expired = self
                .incoming_latest
                .keys()
                .find(|key| {
                    key.as_str() != channel
                        && !self
                            .state
                            .direct_messages
                            .iter()
                            .any(|item| item.id == **key)
                        && !self.channels.contains_key(*key)
                })
                .cloned();
            let Some(expired) = expired else { break };
            self.incoming_latest.remove(&expired);
        }
    }
    pub(super) fn incoming_message_id<'a>(&'a self, channel: &str, fallback: &'a str) -> &'a str {
        self.incoming_latest
            .get(channel)
            .map(String::as_str)
            .unwrap_or(fallback)
    }
    pub(super) fn observe_message_preview(&mut self, message: &ChatMessage) {
        if self.features.log_is_purged(&message.channel_id, &message.id) { return; }
        if self
            .latest
            .get(&message.channel_id)
            .is_some_and(|last| last != &message.id)
        {
            return;
        }
        let preview = parsing::message_preview(message);
        for item in self
            .state
            .direct_messages
            .iter_mut()
            .chain(&mut self.state.channels)
        {
            if item.id == message.channel_id {
                item.message_preview = Some(preview.clone());
            }
        }
    }
    /// Apply only the confirmed watermark. A delayed ACK can never clear a newer message.
    pub(super) fn mark_read_through(&mut self, channel: &str, last: &str) -> bool {
        if !parsing::valid_id(channel) || !parsing::valid_id(last) {
            return false;
        }
        let advanced = self
            .read
            .get(channel)
            .is_none_or(|previous| snowflake_greater(last, previous));
        if advanced {
            let mentions_before = self.mention.get(channel).copied().unwrap_or(0).max(0);
            self.read.insert(channel.into(), last.into());
            let mut removed = 0i32;
            self.unread_seen.retain(|(seen_channel, id)| {
                let read = seen_channel == channel && !snowflake_greater(id, last);
                if read {
                    removed = removed.saturating_add(1);
                }
                !read
            });
            let newest = [self.incoming_latest.get(channel).cloned(),self.latest.get(channel).cloned(),
                self.state.direct_messages.iter().chain(&self.state.channels).find(|item|item.id==channel).map(|item|item.last_message_id.clone()),
                self.read_index.latest(channel).map(str::to_owned)]
                .into_iter().flatten().filter(|id|parsing::valid_id(id)).max_by(|a,b|a.len().cmp(&b.len()).then(a.cmp(b)));
            if newest
                .as_ref()
                .is_some_and(|newest| !snowflake_greater(newest, last))
            {
                self.mention.remove(channel);
            } else if let Some(count) = self.mention.get_mut(channel) {
                *count = count.saturating_sub(removed).max(0);
            }
            let mentions_after = self.mention.get(channel).copied().unwrap_or(0).max(0);
            self.refresh_read_guild(channel, mentions_before.saturating_sub(mentions_after));
        }
        self.data("notice-read", json!({"channelId":channel,"messageId":last}));
        advanced
    }
    fn refresh_read_guild(&mut self, channel: &str, removed_mentions: i32) {
        if self.state.direct_messages.iter().any(|item|item.id==channel){return;}
        let guild = self
            .channels
            .get(channel)
            .map(|raw| s(raw, "guild_id"))
            .filter(|id| !id.is_empty())
            .or_else(|| {
                self.state
                    .channels
                    .iter()
                    .any(|item| item.id == channel)
                    .then(|| self.state.selected_guild_id.clone())
            })
            .or_else(||self.read_index.guild_for(channel))
            .unwrap_or_default();
        if guild.is_empty() {
            return;
        }
        // Only the selected guild has a complete channel catalogue. Unknown or
        // archived channels must not be cleared on the strength of another read.
        let complete = guild == self.state.selected_guild_id
            && s(&self.guild_raw, "id") == guild
            && self.guild_acl.is_some()
            && self.channels.len() < 2000;
        let mut known = complete;
        let mut any_unread = false;
        let mut readable_channels = 0usize;
        if complete {
            for (id, raw) in &self.channels {
                if i(raw, "type") == 4 {
                    continue;
                }
                let access = self.channel_access(raw);
                if !access.known {
                    known = false;
                }
                if !access.can_view {
                    continue;
                }
                readable_channels += 1;
                any_unread |= self.mention.get(id).is_some_and(|count| *count > 0);
                let newest = self
                    .latest
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| s(raw, "last_message_id"));
                if parsing::valid_id(&newest) {
                    if let Some(read) = self.read.get(id).filter(|read| parsing::valid_id(read)) {
                        any_unread |=
                            snowflake_greater(self.incoming_message_id(id, &newest), read);
                    } else {
                        known = false;
                    }
                } else if raw.get("last_message_id").is_none() {
                    known = false;
                }
            }
        }
        for item in &mut self.state.guilds {
            if item.id != guild {
                continue;
            }
            item.mention_count = item.mention_count.saturating_sub(removed_mentions).max(0);
            if any_unread || item.mention_count > 0 {
                item.unread = true;
            } else if known && readable_channels > 0 || self.read_index.unread(&guild,&self.read,&self.mention,&self.incoming_latest)==Some(false) {
                item.unread = false;
            }
        }
    }
    /// A UI report is authoritative only for a real visible message in the selected chat.
    pub(super) fn report_chat_visibility(&mut self, action: &UiAction) -> Result<(), String> {
        if action.text != "reading" {
            if action.channel_id.is_empty() || self.reading_channel == action.channel_id {
                self.reading_channel.clear();
            }
            return Ok(());
        }
        let channel = &action.channel_id;
        let last = &action.id;
        if !self.state.signed_in
            || self.state.view != "chat"
            || self.state.selected_channel_id != *channel
            || !self.state.channel_access.can_view
            || !self.state.channel_access.can_read_history
            || !parsing::valid_id(last)
            || !self
                .state
                .messages
                .iter()
                .any(|message| message.channel_id == *channel && message.id == *last)
        {
            return Ok(());
        }
        self.reading_channel = if self
            .latest
            .get(channel)
            .is_some_and(|newest| snowflake_greater(newest, last))
        {
            String::new()
        } else {
            channel.clone()
        };
        let advanced = self.mark_read_through(channel, last);
        self.decorate_dms();
        if !self.state.selected_guild_id.is_empty() {
            self.rebuild_channels();
        }
        self.publish();
        if advanced {
            if self.api.is_some(){self.read_acks.advance(channel,last,Instant::now());}
        }
        Ok(())
    }
    fn presence(&mut self, value: &Value) {
        let id = if value["user"].is_object() {
            s(&value["user"], "id")
        } else {
            s(value, "user_id")
        };
        if id.is_empty() {
            return;
        }
        if value["user"].get("username").is_some() {
            self.remember(parsing::person(&value["user"]));
        }
        let mut p = parsing::presence(value);
        if let Some(previous)=self.state.user_presences.get(&id) {
            if value.get("status").is_none(){p.status=previous.status.clone();}
            if value.get("activities").is_none(){
                p.activities=previous.activities.clone();p.custom_status=previous.custom_status.clone();p.custom_status_emoji=previous.custom_status_emoji.clone();
            }
        }
        if id==self.me() && notdiscord_core::settings_proto::valid_presence(&self.state.self_presence){p.status=self.state.self_presence.clone();}
        if self.state.user_presences.len() >= 2000 && !self.state.user_presences.contains_key(&id) {
            if let Some(key) = self.state.user_presences.keys().next().cloned() {
                self.state.user_presences.remove(&key);
            }
        }
        self.state.user_presences.insert(id.clone(), p.clone());
        self.state.presence_version += 1;
        for member in self
            .state
            .group_members
            .iter_mut()
            .chain(&mut self.state.guild_members)
        {
            if member.user.id == id {
                member.presence = p.status.clone();
                member.custom_status = p.custom_status.clone();
                member.custom_status_emoji = p.custom_status_emoji.clone();
                member.activities = p.activities.clone();
            }
        }
        if let Some(profile) = self
            .state
            .profile_details
            .as_mut()
            .filter(|d| d.user.id == id)
        {
            profile.presence = p.status;
            profile.custom_status = p.custom_status;
            profile.custom_status_emoji = p.custom_status_emoji;
            profile.activities = p.activities;
        }
        self.decorate_dms();
    }
    fn notification_settings(&mut self, value: &Value) {
        self.notification_settings.apply(value);
        for c in items(&value["channel_overrides"]).iter().take(2000) {
            let id = s(c, "channel_id");
            if parsing::valid_id(&id) && (self.muted.len() < 2000 || self.muted.contains_key(&id)) {
                if let Some(muted) = c["muted"].as_bool() { self.muted.insert(id, muted); }
            }
        }
        if let Some(overrides) = value["channel_overrides"].as_object() {
            for (id,setting) in overrides.iter().take(2000) {
                if parsing::valid_id(id) && (self.muted.len()<2000 || self.muted.contains_key(id)) {
                    if let Some(muted)=setting["muted"].as_bool() { self.muted.insert(id.clone(),muted); }
                }
            }
        }
        let guild = s(value, "guild_id");
        if !guild.is_empty() {
            for g in &mut self.state.guilds {
                if g.id == guild {
                    if let Some(muted) = value["muted"].as_bool() { g.is_muted = muted; }
                }
            }
        }
        self.refresh_notification_nav();
        if !self.state.selected_guild_id.is_empty() { self.rebuild_channels(); }
    }
    pub(super) fn refresh_notification_nav(&mut self) {
        for guild in &mut self.state.guilds {
            guild.guild_notifications = self.notification_settings.get(&guild.id);
            if let Some(muted) = guild.guild_notifications.muted { guild.is_muted = muted; }
        }
    }
    pub(super) fn load_local_notification_views(&mut self) {
        let account = self.me();
        if !parsing::valid_id(&account) || self.notification_settings.local_account_loaded == account { return; }
        let hidden: Vec<String> = storage::read_json(&self.data.join("notification-views").join(format!("{account}.json")), 32768);
        for guild in hidden.into_iter().take(500) { let _ = self.notification_settings.set_hidden(&guild, true); }
        self.notification_settings.local_account_loaded = account;
        self.refresh_notification_nav();
    }
    fn message_mention_flags(&self, message: &ChatMessage, value: &Value) -> notdiscord_core::notifications::MentionFlags {
        notdiscord_core::notifications::MentionFlags {
            direct: if message.forwarded { items(&value["mentions"]).iter().any(|person|s(person,"id")==self.me()) }
                else { message.mentions.iter().any(|person| person.id == self.me()) },
            everyone: b(value,"mention_everyone"),
            role: self.guild_acl.as_ref().filter(|acl|acl.guild_id==s(value,"guild_id"))
                .is_some_and(|acl| items(&value["mention_roles"]).iter().filter_map(Value::as_str).any(|role|acl.has_role(role))),
        }
    }
    fn notification_delivery(&self, message: &ChatMessage, value: &Value) -> notdiscord_core::notifications::Delivery {
        let guild = s(value,"guild_id");
        let parent = self.channels.get(&message.channel_id).map(|channel|s(channel,"parent_id"));
        self.notification_settings.delivery(&guild,&message.channel_id,parent.as_deref(),self.message_mention_flags(message,value))
    }
    pub(super) fn notification_muted(&self, channel: &str, guild: &str) -> bool {
        if self.notification_settings.get(guild).muted.unwrap_or_else(||self.state.guilds.iter().any(|item|item.id==guild&&item.is_muted)) { return true; }
        let mut current=channel.to_owned();
        for _ in 0..16 {
            if self.muted.get(&current).copied().unwrap_or(false) { return true; }
            let Some(parent)=self.channels.get(&current).map(|channel|s(channel,"parent_id")).filter(|id|!id.is_empty()) else {break};
            current=parent;
        }
        false
    }
    pub(super) fn refresh_typing(&mut self) {
        let names = self
            .typing
            .keys()
            .take(3)
            .map(|id| {
                self.people
                    .get(id)
                    .map(|p| p.name.as_str())
                    .unwrap_or("Alguien")
            })
            .collect::<Vec<_>>();
        self.state.typing_text = if names.is_empty() {
            String::new()
        } else {
            format!(
                "{} {}",
                names.join(", "),
                if names.len() == 1 {
                    "está escribiendo…"
                } else {
                    "están escribiendo…"
                }
            )
        };
    }
    fn notice_message(&mut self, message: &ChatMessage, value: &Value, personal: bool) {
        if !self.notification_delivery(message,value).notify || self.notice_seen.contains(&message.id)
            || self.state.self_presence == "dnd"
            || self.notification_muted(&message.channel_id,&s(value,"guild_id"))
        {
            return;
        }
        self.notice_seen.push_back(message.id.clone());
        while self.notice_seen.len() > 1024 {
            self.notice_seen.pop_front();
        }
        let notice = self.message_notice(message, value, personal);
        self.queue_notice(notice);
    }
    fn message_notice(
        &self,
        message: &ChatMessage,
        value: &Value,
        personal: bool,
    ) -> DesktopNotice {
        let source = self
            .state
            .direct_messages
            .iter()
            .chain(&self.state.channels)
            .find(|n| n.id == message.channel_id);
        let sender = if message.author.name.trim().is_empty() {
            if message.author.username.trim().is_empty() {
                "Usuario"
            } else {
                &message.author.username
            }
        } else {
            &message.author.name
        };
        let guild_id = s(value, "guild_id");
        let guild = self.state.guilds.iter().find(|guild| guild.id == guild_id);
        let conversation = source
            .map(|item| item.name.as_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(if personal { "Mensaje directo" } else { "Canal" });
        let body = if message.content.trim().is_empty() || message.forwarded || !message.stickers.is_empty(){message_notice_body(message)}else{
            let mut names=crate::notification_text::Names::default();
            for person in &message.mentions{
                let name=if !person.name.trim().is_empty(){&person.name}else if !person.display_name.trim().is_empty(){&person.display_name}else{&person.username};
                names.users.insert(person.id.clone(),name.clone());
            }
            // A nickname cache is scoped to the selected guild. Only use it
            // when it is also the origin guild of this particular toast.
            if !guild_id.is_empty() && guild_id==self.state.selected_guild_id{
                for (id,nick) in &self.state.guild_nicknames{if !nick.trim().is_empty(){names.users.insert(id.clone(),nick.clone());}}
                for role in &self.state.guild_roles{names.roles.insert(role.id.clone(),role.name.clone());}
                for channel in &self.state.channels{names.channels.insert(channel.id.clone(),channel.name.trim_start_matches('#').into());}
            }
            for person in items(&value["mentions"]).iter().take(100){
                let nick=person.get("member").and_then(|member|member.get("nick")).or_else(||person.get("nick"));
                if let Some(nick)=nick{
                    let id=s(person,"id");
                    if let Some(nick)=nick.as_str().filter(|nick|!nick.trim().is_empty()){names.users.insert(id,nick.into());}
                    else if let Some(explicit)=message.mentions.iter().find(|mention|mention.id==id){
                        let name=if !explicit.name.trim().is_empty(){&explicit.name}else{&explicit.username};
                        names.users.insert(id,name.clone());
                    }else{names.users.remove(&id);}
                }
            }
            crate::notification_text::readable(&message.content,&names).chars().take(50).collect()
        };
        let title = if !personal {
            format!(
                "{sender} · {} / #{}",
                guild
                    .map(|item| item.name.as_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or("Servidor"),
                conversation.trim_start_matches('#')
            )
        } else if conversation == sender {
            sender.into()
        } else {
            format!("{sender} · {conversation}")
        };
        let avatar_url = if !personal {
            guild.and_then(|guild| guild.icon_url.clone())
        } else if source.is_some_and(|source| source.kind == "group") {
            source.and_then(|source| source.icon_url.clone())
        } else {
            message
                .author
                .avatar_url
                .clone()
                .or_else(|| source.and_then(|source| source.icon_url.clone()))
        };
        DesktopNotice {
            channel_id: message.channel_id.clone(),
            guild_id,
            message_id: message.id.clone(),
            title,
            body,
            is_call: false,
            avatar_url,
            image_path: None,
            notification_mentions: Some(self.message_mention_flags(message,value)),
        }
    }
    fn queue_notice(&mut self, mut notice: DesktopNotice) {
        notdiscord_core::diagnostics::record(notdiscord_core::diagnostics::Component::Notifications,notdiscord_core::diagnostics::Code::ToastQueued,notdiscord_core::diagnostics::Phase::Enqueue,0);
        let account = self.me();
        let generation = self.generation;
        let epoch = self.state.notification_epoch;
        // Fixtures/offline state never start downloads. Two live preparations
        // are allowed; bursts fall back to the registered application image.
        static IMAGE_SLOTS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
            std::sync::OnceLock::new();
        let permit = IMAGE_SLOTS
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
            .clone()
            .try_acquire_owned();
        if self.api.is_none() || permit.is_err() {
            self.data("notice", notice_with_epoch(&notice, &account, generation, epoch));
            return;
        }
        let permit = permit.unwrap();
        let root = self.data.join("notification-images");
        let action = UiAction {
            kind: "__notice-image-ready".into(),
            id: notice.message_id.clone(),
            channel_id: notice.channel_id.clone(),
            ..Default::default()
        };
        self.launch(action, None, async move {
            let fallback = notice.clone();
            let notice = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                notice.image_path = crate::notification_identity::prepare_toast_image(
                    &root,
                    notice.avatar_url.as_deref(),
                )
                .map(|path| path.to_string_lossy().into_owned());
                notice
            })
            .await
            .unwrap_or(fallback);
            Ok(notice_with_epoch(&notice, &account, generation, epoch))
        });
    }
    pub(super) fn complete_notice(
        &mut self,
        action: &UiAction,
        value: Value,
    ) -> Result<(), String> {
        if value["accountId"].as_str() != Some(self.me().as_str())
            || value["generation"].as_u64() != Some(self.generation)
            || value["notificationEpoch"].as_u64().unwrap_or(0) != self.state.notification_epoch
        {
            return Ok(());
        }
        let notice: DesktopNotice =
            serde_json::from_value(value.clone()).map_err(|_| "Aviso no válido.")?;
        if !notice.is_call && !notice.guild_id.is_empty() {
            let settings = self.notification_settings.get(&notice.guild_id);
            let parent = self.channels.get(&notice.channel_id).map(|channel|s(channel,"parent_id"));
            let permitted = notice.notification_mentions.map(|flags|
                self.notification_settings.delivery(&notice.guild_id,&notice.channel_id,parent.as_deref(),flags).notify)
                .unwrap_or(settings.message_notifications!=Some(2)&&!settings.suppress_everyone&&!settings.suppress_roles);
            if !permitted { return Ok(()); }
        }
        if notice.channel_id != action.channel_id
            || notice.message_id != action.id
            || self.state.self_presence == "dnd"
            || self.notification_muted(&notice.channel_id,&notice.guild_id)
            || !notice.message_id.is_empty()
                && self
                    .read
                    .get(&notice.channel_id)
                    .is_some_and(|read| !snowflake_greater(&notice.message_id, read))
            || notice.is_call
                && !self
                    .state
                    .incoming_calls
                    .iter()
                    .any(|call| call.channel_id == notice.channel_id && call.ringing)
        {
            return Ok(());
        }
        // Preserve the origin captured before preparation; the UI must reject
        // already queued data if its coalesced snapshot switches accounts.
        self.data("notice", value);
        Ok(())
    }
    pub(super) fn revoke_access(&mut self) {
        let id = self.state.selected_channel_id.clone();
        let access = self
            .channels
            .get(&id)
            .map(|v| self.channel_access(v))
            .unwrap_or_default();
        if !access.can_read_history {
            self.reading_channel.clear();
            self.state.messages.clear();
            self.cache.replace(&id, Vec::new());
            self.state.pinned_messages.clear();
        }
        if !access.can_view_members {
            self.state.guild_members.clear();
            self.state.guild_members_note.clear();
            self.guild_member_slots = Vec::new();
            self.member_list_id.clear();
        }
        self.state.channel_access = access;
        // A partial list derived from history must disappear as soon as that
        // history becomes inaccessible, even while member access remains.
        self.member_list_fallback();
        self.state.can_pin_messages = self.state.channel_access.can_view
            && self.state.channels.iter().any(|n| n.id == id && n.can_pin);
        if !self.state.channel_access.can_view {
            self.state.search_people.clear();
            if let Some(profile) = &mut self.state.profile_details {
                profile.guild_id.clear();
                profile.guild_name.clear();
                profile.guild_roles.clear();
                profile.nickname.clear();
            }
        }
        self.search_people();
        if !self.state.channel_access.can_view && !self.state.selected_guild_id.is_empty() {
            if let Err(error) = self.recover_guild_navigation() {
                self.error(error);
            }
        }
    }
    fn reaction_event(&mut self, kind: &str, value: &Value) {
        let channel = s(value, "channel_id");
        let id = s(value, "message_id");
        let mut messages = self.cache.get(&channel);
        let Some(message) = messages.iter_mut().find(|m| m.id == id) else {
            return;
        };
        let e = &value["emoji"];
        let emoji = StatusEmoji {
            name: s(e, "name"),
            id: s(e, "id"),
            animated: b(e, "animated"),
        };
        let key = emoji.reaction_key();
        let me = s(value, "user_id") == self.me();
        let burst = b(value, "burst") || i(value, "type") == 1;
        if kind == "MESSAGE_REACTION_REMOVE_ALL" {
            message.reactions.clear();
        } else if kind == "MESSAGE_REACTION_REMOVE_EMOJI" {
            message.reactions.retain(|r| r.emoji.reaction_key() != key);
        } else {
            let existing = message
                .reactions
                .iter()
                .position(|r| r.emoji.reaction_key() == key);
            if kind == "MESSAGE_REACTION_ADD" {
                if let Some(index) = existing {
                    let r = &mut message.reactions[index];
                    r.count += 1;
                    if me {
                        if burst {
                            r.burst_me = true
                        } else {
                            r.normal_me = Some(true)
                        }
                        r.me = true;
                    }
                } else if message.reactions.len() < 100 {
                    message.reactions.push(MessageReaction {
                        emoji,
                        count: 1,
                        me,
                        normal_me: Some(me && !burst),
                        burst_me: me && burst,
                    });
                }
            } else if let Some(index) = existing {
                let r = &mut message.reactions[index];
                r.count = (r.count - 1).max(0);
                if me {
                    if burst {
                        r.burst_me = false
                    } else {
                        r.normal_me = Some(false)
                    }
                    r.me = r.normal_me.unwrap_or(false) || r.burst_me;
                }
                message.reactions.retain(|r| r.count > 0);
            }
        }
        self.cache.replace(&channel, messages);
    }
    pub(super) fn member_list_fallback(&mut self) {
        let guild = &self.state.selected_guild_id;
        let channel = &self.state.selected_channel_id;
        if guild.is_empty()
            || !self.member_list_id.is_empty()
            || self.state.view != "chat"
            || !self.state.channel_access.can_view
            || !self.state.channel_access.can_view_members
            || !self.member_demand.as_ref().is_some_and(|(g, c)| g == guild && c == channel)
        {
            return;
        }
        let expected = self.channels.get(channel).and_then(|raw| {
            self.guild_acl.as_ref().map(|acl| acl.confirmed_member_list_id(raw))
        }).unwrap_or_default();
        if !expected.is_empty() {
            self.state.guild_members.clear();self.state.guild_member_groups.clear();
            self.state.guild_members_total=0;self.state.guild_members_partial=true;self.state.guild_members_has_more=false;
            self.state.guild_members_note="Esperando la lista verificada de miembros del canal.".into();
            return;
        }
        let mut seen = HashSet::new();
        self.state.guild_members = self.state.messages.iter()
            .filter(|m| self.state.channel_access.can_read_history && m.channel_id == *channel && parsing::valid_id(&m.author.id))
            .filter(|m| seen.insert(m.author.id.clone()))
            .take(200)
            .map(|m| ChannelMember { user: m.author.clone(), ..Default::default() })
            .collect();
        self.state.guild_member_groups.clear();
        self.state.guild_members_total = 0;
        self.state.guild_members_partial = true;
        self.state.guild_members_has_more = false;
        self.state.guild_members_note = if self.state.guild_members.is_empty() {
            "Esperando una lista verificada del canal."
        } else {
            "Participantes del historial visible; lista de miembros sin confirmar."
        }.into();
    }
    fn member_list(&mut self, value: &Value) {
        if s(value, "guild_id") != self.state.selected_guild_id
            || !self.state.channel_access.can_view_members
        {
            return;
        }
        let expected = self
            .channels
            .get(&self.state.selected_channel_id)
            .and_then(|c| {
                self.guild_acl
                    .as_ref()
                    .map(|a| a.confirmed_member_list_id(c))
            })
            .unwrap_or_default();
        let id = s(value, "id");
        let packet_channel = s(value, "channel_id");
        if !packet_channel.is_empty() && packet_channel != self.state.selected_channel_id {
            return;
        }
        if expected.is_empty()||id!=expected {
            // A matching channel alone is not a binding for an unknown list.
            // Private lists use the original visibility-overwrite hash.
            self.member_list_fallback();
            return;
        }
        if packet_channel.is_empty() {
            // Once an explicit channel packet established a binding, subsequent
            // incremental packets often omit channel_id. Navigation and ACL
            // changes clear this binding; never adopt an unknown private list.
            let confirmed = if self.member_list_id.is_empty() {
                &expected
            } else {
                &self.member_list_id
            };
            if confirmed.is_empty() || id != *confirmed {
                self.state.guild_members_partial = true;
                self.member_list_fallback();
                return;
            }
        } else if !self.member_list_id.is_empty() && self.member_list_id != id {
            // A new explicit binding must not apply INSERT/UPDATE operations to
            // the previous list's slots, even when it is the same channel.
            self.guild_member_slots = Vec::new();
            self.state.guild_members.clear();
            self.state.guild_member_groups.clear();
        }
        self.member_list_id = id;
        self.state.guild_members_note.clear();
        let total = value.get("member_count").and_then(Value::as_i64)
            .map(|total|total.clamp(0,i32::MAX as i64) as i32).unwrap_or(self.state.guild_members_total);
        self.state.guild_members_total = total;
        for op in items(&value["ops"]).iter().take(2000) {
            let name = s(op, "op");
            if matches!(name.as_str(), "INSERT" | "UPDATE" | "DELETE")
                && op
                    .get("index")
                    .and_then(Value::as_i64)
                    .is_none_or(|n| !(0..=1999).contains(&n))
            {
                continue;
            }
            match s(op, "op").as_str() {
                "SYNC" => {
                    let range = items(&op["range"]);
                    if range.len() != 2 {
                        continue;
                    }
                    let (Some(start), Some(end)) = (range[0].as_u64(), range[1].as_u64()) else {
                        continue;
                    };
                    if start > 1999 || end < start {
                        continue;
                    }
                    let start = start as usize;
                    let end = end.min(1999) as usize;
                    for index in start..=end {
                        if let Some(slot) = self.guild_member_slots.get_mut(index) {
                            *slot = None;
                        }
                    }
                    for (offset, item) in
                        items(&op["items"]).iter().take(end - start + 1).enumerate()
                    {
                        self.set_member_slot(start + offset, item);
                    }
                }
                "INSERT" => {
                    let index = i(op, "index").max(0) as usize;
                    if index <= self.guild_member_slots.len() && index < 2000 {
                        let member = self.parse_member(&op["item"]);
                        self.guild_member_slots.insert(index, member);
                        self.guild_member_slots.truncate(2000);
                    }
                }
                "UPDATE" => self.set_member_slot(i(op, "index").max(0) as usize, &op["item"]),
                "DELETE" => {
                    let index = i(op, "index").max(0) as usize;
                    if index < self.guild_member_slots.len() {
                        self.guild_member_slots.remove(index);
                    }
                }
                "INVALIDATE" => {
                    let range = items(&op["range"]);
                    if range.len() != 2 {
                        continue;
                    }
                    let (Some(start), Some(end)) = (range[0].as_u64(), range[1].as_u64()) else {
                        continue;
                    };
                    if start > 1999 || end < start {
                        continue;
                    }
                    let start = start as usize;
                    let end = end.min(1999) as usize;
                    for index in start..=end {
                        if let Some(v) = self.guild_member_slots.get_mut(index) {
                            *v = None;
                        }
                    }
                }
                _ => (),
            }
        }
        let mut seen = HashSet::new();
        self.state.guild_members = self
            .guild_member_slots
            .iter()
            .flatten()
            .filter(|m| seen.insert(m.user.id.clone()))
            .cloned()
            .collect();
        self.state.guild_members_partial = self.state.guild_members.len() < total.max(0) as usize;
        if value["groups"].is_array() {
        self.state.guild_member_groups = items(&value["groups"])
            .iter()
            .take(1000)
            .map(|g| {
                let id = s(g, "id");
                let role = self.state.guild_roles.iter().find(|r| r.id == id);
                GuildMemberGroup {
                    id,
                    name: role.map(|r| r.name.clone()).unwrap_or_else(|| s(g, "id")),
                    count: i(g, "count"),
                    color: role.map(|r| r.color).unwrap_or(0),
                }
            })
            .collect();
        }
        // Group headers occupy slots too. Private-channel groups describe
        // the visible list, whereas member_count may cover the whole guild.
        let rows=if self.state.guild_member_groups.is_empty(){total.max(0) as usize}else{
            self.state.guild_member_groups.iter().fold(0usize,|rows,group|
                rows.saturating_add(group.count.max(0) as usize).saturating_add(1))
        };
        self.state.guild_members_has_more=self.member_range<1999 && self.member_range+1<rows;
        self.search_people();
    }
    fn set_member_slot(&mut self, index: usize, value: &Value) {
        if index >= 2000 {
            return;
        }
        if self.guild_member_slots.len() <= index {
            self.guild_member_slots.resize(index + 1, None);
        }
        let member = self.parse_member(value);
        self.guild_member_slots[index] = member;
    }
    fn parse_member(&mut self, value: &Value) -> Option<ChannelMember> {
        let v = value.get("member")?;
        let user = parsing::person(&v["user"]);
        if user.id.is_empty() {
            return None;
        }
        self.remember(user.clone());
        self.remember_guild_nickname(&self.state.selected_guild_id.clone(), &user.id, v);
        let mut m = self.member(user, &s(&self.guild_raw, "owner_id"));
        m.nickname = s(v, "nick");
        update_member_roles(&mut m,v,&self.state.guild_roles);
        if let Some(raw)=value.get("presence").filter(|value|value.is_object())
            .or_else(||v.get("presence").filter(|value|value.is_object())) {
            let mut presence=raw.clone();
            presence["user"]=json!({"id":m.user.id});
            self.presence(&presence);
            if let Some(p)=self.state.user_presences.get(&m.user.id) {
                m.presence=p.status.clone();m.activities=p.activities.clone();
                m.custom_status=p.custom_status.clone();m.custom_status_emoji=p.custom_status_emoji.clone();
            }
        }
        Some(m)
    }
    pub(super) fn incoming_participant(
        &mut self,
        value: &Value,
        previous: Option<&CallParticipant>,
    ) -> Option<CallParticipant> {
        let id = s(value, "user_id");
        if !parsing::valid_id(&id) {
            return None;
        }
        let mut user = self
            .people
            .get(&id)
            .cloned()
            .or_else(|| previous.map(|p| p.user.clone()))
            .unwrap_or(Person {
                id: id.clone(),
                name: "Participante".into(),
                ..Default::default()
            });
        let raw = if value["member"]["user"].is_object() {
            &value["member"]["user"]
        } else {
            &value["user"]
        };
        if s(raw, "id") == id {
            let parsed = parsing::person(raw);
            if raw.get("username").is_some() {
                user.username = parsed.username;
            }
            if raw.get("global_name").is_some() {
                user.display_name = parsed.display_name;
            }
            if raw.get("avatar").is_some() {
                user.avatar_url = parsed.avatar_url;
            }
            if raw.get("bot").is_some() {
                user.is_bot = parsed.is_bot;
            }
            user.name = if !user.display_name.is_empty() {
                user.display_name.clone()
            } else if !user.username.is_empty() {
                user.username.clone()
            } else {
                user.name
            };
            self.remember(user.clone());
        }
        let mut participant = previous.cloned().unwrap_or_default();
        participant.user = user;
        participant.is_self = id == self.me();
        if let Some(mute)=value["self_mute"].as_bool(){participant.self_mute=mute;}
        if let Some(deaf)=value["self_deaf"].as_bool(){participant.self_deaf=deaf;}
        if let Some(mute)=value["mute"].as_bool(){participant.server_mute=mute;}
        if let Some(deaf)=value["deaf"].as_bool(){participant.server_deaf=deaf;}
        if let Some(video) = value["self_video"].as_bool() {
            participant.self_video = video;
        }
        if let Some(screen) = value["self_stream"].as_bool() {
            participant.screen_sharing = screen;
        }
        Some(participant)
    }
    pub(super) fn incoming_media_dispatch(&mut self, kind: &str, value: &Value) {
        let mut calls = std::mem::take(&mut self.state.incoming_calls);
        if kind == "VOICE_STATE_UPDATE" {
            let id = s(value, "user_id");
            let channel = s(value, "channel_id");
            if parsing::valid_id(&id) {
                for call in &mut calls {
                    if call.call_participants.is_empty() {
                        call.call_participants = call
                            .participants
                            .iter()
                            .map(|user| CallParticipant {
                                user: user.clone(),
                                is_self: user.id == self.me(),
                                ..Default::default()
                            })
                            .collect();
                    }
                    let known_partial=value.get("channel_id").is_none()&&call.call_participants.iter().any(|person|person.user.id==id);
                    if call.channel_id == channel || known_partial {
                        let previous = call.call_participants.iter().find(|p| p.user.id == id);
                        if let Some(participant) = self.incoming_participant(value, previous) {
                            call.call_participants.retain(|p| p.user.id != id);
                            if call.call_participants.len() < 25 {
                                call.call_participants.push(participant);
                            }
                        }
                    } else if s(value, "guild_id").is_empty() {
                        call.call_participants.retain(|p| p.user.id != id);
                    }
                    call.participants = call
                        .call_participants
                        .iter()
                        .map(|p| p.user.clone())
                        .collect();
                }
            }
        } else if matches!(kind, "STREAM_CREATE" | "STREAM_UPDATE" | "STREAM_DELETE") {
            if let Some((channel, owner)) = incoming_stream_identity(&s(value, "stream_key")) {
                for call in calls.iter_mut().filter(|call| call.channel_id == channel) {
                    if call.call_participants.is_empty() {
                        call.call_participants = call
                            .participants
                            .iter()
                            .map(|user| CallParticipant {
                                user: user.clone(),
                                is_self: user.id == self.me(),
                                ..Default::default()
                            })
                            .collect();
                    }
                    if let Some(participant) = call
                        .call_participants
                        .iter_mut()
                        .find(|p| p.user.id == owner)
                    {
                        participant.screen_sharing = kind != "STREAM_DELETE";
                    }
                }
            }
        }
        self.state.incoming_calls = calls;
    }
    fn incoming_call(&mut self, kind: &str, value: &Value) {
        let channel = s(value, "channel_id");
        if !parsing::valid_id(&channel) {
            return;
        }
        if kind == "CALL_DELETE" {
            self.remove_incoming_invitation(&channel);
            self.state
                .incoming_calls
                .retain(|c| c.channel_id != channel);
            return;
        }
        let previous = self
            .state
            .incoming_calls
            .iter()
            .find(|c| c.channel_id == channel)
            .cloned();
        let ringing = if value["ringing"].is_array() {
            items(&value["ringing"])
                .iter()
                .any(|v| v.as_str() == Some(&self.me()))
        } else {
            self.incoming_service_ring(&channel).unwrap_or_else(||previous.as_ref().is_some_and(|c| c.ringing))
        };
        let call_participants = if value["voice_states"].is_array() {
            items(&value["voice_states"])
                .iter()
                .take(25)
                .filter_map(|v| {
                    let id = s(v, "user_id");
                    let old = previous
                        .as_ref()
                        .and_then(|call| call.call_participants.iter().find(|p| p.user.id == id));
                    self.incoming_participant(v, old)
                })
                .collect::<Vec<_>>()
        } else {
            previous
                .as_ref()
                .map(|c| {
                    if c.call_participants.is_empty() {
                        c.participants
                            .iter()
                            .map(|p| CallParticipant {
                                user: p.clone(),
                                ..Default::default()
                            })
                            .collect()
                    } else {
                        c.call_participants.clone()
                    }
                })
                .unwrap_or_default()
        };
        let participants = call_participants
            .iter()
            .map(|p| p.user.clone())
            .collect::<Vec<_>>();
        let fresh_instance = previous.is_none()
            || kind == "CALL_CREATE"
            || ringing && !self.incoming_service_ring(&channel).unwrap_or_else(||previous.as_ref().is_some_and(|call|call.ringing));
        if fresh_instance {
            self.voice_lifecycle.incoming_sequence = self
                .voice_lifecycle
                .incoming_sequence
                .wrapping_add(1)
                .max(1);
        }
        let instance_id=if fresh_instance {self.voice_lifecycle.incoming_sequence}else{previous.as_ref().map(|call|call.instance_id).unwrap_or(0)};
        let ringing=self.track_incoming_invitation(&channel,instance_id,ringing,fresh_instance);
        let item = self.state.direct_messages.iter().find(|n| n.id == channel);
        let call = IncomingCall {
            channel_id: channel.clone(),
            channel_name: item.map(|n| n.name.clone()).unwrap_or("Llamada".into()),
            kind: item.map(|n| n.kind.clone()).unwrap_or("dm".into()),
            icon_url: item.and_then(|n| n.icon_url.clone()),
            caller: previous
                .as_ref()
                .and_then(|call| call.caller.clone())
                .filter(|caller| participants.iter().any(|p| p.id == caller.id))
                .or_else(|| participants.iter().find(|p| p.id != self.me()).cloned())
                .or_else(|| participants.first().cloned()),
            started_at: previous
                .as_ref()
                .filter(|_| !fresh_instance)
                .map(|c| c.started_at)
                .unwrap_or_else(|| chrono::Utc::now().fixed_offset()),
            participants,
            call_participants,
            instance_id,
            muted: self
                .muted
                .get(&channel)
                .copied()
                .unwrap_or_else(|| item.is_some_and(|item| item.is_muted)),
            ringing,
            unread: true,
        };
        self.state
            .incoming_calls
            .retain(|c| c.channel_id != channel);
        if self.state.incoming_calls.len() < 32 {
            self.state.incoming_calls.push(call.clone());
        }
        if !self.state.direct_messages.iter().any(|n| n.id == channel) {
            self.load_private_channel(&channel);
        }
        // The dedicated incoming-call window owns this notification. Ordinary
        // message toasts remain independent; do not show a duplicate call toast.
    }
    fn reject_call_request(&self, action: &UiAction) -> Result<(String, Value), String> {
        let channel = Self::valid(&action.id)?;
        let me = self.me();
        Self::valid(&me)?;
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        let instance = extra["instanceId"].as_u64().unwrap_or(0);
        if (!action.channel_id.is_empty() && action.channel_id != channel)
            || !self.state.incoming_calls.iter().any(|call| {
                call.channel_id == channel
                    && call.ringing
                    && (instance == 0 || call.instance_id == instance)
            })
        {
            return Err("La llamada entrante ya no está disponible.".into());
        }
        Ok((
            format!("channels/{channel}/call/stop-ringing"),
            json!({"recipients":[me]}),
        ))
    }
    fn dismiss_incoming_call(&mut self, channel: &str) {
        for call in &mut self.state.incoming_calls {
            if call.channel_id == channel {
                call.ringing = false;
                call.unread = false;
            }
        }
        self.decorate_dms();
    }
    pub(super) fn reject_call(&mut self, action: &UiAction) -> Result<(), String> {
        let (path, body) = self.reject_call_request(action)?;
        let api = self.api()?;
        self.launch(
            UiAction::id("__call-rejected", &action.id),
            None,
            async move {
                api.request(Method::POST, &path, Some(body))
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Value::Null)
            },
        );
        self.dismiss_incoming_call(&action.id);
        let context:Value=serde_json::from_str(&action.extra).unwrap_or_default();
        self.end_incoming_invitation(&action.id,context["instanceId"].as_u64().unwrap_or(0),false);
        self.state.status = "Llamada rechazada; deteniendo el timbre de este canal…".into();
        self.publish();
        Ok(())
    }
    fn load_private_channel(&mut self, id: &str) {
        if !parsing::valid_id(id)
            || self.private_channel_loads.len() >= 8
            || self.private_channel_loads.contains(id)
            || self
                .private_channel_failures
                .get(id)
                .is_some_and(|at| at.elapsed() < Duration::from_secs(60))
        {
            return;
        }
        let Ok(api) = self.api() else { return };
        self.private_channel_loads.insert(id.into());
        let channel = id.to_owned();
        self.launch(UiAction::id("__incoming-dm", id), None, async move {
            api.get(&format!("channels/{channel}"))
                .await
                .map_err(|e| e.to_string())
        });
    }
}
pub(super) fn incoming_stream_identity(key: &str) -> Option<(String, String)> {
    let parts = key.split(':').collect::<Vec<_>>();
    let (channel, owner) = match parts.as_slice() {
        ["call", channel, owner] => (*channel, *owner),
        ["guild", _, channel, owner] => (*channel, *owner),
        _ => return None,
    };
    (parsing::valid_id(channel) && parsing::valid_id(owner)).then(|| (channel.into(), owner.into()))
}
pub(super) fn merge_object(previous: &Value, update: &Value) -> Value {
    let mut merged = previous.as_object().cloned().unwrap_or_default();
    if let Some(fields) = update.as_object() {
        for (key, value) in fields {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}
fn numeric_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.len().cmp(&b.len()).then(a.cmp(b))
}
pub(super) fn snowflake_greater(a: &str, b: &str) -> bool {
    numeric_cmp(a, b) == std::cmp::Ordering::Greater
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn guild_modes_share_toast_and_sound_policy_without_changing_unread_accounting() {
        let (mut c, updates) = Controller::test_fixture(); private_chat(&mut c);
        c.state.guilds=vec![NavItem {id:"50".into(),kind:"guild".into(),..Default::default()}];
        c.guild_acl=Some(GuildAccess::from_values(&json!({"id":"50","roles":[]}),&json!({"roles":["60"]}),"1"));
        c.notification_settings.apply(&json!({"guild_id":"50","message_notifications":1,"suppress_everyone":true,"suppress_roles":true}));
        let packet=|id:&str,direct:bool|json!({"id":id,"channel_id":"20","guild_id":"50","content":"Offline notification fixture","author":{"id":"9","username":"Peer"},"mention_everyone":true,"mention_roles":["60"],"mentions":if direct{json!([{"id":"1","username":"Self"}])}else{json!([])}});
        c.dispatch("MESSAGE_CREATE",packet("100",false)).await.unwrap();
        assert!(!updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,_)if kind=="sound"||kind=="notice")));
        assert_eq!(c.mention.get("20"),Some(&1));
        c.dispatch("MESSAGE_CREATE",packet("101",true)).await.unwrap();
        let emitted=updates.try_iter().filter_map(|update|match update{UiUpdate::Data(kind,value)if kind=="sound"||kind=="notice"=>Some((kind,value)),_=>None}).collect::<Vec<_>>();
        assert_eq!(emitted.iter().filter(|(kind,_)|kind=="notice").count(),1);
        assert_eq!(emitted.iter().find(|(kind,_)|kind=="sound").unwrap().1["event"],"mention");
        let notice=emitted.iter().find(|(kind,_)|kind=="notice").unwrap().1.clone();
        c.notification_settings.apply(&json!({"guild_id":"50","message_notifications":2}));
        c.complete_notice(&UiAction {channel_id:"20".into(),id:"101".into(),..Default::default()},notice).unwrap();
        assert!(!updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,_)if kind=="notice")));
        c.dispatch("MESSAGE_CREATE",packet("102",true)).await.unwrap();
        assert!(!updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,_)if kind=="sound"||kind=="notice")));
        assert_eq!(c.mention.get("20"),Some(&3));
        c.notification_settings.apply(&json!({"guild_id":"50","message_notifications":0}));
        c.dispatch("MESSAGE_CREATE",packet("103",false)).await.unwrap();
        let emitted=updates.try_iter().filter_map(|update|match update{UiUpdate::Data(kind,value)if kind=="sound"||kind=="notice"=>Some((kind,value)),_=>None}).collect::<Vec<_>>();
        assert_eq!(emitted.iter().filter(|(kind,_)|kind=="notice").count(),1);
        assert_eq!(emitted.iter().find(|(kind,_)|kind=="sound").unwrap().1["event"],"message");
        assert_eq!(c.mention.get("20"),Some(&4));
    }
    #[tokio::test]
    async fn event_audio_ignores_own_duplicate_and_muted_messages_and_distinguishes_roles() {
        let (mut c, updates) = Controller::test_fixture();
        private_chat(&mut c);
        let packet = |id:&str,author:&str| json!({"id":id,"channel_id":"20","content":"synthetic","author":{"id":author,"username":"Synthetic"}});
        c.dispatch("MESSAGE_CREATE",packet("100","1")).await.unwrap();
        assert!(!updates.try_iter().any(|u|matches!(u,UiUpdate::Data(k,_)if k=="sound")));
        c.dispatch("MESSAGE_CREATE",packet("101","9")).await.unwrap();
        assert_eq!(updates.try_iter().filter_map(|u|match u{UiUpdate::Data(k,v)if k=="sound"=>Some(v["event"].clone()),_=>None}).collect::<Vec<_>>(),vec![json!("message")]);
        c.dispatch("MESSAGE_CREATE",packet("101","9")).await.unwrap();
        c.muted.insert("20".into(),true);c.dispatch("MESSAGE_CREATE",packet("102","9")).await.unwrap();
        assert!(!updates.try_iter().any(|u|matches!(u,UiUpdate::Data(k,_)if k=="sound")));
        c.muted.clear();c.guild_acl=Some(GuildAccess::from_values(&json!({"id":"50","roles":[]}),&json!({"roles":["60"]}),"1"));
        let mut role=packet("103","9");role["guild_id"]=json!("50");role["mention_roles"]=json!(["60"]);
        c.dispatch("MESSAGE_CREATE",role).await.unwrap();
        assert_eq!(updates.try_iter().filter_map(|u|match u{UiUpdate::Data(k,v)if k=="sound"=>Some(v["event"].clone()),_=>None}).collect::<Vec<_>>(),vec![json!("mention")]);
    }
    #[tokio::test]
    async fn message_sound_packets_keep_the_real_destination_and_delivery_identity() {
        let (mut c, updates) = Controller::test_fixture();
        private_chat(&mut c);
        let packet=json!({"id":"100","channel_id":"30","content":"offline sound context","author":{"id":"9","username":"Peer"}});
        c.dispatch("MESSAGE_CREATE",packet.clone()).await.unwrap();
        let sounds=updates.try_iter().filter_map(|update|match update {UiUpdate::Data(kind,value) if kind=="sound"=>Some(value),_=>None}).collect::<Vec<_>>();
        assert_eq!(sounds,vec![json!({"event":"message","channel_id":"30","message_id":"100","account_id":"1"})]);
        assert_eq!(c.state.selected_channel_id,"20","Receiving a notification must not change navigation");
        c.dispatch("MESSAGE_CREATE",packet).await.unwrap();
        assert!(!updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,_)if kind=="sound")),"A repeated Gateway delivery must not emit another sound");
    }
    #[tokio::test]
    async fn call_window_replaces_call_toast_and_old_reject_cannot_target_a_new_instance() {
        let (mut c, updates) = Controller::test_fixture();
        private_chat(&mut c);
        c.incoming_call(
            "CALL_CREATE",
            &json!({"channel_id":"20","ringing":["1"],"voice_states":[{"user_id":"9"}]}),
        );
        assert!(c.state.incoming_calls[0].ringing);
        assert!(
            !updates
                .try_iter()
                .any(|update| matches!(update,UiUpdate::Data(kind,_)if kind=="notice"))
        );
        let current = c.state.incoming_calls[0].instance_id;
        let mut action = UiAction::id("reject-call", "20");
        action.extra = json!({"instanceId":current+1}).to_string();
        assert!(c.reject_call_request(&action).is_err());
        action.extra = json!({"instanceId":current}).to_string();
        assert!(c.reject_call_request(&action).is_ok());
    }
    #[tokio::test]
    async fn incoming_media_updates_keep_person_and_call_instance_without_joining() {
        let (mut c, _) = Controller::test_fixture();
        c.state.current_user = Some(Person {
            id: "1".into(),
            ..Default::default()
        });
        let user = Person {
            id: "9".into(),
            name: "Nombre completo".into(),
            username: "synthetic".into(),
            display_name: "Nombre completo".into(),
            avatar_url: Some("https://example.invalid/own-fixture.png".into()),
            is_bot: true,
        };
        c.remember(user.clone());
        c.muted.insert("30".into(), true);
        c.incoming_call("CALL_CREATE",&json!({"channel_id":"30","ringing":["1"],"voice_states":[{"user_id":"9","self_video":true,"self_stream":true}]}));
        let call = c.state.incoming_calls[0].clone();
        assert_eq!(call.participants, [user.clone()]);
        assert!(call.muted && call.instance_id > 0);
        assert!(call.call_participants[0].self_video && call.call_participants[0].screen_sharing);
        c.incoming_call(
            "CALL_UPDATE",
            &json!({"channel_id":"30","voice_states":[{"user_id":"9","self_video":false}]}),
        );
        let updated = &c.state.incoming_calls[0];
        assert_eq!(updated.instance_id, call.instance_id);
        assert_eq!(updated.started_at, call.started_at);
        assert!(
            !updated.call_participants[0].self_video && updated.call_participants[0].screen_sharing
        );
        assert_eq!(updated.participants[0], user);
        c.incoming_media_dispatch("VOICE_STATE_UPDATE",&json!({"channel_id":"30","user_id":"9","self_video":true,"member":{"user":{"id":"9","username":"new-username"}}}));
        assert_eq!(
            c.state.incoming_calls[0].participants[0].avatar_url,
            user.avatar_url
        );
        assert!(c.state.incoming_calls[0].call_participants[0].self_video);
        c.incoming_media_dispatch("STREAM_DELETE", &json!({"stream_key":"call:30:9"}));
        assert!(!c.state.incoming_calls[0].call_participants[0].screen_sharing);
        c.incoming_media_dispatch(
            "STREAM_UPDATE",
            &json!({"stream_key":"call:30:9","paused":false}),
        );
        assert!(c.state.incoming_calls[0].call_participants[0].screen_sharing);
        c.incoming_media_dispatch(
            "VOICE_STATE_UPDATE",
            &json!({"channel_id":null,"user_id":"9"}),
        );
        assert!(c.state.incoming_calls[0].participants.is_empty());
        assert!(c.voice_channel.is_empty() && c.media.is_none() && !c.state.voice_connecting);
    }
    #[tokio::test]
    async fn incoming_ring_instance_changes_only_for_a_new_call_or_new_ring() {
        let (mut c, _) = Controller::test_fixture();
        c.state.current_user = Some(Person {
            id: "1".into(),
            ..Default::default()
        });
        c.incoming_call("CALL_CREATE", &json!({"channel_id":"30","ringing":["1"]}));
        let first = c.state.incoming_calls[0].instance_id;
        c.incoming_call("CALL_UPDATE", &json!({"channel_id":"30","ringing":["1"]}));
        assert_eq!(c.state.incoming_calls[0].instance_id, first);
        c.incoming_call("CALL_UPDATE", &json!({"channel_id":"30","ringing":[]}));
        assert_eq!(c.state.incoming_calls[0].instance_id, first);
        c.incoming_call("CALL_UPDATE", &json!({"channel_id":"30","ringing":["1"]}));
        assert!(c.state.incoming_calls[0].instance_id > first);
        let previous = c.state.incoming_calls[0].instance_id;
        c.incoming_call("CALL_CREATE", &json!({"channel_id":"30","ringing":["1"]}));
        assert!(c.state.incoming_calls[0].instance_id > previous);
        let legacy: IncomingCall = serde_json::from_value(
            json!({"ChannelId":"30","Participants":[{"Id":"9","Name":"Legacy"}]}),
        )
        .unwrap();
        assert_eq!(legacy.participants[0].name, "Legacy");
        assert!(legacy.call_participants.is_empty());
    }
    fn private_chat(c: &mut Controller) {
        channel(c);
        c.state.selected_guild_id.clear();
        c.state.self_presence = "online".into();
        c.state.direct_messages.push(NavItem {
            id: "20".into(),
            kind: "dm".into(),
            name: "other".into(),
            recipient_id: "2".into(),
            ..Default::default()
        });
    }
    fn created(id: &str) -> Value {
        json!({"id":id,"channel_id":"20","content":"fixture message","author":{"id":"2","username":"other"},"timestamp":"2026-10-20T10:00:00Z"})
    }
    fn reading(id: &str) -> UiAction {
        UiAction {
            kind: "chat-visibility".into(),
            channel_id: "20".into(),
            id: id.into(),
            text: "reading".into(),
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn selected_chat_is_unread_until_actually_reported_visible_and_create_is_deduped() {
        let (mut c, updates) = Controller::test_fixture();
        private_chat(&mut c);
        c.dispatch("MESSAGE_CREATE", created("100")).await.unwrap();
        c.dispatch("MESSAGE_CREATE", created("100")).await.unwrap();
        assert_eq!(c.mention["20"], 1);
        assert!(!c.read.contains_key("20"));
        assert_eq!(c.state.unread_message_count, 1);
        let notices = updates
            .try_iter()
            .filter_map(|update| match update {
                UiUpdate::Data(kind, value) if kind == "notice" => Some(value),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0]["MessageId"], "100");
        c.report_chat_visibility(&reading("100")).unwrap();
        assert_eq!(c.read["20"], "100");
        assert_eq!(c.state.unread_message_count, 0);
    }
    #[test]
    fn notification_origin_uses_person_group_or_guild_image_and_keeps_sender_and_context() {
        let (mut c, _) = Controller::test_fixture();
        private_chat(&mut c);
        let mut message = parsing::message(&created("100"));
        message.author.avatar_url = Some("https://cdn.discordapp.com/avatars/2/person.png".into());
        let notice = c.message_notice(&message, &created("100"), true);
        assert_eq!(notice.avatar_url, message.author.avatar_url);
        assert!(notice.title.contains("other"));
        c.state.direct_messages[0].kind = "group".into();
        c.state.direct_messages[0].name = "My group".into();
        c.state.direct_messages[0].icon_url =
            Some("https://cdn.discordapp.com/channel-icons/20/group.png".into());
        let group = c.message_notice(&message, &created("100"), true);
        assert_eq!(group.avatar_url, c.state.direct_messages[0].icon_url);
        assert_eq!(group.title, "other · My group");
        c.state.direct_messages[0].icon_url = None;
        assert!(
            c.message_notice(&message, &created("100"), true)
                .avatar_url
                .is_none(),
            "missing group icon must not substitute the sender"
        );
        c.state.direct_messages.clear();
        c.state.channels.push(NavItem {
            id: "20".into(),
            name: "general".into(),
            ..Default::default()
        });
        c.state.guilds.push(NavItem {
            id: "10".into(),
            name: "My server".into(),
            icon_url: Some("https://cdn.discordapp.com/icons/10/server.png".into()),
            ..Default::default()
        });
        let guild = c.message_notice(&message, &json!({"guild_id":"10"}), false);
        assert_eq!(guild.avatar_url, c.state.guilds[0].icon_url);
        assert_eq!(guild.title, "other · My server / #general");
        assert_eq!(guild.guild_id, "10");
    }
    #[test]
    fn sticker_and_forward_notifications_use_real_summary_without_replacing_chat_body() {
        let sticker = parsing::message(&json!({"sticker_items":[{"id":"301","name":"wave"}]}));
        assert_eq!(message_notice_body(&sticker), ":wave:");
        let different = parsing::message(&json!({"sticker_items":[{"id":"302","name":"hug"}]}));
        assert_eq!(message_notice_body(&different), ":hug:");
        let forward = parsing::message(
            &json!({"author":{"id":"2","username":"Forwarder"},"message_reference":{"type":1},"message_snapshots":[{"message":{"content":"Original message"}}]}),
        );
        assert_eq!(message_notice_body(&forward), "Reenvió un mensaje");
        assert_eq!(forward.content, "Original message");
        assert_eq!(forward.author.id, "2");
    }
    #[test]
    fn delayed_image_completion_does_not_show_read_muted_or_cancelled_call_notices() {
        let (mut c, updates) = Controller::test_fixture();
        private_chat(&mut c);
        let notice = c.message_notice(&parsing::message(&created("100")), &created("100"), true);
        let action = UiAction {
            kind: "__notice-image-ready".into(),
            id: "100".into(),
            channel_id: "20".into(),
            ..Default::default()
        };
        c.read.insert("20".into(), "100".into());
        c.complete_notice(&action, notice_with_origin(&notice,"1",c.generation))
            .unwrap();
        assert!(updates.try_iter().next().is_none());
        c.read.clear();
        c.muted.insert("20".into(), true);
        c.complete_notice(&action, notice_with_origin(&notice,"1",c.generation))
            .unwrap();
        assert!(updates.try_iter().next().is_none());
        c.muted.clear();
        c.complete_notice(&action, notice_with_origin(&notice,"1",c.generation))
            .unwrap();
        assert!(matches!(updates.try_recv().unwrap(),UiUpdate::Data(kind,_) if kind=="notice"));
        let call = DesktopNotice {
            channel_id: "20".into(),
            is_call: true,
            ..Default::default()
        };
        let action = UiAction {
            kind: "__notice-image-ready".into(),
            channel_id: "20".into(),
            ..Default::default()
        };
        c.complete_notice(&action, notice_with_origin(&call,"1",c.generation))
            .unwrap();
        assert!(updates.try_iter().next().is_none());
    }
    #[test]
    fn notices_capture_account_and_session_before_preparation_and_reject_stale_completion() {
        let (mut c, updates) = Controller::test_fixture();
        private_chat(&mut c);
        let notice=c.message_notice(&parsing::message(&created("100")),&created("100"),true);
        c.queue_notice(notice);
        let payload=updates.try_iter().find_map(|update|match update {UiUpdate::Data(kind,value) if kind=="notice"=>Some(value),_=>None}).unwrap();
        assert_eq!(payload["accountId"],"1");
        assert_eq!(payload["generation"],c.generation);
        let action=UiAction {kind:"__notice-image-ready".into(),channel_id:"20".into(),id:"100".into(),..Default::default()};
        c.state.current_user.as_mut().unwrap().id="2".into();
        c.complete_notice(&action,payload.clone()).unwrap();
        assert!(updates.try_iter().next().is_none());
        c.state.current_user.as_mut().unwrap().id="1".into();
        c.generation=c.generation.wrapping_add(1);
        c.complete_notice(&action,payload).unwrap();
        assert!(updates.try_iter().next().is_none(),"Even the same account must reject an image completion from an earlier session");
    }
    #[test]
    fn rejecting_one_incoming_call_targets_only_self_and_never_leaves_active_call() {
        let (mut c, _) = Controller::test_fixture();
        private_chat(&mut c);
        c.voice_channel = "99".into();
        c.state.voice_connected = true;
        c.state.incoming_calls = vec![
            IncomingCall {
                channel_id: "20".into(),
                ringing: true,
                unread: true,
                ..Default::default()
            },
            IncomingCall {
                channel_id: "21".into(),
                ringing: true,
                ..Default::default()
            },
        ];
        let action = UiAction::id("reject-call", "20");
        let (path, body) = c.reject_call_request(&action).unwrap();
        assert_eq!(path, "channels/20/call/stop-ringing");
        assert_eq!(body, json!({"recipients":["1"]}));
        c.dismiss_incoming_call("20");
        assert_eq!(c.voice_channel, "99");
        assert!(c.state.voice_connected);
        assert!(!c.state.incoming_calls[0].ringing);
        assert!(!c.state.incoming_calls[0].unread);
        assert!(c.state.incoming_calls[1].ringing);
        assert!(c.reject_call_request(&action).is_err());
        assert!(
            c.reject_call_request(&UiAction::id("reject-call", "404"))
                .is_err()
        );
    }
    #[tokio::test]
    async fn partial_read_and_late_ack_preserve_new_messages_and_other_channels() {
        let (mut c, _) = Controller::test_fixture();
        private_chat(&mut c);
        c.dispatch("MESSAGE_CREATE", created("100")).await.unwrap();
        c.dispatch("MESSAGE_CREATE", created("101")).await.unwrap();
        c.mention.insert("21".into(), 7);
        c.report_chat_visibility(&reading("100")).unwrap();
        assert_eq!(c.read["20"], "100");
        assert_eq!(c.mention["20"], 1);
        assert!(c.reading_channel.is_empty());
        c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"90"}))
            .await
            .unwrap();
        assert_eq!(c.read["20"], "100");
        assert_eq!(c.mention["20"], 1);
        assert_eq!(c.mention["21"], 7);
        c.report_chat_visibility(&reading("101")).unwrap();
        assert!(!c.mention.contains_key("20"));
        assert_eq!(c.mention["21"], 7);
    }
    #[tokio::test]
    async fn visibility_rejects_fabricated_messages_wrong_channel_and_nonchat_views() {
        let (mut c, _) = Controller::test_fixture();
        private_chat(&mut c);
        c.dispatch("MESSAGE_CREATE", created("100")).await.unwrap();
        c.report_chat_visibility(&reading("999")).unwrap();
        let mut wrong = reading("100");
        wrong.channel_id = "21".into();
        c.report_chat_visibility(&wrong).unwrap();
        c.state.view = "pins".into();
        c.report_chat_visibility(&reading("100")).unwrap();
        assert!(!c.read.contains_key("20"));
        assert!(!c.read.contains_key("21"));
        assert_eq!(c.mention["20"], 1);
        c.state.view = "chat".into();
        c.report_chat_visibility(&reading("100")).unwrap();
        assert_eq!(c.reading_channel, "20");
        let mut hidden = reading("");
        hidden.text.clear();
        c.report_chat_visibility(&hidden).unwrap();
        assert!(c.reading_channel.is_empty());
    }
    #[tokio::test]
    async fn own_and_read_messages_never_notify_dnd_and_mute_preserve_badges() {
        let (mut c, updates) = Controller::test_fixture();
        private_chat(&mut c);
        let mut own = created("100");
        own["author"]["id"] = json!("1");
        c.dispatch("MESSAGE_CREATE", own).await.unwrap();
        c.read.insert("20".into(), "101".into());
        c.dispatch("MESSAGE_CREATE", created("101")).await.unwrap();
        c.state.self_presence = "dnd".into();
        c.dispatch("MESSAGE_CREATE", created("102")).await.unwrap();
        c.state.self_presence = "online".into();
        c.muted.insert("20".into(), true);
        c.dispatch("MESSAGE_CREATE", created("103")).await.unwrap();
        assert_eq!(c.mention["20"], 2);
        assert_eq!(
            updates
                .try_iter()
                .filter(|update| matches!(update,UiUpdate::Data(kind,_) if kind=="notice"))
                .count(),
            0
        );
    }
    fn own_created(id: &str) -> Value {
        let mut message = created(id);
        message["author"]["id"] = json!("1");
        message
    }
    fn clean_private_chat(c: &mut Controller) {
        private_chat(c);
        c.state.direct_messages[0].last_message_id = "90".into();
        c.read.insert("20".into(), "90".into());
        c.state.direct_messages.push(NavItem {
            id: "21".into(),
            kind: "dm".into(),
            last_message_id: "99".into(),
            ..Default::default()
        });
        c.read.insert("21".into(), "80".into());
        c.mention.insert("21".into(), 2);
        c.decorate_dms();
    }
    fn assert_private_unread(c: &Controller, channel: &str, unread: bool, mentions: i32) {
        let item = c
            .state
            .direct_messages
            .iter()
            .find(|n| n.id == channel)
            .unwrap();
        assert_eq!(item.unread, unread, "channel {channel}");
        assert_eq!(item.mention_count, mentions, "channel {channel}");
    }
    async fn confirm_own(c: &mut Controller, id: &str) {
        let mut action = UiAction::new("__sent");
        action.channel_id = "20".into();
        c.complete(action, own_created(id)).await.unwrap();
    }
    #[tokio::test]
    async fn own_delivery_never_flashes_unread_for_either_gateway_rest_order() {
        for rest_first in [false, true] {
            let (mut c, updates) = Controller::test_fixture();
            clean_private_chat(&mut c);
            if rest_first {
                confirm_own(&mut c, "100").await;
            } else {
                c.dispatch("MESSAGE_CREATE", own_created("100"))
                    .await
                    .unwrap();
            }
            assert_private_unread(&c, "20", false, 0);
            assert_private_unread(&c, "21", true, 2);
            assert_eq!(c.read["20"], "90", "sending is not a read ACK");
            if rest_first {
                c.dispatch("MESSAGE_CREATE", own_created("100"))
                    .await
                    .unwrap();
            } else {
                confirm_own(&mut c, "100").await;
            }
            assert_private_unread(&c, "20", false, 0);
            assert_private_unread(&c, "21", true, 2);
            assert_eq!(
                c.latest["20"], "100",
                "delivery still updates conversation order"
            );
            assert_eq!(c.state.unread_message_count, 2);
            assert_eq!(
                updates
                    .try_iter()
                    .filter(|u| matches!(u, UiUpdate::Data(k, _) if k == "notice"))
                    .count(),
                0
            );
        }
    }
    #[tokio::test]
    async fn own_delivery_preserves_prior_foreign_unread_and_partial_ack_in_both_orders() {
        for rest_first in [false, true] {
            let (mut c, _) = Controller::test_fixture();
            clean_private_chat(&mut c);
            c.dispatch("MESSAGE_CREATE", created("101")).await.unwrap();
            if rest_first {
                confirm_own(&mut c, "102").await;
            } else {
                c.dispatch("MESSAGE_CREATE", own_created("102"))
                    .await
                    .unwrap();
            }
            assert_private_unread(&c, "20", true, 1);
            if rest_first {
                c.dispatch("MESSAGE_CREATE", own_created("102"))
                    .await
                    .unwrap();
            } else {
                confirm_own(&mut c, "102").await;
            }
            c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"100"}))
                .await
                .unwrap();
            assert_private_unread(&c, "20", true, 1);
            assert_eq!(c.read["20"], "100");
            c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"101"}))
                .await
                .unwrap();
            assert_private_unread(&c, "20", false, 0);
            assert_private_unread(&c, "21", true, 2);
            assert_eq!(c.latest["20"], "102");
            c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"90"}))
                .await
                .unwrap();
            assert_eq!(c.read["20"], "101");
            assert_private_unread(&c, "20", false, 0);
        }
    }
    #[tokio::test]
    async fn foreign_create_delayed_until_after_own_delivery_remains_unread() {
        let (mut c, _) = Controller::test_fixture();
        clean_private_chat(&mut c);
        confirm_own(&mut c, "102").await;
        c.dispatch("MESSAGE_CREATE", own_created("102"))
            .await
            .unwrap();
        c.dispatch("MESSAGE_CREATE", created("101")).await.unwrap();
        assert_private_unread(&c, "20", true, 1);
        assert_eq!(c.latest["20"], "102");
        assert_eq!(c.read["20"], "90");
        c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"101"}))
            .await
            .unwrap();
        assert_private_unread(&c, "20", false, 0);
        c.dispatch("MESSAGE_CREATE", created("103")).await.unwrap();
        c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"100"}))
            .await
            .unwrap();
        assert_private_unread(&c, "20", true, 1);
        assert_private_unread(&c, "21", true, 2);
        assert_eq!(c.read["20"], "101");
    }
    #[test]
    fn partial_read_keeps_tracked_incoming_when_delivery_metadata_has_been_evicted() {
        let (mut c, _) = Controller::test_fixture();
        c.incoming_latest.insert("20".into(), "101".into());
        c.read.insert("20".into(), "90".into());
        c.mention.insert("20".into(), 2);
        c.unread_seen
            .extend([("20".into(), "100".into()), ("20".into(), "101".into())]);
        assert!(c.mark_read_through("20", "100"));
        assert_eq!(c.mention["20"], 1);
        assert_eq!(c.read["20"], "100");
        assert!(c.mark_read_through("20", "101"));
        assert!(!c.mention.contains_key("20"));
    }
    #[tokio::test]
    async fn own_server_delivery_does_not_create_unread_and_reading_foreign_clears_only_its_guild()
    {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.guild_raw = json!({"id":"10","roles":[{"id":"10","permissions":"68608"}]});
        c.guild_acl = Some(GuildAccess::from_values(&c.guild_raw, &json!({}), "1"));
        c.channels.insert(
            "20".into(),
            json!({"id":"20","guild_id":"10","type":0,"last_message_id":"90"}),
        );
        c.state.guilds = vec![
            NavItem {
                id: "10".into(),
                ..Default::default()
            },
            NavItem {
                id: "11".into(),
                unread: true,
                mention_count: 7,
                ..Default::default()
            },
        ];
        c.read.insert("20".into(), "90".into());
        c.rebuild_channels();
        let mut own = own_created("100");
        own["guild_id"] = json!("10");
        c.dispatch("MESSAGE_CREATE", own).await.unwrap();
        assert!(!c.state.channels[0].unread);
        assert!(!c.state.guilds[0].unread);
        let mut foreign = created("101");
        foreign["guild_id"] = json!("10");
        foreign["mentions"] = json!([{"id":"1"}]);
        c.dispatch("MESSAGE_CREATE", foreign).await.unwrap();
        confirm_own(&mut c, "102").await;
        assert_eq!(c.state.channels[0].mention_count, 1);
        assert_eq!(c.state.guilds[0].mention_count, 1);
        c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"101"}))
            .await
            .unwrap();
        assert!(!c.state.channels[0].unread);
        assert_eq!(c.state.guilds[0].mention_count, 0);
        assert!(!c.state.guilds[0].unread);
        assert!(c.state.guilds[1].unread);
        assert_eq!(c.state.guilds[1].mention_count, 7);
    }
    fn channel(c: &mut Controller) {
        c.state.signed_in = true;
        c.state.selected_guild_id = "10".into();
        c.state.selected_channel_id = "20".into();
        c.state.view = "chat".into();
        c.state.channel_access = ChannelAccess::private();
        c.state.current_user = Some(Person {
            id: "1".into(),
            ..Default::default()
        });
    }
    fn member(id: &str) -> Value {
        json!({"member":{"user":{"id":id,"username":format!("user{id}")}}})
    }
    #[test]
    fn partial_channel_update_preserves_acl() {
        let previous = json!({"id":"20","type":0,"permission_overwrites":[{"id":"10","type":0,"deny":"1024","allow":"0"}]});
        let updated = merge_object(&previous, &json!({"id":"20","name":"new"}));
        assert_eq!(updated["type"], 0);
        assert_eq!(
            updated["permission_overwrites"],
            previous["permission_overwrites"]
        );
    }
    #[tokio::test]
    async fn revoked_channel_discards_sensitive_views() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.guild_raw = json!({"id":"10","roles":[{"id":"10","permissions":"3072"}]});
        c.guild_acl = Some(GuildAccess::from_values(&c.guild_raw, &json!({}), "1"));
        c.channels.insert("20".into(),json!({"id":"20","guild_id":"10","type":0,"permission_overwrites":[{"id":"10","type":0,"deny":"1024","allow":"0"}]}));
        c.cache.upsert(ChatMessage {
            id: "4".into(),
            channel_id: "20".into(),
            ..Default::default()
        });
        c.state.messages = c.cache.get("20").into();
        c.state.guild_members.push(ChannelMember {
            user: Person {
                id: "2".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        c.dispatch(
            "CHANNEL_UPDATE",
            json!({"id":"20","guild_id":"10","name":"renamed"}),
        )
        .await
        .unwrap();
        assert!(!c.state.channel_access.can_view);
        assert!(c.state.messages.is_empty());
        assert!(c.state.guild_members.is_empty());
        assert!(c.state.search_people.is_empty());
        assert!(c.cache.get("20").is_empty());
    }
    #[test]
    fn member_lists_scoped_to_explicit_channel_and_sync_clears_range() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        let packet = |channel: &str, entries: Vec<Value>| json!({"guild_id":"10","channel_id":channel,"id":"private-list","member_count":2,"ops":[{"op":"SYNC","range":[0,1],"items":entries}]});
        c.member_list(&packet("21", vec![member("2")]));
        assert!(c.state.guild_members.is_empty());
        c.member_list(&packet("20", vec![member("2"), member("3")]));
        assert_eq!(c.state.guild_members.len(), 2);
        c.member_list(&packet("20", vec![member("4")]));
        assert_eq!(c.state.guild_members.len(), 1);
        assert_eq!(c.state.guild_members[0].user.id, "4");
    }
    #[test]
    fn malformed_member_operation_does_not_modify_zero() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.set_member_slot(0, &member("2"));
        c.member_list(&json!({"guild_id":"10","channel_id":"20","id":"list","ops":[{"op":"DELETE","index":-1},{"op":"UPDATE","index":-20,"item":member("3")}]}));
        assert_eq!(c.guild_member_slots[0].as_ref().unwrap().user.id, "2");
    }
    #[test]
    fn confirmed_private_member_binding_accepts_incremental_packets_and_dies_on_navigation() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.member_list(&json!({"guild_id":"10","channel_id":"20","id":"scoped-list","member_count":1,"ops":[{"op":"SYNC","range":[0,0],"items":[member("2")]}]}));
        c.member_list(&json!({"guild_id":"10","id":"scoped-list","member_count":1,"ops":[{"op":"UPDATE","index":0,"item":member("3") }]}));
        assert_eq!(c.state.guild_members[0].user.id, "3");
        c.reset_navigation();
        c.state.selected_channel_id = "21".into();
        c.member_list(&json!({"guild_id":"10","id":"scoped-list","member_count":1,"ops":[{"op":"SYNC","range":[0,0],"items":[member("2")]}]}));
        assert!(c.state.guild_members.is_empty());
    }
    fn member_acl_context(c: &mut Controller, public: bool, binding: &str) {
        channel(c);
        c.guild_raw = json!({"id":"10","roles":[
            {"id":"10","permissions":if public {"66560"} else {"0"}},
            {"id":"30","permissions":"66560"},
            {"id":"31","permissions":"66560"}
        ]});
        c.self_member = json!({"roles":["30"]});
        c.guild_acl = Some(GuildAccess::from_values(&c.guild_raw, &c.self_member, "1"));
        c.channels.insert("20".into(), json!({"id":"20","guild_id":"10","type":0}));
        c.rebuild_channels();
        c.state.channel_access = c.channel_access(&c.channels["20"]);
        c.member_demand = Some(("10".into(), "20".into()));
        c.member_list(&json!({"guild_id":"10","channel_id":"20","id":binding,"member_count":1,"ops":[{"op":"SYNC","range":[0,0],"items":[member("2")]}]}));
        assert_eq!(c.state.guild_members.len(), 1);
        assert_eq!(c.member_list_id, binding);
    }
    #[tokio::test]
    async fn everyone_permission_change_retires_old_member_binding_while_own_view_remains() {
        let (mut c, _) = Controller::test_fixture();
        member_acl_context(&mut c, true, "everyone");
        let version = c.state.member_context_version;
        c.dispatch("GUILD_ROLE_UPDATE", json!({"guild_id":"10","role":{"id":"10","permissions":"0"}})).await.unwrap();
        assert!(c.state.channel_access.can_view && c.state.channel_access.can_read_history && c.state.channel_access.can_view_members);
        assert!(c.member_list_id.is_empty());
        assert!(c.member_demand.is_none());
        assert!(c.guild_member_slots.is_empty() && c.state.guild_members.is_empty());
        assert_ne!(c.state.member_context_version, version);
        c.member_demand = Some(("10".into(), "20".into()));
        c.dispatch("GUILD_MEMBER_LIST_UPDATE", json!({"guild_id":"10","id":"everyone","member_count":1,"ops":[{"op":"SYNC","range":[0,0],"items":[member("3")]}]})).await.unwrap();
        assert!(c.state.guild_members.is_empty(), "The old public list is not a confirmed private list");
        assert!(c.member_list_id.is_empty());
        let acl_revision = c.state.member_context_version;
        c.reset_navigation();
        assert_ne!(c.state.member_context_version, acl_revision, "Navigation after ACL invalidation must have a distinct revision");
    }
    #[tokio::test]
    async fn own_role_change_retires_private_member_binding_but_nickname_update_does_not() {
        let (mut c, _) = Controller::test_fixture();
        member_acl_context(&mut c, false, "private-old");
        let version = c.state.member_context_version;
        c.dispatch("GUILD_MEMBER_UPDATE", json!({"guild_id":"10","user":{"id":"1"},"nick":"Synthetic nick"})).await.unwrap();
        assert_eq!(c.member_list_id, "private-old");
        assert_eq!(c.state.member_context_version, version);
        c.dispatch("GUILD_MEMBER_UPDATE", json!({"guild_id":"10","user":{"id":"1"},"roles":["31"]})).await.unwrap();
        assert!(c.state.channel_access.can_view && c.state.channel_access.can_view_members);
        assert!(c.member_list_id.is_empty());
        assert!(c.member_demand.is_none());
        assert_ne!(c.state.member_context_version, version);
        c.member_demand = Some(("10".into(), "20".into()));
        c.dispatch("GUILD_MEMBER_LIST_UPDATE", json!({"guild_id":"10","id":"private-old","member_count":1,"ops":[{"op":"SYNC","range":[0,0],"items":[member("3")]}]})).await.unwrap();
        assert!(c.state.guild_members.is_empty());
        assert!(c.member_list_id.is_empty());
    }

    #[test]
    fn explicitly_rebound_member_lists_never_merge_old_slots_or_accept_old_ids() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.member_list(&json!({"guild_id":"10","channel_id":"20","id":"old","member_count":2,"ops":[{"op":"SYNC","range":[0,1],"items":[member("2"),member("3")]}]}));
        c.member_list(&json!({"guild_id":"10","channel_id":"20","id":"new","member_count":1,"ops":[{"op":"UPDATE","index":0,"item":member("4") }]}));
        assert_eq!(c.state.guild_members.len(), 1);
        assert_eq!(c.state.guild_members[0].user.id, "4");
        c.member_list(&json!({"guild_id":"10","id":"old","member_count":2,"ops":[{"op":"INSERT","index":0,"item":member("2") }]}));
        assert_eq!(c.state.guild_members.len(), 1);
        assert_eq!(c.state.guild_members[0].user.id, "4");
    }
    #[test]
    fn unknown_member_lists_show_only_visible_channel_authors_and_never_trust_the_packet() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.member_demand = Some(("10".into(), "20".into()));
        c.state.messages = vec![
            ChatMessage { channel_id:"20".into(), author:Person{id:"2".into(), ..Default::default()}, ..Default::default() },
            ChatMessage { channel_id:"21".into(), author:Person{id:"3".into(), ..Default::default()}, ..Default::default() },
        ].into();
        c.member_list(&json!({"guild_id":"10","id":"unknown-list","member_count":999,"ops":[{"op":"SYNC","range":[0,0],"items":[member("4")]}]}));
        assert_eq!(c.state.guild_members.len(), 1);
        assert_eq!(c.state.guild_members[0].user.id, "2");
        assert_eq!(c.state.guild_members_total, 0);
        assert!(!c.state.guild_members_note.is_empty());
        assert!(c.member_list_id.is_empty());
        c.state.channel_access.can_read_history = false;
        c.member_list_fallback();
        assert!(c.state.guild_members.is_empty());
        c.reset_navigation();
        assert!(c.state.guild_members_note.is_empty());
    }

    #[test]
    fn history_permission_revocation_discards_partial_member_authors_immediately() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.member_demand = Some(("10".into(), "20".into()));
        c.guild_raw = json!({"id":"10","roles":[{"id":"10","permissions":"1024"}]});
        c.guild_acl = Some(GuildAccess::from_values(&c.guild_raw, &json!({}), "1"));
        c.channels.insert("20".into(), json!({"id":"20","guild_id":"10","type":0,"permission_overwrites":[{"id":"99","type":0,"deny":"1024","allow":"0"}]}));
        c.state.messages = vec![ChatMessage {
            channel_id: "20".into(),
            author: Person { id: "2".into(), ..Default::default() },
            ..Default::default()
        }].into();
        c.member_list_fallback();
        assert_eq!(c.state.guild_members.len(), 1);
        c.revoke_access();
        assert!(c.state.channel_access.can_view_members);
        assert!(!c.state.channel_access.can_read_history);
        assert!(c.state.messages.is_empty());
        assert!(c.state.guild_members.is_empty());
    }

    #[test]
    fn partial_call_update_keeps_participants_and_start() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.incoming_call(
            "CALL_CREATE",
            &json!({"channel_id":"30","ringing":["1"],"voice_states":[{"user_id":"9"}]}),
        );
        let old = c.state.incoming_calls[0].clone();
        c.incoming_call("CALL_UPDATE", &json!({"channel_id":"30"}));
        let call = &c.state.incoming_calls[0];
        assert_eq!(call.started_at, old.started_at);
        assert_eq!(call.participants.len(), 1);
        assert_eq!(call.participants[0].id, "9");
        assert!(call.ringing);
    }
    #[tokio::test]
    async fn stale_ack_does_not_clear_new_mentions() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.latest.insert("20".into(), "100".into());
        c.read.insert("20".into(), "90".into());
        c.mention.insert("20".into(), 2);
        c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"80"}))
            .await
            .unwrap();
        assert_eq!(c.read["20"], "90");
        assert_eq!(c.mention["20"], 2);
        c.dispatch("MESSAGE_ACK", json!({"channel_id":"20","message_id":"100"}))
            .await
            .unwrap();
        assert!(!c.mention.contains_key("20"));
    }
    #[test]
    fn reading_one_guild_channel_retires_only_its_badge_and_preserves_other_channels_and_guilds() {
        let (mut c, _) = Controller::test_fixture();
        channel(&mut c);
        c.guild_raw = json!({"id":"10","roles":[{"id":"10","permissions":"68608"}]});
        c.guild_acl = Some(GuildAccess::from_values(&c.guild_raw, &json!({}), "1"));
        for (id, last) in [("20", "100"), ("21", "102")] {
            c.channels.insert(
                id.into(),
                json!({"id":id,"guild_id":"10","type":0,"last_message_id":last}),
            );
            c.latest.insert(id.into(), last.into());
            c.read.insert(id.into(), "90".into());
            c.mention.insert(id.into(), 1);
            c.unread_seen.push_back((id.into(), last.into()));
        }
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
                mention_count: 3,
                ..Default::default()
            },
        ];
        c.mention.insert("30".into(), 3);
        c.rebuild_channels();
        assert!(c.mark_read_through("20", "100"));
        c.rebuild_channels();
        assert!(!c.mention.contains_key("20"));
        assert_eq!(c.mention["21"], 1);
        assert_eq!(c.mention["30"], 3);
        assert_eq!(c.state.guilds[0].mention_count, 1);
        assert!(c.state.guilds[0].unread);
        assert_eq!(c.state.guilds[1].mention_count, 3);
        assert!(c.state.guilds[1].unread);
        assert!(
            !c.state
                .channels
                .iter()
                .find(|item| item.id == "20")
                .unwrap()
                .unread
        );
        assert!(
            c.state
                .channels
                .iter()
                .find(|item| item.id == "21")
                .unwrap()
                .unread
        );
        assert!(!c.mark_read_through("20", "95"));
        assert_eq!(c.state.guilds[0].mention_count, 1);
        c.mark_read_through("21", "102");
        assert_eq!(c.state.guilds[0].mention_count, 0);
        assert!(!c.state.guilds[0].unread);
        assert_eq!(c.state.guilds[1].mention_count, 3);
        assert!(c.state.guilds[1].unread);
    }
}

#[cfg(test)]
mod ui12_activity_updates {
    use super::*;
    #[tokio::test]
    async fn partial_presence_keeps_game_until_explicit_stop_and_never_plays_activity_sounds() {
        let (mut controller,updates)=Controller::test_fixture();
        controller.state.guild_members=vec![ChannelMember{user:Person{id:"2".into(),name:"Player".into(),..Default::default()},..Default::default()}].into();
        controller.dispatch("PRESENCE_UPDATE",json!({"user":{"id":"2"},"status":"online","activities":[{"type":0,"name":"Game A"}]})).await.unwrap();
        assert_eq!(controller.state.guild_members[0].activities[0].name,"Game A");
        controller.dispatch("PRESENCE_UPDATE",json!({"user":{"id":"2"},"status":"idle"})).await.unwrap();
        assert_eq!(controller.state.guild_members[0].activities[0].name,"Game A");
        assert_eq!(controller.state.guild_members[0].presence,"idle");
        controller.dispatch("PRESENCE_UPDATE",json!({"user":{"id":"2"},"activities":[{"type":0,"name":"Game B"}]})).await.unwrap();
        assert_eq!(controller.state.guild_members[0].activities[0].name,"Game B");
        assert_eq!(controller.state.guild_members[0].presence,"idle");
        controller.dispatch("PRESENCE_UPDATE",json!({"user":{"id":"2"},"activities":[]})).await.unwrap();
        assert!(controller.state.guild_members[0].activities.is_empty());
        assert!(controller.state.user_presences["2"].activities.is_empty());
        assert!(updates.try_iter().all(|update|!matches!(update,UiUpdate::Data(kind,_)if kind=="sound")));
        assert!(controller.api.is_none());
    }
}
