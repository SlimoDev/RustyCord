use super::*;
use crate::sounds::{SoundEvent, SoundPreferences};

impl NotDiscordUi {
    pub(super) fn draw_sound_settings(&mut self, ctx: &egui::Context, open: &mut bool) {
        super::dialogs::centered_dialog(ctx,"Sonidos de eventos","sound-settings",egui::vec2(650.,680.))
            .open(open).show(ctx,|ui|{
                ui.label("Importar WAV, MP3 u OGG/Vorbis · efectos: 5 s / 2 MiB · timbres: 60 s / 8 MiB.");
                ui.small("Los sonidos se guardan en tu carpeta de datos y se reproducen por la salida predeterminada de Windows.");
                if self.sound_importer.busy() {ui.label("Convirtiendo sonido…");if ui.button("Cancelar importación").clicked(){self.sound_importer.cancel();}}
                ui.separator();
                theme::vertical_scroll(ui).id_salt("event-sounds").max_height(490.).show(ui,|ui|{
                    for (index,event) in SoundEvent::ALL.into_iter().enumerate() {
                        if index>0 {ui.separator();}
                        ui.push_id(event,|ui|{
                            let mut setting=self.sound_preferences.setting(event);
                            ui.horizontal(|ui|{ui.checkbox(&mut setting.enabled,event.label());});
                            ui.horizontal_wrapped(|ui|{
                                ui.add(egui::Slider::new(&mut setting.volume,0.0..=1.).text("Volumen").fixed_decimals(2));
                                if ui.add_enabled(!self.sound_importer.busy(),egui::Button::new("Archivo…")).clicked(){
                                    if let Some(path)=rfd::FileDialog::new().add_filter("Audio compatible",&["wav","mp3","ogg"]).pick_file(){
                                        let protected=self.sound_preferences.events.values().filter_map(|s|s.file.clone()).collect();
                                        if let Err(error)=self.sound_importer.start(event,path,self.data_dir.clone(),protected,ctx.clone()){self.dialog_error=error;}
                                    }
                                }
                                if ui.button("Probar").clicked(){
                                    self.preview_event_sound(event,&setting);
                                }
                                if ui.button("Parar prueba").clicked(){self.sounds.stop_preview();}
                                if ui.add_enabled(!self.sound_importer.busy(),egui::Button::new("Restaurar")).clicked(){setting=Default::default();self.sounds.clear_cache();}
                            });
                            ui.label(RichText::new(if !setting.source_name.is_empty(){setting.source_name.clone()}else{setting.file.as_ref().and_then(|p|p.file_name()).map(|p|p.to_string_lossy().into_owned()).unwrap_or_else(||"Sonido predeterminado".into())}).size(12.).color(self.theme.muted));
                            self.sound_preferences.events.insert(event,setting);
                        });
                    }
                });
                ui.separator();
                if let Some(error)=self.sounds.poll_error(){self.dialog_error=error;}
                if !self.dialog_error.is_empty(){ui.colored_label(ui.visuals().error_fg_color,&self.dialog_error);}
                ui.horizontal(|ui|{
                    if ui.add_enabled(!self.sound_importer.busy(),egui::Button::new("Guardar")).clicked(){match self.sound_preferences.save(&self.data_dir){Ok(())=>self.dialog_error="Sonidos guardados.".into(),Err(e)=>self.dialog_error=e}}
                    if ui.button("Detener prueba").clicked(){self.sounds.stop_preview();}
                });
                ui.small("Una prueba respeta el volumen elegido. Desactivar un evento silencia su reproducción automática. Guarda para conservar los cambios al reiniciar.");
            });
    }

