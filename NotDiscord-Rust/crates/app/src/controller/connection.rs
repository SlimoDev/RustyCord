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
    /// Positive monitor events, deduplicated independently of call lifetime.
    pub(super) power_episode: u64,
    pub(super) power_suspended: bool,
}
impl Recovery {
    pub(super) fn defer_view(&mut self) { self.deferred_view = true; }
}
impl Controller {
    pub(super) fn transport_suspended(&self) -> bool {
        self.state.connection_phase.suspended() || self.voice_lifecycle.recovery_pending()
    }
    pub(super) fn sync_transport_gate(&mut self) {
        let suspended = self.transport_suspended();
        if let Some(api) = &self.api { api.set_transport_available(!suspended); }
        // A voice/viewer lease holds optional admission without pretending that
        // valid Gateway authority or already emitted reads were lost.
        if self.state.connection_phase.suspended() { self.suspend_presence_changes(); self.suspend_call_discovery(); }
        if suspended { self.read_acks.discard_pending(); }
    }
    pub(super) fn connection_changed(&mut self, change: ConnectionChange) {
        let previous = self.state.connection_phase;
        // A replacement task opens normally; the session is still recovering.
        let phase = if change.phase == ConnectionPhase::Synchronized
            && self.gateway.as_ref().is_some_and(|gateway| !gateway.is_ready()) {
            // A queued pre-sleep notification cannot reopen HTTP/voice after
            // the actual Gateway owner invalidated that transport generation.
            ConnectionPhase::Recovering
        } else if change.phase == ConnectionPhase::Connecting && previous.lost() {
            ConnectionPhase::Recovering
        } else {
            change.phase
        };
        self.state.connection_phase = phase;
        // Visibility can advance while offline. Do not drain those watermarks
        // when synchronization returns; later visible demand may queue an ACK.
        if previous.suspended() { self.read_acks.discard_pending(); }
        self.sync_transport_gate();
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
    pub(super) fn reload_deferred_view(&mut self) {
        if self.transport_suspended() { return; }
        if self.state.view != "chat" || self.state.selected_channel_id.is_empty()
            || !std::mem::take(&mut self.recovery.deferred_view) { return; }
        let channel = self.state.selected_channel_id.clone();
        if let Err(error) = self.select_channel(&channel) { self.error(error); }
    }
    /// Logout and safety stops release the receiver themselves, so a closed
    /// channel here means the task ended on its own. Never show a stale online.
    pub(super) fn gateway_ended(&mut self) {
        // A new handle owns a new Core budget. Never let it re-admit the old
        // call's already interrupted recovery as a new automatic episode.
        self.gateway_voice_owner_ended();
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
            self.sync_transport_gate();
            self.state.connection_retry_at = None;
            self.publish();
        }
    }
    /// Repeated clicks stop here: only an offered attempt changes the phase.
    pub(super) fn retry_connection(&mut self) -> Result<(), String> {
        if self.state.connection_phase != ConnectionPhase::RetryAvailable { return Ok(()); }
        self.state.connection_phase = ConnectionPhase::Recovering;
        self.sync_transport_gate();
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
    async fn initial_ready_catalog_waits_for_sync_without_pinning_an_offline_api() {
        use notdiscord_core::api::ErrorKind;

        let (mut c, _, mut replies) = Controller::test_fixture_with_replies();
        // Model the prepared session immediately after __login, without opening
        // a Gateway, account, loopback server or device.
        c.api = Some(Api::new("synthetic-startup-credential-only".into()).unwrap());
        c.state.signed_in = true;
        c.state.current_user = Some(Person { id: "1".into(), ..Default::default() });
        c.state.connection_phase = ConnectionPhase::Connecting;
        c.sync_transport_gate();
        let offline_work = c.api().unwrap();
        let previous_epoch = c.sidebar_epoch;
        let seed = json!({
            "guilds": null,
            "dms": [{"id":"20","type":1,"recipients":[{"id":"42","username":"Fixture peer"}]}],
            "relationships": [{"id":"42","type":1,"user":{"id":"42","username":"Fixture peer"}}]
        });

        // The real delivery order is READY_CATALOG, then Synchronized. Do not
        // yield between them: even a future first polled after sync must not
        // inherit an API clone captured while Connecting.
        c.dispatch("READY_CATALOG", seed.clone()).await.unwrap();
        assert_eq!(c.sidebar_seed, seed);
        assert_eq!(c.sidebar_epoch, previous_epoch.wrapping_add(1));
        assert!(!c.sidebar_inflight);
        assert!(c.sidebar_wait.is_some());
        assert!(replies.try_recv().is_err());
        c.connection_changed(ConnectionChange {
            phase: ConnectionPhase::Synchronized, wait: Duration::ZERO, resumed: false,
        });
        tokio::task::yield_now().await;
        assert!(replies.try_recv().is_err(), "Connecting must not have launched a failing sidebar task");
        assert_eq!(c.sidebar_seed, seed);
        assert!(c.sidebar_wait.is_some());

        // An invalid route is rejected before any I/O, but only after checking
        // the transport ticket. Thus admission can be checked without HTTP.
        const NO_IO_ROUTE: &str = "../startup-admission-fixture";
        assert_eq!(offline_work.clone().get_once(NO_IO_ROUTE).await.unwrap_err().kind,
            ErrorKind::TransportUnavailable);
        let fresh_work = c.api().unwrap();
        assert_eq!(fresh_work.get_once(NO_IO_ROUTE).await.unwrap_err().kind, ErrorKind::Other);

        // Exercise the missing-component read with the existing pure loader
        // fixture, rather than sending an authenticated request in this test.
        let reads = std::cell::RefCell::new(Vec::new());
        let canonical = request_policy::load_sidebar(c.sidebar_seed.clone(), |path| {
            reads.borrow_mut().push(path);
            std::future::ready(Ok(json!([{"id":"10","name":"Fixture guild","permissions":"1024"}])))
        }).await.unwrap();
        assert_eq!(reads.into_inner(), ["users/@me/guilds?limit=200"]);

        // The production scheduler can consume the now-complete fixture with
        // no GETs. Repeated admission while it is in flight is coalesced.
        c.sidebar_seed = canonical.clone();
        c.sidebar();
        c.sidebar();
        assert!(c.sidebar_inflight && c.sidebar_wait.is_none());
        let done = tokio::time::timeout(Duration::from_secs(1), replies.recv())
            .await.unwrap().unwrap();
        assert!(c.accepts(&done));
        assert_eq!(done.action.kind, "__sidebar");
        assert_eq!(done.result.as_ref().unwrap(), &canonical);
        assert!(replies.try_recv().is_err());
        let stale = Completed {
            generation: c.generation, nav: None,
            action: UiAction { kind: "__sidebar".into(), extra: previous_epoch.to_string(), ..Default::default() },
            result: Ok(canonical), credential: None,
        };
        assert!(!c.accepts(&stale));
        // Prevent navigation restoration from scheduling another page request.
        c.state.selected_channel_id = "20".into();
        c.complete(done.action, done.result.unwrap()).await.unwrap();
        assert_eq!(c.state.guilds.len(), 1);
        assert_eq!(c.state.direct_messages.len(), 1);
        assert_eq!(c.state.relationships.len(), 1);
        assert!(!c.sidebar_inflight && c.sidebar_wait.is_none());
        assert!(replies.try_recv().is_err());
    }

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
    #[tokio::test]
    async fn recovery_discards_old_and_offline_ack_demand_without_forgetting_in_flight() {
        let (mut c, _) = Controller::test_fixture();
        let now = Instant::now();
        let visible = now - Duration::from_secs(1);
        c.state.connection_phase = ConnectionPhase::Synchronized;
        c.read_acks.advance("123", "111", visible);
        c.read_acks.advance("456", "222", visible);
        let sent = c.read_acks.ready(now);
        assert_eq!(sent.len(), 1);
        c.connection_changed(ConnectionChange { phase: ConnectionPhase::Cooldown, wait: RESTART_COOLDOWN, resumed: false });
        c.read_acks.advance("789", "333", visible);
        c.recovery.restart_at = Some(Instant::now());
        c.tick_connection();
        assert_eq!(c.state.connection_phase, ConnectionPhase::RetryAvailable);
        c.retry_connection().unwrap();
        assert_eq!(c.state.connection_phase, ConnectionPhase::Recovering);
        c.read_acks.advance("789", "444", visible);
        c.connection_changed(ConnectionChange { phase: ConnectionPhase::Synchronized, wait: Duration::ZERO, resumed: true });
        // The real request remains in flight; a fresh visible watermark cannot
        // overlap it, and no historical demand returns after its completion.
        c.read_acks.advance("789", "555", visible);
        assert!(c.read_acks.ready(now).is_empty());
        c.read_acks.discard_pending();
        c.read_acks.complete(&sent[0].0);
        assert!(c.read_acks.ready(now).is_empty());
        c.read_acks.advance("789", "666", visible);
        assert_eq!(c.read_acks.ready(now), vec![("789".into(), "666".into())]);
    }
}
