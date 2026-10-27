//! Rust owns every callback payload. Node-API receives only a coalesced wake,
//! never a credential/frame-bearing Event whose ownership a rejected call loses.
use super::{Event, Tag};
use crate::protocol::MediaError;
use napi::{Status, threadsafe_function::{ErrorStrategy, ThreadsafeFunction, ThreadsafeFunctionCallMode}};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc}, time::Duration};

pub(super) const WAKE_CAPACITY: usize = 1;
pub(super) const BATCH: usize = 32;
const CONTROL_MESSAGES: usize = 128;
const CONTROL_BYTES: usize = 64 * 1024 * 1024;
const MIC_BYTES: usize = 4096;

#[derive(Default)]
pub(super) struct Cancellation { cancelled: AtomicBool, notify: tokio::sync::Notify }
impl Cancellation {
    pub(super) fn cancel(&self) { self.cancelled.store(true, Ordering::Release); self.notify.notify_waiters(); }
    pub(super) fn is_cancelled(&self) -> bool { self.cancelled.load(Ordering::Acquire) }
    pub(super) async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified(); tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() { return; }
            notified.await;
        }
    }
}

struct Entry { event: Event, bytes: usize }
#[derive(Default)]
struct State {
    control: VecDeque<Entry>, messages: usize, bytes: usize,
    tick: bool, mic: Option<Event>, controls_since_mic: usize, scheduled: bool,
}
pub(super) struct Mailbox {
    state: Mutex<State>, closing: Arc<AtomicBool>, output: mpsc::Sender<Value>,
    pub(super) cancel: Arc<Cancellation>, faulted: AtomicBool,
    rejected: AtomicU64, coalesced: AtomicU64,
}
pub(super) struct Delivery { pub(super) event: Event, pub(super) bytes: usize }
impl Mailbox {
    pub(super) fn new(closing: Arc<AtomicBool>, output: mpsc::Sender<Value>) -> Self {
        Self { state: Mutex::new(State::default()), closing, output, cancel: Arc::new(Cancellation::default()),
            faulted: AtomicBool::new(false), rejected: AtomicU64::new(0), coalesced: AtomicU64::new(0) }
    }
    fn push(&self, event: Event) -> std::result::Result<(), &'static str> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        match event {
            Event::Tick => { if state.tick { self.coalesced.fetch_add(1, Ordering::Relaxed); } state.tick = true; }
            event @ Event::Callback(Tag::MicLevel, _) => {
                if event_bytes(&event) > MIC_BYTES { return Err("MEDIA_CALLBACK_TOO_LARGE"); }
                if state.mic.replace(event).is_some() { self.coalesced.fetch_add(1, Ordering::Relaxed); }
            }
            event => {
                let bytes = event_bytes(&event);
                if bytes > CONTROL_BYTES || state.messages >= CONTROL_MESSAGES
                    || state.bytes.saturating_add(bytes) > CONTROL_BYTES { return Err("MEDIA_EVENT_QUEUE_FULL"); }
                state.bytes += bytes; state.messages += 1;
                state.control.push_back(Entry { event, bytes });
            }
        }
        Ok(())
    }
    pub(super) fn pop(&self) -> Option<Delivery> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        // A queued tick must still enforce deadlines during a stream of control
        // callbacks. Mic samples are latest-value telemetry; do not starve MLS.
        if std::mem::take(&mut state.tick) { return Some(Delivery { event: Event::Tick, bytes: 0 }); }
        if state.controls_since_mic >= 8 || state.control.is_empty() {
            if let Some(event) = state.mic.take() { state.controls_since_mic = 0; return Some(Delivery { event, bytes: 0 }); }
        }
        let entry = state.control.pop_front()?;
        state.controls_since_mic = state.controls_since_mic.saturating_add(1);
        // Keep its reservation until dispatch finishes, including synchronous
        // native callbacks raised while the actor processes this event.
        Some(Delivery { event: entry.event, bytes: entry.bytes })
    }
    pub(super) fn complete(&self, bytes: usize) {
        if bytes == 0 { return; }
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.bytes = state.bytes.saturating_sub(bytes); state.messages = state.messages.saturating_sub(1);
    }
    fn reserve_wake(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.scheduled { return false; } state.scheduled = true; true
    }
    fn finish(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.scheduled = false;
        state.tick || state.mic.is_some() || !state.control.is_empty()
    }
    fn fail(&self, code: &'static str) {
        self.rejected.fetch_add(1, Ordering::Relaxed);
        if self.faulted.swap(true, Ordering::AcqRel) { return; }
        self.closing.store(true, Ordering::Release); self.cancel.cancel();
        *self.state.lock().unwrap_or_else(|error| error.into_inner()) = State::default();
        let _ = self.output.send(json!({"event":"fatal","error":{"code":code,
            "message":"Se cerró el motor multimedia porque su cola de control no pudo entregar un evento de forma segura."},
            "eventQueue":{"rejected":self.rejected.load(Ordering::Relaxed),"coalesced":self.coalesced.load(Ordering::Relaxed),
                "maxMessages":CONTROL_MESSAGES,"maxBytes":CONTROL_BYTES}}));
        // Closing/QueueFull may prevent *any* JS callback. Cancel sockets now;
        // a fixed watchdog also releases native devices if JS cannot clean up.
        // Only this isolated media helper exits, never the parent application.
        std::thread::spawn(|| { std::thread::sleep(Duration::from_millis(500)); std::process::exit(1); });
    }
    pub(super) fn actor_unavailable(&self) { self.fail("MEDIA_ACTOR_UNAVAILABLE"); }
}