    fn event_sound(&mut self, event: SoundEvent) {
        self.sounds.play(event, &self.sound_preferences);
    }
    fn preview_event_sound(&mut self, event: SoundEvent, setting: &crate::sounds::SoundSetting) {
        let mut preview = SoundPreferences::default();
        let mut audible = setting.clone();
        audible.enabled = true;
        preview.events.insert(event, audible);
        self.sounds.play(event, &preview);
    }
    /// The current snapshot may already target a different channel from the
    /// last rendered frame. Require both to match, and evaluate focus now.
    /// Being away from the end of history does not make this a different chat.
    pub(super) fn active_message_chat(&self, channel: &str) -> bool {
        if channel.is_empty()
            || channel != self.state.selected_channel_id
            || self.state.view != "chat"
            || self.dialog.is_some()
            || self.viewer.is_some()
        {
            return false;
        }
        let focused = self.ctx.input(|input| {
            input.focused
                && input.viewport().focused.unwrap_or(input.focused)
                && !input.viewport().minimized.unwrap_or(false)
        });
        if !focused {
            return false;
        }
        let account = self
            .state
            .current_user
            .as_ref()
            .map(|person| person.id.as_str())
            .unwrap_or("");
        self.ctx
            .data(|data| {
                data.get_temp::<(String, String, String, bool)>(egui::Id::new(
                    "last-chat-visibility",
                ))
            })
            .is_some_and(|(rendered_account, rendered_channel, _, _)| {
                !account.is_empty() && rendered_account == account && rendered_channel == channel
            })
    }
    pub(super) fn automatic_message_notice_allowed(
        &self,
        notice: &DesktopNotice,
        account: &str,
    ) -> bool {
        self.preview.is_none()
            && !self.state.demo
            && self.state.signed_in
            && notdiscord_core::parsing::valid_id(account)
            && self
                .state
                .current_user
                .as_ref()
                .is_some_and(|person| person.id == account)
            && self.prefs.windows_notifications
            && self.state.self_presence != "dnd"
            && notdiscord_core::parsing::valid_id(&notice.channel_id)
            && (notice.is_call || notdiscord_core::parsing::valid_id(&notice.message_id))
            && (notice.is_call || !self.active_message_chat(&notice.channel_id))
    }
    pub(super) fn automatic_notice(&mut self, data: Value) {
        if data["notificationEpoch"].as_u64().unwrap_or(0) != self.state.notification_epoch {
            return;
        }
        let account = s(&data, "accountId").to_owned();
        let Some(generation) = data["generation"].as_u64() else {
            return;
        };
        if generation != self.state.notification_generation { return; }
        let Ok(notice) = serde_json::from_value::<DesktopNotice>(data) else {
            return;
        };
        if !self.automatic_message_notice_allowed(&notice, &account) {
            return;
        }
        #[cfg(test)]
        self.ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<Vec<(DesktopNotice, String, u64)>>(egui::Id::new(
                "observed-automatic-notices",
            ))
            .push((notice, account, generation));
        });
        #[cfg(not(test))]
        if let Err(error) = self.notifications.show_prepared_message_for_session(
            &notice.title,
            &notice.body,
            &notice.channel_id,
            &notice.message_id,
            notice.image_path.as_deref(),
            &account,
            generation,
            self.command.clone(),
        ) {
            self.error = error;
        }
    }
    pub(super) fn automatic_sound(&mut self, data: &Value) {
        if self.preview.is_some() || self.state.demo || !self.state.signed_in {
            return;
        }
        let Some(event) = SoundEvent::parse(s(data, "event")) else {
            return;
        };
        if matches!(event, SoundEvent::Message | SoundEvent::Mention) {
            if data["session_generation"].as_u64().unwrap_or(0) != self.state.notification_generation { return; }
            if data["notification_epoch"].as_u64().unwrap_or(0) != self.state.notification_epoch {
                return;
            }
            let channel = s(data, "channel_id");
            let message = s(data, "message_id");
            let account = s(data, "account_id");
            if !notdiscord_core::parsing::valid_id(channel)
                || !notdiscord_core::parsing::valid_id(message)
                || !notdiscord_core::parsing::valid_id(account)
                || !self
                    .state
                    .current_user
                    .as_ref()
                    .is_some_and(|person| person.id == account)
            {
                return;
            }
            // Both controller delivery and UI queues may coalesce snapshots.
            // Never infer a destination from whichever chat is selected now,
            // nor replay an ignored/handled message after its focus changes.
            let key = (account.to_owned(), channel.to_owned(), message.to_owned());
            let duplicate = self.ctx.data_mut(|data| {
                let id = egui::Id::new("automatic-message-sound-seen");
                let seen = data.get_temp_mut_or_default::<VecDeque<(String, String, String)>>(id);
                if seen.contains(&key) {
                    return true;
                }
                seen.push_back(key);
                while seen.len() > 256 {
                    seen.pop_front();
                }
                false
            });
            if duplicate || self.state.self_presence == "dnd" || self.active_message_chat(channel) {
                return;
            }
        }
        self.event_sound(event);
    }
    pub(super) fn observe_sound_snapshot(&mut self, next: &AppSnapshot) {
        let old = &self.state;
        if self.preview.is_some()
            || next.demo
            || !old.signed_in
            || !next.signed_in
            || old.current_user.as_ref().map(|u| &u.id) != next.current_user.as_ref().map(|u| &u.id)
        {
            self.sounds.stop_ring();
            return;
        }
        let mut events = Vec::new();
        if !old.voice_connected && next.voice_connected {
            events.push(SoundEvent::VoiceConnected);
        }
        if old.voice_connected
            && !next.voice_connected
            && !next.voice_connecting
            && old.call_move_version == next.call_move_version
        {
            events.push(SoundEvent::VoiceDisconnected);
        }
        if old.self_mute != next.self_mute {
            events.push(if next.self_mute {
                SoundEvent::Mute
            } else {
                SoundEvent::Unmute
            });
        }
        if old.self_deaf != next.self_deaf {
            events.push(if next.self_deaf {
                SoundEvent::Deafen
            } else {
                SoundEvent::Undeafen
            });
        }
        if old.screen_sharing != next.screen_sharing {
            events.push(if next.screen_sharing {
                SoundEvent::StreamStarted
            } else {
                SoundEvent::StreamEnded
            });
        }
        if old.voice_connected
            && next.voice_connected
            && old.call_channel_id == next.call_channel_id
        {
            let moving = old.call_move_version != next.call_move_version;
            if !moving
                && next.call_participants.iter().any(|p| {
                    !p.is_self && !old.call_participants.iter().any(|o| o.user.id == p.user.id)
                })
            {
                events.push(SoundEvent::UserJoined);
            }
            if !moving
                && old.call_participants.iter().any(|p| {
                    !p.is_self
                        && !next
                            .call_participants
                            .iter()
                            .any(|o| o.user.id == p.user.id)
                })
            {
                events.push(SoundEvent::UserLeft);
            }
            for participant in next.call_participants.iter().filter(|p| !p.is_self) {
                if let Some(previous) = old
                    .call_participants
                    .iter()
                    .find(|p| p.user.id == participant.user.id)
                {
                    if previous.screen_sharing != participant.screen_sharing {
                        events.push(if participant.screen_sharing {
                            SoundEvent::StreamStarted
                        } else {
                            SoundEvent::StreamEnded
                        });
                    }
                }
            }
        } else if old.voice_connected
            && next.voice_connected
            && old.call_channel_id != next.call_channel_id
        {
            events.push(SoundEvent::UserMove);
        }
        events.dedup();
        for event in events {
            self.event_sound(event);
        }
    }
    pub(super) fn sync_ring_sound(&mut self, state: &AppSnapshot) {
        if self.preview.is_some() || state.demo || !state.signed_in || state.self_presence == "dnd"
        {
            self.sounds.stop_ring();
            return;
        }
        let incoming = self.incoming_popup.audible_call(state).map(|call| {
            (
                format!("in:{}:{}", call.channel_id, call.instance_id),
                SoundEvent::IncomingRing,
            )
        });
        let desired = incoming.or_else(||crate::sounds::outgoing_ring_token(state));
        self.sounds.set_ring(desired, &self.sound_preferences);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn initial() -> AppSnapshot {
        AppSnapshot {
            signed_in: true,
            notification_generation: 7,
            current_user: Some(Person {
                id: "1".into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    fn visible_chat(app: &mut NotDiscordUi, channel: &str) {
        app.state = initial();
        app.state.view = "chat".into();
        app.state.selected_channel_id = channel.into();
        app.ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("last-chat-visibility"),
                ("1".to_owned(), channel.to_owned(), "90".to_owned(), false),
            )
        });
    }
    fn sound_packet(event: &str, channel: &str, message: &str) -> Value {
        serde_json::json!({"event":event,"channel_id":channel,"message_id":message,"account_id":"1","session_generation":7})
    }
    fn notice_packet(account: &str, generation: u64, channel: &str, message: &str) -> Value {
        let notice = DesktopNotice {
            channel_id: channel.into(),
            message_id: message.into(),
            title: "Synthetic peer".into(),
            body: "Offline notice fixture".into(),
            ..Default::default()
        };
        let mut value = serde_json::to_value(notice).unwrap();
        value["accountId"] = serde_json::json!(account);
        value["generation"] = serde_json::json!(generation);
        value
    }
    fn observed_notices(app: &NotDiscordUi) -> Vec<(DesktopNotice, String, u64)> {
        app.ctx
            .data(|data| data.get_temp(egui::Id::new("observed-automatic-notices")))
            .unwrap_or_default()
    }
    #[test]
    fn queued_notice_for_account_a_is_rejected_after_a_coalesced_snapshot_of_b() {
        let (mut app, updates, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        app.prefs.windows_notifications = true;
        updates
            .send(UiUpdate::Data(
                "notice".into(),
                notice_packet("1", 7, "20", "100"),
            ))
            .unwrap();
        let mut account_b = initial();
        account_b.notification_generation = 8;
        account_b.current_user.as_mut().unwrap().id = "2".into();
        account_b.view = "chat".into();
        account_b.selected_channel_id = "30".into();
        app.latest_snapshot = Some(std::sync::Arc::new(std::sync::Mutex::new(Some(account_b))));
        let ctx = app.ctx.clone();
        let _ = ctx.run(
            egui::RawInput {
                focused: false,
                ..Default::default()
            },
            |ctx| app.process_updates(ctx),
        );
        assert_eq!(app.state.current_user.as_ref().unwrap().id, "2");
        assert!(
            observed_notices(&app).is_empty(),
            "A queued notice cannot disclose account A content in account B"
        );
        app.data_update("notice", notice_packet("2", 8, "30", "101"));
        let notices = observed_notices(&app);
        assert_eq!(notices.len(), 1);
        assert_eq!(
            (
                notices[0].0.channel_id.as_str(),
                notices[0].1.as_str(),
                notices[0].2
            ),
            ("30", "2", 8)
        );
    }
    #[test]
    fn notices_require_captured_origin_and_recheck_the_active_chat_when_dispatched() {
        let (mut app, _, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        app.prefs.windows_notifications = true;
        sound_frame(&mut app, true, false, &[]);
        app.data_update("notice", notice_packet("1", 7, "20", "100"));
        assert!(observed_notices(&app).is_empty());
        sound_frame(&mut app, false, false, &[]);
        let mut missing_account = notice_packet("1", 7, "20", "101");
        missing_account.as_object_mut().unwrap().remove("accountId");
        let mut missing_generation = notice_packet("1", 7, "20", "102");
        missing_generation
            .as_object_mut()
            .unwrap()
            .remove("generation");
        for value in [
            missing_account,
            missing_generation,
            notice_packet("2", 7, "20", "103"),
        ] {
            app.data_update("notice", value);
        }
        assert!(observed_notices(&app).is_empty());
        let mut prepared = notice_packet("1", 7, "20", "104");
        prepared["ImagePath"] = serde_json::json!("E:\\synthetic-only.png");
        app.data_update("notice", prepared);
        let notices = observed_notices(&app);
        assert_eq!(notices.len(), 1);
        assert_eq!(
            notices[0].0.image_path.as_deref(),
            Some("E:\\synthetic-only.png")
        );
        assert_eq!((notices[0].1.as_str(), notices[0].2), ("1", 7));
    }
    fn sound_frame(app: &mut NotDiscordUi, focused: bool, minimized: bool, packets: &[Value]) {
        let ctx = app.ctx.clone();
        let mut input = egui::RawInput {
            focused,
            ..Default::default()
        };
        let viewport = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        viewport.focused = Some(focused);
        viewport.minimized = Some(minimized);
        let _ = ctx.run(input, |_| {
            for packet in packets {
                app.data_update("sound", packet.clone());
            }
        });
    }
    #[test]
    fn active_chat_silences_both_message_events_even_away_from_the_bottom() {
        let (mut app, _, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        sound_frame(
            &mut app,
            true,
            false,
            &[
                sound_packet("message", "20", "100"),
                sound_packet("mention", "20", "101"),
                sound_packet("message", "30", "102"),
                sound_packet("mention", "30", "103"),
            ],
        );
        assert_eq!(
            app.sounds.observed,
            vec![SoundEvent::Message, SoundEvent::Mention],
            "Only events for the other channel may reach the audio engine"
        );
    }
    #[test]
    fn background_minimized_and_nonchat_views_keep_message_notifications() {
        for (view, focused, minimized) in [
            ("chat", false, false),
            ("chat", true, true),
            ("search", true, false),
            ("pins", true, false),
            ("history", true, false),
            ("friends", true, false),
        ] {
            let (mut app, _, _) = super::super::tests::harness();
            visible_chat(&mut app, "20");
            app.state.view = view.into();
            sound_frame(
                &mut app,
                focused,
                minimized,
                &[
                    sound_packet("message", "20", "100"),
                    sound_packet("mention", "20", "101"),
                ],
            );
            assert_eq!(
                app.sounds.observed,
                vec![SoundEvent::Message, SoundEvent::Mention],
                "view={view}, focused={focused}, minimized={minimized}"
            );
        }
    }
    #[test]
    fn advanced_snapshots_do_not_substitute_the_channel_or_replay_handled_messages() {
        let (mut app, _, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        // A snapshot has navigated ahead of the last actually rendered chat.
        app.state.selected_channel_id = "30".into();
        sound_frame(
            &mut app,
            true,
            false,
            &[sound_packet("message", "30", "100")],
        );
        assert_eq!(app.sounds.observed, vec![SoundEvent::Message]);
        app.sounds.observed.clear();
        visible_chat(&mut app, "20");
        let handled = sound_packet("mention", "20", "101");
        sound_frame(&mut app, true, false, &[handled.clone()]);
        assert!(app.sounds.observed.is_empty());
        sound_frame(
            &mut app,
            false,
            false,
            &[handled.clone(), sound_packet("message", "20", "101")],
        );
        assert!(
            app.sounds.observed.is_empty(),
            "A duplicate stays handled after focus changes and across Message/Mention classification"
        );
        sound_frame(
            &mut app,
            false,
            false,
            &[sound_packet("mention", "20", "102")],
        );
        assert_eq!(app.sounds.observed, vec![SoundEvent::Mention]);
    }
    #[test]
    fn notification_sounds_require_real_channel_message_and_current_account() {
        let (mut app, _, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        let mut wrong_account = sound_packet("message", "30", "100");
        wrong_account["account_id"] = serde_json::json!("2");
        sound_frame(
            &mut app,
            false,
            false,
            &[
                serde_json::json!({"event":"message"}),
                sound_packet("mention", "", "101"),
                sound_packet("message", "30", ""),
                wrong_account,
            ],
        );
        assert!(app.sounds.observed.is_empty());
        app.state.self_presence = "dnd".into();
        sound_frame(
            &mut app,
            false,
            false,
            &[
                sound_packet("message", "30", "102"),
                sound_packet("mention", "30", "103"),
            ],
        );
        assert!(app.sounds.observed.is_empty());
        app.state.self_presence = "online".into();
        sound_frame(
            &mut app,
            false,
            false,
            &[sound_packet("message", "30", "104")],
        );
        assert_eq!(app.sounds.observed, vec![SoundEvent::Message]);
    }
    #[test]
    fn dnd_delivery_epoch_rejects_queued_audio_and_windows_notice_then_accepts_new_events() {
        let (mut app,_,_) = super::super::tests::harness();
        visible_chat(&mut app,"20");
        app.prefs.windows_notifications=true;
        app.state.notification_epoch=1;
        let old_sound=sound_packet("message","30","100");
        let mut new_sound=sound_packet("message","30","101");
        new_sound["notification_epoch"]=serde_json::json!(1);
        sound_frame(&mut app,false,false,&[old_sound,new_sound]);
        assert_eq!(app.sounds.observed,vec![SoundEvent::Message]);
        let mut packet=serde_json::to_value(DesktopNotice{channel_id:"30".into(),message_id:"102".into(),..Default::default()}).unwrap();
        packet["accountId"]=serde_json::json!("1");packet["generation"]=serde_json::json!(7);
        app.automatic_notice(packet.clone());
        let seen=egui::Id::new("observed-automatic-notices");
        assert!(app.ctx.data(|d|d.get_temp::<Vec<(DesktopNotice,String,u64)>>(seen)).is_none());
        packet["notificationEpoch"]=serde_json::json!(1);
        app.automatic_notice(packet);
        assert_eq!(app.ctx.data(|d|d.get_temp::<Vec<(DesktopNotice,String,u64)>>(seen)).unwrap().len(),1);
    }
    #[test]
    fn ordered_policy_barrier_restores_immediate_notifications_before_the_next_snapshot() {
        let (mut app, updates, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        app.prefs.windows_notifications = true;
        app.state.self_presence = "dnd".into();
        app.state.notification_epoch = 1;
        let old_dnd = app.state.clone();
        app.latest_snapshot = Some(std::sync::Arc::new(std::sync::Mutex::new(Some(old_dnd.clone()))));
        updates.send(UiUpdate::Data("notification-policy".into(), serde_json::json!({"account_id":"1","presence":"online","epoch":1,"generation":7}))).unwrap();
        let mut sound = sound_packet("message", "30", "101");
        sound["notification_epoch"] = serde_json::json!(1);
        let mut notice = notice_packet("1", 7, "30", "101");
        notice["notificationEpoch"] = serde_json::json!(1);
        updates.send(UiUpdate::Data("sound".into(), sound)).unwrap();
        updates.send(UiUpdate::Data("notice".into(), notice)).unwrap();
        let ctx = app.ctx.clone();
        let _ = ctx.run(egui::RawInput { focused:false, ..Default::default() }, |ctx| app.process_updates(ctx));
        assert_eq!(app.state.self_presence, "online");
        assert_eq!(app.sounds.observed, vec![SoundEvent::Message]);
        assert_eq!(observed_notices(&app).len(), 1);
        // A snapshot flushed just before that barrier may only be taken by
        // the following UI frame. It must not restore the obsolete DND policy.
        let mut delayed = old_dnd;
        delayed.selected_channel_id = "40".into();
        *app.latest_snapshot.as_ref().unwrap().lock().unwrap() = Some(delayed);
        let mut after_delayed_snapshot = sound_packet("message", "30", "104");
        after_delayed_snapshot["notification_epoch"] = serde_json::json!(1);
        updates.send(UiUpdate::Data("sound".into(), after_delayed_snapshot)).unwrap();
        let _ = ctx.run(egui::RawInput { focused:false, ..Default::default() }, |ctx| app.process_updates(ctx));
        assert_eq!(app.state.self_presence, "online");
        assert_eq!(app.state.selected_channel_id, "40", "the barrier retains only notification policy");
        assert_eq!(app.sounds.observed, vec![SoundEvent::Message, SoundEvent::Message]);
        // Entering and leaving DND may coalesce into one visible snapshot.
        // Packets queued before entry cannot revive after the exit barrier.
        updates.send(UiUpdate::Data("notification-policy".into(), serde_json::json!({"account_id":"1","presence":"dnd","epoch":2,"generation":7}))).unwrap();
        updates.send(UiUpdate::Data("notification-policy".into(), serde_json::json!({"account_id":"1","presence":"online","epoch":2,"generation":7}))).unwrap();
        let mut stale = sound_packet("message", "30", "102"); stale["notification_epoch"] = serde_json::json!(1);
        updates.send(UiUpdate::Data("sound".into(), stale)).unwrap();
        let mut fresh = sound_packet("message", "30", "103"); fresh["notification_epoch"] = serde_json::json!(2);
        updates.send(UiUpdate::Data("sound".into(), fresh)).unwrap();
        let _ = ctx.run(egui::RawInput { focused:false, ..Default::default() }, |ctx| app.process_updates(ctx));
        assert_eq!(app.sounds.observed, vec![SoundEvent::Message, SoundEvent::Message, SoundEvent::Message]);
        assert_eq!(app.state.notification_epoch, 2);
    }
    #[test]
    fn old_policy_and_notice_packets_cannot_cross_a_same_account_relogin() {
        let (mut app, updates, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        app.prefs.windows_notifications = true;
        app.data_update("notification-policy", serde_json::json!({"account_id":"1","presence":"dnd","epoch":9,"generation":7}));
        assert_eq!(app.state.self_presence, "dnd");
        let mut relogged = initial();
        relogged.notification_generation = 8;
        relogged.self_presence = "online".into();
        relogged.view = "chat".into();
        relogged.selected_channel_id = "20".into();
        app.latest_snapshot = Some(std::sync::Arc::new(std::sync::Mutex::new(Some(relogged))));
        updates.send(UiUpdate::Data("notification-policy".into(), serde_json::json!({"account_id":"1","presence":"dnd","epoch":10,"generation":7}))).unwrap();
        let mut old_sound = sound_packet("message", "30", "101");
        old_sound["notification_epoch"] = serde_json::json!(9);
        updates.send(UiUpdate::Data("sound".into(), old_sound)).unwrap();
        let mut old_notice = notice_packet("1", 7, "30", "101");
        old_notice["notificationEpoch"] = serde_json::json!(9);
        updates.send(UiUpdate::Data("notice".into(), old_notice)).unwrap();
        let ctx = app.ctx.clone();
        let _ = ctx.run(egui::RawInput { focused:false, ..Default::default() }, |ctx| app.process_updates(ctx));
        assert_eq!(app.state.self_presence, "online");
        assert_eq!(app.state.notification_generation, 8);
        assert_eq!(app.state.notification_epoch, 0);
        assert!(app.sounds.observed.is_empty());
        assert!(observed_notices(&app).is_empty());
        let mut fresh_sound = sound_packet("message", "30", "102");
        fresh_sound["session_generation"] = serde_json::json!(8);
        updates.send(UiUpdate::Data("notification-policy".into(), serde_json::json!({"account_id":"1","presence":"idle","epoch":0,"generation":8}))).unwrap();
        updates.send(UiUpdate::Data("sound".into(), fresh_sound)).unwrap();
        updates.send(UiUpdate::Data("notice".into(), notice_packet("1", 8, "30", "102"))).unwrap();
        let _ = ctx.run(egui::RawInput { focused:false, ..Default::default() }, |ctx| app.process_updates(ctx));
        assert_eq!(app.state.self_presence, "idle");
        assert_eq!(app.sounds.observed, vec![SoundEvent::Message]);
        assert_eq!(observed_notices(&app).len(), 1);
    }
    #[test]
    fn deferred_message_notice_rechecks_focus_channel_and_visibility_but_calls_remain_independent()
    {
        let (mut app, _, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        app.prefs.windows_notifications = true;
        let notice = DesktopNotice {
            channel_id: "20".into(),
            message_id: "100".into(),
            ..Default::default()
        };
        for (focused, minimized, expected) in [
            (true, false, false),
            (false, false, true),
            (true, true, true),
        ] {
            sound_frame(&mut app, focused, minimized, &[]);
            assert_eq!(app.automatic_message_notice_allowed(&notice, "1"), expected);
        }
        sound_frame(&mut app, true, false, &[]);
        let mut other = notice.clone();
        other.channel_id = "30".into();
        assert!(app.automatic_message_notice_allowed(&other, "1"));
        let mut call = notice.clone();
        call.is_call = true;
        call.message_id.clear();
        assert!(app.automatic_message_notice_allowed(&call, "1"));
        app.state.view = "search".into();
        assert!(app.automatic_message_notice_allowed(&notice, "1"));
        app.state.view = "chat".into();
        app.dialog = Some(Dialog::Sounds);
        assert!(
            app.automatic_message_notice_allowed(&notice, "1"),
            "An overlay covering the chat does not count as viewing it"
        );
        app.state.self_presence = "dnd".into();
        assert!(!app.automatic_message_notice_allowed(&notice, "1"));
        assert!(!app.automatic_message_notice_allowed(&call, "1"));
    }
    #[test]
    fn voice_events_and_manual_sound_tests_bypass_active_chat_suppression() {
        let (mut app, _, _) = super::super::tests::harness();
        visible_chat(&mut app, "20");
        sound_frame(
            &mut app,
            true,
            false,
            &[
                serde_json::json!({"event":"voice_connected"}),
                serde_json::json!({"event":"ptt_activate"}),
                serde_json::json!({"event":"stream_viewer_joined"}),
            ],
        );
        assert_eq!(
            app.sounds.observed,
            vec![
                SoundEvent::VoiceConnected,
                SoundEvent::PttActivate,
                SoundEvent::StreamViewerJoined
            ]
        );
        app.sounds.observed.clear();
        app.state.self_presence = "dnd".into();
        let setting = crate::sounds::SoundSetting {
            enabled: false,
            ..Default::default()
        };
        app.preview_event_sound(SoundEvent::Message, &setting);
        app.preview_event_sound(SoundEvent::Mention, &setting);
        assert_eq!(
            app.sounds.observed,
            vec![SoundEvent::Message, SoundEvent::Mention]
        );
    }
    #[test]
    fn snapshot_events_are_transitions_and_seed_participants_are_silent() {
        let (mut app, _, _) = super::super::tests::harness();
        app.state = initial();
        let mut next = app.state.clone();
        next.voice_connected = true;
        next.call_channel_id = "50".into();
        next.call_participants = vec![CallParticipant {
            user: Person {
                id: "2".into(),
                ..Default::default()
            },
            ..Default::default()
        }];
        app.observe_sound_snapshot(&next);
        assert_eq!(app.sounds.observed, vec![SoundEvent::VoiceConnected]);
        app.sounds.observed.clear();
        app.state = next.clone();
        app.observe_sound_snapshot(&next);
        assert!(app.sounds.observed.is_empty());
        next.call_participants.push(CallParticipant {
            user: Person {
                id: "3".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        app.observe_sound_snapshot(&next);
        assert_eq!(app.sounds.observed, vec![SoundEvent::UserJoined]);
        app.sounds.observed.clear();
        app.state = next.clone();
        next.call_participants.pop();
        next.self_mute = true;
        next.self_deaf = true;
        next.screen_sharing = true;
        app.observe_sound_snapshot(&next);
        assert_eq!(
            app.sounds.observed,
            vec![
                SoundEvent::Mute,
                SoundEvent::Deafen,
                SoundEvent::StreamStarted,
                SoundEvent::UserLeft
            ]
        );
    }
    #[test]
    fn demo_account_switch_and_signout_never_play_event_audio() {
        let (mut app, _, _) = super::super::tests::harness();
        app.state = initial();
        for mut next in [initial(), initial(), initial()] {
            next.voice_connected = true;
            next.demo = true;
            app.observe_sound_snapshot(&next);
            next.demo = false;
            next.current_user.as_mut().unwrap().id = "other".into();
            app.observe_sound_snapshot(&next);
            next.signed_in = false;
            app.observe_sound_snapshot(&next);
        }
        assert!(app.sounds.observed.is_empty());
    }
}
