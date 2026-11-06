//! One cancellable task per signalling session. Bounded data and control lanes
//! keep a stalled writer independent of reception, heartbeats and local close.
use super::dispatch_queue::{Cancellation, callback_delivery_error};
use super::{Event, Sender, socket_closed, socket_failure};
use crate::protocol::{self, MediaError, Result};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use napi::{Status, threadsafe_function::ThreadsafeFunctionCallMode};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};

#[path = "voice_resume.rs"]
mod resume;
pub(super) use resume::{ResumeObservation, ResumeState};

/// Preserve only the numeric server code. Server-supplied close reasons never
/// enter diagnostics, IPC or recovery decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CloseCode(u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecoveryClass {
    ResumeCandidate,
    SessionInvalid,
    Terminal,
}
impl CloseCode {
    pub(super) fn new(code: u16) -> Self {
        Self(code)
    }
    pub(super) fn value(self) -> u16 {
        self.0
    }
    pub(super) fn disposition(self) -> RecoveryClass {
        match self.0 {
            4015 => RecoveryClass::ResumeCandidate,
            4006 | 4009 => RecoveryClass::SessionInvalid,
            // 4014/4021/4022 explicitly prohibit reconnect. Authentication,
            // protocol, encryption and unknown closes are conservative stops.
            _ => RecoveryClass::Terminal,
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Failure {
    pub(super) phase: &'static str,
    pub(super) error: MediaError,
    pub(super) close_code: Option<CloseCode>,
}
impl Failure {
    pub(super) fn new(phase: &'static str, error: MediaError) -> Self {
        Self {
            phase,
            error,
            close_code: None,
        }
    }
    fn closed(phase: &'static str, code: Option<u16>) -> Self {
        Self {
            phase,
            error: socket_closed(code),
            close_code: code.map(CloseCode::new),
        }
    }
    pub(super) fn disposition(&self) -> RecoveryClass {
        if let Some(code) = self.close_code {
            return code.disposition();
        }
        match self.error.code {
            "VOICE_SOCKET_NETWORK"
            | "VOICE_HEARTBEAT_TIMEOUT"
            | "VOICE_SOCKET_TIMEOUT"
            | "VOICE_WRITE_TIMEOUT"
            | "VOICE_SOCKET_CLOSED" => RecoveryClass::ResumeCandidate,
            _ => RecoveryClass::Terminal,
        }
    }
}

fn transport_error(error: &tokio_tungstenite::tungstenite::Error) -> MediaError {
    use tokio_tungstenite::tungstenite::{Error, error::ProtocolError};
    match error {
        // An EOF/reset without a Close frame is a transport interruption, not
        // evidence that our voice/DAVE protocol was rejected by the server.
        Error::Protocol(ProtocolError::ResetWithoutClosingHandshake) => MediaError::new(
            "VOICE_SOCKET_NETWORK",
            "La conexión de red con el servidor de voz se interrumpió.",
        ),
        _ => socket_failure(error),
    }
}

const MAX_MESSAGES: usize = 64;
const MAX_BYTES: usize = 4 * protocol::MAX_FRAME;
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

struct Queued {
    message: Message,
    _bytes: OwnedSemaphorePermit,
    _message: OwnedSemaphorePermit,
}
#[derive(Clone)]
struct Outbox {
    sender: mpsc::Sender<Queued>,
    bytes: Arc<Semaphore>,
    messages: Arc<Semaphore>,
}
impl Outbox {
    fn channel() -> (Self, mpsc::Receiver<Queued>) {
        let (sender, receiver) = mpsc::channel(MAX_MESSAGES);
        (
            Self {
                sender,
                bytes: Arc::new(Semaphore::new(MAX_BYTES)),
                messages: Arc::new(Semaphore::new(MAX_MESSAGES)),
            },
            receiver,
        )
    }
    fn send(&self, message: Message) -> Result<()> {
        let length = message.len().max(1);
        if length > protocol::MAX_FRAME {
            return Err(MediaError::new(
                "VOICE_SEND_TOO_LARGE",
                "La señalización de voz supera el límite de su protocolo.",
            ));
        }
        let error = || {
            MediaError::new(
                "VOICE_SEND_QUEUE_FULL",
                "Se cerró la conexión porque la cola de señalización de voz alcanzó su límite.",
            )
        };
        let bytes = self
            .bytes
            .clone()
            .try_acquire_many_owned(length as u32)
            .map_err(|_| error())?;
        let count = self
            .messages
            .clone()
            .try_acquire_owned()
            .map_err(|_| error())?;
        self.sender
            .try_send(Queued {
                message,
                _bytes: bytes,
                _message: count,
            })
            .map_err(|failure| match failure {
                mpsc::error::TrySendError::Closed(_) => {
                    MediaError::new("VOICE_SEND_CLOSED", "La conexión de voz ya se cerró.")
                }
                mpsc::error::TrySendError::Full(_) => error(),
            })
    }
}

#[derive(Default)]
struct HeartbeatState {
    first_unacknowledged: Option<Instant>,
    deadline: Option<Instant>,
}
#[derive(Default)]
pub(super) struct Heartbeat {
    state: Mutex<HeartbeatState>,
    changed: tokio::sync::Notify,
}
impl Heartbeat {
    pub(super) fn sent(&self, now: Instant, interval: Duration) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.deadline.is_some_and(|deadline| now >= deadline) {
            return Err(heartbeat_timeout());
        }
        if state.first_unacknowledged.is_none() {
            let deadline = now
                .checked_add(interval.saturating_mul(2))
                .ok_or_else(heartbeat_timeout)?;
            state.first_unacknowledged = Some(now);
            state.deadline = Some(deadline);
        }
        drop(state);
        self.changed.notify_waiters();
        Ok(())
    }
    fn acknowledge(&self) {
        *self.state.lock().unwrap_or_else(|error| error.into_inner()) = HeartbeatState::default();
        self.changed.notify_waiters();
    }
    fn deadline(&self) -> Option<Instant> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .deadline
    }
    pub(super) fn first_pending(&self) -> Option<Instant> {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .first_unacknowledged
    }
}
fn heartbeat_timeout() -> MediaError {
    MediaError::new(
        "VOICE_HEARTBEAT_TIMEOUT",
        "El servidor de voz dejó de confirmar los latidos dentro del plazo permitido.",
    )
}

pub(super) struct Socket {
    outbox: Outbox,
    cancel: Arc<Cancellation>,
    task: Option<tokio::task::JoinHandle<()>>,
    pub(super) heartbeat: Arc<Heartbeat>,
    failed: AtomicBool,
    generation: u64,
}
impl Socket {
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }
    pub(super) fn send(&self, message: Message) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(MediaError::new(
                "VOICE_SEND_CLOSED",
                "La conexión de voz ya se cerró.",
            ));
        }
        self.outbox.send(message)
    }
    pub(super) fn cancel(&self) {
        self.cancel.cancel();
        if let Some(task) = &self.task {
            task.abort();
        }
    }
    /// Cancel immediately; the replacement must retain and await this handle.
    /// Aborting without joining is not evidence that a split socket was dropped.
    pub(super) fn stop(mut self) -> StoppedSocket {
        self.cancel();
        StoppedSocket {
            task: self.task.take(),
        }
    }
    pub(super) fn fail_once(&self) -> bool {
        let first = !self.failed.swap(true, Ordering::AcqRel);
        self.cancel();
        first
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(super) struct StoppedSocket {
    task: Option<tokio::task::JoinHandle<()>>,
}
impl StoppedSocket {
    pub(super) async fn wait(mut self) {
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}
impl Drop for StoppedSocket {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

/// Exercises the production bounded outbox and cancellation without a socket,
/// DNS, TLS or a native engine. Available only in the diagnostic build.
#[cfg(feature = "native-probe")]
pub(super) struct OfflineReceiver {
    receiver: mpsc::Receiver<Queued>,
    cancel: Arc<Cancellation>,
}
#[cfg(feature = "native-probe")]
impl OfflineReceiver {
    pub(super) fn try_recv(&mut self) -> std::result::Result<Message, mpsc::error::TryRecvError> {
        self.receiver.try_recv().map(|queued| queued.message)
    }
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}
#[cfg(feature = "native-probe")]
pub(super) fn offline(runtime: &tokio::runtime::Handle) -> (Socket, OfflineReceiver) {
    let (outbox, receiver) = Outbox::channel();
    let cancel = Arc::new(Cancellation::default());
    let task = runtime.spawn(std::future::pending::<()>());
    (
        Socket {
            outbox,
            cancel: cancel.clone(),
            task: Some(task),
            heartbeat: Arc::new(Heartbeat::default()),
            failed: AtomicBool::new(false),
            generation: 0,
        },
        OfflineReceiver { receiver, cancel },
    )
}

pub(super) fn spawn(
    runtime: &tokio::runtime::Handle,
    endpoint: String,
    id: u64,
    sender: Sender,
) -> Socket {
    spawn_after(runtime, endpoint, id, 0, sender, None)
}

/// Exactly one connection attempt, already admitted by the shared coordinator.
/// There is no retry, Identify fallback, endpoint rotation or sleep/retry loop.
pub(super) fn spawn_after(
    runtime: &tokio::runtime::Handle,
    endpoint: String,
    id: u64,
    generation: u64,
    sender: Sender,
    predecessor: Option<StoppedSocket>,
) -> Socket {
    let (outbox, receiver) = Outbox::channel();
    let cancel = Arc::new(Cancellation::default());
    let stop = cancel.clone();
    let heartbeat = Arc::new(Heartbeat::default());
    let heart = heartbeat.clone();
    let global = sender.mailbox.cancel.clone();
    let task = runtime.spawn(async move {
        let error = tokio::select! { biased;
            _ = stop.cancelled() => return,
            _ = global.cancelled() => return,
            result = async {
                if let Some(predecessor) = predecessor { predecessor.wait().await; }
                run(endpoint, id, generation, sender.clone(), receiver, heart).await
            } => result,
        };
        if !stop.is_cancelled() && !global.is_cancelled() {
            let _ = sender.call(
                Event::WsError(id, generation, error),
                ThreadsafeFunctionCallMode::NonBlocking,
            );
        }
    });
    Socket {
        outbox,
        cancel,
        task: Some(task),
        heartbeat,
        failed: AtomicBool::new(false),
        generation,
    }
}

fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(16 * 1024)
        .write_buffer_size(0)
        .max_write_buffer_size(2 * protocol::MAX_FRAME)
        .max_message_size(Some(protocol::MAX_FRAME))
        .max_frame_size(Some(protocol::MAX_FRAME))
}

async fn run(
    endpoint: String,
    id: u64,
    generation: u64,
    sender: Sender,
    receiver: mpsc::Receiver<Queued>,
    heartbeat: Arc<Heartbeat>,
) -> Failure {
    let socket = match tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio_tungstenite::connect_async_with_config(&endpoint, Some(config()), false),
    )
    .await
    {
        Ok(Ok((socket, _))) => socket,
        Ok(Err(error)) => return Failure::new("connect", transport_error(&error)),
        Err(_) => {
            return Failure::new(
                "connect",
                MediaError::new(
                    "VOICE_SOCKET_TIMEOUT",
                    "El servidor de voz no respondió a tiempo al abrir la conexión.",
                ),
            );
        }
    };
    if sender.call(
        Event::WsOpen(id, generation),
        ThreadsafeFunctionCallMode::NonBlocking,
    ) != Status::Ok
    {
        return Failure::new("dispatch", callback_delivery_error());
    }
    let (write, read) = socket.split();
    // WebSocket Pongs have reserved bounded capacity and bypass the data
    // backlog. They still obey the same finite per-write deadline.
    let (pong_sender, pongs) = mpsc::channel(2);
    tokio::select! { biased;
        // Preserve a received numeric close instead of racing the writer's
        // resulting generic ConnectionClosed/Pong-channel shutdown.
        error = reader(read, id, generation, sender, pong_sender, heartbeat) => error,
        error = writer(write, receiver, pongs) => Failure::new("write", error),
    }
}

async fn writer<W>(
    mut write: W,
    mut receiver: mpsc::Receiver<Queued>,
    mut pongs: mpsc::Receiver<Message>,
) -> MediaError
where
    W: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
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
            Ok(Ok(())) => {}
            Ok(Err(error)) => return transport_error(&error),
            Err(_) => {
                return MediaError::new(
                    "VOICE_WRITE_TIMEOUT",
                    "El servidor de voz no recibió la señalización dentro del plazo de escritura.",
                );
            }
        }
    }
}

