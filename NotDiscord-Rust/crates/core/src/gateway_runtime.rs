//! Bounded Gateway reader, UI delivery and prioritized writer. No account data
//! is logged. Test policies and loopback URLs are private to this module.
use super::*;
use futures_util::{Sink, stream::SplitSink};
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
const PENDING_EVENTS: usize = 32;
const PENDING_EVENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 15 * 1024;
const USER_QUEUE: usize = 32;
const CONTROL_QUEUE: usize = 8;

#[derive(Clone, Copy)]
pub(super) struct Policy {
    pub connect: Duration,
    pub hello: Duration,
    pub ready: Duration,
    pub write: Duration,
    pub stable: Duration,
    pub backoff: Duration,
    pub identify_gap: Duration,
    pub rate_cooldown: Duration,
    pub min_heartbeat: Duration,
    pub attempts: u32,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            hello: Duration::from_secs(10),
            ready: Duration::from_secs(20),
            write: Duration::from_secs(2),
            stable: Duration::from_secs(60),
            backoff: Duration::from_secs(1),
            identify_gap: Duration::from_secs(5),
            rate_cooldown: Duration::from_secs(60),
            min_heartbeat: Duration::from_secs(1),
            attempts: 6,
        }
    }
}

struct BufferedEvent {
    event: GatewayEvent,
    bytes: usize,
    since: Instant,
}
struct Delivery {
    output: mpsc::Sender<GatewayEvent>,
    pending: VecDeque<BufferedEvent>,
    bytes: usize,
}
impl Delivery {
    fn new(output: mpsc::Sender<GatewayEvent>) -> Self {
        Self {
            output,
            pending: VecDeque::new(),
            bytes: 0,
        }
    }
    fn push(&mut self, event: GatewayEvent, safety: &SafetyGate) -> bool {
        if self.pending.is_empty() {
            match self.output.try_send(event) {
                Ok(()) => return true,
                Err(mpsc::error::TrySendError::Closed(_)) => return false,
                Err(mpsc::error::TrySendError::Full(event)) => return self.buffer(event, safety),
            }
        }
        self.buffer(event, safety)
    }
    fn buffer(&mut self, event: GatewayEvent, safety: &SafetyGate) -> bool {
        let bytes = event_size(&event);
        if self.pending.len() >= PENDING_EVENTS
            || self.bytes.saturating_add(bytes) > PENDING_EVENT_BYTES
        {
            safety.block(SafetyState::SynchronizationRequired);
            return false;
        }
        self.bytes += bytes;
        self.pending.push_back(BufferedEvent {
            event,
            bytes,
            since: Instant::now(),
        });
        true
    }
    fn deadline(&self) -> Option<Instant> {
        self.pending
            .front()
            .map(|event| event.since + EVENT_DELIVERY_TIMEOUT)
    }
    fn send(&mut self, permit: mpsc::Permit<'_, GatewayEvent>) {
        if let Some(event) = self.pending.pop_front() {
            self.bytes -= event.bytes;
            permit.send(event.event);
        }
    }
}
fn event_size(event: &GatewayEvent) -> usize {
    fn value_size(value: &Value, budget: &mut usize) -> usize {
        if *budget == 0 {
            return PENDING_EVENT_BYTES + 1;
        }
        *budget -= 1;
        match value {
            Value::String(value) => value.len().saturating_add(32),
            Value::Array(values) => values.iter().fold(32usize, |size, value| {
                size.saturating_add(value_size(value, budget))
            }),
            Value::Object(values) => values.iter().fold(64usize, |size, (key, value)| {
                size.saturating_add(key.len())
                    .saturating_add(48)
                    .saturating_add(value_size(value, budget))
            }),
            _ => 32,
        }
    }
    match event {
        GatewayEvent::Dispatch { kind, data } => {
            kind.len().saturating_add(value_size(data, &mut 131_072))
        }
        GatewayEvent::Status(value) => value.len() + 64,
        GatewayEvent::Safety(_) => 64,
    }
}

