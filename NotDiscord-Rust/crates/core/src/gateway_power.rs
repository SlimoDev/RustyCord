//! Positive power invalidation of the existing Gateway owner. The epoch keeps
//! a suspend/resume pair visible even when both arrive before its next poll.
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct State {
    epoch: u64,
    paused: bool,
    ready_epoch: Option<u64>,
    writer: Option<CancellationToken>,
}
impl Default for State {
    fn default() -> Self {
        // `ready` itself is false until a handshake. Keeping epoch0 admissible
        // also preserves the explicit offline-recorder fixture's ready handle.
        Self {
            epoch: 0,
            paused: false,
            ready_epoch: Some(0),
            writer: None,
        }
    }
}
#[derive(Default)]
pub(super) struct PowerSignal {
    state: Mutex<State>,
    changed: Notify,
}
impl PowerSignal {
    pub(super) fn epoch(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .epoch
    }
    pub(super) fn paused(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .paused
    }
    pub(super) fn is_ready(&self, ready: &AtomicBool) -> bool {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        !state.paused && state.ready_epoch == Some(state.epoch) && ready.load(Ordering::Acquire)
    }
    pub(super) fn suspend(&self, ready: &AtomicBool) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.paused {
            state.epoch = state.epoch.saturating_add(1);
        }
        state.paused = true;
        state.ready_epoch = None;
        ready.store(false, Ordering::Release);
        // Stop the real writer immediately, including a pending sink flush.
        // The runtime still joins it before creating a replacement socket.
        if let Some(writer) = &state.writer {
            writer.cancel();
        }
        drop(state);
        self.changed.notify_waiters();
    }
    pub(super) fn resume(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .paused = false;
        // Never restore ready here: only a new READY/RESUMED can do so.
        self.changed.notify_waiters();
    }
    pub(super) fn register_writer(&self, epoch: u64, writer: CancellationToken) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.paused || state.epoch != epoch {
            writer.cancel();
        }
        state.writer = Some(writer);
    }
    pub(super) fn mark_ready(&self, epoch: u64, ready: &AtomicBool) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.paused || state.epoch != epoch {
            return false;
        }
        state.ready_epoch = Some(epoch);
        ready.store(true, Ordering::Release);
        true
    }
    pub(super) async fn changed(&self, epoch: u64) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.epoch() != epoch {
                return;
            }
            notified.await;
        }
    }
    pub(super) async fn resumed(&self) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.paused() {
                return;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn coalesced_suspend_resume_invalidates_old_ready_and_writer_until_a_new_handshake() {
        let signal = PowerSignal::default();
        let ready = AtomicBool::new(true);
        let epoch = signal.epoch();
        let writer = CancellationToken::new();
        signal.register_writer(epoch, writer.clone());
        signal.suspend(&ready);
        signal.suspend(&ready);
        assert_eq!(signal.epoch(), epoch + 1);
        assert!(writer.is_cancelled());
        assert!(!signal.is_ready(&ready));
        signal.resume();
        signal.resume();
        signal.changed(epoch).await;
        assert!(!signal.is_ready(&ready));
        assert!(!signal.mark_ready(epoch, &ready));
        assert!(signal.mark_ready(epoch + 1, &ready));
        assert!(signal.is_ready(&ready));
    }
    #[tokio::test]
    async fn power_episode_waits_for_new_resumed_before_voice_without_a_second_reconnect() {
        use crate::gateway::{GamePresencePolicy, GatewayHandle, Inner, RecoverySignal, runtime};
        use futures_util::{SinkExt, StreamExt};
        use serde_json::{Value, json};
        use std::{sync::Arc, time::Duration};
        use tokio::{net::TcpListener, sync::mpsc, time::timeout};
        use tokio_tungstenite::tungstenite::Message;
        use zeroize::Zeroizing;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (commands, incoming) = mpsc::channel(32);
        let (events, _received) = mpsc::channel(32);
        let ready = Arc::new(AtomicBool::new(false));
        let cancel = CancellationToken::new();
        let safety = crate::safety::SafetyGate::default();
        let recovery = Arc::new(RecoverySignal::default());
        let game = Arc::new(GamePresencePolicy::default());
        let handle = GatewayHandle {
            inner: Arc::new(Inner {
                commands,
                cancel: cancel.clone(),
                ready: ready.clone(),
                safety: safety.clone(),
                game_presence: game.clone(),
                recovery: recovery.clone(),
            }),
        };
        let policy = runtime::Policy {
            connect: Duration::from_millis(200),
            hello: Duration::from_millis(100),
            ready: Duration::from_millis(200),
            write: Duration::from_millis(60),
            backoff: Duration::from_millis(2),
            manual_cooldown: Duration::from_millis(20),
            min_heartbeat: Duration::from_millis(5),
            ..Default::default()
        };
        let task = tokio::spawn(runtime::run(
            Zeroizing::new("offline-power-fixture-credential-only".into()),
            None,
            incoming,
            events,
            cancel.clone(),
            ready,
            safety,
            format!("ws://{}/", listener.local_addr().unwrap()),
            policy,
            game,
            recovery,
        ));
        let (tcp, _) = timeout(Duration::from_secs(1), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut old = tokio_tungstenite::accept_async(tcp).await.unwrap();
        old.send(Message::Text(
            json!({"op":10,"d":{"heartbeat_interval":1000}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
        let Message::Text(auth) = timeout(Duration::from_secs(1), old.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        else {
            panic!("missing Identify");
        };
        assert_eq!(serde_json::from_str::<Value>(&auth).unwrap()["op"], 2);
        old.send(Message::Text(json!({"op":0,"s":7,"t":"READY","d":{"session_id":"synthetic-power-session","user":{"id":"42"}}}).to_string().into())).await.unwrap();
        timeout(Duration::from_secs(1), async {
            while !handle.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        handle.suspend_for_power();
        handle.suspend_for_power();
        assert!(!handle.is_ready());
        assert!(handle.try_voice_recovery().is_none());
        assert!(
            timeout(Duration::from_millis(35), listener.accept())
                .await
                .is_err(),
            "no socket while Windows is suspended"
        );
        handle.resume_after_power();
        handle.resume_after_power();
        let (tcp, _) = timeout(Duration::from_secs(1), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut resumed = tokio_tungstenite::accept_async(tcp).await.unwrap();
        resumed
            .send(Message::Text(
                json!({"op":10,"d":{"heartbeat_interval":1000}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let Message::Text(auth) = timeout(Duration::from_secs(1), resumed.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
        else {
            panic!("missing Resume");
        };
        assert_eq!(serde_json::from_str::<Value>(&auth).unwrap()["op"], 6);
        assert!(!handle.is_ready());
        assert!(
            handle.try_voice_recovery().is_none(),
            "pre-sleep READY has no authority"
        );
        resumed
            .send(Message::Text(
                json!({"op":0,"s":8,"t":"RESUMED","d":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        timeout(Duration::from_secs(1), async {
            while !handle.is_ready() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let voice = handle.try_voice_recovery().unwrap();
        let episode = voice.episode();
        assert!(handle.try_view_recovery().is_none());
        voice.finish(true);
        let viewer = handle.try_view_recovery().unwrap();
        assert_eq!(viewer.episode(), episode);
        viewer.finish(true);
        handle.resume_after_power();
        assert!(
            timeout(Duration::from_millis(35), listener.accept())
                .await
                .is_err(),
            "duplicate wake cannot start another Gateway"
        );
        cancel.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
}
