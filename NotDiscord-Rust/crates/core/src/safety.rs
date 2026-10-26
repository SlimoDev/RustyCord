//! Session-wide stop state. It contains no credentials or challenge payloads.
use crate::api::{ErrorKind, ServiceError};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SafetyState {
    #[default]
    Allowed,
    SynchronizationRequired,
    AuthenticationRequired,
    ProtocolUnsupported,
    VerificationRequired,
}
impl SafetyState {
    fn priority(self) -> u8 {
        match self {
            Self::Allowed => 0,
            Self::SynchronizationRequired => 1,
            Self::AuthenticationRequired => 2,
            Self::ProtocolUnsupported => 3,
            // A later rejection must not erase an unresolved verification.
            Self::VerificationRequired => 4,
        }
    }
    pub fn error(self) -> Option<ServiceError> {
        let (kind, message) = match self {
            Self::Allowed => return None,
            Self::SynchronizationRequired => (
                ErrorKind::Network,
                "La conexión dejó de estar sincronizada. Las operaciones están suspendidas; vuelve a conectar.",
            ),
            Self::VerificationRequired => (
                ErrorKind::VerificationRequired,
                "La sesión necesita una revisión de seguridad. Las operaciones de esta cuenta están suspendidas; continúa desde Discord oficial.",
            ),
            Self::AuthenticationRequired => (
                ErrorKind::AuthenticationRequired,
                "La sesión fue rechazada. Las operaciones están suspendidas; vuelve a iniciar sesión.",
            ),
            Self::ProtocolUnsupported => (
                ErrorKind::ProtocolUnsupported,
                "Discord rechazó esta versión del protocolo. Las operaciones están suspendidas hasta actualizar el cliente.",
            ),
        };
        Some(ServiceError::typed(kind, message))
    }
}

#[derive(Clone)]
pub struct SafetyGate {
    state: Arc<watch::Sender<SafetyState>>,
    observer: Option<Arc<BlockObserver>>,
}
struct BlockObserver {
    callback: Box<dyn Fn(SafetyState) + Send + Sync>,
    dispatch: Mutex<()>,
}
impl Default for SafetyGate {
    fn default() -> Self {
        let (state, _) = watch::channel(SafetyState::Allowed);
        Self {
            state: Arc::new(state),
            observer: None,
        }
    }
}
impl SafetyGate {
    /// A session owner may synchronously persist a terminal stop before the
    /// caller discards its API handle. The callback must not call block itself.
    /// It receives only a state, never credentials or challenge response data.
    pub fn with_observer(observer: impl Fn(SafetyState) + Send + Sync + 'static) -> Self {
        Self {
            observer: Some(Arc::new(BlockObserver {
                callback: Box::new(observer),
                dispatch: Mutex::new(()),
            })),
            ..Self::default()
        }
    }
    pub fn state(&self) -> SafetyState {
        *self.state.borrow()
    }
    /// Stop states only move forwards within one session. A fresh session needs
    /// a fresh gate; this method cannot clear a challenge by accepting Allowed.
    pub fn block(&self, state: SafetyState) {
        let changed = self.state.send_if_modified(|current| {
            if state.priority() > current.priority() {
                *current = state;
                true
            } else {
                false
            }
        });
        if changed && let Some(observer) = &self.observer {
            // send_if_modified has released the watch lock. Serializing only
            // callbacks prevents concurrent stops from persisting a downgrade.
            let _dispatch = observer.dispatch.lock().unwrap_or_else(|e| e.into_inner());
            (observer.callback)(self.state());
        }
    }
    pub fn check(&self) -> Result<(), ServiceError> {
        self.state().error().map_or(Ok(()), Err)
    }
    pub async fn wait_blocked(&self) -> SafetyState {
        let mut changes = self.state.subscribe();
        loop {
            let current = *changes.borrow_and_update();
            if current != SafetyState::Allowed {
                return current;
            }
            // The sender is retained by self, so closure is not a recovery path.
            if changes.changed().await.is_err() {
                return self.state();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn challenge_cannot_be_cleared_or_downgraded_and_wait_has_no_lost_wake() {
        let gate = SafetyGate::default();
        let other = gate.clone();
        let waiter = tokio::spawn(async move { other.wait_blocked().await });
        gate.block(SafetyState::VerificationRequired);
        gate.block(SafetyState::Allowed);
        gate.block(SafetyState::AuthenticationRequired);
        gate.block(SafetyState::ProtocolUnsupported);
        assert_eq!(waiter.await.unwrap(), SafetyState::VerificationRequired);
        assert_eq!(
            gate.check().unwrap_err().kind,
            ErrorKind::VerificationRequired
        );
        assert_eq!(gate.wait_blocked().await, SafetyState::VerificationRequired);
        assert!(SafetyGate::default().check().is_ok());
    }
    #[test]
    fn observer_is_shared_by_clones_and_sees_only_monotonic_stop_states() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let captured = observed.clone();
        let gate = SafetyGate::with_observer(move |state| {
            captured.lock().unwrap().push(state);
        });
        let other = gate.clone();
        other.block(SafetyState::SynchronizationRequired);
        gate.block(SafetyState::AuthenticationRequired);
        other.block(SafetyState::VerificationRequired);
        gate.block(SafetyState::Allowed);
        gate.block(SafetyState::ProtocolUnsupported);
        assert_eq!(
            *observed.lock().unwrap(),
            [
                SafetyState::SynchronizationRequired,
                SafetyState::AuthenticationRequired,
                SafetyState::VerificationRequired,
            ]
        );
        assert_eq!(gate.state(), SafetyState::VerificationRequired);
    }
}
