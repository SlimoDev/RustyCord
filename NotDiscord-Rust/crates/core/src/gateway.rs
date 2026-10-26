use crate::{
    api::ServiceError,
    parsing::{b, i, items, limited, s},
    safety::{SafetyGate, SafetyState},
    settings_proto,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, Notify},
    time::{Instant, sleep_until},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[path = "gateway_runtime.rs"]
mod runtime;

const EVENT_DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub enum GatewayEvent {
    Dispatch { kind: String, data: Value },
    Status(String),
    Safety(SafetyState),
}
#[derive(Clone, Debug)]
pub enum GatewayCommand {
    #[cfg(test)]
    Raw(Value),
    StreamCreate {
        guild_id: Option<String>,
        channel_id: String,
    },
    StreamDelete {
        stream_key: String,
    },
    StreamWatch {
        stream_key: String,
    },
    StreamSetPaused {
        stream_key: String,
        paused: bool,
    },
    VoiceState {
        guild_id: Option<String>,
        channel_id: Option<String>,
        mute: bool,
        deaf: bool,
        video: bool,
    },
    SubscribeGuild {
        guild_id: String,
        channel_id: String,
        range_end: i32,
    },
    Presence {
        status: String,
        activity: Option<String>,
    },
    RichPresence {
        status: String,
        activity: Option<crate::game_presence::GamePresence>,
    },
}
struct Envelope {
    command: GatewayCommand,
    done: Option<oneshot::Sender<Result<(), ServiceError>>>,
    game_epoch: u64,
}
struct GamePresencePolicy {
    state: Mutex<(u64, bool)>,
    changed: Notify,
}
impl Default for GamePresencePolicy {
    fn default() -> Self { Self { state: Mutex::new((0, true)), changed: Notify::new() } }
}
impl GamePresencePolicy {
    fn current(&self) -> (u64, bool) { *self.state.lock().unwrap_or_else(|error| error.into_inner()) }
    fn withdraw(&self) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.0 = state.0.wrapping_add(1);state.1 = false;
        let epoch = state.0;drop(state);self.changed.notify_one();epoch
    }
    fn allow(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.1 { state.0 = state.0.wrapping_add(1);state.1 = true;drop(state);self.changed.notify_one(); }
    }
    fn check(&self, command: &GatewayCommand, epoch: u64) -> Result<(), ServiceError> {
        if let GatewayCommand::RichPresence { activity, .. } = command {
            let (current, allowed) = self.current();
            if epoch != current || activity.is_some() && !allowed {
                return Err(ServiceError::new("La actividad fue retirada por una preferencia de privacidad más reciente."));
            }
        }
        Ok(())
    }
}
struct Inner {
    commands: mpsc::Sender<Envelope>,
    cancel: CancellationToken,
    ready: Arc<AtomicBool>,
    safety: SafetyGate,
    game_presence: Arc<GamePresencePolicy>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.cancel.cancel()
    }
}
#[derive(Clone)]
pub struct GatewayHandle {
    inner: Arc<Inner>,
}
impl GatewayHandle {
    /// Withdraw desired game activity even during reconnection. This only
    /// acknowledges local queue admission; it never claims a remote wire ACK.
    /// No nonempty activity or arbitrary Gateway operation is admitted offline.
    pub fn enqueue_clear_game_presence(&self, status: &str) -> Result<(), ServiceError> {
        self.inner.safety.check()?;
        let command = GatewayCommand::RichPresence { status: status.into(), activity: None };
        command_payload(&command, "")?;
        let game_epoch = self.inner.game_presence.withdraw();
        self.inner.commands.try_send(Envelope { command, done: None, game_epoch }).map_err(|_| {
            ServiceError::typed(crate::api::ErrorKind::QueueFull, "La cola del Gateway está llena. Espera antes de volver a intentarlo.")
        })
    }
    /// Explicitly permit a new game publication after local preferences and
    /// current presence allow it. Repeated calls are idempotent; old queued
    /// publications retain their previous epoch and cannot be resurrected.
    pub fn allow_game_presence(&self) -> Result<(), ServiceError> {
        self.inner.safety.check()?;self.inner.game_presence.allow();Ok(())
    }
    pub fn game_presence_epoch(&self) -> u64 { self.inner.game_presence.current().0 }
    /// Keep an activity captured before an unrelated async operation bound to
    /// the same privacy generation, even when preferences are toggled twice.
    pub async fn send_rich_presence_for_epoch(&self, status: String, activity: Option<crate::game_presence::GamePresence>, epoch: u64) -> Result<(), ServiceError> {
        self.send_at_game_epoch(GatewayCommand::RichPresence { status, activity }, Some(epoch)).await
    }
    /// Explicit offline-only transport. Captures validated outgoing payloads
    /// and acknowledges them locally; no credentials, DNS, socket or reader.
    #[cfg(feature = "offline-probes")]
    pub fn offline_recorder(safety: SafetyGate) -> (Self, mpsc::Receiver<Value>, tokio::task::JoinHandle<()>) {
        let (commands, mut incoming) = mpsc::channel::<Envelope>(32);
        let (record, values) = mpsc::channel(32);
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let gate = safety.clone();
        let game_presence = Arc::new(GamePresencePolicy::default());
        let policy = game_presence.clone();
        let task = tokio::spawn(async move {
            loop {
                let envelope = tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    value = incoming.recv() => match value {Some(value)=>value,None=>break},
                };
                let result = gate.check().and_then(|_|policy.check(&envelope.command,envelope.game_epoch)).and_then(|_|command_payload(&envelope.command,"offline-session"));
                let result = match result {
                    Ok(value) => tokio::select! {
                        biased;
                        _=stop.cancelled()=>Err(ServiceError::network()),
                        _=gate.wait_blocked()=>gate.check(),
                        sent=record.send(value)=>sent.map_err(|_|ServiceError::network()),
                    },
                    Err(error)=>Err(error),
                };
                if let Some(done)=envelope.done { let _=done.send(result); }
            }
        });
        (Self {inner:Arc::new(Inner {commands,cancel,ready:Arc::new(AtomicBool::new(true)),safety,game_presence})},values,task)
    }
    /// Admit a desired-state update without waiting for its wire flush. Only
    /// presence and subscriptions are coalesced; media mutations retain order.
    pub fn enqueue(&self, command: GatewayCommand) -> Result<(), ServiceError> {
        self.inner.safety.check()?;
        if !self.is_ready() {
            return Err(ServiceError::new("La conexión aún no está preparada."));
        }
        command_payload(&command, "")?;
        let game_epoch = self.inner.game_presence.current().0;
        self.inner.game_presence.check(&command,game_epoch)?;
        self.inner
            .commands
            .try_send(Envelope {
                command,
                done: None,
                game_epoch,
            })
            .map_err(|_| {
                ServiceError::typed(
                    crate::api::ErrorKind::QueueFull,
                    "La cola del Gateway está llena. Espera antes de volver a intentarlo.",
                )
            })
    }
    pub async fn send(&self, command: GatewayCommand) -> Result<(), ServiceError> {
        self.send_at_game_epoch(command,None).await
    }
    async fn send_at_game_epoch(&self, command: GatewayCommand, expected_epoch: Option<u64>) -> Result<(), ServiceError> {
        self.inner.safety.check()?;
        if !self.inner.ready.load(Ordering::Acquire) {
            return Err(ServiceError::new("La conexión aún no está preparada."));
        }
        let (tx, rx) = oneshot::channel();
        command_payload(&command, "")?;
        let game_epoch = expected_epoch.unwrap_or_else(||self.inner.game_presence.current().0);
        self.inner.game_presence.check(&command,game_epoch)?;
        let wait = async {
            tokio::select! {biased;_=self.inner.cancel.cancelled()=>return Err(ServiceError::network()),_=self.inner.safety.wait_blocked()=>return self.inner.safety.check(),r=self.inner.commands.send(Envelope{command,done:Some(tx),game_epoch})=>r.map_err(|_|ServiceError::network())?}
            tokio::select! {biased;_=self.inner.cancel.cancelled()=>Err(ServiceError::network()),_=self.inner.safety.wait_blocked()=>self.inner.safety.check(),r=rx=>r.unwrap_or_else(|_|Err(ServiceError::network()))}
        };
        tokio::time::timeout(Duration::from_secs(10),wait).await.unwrap_or_else(|_|Err(ServiceError::typed(crate::api::ErrorKind::OutcomeUnknown,"No se pudo confirmar el envío de la orden a Discord. Comprueba el estado antes de repetirla.")))
    }
    pub fn stop(&self) {
        self.inner.cancel.cancel()
    }
    pub fn is_ready(&self) -> bool {
        self.inner.ready.load(Ordering::Acquire)
            && self.inner.safety.state() == SafetyState::Allowed
    }
}
pub fn spawn(
    credential: String,
    initial_status: Option<String>,
) -> (GatewayHandle, mpsc::Receiver<GatewayEvent>) {
    spawn_with_safety(credential, initial_status, SafetyGate::default())
}
pub fn spawn_with_safety(
    credential: String,
    initial_status: Option<String>,
    safety: SafetyGate,
) -> (GatewayHandle, mpsc::Receiver<GatewayEvent>) {
    spawn_at(
        credential,
        initial_status,
        safety,
        "wss://gateway.discord.gg/?v=9&encoding=json".into(),
    )
}
// Production callers can only select the fixed Discord origin above. The private
// URL argument lets unit tests exercise a loopback WebSocket without an account.
fn spawn_at(
    credential: String,
    initial_status: Option<String>,
    safety: SafetyGate,
    url: String,
) -> (GatewayHandle, mpsc::Receiver<GatewayEvent>) {
    let (commands, rx) = mpsc::channel(32);
    let (events, event_rx) = mpsc::channel(256);
    let cancel = CancellationToken::new();
    let ready = Arc::new(AtomicBool::new(false));
    let game_presence = Arc::new(GamePresencePolicy::default());
    let handle = GatewayHandle {
        inner: Arc::new(Inner {
            commands,
            cancel: cancel.clone(),
            ready: ready.clone(),
            safety: safety.clone(),
            game_presence: game_presence.clone(),
        }),
    };
    tokio::spawn(run_with_game_policy(
        Zeroizing::new(credential),
        initial_status,
        rx,
        events,
        cancel,
        ready,
        safety,
        url,
        game_presence,
    ));
    (handle, event_rx)
}
#[cfg(test)]
async fn emit(
    events: &mpsc::Sender<GatewayEvent>,
    event: GatewayEvent,
    cancel: &CancellationToken,
    safety: &SafetyGate,
) -> bool {
    tokio::select! {
        biased;
        _=cancel.cancelled()=>false,
        _=safety.wait_blocked()=>false,
        result=tokio::time::timeout(EVENT_DELIVERY_TIMEOUT, events.send(event))=>match result {
            Ok(result)=>result.is_ok(),
            Err(_)=>{
                // An unconsumed queue also prevents reading a terminal close.
                // Stop the whole session rather than keep REST operating with
                // unknown Gateway state or silently drop ordered events.
                safety.block(SafetyState::SynchronizationRequired);
                false
            }
        }
    }
}
#[cfg(test)]
async fn run(
    token: Zeroizing<String>,
    initial_status: Option<String>,
    commands: mpsc::Receiver<Envelope>,
    events: mpsc::Sender<GatewayEvent>,
    cancel: CancellationToken,
    ready: Arc<AtomicBool>,
    safety: SafetyGate,
    url: String,
) {
    run_with_game_policy(token,initial_status,commands,events,cancel,ready,safety,url,Arc::new(GamePresencePolicy::default())).await;
}
async fn run_with_game_policy(
    token: Zeroizing<String>, initial_status: Option<String>, commands: mpsc::Receiver<Envelope>,
    events: mpsc::Sender<GatewayEvent>, cancel: CancellationToken, ready: Arc<AtomicBool>,
    safety: SafetyGate, url: String, game_presence: Arc<GamePresencePolicy>,
) {
    runtime::run(
        token,
        initial_status,
        commands,
        events,
        cancel,
        ready,
        safety,
        url,
        runtime::Policy::default(),
        game_presence,
    )
    .await;
}
fn terminal_safety(code: u16) -> Option<SafetyState> {
    match code {
        4004 => Some(SafetyState::AuthenticationRequired),
        4010..=4014 => Some(SafetyState::ProtocolUnsupported),
        _ => None,
    }
}
fn presence_payload(status: &str, activity: Option<&str>) -> Value {
    json!({"op":3,"d":{"since":0,"status":status,"afk":false,"activities":activity.map(|name|vec![json!({"name":name,"type":0})]).unwrap_or_default()}})
}
pub fn command_payload(
    command: &GatewayCommand,
    previous_guild: &str,
) -> Result<Value, ServiceError> {
    Ok(match command {
        #[cfg(test)]
        GatewayCommand::Raw(value) => value.clone(),
        GatewayCommand::StreamCreate {
            guild_id,
            channel_id,
        } => {
            if guild_id
                .as_deref()
                .is_some_and(|id| !crate::parsing::valid_id(id))
                || !crate::parsing::valid_id(channel_id)
            {
                return Err(ServiceError::new("Origen de pantalla no válido."));
            }
            json!({"op":18,"d":{"type":if guild_id.is_some(){"guild"}else{"call"},"guild_id":guild_id,"channel_id":channel_id,"preferred_region":null}})
        }
        GatewayCommand::StreamDelete { stream_key }
        | GatewayCommand::StreamWatch { stream_key }
        | GatewayCommand::StreamSetPaused { stream_key, .. } => {
            if !valid_stream_key(stream_key) {
                return Err(ServiceError::new("Identificador de pantalla no válido."));
            }
            match command {
                GatewayCommand::StreamDelete { .. } => {
                    json!({"op":19,"d":{"stream_key":stream_key}})
                }
                GatewayCommand::StreamWatch { .. } => {
                    json!({"op":20,"d":{"stream_key":stream_key}})
                }
                GatewayCommand::StreamSetPaused { paused, .. } => {
                    json!({"op":22,"d":{"stream_key":stream_key,"paused":paused}})
                }
                _ => unreachable!(),
            }
        }
        GatewayCommand::VoiceState {
            guild_id,
            channel_id,
            mute,
            deaf,
            video,
        } => {
            if guild_id
                .as_deref()
                .is_some_and(|v| !crate::parsing::valid_id(v))
                || channel_id
                    .as_deref()
                    .is_some_and(|v| !crate::parsing::valid_id(v))
            {
                return Err(ServiceError::new("Canal de voz inválido."));
            }
            json!({"op":4,"d":{"guild_id":guild_id,"channel_id":channel_id,"self_mute":mute,"self_deaf":deaf,"self_video":video}})
        }
        GatewayCommand::Presence { status, activity } => {
            if !settings_proto::valid_presence(status)
                || activity
                    .as_ref()
                    .is_some_and(|value| value.chars().count() > 128)
            {
                return Err(ServiceError::new("El estado de presencia no es válido."));
            }
            presence_payload(status, activity.as_deref())
        }
        GatewayCommand::RichPresence { status, activity } => {
            if !settings_proto::valid_presence(status) && status != "offline"
                || activity.is_some() && matches!(status.as_str(), "invisible" | "offline")
            {
                return Err(ServiceError::new("El estado no permite publicar una actividad de juego."));
            }
            let activities = activity.as_ref().map(|activity| activity.payload()).transpose()
                .map_err(ServiceError::new)?.into_iter().collect::<Vec<_>>();
            json!({"op":3,"d":{"since":0,"status":status,"afk":false,"activities":activities}})
        }
        GatewayCommand::SubscribeGuild {
            guild_id,
            channel_id,
            range_end,
        } => {
            if !guild_id.is_empty() && !crate::parsing::valid_id(guild_id)
                || !channel_id.is_empty() && !crate::parsing::valid_id(channel_id)
            {
                return Err(ServiceError::new("Suscripción no válida."));
            }
            let mut subscriptions = serde_json::Map::new();
            if !previous_guild.is_empty() && previous_guild != guild_id {
                subscriptions.insert(
                    previous_guild.into(),
                    json!({"typing":false,"activities":false,"threads":false,"channels":{}}),
                );
            }
            if !guild_id.is_empty() {
                let mut channels = serde_json::Map::new();
                if !channel_id.is_empty() {
                    channels.insert(
                        channel_id.clone(),
                        json!([[0, range_end.clamp(&99, &1999)]]),
                    );
                }
                subscriptions.insert(
                    guild_id.clone(),
                    json!({"typing":true,"activities":true,"threads":false,"channels":channels}),
                );
            }
            json!({"op":37,"d":{"subscriptions":subscriptions}})
        }
    })
}
fn valid_stream_key(value: &str) -> bool {
    let parts: Vec<_> = value.split(':').collect();
    matches!(parts.as_slice(), ["call", _, _] | ["guild", _, _, _])
        && parts[1..].iter().all(|id| crate::parsing::valid_id(id))
}
pub fn resume_url(value: &str) -> Option<String> {
    let u = url::Url::parse(value).ok()?;
    let host = u.host_str()?;
    if u.scheme() != "wss"
        || !(host == "discord.gg" || host.ends_with(".discord.gg"))
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port_or_known_default() != Some(443)
    {
        return None;
    }
    let mut u = u;
    u.set_query(Some("v=9&encoding=json"));
    u.set_fragment(None);
    Some(u.to_string())
}
fn retained_dispatch(kind: &str) -> bool {
    matches!(
        kind,
        "MESSAGE_CREATE"
            | "MESSAGE_UPDATE"
            | "MESSAGE_DELETE"
            | "MESSAGE_REACTION_ADD"
            | "MESSAGE_REACTION_REMOVE"
            | "MESSAGE_REACTION_REMOVE_ALL"
            | "MESSAGE_REACTION_REMOVE_EMOJI"
            | "RELATIONSHIP_ADD"
            | "RELATIONSHIP_UPDATE"
            | "RELATIONSHIP_REMOVE"
            | "CHANNEL_CREATE"
            | "CHANNEL_UPDATE"
            | "CHANNEL_DELETE"
            | "THREAD_CREATE"
            | "THREAD_UPDATE"
            | "THREAD_DELETE"
            | "THREAD_LIST_SYNC"
            | "THREAD_MEMBER_UPDATE"
            | "THREAD_MEMBERS_UPDATE"
            | "GUILD_CREATE"
            | "GUILD_UPDATE"
            | "GUILD_DELETE"
            | "CHANNEL_RECIPIENT_ADD"
            | "CHANNEL_RECIPIENT_REMOVE"
            | "CHANNEL_PINS_UPDATE"
            | "VOICE_STATE_UPDATE"
            | "VOICE_SERVER_UPDATE"
            | "STREAM_CREATE"
            | "STREAM_UPDATE"
            | "STREAM_SERVER_UPDATE"
            | "STREAM_DELETE"
            | "PRESENCE_UPDATE"
            | "TYPING_START"
            | "GUILD_MEMBER_LIST_UPDATE"
            | "GUILD_MEMBER_UPDATE"
            | "GUILD_MEMBER_REMOVE"
            | "GUILD_ROLE_CREATE"
            | "GUILD_ROLE_UPDATE"
            | "GUILD_ROLE_DELETE"
            | "USER_GUILD_SETTINGS_UPDATE"
            | "MESSAGE_ACK"
            | "USER_UPDATE"
            | "CALL_CREATE"
            | "CALL_UPDATE"
            | "CALL_DELETE"
    )
}
pub fn ready_metadata(data: &Value) -> Value {
    let presences=items(&data["presences"]).iter().chain(items(&data["merged_presences"]["friends"]).iter()).chain(items(&data["merged_presences"]["guilds"]).iter().flat_map(|v|items(v).iter())).take(2000).map(|p|json!({"user":{"id":if p.get("user").is_some(){s(&p["user"],"id")}else{s(p,"user_id")}},"status":s(p,"status"),"activities":items(&p["activities"]).iter().take(8).map(|a|json!({"name":limited(&s(a,"name"),160),"state":limited(&s(a,"state"),300),"details":limited(&s(a,"details"),300),"application_id":limited(&s(a,"application_id"),24),"type":i(a,"type"),"emoji":a.get("emoji").map(|e|json!({"name":limited(&s(e,"name"),80),"id":limited(&s(e,"id"),24),"animated":b(e,"animated")})),"assets":{"large_image":limited(&s(&a["assets"],"large_image"),200)},"timestamps":{"start":a["timestamps"]["start"].as_i64(),"end":a["timestamps"]["end"].as_i64()}})).collect::<Vec<_>>()})).collect::<Vec<_>>();
    let source = data["user_guild_settings"]
        .get("entries")
        .unwrap_or(&data["user_guild_settings"]);
    let mut budget = 2000;
    let settings = items(source)
        .iter()
        .take(500)
        .map(|v| {
            let entries = items(&v["channel_overrides"])
                .iter()
                .take(budget)
                .map(|c| json!({"channel_id":s(c,"channel_id"),"muted":c["muted"].as_bool(),"message_notifications":c["message_notifications"].as_u64().filter(|mode|*mode<=3)}))
                .collect::<Vec<_>>();
            budget = budget.saturating_sub(entries.len());
            json!({"guild_id":s(v,"guild_id"),"muted":v["muted"].as_bool(),"message_notifications":v["message_notifications"].as_u64().filter(|mode|*mode<=2),"suppress_everyone":v["suppress_everyone"].as_bool(),"suppress_roles":v["suppress_roles"].as_bool(),"channel_overrides":entries})
        })
        .collect::<Vec<_>>();
    let source = data["read_state"]
        .get("entries")
        .unwrap_or(&data["read_state"]);
    let reads=items(source).iter().take(2000).map(|v|json!({"channel_id":s(v,"id"),"last_message_id":s(v,"last_message_id"),"mention_count":i(v,"mention_count").clamp(0,9999)})).collect::<Vec<_>>();
    let status = s(&data["user_settings"], "status");
    // Retain only a bounded participant seed; never transport/session keys or
    // whole guild/member payloads. The budget is global across all guilds.
    let mut voice_budget = 1024usize;
    let known_users:std::collections::HashMap<_,_>=items(&data["users"]).iter().take(3000).chain(data.get("user")).filter_map(|user|user["id"].as_str().map(|id|(id,user))).collect();
    let guild_voice_states = items(&data["guilds"]).iter().take(500).filter_map(|guild| {
        let guild_id = s(guild,"id");
        let source = guild["voice_states"].as_array()?;
        if !crate::parsing::valid_id(&guild_id) { return None; }
        let members:std::collections::HashMap<_,_>=items(&guild["members"]).iter().take(3000).filter_map(|member|member["user"]["id"].as_str().or_else(||member["user_id"].as_str()).map(|id|(id,member))).collect();
        let complete = source.len() <= voice_budget;
        let values = source.iter().take(voice_budget).filter_map(|value| {
            let user = s(value,"user_id"); let channel = s(value,"channel_id");
            (crate::parsing::valid_id(&user) && crate::parsing::valid_id(&channel)).then(|| {
                let mut compact=json!({
                "guild_id":guild_id,"user_id":user,"channel_id":channel,
                "self_mute":b(value,"self_mute"),"self_deaf":b(value,"self_deaf"),
                "mute":b(value,"mute"),"deaf":b(value,"deaf"),
                "self_video":b(value,"self_video"),"self_stream":b(value,"self_stream")
                });
                let member=if value["member"].is_object(){Some(&value["member"])}else{members.get(user.as_str()).copied()};
                let identity=member.and_then(|member|member.get("user")).filter(|identity|s(identity,"id")==user)
                    .or_else(||value.get("user").filter(|identity|s(identity,"id")==user)).or_else(||known_users.get(user.as_str()).copied());
                let mut projected=serde_json::Map::new();
                if let Some(nick)=member.and_then(|member|member.get("nick")){projected.insert("nick".into(),if nick.is_null(){Value::Null}else{json!(limited(nick.as_str().unwrap_or(""),32))});}
                if let Some(identity)=identity {
                    let mut fields=serde_json::Map::new();fields.insert("id".into(),json!(user));
                    for field in ["username","global_name","avatar"]{if let Some(value)=identity.get(field){fields.insert(field.into(),if value.is_null(){Value::Null}else{json!(limited(value.as_str().unwrap_or(""),128))});}}
                    if let Some(bot)=identity.get("bot").and_then(Value::as_bool){fields.insert("bot".into(),json!(bot));}
                    projected.insert("user".into(),Value::Object(fields));
                }
                if !projected.is_empty(){compact["member"]=Value::Object(projected);}
                compact
            })
        }).collect::<Vec<_>>();
        voice_budget = voice_budget.saturating_sub(source.len());
        Some(json!({"guild_id":guild_id,"partial":!complete,"states":values}))
    }).collect::<Vec<_>>();
    json!({"presences":presences,"settings":settings,"read_states":reads,"guild_voice_states":guild_voice_states,"self_status":if settings_proto::valid_presence(&status){status}else{String::new()}})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thread_catalog_and_membership_events_reach_the_controller_without_expanding_unrelated_events() {
        for kind in ["THREAD_CREATE","THREAD_UPDATE","THREAD_DELETE","THREAD_LIST_SYNC","THREAD_MEMBER_UPDATE","THREAD_MEMBERS_UPDATE"] {
            assert!(retained_dispatch(kind), "{kind}");
        }
        assert!(!retained_dispatch("UNRECOGNIZED_EVENT"));
    }
    #[test]
    fn ready_notification_projection_preserves_modes_suppression_and_missing_fields() {
        let metadata=ready_metadata(&json!({"user_guild_settings":{"entries":[{"guild_id":"50","message_notifications":2,
            "suppress_everyone":true,"suppress_roles":false,"channel_overrides":[{"channel_id":"20","message_notifications":3,"muted":true}]},
            {"guild_id":"51","suppress_roles":true}]}}));
        assert_eq!(metadata["settings"][0]["message_notifications"],2);
        assert_eq!(metadata["settings"][0]["suppress_everyone"],true);
        assert_eq!(metadata["settings"][0]["suppress_roles"],false);
        assert_eq!(metadata["settings"][0]["channel_overrides"][0]["message_notifications"],3);
        assert!(metadata["settings"][1]["muted"].is_null(),"Partial updates must not synthesize an unmute");
        assert!(metadata["settings"][1]["message_notifications"].is_null());
    }
    #[cfg(feature = "offline-probes")]
    #[tokio::test]
    async fn offline_recorder_validates_safety_and_shuts_down_without_socket_or_credentials() {
        let gate=SafetyGate::default();
        let (handle,mut values,task)=GatewayHandle::offline_recorder(gate.clone());
        handle.send(GatewayCommand::VoiceState {guild_id:Some("500".into()),channel_id:Some("200".into()),mute:true,deaf:false,video:false}).await.unwrap();
        let payload=values.recv().await.unwrap();
        assert_eq!(payload["op"],4);assert_eq!(payload["d"]["channel_id"],"200");
        assert!(!payload.to_string().contains("token"));
        gate.block(SafetyState::VerificationRequired);
        assert!(handle.send(GatewayCommand::StreamWatch {stream_key:"call:200:300".into()}).await.is_err());
        assert!(values.try_recv().is_err());
        handle.stop();tokio::time::timeout(Duration::from_secs(1),task).await.unwrap().unwrap();
        assert!(values.recv().await.is_none());
    }
    #[test]
    fn terminal_closes_classify_authentication_and_protocol_without_guessing_a_ban() {
        assert_eq!(
            terminal_safety(4004),
            Some(SafetyState::AuthenticationRequired)
        );
        for code in 4010..=4014 {
            assert_eq!(
                terminal_safety(code),
                Some(SafetyState::ProtocolUnsupported)
            );
        }
        for code in [1000, 1006, 4007, 4008, 4009] {
            assert_eq!(terminal_safety(code), None);
        }
    }
    #[tokio::test]
    async fn blocked_gateway_rejects_new_and_waiting_commands_without_a_socket() {
        let gate = SafetyGate::default();
        let (commands, mut queue) = mpsc::channel(1);
        let (done, _ignored) = oneshot::channel();
        commands
            .send(Envelope {
                command: GatewayCommand::Raw(json!({"op":3})),
                done: Some(done),
                game_epoch: 0,
            })
            .await
            .unwrap();
        let handle = GatewayHandle {
            inner: Arc::new(Inner {
                commands,
                cancel: CancellationToken::new(),
                ready: Arc::new(AtomicBool::new(true)),
                safety: gate.clone(),
                game_presence: Arc::new(GamePresencePolicy::default()),
            }),
        };
        let pending_handle = handle.clone();
        let pending = tokio::spawn(async move {
            pending_handle
                .send(GatewayCommand::Raw(json!({"op":3})))
                .await
        });
        tokio::task::yield_now().await;
        gate.block(SafetyState::VerificationRequired);
        let result = tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        assert!(!handle.is_ready());
        assert!(
            handle
                .send(GatewayCommand::Raw(json!({"op":3})))
                .await
                .is_err()
        );
        assert!(
            queue.try_recv().is_ok(),
            "The existing unsent envelope is still local"
        );
        assert!(
            queue.try_recv().is_err(),
            "No blocked command entered the queue"
        );
    }
    async fn assert_terminal_loopback(code: u16, expected: SafetyState) {
        use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            socket
                .close(Some(CloseFrame {
                    code: CloseCode::from(code),
                    reason: "Offline rejection fixture".into(),
                }))
                .await
                .unwrap();
        });
        let (_commands, rx) = mpsc::channel(2);
        let (events, mut received) = mpsc::channel(8);
        let safety = SafetyGate::default();
        let ready = Arc::new(AtomicBool::new(false));
        let runner = tokio::spawn(run(
            Zeroizing::new("offline-synthetic-credential-only".into()),
            None,
            rx,
            events,
            CancellationToken::new(),
            ready.clone(),
            safety.clone(),
            format!("ws://{address}/"),
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match received.recv().await {
                    Some(GatewayEvent::Safety(state)) => {
                        assert_eq!(state, expected);
                        break;
                    }
                    Some(_) => {}
                    None => panic!("Missing typed rejection event"),
                }
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(1), runner)
            .await
            .expect("Terminal failures must stop rather than reconnect")
            .unwrap();
        server.await.unwrap();
        assert_eq!(safety.state(), expected);
        assert!(safety.check().is_err(), "The same gate protects the API");
        assert!(!ready.load(Ordering::Acquire));
    }
    #[tokio::test]
    async fn rejected_authentication_blocks_shared_gate_and_does_not_reconnect() {
        assert_terminal_loopback(4004, SafetyState::AuthenticationRequired).await;
    }
    #[tokio::test]
    async fn unsupported_protocol_blocks_shared_gate_and_does_not_reconnect() {
        assert_terminal_loopback(4012, SafetyState::ProtocolUnsupported).await;
    }
    #[tokio::test]
    async fn api_safety_stop_closes_an_existing_socket_without_waiting_for_the_ui() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (connected, connection_ready) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            // Complete a round trip before blocking: server-side handshake
            // completion alone can precede the client's connect future.
            socket.send(Message::Ping(vec![1].into())).await.unwrap();
            let pong = tokio::time::timeout(Duration::from_secs(3), socket.next())
                .await
                .unwrap();
            assert!(matches!(pong, Some(Ok(Message::Pong(_)))));
            connected.send(()).unwrap();
            // No HELLO: this test needs no credential on the wire at all.
            let close = tokio::time::timeout(Duration::from_secs(3), socket.next())
                .await
                .unwrap();
            assert!(matches!(close, Some(Ok(Message::Close(_)))));
        });
        let (_commands, rx) = mpsc::channel(2);
        let (events, _not_consumed) = mpsc::channel(8);
        let safety = SafetyGate::default();
        let ready = Arc::new(AtomicBool::new(false));
        let runner = tokio::spawn(run(
            Zeroizing::new("offline-synthetic-credential-only".into()),
            None,
            rx,
            events,
            CancellationToken::new(),
            ready.clone(),
            safety.clone(),
            format!("ws://{address}/"),
        ));
        tokio::time::timeout(Duration::from_secs(3), connection_ready)
            .await
            .unwrap()
            .unwrap();
        safety.block(SafetyState::VerificationRequired);
        tokio::time::timeout(Duration::from_secs(2), runner)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
        assert!(!ready.load(Ordering::Acquire));
        assert_eq!(safety.state(), SafetyState::VerificationRequired);
    }
    #[tokio::test]
    async fn full_event_queue_cannot_hide_shared_safety_or_block_shutdown() {
        let gate = SafetyGate::default();
        let (events, _not_consumed) = mpsc::channel(1);
        events
            .send(GatewayEvent::Status("Queued fixture".into()))
            .await
            .unwrap();
        let safety = gate.clone();
        let waiting = tokio::spawn(async move {
            emit(
                &events,
                GatewayEvent::Status("Unsent fixture".into()),
                &CancellationToken::new(),
                &safety,
            )
            .await
        });
        tokio::task::yield_now().await;
        gate.block(SafetyState::AuthenticationRequired);
        assert!(
            !tokio::time::timeout(Duration::from_secs(1), waiting)
                .await
                .unwrap()
                .unwrap()
        );
        assert_eq!(gate.state(), SafetyState::AuthenticationRequired);
    }
    #[tokio::test(start_paused = true)]
    async fn stalled_event_delivery_stops_rest_instead_of_losing_ordered_events() {
        let gate = SafetyGate::default();
        let (events, _not_consumed) = mpsc::channel(1);
        events
            .send(GatewayEvent::Status("Queued fixture".into()))
            .await
            .unwrap();
        assert!(
            !emit(
                &events,
                GatewayEvent::Status("Unsent fixture".into()),
                &CancellationToken::new(),
                &gate,
            )
            .await
        );
        assert_eq!(gate.state(), SafetyState::SynchronizationRequired);
        assert!(
            gate.check().is_err(),
            "REST shares this stop without waiting for UI"
        );
    }
    #[tokio::test]
    async fn a_terminal_close_behind_a_full_event_queue_cannot_leave_rest_enabled() {
        use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            // No HELLO or token: only a synthetic event followed by rejection.
            socket
                .send(Message::Text(
                    json!({"op":0,"t":"MESSAGE_CREATE","s":1,"d":{"id":"123"}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            socket
                .close(Some(CloseFrame {
                    code: CloseCode::from(4004),
                    reason: "Offline rejection behind queue".into(),
                }))
                .await
                .unwrap();
        });
        let (_commands, rx) = mpsc::channel(2);
        let (events, _not_consumed) = mpsc::channel(1);
        let safety = SafetyGate::default();
        let ready = Arc::new(AtomicBool::new(false));
        let runner = tokio::spawn(run(
            Zeroizing::new("offline-synthetic-credential-only".into()),
            None,
            rx,
            events,
            CancellationToken::new(),
            ready.clone(),
            safety.clone(),
            format!("ws://{address}/"),
        ));
        tokio::time::timeout(EVENT_DELIVERY_TIMEOUT + Duration::from_secs(3), runner)
            .await
            .expect("The event consumer cannot delay safety indefinitely")
            .unwrap();
        server.await.unwrap();
        // UI delivery now has its own bounded buffer, so the terminal close is
        // still read and classified while the UI queue remains unconsumed.
        assert_eq!(safety.state(), SafetyState::AuthenticationRequired);
        assert!(safety.check().is_err());
        assert!(!ready.load(Ordering::Acquire));
    }
    #[test]
    fn resume_origin_validation() {
        assert!(resume_url("wss://gateway-us-east1.discord.gg").is_some());
        for u in [
            "ws://gateway.discord.gg",
            "wss://discord.gg.evil.com",
            "wss://u@gateway.discord.gg",
            "wss://gateway.discord.gg:1234",
        ] {
            assert!(resume_url(u).is_none())
        }
    }
    #[test]
    fn subscription_unsubscribes_previous() {
        let p = command_payload(
            &GatewayCommand::SubscribeGuild {
                guild_id: "2".into(),
                channel_id: "3".into(),
                range_end: 99999,
            },
            "1",
        )
        .unwrap();
        assert_eq!(p["op"], 37);
        assert_eq!(p["d"]["subscriptions"]["1"]["typing"], false);
        assert_eq!(p["d"]["subscriptions"]["2"]["channels"]["3"][0][1], 1999)
    }
    #[test]
    fn presence_preserves_choice() {
        let p = command_payload(
            &GatewayCommand::Presence {
                status: "invisible".into(),
                activity: Some("Game".into()),
            },
            "",
        )
        .unwrap();
        assert_eq!(p["d"]["status"], "invisible");
        assert_eq!(p["d"]["activities"][0]["name"], "Game")
    }
    #[test]
    fn typed_game_presence_preserves_metadata_and_forbids_hidden_publications() {
        use crate::game_presence::GamePresence;
        let game=GamePresence::from_rpc("424004941485572097","RPCS3",&json!({"details":"Synthetic game","state":"Playing","timestamps":{"start":1_700_000_000u64}})).unwrap();
        let payload=command_payload(&GatewayCommand::RichPresence{status:"online".into(),activity:Some(game.clone())},"").unwrap();
        assert_eq!(payload,json!({"op":3,"d":{"status":"online","since":0,"afk":false,"activities":[{"name":"RPCS3","application_id":"424004941485572097","type":0,"details":"Synthetic game","state":"Playing","timestamps":{"start":1_700_000_000_000u64}}]}}));
        for status in ["invisible","offline"] {
            assert!(command_payload(&GatewayCommand::RichPresence{status:status.into(),activity:Some(game.clone())},"").is_err());
            assert_eq!(command_payload(&GatewayCommand::RichPresence{status:status.into(),activity:None},"").unwrap()["d"]["activities"],json!([]));
        }
        let mut forged=game;forged.assets=Some(crate::game_presence::GamePresenceAssets{large_image:Some("mp:unresolved".into()),..Default::default()});
        assert!(command_payload(&GatewayCommand::RichPresence{status:"online".into(),activity:Some(forged)},"").is_err());
    }
    #[test]
    fn privacy_withdrawal_invalidates_an_already_queued_publication_even_when_clear_queue_is_full() {
        let (commands,mut queue)=mpsc::channel(1);let policy=Arc::new(GamePresencePolicy::default());
        let handle=GatewayHandle{inner:Arc::new(Inner{commands,cancel:CancellationToken::new(),ready:Arc::new(AtomicBool::new(true)),safety:SafetyGate::default(),game_presence:policy.clone()})};
        let game=crate::game_presence::GamePresence::from_name("Old synthetic game").unwrap();
        handle.enqueue(GatewayCommand::RichPresence{status:"online".into(),activity:Some(game.clone())}).unwrap();
        let captured=handle.game_presence_epoch();assert!(handle.enqueue_clear_game_presence("online").is_err());
        assert_ne!(captured,handle.game_presence_epoch());assert_eq!(policy.current().1,false);
        let queued=queue.try_recv().unwrap();assert!(policy.check(&queued.command,queued.game_epoch).is_err());
        assert!(handle.enqueue(GatewayCommand::RichPresence{status:"online".into(),activity:Some(game.clone())}).is_err());
        handle.allow_game_presence().unwrap();assert!(policy.check(&queued.command,queued.game_epoch).is_err());
        handle.enqueue(GatewayCommand::RichPresence{status:"online".into(),activity:Some(game)}).unwrap();
        let fresh=queue.try_recv().unwrap();assert!(policy.check(&fresh.command,fresh.game_epoch).is_ok());
    }
    #[test]
    fn typed_stream_commands_validate_keys_and_keep_compatible_payloads() {
        assert_eq!(
            command_payload(
                &GatewayCommand::StreamCreate {
                    guild_id: None,
                    channel_id: "123".into()
                },
                ""
            )
            .unwrap(),
            json!({"op":18,"d":{"type":"call","guild_id":null,"channel_id":"123","preferred_region":null}})
        );
        assert_eq!(
            command_payload(
                &GatewayCommand::StreamWatch {
                    stream_key: "guild:1:2:3".into()
                },
                ""
            )
            .unwrap()["op"],
            20
        );
        assert_eq!(
            command_payload(
                &GatewayCommand::StreamSetPaused {
                    stream_key: "call:2:3".into(),
                    paused: false
                },
                ""
            )
            .unwrap()["d"]["paused"],
            false
        );
        for key in ["invalid", "call:2", "guild:1:2:3:4", "call:2:evil"] {
            assert!(
                command_payload(
                    &GatewayCommand::StreamDelete {
                        stream_key: key.into()
                    },
                    ""
                )
                .is_err()
            );
        }
    }
    #[tokio::test(start_paused = true)]
    async fn confirmation_wait_is_bounded_and_queued_timeout_can_never_be_replayed() {
        let (commands, mut queue) = mpsc::channel(1);
        let handle = GatewayHandle {
            inner: Arc::new(Inner {
                commands,
                cancel: CancellationToken::new(),
                ready: Arc::new(AtomicBool::new(true)),
                safety: SafetyGate::default(),
                game_presence: Arc::new(GamePresencePolicy::default()),
            }),
        };
        let result = handle
            .send(GatewayCommand::VoiceState {
                guild_id: None,
                channel_id: Some("1".into()),
                mute: true,
                deaf: false,
                video: false,
            })
            .await;
        assert_eq!(
            result.unwrap_err().kind,
            crate::api::ErrorKind::OutcomeUnknown
        );
        assert!(queue.recv().await.unwrap().done.unwrap().is_closed());
    }
    #[test]
    fn ready_does_not_retain_session_or_token() {
        let p = ready_metadata(
            &json!({"token":"private","session_id":"private","user":{"id":"secret"},"presences":[{"user":{"id":"1"},"status":"online"}]}),
        );
        assert!(p.get("token").is_none());
        assert!(p.get("session_id").is_none());
        assert_eq!(p["presences"][0]["user"]["id"], "1")
    }
    #[test]
    fn ready_voice_seed_is_globally_bounded_and_drops_private_transport_fields() {
        let states = (1..=800).map(|id|json!({"user_id":id.to_string(),"channel_id":"99","session_id":"secret","token":"secret","member":{"private":"secret"},"self_video":true})).collect::<Vec<_>>();
        let p=ready_metadata(&json!({"guilds":[{"id":"10","voice_states":states},{"id":"11","voice_states":states}]}));
        let snapshots=items(&p["guild_voice_states"]);
        assert_eq!(items(&snapshots[0]["states"]).len(),800);
        assert_eq!(items(&snapshots[1]["states"]).len(),224);
        assert_eq!(snapshots[0]["partial"],false);
        assert_eq!(snapshots[1]["partial"],true);
        let output=p.to_string();
        assert!(!output.contains("secret") && !output.contains("session_id") && !output.contains("token"));
        assert_eq!(snapshots[0]["states"][0]["self_video"],true);
    }
    #[test]
    fn ready_voice_seed_retains_only_bounded_display_identity_and_server_nickname(){
        let metadata=ready_metadata(&json!({"users":[{"id":"2","username":"Account","global_name":"Global","avatar":"face","email":"private"}],"guilds":[{"id":"10","members":[{"user_id":"2","nick":"Server","roles":["private"]}],"voice_states":[{"user_id":"2","channel_id":"30","session_id":"private"}]}]}));
        let state=&metadata["guild_voice_states"][0]["states"][0];
        assert_eq!(state["member"]["nick"],"Server");assert_eq!(state["member"]["user"]["username"],"Account");assert_eq!(state["member"]["user"]["global_name"],"Global");
        assert!(state["member"]["user"].get("email").is_none());assert!(state.get("session_id").is_none());assert!(state["member"].get("roles").is_none());
        assert!(retained_dispatch("GUILD_MEMBER_REMOVE"));
    }
}
