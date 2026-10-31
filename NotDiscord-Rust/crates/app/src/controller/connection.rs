//! One recovery owner per session: the Gateway task keeps its resume state and
//! attempts, and this side only mirrors it and holds back optional traffic.
use super::*;
use notdiscord_core::{
    diagnostics::{self as log, Code, Component, Phase},
    gateway::ConnectionChange,
};

pub(super) const OFFLINE: &str = "Sin conexión con Discord. Podrás intentarlo cuando se recupere.";
const RESTART_COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct Recovery {
    /// The Gateway task ended by itself; only then a new task may replace it.
    restart_at: Option<Instant>,
    restart: bool,
    /// A visible page could not load while the transport was down.
    deferred_view: bool,
}
impl Recovery {
    pub(super) fn defer_view(&mut self) { self.deferred_view = true; }
}
impl Controller {
    pub(super) fn transport_suspended(&self) -> bool {
        self.state.connection_phase.suspended()
    }
    pub(super) fn connection_changed(&mut self, change: ConnectionChange) {
        let previous = self.state.connection_phase;
        // A replacement task opens normally; the session is still recovering.
        let phase = if change.phase == ConnectionPhase::Connecting && previous.lost() {
            ConnectionPhase::Recovering
        } else {
            change.phase
        };
        self.state.connection_phase = phase;
        self.state.connection_retry_at = (phase == ConnectionPhase::Cooldown).then(|| retry_stamp(change.wait));
        if phase.lost() && !previous.lost() {
            log::record(Component::Controller, Code::TransportClosed, Phase::Connect, 0);
        }
        if phase == ConnectionPhase::Synchronized {
            if previous.lost() {
                log::record(Component::Controller, Code::Recoverable, Phase::Connect, change.resumed as u64);
                // A new session did not replay what happened while it was away.
                if !change.resumed { self.recovery.deferred_view = true; }
            }
            self.reload_deferred_view();
        }
        self.publish();
    }
    fn reload_deferred_view(&mut self) {
        if !std::mem::take(&mut self.recovery.deferred_view)
            || self.state.view != "chat" || self.state.selected_channel_id.is_empty() { return; }
        let channel = self.state.selected_channel_id.clone();
        if let Err(error) = self.select_channel(&channel) { self.error(error); }
    }
    /// Logout and safety stops release the receiver themselves, so a closed
    /// channel here means the task ended on its own. Never show a stale online.
    pub(super) fn gateway_ended(&mut self) {
        self.gateway = None;
        if !self.state.signed_in
            || self.api.as_ref().is_none_or(|api| api.safety().state() != SafetyState::Allowed) { return; }
        self.recovery.restart = true;
        self.recovery.restart_at = Some(Instant::now() + RESTART_COOLDOWN);
        self.state.connection = "Sin conexión".into();
        self.connection_changed(ConnectionChange {
            phase: ConnectionPhase::Cooldown, wait: RESTART_COOLDOWN, resumed: false,
        });
    }
    pub(super) fn tick_connection(&mut self) {
        if self.recovery.restart_at.is_some_and(|at| Instant::now() >= at) {
            self.recovery.restart_at = None;
            self.state.connection_phase = ConnectionPhase::RetryAvailable;
            self.state.connection_retry_at = None;
            self.publish();
        }
    }
    /// Repeated clicks stop here: only an offered attempt changes the phase.
    pub(super) fn retry_connection(&mut self) -> Result<(), String> {
        if self.state.connection_phase != ConnectionPhase::RetryAvailable { return Ok(()); }
        self.state.connection_phase = ConnectionPhase::Recovering;
        self.state.connection_retry_at = None;
        self.state.connection = "Conectando".into();
        self.publish();
        if !std::mem::take(&mut self.recovery.restart) {
            if let Some(gateway) = &self.gateway { gateway.request_recovery(); }
            return Ok(());
        }
        let api = self.api()?;
        api.safety().check().map_err(|error| error.to_string())?;
        let status = notdiscord_core::settings_proto::valid_presence(&self.state.self_presence)
            .then(|| self.state.self_presence.clone());
        let (gateway, events) = api.gateway(status);
        self.gateway = Some(gateway);
        self.gateway_rx = Some(events);
        self.member_subscription = None;
        Ok(())
    }
}
pub(super) fn retry_stamp(wait: Duration) -> Timestamp {
    let wait = chrono::Duration::from_std(wait).unwrap_or_default();
    (chrono::Utc::now() + wait).fixed_offset()
}
/// Network actions held back while the session is not synchronized. Sends and
/// voice have their own paths; picker replies are answered by their owners.
pub(super) fn deferred_while_offline(kind: &str) -> bool {
    matches!(kind, "edit" | "delete" | "react" | "react-toggle" | "pin-message" | "unpin-message"
        | "forward" | "forward-message" | "forward-many" | "add-friend" | "accept-friend"
        | "remove-friend" | "open-dm" | "add-group" | "group-add" | "mark-read" | "close-dm"
        | "mute-channel" | "mute-dm" | "custom-status" | "save-profile-bio" | "create-invite"
        | "member-moderation" | "remove-group-member" | "guild" | "older" | "present"
        | "jump-message" | "pins" | "search" | "forward-channels" | "dm-sidebar-profile"
        | "group-members" | "members-more")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn repeated_retries_coalesce_and_optional_traffic_waits_for_synchronization() {
        let (mut c, _) = Controller::test_fixture();
        c.state.signed_in = true;
        c.state.connection_phase = ConnectionPhase::Cooldown;
        for _ in 0..10 { c.act(UiAction::new("connection-retry")).await.unwrap(); }
        assert_eq!(c.state.connection_phase, ConnectionPhase::Cooldown);
        assert!(c.act(UiAction::new("pins")).await.is_err());
        c.state.connection_phase = ConnectionPhase::RetryAvailable;
        for _ in 0..10 { c.act(UiAction::new("connection-retry")).await.unwrap(); }
        assert_eq!(c.state.connection_phase, ConnectionPhase::Recovering);
    }
    #[test]
    fn a_visible_presence_or_isolated_error_never_means_a_lost_connection() {
        for phase in [ConnectionPhase::Idle, ConnectionPhase::Synchronized, ConnectionPhase::Connecting] {
            assert!(!phase.lost());
        }
        assert!(!ConnectionPhase::Synchronized.suspended() && !ConnectionPhase::Idle.suspended());
        assert!(ConnectionPhase::Recovering.suspended() && ConnectionPhase::Cooldown.lost());
    }
}
