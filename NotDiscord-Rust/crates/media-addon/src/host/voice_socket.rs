//! One cancellable task per signalling session. Bounded data and control lanes
//! keep a stalled writer independent of reception, heartbeats and local close.
use super::{Event, Sender, socket_closed, socket_failure};
use super::dispatch_queue::{Cancellation, callback_delivery_error};
use crate::protocol::{self, MediaError, Result};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use napi::{Status, threadsafe_function::ThreadsafeFunctionCallMode};
use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};

const MAX_MESSAGES: usize = 64;
const MAX_BYTES: usize = 4 * protocol::MAX_FRAME;
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

struct Queued { message: Message, _bytes: OwnedSemaphorePermit, _message: OwnedSemaphorePermit }
#[derive(Clone)]
struct Outbox { sender: mpsc::Sender<Queued>, bytes: Arc<Semaphore>, messages: Arc<Semaphore> }
impl Outbox {
    fn channel() -> (Self, mpsc::Receiver<Queued>) {
        let (sender, receiver) = mpsc::channel(MAX_MESSAGES);
        (Self { sender, bytes: Arc::new(Semaphore::new(MAX_BYTES)), messages: Arc::new(Semaphore::new(MAX_MESSAGES)) }, receiver)
    }
    fn send(&self, message: Message) -> Result<()> {
        let length = message.len().max(1);
        if length > protocol::MAX_FRAME { return Err(MediaError::new("VOICE_SEND_TOO_LARGE", "La señalización de voz supera el límite de su protocolo.")); }
        let error = || MediaError::new("VOICE_SEND_QUEUE_FULL", "Se cerró la conexión porque la cola de señalización de voz alcanzó su límite.");
        let bytes = self.bytes.clone().try_acquire_many_owned(length as u32).map_err(|_| error())?;
        let count = self.messages.clone().try_acquire_owned().map_err(|_| error())?;
        self.sender.try_send(Queued { message, _bytes: bytes, _message: count }).map_err(|failure|
            match failure { mpsc::error::TrySendError::Closed(_) => MediaError::new("VOICE_SEND_CLOSED", "La conexión de voz ya se cerró."),
                mpsc::error::TrySendError::Full(_) => error() })
    }
}

#[derive(Default)]
struct HeartbeatState { first_unacknowledged: Option<Instant>, deadline: Option<Instant> }
#[derive(Default)]
pub(super) struct Heartbeat { state: Mutex<HeartbeatState>, changed: tokio::sync::Notify }
impl Heartbeat {
    pub(super) fn sent(&self, now: Instant, interval: Duration) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.deadline.is_some_and(|deadline| now >= deadline) { return Err(heartbeat_timeout()); }
        if state.first_unacknowledged.is_none() {
            let deadline = now.checked_add(interval.saturating_mul(2)).ok_or_else(heartbeat_timeout)?;
            state.first_unacknowledged = Some(now); state.deadline = Some(deadline);
        }
        drop(state); self.changed.notify_waiters(); Ok(())
    }
    fn acknowledge(&self) {
        *self.state.lock().unwrap_or_else(|error| error.into_inner()) = HeartbeatState::default();
        self.changed.notify_waiters();
    }
    fn deadline(&self) -> Option<Instant> { self.state.lock().unwrap_or_else(|error| error.into_inner()).deadline }
    pub(super) fn first_pending(&self) -> Option<Instant> { self.state.lock().unwrap_or_else(|error| error.into_inner()).first_unacknowledged }
}
fn heartbeat_timeout() -> MediaError { MediaError::new("VOICE_HEARTBEAT_TIMEOUT", "El servidor de voz dejó de confirmar los latidos dentro del plazo permitido.") }

pub(super) struct Socket {
    outbox: Outbox, cancel: Arc<Cancellation>, task: tokio::task::AbortHandle,
    pub(super) heartbeat: Arc<Heartbeat>,
    failed: AtomicBool,
}
impl Socket {
    pub(super) fn send(&self, message: Message) -> Result<()> { self.outbox.send(message) }
    pub(super) fn cancel(&self) { self.cancel.cancel(); self.task.abort(); }
    pub(super) fn fail_once(&self) -> bool {
        let first = !self.failed.swap(true, Ordering::AcqRel); self.cancel(); first
    }
}
impl Drop for Socket { fn drop(&mut self) { self.cancel(); } }

pub(super) fn spawn(runtime: &tokio::runtime::Handle, endpoint: String, id: u64, sender: Sender) -> Socket {
    let (outbox, receiver) = Outbox::channel();
    let cancel = Arc::new(Cancellation::default()); let stop = cancel.clone();
    let heartbeat = Arc::new(Heartbeat::default()); let heart = heartbeat.clone();
    let global = sender.mailbox.cancel.clone();
    let task = runtime.spawn(async move {
        let error = tokio::select! { biased;
            _ = stop.cancelled() => return,
            _ = global.cancelled() => return,
            result = run(endpoint, id, sender.clone(), receiver, heart) => result,
        };
        if !stop.is_cancelled() && !global.is_cancelled() {
            let _ = sender.call(Event::WsError(id, error.0, error.1), ThreadsafeFunctionCallMode::NonBlocking);
        }
    });
    Socket { outbox, cancel, task: task.abort_handle(), heartbeat, failed: AtomicBool::new(false) }
}

fn config() -> WebSocketConfig {
    WebSocketConfig::default().read_buffer_size(16 * 1024).write_buffer_size(0)
        .max_write_buffer_size(2 * protocol::MAX_FRAME)
        .max_message_size(Some(protocol::MAX_FRAME)).max_frame_size(Some(protocol::MAX_FRAME))
}