async fn reader<R>(
    mut read: R,
    id: u64,
    generation: u64,
    sender: Sender,
    pongs: mpsc::Sender<Message>,
    heartbeat: Arc<Heartbeat>,
) -> Failure
where
    R: Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let changed = heartbeat.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let deadline = heartbeat.deadline();
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Failure::new("heartbeat", heartbeat_timeout());
        }
        let expiry = async {
            match deadline {
                Some(deadline) => {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await
                }
                None => std::future::pending::<()>().await,
            }
        };
        let incoming = tokio::select! { biased;
            _ = changed => continue,
            _ = expiry => { if heartbeat.deadline().is_some_and(|deadline| Instant::now() >= deadline) { return Failure::new("heartbeat", heartbeat_timeout()); } else { continue; } },
            incoming = read.next() => incoming,
        };
        let frame = match incoming {
            Some(Ok(Message::Text(text))) => protocol::json_frame(&text),
            Some(Ok(Message::Binary(bytes))) => protocol::binary_frame(&bytes),
            Some(Ok(Message::Ping(bytes))) => {
                if pongs.try_send(Message::Pong(bytes)).is_err() {
                    return Failure::new(
                        "read",
                        MediaError::new(
                            "VOICE_CONTROL_QUEUE_FULL",
                            "La cola de control WebSocket no pudo entregar su respuesta de forma segura.",
                        ),
                    );
                }
                continue;
            }
            Some(Ok(Message::Close(frame))) => {
                return Failure::closed("read", frame.as_ref().map(|frame| u16::from(frame.code)));
            }
            Some(Err(error)) => return Failure::new("read", transport_error(&error)),
            None => return Failure::closed("read", None),
            _ => continue,
        };
        let frame = match frame {
            Ok(frame) => frame,
            Err(error) => return Failure::new("read", error),
        };
        // Receipt, rather than a delayed JS dispatch, confirms server liveness.
        // The actor still receives the frame for protocol sequence bookkeeping.
        if frame.op == 6 {
            heartbeat.acknowledge();
        }
        if sender.call(
            Event::WsFrame(id, generation, frame),
            ThreadsafeFunctionCallMode::NonBlocking,
        ) != Status::Ok
        {
            return Failure::new("dispatch", callback_delivery_error());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_close_policy_is_numeric_and_unknown_codes_cannot_enable_resume() {
        for code in [4014, 4021, 4022, 4004, 4012, 4016, 4017, 4999] {
            let failure = Failure::closed("read", Some(code));
            assert_eq!(failure.close_code.unwrap().value(), code);
            assert_eq!(failure.disposition(), RecoveryClass::Terminal);
        }
        assert_eq!(
            Failure::closed("read", Some(4015)).disposition(),
            RecoveryClass::ResumeCandidate
        );
        for code in [4006, 4009] {
            assert_eq!(
                Failure::closed("read", Some(code)).disposition(),
                RecoveryClass::SessionInvalid
            );
        }
        for code in [
            "VOICE_SOCKET_NETWORK",
            "VOICE_HEARTBEAT_TIMEOUT",
            "VOICE_WRITE_TIMEOUT",
        ] {
            assert_eq!(
                Failure::new("read", MediaError::new(code, "fixture")).disposition(),
                RecoveryClass::ResumeCandidate
            );
        }
        assert_eq!(
            Failure::new("read", MediaError::new("VOICE_SOCKET_PROTOCOL", "fixture")).disposition(),
            RecoveryClass::Terminal
        );
    }
    #[tokio::test]
    async fn stopped_socket_is_joined_before_a_replacement_can_open() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(dropped.clone());
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _guard = guard;
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        ready.await.unwrap();
        let (outbox, _receiver) = Outbox::channel();
        let socket = Socket {
            outbox,
            cancel: Arc::new(Cancellation::default()),
            task: Some(task),
            heartbeat: Arc::new(Heartbeat::default()),
            failed: AtomicBool::new(false),
            generation: 7,
        };
        assert_eq!(socket.generation(), 7);
        let predecessor = socket.stop();
        // This is the production handover wait used before connect_async.
        predecessor.wait().await;
        assert!(
            dropped.load(Ordering::Acquire),
            "the previous reader/writer must have released their socket"
        );
    }
    #[test]
    fn retransmission_does_not_move_first_unacknowledged_deadline() {
        let heartbeat = Heartbeat::default();
        let start = Instant::now();
        let interval = Duration::from_secs(2);
        heartbeat.sent(start, interval).unwrap();
        heartbeat.sent(start + interval, interval).unwrap();
        assert_eq!(heartbeat.deadline(), Some(start + interval * 2));
        assert!(heartbeat.sent(start + interval * 2, interval).is_err());
        heartbeat.acknowledge();
        heartbeat.sent(start + interval * 3, interval).unwrap();
        assert_eq!(heartbeat.deadline(), Some(start + interval * 5));
        assert_eq!(Heartbeat::default().deadline(), None);
    }
    #[test]
    fn messages_and_bytes_are_bounded_and_drop_releases_reservations() {
        let (outbox, mut receiver) = Outbox::channel();
        for _ in 0..MAX_MESSAGES {
            outbox.send(Message::Text("x".into())).unwrap();
        }
        assert!(outbox.send(Message::Text("x".into())).is_err());
        let queued = receiver.try_recv().unwrap();
        assert!(outbox.send(Message::Text("x".into())).is_err());
        drop(queued);
        outbox.send(Message::Text("x".into())).unwrap();
        let (outbox, _receiver) = Outbox::channel();
        for _ in 0..4 {
            outbox
                .send(Message::Binary(vec![0; protocol::MAX_FRAME].into()))
                .unwrap();
        }
        assert!(outbox.send(Message::Text("x".into())).is_err());
        assert_eq!(config().max_message_size, Some(protocol::MAX_FRAME));
        assert_eq!(config().max_frame_size, Some(protocol::MAX_FRAME));
    }
}