#[derive(Default)]
struct Budget {
    sent: VecDeque<(Instant, bool)>,
    presence: VecDeque<Instant>,
}
impl Budget {
    fn prune(&mut self, now: Instant) {
        while self
            .sent
            .front()
            .is_some_and(|(at, _)| *at + Duration::from_secs(60) <= now)
        {
            self.sent.pop_front();
        }
        while self
            .presence
            .front()
            .is_some_and(|at| *at + Duration::from_secs(20) <= now)
        {
            self.presence.pop_front();
        }
    }
    fn control_allowed(&mut self) -> bool {
        self.prune(Instant::now());
        self.sent.len() < 120
    }
    fn user_at(&mut self, presence: bool) -> Instant {
        let now = Instant::now();
        self.prune(now);
        let mut at = now;
        if self.sent.len() >= 110 {
            at = at.max(self.sent.front().unwrap().0 + Duration::from_secs(60));
        }
        if self.sent.iter().filter(|(_, user)| *user).count() >= 100 {
            at = at
                .max(self.sent.iter().find(|(_, user)| *user).unwrap().0 + Duration::from_secs(60));
        }
        if presence && self.presence.len() >= 5 {
            at = at.max(*self.presence.front().unwrap() + Duration::from_secs(20));
        }
        at
    }
    fn record(&mut self, user: bool, presence: bool) {
        let now = Instant::now();
        self.prune(now);
        self.sent.push_back((now, user));
        if presence {
            self.presence.push_back(now);
        }
    }
}
fn finish(envelope: Envelope, result: Result<(), ServiceError>) {
    if let Some(done) = envelope.done {
        let _ = done.send(result);
    }
}
fn coalescible(command: &GatewayCommand) -> u8 {
    match command {
        GatewayCommand::Presence { .. } | GatewayCommand::RichPresence { .. } => 1,
        GatewayCommand::SubscribeGuild { .. } => 2,
        _ => 0,
    }
}
fn is_presence(command: &GatewayCommand) -> bool {
    matches!(command, GatewayCommand::Presence { .. } | GatewayCommand::RichPresence { .. })
}