async fn run(endpoint: String, id: u64, sender: Sender, receiver: mpsc::Receiver<Queued>, heartbeat: Arc<Heartbeat>) -> (&'static str, MediaError) {
    let socket = match tokio::time::timeout(CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_with_config(&endpoint, Some(config()), false)).await {
        Ok(Ok((socket, _))) => socket,
        Ok(Err(error)) => return ("connect", socket_failure(&error)),
        Err(_) => return ("connect", MediaError::new("VOICE_SOCKET_TIMEOUT", "El servidor de voz no respondió a tiempo al abrir la conexión.")),
    };
    if sender.call(Event::WsOpen(id), ThreadsafeFunctionCallMode::NonBlocking) != Status::Ok { return ("dispatch", callback_delivery_error()); }
    let (write, read) = socket.split();
    // WebSocket Pongs have reserved bounded capacity and bypass the data
    // backlog. They still obey the same finite per-write deadline.
    let (pong_sender, pongs) = mpsc::channel(2);
    tokio::select! {
        error = writer(write, receiver, pongs) => ("write", error),
        error = reader(read, id, sender, pong_sender, heartbeat) => ("read", error),
    }
}

async fn writer<W>(mut write: W, mut receiver: mpsc::Receiver<Queued>, mut pongs: mpsc::Receiver<Message>) -> MediaError
where W: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin {
    loop {
        // Keep the data reservation alive until the whole send/flush returns.
        let (message, reservation) = tokio::select! { biased;
            pong = pongs.recv() => match pong { Some(message) => (message, None), None => return socket_closed(None) },
            queued = receiver.recv() => match queued {
                Some(queued) => { let Queued { message, _bytes, _message } = queued; (message, Some((_bytes, _message))) },
                None => return socket_closed(None),
            },
        };
        let result = tokio::time::timeout(WRITE_TIMEOUT, write.send(message)).await;
        drop(reservation);
        match result {
            Ok(Ok(())) => {}, Ok(Err(error)) => return socket_failure(&error),
            Err(_) => return MediaError::new("VOICE_WRITE_TIMEOUT", "El servidor de voz no recibió la señalización dentro del plazo de escritura."),
        }
    }
}

async fn reader<R>(mut read: R, id: u64, sender: Sender, pongs: mpsc::Sender<Message>, heartbeat: Arc<Heartbeat>) -> MediaError
where R: Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin {
    loop {
        let changed = heartbeat.changed.notified(); tokio::pin!(changed); changed.as_mut().enable();
        let deadline = heartbeat.deadline();
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) { return heartbeat_timeout(); }
        let expiry = async { match deadline {
            Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
            None => std::future::pending::<()>().await,
        }};
        let incoming = tokio::select! { biased;
            _ = changed => continue,
            _ = expiry => { if heartbeat.deadline().is_some_and(|deadline| Instant::now() >= deadline) { return heartbeat_timeout(); } else { continue; } },
            incoming = read.next() => incoming,
        };
        let frame = match incoming {
            Some(Ok(Message::Text(text))) => protocol::json_frame(&text),
            Some(Ok(Message::Binary(bytes))) => protocol::binary_frame(&bytes),
            Some(Ok(Message::Ping(bytes))) => {
                if pongs.try_send(Message::Pong(bytes)).is_err() { return MediaError::new("VOICE_CONTROL_QUEUE_FULL", "La cola de control WebSocket no pudo entregar su respuesta de forma segura."); }
                continue;
            },
            Some(Ok(Message::Close(frame))) => return socket_closed(frame.as_ref().map(|frame| u16::from(frame.code))),
            Some(Err(error)) => return socket_failure(&error), None => return socket_closed(None), _ => continue,
        };
        let frame = match frame { Ok(frame) => frame, Err(error) => return error };
        // Receipt, rather than a delayed JS dispatch, confirms server liveness.
        // The actor still receives the frame for protocol sequence bookkeeping.
        if frame.op == 6 { heartbeat.acknowledge(); }
        if sender.call(Event::WsFrame(id, frame), ThreadsafeFunctionCallMode::NonBlocking) != Status::Ok { return callback_delivery_error(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retransmission_does_not_move_first_unacknowledged_deadline() {
        let heartbeat = Heartbeat::default(); let start = Instant::now(); let interval = Duration::from_secs(2);
        heartbeat.sent(start, interval).unwrap();
        heartbeat.sent(start + interval, interval).unwrap();
        assert_eq!(heartbeat.deadline(), Some(start + interval * 2));
        assert!(heartbeat.sent(start + interval * 2, interval).is_err());
        heartbeat.acknowledge(); heartbeat.sent(start + interval * 3, interval).unwrap();
        assert_eq!(heartbeat.deadline(), Some(start + interval * 5));
        assert_eq!(Heartbeat::default().deadline(), None);
    }
    #[test]
    fn messages_and_bytes_are_bounded_and_drop_releases_reservations() {
        let (outbox, mut receiver) = Outbox::channel();
        for _ in 0..MAX_MESSAGES { outbox.send(Message::Text("x".into())).unwrap(); }
        assert!(outbox.send(Message::Text("x".into())).is_err());
        let queued = receiver.try_recv().unwrap();
        assert!(outbox.send(Message::Text("x".into())).is_err());
        drop(queued); outbox.send(Message::Text("x".into())).unwrap();
        let (outbox, _receiver) = Outbox::channel();
        for _ in 0..4 { outbox.send(Message::Binary(vec![0; protocol::MAX_FRAME].into())).unwrap(); }
        assert!(outbox.send(Message::Text("x".into())).is_err());
        assert_eq!(config().max_message_size, Some(protocol::MAX_FRAME));
        assert_eq!(config().max_frame_size, Some(protocol::MAX_FRAME));
    }
}
