//! Session-local duplicate prevention. Retain no request bodies or credentials.
use crate::api::{ErrorKind, ServiceError};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::time::Instant;

const CAPACITY: usize = 256;
const COMPLETED_WINDOW: Duration = Duration::from_secs(3);
type Key = [u8; 32];
#[derive(Clone, Copy)]
enum State {
    Pending,
    Unknown,
    Confirmed(Instant),
}
pub(crate) struct Operations {
    entries: Mutex<HashMap<Key, State>>,
}
impl Default for Operations {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}
pub(crate) struct Operation {
    registry: Arc<Operations>,
    key: Key,
    sending: bool,
    finished: bool,
    keep_confirmed: bool,
}
pub(crate) fn key(method: &str, endpoint: &str, body: Option<&Value>) -> Key {
    let mut digest = Sha256::new();
    digest.update(method.as_bytes());
    digest.update([0]);
    digest.update(endpoint.as_bytes());
    digest.update([0]);
    if let Some(body) = body {
        // A prepared message keeps its identity even if a selected file changes
        // after an uncertain upload. A new intent must have its own nonce.
        if body.get("enforce_nonce").and_then(Value::as_bool) == Some(true)
            && let Some(nonce) = body
                .get("nonce")
                .filter(|value| value.is_string() || value.is_number())
        {
            digest.update(b"nonce:");
            digest.update(nonce.to_string().as_bytes());
        } else {
            digest.update(body.to_string().as_bytes());
        }
    }
    digest.finalize().into()
}
impl Operations {
    #[cfg(test)]
    pub(crate) fn begin(self: &Arc<Self>, key: Key) -> Result<Operation, ServiceError> {
        self.begin_for(key, "POST")
    }
    pub(crate) fn begin_for(
        self: &Arc<Self>,
        key: Key,
        method: &str,
    ) -> Result<Operation, ServiceError> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        entries.retain(
            |_, state| !matches!(state, State::Confirmed(until) if *until <= Instant::now()),
        );
        if let Some(state) = entries.get(&key) {
            return Err(match state {
                State::Unknown => ServiceError::typed(
                    ErrorKind::OutcomeUnknown,
                    "El resultado de esta operación sigue sin confirmar. Compruébalo en Discord antes de preparar otra operación.",
                ),
                State::Pending => ServiceError::typed(
                    ErrorKind::OperationPending,
                    "Esta operación ya está pendiente. Espera su confirmación.",
                ),
                State::Confirmed(_) => ServiceError::typed(
                    ErrorKind::OperationCompleted,
                    "Esta operación acaba de completarse. Espera a que se actualice la vista.",
                ),
            });
        }
        if entries.len() >= CAPACITY {
            return Err(ServiceError::typed(
                ErrorKind::QueueFull,
                "El registro de operaciones pendientes está lleno. Comprueba los resultados sin confirmar antes de continuar.",
            ));
        }
        entries.insert(key, State::Pending);
        Ok(Operation {
            registry: self.clone(),
            key,
            sending: false,
            finished: false,
            keep_confirmed: method == "POST",
        })
    }
}
impl Operation {
    pub(crate) fn sending(&mut self) {
        self.sending = true;
    }
    pub(crate) fn finish(&mut self, result: &Result<Value, ServiceError>) {
        let mut entries = self
            .registry
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match result {
            Ok(_) if self.keep_confirmed => {
                entries.insert(
                    self.key,
                    State::Confirmed(Instant::now() + COMPLETED_WINDOW),
                );
            }
            Ok(_) => {
                // A confirmed setter may legitimately return to an earlier
                // value after an intervening edit or an inverse operation.
                entries.remove(&self.key);
            }
            Err(error)
                if self.sending
                    && matches!(error.kind, ErrorKind::OutcomeUnknown | ErrorKind::Network) =>
            {
                entries.insert(self.key, State::Unknown);
            }
            Err(_) => {
                entries.remove(&self.key);
            }
        }
        self.finished = true;
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        if !self.finished {
            let mut entries = self
                .registry
                .entries
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if self.sending {
                entries.insert(self.key, State::Unknown);
            } else {
                entries.remove(&self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn error(registry: &Arc<Operations>, key: Key) -> ErrorKind {
        match registry.begin(key) {
            Err(error) => error.kind,
            Ok(_) => panic!("Duplicate was accepted"),
        }
    }
    #[tokio::test(start_paused = true)]
    async fn pending_is_shared_and_unsent_cancellation_releases_only_its_own_operation() {
        let registry = Arc::new(Operations::default());
        let first = key("POST", "/one", Some(&json!({"a":1})));
        let other = key("POST", "/two", Some(&json!({"a":1})));
        let operation = registry.begin(first).unwrap();
        let _other = registry.begin(other).unwrap();
        assert_eq!(error(&registry.clone(), first), ErrorKind::OperationPending);
        drop(operation);
        assert!(registry.begin(first).is_ok());
        assert_eq!(error(&registry, other), ErrorKind::OperationPending);
    }
    #[tokio::test(start_paused = true)]
    async fn confirmed_double_click_is_blocked_briefly_without_disabling_future_intent() {
        let registry = Arc::new(Operations::default());
        let first = key("POST", "/one", None);
        let mut operation = registry.begin(first).unwrap();
        operation.sending();
        operation.finish(&Ok(json!({"id":"123"})));
        assert_eq!(error(&registry, first), ErrorKind::OperationCompleted);
        tokio::time::advance(COMPLETED_WINDOW).await;
        assert!(registry.begin(first).is_ok());
    }
    #[tokio::test(start_paused = true)]
    async fn confirmed_setters_can_reverse_without_clearing_pending_or_unknown_guards() {
        let registry = Arc::new(Operations::default());
        for method in ["PUT", "DELETE", "PUT", "PATCH", "PATCH"] {
            let key = key(method, "/resource", None);
            let mut operation = registry.begin_for(key, method).unwrap();
            assert!(
                matches!(registry.begin_for(key, method), Err(error) if error.kind == ErrorKind::OperationPending)
            );
            operation.sending();
            operation.finish(&Ok(Value::Null));
        }
        let key = key("PATCH", "/resource", None);
        let mut operation = registry.begin_for(key, "PATCH").unwrap();
        operation.sending();
        drop(operation);
        assert!(
            matches!(registry.begin_for(key, "PATCH"), Err(error) if error.kind == ErrorKind::OutcomeUnknown)
        );
    }
    #[tokio::test(start_paused = true)]
    async fn sent_cancellation_or_unknown_result_never_expires_and_capacity_cannot_evict_it() {
        let registry = Arc::new(Operations::default());
        for index in 0..CAPACITY {
            let mut operation = registry
                .begin(key("POST", &format!("/{index}"), None))
                .unwrap();
            operation.sending();
            if index % 2 == 0 {
                operation.finish(&Err(ServiceError::outcome_unknown()));
            }
        }
        tokio::time::advance(Duration::from_secs(3600)).await;
        assert_eq!(
            error(&registry, key("POST", "/0", None)),
            ErrorKind::OutcomeUnknown
        );
        assert_eq!(
            error(&registry, key("POST", "/overflow", None)),
            ErrorKind::QueueFull
        );
        assert_eq!(registry.entries.lock().unwrap().len(), CAPACITY);
    }
    #[test]
    fn nonce_is_stable_and_conclusive_rejection_can_be_corrected_without_retaining_private_body() {
        let first = key(
            "POST",
            "/one",
            Some(&json!({"nonce":"123","enforce_nonce":true,"content":"private a"})),
        );
        assert_eq!(
            first,
            key(
                "POST",
                "/one",
                Some(&json!({"nonce":"123","enforce_nonce":true,"content":"private b"}))
            )
        );
        assert_ne!(
            first,
            key(
                "POST",
                "/two",
                Some(&json!({"nonce":"123","enforce_nonce":true}))
            )
        );
        assert_ne!(
            first,
            key(
                "POST",
                "/one",
                Some(&json!({"nonce":"124","enforce_nonce":true}))
            )
        );
        let registry = Arc::new(Operations::default());
        let mut operation = registry.begin(first).unwrap();
        operation.sending();
        operation.finish(&Err(ServiceError::status("Rejected fixture", 400)));
        assert!(registry.begin(first).is_ok());
    }
}
