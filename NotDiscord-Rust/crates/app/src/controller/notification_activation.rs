//! A toast restores the existing window before its navigation can run.
use super::*;
impl Controller {
    pub(super) fn request_notification_activation(
        &mut self,
        action: &UiAction,
    ) -> Result<(), String> {
        let channel = Self::valid(&action.id)?;
        let Ok(origin) = serde_json::from_str::<Value>(&action.extra) else {
            return Ok(());
        };
        if !self.state.signed_in
            || !parsing::valid_id(&s(&origin, "accountId"))
            || s(&origin, "accountId") != self.me()
            || origin["generation"].as_u64() != Some(self.generation)
        {
            return Ok(());
        }
        let version = self.next_request("notification-activation");
        self.data("notification-activate", json!({"channelId":channel,"accountId":self.me(),"generation":self.generation,"navigationVersion":self.nav,"requestVersion":version}));
        Ok(())
    }
    fn current_notification_activation(&self, action: &UiAction) -> bool {
        let Ok(context) = serde_json::from_str::<Value>(&action.extra) else {
            return false;
        };
        context["channelId"].as_str() == Some(action.id.as_str())
            && context["accountId"].as_str() == Some(self.me().as_str())
            && context["generation"].as_u64() == Some(self.generation)
            && context["navigationVersion"].as_u64() == Some(self.nav)
            && context["requestVersion"]
                .as_u64()
                .is_some_and(|version| self.request_current("notification-activation", version))
    }
    pub(super) fn navigate_notification(&mut self, mut action: UiAction) -> Result<(), String> {
        if !self.current_notification_activation(&action) {
            return Ok(());
        }
        let id = Self::valid(&action.id)?.to_owned();
        if self
            .state
            .direct_messages
            .iter()
            .chain(&self.state.channels)
            .any(|channel| channel.id == id)
        {
            self.select_channel(&id)?;
        } else {
            let api = self.api()?;
            action.kind = "__notification-channel".into();
            // A later manual navigation cancels resolution of an older toast.
            self.launch(action, Some(self.nav), async move {
                api.get(&format!("channels/{id}"))
                    .await
                    .map_err(|error| error.to_string())
            });
        }
        Ok(())
    }
    pub(super) fn complete_notification_channel(
        &mut self,
        action: &UiAction,
        value: &Value,
    ) -> Result<(), String> {
        if !self.current_notification_activation(action) {
            return Ok(());
        }
        let channel = parsing::channel(value);
        if channel.id != action.id || !parsing::valid_id(&channel.id) {
            return Err("Discord no confirmó la conversación de la notificación.".into());
        }
        if matches!(channel.kind.as_str(), "dm" | "group") {
            self.recipients(value);
            self.state
                .direct_messages
                .retain(|item| item.id != channel.id);
            self.state.direct_messages.push(channel.clone());
            self.select_channel(&channel.id)?;
        } else {
            let guild = s(value, "guild_id");
            if !self.state.guilds.iter().any(|item| item.id == guild) {
                return Err("La conversación de la notificación ya no está disponible.".into());
            }
            self.select_guild(&guild)?;
            self.restore_channel = channel.id;
            let version = self
                .request_versions
                .get("notification-activation")
                .copied()
                .unwrap_or(0);
            self.resolved_notification_channel = Some((version, value.clone()));
        }
        Ok(())
    }
    pub(super) fn restore_resolved_notification_channel(
        &mut self,
        explicit: &str,
    ) -> Result<(), String> {
        let Some((version, value)) = self.resolved_notification_channel.take() else {
            return Ok(());
        };
        if !self.request_current("notification-activation", version)
            || explicit.is_empty()
            || s(&value, "id") != explicit
            || s(&value, "guild_id") != self.state.selected_guild_id
            || !parsing::valid_id(explicit)
        {
            return Err("El destino de la notificación cambió.".into());
        }
        // Guild channel lists omit threads. Retain the validated endpoint object
        // and evaluate it with the newly loaded parent/ACL instead of falling back.
        if !self.channel_access(&value).can_view {
            return Err("No se pudo confirmar acceso a la conversación de la notificación.".into());
        }
        self.channels.insert(explicit.into(), value);
        Ok(())
    }
    pub(super) fn discard_superseded_notification_guild(&mut self) -> bool {
        let superseded = self
            .resolved_notification_channel
            .as_ref()
            .is_some_and(|(version, _)| !self.request_current("notification-activation", *version));
        if superseded {
            self.resolved_notification_channel = None;
            self.restore_channel.clear();
        }
        superseded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn activation(
        c: &mut Controller,
        updates: &std::sync::mpsc::Receiver<UiUpdate>,
        channel: &str,
    ) -> UiAction {
        if c.state.current_user.is_none() {
            c.state.current_user = Some(Person {
                id: "1".into(),
                ..Default::default()
            });
        }
        c.state.signed_in = true;
        let request =
            crate::platform::notification_open_action(channel, &c.me(), c.generation).unwrap();
        c.request_notification_activation(&request).unwrap();
        let payload = updates
            .try_iter()
            .find_map(|update| match update {
                UiUpdate::Data(kind, data) if kind == "notification-activate" => Some(data),
                _ => None,
            })
            .unwrap();
        let mut action = UiAction::id("notification-navigate", channel);
        action.extra = payload.to_string();
        action
    }
    #[tokio::test]
    async fn toast_requires_a_window_ack_before_selecting_its_validated_chat() {
        let (mut c, updates) = Controller::test_fixture();
        c.state.signed_in = true;
        c.state.current_user = Some(Person {
            id: "1".into(),
            ..Default::default()
        });
        c.state.view = "friends".into();
        c.state.selected_guild_id = "10".into();
        c.state.channels = vec![NavItem {
            id: "20".into(),
            kind: "text".into(),
            name: "Synthetic target".into(),
            ..Default::default()
        }];
        c.channels
            .insert("20".into(), json!({"id":"20","guild_id":"10","type":0}));
        // VIEW_CHANNEL without READ_MESSAGE_HISTORY exercises real selection
        // without constructing an API, scheduling a request, or using an account.
        c.guild_acl = Some(GuildAccess::from_values(
            &json!({"id":"10","roles":[{"id":"10","permissions":"1024"}]}),
            &json!({"roles":[]}),
            "1",
        ));
        let click = crate::platform::notification_open_action("20", "1", c.generation).unwrap();
        c.act(click).await.unwrap();
        assert!(c.state.selected_channel_id.is_empty());
        assert_eq!(c.state.view, "friends");
        let payload = updates
            .try_iter()
            .find_map(|update| match update {
                UiUpdate::Data(kind, data) if kind == "notification-activate" => Some(data),
                _ => None,
            })
            .unwrap();
        let mut ack = UiAction::id("notification-navigate", "20");
        ack.extra = payload.to_string();
        c.act(ack).await.unwrap();
        assert_eq!(
            (c.state.view.as_str(), c.state.selected_channel_id.as_str()),
            ("chat", "20")
        );
        assert!(c.state.channel_access.can_view);
        assert!(!c.state.channel_access.can_read_history);
        assert!(c.api.is_none());
        assert!(
            c.act(UiAction::id("notification-open", "20/other"))
                .await
                .is_err()
        );
        assert!(
            !updates.try_iter().any(
                |update| matches!(update,UiUpdate::Data(kind,_)if kind=="notification-activate")
            )
        );
    }
    #[test]
    fn stale_click_navigation_account_and_wrong_channel_responses_cannot_redirect() {
        let (mut c, updates) = Controller::test_fixture();
        let old = activation(&mut c, &updates, "20");
        let fresh = activation(&mut c, &updates, "21");
        assert!(!c.current_notification_activation(&old));
        assert!(c.current_notification_activation(&fresh));
        assert!(
            c.complete_notification_channel(&fresh, &json!({"id":"22","type":1}))
                .is_err()
        );
        assert!(c.state.direct_messages.is_empty());
        c.reset_navigation();
        assert!(!c.current_notification_activation(&fresh));
        let other = activation(&mut c, &updates, "23");
        c.generation = c.generation.wrapping_add(1);
        assert!(!c.current_notification_activation(&other));
        c.complete_notification_channel(&old, &json!({"id":"20","type":1}))
            .unwrap();
        assert!(c.state.selected_channel_id.is_empty());
    }
    #[tokio::test]
    async fn an_old_toast_cannot_borrow_the_new_account_or_session_identity() {
        let (mut c, updates) = Controller::test_fixture();
        c.state.signed_in = true;
        c.state.current_user = Some(Person {
            id: "2".into(),
            ..Default::default()
        });
        c.generation = 8;
        for click in [
            crate::platform::notification_open_action("20", "1", 7).unwrap(),
            crate::platform::notification_open_action("20", "2", 7).unwrap(),
            UiAction::id("notification-open", "20"),
        ] {
            c.act(click).await.unwrap();
        }
        assert!(
            !updates.try_iter().any(
                |update| matches!(update,UiUpdate::Data(kind,_)if kind=="notification-activate")
            )
        );
        assert!(c.state.selected_channel_id.is_empty());
    }
    #[tokio::test]
    async fn a_resolved_thread_is_opened_exactly_even_when_the_guild_list_omits_it() {
        let (mut c, _) = Controller::test_fixture();
        c.state.signed_in = true;
        c.state.current_user = Some(Person {
            id: "1".into(),
            ..Default::default()
        });
        c.state.selected_guild_id = "10".into();
        c.restore_channel = "30".into();
        let version = c.next_request("notification-activation");
        c.resolved_notification_channel = Some((
            version,
            json!({"id":"30","guild_id":"10","type":11,"parent_id":"20","name":"Exact synthetic thread"}),
        ));
        c.complete(UiAction::id("__guild","10"),json!({"guild":{"id":"10","roles":[{"id":"10","permissions":"1024"}]},"member":{"roles":[]},"channels":[{"id":"20","guild_id":"10","type":0,"name":"Other channel"}]})).await.unwrap();
        assert_eq!(c.state.selected_channel_id, "30");
        assert_eq!(c.state.selected_channel_kind, "thread");
        assert!(c.api.is_none());
        assert!(c.resolved_notification_channel.is_none());
        // Missing parent means access is unknown; never choose the other channel.
        c.reset_navigation();
        c.state.selected_channel_id.clear();
        c.restore_channel = "31".into();
        c.resolved_notification_channel = Some((
            version,
            json!({"id":"31","guild_id":"10","type":11,"parent_id":"404"}),
        ));
        assert!(c.complete(UiAction::id("__guild","10"),json!({"guild":{"id":"10","roles":[{"id":"10","permissions":"1024"}]},"member":{"roles":[]},"channels":[{"id":"20","guild_id":"10","type":0}]})).await.is_err());
        assert!(c.state.selected_channel_id.is_empty());
    }
    #[tokio::test]
    async fn a_new_toast_stays_current_when_an_older_toasts_guild_load_finishes() {
        let (mut c, updates) = Controller::test_fixture();
        let old = activation(&mut c, &updates, "30");
        let version = serde_json::from_str::<Value>(&old.extra).unwrap()["requestVersion"]
            .as_u64()
            .unwrap();
        c.state.selected_guild_id = "10".into();
        c.restore_channel = "30".into();
        c.resolved_notification_channel = Some((
            version,
            json!({"id":"30","guild_id":"10","type":11,"parent_id":"20"}),
        ));
        let fresh = activation(&mut c, &updates, "40");
        let nav = c.nav;
        c.complete(UiAction::id("__guild","10"),json!({"guild":{"id":"10","roles":[{"id":"10","permissions":"1024"}]},"member":{"roles":[]},"channels":[{"id":"20","guild_id":"10","type":0}]})).await.unwrap();
        assert_eq!(
            c.nav, nav,
            "The superseded guild completion must not invalidate the newer window ACK"
        );
        assert!(c.current_notification_activation(&fresh));
        assert!(c.state.selected_channel_id.is_empty());
        assert!(c.guild_raw.is_null());
        assert!(c.restore_channel.is_empty() && c.resolved_notification_channel.is_none());
    }
}
