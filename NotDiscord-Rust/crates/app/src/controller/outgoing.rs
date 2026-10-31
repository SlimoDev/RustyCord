//! Sole conciliation owner for captured sends. HTTP and Gateway both report here
//! by nonce, so a send is confirmed once and a late failure cannot undo that.
use super::*;
use notdiscord_core::api::{ErrorKind, ServiceError};

const CAPACITY: usize = 128;
const RETRY_BUSY: &str = "Espera a que termine el intento anterior.";
const STILL_UNKNOWN: &str = "No aparece entre los mensajes recientes, pero eso no demuestra que no se enviara. No se volvió a enviar.";

struct Entry { submission: String, revision: u64, channel: String, attempt: bool, confirmed: bool }
#[derive(Default)]
pub(super) struct Sends {
    entries: HashMap<String, Entry>,
    // Recovery work runs one at a time per account, whatever the message.
    retrying: Option<String>,
}
impl Sends {
    pub(super) fn clear(&mut self) { *self = Self::default(); }
    fn nonce_of(&self, submission: &str) -> Option<String> {
        self.entries.iter().find(|(_, entry)| entry.submission == submission).map(|(nonce, _)| nonce.clone())
    }
    /// False when this nonce already has an attempt in flight.
    fn begin(&mut self, nonce: &str, submission: &str, revision: u64, channel: &str, retry: bool) -> Result<bool, String> {
        if self.entries.get(nonce).is_some_and(|entry| entry.attempt) { return Ok(false); }
        if retry && self.retrying.is_some() { return Err(RETRY_BUSY.into()); }
        if !self.entries.contains_key(nonce) && self.entries.len() >= CAPACITY {
            return Err("Hay demasiados envíos sin confirmar. Resuelve alguno antes de continuar.".into());
        }
        self.entries.insert(nonce.into(), Entry { submission: submission.into(), revision,
            channel: channel.into(), attempt: true, confirmed: false });
        if retry { self.retrying = Some(nonce.into()); }
        Ok(true)
    }
    fn finish_attempt(&mut self, nonce: &str) {
        if self.retrying.as_deref() == Some(nonce) { self.retrying = None; }
        if let Some(entry) = self.entries.get_mut(nonce) { entry.attempt = false; }
    }
}
/// Types the failure before it becomes text. An unknown result is never offered
/// as repeatable: the Operations barrier and the UI both keep it stopped.
pub(super) fn failure(error: &ServiceError) -> Value {
    let class = match error.kind {
        ErrorKind::RateLimited => "wait",
        ErrorKind::OutcomeUnknown | ErrorKind::OperationPending | ErrorKind::OperationCompleted | ErrorKind::Network => "unknown",
        ErrorKind::AuthenticationRequired | ErrorKind::VerificationRequired
            | ErrorKind::ProtocolUnsupported | ErrorKind::PermissionDenied => "blocked",
        _ if error.status == Some(413) => "blocked",
        _ => "retryable",
    };
    json!({"class":class,"message":error.message,
        "retryAfterMs":error.retry_after.map(|delay|delay.as_millis().min(u64::MAX as u128) as u64)})
}
fn submission_of(extra: &Value) -> Option<(String, u64)> {
    let submission = s(extra, "submissionId");
    (!submission.is_empty()).then(|| (submission, extra["revision"].as_u64().unwrap_or(0)))
}
impl Controller {
    pub(super) fn begin_send(&mut self, nonce: &str, extra: &Value, channel: &str) -> Result<bool, String> {
        if self.transport_suspended() { return Err(super::connection::OFFLINE.into()); }
        let Some((submission, revision)) = submission_of(extra) else { return Ok(true); };
        self.sends.begin(nonce, &submission, revision, channel, b(extra, "retry"))
    }
    fn send_error(&self, channel: &str, extra: &Value, failure: Value) {
        let mut update = failure;
        update["channelId"] = json!(channel);
        update["submissionId"] = extra["submissionId"].clone();
        update["revision"] = extra["revision"].clone();
        self.data("send-error", update);
    }
    /// Rejected before leaving: the intent stays local and can be retried, unless
    /// the same reason would reject it again.
    pub(super) fn send_rejected(&mut self, action: &UiAction, error: &str) {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        if let Ok(nonce) = prepared_nonce(&extra) { self.sends.finish_attempt(&nonce); }
        let stopped = self.api.as_ref().is_some_and(|api| api.safety().state() != SafetyState::Allowed)
            || !self.state.channel_access.allows("send");
        self.send_error(&action.channel_id, &extra, json!({"class":if stopped {"blocked"} else {"retryable"},"message":error}));
    }
    /// The canonical row must already be in the snapshot the UI reads next, or
    /// the local one would disappear for a frame before it arrives.
    fn acknowledge(&self, channel: &str, submission: &str, revision: u64) {
        self.publish();
        self.flush_snapshot(true);
        self.data("send-ack", json!({"channelId":channel,"submissionId":submission,"revision":revision}));
    }
    fn confirm(&mut self, nonce: &str) {
        let Some(entry) = self.sends.entries.get_mut(nonce) else { return; };
        if entry.confirmed { return; }
        entry.confirmed = true;
        let (channel, submission, revision) = (entry.channel.clone(), entry.submission.clone(), entry.revision);
        if !entry.attempt { self.sends.entries.remove(nonce); }
        self.acknowledge(&channel, &submission, revision);
    }
    /// A replayed event finds no entry. Text or time alone is not identity.
    pub(super) fn observe_own_send(&mut self, value: &Value) {
        let message = parsing::message(value);
        if message.nonce.is_empty() || message.author.id != self.me() { return; }
        if self.sends.entries.get(&message.nonce).is_some_and(|entry| entry.channel == message.channel_id) {
            self.confirm(&message.nonce);
        }
    }
    pub(super) fn complete_send(&mut self, action: &UiAction, value: &Value) -> Result<(), String> {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        let nonce = prepared_nonce(&extra).ok();
        if let Some(nonce) = &nonce { self.sends.finish_attempt(nonce); }
        let confirmed = nonce.as_ref().and_then(|nonce| self.sends.entries.get(nonce)).is_some_and(|entry| entry.confirmed);
        if let Some(failure) = value.get("sendError") {
            if confirmed {
                if let Some(nonce) = &nonce { self.sends.entries.remove(nonce); }
                return Ok(());
            }
            if submission_of(&extra).is_none() { return Err(s(failure, "message")); }
            self.send_error(&action.channel_id, &extra, failure.clone());
            return Ok(());
        }
        let message = parsing::message(value);
        if !parsing::valid_id(&message.id) || message.channel_id != action.channel_id {
            if confirmed { return Ok(()); }
            return Err("No se pudo confirmar el envío. Conserva el borrador.".into());
        }
        self.store_sent(message);
        self.status("Mensaje enviado");
        match nonce {
            Some(nonce) if confirmed => { self.sends.entries.remove(&nonce); }
            Some(nonce) if self.sends.entries.contains_key(&nonce) => self.confirm(&nonce),
            _ => if let Some((submission, revision)) = submission_of(&extra) {
                self.acknowledge(&action.channel_id, &submission, revision);
            },
        }
        Ok(())
    }
    fn store_sent(&mut self, message: ChatMessage) {
        self.observe_message_delivery(&message.channel_id, &message.id, true);
        self.observe_message_preview(&message);
        self.cache.upsert(message);
        self.refresh_messages();
        self.decorate_dms();
        if !self.state.selected_guild_id.is_empty() { self.rebuild_channels(); }
    }
    pub(super) fn send_task_failed(&mut self, action: &UiAction) {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        let Ok(nonce) = prepared_nonce(&extra) else { return; };
        self.sends.finish_attempt(&nonce);
        if self.sends.entries.get(&nonce).is_some_and(|entry| entry.confirmed) {
            self.sends.entries.remove(&nonce);
        } else {
            self.send_error(&action.channel_id, &extra, json!({"class":"unknown","message":""}));
        }
    }
    /// Never a second POST: at most one bounded page, on explicit request.
    pub(super) fn reconcile_send(&mut self, mut action: UiAction) -> Result<(), String> {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        let nonce = prepared_nonce(&extra)?;
        let channel = Self::valid(&action.channel_id)?.to_owned();
        if self.transport_suspended() {
            self.send_error(&channel, &extra, json!({"class":"unknown","message":super::connection::OFFLINE}));
            return Ok(());
        }
        if self.sends.retrying.is_some() {
            self.send_error(&channel, &extra, json!({"class":"unknown","message":RETRY_BUSY}));
            return Ok(());
        }
        let api = self.api()?;
        self.sends.retrying = Some(nonce);
        action.kind = "__send-reconcile".into();
        self.launch(action, None, async move {
            api.get_once(&format!("channels/{channel}/messages?limit=50")).await.map_err(|error| error.to_string())
        });
        Ok(())
    }
    pub(super) fn complete_reconcile(&mut self, action: &UiAction, value: &Value) {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        let Ok(nonce) = prepared_nonce(&extra) else { return; };
        if self.sends.retrying.as_deref() == Some(nonce.as_str()) { self.sends.retrying = None; }
        let me = self.me();
        let found = items(value).iter().map(parsing::message)
            .find(|message| message.nonce == nonce && message.author.id == me && message.channel_id == action.channel_id);
        if let Some(message) = found {
            self.store_sent(message);
            if self.sends.entries.contains_key(&nonce) { self.confirm(&nonce); }
            else if let Some((submission, revision)) = submission_of(&extra) {
                self.acknowledge(&action.channel_id, &submission, revision);
            }
        } else {
            self.send_error(&action.channel_id, &extra, json!({"class":"unknown","message":STILL_UNKNOWN,"checked":true}));
        }
    }
    pub(super) fn reconcile_failed(&mut self, action: &UiAction) {
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        if let Ok(nonce) = prepared_nonce(&extra)
            && self.sends.retrying.as_deref() == Some(nonce.as_str()) { self.sends.retrying = None; }
        // The check itself failed, so it proved nothing: it can be asked again.
        self.send_error(&action.channel_id, &extra, json!({"class":"unknown","message":"No se pudo comprobar. El resultado sigue sin confirmar.","retryAfterMs":30_000}));
    }
    pub(super) fn forget_send(&mut self, action: &UiAction) {
        if let Some(nonce) = self.sends.nonce_of(&action.id) { self.sends.entries.remove(&nonce); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SUBMISSION: &str = "0123456789abcdef0123456789abcdef";
    fn fixture() -> (Controller, std::sync::mpsc::Receiver<UiUpdate>, UiAction, String) {
        let (mut c, updates) = Controller::test_fixture();
        c.state.current_user = Some(Person { id: "42".into(), ..Default::default() });
        let extra = json!({"submissionId":SUBMISSION,"revision":3});
        let nonce = prepared_nonce(&extra).unwrap();
        c.begin_send(&nonce, &extra, "20").unwrap();
        let mut action = UiAction::new("__sent");
        action.channel_id = "20".into();
        action.extra = extra.to_string();
        (c, updates, action, nonce)
    }
    fn acks(updates: &std::sync::mpsc::Receiver<UiUpdate>) -> (usize, usize) {
        updates.try_iter().fold((0, 0), |(ack, error), update| match update {
            UiUpdate::Data(kind, _) if kind == "send-ack" => (ack + 1, error),
            UiUpdate::Data(kind, _) if kind == "send-error" => (ack, error + 1),
            _ => (ack, error),
        })
    }
    fn message(nonce: Value) -> Value {
        json!({"id":"100","channel_id":"20","nonce":nonce,"author":{"id":"42"},"content":"x","timestamp":"2026-10-31T12:00:00+00:00"})
    }
    #[tokio::test]
    async fn gateway_then_http_timeout_stays_confirmed_once() {
        let (mut c, updates, action, nonce) = fixture();
        c.observe_own_send(&message(json!(nonce)));
        c.observe_own_send(&message(json!(nonce)));
        c.complete_send(&action, &json!({"sendError":{"class":"unknown","message":""}})).unwrap();
        assert_eq!(acks(&updates), (1, 0));
        assert!(c.sends.entries.is_empty());
    }
    #[tokio::test]
    async fn http_then_gateway_replay_confirms_once_and_unknown_can_still_be_confirmed() {
        let (mut c, updates, action, nonce) = fixture();
        c.complete_send(&action, &message(json!(nonce))).unwrap();
        c.observe_own_send(&message(json!(nonce)));
        assert_eq!(acks(&updates), (1, 0));
        let (mut c, updates, action, nonce) = fixture();
        c.complete_send(&action, &json!({"sendError":{"class":"unknown","message":""}})).unwrap();
        c.observe_own_send(&message(json!(nonce)));
        assert_eq!(acks(&updates), (1, 1));
    }
    #[test]
    fn another_author_channel_or_missing_nonce_is_not_identity() {
        let (mut c, updates, _, nonce) = fixture();
        c.observe_own_send(&json!({"id":"1","channel_id":"20","author":{"id":"42"},"content":"x"}));
        c.observe_own_send(&json!({"id":"1","channel_id":"20","nonce":nonce,"author":{"id":"7"}}));
        c.observe_own_send(&json!({"id":"1","channel_id":"21","nonce":nonce,"author":{"id":"42"}}));
        assert_eq!(acks(&updates), (0, 0));
    }
    #[test]
    fn integer_nonces_keep_their_digits_and_retries_are_serialized() {
        assert_eq!(parsing::message(&json!({"nonce":1234567890123456789_u64})).nonce, "1234567890123456789");
        let mut sends = Sends::default();
        assert_eq!(sends.begin("a", "s", 0, "c", true), Ok(true));
        assert_eq!(sends.begin("a", "s", 0, "c", true), Ok(false));
        assert!(sends.begin("b", "t", 0, "c", true).is_err());
        sends.finish_attempt("a");
        assert_eq!(sends.begin("b", "t", 0, "c", true), Ok(true));
    }
    #[test]
    fn failures_are_typed_without_inventing_a_retry() {
        assert_eq!(failure(&ServiceError::outcome_unknown())["class"], "unknown");
        assert_eq!(failure(&ServiceError::status("x", 403))["class"], "blocked");
        assert_eq!(failure(&ServiceError::status("x", 413))["class"], "blocked");
        assert_eq!(failure(&ServiceError::status("x", 404))["class"], "retryable");
        let mut limited = ServiceError::status("x", 429);
        limited.retry_after = Some(Duration::from_secs(5));
        assert_eq!(failure(&limited)["retryAfterMs"], 5000);
    }
}