#[derive(Clone)]
enum ActivitySelection {
    Legacy(Option<String>),
    Rich(Option<crate::game_presence::GamePresence>, u64),
}
impl ActivitySelection {
    fn has_activity(&self) -> bool {
        match self { Self::Legacy(value) => value.is_some(), Self::Rich(value, _) => value.is_some() }
    }
    fn command(&self, status: &str, show_current_game: Option<bool>, policy: &GamePresencePolicy) -> GatewayCommand {
        self.command_for_permission(status,show_current_game,policy.current())
    }
    fn envelope(&self, status: &str, show_current_game: Option<bool>, policy: &GamePresencePolicy) -> Envelope {
        let permission=policy.current();
        Envelope {command:self.command_for_permission(status,show_current_game,permission),done:None,game_epoch:permission.0}
    }
    fn command_for_permission(&self, status: &str, show_current_game: Option<bool>, permission: (u64,bool)) -> GatewayCommand {
        match self {
            Self::Legacy(activity) => GatewayCommand::Presence {
                status: status.into(), activity: if show_current_game == Some(false) { None } else { activity.clone() },
            },
            Self::Rich(activity, epoch) => GatewayCommand::RichPresence {
                status: status.into(),
                activity: if permission != (*epoch, true) || show_current_game == Some(false) || matches!(status, "invisible" | "offline") { None } else { activity.clone() },
            },
        }
    }
}
fn synchronize_game_policy(activity: &mut ActivitySelection, requested: &mut bool, policy: &GamePresencePolicy) -> bool {
    let (epoch, allowed) = policy.current();
    if !allowed || matches!(activity,ActivitySelection::Rich(_,old) if *old!=epoch) {
        let changed = !matches!(activity,ActivitySelection::Rich(None,old) if *old==epoch);
        *activity = ActivitySelection::Rich(None,epoch);*requested = true;return changed;
    }
    false
}
fn disconnected_command(envelope: Envelope, status: &mut String, activity: &mut ActivitySelection, requested: &mut bool, policy: &GamePresencePolicy) {
    if envelope.done.is_none()
        && let GatewayCommand::RichPresence { status: chosen, activity: None } = &envelope.command
        && command_payload(&envelope.command, "").is_ok()
        && policy.check(&envelope.command,envelope.game_epoch).is_ok()
    {
        *status = chosen.clone();
        *activity = ActivitySelection::Rich(None,envelope.game_epoch);
        *requested = true;
        // Local desired-state withdrawal has no remote completion to fabricate.
        return;
    }
    finish(envelope, Err(ServiceError::new("La conexión aún no está preparada.")));
}
fn drain_disconnected_clears(commands: &mut mpsc::Receiver<Envelope>, status: &mut String, activity: &mut ActivitySelection, requested: &mut bool, policy: &GamePresencePolicy) {
    synchronize_game_policy(activity,requested,policy);
    while let Ok(envelope) = commands.try_recv() { disconnected_command(envelope, status, activity, requested,policy); }
}
fn queue(pending: &mut VecDeque<Envelope>, envelope: Envelope) -> Result<(), Envelope> {
    let key = coalescible(&envelope.command);
    if key != 0
        && let Some(index) = pending
            .iter()
            .position(|old| coalescible(&old.command) == key)
    {
        let previous = pending.remove(index).unwrap();
        finish(
            previous,
            Err(ServiceError::new(
                "La actualización fue sustituida por una selección más reciente.",
            )),
        );
    }
    if pending.len() >= USER_QUEUE {
        return Err(envelope);
    }
    pending.push_back(envelope);
    Ok(())
}
fn next_user(pending: &VecDeque<Envelope>, budget: &mut Budget) -> Option<(usize, Instant)> {
    pending
        .iter()
        .enumerate()
        .map(|(index, envelope)| {
            (
                index,
                budget.user_at(is_presence(&envelope.command)),
            )
        })
        .min_by_key(|(index, at)| (*at, *index))
}
#[derive(Default)]
struct SubscriptionState {
    guild: String,
    latest: Option<(String, String, i32, Value)>,
}
#[derive(Default)]
struct IdentifyWindow {
    latest: Mutex<Option<Instant>>,
}
impl IdentifyWindow {
    fn begin(&self, max_write: Duration) {
        *self
            .latest
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Instant::now() + max_write);
    }
    fn confirmed(&self) {
        *self
            .latest
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
    }
    fn until(&self, gap: Duration) -> Option<Instant> {
        self.latest
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .map(|at| at + gap)
    }
}
impl SubscriptionState {
    fn payload(&mut self, command: &GatewayCommand) -> Result<Value, ServiceError> {
        if let GatewayCommand::SubscribeGuild {
            guild_id,
            channel_id,
            range_end,
        } = command
        {
            if let Some((guild, channel, range, payload)) = &self.latest
                && guild == guild_id
                && channel == channel_id
                && range == range_end
            {
                return Ok(payload.clone());
            }
            let payload = command_payload(command, &self.guild)?;
            self.latest = Some((
                guild_id.clone(),
                channel_id.clone(),
                *range_end,
                payload.clone(),
            ));
            Ok(payload)
        } else {
            command_payload(command, &self.guild)
        }
    }
    fn sent(&mut self, command: &GatewayCommand) {
        if let GatewayCommand::SubscribeGuild { guild_id, .. } = command {
            self.guild = guild_id.clone();
        }
    }
}
async fn write<S>(
    sink: &mut S,
    message: Message,
    safety: &SafetyGate,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<(), ServiceError>
where
    S: Sink<Message> + Unpin,
{
    safety.check()?;
    tokio::select! {
        biased;
        _=cancel.cancelled()=>Err(ServiceError::network()),
        state=safety.wait_blocked()=>Err(state.error().unwrap()),
        result=tokio::time::timeout(timeout,sink.send(message))=>match result {Ok(Ok(()))=>Ok(()),_=>Err(ServiceError::network())},
    }
}
async fn writer(
    mut sink: SplitSink<Socket, Message>,
    mut controls: mpsc::Receiver<Control>,
    mut users: mpsc::Receiver<Envelope>,
    safety: SafetyGate,
    cancel: CancellationToken,
    policy: Policy,
    subscription: Arc<Mutex<SubscriptionState>>,
    identify_window: Arc<IdentifyWindow>,
    game_policy: Arc<GamePresencePolicy>,
) -> Result<(), ServiceError> {
    let mut pending = VecDeque::new();
    let mut budget = Budget::default();
    let mut last_presence = None;
    let mut last_subscription = None;
    let mut admissions_since_flush = 0usize;
    loop {
        let far = Instant::now() + Duration::from_secs(86_400);
        let next = next_user(&pending, &mut budget);
        let user_at = next.map(|(_, at)| at).unwrap_or(far);
        tokio::select! {
            biased;
            _=cancel.cancelled()=>break,
            _=safety.wait_blocked()=>break,
            control=controls.recv()=>{
                let Some(control)=control else {break};
                let Control::Send{message:control,identify}=control else {
                    // tungstenite queues its Pong while reading Ping. Flush
                    // that response instead of manufacturing a second Pong.
                    tokio::select! {
                        biased;
                        _=cancel.cancelled()=>break,
                        _=safety.wait_blocked()=>break,
                        result=tokio::time::timeout(policy.write,sink.flush())=>match result {Ok(Ok(()))=>{},_=>return Err(ServiceError::network())},
                    }
                    continue;
                };
                let counted=control.is_text();
                if counted && !budget.control_allowed() {safety.block(SafetyState::SynchronizationRequired);break;}
                if counted {budget.record(false,false);}
                if identify {identify_window.begin(policy.write);}
                control.write(&mut sink,&safety,&cancel,policy.write).await?;
                if identify {identify_window.confirmed();}
            },
            envelope=users.recv(),if pending.len()<USER_QUEUE && (admissions_since_flush<8 || user_at>Instant::now())=>{
                let Some(envelope)=envelope else {break};
                admissions_since_flush=admissions_since_flush.saturating_add(1);
                if let Err(envelope)=queue(&mut pending,envelope) {finish(envelope,Err(ServiceError::typed(crate::api::ErrorKind::QueueFull,"La cola del Gateway está llena.")));}
            },
            _=sleep_until(user_at),if !pending.is_empty()=>{
                admissions_since_flush=0;
                let envelope=pending.remove(next.unwrap().0).unwrap();
                if envelope.done.as_ref().is_some_and(|done|done.is_closed()) {continue;}
                if let Err(error)=game_policy.check(&envelope.command,envelope.game_epoch) {finish(envelope,Err(error));continue;}
                let payload=match subscription.lock().unwrap_or_else(|error|error.into_inner()).payload(&envelope.command) {Ok(value)=>value,Err(error)=>{finish(envelope,Err(error));continue}};
                let duplicate=match &envelope.command {GatewayCommand::Presence{..}|GatewayCommand::RichPresence{..}=>last_presence.as_ref()==Some(&payload),GatewayCommand::SubscribeGuild{..}=>last_subscription.as_ref()==Some(&payload),_=>false};
                if duplicate {finish(envelope,Ok(()));continue;}
                let text=payload.to_string();
                if text.len()>MAX_PAYLOAD_BYTES {finish(envelope,Err(ServiceError::new("La operación supera el tamaño admitido del Gateway.")));continue;}
                let presence=is_presence(&envelope.command);
                budget.record(true,presence);
                let result=write(&mut sink,Message::Text(text.into()),&safety,&cancel,policy.write).await;
                if result.is_ok() {
                    subscription.lock().unwrap_or_else(|error|error.into_inner()).sent(&envelope.command);
                    match &envelope.command {GatewayCommand::Presence{..}|GatewayCommand::RichPresence{..}=>last_presence=Some(payload),GatewayCommand::SubscribeGuild{..}=>last_subscription=Some(payload),_=>{}}
                }
                let failed=result.is_err();finish(envelope,result.map_err(|_|ServiceError::typed(crate::api::ErrorKind::OutcomeUnknown,"No se pudo confirmar el envío de la orden a Discord. Comprueba el estado antes de repetirla.")));
                if failed {return Err(ServiceError::network());}
                tokio::task::yield_now().await;
            }
        }
    }
    for envelope in pending {
        finish(envelope, Err(ServiceError::network()));
    }
    while let Ok(envelope) = users.try_recv() {
        finish(envelope, Err(ServiceError::network()));
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), sink.close()).await;
    Ok(())
}
enum Control {
    Send { message: ControlMessage, identify: bool },
    Flush,
}
struct AuthenticationText(Zeroizing<String>);
impl std::fmt::Debug for AuthenticationText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthenticationText([REDACTED])")
    }
}
enum ControlMessage {
    Plain(Message),
    Authentication(AuthenticationText),
}
impl ControlMessage {
    fn is_text(&self) -> bool {matches!(self,Self::Plain(Message::Text(_))|Self::Authentication(_))}
    async fn write<S>(self,sink:&mut S,safety:&SafetyGate,cancel:&CancellationToken,timeout:Duration)->Result<(),ServiceError>
    where S:Sink<Message>+Unpin {
        match self {
            Self::Plain(message)=>write(sink,message,safety,cancel,timeout).await,
            Self::Authentication(secret)=>{
                // The queue owns a wipeable buffer. Only the transport boundary
                // gets its required owned copy; its internal buffers are not
                // controlled by zeroize. Never log the message or that buffer.
                let message=Message::Text(secret.0.as_str().to_owned().into());
                let result=write(sink,message,safety,cancel,timeout).await;
                drop(secret);
                result
            }
        }
    }
}
#[derive(serde::Serialize)]
struct AuthenticationEnvelope<T> {op:u8,d:T}
#[derive(serde::Serialize)]
struct ResumeAuthentication<'a> {token:&'a str,session_id:&'a str,seq:Option<i64>}
#[derive(serde::Serialize)]
struct IdentifyAuthentication<'a> {
    token:&'a str,capabilities:u8,compress:bool,properties:IdentifyProperties,
    #[serde(skip_serializing_if="Option::is_none")]
    presence:Option<IdentifyPresence<'a>>,
}
#[derive(serde::Serialize)]
struct IdentifyProperties {os:&'static str,browser:&'static str,device:&'static str,system_locale:&'static str}
#[derive(serde::Serialize)]
struct IdentifyPresence<'a> {status:&'a str,since:u8,activities:[u8;0],afk:bool}
fn authentication_text(token:&str,session_id:Option<&str>,sequence:Option<i64>,status:&str)->Result<AuthenticationText,ServiceError> {
    // Borrow the credential instead of putting a cloned token into a JSON Value.
    let text=Zeroizing::new(if let Some(id)=session_id {
        serde_json::to_string(&AuthenticationEnvelope {op:6,d:ResumeAuthentication {token,session_id:id,seq:sequence}})
    } else {
        serde_json::to_string(&AuthenticationEnvelope {op:2,d:IdentifyAuthentication {token,capabilities:0,compress:false,
            properties:IdentifyProperties {os:"Windows",browser:"NotDiscordTerminal",device:"NotDiscordTerminal",system_locale:"es-ES"},
            presence:settings_proto::valid_presence(status).then_some(IdentifyPresence {status,since:0,activities:[],afk:false})}})
    }.map_err(|_|ServiceError::new("No se pudo preparar la autenticación del Gateway."))?);
    Ok(AuthenticationText(text))
}
fn authentication_control(tx:&mpsc::Sender<Control>,token:&str,session_id:Option<&str>,sequence:Option<i64>,status:&str,safety:&SafetyGate)->bool {
    if safety.check().is_err() {return false;}
    let Ok(secret)=authentication_text(token,session_id,sequence,status) else {safety.block(SafetyState::SynchronizationRequired);return false;};
    if secret.0.len()>MAX_PAYLOAD_BYTES || tx.try_send(Control::Send {
        message:ControlMessage::Authentication(secret),identify:session_id.is_none(),
    }).is_err() {safety.block(SafetyState::SynchronizationRequired);false} else {true}
}
fn control(tx: &mpsc::Sender<Control>, payload: Value, safety: &SafetyGate) -> bool {
    if safety.check().is_err() {
        return false;
    }
    let text = payload.to_string();
    let identify = payload["op"] == 2;
    if text.len() > MAX_PAYLOAD_BYTES
        || tx
            .try_send(Control::Send {
                message: ControlMessage::Plain(Message::Text(text.into())),
                identify,
            })
            .is_err()
    {
        safety.block(SafetyState::SynchronizationRequired);
        false
    } else {
        true
    }
}
fn user(tx: &mpsc::Sender<Envelope>, envelope: Envelope, safety: &SafetyGate) -> bool {
    match tx.try_send(envelope) {
        Ok(()) => true,
        Err(error) => {
            let envelope = error.into_inner();
            let local_only = envelope.done.is_none();
            finish(
                envelope,
                Err(ServiceError::typed(
                    crate::api::ErrorKind::QueueFull,
                    "La cola del Gateway está llena.",
                )),
            );
            if local_only {
                safety.block(SafetyState::SynchronizationRequired);
            }
            !local_only
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseAction {
    Resume,
    Fresh,
    RateLimited,
    Terminal(SafetyState),
}
fn close_action(code: u16) -> CloseAction {
    if let Some(state) = terminal_safety(code) {
        CloseAction::Terminal(state)
    } else {
        match code {
            1000 | 1001 | 4007 | 4009 => CloseAction::Fresh,
            4008 => CloseAction::RateLimited,
            _ => CloseAction::Resume,
        }
    }
}

pub(super) async fn run(
    token: impl Into<Arc<Zeroizing<String>>>,
    initial_status: Option<String>,
    mut commands: mpsc::Receiver<Envelope>,
    events: mpsc::Sender<GatewayEvent>,
    cancel: CancellationToken,
    ready: Arc<AtomicBool>,
    safety: SafetyGate,
    mut url: String,
    policy: Policy,
    game_policy: Arc<GamePresencePolicy>,
) {
    let token=token.into();
    let initial_url = url.clone();
    let mut delivery = Delivery::new(events.clone());
    let mut sequence = None;
    let mut session_id: Option<String> = None;
    let mut failures = 0u32;
    let mut status = initial_status
        .filter(|value| settings_proto::valid_presence(value))
        .unwrap_or_default();
    let mut activity = ActivitySelection::Legacy(None);
    let mut show_current_game = None;
    let mut presence_requested = false;
    let mut subscription: Option<GatewayCommand> = None;
    let subscription_wire = Arc::new(Mutex::new(SubscriptionState::default()));
    let identify_window = Arc::new(IdentifyWindow::default());
    'connections: while !cancel.is_cancelled() && safety.state() == SafetyState::Allowed {
        drain_disconnected_clears(&mut commands, &mut status, &mut activity, &mut presence_requested,&game_policy);
        if !delivery.push(GatewayEvent::Status("Conectando".into()), &safety) {
            break;
        }
        let config = WebSocketConfig::default()
            .max_message_size(Some(32 * 1024 * 1024))
            .max_frame_size(Some(32 * 1024 * 1024));
        let connection = tokio::select! {biased;_=cancel.cancelled()=>break,_=safety.wait_blocked()=>break,result=tokio::time::timeout(policy.connect,connect_async_with_config(&url,Some(config),false))=>result};
        let mut stable = false;
        let mut cooldown = Duration::ZERO;
        match connection {
            Ok(Ok((socket, _))) => {
                let (sink, mut stream) = socket.split();
                let (controls, control_rx) = mpsc::channel(CONTROL_QUEUE);
                let (users, user_rx) = mpsc::channel(USER_QUEUE);
                let writer_cancel = cancel.child_token();
                let mut writer_task = tokio::spawn(writer(
                    sink,
                    control_rx,
                    user_rx,
                    safety.clone(),
                    writer_cancel.clone(),
                    policy,
                    subscription_wire.clone(),
                    identify_window.clone(),
                    game_policy.clone(),
                ));
                let mut hello_at = Some(Instant::now() + policy.hello);
                let mut ready_at = None;
                let mut ready_since = None;
                let mut acknowledged = false;
                let mut resume_handshake = None;
                let mut handshake_complete = false;
                let mut prefer_command = false;
                let mut heartbeat_interval = Duration::from_secs(30);
                let mut heartbeat_at = None;
                let mut heartbeat_ack = true;
                loop {
                    let far = Instant::now() + Duration::from_secs(86_400);
                    let output = delivery.output.clone();
                    let emit_deadline = delivery.deadline();
                    tokio::select! {
                        biased;
                        _=cancel.cancelled()=>break,
                        _=safety.wait_blocked()=>break,
                        _=game_policy.changed.notified()=>{
                            if synchronize_game_policy(&mut activity,&mut presence_requested,&game_policy) && ready.load(Ordering::Acquire) && settings_proto::valid_presence(&status) {
                                if !user(&users,activity.envelope(&status,show_current_game,&game_policy),&safety){break;}
                            }
                        },
                        _=sleep_until(hello_at.unwrap_or(far)),if hello_at.is_some()=>break,
                        _=sleep_until(ready_at.unwrap_or(far)),if ready_at.is_some()=>break,
                        _=sleep_until(emit_deadline.unwrap_or(far)),if emit_deadline.is_some()=>{safety.block(SafetyState::SynchronizationRequired);break;},
                        _=sleep_until(heartbeat_at.unwrap_or(far)),if heartbeat_at.is_some()=>{
                            if !heartbeat_ack {break;}
                            heartbeat_ack=false;
                            if !control(&controls,json!({"op":1,"d":sequence}),&safety) {break;}
                            heartbeat_at=Some(Instant::now()+heartbeat_interval);
                        },
                        _=&mut writer_task=>{break;},
                        permit=output.reserve(),if !delivery.pending.is_empty()=>match permit {Ok(permit)=>delivery.send(permit),Err(_)=>{cancel.cancel();break;}},
                        received=stream.next(),if !prefer_command || commands.is_empty()=>{
                            prefer_command=true;
                            let Some(Ok(message))=received else {break};
                            let text=match message {
                                Message::Text(text)=>text,
                                Message::Ping(_)=>{if controls.try_send(Control::Flush).is_err(){safety.block(SafetyState::SynchronizationRequired);break;}continue;},
                                Message::Pong(_)=>continue,
                                Message::Close(frame)=>{if let Some(frame)=frame {match close_action(frame.code.into()) {CloseAction::Terminal(state)=>safety.block(state),CloseAction::Fresh=>{session_id=None;sequence=None;url=initial_url.clone();},CloseAction::RateLimited=>cooldown=policy.rate_cooldown,CloseAction::Resume=>{}}}break;},
                                _=>break,
                            };
                            let Ok(root)=serde_json::from_str::<Value>(&text) else {break};
                            if let Some(seq)=root["s"].as_i64() {sequence=Some(seq);}
                            let data=&root["d"];
                            match i(&root,"op") {
                                10=>{
                                    if hello_at.is_none() {safety.block(SafetyState::SynchronizationRequired);break;}
                                    let Some(interval)=data["heartbeat_interval"].as_u64().map(Duration::from_millis).filter(|interval|*interval>=policy.min_heartbeat && *interval<=Duration::from_secs(120)) else {safety.block(SafetyState::ProtocolUnsupported);break};
                                    hello_at=None;ready_at=Some(Instant::now()+policy.ready);heartbeat_interval=interval;heartbeat_at=Some(Instant::now()+interval.mul_f64(rand::random::<f64>()));
                                    resume_handshake=Some(session_id.is_some());
                                    drain_disconnected_clears(&mut commands,&mut status,&mut activity,&mut presence_requested,&game_policy);
                                    if session_id.is_none() {
                                        *subscription_wire.lock().unwrap_or_else(|error|error.into_inner())=SubscriptionState::default();
                                    }
                                    if !authentication_control(&controls,token.as_str(),session_id.as_deref(),sequence,&status,&safety) {break;}
                                },
                                11=>{heartbeat_ack=true;acknowledged=true;},
                                1=>{if !control(&controls,json!({"op":1,"d":sequence}),&safety) {break;}},
                                7=>break,
                                9=>{if data.as_bool()!=Some(true) {session_id=None;sequence=None;url=initial_url.clone();}break;},
                                0=>{
                                    let kind=s(&root,"t");
                                    if matches!(kind.as_str(),"READY"|"RESUMED") && (hello_at.is_some() || handshake_complete || resume_handshake!=Some(kind=="RESUMED") || kind=="RESUMED" && session_id.is_none()) {
                                        safety.block(SafetyState::SynchronizationRequired);break;
                                    }
                                    if kind=="READY" {
                                        if hello_at.is_some() {safety.block(SafetyState::SynchronizationRequired);break;}
                                        let sid=s(data,"session_id");session_id=(!sid.is_empty() && sid.len()<=256).then_some(sid);
                                        let chosen=s(&data["user_settings"],"status");if settings_proto::valid_presence(&chosen) {status=chosen;}
                                        if let Ok(values)=settings_proto::read(&s(data,"user_settings_proto"),true) {if let Some(chosen)=values.status {status=chosen;}show_current_game=values.show_current_game;}
                                        if let Some(next)=resume_url(&s(data,"resume_gateway_url")) {url=next;}
                                        if !delivery.push(GatewayEvent::Dispatch{kind:"READY_CATALOG".into(),data:crate::catalog::ready_catalog(data)},&safety) {break;}
                                    }
                                    if matches!(kind.as_str(),"READY"|"RESUMED") {
                                        drain_disconnected_clears(&mut commands,&mut status,&mut activity,&mut presence_requested,&game_policy);
                                        handshake_complete=true;
                                        ready_at=None;ready_since=Some(Instant::now());ready.store(true,Ordering::Release);
                                        if !delivery.push(GatewayEvent::Status("En línea".into()),&safety) {break;}
                                        if let Some(command)=subscription.clone() {if !user(&users,Envelope{command,done:None,game_epoch:game_policy.current().0},&safety){break;}}
                                        if presence_requested && settings_proto::valid_presence(&status) {
                                            if !user(&users,activity.envelope(&status,show_current_game,&game_policy),&safety){break;}
                                        }
                                    }
                                    if kind=="USER_SETTINGS_UPDATE" {
                                        let chosen=s(data,"status");if settings_proto::valid_presence(&chosen){status=chosen;}
                                        if presence_requested && matches!(&activity,ActivitySelection::Rich(..)) && settings_proto::valid_presence(&status) {
                                            if !user(&users,activity.envelope(&status,show_current_game,&game_policy),&safety){break;}
                                        }
                                        if !delivery.push(GatewayEvent::Dispatch{kind:"SELF_PRESENCE".into(),data:json!({"status":status})},&safety){break;}
                                    }
                                    if kind=="USER_SETTINGS_PROTO_UPDATE" && i(&data["settings"],"type")==1 {
                                        if let Ok(values)=settings_proto::read(&s(&data["settings"],"proto"),!b(data,"partial")) {
                                            if let Some(chosen)=values.status {status=chosen;}if values.show_current_game.is_some(){show_current_game=values.show_current_game;}
                                            if activity.has_activity() && settings_proto::valid_presence(&status) {if !user(&users,activity.envelope(&status,show_current_game,&game_policy),&safety){break;}}
                                            if !delivery.push(GatewayEvent::Dispatch{kind:"SELF_PRESENCE".into(),data:json!({"status":status,"show_current_game":show_current_game})},&safety){break;}
                                        }
                                    }
                                    if matches!(kind.as_str(),"READY"|"READY_SUPPLEMENTAL") && !delivery.push(GatewayEvent::Dispatch{kind:"READY_METADATA".into(),data:ready_metadata(data)},&safety){break;}
                                    if retained_dispatch(&kind) && !delivery.push(GatewayEvent::Dispatch{kind,data:data.clone()},&safety){break;}
                                },_=>{},
                            }
                        },
                        envelope=commands.recv()=>{
                            prefer_command=false;
                            let Some(mut envelope)=envelope else {cancel.cancel();break};
                            if !ready.load(Ordering::Acquire) {disconnected_command(envelope,&mut status,&mut activity,&mut presence_requested,&game_policy);continue;}
                            if let Err(error)=game_policy.check(&envelope.command,envelope.game_epoch) {finish(envelope,Err(error));continue;}
                            match &envelope.command {
                                GatewayCommand::Presence{status:chosen,activity:selected}=>{status=chosen.clone();activity=ActivitySelection::Legacy(selected.clone());presence_requested=true;},
                                GatewayCommand::RichPresence{status:chosen,activity:selected}=>{status=chosen.clone();activity=ActivitySelection::Rich(selected.clone(),envelope.game_epoch);presence_requested=true;},
                                GatewayCommand::SubscribeGuild{..}=>{subscription=Some(envelope.command.clone());},
                                _=>{},
                            }
                            if matches!(&envelope.command,GatewayCommand::RichPresence{..}) {
                                envelope.command=activity.command(&status,show_current_game,&game_policy);
                            }
                            if !user(&users,envelope,&safety) {break;}
                        },
                    }
                }
                stable = acknowledged
                    && ready_since.is_some_and(|at: Instant| at.elapsed() >= policy.stable);
                writer_cancel.cancel();
                // The sink is independently owned, so cancellation closes it
                // even when the reader is idle or the UI stopped consuming.
                if !writer_task.is_finished() {
                    let _ = tokio::time::timeout(
                        policy.write + Duration::from_secs(1),
                        &mut writer_task,
                    )
                    .await;
                }
                writer_task.abort();
            }
            Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response)))
                if response.status().as_u16() == 401 =>
            {
                safety.block(SafetyState::AuthenticationRequired);
            }
            _ => {}
        }
        ready.store(false, Ordering::Release);
        while let Ok(envelope) = commands.try_recv() {
            if safety.state() == SafetyState::Allowed && !cancel.is_cancelled() {
                disconnected_command(envelope,&mut status,&mut activity,&mut presence_requested,&game_policy);
            } else {finish(envelope,Err(safety.state().error().unwrap_or_else(ServiceError::network)));}
        }
        if cancel.is_cancelled() || safety.state() != SafetyState::Allowed {
            break;
        }
        failures = if stable {
            1
        } else {
            failures.saturating_add(1)
        };
        if failures >= policy.attempts {
            safety.block(SafetyState::SynchronizationRequired);
            break;
        }
        if !delivery.push(
            GatewayEvent::Status(
                if cooldown.is_zero() {
                    "Reconectando"
                } else {
                    "Esperando límite del Gateway"
                }
                .into(),
            ),
            &safety,
        ) {
            break;
        }
        let factor = 2u32.pow(failures.min(5)) as f64;
        let mut until = Instant::now()
            + policy.backoff.mul_f64(factor)
            + policy.backoff.mul_f64(rand::random::<f64>());
        until = until.max(Instant::now() + cooldown);
        if session_id.is_none()
            && let Some(at) = identify_window.until(policy.identify_gap)
        {
            until = until.max(at);
        }
        while Instant::now() < until {
            let output = delivery.output.clone();
            let deadline = delivery.deadline();
            tokio::select! {
                biased;
                _=cancel.cancelled()=>break 'connections,
                _=safety.wait_blocked()=>break 'connections,
                _=sleep_until(deadline.unwrap_or(until)),if deadline.is_some()=>{safety.block(SafetyState::SynchronizationRequired);break 'connections;},
                permit=output.reserve(),if !delivery.pending.is_empty()=>match permit {Ok(permit)=>delivery.send(permit),Err(_)=>break 'connections},
                _=game_policy.changed.notified()=>{synchronize_game_policy(&mut activity,&mut presence_requested,&game_policy);},
                envelope=commands.recv()=>match envelope {Some(envelope)=>disconnected_command(envelope,&mut status,&mut activity,&mut presence_requested,&game_policy),None=>break 'connections},
                _=sleep_until(until)=>break,
            }
        }
    }
    ready.store(false, Ordering::Release);
    if safety.state() != SafetyState::Allowed {
        let _ = events.try_send(GatewayEvent::Safety(safety.state()));
    }
}

#[cfg(test)]
#[path = "gateway_runtime_tests.rs"]
mod tests;
#[cfg(test)]
mod authentication_tests {
    use super::*;
    #[test]
    fn borrowed_authentication_preserves_wire_fields_and_redacts_debug() {
        let token="synthetic-credential-with-quote\"-and-backslash\\";
        let identify=authentication_text(token,None,None,"online").unwrap();
        assert_eq!(format!("{identify:?}"),"AuthenticationText([REDACTED])");
        let value:Value=serde_json::from_str(&identify.0).unwrap();
        assert_eq!(value["op"],2);
        assert_eq!(value["d"]["token"],token);
        assert_eq!(value["d"]["capabilities"],0);
        assert_eq!(value["d"]["compress"],false);
        assert_eq!(value["d"]["properties"]["browser"],"NotDiscordTerminal");
        assert_eq!(value["d"]["presence"]["status"],"online");
        let resume=authentication_text(token,Some("synthetic-session"),Some(42),"").unwrap();
        let value:Value=serde_json::from_str(&resume.0).unwrap();
        assert_eq!(value,json!({"op":6,"d":{"token":token,"session_id":"synthetic-session","seq":42}}));
        let absent=authentication_text(token,None,None,"").unwrap();
        let value:Value=serde_json::from_str(&absent.0).unwrap();
        assert!(value["d"].get("presence").is_none());
    }
}
