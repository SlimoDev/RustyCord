use super::*;
impl NotDiscordUi {
    pub(super) fn activate_message_notification(&mut self, data: Value) {
        let channel = s(&data, "channelId");
        if !notdiscord_core::parsing::valid_id(channel)
            || !self.state.signed_in
            || self
                .state
                .current_user
                .as_ref()
                .is_none_or(|user| user.id != s(&data, "accountId"))
            || data["generation"].as_u64().is_none()
            || data["navigationVersion"].as_u64().is_none()
            || data["requestVersion"].as_u64().is_none()
        {
            return;
        }
        // A modal belonging to the previous conversation must not cover the
        // chat selected by this deliberate Windows notification activation.
        self.dialog = None;
        self.viewer = None;
        self.picker = None;
        self.search_open = false;
        self.search_focused = false;
        self.ctx.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
        crate::platform::restore_notification_window(&self.ctx);
        // The actor can navigate only after the UI thread restored its window.
        let mut action = UiAction::id("notification-navigate", channel);
        action.extra = data.to_string();
        let _ = self.command.send(action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_valid_target_can_restore_then_acknowledge_navigation() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        app.state.signed_in = true;
        app.state.current_user = Some(Person {
            id: "1".into(),
            ..Default::default()
        });
        app.state.selected_channel_id = "20".into();
        app.state.view = "chat".into();
        app.last_channel = "20".into();
        app.last_view = "chat".into();
        app.search_open = true;
        app.search_focused = true;
        let ctx = app.ctx.clone();
        let payload = serde_json::json!({"channelId":"20","accountId":"1","generation":7,"navigationVersion":2,"requestVersion":3});
        let output = ctx.run(egui::RawInput::default(), |_| {
            ctx.memory_mut(|memory| {
                memory.request_focus(egui::Id::new("offline-notification-search"))
            });
            app.data_update("notification-activate", payload.clone());
            assert!(!app.search_open && !app.search_focused);
            assert!(ctx.memory(|memory| memory.focused()).is_none());
        });
        let viewport = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(
            viewport
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Minimized(false)))
        );
        assert!(
            viewport
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Maximized(true)))
        );
        assert!(
            viewport
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Focus))
        );
        let action = commands.try_recv().unwrap();
        assert_eq!(
            (action.kind.as_str(), action.id.as_str()),
            ("notification-navigate", "20")
        );
        assert_eq!(
            serde_json::from_str::<Value>(&action.extra).unwrap(),
            payload
        );
        assert!(commands.try_recv().is_err());
        let output = ctx.run(egui::RawInput::default(), |_| {
            app.data_update("notification-activate", serde_json::json!({"channelId":"20/evil","accountId":"1","generation":7,"navigationVersion":2,"requestVersion":3}));
        });
        assert!(commands.try_recv().is_err());
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .is_empty()
        );
        app.state.current_user.as_mut().unwrap().id = "2".into();
        app.search_open = true;
        let output = ctx.run(egui::RawInput::default(), |_| {
            app.data_update("notification-activate", payload.clone())
        });
        assert!(commands.try_recv().is_err());
        assert!(app.search_open);
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .is_empty()
        );
    }
}