#[derive(Clone)]
pub(super) struct Sender { wake: ThreadsafeFunction<(), ErrorStrategy::Fatal>, pub(super) mailbox: Arc<Mailbox> }
impl Sender {
    pub(super) fn new(wake: ThreadsafeFunction<(), ErrorStrategy::Fatal>, mailbox: Arc<Mailbox>) -> Self { Self { wake, mailbox } }
    pub(super) fn is_closing(&self) -> bool { self.mailbox.closing.load(Ordering::Acquire) }
    pub(super) fn reject_callback(&self) { self.mailbox.fail("MEDIA_CALLBACK_INVALID"); }
    pub(super) fn call(&self, event: Event, _mode: ThreadsafeFunctionCallMode) -> Status {
        if self.is_closing() { return Status::Closing; }
        if let Err(code) = self.mailbox.push(event) { self.mailbox.fail(code); return Status::QueueFull; }
        self.wake()
    }
    fn wake(&self) -> Status {
        if self.is_closing() { return Status::Closing; }
        if !self.mailbox.reserve_wake() { return Status::Ok; }
        // This is always NonBlocking, including when a native callback runs on
        // JavaScript's own thread. Coalescing permits at most one queued wake.
        let status = self.wake.call((), ThreadsafeFunctionCallMode::NonBlocking);
        if status != Status::Ok { self.mailbox.fail(if status == Status::Closing { "MEDIA_DISPATCH_CLOSING" } else { "MEDIA_DISPATCH_QUEUE_FULL" }); }
        status
    }
    pub(super) fn finish_dispatch(&self) { if self.mailbox.finish() { let _ = self.wake(); } }
}

fn value_bytes(value: &Value, depth: usize) -> usize {
    if depth > 64 { return CONTROL_BYTES + 1; }
    let mut bytes = std::mem::size_of::<Value>();
    match value {
        Value::String(text) => bytes = bytes.saturating_add(text.len()),
        Value::Array(values) => for value in values { bytes = bytes.saturating_add(value_bytes(value, depth + 1)); if bytes > CONTROL_BYTES { break; } },
        Value::Object(values) => for (key, value) in values { bytes = bytes.saturating_add(key.len() + 32).saturating_add(value_bytes(value, depth + 1)); if bytes > CONTROL_BYTES { break; } },
        _ => {}
    }
    bytes
}
fn event_bytes(event: &Event) -> usize {
    let payload = match event {
        Event::Request(value) => value_bytes(value, 0),
        Event::WsFrame(_, frame) => frame.bytes.len().saturating_add(value_bytes(&frame.data, 0)),
        Event::Callback(tag, values) => {
            let tag_bytes = match tag { Tag::Modes(_, a, b) => value_bytes(a, 0).saturating_add(value_bytes(b, 0)),
                Tag::Codecs(_, value) => value_bytes(value, 0),
                Tag::Inventory { request, part } | Tag::Configure { request, part } => value_bytes(request, 0).saturating_add(part.len()),
                Tag::Stats { request, .. } => value_bytes(request, 0), _ => 0 };
            values.iter().fold(tag_bytes, |bytes, value| bytes.saturating_add(value_bytes(value, 0)))
        }
        Event::WsError(_, error) => error.message.len(),
        Event::PreviewResult { source, .. } => source.len(), _ => 0,
    };
    std::mem::size_of::<Event>().saturating_add(payload).max(1)
}

pub(super) fn callback_delivery_error() -> MediaError {
    MediaError::new("MEDIA_CALLBACK_UNAVAILABLE", "No se pudo entregar un evento de control multimedia de forma segura.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_budget_keeps_inflight_reservations_and_telemetry_coalesces() {
        let (output, _) = mpsc::channel();
        let mailbox = Mailbox::new(Arc::new(AtomicBool::new(false)), output);
        for id in 0..CONTROL_MESSAGES { assert!(mailbox.push(Event::WsOpen(id as u64)).is_ok()); }
        assert!(mailbox.push(Event::WsOpen(999)).is_err());
        let delivery = mailbox.pop().unwrap();
        assert!(mailbox.push(Event::WsOpen(999)).is_err());
        mailbox.complete(delivery.bytes);
        assert!(mailbox.push(Event::WsOpen(999)).is_ok());
        assert!(mailbox.push(Event::Tick).is_ok()); assert!(mailbox.push(Event::Tick).is_ok());
        assert!(matches!(mailbox.pop().unwrap().event, Event::Tick));
        assert_eq!(mailbox.coalesced.load(Ordering::Relaxed), 1);
    }
}
