//! Actor-owned recovery of the retained call. Core owns admission of the one
//! compound episode; this module owns its voice substep and actual completion.
use super::*;
use crate::platform::power::PowerEvent;
use notdiscord_core::diagnostics::{
    self as diagnostics, Code, Component, Phase as DiagnosticPhase,
};
use notdiscord_core::gateway::RecoveryPermit;

const WAIT_LIMIT: Duration = Duration::from_secs(90);
const ATTEMPT_LIMIT: Duration = Duration::from_secs(45);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Resume,
    Fresh,
}

#[cfg(test)]
mod tests {
    use super::*;
    use notdiscord_core::safety::SafetyGate;

    fn fixture() -> (
        Controller,
        Arc<Mutex<Vec<String>>>,
        mpsc::Receiver<Value>,
        tokio::task::JoinHandle<()>,
    ) {
        let (mut c, _) = Controller::test_fixture();
        let (gateway, wire, task) = GatewayHandle::offline_recorder(SafetyGate::default());
        c.gateway = Some(gateway);
        c.state.current_user = Some(Person {
            id: "1".into(),
            ..Default::default()
        });
        c.state.signed_in = true;
        c.state.connection_phase = ConnectionPhase::Synchronized;
        c.voice_channel = "2".into();
        c.voice_attempt = 7;
        c.voice_started = true;
        c.voice_session = "synthetic-session".into();
        c.voice_lifecycle.native_voice = Some(10);
        c.voice_lifecycle.media_epoch = 3;
        c.state.voice_connected = true;
        c.state.voice_encryption_ready = true;
        c.state.call_started_at = Some(chrono::Utc::now().fixed_offset());
        let (media, calls) = MediaBridge::fake(|command, args| match command {
            "suspendVoice" => Ok(json!({"suspended":true,"mediaSession":args["mediaSession"]})),
            "resumeVoice" => {
                Ok(json!({"admitted":true,"mediaSession":args["mediaSession"],"generation":1}))
            }
            "disconnect" => Ok(json!({"state":"disconnected"})),
            "connectVoice" => Ok(json!({"session":11,"state":"connecting"})),
            _ => Ok(json!({})),
        });
        c.media = Some(media);
        (c, calls, wire, task)
    }
    fn interrupted() -> Value {
        json!({"event":"voiceState","mediaSession":10,"state":"interrupted","transportGeneration":0,
            "transportReady":true,"encryptionReady":false,"resumeEligible":true,"retainedNative":true,
            "reason":{"code":"VOICE_SOCKET_NETWORK"},"closeCode":null})
    }
    fn connected(generation: u64, session: u64) -> Value {
        json!({"event":"voiceState","mediaSession":session,"state":"connected","transportGeneration":generation,
            "transportReady":true,"encryptionReady":true})
    }
    async fn pump(c: &mut Controller) {
        for _ in 0..12 {
            c.tick_voice_recovery().await.unwrap();
            tokio::task::yield_now().await;
        }
    }
    #[tokio::test]
    async fn io_recovery_keeps_membership_and_requires_current_connected_after_the_queue_ack() {
        let (mut c, calls, mut wire, task) = fixture();
        let started = c.state.call_started_at;
        let frames = c.media.as_ref().unwrap().video_frames();
        let (receiver, _) = MediaBridge::fake_receiver(|_, _| Ok(json!({})), frames.clone());
        let id = receiver.fake_video_id();
        {
            let mut latest = frames.lock().unwrap();
            assert!(latest.activate(id));
            assert!(latest.publish(notdiscord_core::video::VideoFrame {
                subscription_id: id,
                sequence: 1,
                width: 1,
                height: 1,
                rgba: vec![1, 2, 3, 255]
            }));
        }
        c.voice_lifecycle.receiver = Some(receiver);
        for _ in 0..8 {
            c.media_event(interrupted()).await.unwrap();
        }
        assert_eq!(
            frames.lock().unwrap().buffered_bytes(),
            0,
            "pre-sleep receiver frames must be discarded too"
        );
        pump(&mut c).await;
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|command| *command == "resumeVoice")
                .count(),
            1
        );
        assert!(
            !c.state.voice_connected && !c.state.voice_encryption_ready,
            "admitted is only queue admission"
        );
        assert_eq!(c.voice_channel, "2");
        assert_eq!(c.voice_lifecycle.native_voice, Some(10));
        assert!(c.stale_voice_transport_event(&connected(0, 10)));
        c.media_event(connected(0, 10)).await.unwrap();
        assert!(!c.state.voice_encryption_ready);
        c.media_event(connected(1, 10)).await.unwrap();
        pump(&mut c).await;
        assert!(c.state.voice_connected && c.state.voice_encryption_ready);
        assert_eq!(c.state.call_started_at, started);
        assert!(
            !c.voice_lifecycle
                .transport_recovery
                .as_ref()
                .unwrap()
                .pending()
        );
        assert!(
            wire.try_recv().is_err(),
            "Resume does not send VoiceState(null) or another join"
        );
        task.abort();
    }
    #[tokio::test]
    async fn duplicate_power_notices_wait_for_gateway_and_do_not_add_resume_attempts() {
        let (mut c, calls, mut wire, task) = fixture();
        c.state.connection_phase = ConnectionPhase::Recovering;
        for _ in 0..8 {
            c.observe_voice_power(PowerEvent::Suspend { episode: 1 });
        }
        pump(&mut c).await;
        c.media_event(interrupted()).await.unwrap();
        for _ in 0..8 {
            c.observe_voice_power(PowerEvent::Resume {
                episode: 1,
                source: crate::platform::power::ResumeSource::Automatic,
            });
        }
        pump(&mut c).await;
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|command| *command == "suspendVoice")
                .count(),
            1
        );
        assert!(
            !calls
                .lock()
                .unwrap()
                .iter()
                .any(|command| command == "resumeVoice")
        );
        c.connection_changed(notdiscord_core::gateway::ConnectionChange {
            phase: ConnectionPhase::Synchronized,
            wait: Duration::ZERO,
            resumed: true,
        });
        pump(&mut c).await;
        c.observe_voice_power(PowerEvent::Resume {
            episode: 1,
            source: crate::platform::power::ResumeSource::User,
        });
        pump(&mut c).await;
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|command| *command == "resumeVoice")
                .count(),
            0,
            "a synthetic old Synchronized event cannot restore the real paused Gateway"
        );
        assert_eq!(c.state.connection_phase, ConnectionPhase::Recovering);
        assert!(wire.try_recv().is_err());
        task.abort();
    }
    #[tokio::test]
    async fn fresh_mode_closes_old_native_owner_before_one_current_channel_state_and_new_credentials()
     {
        let (mut c, calls, mut wire, task) = fixture();
        c.state.connection_phase = ConnectionPhase::Recovering;
        c.voice_lifecycle.native_share = Some(20);
        c.voice_token = "synthetic-old-token".into();
        c.voice_endpoint = "old.discord.media".into();
        c.media_event(json!({"event":"voiceState","mediaSession":10,"transportGeneration":0,"state":"error",
            "closeCode":4006,"recoveryClass":"session-invalid","reason":{"code":"VOICE_SOCKET_CLOSED"}})).await.unwrap();
        // SessionInvalid can retire the main owner before the separate
        // GoLive owner. Its absence must not skip the all-owner close.
        c.voice_lifecycle.native_voice = None;
        assert!(wire.try_recv().is_err());
        c.tick_voice_recovery().await.unwrap();
        assert!(
            wire.try_recv().is_err(),
            "local close must complete before the one Gateway write"
        );
        pump(&mut c).await;
        assert_eq!(*calls.lock().unwrap(), ["disconnect"]);
        assert_eq!(c.voice_lifecycle.native_voice, None);
        assert_eq!(c.voice_lifecycle.native_share, None);
        assert!(
            wire.try_recv().is_err(),
            "Fresh releases the local capture while Gateway is still recovering"
        );
        assert!(
            c.voice_lifecycle.transport_recovery.as_ref().unwrap().permit.is_none(),
            "local release does not admit a network recovery attempt"
        );
        let mut old = interrupted();
        old["resumeEligible"] = json!(false);
        assert!(c.stale_voice_transport_event(&old));
        c.media_event(old).await.unwrap();
        c.connection_changed(notdiscord_core::gateway::ConnectionChange {
            phase: ConnectionPhase::Synchronized,
            wait: Duration::ZERO,
            resumed: true,
        });
        pump(&mut c).await;
        assert_eq!(*calls.lock().unwrap(), ["disconnect"]);
        let state = wire.try_recv().unwrap();
        assert_eq!(state["op"], 4);
        assert_eq!(state["d"]["channel_id"], "2");
        assert!(wire.try_recv().is_err());
        assert!(
            c.voice_token.is_empty() && c.voice_endpoint.is_empty() && c.voice_session.is_empty()
        );
        assert_eq!(c.voice_lifecycle.native_voice, None);
        assert!(c.voice_recovery_allows_transport());
        c.voice_dispatch(
            "VOICE_STATE_UPDATE",
            &json!({"user_id":"1","channel_id":"2","session_id":"new-synthetic-session"}),
        )
        .await
        .unwrap();
        c.voice_dispatch("VOICE_SERVER_UPDATE", &json!({"channel_id":"2","endpoint":"fresh.discord.media","token":"new-synthetic-token"})).await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|command| *command == "connectVoice")
                .count(),
            1
        );
        assert!(
            !calls
                .lock()
                .unwrap()
                .iter()
                .any(|command| command == "resumeVoice")
        );
        task.abort();
    }
    #[tokio::test]
    async fn invalid_session_after_resume_cannot_chain_a_fresh_join_or_reactivate_from_old_events()
    {
        let (mut c, calls, mut wire, task) = fixture();
        c.state.camera_enabled = true;
        c.voice_lifecycle.selected_camera = "synthetic-camera".into();
        c.media_event(interrupted()).await.unwrap();
        pump(&mut c).await;
        c.media_event(json!({"event":"voiceState","mediaSession":10,"transportGeneration":1,"state":"error",
            "closeCode":4009,"recoveryClass":"session-invalid","reason":{"code":"VOICE_SOCKET_CLOSED"}})).await.unwrap();
        pump(&mut c).await;
        assert!(!c.voice_recovery_allows_transport());
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|command| *command == "resumeVoice")
                .count(),
            1
        );
        assert!(
            !calls
                .lock()
                .unwrap()
                .iter()
                .any(|command| command == "connectVoice")
        );
        assert!(wire.try_recv().is_err());
        assert!(c.stale_voice_transport_event(&connected(1, 10)));
        assert!(!c.state.voice_encryption_ready);
        assert!(!c.state.voice_error.is_empty());
        assert!(!c.state.camera_preparing && !c.state.screen_preparing);
        assert_eq!(c.state.call_status_label, "Recuperación de voz detenida");
        task.abort();
    }
    #[tokio::test]
    async fn first_failed_open_has_no_extra_attempt_and_cancellation_retains_lease_until_actual_close()
     {
        let (mut c, _, _, task) = fixture();
        c.state.voice_connected = false;
        c.state.voice_encryption_ready = false;
        c.state.call_started_at = None;
        assert!(
            !c.handle_voice_transport_event(
                &json!({"event":"voiceState","mediaSession":10,"state":"error",
            "closeCode":4006,"recoveryClass":"session-invalid"})
            )
            .unwrap()
        );
        assert!(c.voice_lifecycle.transport_recovery.is_none());
        c.state.voice_connected = true;
        c.state.voice_encryption_ready = true;
        c.media_event(interrupted()).await.unwrap();
        c.tick_voice_recovery().await.unwrap();
        c.cancel_voice_transport_recovery();
        assert!(
            c.voice_lifecycle
                .transport_recovery
                .as_ref()
                .unwrap()
                .permit
                .is_some()
        );
        assert!(c.gateway.as_ref().unwrap().try_view_recovery().is_none());
        c.complete_voice_transport_close(false);
        assert!(
            c.voice_lifecycle
                .transport_recovery
                .as_ref()
                .unwrap()
                .permit
                .is_some()
        );
        c.complete_voice_transport_close(true);
        assert!(c.voice_lifecycle.transport_recovery.is_none());
        assert!(c.gateway.as_ref().unwrap().try_view_recovery().is_none());
        task.abort();
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Waiting,
    ClosingOld,
    FreshWaiting,
    Resuming,
    Stopping,
    Retiring,
    Stopped,
    Restored,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkKind {
    Suspend,
    CloseOld,
    Resume,
    FreshState,
}
struct Work {
    kind: WorkKind,
    task: tokio::task::JoinHandle<Result<Value, String>>,
}

pub(super) struct VoiceRecovery {
    account: String,
    generation: u64,
    attempt: u64,
    media_epoch: u64,
    channel: String,
    guild: String,
    media_session: u64,
    old_share: Option<u64>,
    transport_generation: u64,
    expected_generation: Option<u64>,
    mode: Mode,
    phase: Phase,
    fresh_closed: bool,
    eligible: bool,
    power_episode: Option<u64>,
    power_suspended: bool,
    deadline: Instant,
    reason: String,
    media: MediaBridge,
    work: Option<Work>,
    closing: Option<tokio::task::JoinHandle<Result<(), String>>>,
    permit: Option<RecoveryPermit>,
    ready: Option<Value>,
    restore_camera: String,
    restore_share: Option<UiAction>,
}
impl VoiceRecovery {
    pub(super) fn pending(&self) -> bool {
        !matches!(self.phase, Phase::Stopped | Phase::Restored)
    }
    fn current(&self, controller: &Controller) -> bool {
        self.account == controller.me()
            && self.generation == controller.generation
            && self.attempt == controller.voice_attempt
            && self.media_epoch == controller.voice_lifecycle.media_epoch
            && self.channel == controller.voice_channel
            && self.guild == controller.voice_guild
            && controller
                .media
                .as_ref()
                .is_some_and(|media| media.same_instance(&self.media))
    }
    fn media_work(&mut self, kind: WorkKind, command: &'static str, args: Value) {
        debug_assert!(self.work.is_none());
        let media = self.media.clone();
        self.work = Some(Work {
            kind,
            task: tokio::spawn(async move { media.command(command, args).await }),
        });
    }
    fn close_process(&mut self) {
        self.media.close();
        if self.closing.is_none() {
            let media = self.media.clone();
            self.closing = Some(tokio::spawn(async move { media.await_closed().await }));
        }
    }
    fn finish_permit(&mut self, success: bool) -> bool {
        self.permit
            .take()
            .is_none_or(|permit| permit.finish(success))
    }
}

fn safe_reason(value: &Value) -> String {
    // Fixed classes only; never preserve an arbitrary native/server payload.
    match value["reason"]["code"].as_str().unwrap_or("") {
        "VOICE_HEARTBEAT_TIMEOUT" => "El servidor de voz dejó de confirmar los latidos.",
        "VOICE_SOCKET_NETWORK" => "Se interrumpió la conexión de red del servidor de voz.",
        "VOICE_WRITE_TIMEOUT" => "La señalización de voz no terminó de escribirse a tiempo.",
        "VOICE_SOCKET_TIMEOUT" => "El servidor de voz no confirmó la apertura a tiempo.",
        "VOICE_SUSPENDED" => "La llamada espera la reanudación de la conexión.",
        _ => match value["closeCode"].as_u64() {
            Some(4015) => "El servidor de voz se reinició.",
            Some(4006 | 4009) => "La sesión de voz anterior ya no es válida.",
            _ => "La conexión de voz se interrumpió.",
        },
    }
    .into()
}
fn fresh_candidate(value: &Value) -> bool {
    matches!(value["closeCode"].as_u64(), Some(4006 | 4009))
        || matches!(
            value["recoveryClass"].as_str(),
            Some("session-invalid" | "fresh-candidate")
        )
}

impl Controller {
    fn begin_transport_recovery(&mut self, mode: Mode, reason: String) -> bool {
        let Some(media) = self.media.clone() else {
            return false;
        };
        let Some(session) = self
            .voice_lifecycle
            .native_voice
            .filter(|session| *session > 0)
        else {
            return false;
        };
        if self.voice_channel.is_empty()
            || self.voice_lifecycle.leaving.is_some()
            || self.voice_lifecycle.switching.is_some()
            || !self.voice_permissions().0
            || !(self.state.voice_connected
                || self.state.voice_encryption_ready
                || self.state.call_started_at.is_some())
        {
            return false;
        }
        let restored = self
            .voice_lifecycle
            .transport_recovery
            .as_ref()
            .filter(|recovery| recovery.current(self) && recovery.phase == Phase::Restored);
        let transport_generation = restored
            .and_then(|recovery| recovery.expected_generation)
            .unwrap_or(0);
        self.voice_lifecycle.transport_recovery = Some(VoiceRecovery {
            account: self.me(),
            generation: self.generation,
            attempt: self.voice_attempt,
            media_epoch: self.voice_lifecycle.media_epoch,
            channel: self.voice_channel.clone(),
            guild: self.voice_guild.clone(),
            media_session: session,
            old_share: self.voice_lifecycle.native_share,
            transport_generation,
            expected_generation: None,
            mode,
            phase: Phase::Waiting,
            fresh_closed: false,
            eligible: false,
            power_episode: None,
            power_suspended: false,
            deadline: Instant::now() + WAIT_LIMIT,
            reason,
            media,
            work: None,
            closing: None,
            permit: None,
            ready: None,
            restore_camera: if self.state.camera_enabled {
                self.voice_lifecycle.selected_camera.clone()
            } else {
                String::new()
            },
            restore_share: self
                .state
                .screen_sharing
                .then(|| self.voice_lifecycle.selected_share.clone())
                .flatten(),
        });
        self.quiesce_transport_presentation();
        true
    }
    fn quiesce_transport_presentation(&mut self) {
        self.state.voice_connected = false;
        self.state.voice_encryption_ready = false;
        self.state.voice_connecting = true;
        self.state.camera_available = false;
        self.state.screen_available = false;
        self.state.camera_enabled = false;
        self.state.screen_sharing = false;
        self.state.screen_paused = false;
        // Preserve the ability to cancel the existing explicit choice while
        // the actual capture is stopped. These flags do not acquire devices.
        self.state.camera_preparing = !self.voice_lifecycle.selected_camera.is_empty();
        self.state.screen_preparing = self.voice_lifecycle.selected_share.is_some();
        self.voice_lifecycle.camera_attempt = self.voice_lifecycle.camera_attempt.wrapping_add(1);
        self.voice_lifecycle.share_attempt = self.voice_lifecycle.share_attempt.wrapping_add(1);
        self.voice_lifecycle.join_wait = None;
        self.voice_lifecycle.ptt_effective = false;
        self.last_ptt = false;
        self.voice_lifecycle.input_capture = None;
        for person in self.call_people.values_mut() {
            person.speaking = false;
        }
        if let Some(media) = &self.media {
            media.clear_video_frames();
        }
        if let Some(receiver) = &self.voice_lifecycle.receiver {
            receiver.clear_video_frames();
        }
        for video in &mut self.state.call_videos {
            if !video.preview {
                video.active = false;
                video.status = "waiting".into();
            }
        }
        self.state.call_status_label = "Recuperando la conexión de voz…".into();
        self.sync_transport_gate();
        self.refresh_call();
    }
    /// Before the general event match: the same mediaSession survives Resume,
    /// so its generation must also match before any old activity/capture wins.
    pub(in crate::controller) fn stale_voice_transport_event(&self, value: &Value) -> bool {
        if !matches!(
            value["event"].as_str(),
            Some(
                "voiceState"
                    | "shareState"
                    | "watchState"
                    | "cameraState"
                    | "speaking"
                    | "participant"
                    | "inputGateState"
                    | "videoStream"
                    | "videoError"
                    | "transportError"
            )
        ) {
            return false;
        }
        let Some(recovery) = self.voice_lifecycle.transport_recovery.as_ref() else {
            return false;
        };
        let Some(session) = value["mediaSession"].as_u64() else {
            return false;
        };
        if recovery.old_share == Some(session) {
            return true;
        }
        if session != recovery.media_session {
            return false;
        }
        if recovery.phase == Phase::Restored {
            return value["transportGeneration"]
                .as_u64()
                .is_none_or(|generation| Some(generation) != recovery.expected_generation);
        }
        let state = s(value, "state");
        if matches!(
            recovery.phase,
            Phase::Stopped | Phase::Retiring | Phase::Stopping
        ) {
            return true;
        }
        if recovery.mode == Mode::Fresh
            && (recovery.fresh_closed
                || matches!(recovery.phase, Phase::ClosingOld | Phase::FreshWaiting))
        {
            return true;
        }
        let expected = recovery
            .expected_generation
            .unwrap_or(recovery.transport_generation);
        if value["transportGeneration"].as_u64() != Some(expected) {
            return true;
        }
        if recovery.phase == Phase::Waiting {
            return value["event"] != "voiceState"
                || !matches!(
                    state.as_str(),
                    "interrupted" | "resuming" | "error" | "disconnected"
                );
        }
        false
    }
    /// Only Fresh chosen before admission may use the ordinary connectVoice
    /// credentials path. A failed Resume never turns into an Identify fallback.
    pub(in crate::controller) fn voice_recovery_allows_transport(&self) -> bool {
        self.voice_lifecycle
            .transport_recovery
            .as_ref()
            .is_none_or(|recovery| {
                recovery.current(self)
                    && (recovery.phase == Phase::Restored
                        || recovery.mode == Mode::Fresh
                            && recovery.phase == Phase::FreshWaiting
                            && recovery.permit.is_some())
            })
    }
    pub(in crate::controller) fn handle_voice_transport_event(
        &mut self,
        value: &Value,
    ) -> Result<bool, String> {
        let state = s(value, "state");
        let terminal = matches!(state.as_str(), "error" | "disconnected");
        let special = matches!(state.as_str(), "interrupted" | "suspended" | "resuming");
        if !special && !terminal && state != "connected" {
            // Native/DAVE negotiation may replay intermediate readiness, but
            // it cannot turn the recovering call online before op9 + cipher.
            return Ok(self
                .voice_lifecycle
                .transport_recovery
                .as_ref()
                .is_some_and(|recovery| recovery.pending()));
        }
        if special || terminal && fresh_candidate(value) {
            let prior = self.voice_lifecycle.transport_recovery.as_ref();
            if prior.is_some_and(|recovery| recovery.phase == Phase::Stopped) {
                return Ok(true);
            }
            let active = prior.is_some_and(|recovery| recovery.pending());
            if !active {
                let mode = if terminal { Mode::Fresh } else { Mode::Resume };
                if !self.begin_transport_recovery(mode, safe_reason(value)) {
                    return Ok(false);
                }
            }
            let recovery = self.voice_lifecycle.transport_recovery.as_mut().unwrap();
            if recovery.phase == Phase::Resuming || recovery.phase == Phase::FreshWaiting {
                if state != "resuming" {
                    self.stop_transport_recovery("El intento de recuperación de voz falló. No se emitirá otra conexión automáticamente.");
                }
                return Ok(true);
            }
            if let Some(generation) = value["transportGeneration"].as_u64() {
                if generation < recovery.transport_generation {
                    return Ok(true);
                }
                recovery.transport_generation = generation;
            }
            recovery.eligible = value["resumeEligible"] == true;
            if terminal && recovery.mode == Mode::Fresh {
                recovery.eligible = true;
            }
            self.state.call_status_label = if recovery.power_suspended {
                "Llamada suspendida"
            } else {
                "Recuperando la conexión de voz…"
            }
            .into();
            self.sync_transport_gate();
            self.refresh_call();
            return Ok(true);
        }
        let Some(recovery) = self.voice_lifecycle.transport_recovery.as_mut() else {
            return Ok(false);
        };
        if !recovery.pending() {
            return Ok(false);
        }
        if terminal {
            self.stop_transport_recovery("El servidor no confirmó la recuperación de voz. Se detuvo el intento conservando su causa.");
            return Ok(true);
        }
        if state == "connected" {
            let current_session = self.voice_lifecycle.native_voice;
            let session = value["mediaSession"].as_u64();
            let safe = value["transportReady"] == true
                && value["encryptionReady"] == true
                && session.is_some()
                && session == current_session;
            let generation = value["transportGeneration"].as_u64();
            let current = match recovery.mode {
                Mode::Resume => {
                    recovery.phase == Phase::Resuming
                        && session == Some(recovery.media_session)
                        && generation == recovery.expected_generation
                }
                Mode::Fresh => {
                    recovery.phase == Phase::FreshWaiting
                        && session != Some(recovery.media_session)
                        && generation == Some(0)
                }
            };
            if safe && current {
                recovery.ready = Some(value.clone());
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn stop_transport_recovery(&mut self, message: &str) {
        let Some(recovery) = self.voice_lifecycle.transport_recovery.as_mut() else {
            return;
        };
        if matches!(recovery.phase, Phase::Stopped | Phase::Retiring) {
            return;
        }
        recovery.phase = Phase::Stopping;
        recovery.ready = None;
        recovery.eligible = false;
        recovery.reason = format!("{} {}", recovery.reason, message);
        recovery.close_process();
        self.state.voice_connected = false;
        self.state.voice_encryption_ready = false;
        self.state.voice_connecting = false;
        self.state.camera_preparing = false;
        self.state.screen_preparing = false;
        self.state.call_status_label = "Recuperación de voz detenida".into();
        let reason = parsing::limited(&recovery.reason, 256);
        self.report_voice_error(&reason);
        self.sync_transport_gate();
        self.refresh_call();
    }
    /// Called at the beginning of an actual user/permission/account departure.
    /// The permit stays owned until complete_voice_transport_close(true).
    pub(in crate::controller) fn cancel_voice_transport_recovery(&mut self) {
        if let Some(recovery) = self.voice_lifecycle.transport_recovery.as_mut() {
            recovery.phase = Phase::Retiring;
            recovery.ready = None;
            recovery.eligible = false;
            recovery.close_process();
        }
    }
    pub(in crate::controller) fn complete_voice_transport_close(&mut self, closed: bool) {
        if !closed {
            return;
        }
        if let Some(mut recovery) = self.voice_lifecycle.transport_recovery.take() {
            // The native/socket owner is known to have stopped. Aborting a
            // remaining IPC waiter now cannot leave network/capture behind.
            if let Some(work) = recovery.work.take() {
                work.task.abort();
            }
            if let Some(close) = recovery.closing.take() {
                close.abort();
            }
            recovery.finish_permit(false);
        }
        self.sync_transport_gate();
    }
    pub(in crate::controller) fn gateway_voice_owner_ended(&mut self) {
        if self.voice_lifecycle.transport_recovery.is_none() {
            self.begin_transport_recovery(
                Mode::Resume,
                "Terminó el propietario anterior de la conexión.".into(),
            );
        }
        self.stop_transport_recovery(
            "La llamada anterior no se recuperará con un presupuesto nuevo al recrear el Gateway.",
        );
    }
    fn observe_voice_power(&mut self, event: PowerEvent) {
        match event {
            PowerEvent::Suspend { episode } => {
                if episode == 0 || episode <= self.recovery.power_episode {
                    return;
                }
                self.recovery.power_episode = episode;
                self.recovery.power_suspended = true;
                diagnostics::record(
                    Component::Controller,
                    Code::PowerSuspended,
                    DiagnosticPhase::Stop,
                    (episode.min(u64::MAX >> 2) << 2) | 1,
                );
                if let Some(gateway) = &self.gateway {
                    gateway.suspend_for_power();
                    if matches!(
                        self.state.connection_phase,
                        ConnectionPhase::Synchronized | ConnectionPhase::Connecting
                    ) {
                        self.state.connection_phase = ConnectionPhase::Recovering;
                        self.state.connection = "Conexión suspendida".into();
                    }
                    self.sync_transport_gate();
                }
                let started = self
                    .voice_lifecycle
                    .transport_recovery
                    .as_ref()
                    .is_some_and(|recovery| {
                        matches!(recovery.phase, Phase::Resuming | Phase::FreshWaiting)
                    });
                if started {
                    self.stop_transport_recovery(
                        "El equipo se suspendió durante el único intento de recuperación.",
                    );
                    return;
                }
                if self
                    .voice_lifecycle
                    .transport_recovery
                    .as_ref()
                    .is_none_or(|recovery| !recovery.pending())
                    && !self.begin_transport_recovery(
                        Mode::Resume,
                        "Windows notificó la suspensión del equipo.".into(),
                    )
                {
                    return;
                }
                let recovery = self.voice_lifecycle.transport_recovery.as_mut().unwrap();
                if recovery.power_episode.is_some_and(|old| episode <= old) {
                    return;
                }
                recovery.power_episode = Some(episode);
                recovery.power_suspended = true;
                if recovery.work.is_none() && recovery.phase == Phase::Waiting {
                    recovery.media_work(
                        WorkKind::Suspend,
                        "suspendVoice",
                        json!({"mediaSession":recovery.media_session}),
                    );
                }
                self.state.call_status_label = "Llamada suspendida".into();
                self.sync_transport_gate();
                self.refresh_call();
            }
            PowerEvent::Resume { episode, source } => {
                if self.recovery.power_episode != episode || !self.recovery.power_suspended {
                    return;
                }
                self.recovery.power_suspended = false;
                let source = match source {
                    crate::platform::power::ResumeSource::Automatic => 2,
                    crate::platform::power::ResumeSource::User => 3,
                };
                diagnostics::record(
                    Component::Controller,
                    Code::PowerResumed,
                    DiagnosticPhase::Start,
                    (episode.min(u64::MAX >> 2) << 2) | source,
                );
                if let Some(gateway) = &self.gateway {
                    gateway.resume_after_power();
                }
                let Some(recovery) = self.voice_lifecycle.transport_recovery.as_mut() else {
                    return;
                };
                if recovery.power_episode != Some(episode) || !recovery.power_suspended {
                    return;
                }
                recovery.power_suspended = false;
                recovery.deadline = Instant::now() + WAIT_LIMIT;
                self.state.call_status_label =
                    "Esperando conexión para recuperar la llamada…".into();
                self.refresh_call();
            }
        }
    }
    pub(in crate::controller) async fn poll_power_events(&mut self) {
        if self.power.failed() && !self.power_reported {
            self.power_reported = true;
            diagnostics::record(
                Component::Controller,
                Code::PowerObserverUnavailable,
                DiagnosticPhase::Start,
                0,
            );
            self.status("Windows no pudo registrar los avisos de suspensión. La recuperación ante fallos de red sigue disponible.");
        }
        for _ in 0..4 {
            let Some(event) = self.power.take() else {
                break;
            };
            self.observe_voice_power(event);
        }
    }
    pub(in crate::controller) async fn tick_voice_recovery(&mut self) -> Result<(), String> {
        if self
            .voice_lifecycle
            .transport_recovery
            .as_ref()
            .is_none_or(|recovery| recovery.phase == Phase::Restored)
            && self.state.connection_phase.suspended()
            && self.state.voice_connected
            && self.begin_transport_recovery(
                Mode::Resume,
                "Se interrumpió la conexión principal de Discord.".into(),
            )
        {
            let recovery = self.voice_lifecycle.transport_recovery.as_mut().unwrap();
            recovery.media_work(
                WorkKind::Suspend,
                "suspendVoice",
                json!({"mediaSession":recovery.media_session}),
            );
        }
        let Some(recovery) = self.voice_lifecycle.transport_recovery.as_ref() else {
            return Ok(());
        };
        if !recovery.pending() {
            return Ok(());
        }
        if !recovery.current(self) && !matches!(recovery.phase, Phase::Stopping | Phase::Retiring) {
            self.cancel_voice_transport_recovery();
        }
        if !self.voice_permissions().0
            && self
                .voice_lifecycle
                .transport_recovery
                .as_ref()
                .is_some_and(|recovery| {
                    !matches!(recovery.phase, Phase::Stopping | Phase::Retiring)
                })
        {
            self.stop_transport_recovery("Ya no está autorizado el acceso al canal de la llamada.");
        }
        let finished = self
            .voice_lifecycle
            .transport_recovery
            .as_ref()
            .and_then(|recovery| {
                recovery
                    .work
                    .as_ref()
                    .filter(|work| work.task.is_finished())
                    .map(|_| ())
            })
            .is_some();
        if finished {
            let work = self
                .voice_lifecycle
                .transport_recovery
                .as_mut()
                .unwrap()
                .work
                .take()
                .unwrap();
            let kind = work.kind;
            let result = work.task.await.unwrap_or_else(|_| {
                Err("El trabajo local de recuperación terminó sin confirmar su resultado.".into())
            });
            let recovery = self.voice_lifecycle.transport_recovery.as_mut().unwrap();
            if !matches!(recovery.phase, Phase::Stopping | Phase::Retiring) {
                match (kind, result) {
                    (WorkKind::Suspend, Ok(value)) if value["suspended"] == true => {}
                    (WorkKind::CloseOld, Ok(value)) if value["state"] == "disconnected" => {
                        recovery.phase = Phase::Waiting;
                        recovery.fresh_closed = true;
                        recovery.eligible = true;
                        self.voice_lifecycle.native_voice = None;
                        self.voice_lifecycle.native_share = None;
                    }
                    (WorkKind::Resume, Ok(value))
                        if value["admitted"] == true
                            && value["mediaSession"].as_u64() == Some(recovery.media_session)
                            && value["generation"].as_u64() == recovery.expected_generation => {}
                    (WorkKind::FreshState, Ok(_)) => {}
                    (_, Err(error)) => self.stop_transport_recovery(&parsing::limited(&error, 160)),
                    _ => self.stop_transport_recovery(
                        "La orden no confirmó el contexto de recuperación solicitado.",
                    ),
                }
            }
        }
        let recovery = self.voice_lifecycle.transport_recovery.as_ref().unwrap();
        if matches!(recovery.phase, Phase::Stopping | Phase::Retiring) {
            let finished = recovery
                .closing
                .as_ref()
                .is_some_and(|close| close.is_finished());
            if finished {
                let close = self
                    .voice_lifecycle
                    .transport_recovery
                    .as_mut()
                    .unwrap()
                    .closing
                    .take()
                    .unwrap();
                let closed = close
                    .await
                    .unwrap_or_else(|_| Err("No se confirmó el cierre del motor anterior.".into()));
                if closed.is_ok() {
                    let recovery = self.voice_lifecycle.transport_recovery.as_mut().unwrap();
                    if let Some(work) = recovery.work.take() {
                        work.task.abort();
                    }
                    recovery.finish_permit(false);
                    recovery.phase = Phase::Stopped;
                    self.voice_started = false;
                    self.voice_lifecycle.native_voice = None;
                    self.voice_lifecycle.native_share = None;
                    self.voice_lifecycle.join_wait = None;
                    self.state.voice_connecting = false;
                    self.sync_transport_gate();
                    self.refresh_call();
                } else {
                    self.report_voice_error("No se confirmó la liberación del motor anterior. Se conserva la barrera de recuperación.");
                }
            }
            return Ok(());
        }
        if recovery
            .permit
            .as_ref()
            .is_some_and(|permit| !permit.is_current())
        {
            self.stop_transport_recovery(
                "El episodio compartido terminó antes de confirmar la voz.",
            );
            return Ok(());
        }
        if !recovery.power_suspended && Instant::now() >= recovery.deadline {
            self.stop_transport_recovery("La recuperación no terminó dentro de su plazo; no se emitirá otra conexión automáticamente.");
            return Ok(());
        }
        if recovery.ready.is_some() && recovery.work.is_none() {
            return self.finish_transport_recovery().await;
        }
        if recovery.phase != Phase::Waiting
            || recovery.work.is_some()
            || self.voice_lifecycle.leaving.is_some()
            || self.voice_lifecycle.switching.is_some()
        {
            return Ok(());
        }
        // A dead main voice session may still own a separate GoLive capture.
        // Releasing both local owners is mandatory cleanup, not a network
        // recovery attempt, and cannot wait for Gateway/Sends admission.
        if recovery.mode == Mode::Fresh && !recovery.fresh_closed {
            let recovery = self.voice_lifecycle.transport_recovery.as_mut().unwrap();
            recovery.phase = Phase::ClosingOld;
            recovery.media_work(WorkKind::CloseOld, "disconnect", json!({}));
            return Ok(());
        }
        if recovery.power_suspended
            || self.state.connection_phase.suspended()
            || self.sends.retry_in_flight()
        {
            return Ok(());
        }
        if !self.voice_permissions().0
            || self
                .gateway
                .as_ref()
                .is_none_or(|gateway| !gateway.is_ready())
        {
            return Ok(());
        }
        if !recovery.eligible {
            return Ok(());
        }
        let Some(permit) = self
            .gateway
            .as_ref()
            .and_then(|gateway| gateway.try_voice_recovery())
        else {
            self.stop_transport_recovery("El episodio compartido ya consumió su intento o sigue bloqueado. No se abrió otro transporte.");
            return Ok(());
        };
        let recovery = self.voice_lifecycle.transport_recovery.as_mut().unwrap();
        recovery.permit = Some(permit);
        recovery.deadline = Instant::now() + ATTEMPT_LIMIT;
        match recovery.mode {
            Mode::Resume => {
                let Some(generation) = recovery.transport_generation.checked_add(1) else {
                    self.stop_transport_recovery(
                        "La generación de transporte no se pudo renovar de forma segura.",
                    );
                    return Ok(());
                };
                recovery.phase = Phase::Resuming;
                recovery.expected_generation = Some(generation);
                recovery.media_work(
                    WorkKind::Resume,
                    "resumeVoice",
                    json!({"mediaSession":recovery.media_session}),
                );
            }
            Mode::Fresh => {
                recovery.phase = Phase::FreshWaiting;
                recovery.expected_generation = Some(0);
                self.voice_started = false;
                self.voice_session.clear();
                self.voice_endpoint.clear();
                use zeroize::Zeroize;
                self.voice_token.zeroize();
                let gateway = self.gateway.clone().unwrap();
                let command = self.voice_state_update(false);
                let recovery = self.voice_lifecycle.transport_recovery.as_mut().unwrap();
                recovery.work = Some(Work {
                    kind: WorkKind::FreshState,
                    task: tokio::spawn(async move {
                        gateway
                            .send(command)
                            .await
                            .map(|_| json!({}))
                            .map_err(|error| error.to_string())
                    }),
                });
            }
        }
        Ok(())
    }
    async fn finish_transport_recovery(&mut self) -> Result<(), String> {
        let mut recovery = self.voice_lifecycle.transport_recovery.take().unwrap();
        if !recovery.current(self)
            || recovery.power_suspended
            || self.state.connection_phase.suspended()
            || self
                .gateway
                .as_ref()
                .is_none_or(|gateway| !gateway.is_ready())
            || !recovery
                .permit
                .as_ref()
                .is_some_and(|permit| permit.is_current())
        {
            self.voice_lifecycle.transport_recovery = Some(recovery);
            self.stop_transport_recovery("El contexto cambió antes de confirmar la recuperación.");
            return Ok(());
        }
        let value = recovery.ready.take().unwrap();
        if !recovery.finish_permit(true) {
            self.voice_lifecycle.transport_recovery = Some(recovery);
            self.stop_transport_recovery(
                "El episodio terminó antes de aplicar la confirmación de voz.",
            );
            return Ok(());
        }
        apply_readiness(&mut self.state, &value);
        recovery.phase = Phase::Restored;
        recovery.media_session = value["mediaSession"].as_u64().unwrap();
        recovery.transport_generation = value["transportGeneration"].as_u64().unwrap();
        recovery.expected_generation = Some(recovery.transport_generation);
        if !recovery.restore_camera.is_empty()
            && self.voice_lifecycle.selected_camera == recovery.restore_camera
        {
            self.voice_lifecycle.pending_camera =
                Some((recovery.restore_camera.clone(), Instant::now()));
        }
        if let Some(action) = recovery.restore_share.take()
            && self
                .voice_lifecycle
                .selected_share
                .as_ref()
                .is_some_and(|current| current.id == action.id && current.text == action.text)
        {
            self.voice_lifecycle.pending_share = Some((action, Instant::now()));
        }
        recovery.restore_camera.clear();
        self.voice_lifecycle.transport_recovery = Some(recovery);
        self.voice_lifecycle.join_wait = None;
        self.state.voice_error.clear();
        self.state.voice_connecting = false;
        self.sync_transport_gate();
        self.refresh_call();
        self.resume_explicit_capture().await?;
        self.resume_pending_views().await?;
        self.reload_deferred_view();
        Ok(())
    }
}
