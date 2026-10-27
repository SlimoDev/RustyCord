//! Single-owner state actor. Network replies carry session/navigation generations.
mod incoming;
mod deleted_logs;
mod presence;
mod profile_editor;
mod reaction_users;
mod channel_catalog;
mod emoji_policy;
mod notification_activation;
mod game_activity;
mod guild_settings;
mod member_moderation;
mod group_members;
#[cfg(test)]
mod network_safety_tests;
mod request_policy;
#[cfg(test)]
mod request_policy_tests;
mod social;
#[cfg(test)]
mod tests;
mod voice;
use crate::{
    media::MediaBridge,
    pickers,
    storage::{self, Features, MediaPreferences, MessageCache, Navigation, PickerHistory},
    ui::UiUpdate,
};
use notdiscord_core::{
    access::GuildAccess,
    api::{Api, Method},
    gateway::{GatewayCommand, GatewayEvent, GatewayHandle},
    models::*,
    parsing::{self, b, i, items, s},
    safety::{SafetyGate, SafetyState},
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
type Reply = Result<Value, String>;
#[cfg(feature="offline-probes")]
pub fn scan_game_processes_once()->Result<Value,String>{game_activity::scan_once_for_probe()}

// Read the chosen status before Identify. Unknown stays unknown if either
// settings endpoint is unavailable; opening the app never selects "online".
async fn prepare_session<F, R>(mut get: F) -> Reply
where
    F: FnMut(&'static str) -> R,
    R: std::future::Future<Output = Reply>,
{
    let user = get("users/@me").await?;
    if !parsing::valid_id(&s(&user, "id")) {
        return Err("Discord no confirmó la cuenta de esta sesión.".into());
    }
    let mut status = get("users/@me/settings")
        .await
        .ok()
        .map(|value| s(&value, "status"))
        .filter(|status| notdiscord_core::settings_proto::valid_presence(status));
    let proto = get("users/@me/settings-proto/1")
        .await
        .ok()
        .and_then(|value| {
            value["settings"]
                .as_str()
                .and_then(|encoded| notdiscord_core::settings_proto::read(encoded, true).ok())
        });
    if let Some(chosen) = proto.as_ref().and_then(|p| p.status.clone()) {
        status = Some(chosen);
    }
    Ok(
        json!({"user":user,"presence":status,"showCurrentGame":proto.and_then(|p|p.show_current_game)}),
    )
}
struct Completed {
    generation: u64,
    nav: Option<u64>,
    action: UiAction,
    result: Reply,
    credential: Option<crate::authentication::Credential>,
}
#[cfg(test)]
struct TestDataDirectory(PathBuf);
#[cfg(test)]
impl Drop for TestDataDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub struct Controller {
    root: PathBuf,
    data: PathBuf,
    state: AppSnapshot,
    pending_credential: Option<zeroize::Zeroizing<String>>,
    safety_store: Arc<crate::network_safety::SafetyStore>,
    safety_credential_key: String,
    safety_identity: Arc<Mutex<Option<String>>>,
    safety_enforced: SafetyState,
    restore_channel: String,
    resolved_notification_channel: Option<(u64, Value)>,
    last_guild_channels: HashMap<String, String>,
    last_private_channel: String,
    thread_catalog: channel_catalog::ThreadCatalog,
    reading_channel: String,
    message_seen: VecDeque<(String, String)>,
    unread_seen: VecDeque<(String, String)>,
    api: Option<Api>,
    gateway: Option<GatewayHandle>,
    gateway_rx: Option<mpsc::Receiver<GatewayEvent>>,
    generation: u64,
    nav: u64,
    session_cancel: tokio_util::sync::CancellationToken,
    navigation_cancel: tokio_util::sync::CancellationToken,
    completion: mpsc::UnboundedSender<Completed>,
    updates: Sender<UiUpdate>,
    ctx: eframe::egui::Context,
    features: Features,
    reaction_queries: reaction_users::Requests,
    cache: MessageCache,
    people: HashMap<String, Person>,
    channels: HashMap<String, Value>,
    guild_acl: Option<GuildAccess>,
    guild_raw: Value,
    guild_load: channel_catalog::GuildLoad,
    self_member: Value,
    picker: Option<PickerHistory>,
    premium: i32,
    selected_before_profile: String,
    muted: HashMap<String, bool>,
    latest: HashMap<String, String>,
    // Incoming unread watermark is independent of our own latest delivery.
    incoming_latest: HashMap<String, String>,
    read: HashMap<String, String>,
    mention: HashMap<String, i32>,
    pending_deltas: VecDeque<(String, Value)>,
    pending_delta_bytes: usize,
    loading_channel: String,
    typing: HashMap<String, Instant>,
    guild_member_slots: Vec<Option<ChannelMember>>,
    member_list_id: String,
    member_range: usize,
    media: Option<MediaBridge>,
    media_preferences: MediaPreferences,
    voice_channel: String,
    voice_guild: String,
    voice_session: String,
    voice_endpoint: String,
    voice_token: String,
    voice_attempt: u64,
    voice_started: bool,
    stream_key: String,
    stream_server: String,
    stream_endpoint: String,
    stream_token: String,
    stream_source: String,
    stream_quality: Value,
    stream_started: bool,
    call_people: HashMap<String, CallParticipant>,
    pending_video: bool,
    gif_cache: HashMap<String, (Instant, Value)>,
    emoji_cache: HashMap<String, (Instant, Vec<CustomEmoji>)>,
    voice_lifecycle: voice::VoiceLifecycle,
    latest_snapshot: Arc<Mutex<Option<AppSnapshot>>>,
    snapshot_dirty: AtomicBool,
    snapshot_at: Mutex<Instant>,
    request_versions: HashMap<String, u64>,
    last_typing_sent: Option<Instant>,
    friend_pending: HashSet<String>,
    login_pending: bool,
    send_typing: bool,
    member_demand: Option<(String, String)>,
    member_subscription: Option<(String, String, i32)>,
    read_acks: request_policy::AckBatch,
    sidebar_seed: Value,
    sidebar_wait: Option<Instant>,
    sidebar_inflight: bool,
    sidebar_deltas: VecDeque<(String, Value)>,
    sidebar_delta_bytes: usize,
    sidebar_epoch: u64,
    sidebar_cancel: tokio_util::sync::CancellationToken,
    private_channel_loads: HashSet<String>,
    private_channel_failures: HashMap<String, Instant>,
    notice_seen: VecDeque<String>,
    last_ptt: bool,
    game_detector: game_activity::Detector,
    notification_settings: notdiscord_core::notifications::SettingsStore,
    presence_changes: presence::Changes,
    #[cfg(test)]
    test_data_directory: Option<TestDataDirectory>,
}
pub async fn run(
    root: PathBuf,
    data: PathBuf,
    mut actions: mpsc::UnboundedReceiver<UiAction>,
    updates: Sender<UiUpdate>,
    ctx: eframe::egui::Context,
) {
    let _ = std::fs::create_dir_all(&data);
    let features = match Features::new(data.clone()) {
        Ok(f) => f,
        Err(error) => {
            let _ = updates.send(UiUpdate::Error(error));
            return;
        }
    };
    let (completion, mut replies) = mpsc::unbounded_channel();
    let preferences: MediaPreferences = storage::read_json(&data.join("media-settings.json"), 32768);
    let audio: Value = storage::read_json(&data.join("audio-settings.json"), 1024);
    let mut state = AppSnapshot::welcome();
    state.self_mute = b(&audio, "mute");
    state.self_deaf = b(&audio, "deaf");
    let mut c = Controller::new(
        root,
        data,
        state,
        completion,
        updates,
        ctx,
        features,
        preferences,
    );
    let _ = c
        .updates
        .send(UiUpdate::LatestSnapshot(c.latest_snapshot.clone()));
    c.publish();
    c.flush_snapshot(true);
    if let Some(token) = storage::load_session(&c.data) {
        c.connect_secret(token);
    }
    let mut clock = tokio::time::interval(Duration::from_millis(20));
    loop {
        tokio::select! {
            action=actions.recv()=>match action{Some(action)=>{if action.kind=="shutdown"{break}if let Err(error)=c.act(action.clone()).await{c.action_error(&action,error)}},None=>break},
            done=replies.recv()=>if let Some(done)=done{if c.accepts(&done){if let Some(credential)=done.credential {c.login_pending=false;c.disconnect().await;c.connect_secret(credential.into_secret());}else{match done.result{Ok(value)=>if let Err(error)=c.complete(done.action.clone(),value).await{c.complete_failed(&done.action).await;c.completion_error(&done.action,error)},Err(error)=>{c.complete_failed(&done.action).await;c.completion_error(&done.action,error)}}}}},
            event=async{match &mut c.gateway_rx{Some(rx)=>rx.recv().await,None=>std::future::pending().await}}=>match event{Some(GatewayEvent::Dispatch{kind,data})=>{if let Err(error)=c.dispatch(&kind,data).await{c.error(error)}},Some(GatewayEvent::Status(status))=>{c.state.connection=status;c.publish();},Some(GatewayEvent::Safety(state))=>{if let Some(api)=&c.api {api.safety().block(state);}},None=>{c.gateway_rx=None;}},
            _=clock.tick()=>{c.tick().await;},
        }
        c.sync_network_safety().await;
    }
    c.disconnect().await;
}
impl Controller {
    fn new(
        root: PathBuf,
        data: PathBuf,
        state: AppSnapshot,
        completion: mpsc::UnboundedSender<Completed>,
        updates: Sender<UiUpdate>,
        ctx: eframe::egui::Context,
        features: Features,
        preferences: MediaPreferences,
    ) -> Self {
        let safety_store = Arc::new(crate::network_safety::SafetyStore::load(&data));
        let ui_settings: Value = storage::read_json(&data.join("ui-settings.json"), 16384);
        let send_typing = ui_settings
            .get("SendTyping")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let preferences = preferences.normalize();
        let mut state = state;
        state.stream_volume_percent = preferences.stream_volume_percent;
        Self {
            root,
            data,
            state,
            pending_credential: None,
            safety_store,
            safety_credential_key: String::new(),
            safety_identity: Arc::new(Mutex::new(None)),
            safety_enforced: SafetyState::Allowed,
            restore_channel: String::new(),
            resolved_notification_channel: None,
            last_guild_channels: HashMap::new(),
            last_private_channel: String::new(),
            thread_catalog: channel_catalog::ThreadCatalog::default(),
            reading_channel: String::new(),
            message_seen: VecDeque::new(),
            unread_seen: VecDeque::new(),
            api: None,
            gateway: None,
            gateway_rx: None,
            generation: 0,
            nav: 0,
            session_cancel: tokio_util::sync::CancellationToken::new(),
            navigation_cancel: tokio_util::sync::CancellationToken::new(),
            completion,
            updates,
            ctx,
            features,
            reaction_queries: Default::default(),
            cache: MessageCache::default(),
            people: HashMap::new(),
            channels: HashMap::new(),
            guild_acl: None,
            guild_raw: Value::Null,
            guild_load: Default::default(),
            self_member: Value::Null,
            picker: None,
            premium: 0,
            selected_before_profile: String::new(),
            muted: HashMap::new(),
            latest: HashMap::new(),
            incoming_latest: HashMap::new(),
            read: HashMap::new(),
            mention: HashMap::new(),
            pending_deltas: VecDeque::new(),
            pending_delta_bytes: 0,
            loading_channel: String::new(),
            typing: HashMap::new(),
            guild_member_slots: Vec::new(),
            member_list_id: String::new(),
            member_range: 99,
            media: None,
            media_preferences: preferences,
            voice_channel: String::new(),
            voice_guild: String::new(),
            voice_session: String::new(),
            voice_endpoint: String::new(),
            voice_token: String::new(),
            voice_attempt: 0,
            voice_started: false,
            stream_key: String::new(),
            stream_server: String::new(),
            stream_endpoint: String::new(),
            stream_token: String::new(),
            stream_source: String::new(),
            stream_quality: Value::Null,
            stream_started: false,
            call_people: HashMap::new(),
            pending_video: false,
            gif_cache: HashMap::new(),
            emoji_cache: HashMap::new(),
            voice_lifecycle: voice::VoiceLifecycle::default(),
            latest_snapshot: Arc::new(Mutex::new(None)),
            snapshot_dirty: AtomicBool::new(false),
            snapshot_at: Mutex::new(Instant::now() - Duration::from_secs(1)),
            request_versions: HashMap::new(),
            last_typing_sent: None,
            friend_pending: HashSet::new(),
            login_pending: false,
            send_typing,
            member_demand: None,
            member_subscription: None,
            read_acks: request_policy::AckBatch::default(),
            sidebar_seed: Value::Null,
            sidebar_wait: None,
            sidebar_inflight: false,
            sidebar_deltas: VecDeque::new(),
            sidebar_delta_bytes: 0,
            sidebar_epoch: 0,
            sidebar_cancel: tokio_util::sync::CancellationToken::new(),
            private_channel_loads: HashSet::new(),
            private_channel_failures: HashMap::new(),
            notice_seen: VecDeque::new(),
            last_ptt: false,
            game_detector: game_activity::Detector::new(ui_settings.get("DetectGames").and_then(Value::as_bool).unwrap_or(true)),
            notification_settings: Default::default(),
            presence_changes: Default::default(),
            #[cfg(test)]
            test_data_directory: None,
        }
    }

    #[cfg(test)]
    fn test_fixture() -> (Self, std::sync::mpsc::Receiver<UiUpdate>) {
        let (c, updates, _) = Self::test_fixture_with_replies();
        (c, updates)
    }
    #[cfg(test)]
    fn test_fixture_with_replies() -> (
        Self,
        std::sync::mpsc::Receiver<UiUpdate>,
        mpsc::UnboundedReceiver<Completed>,
    ) {
        let data = std::env::temp_dir().join(format!(
            "notdiscord-rust-actor-fixture-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&data).unwrap();
        let features = Features::new(data.clone()).unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let (updates, updates_rx) = std::sync::mpsc::channel();
        let mut c = Self::new(
            data.clone(),
            data.clone(),
            AppSnapshot::welcome(),
            tx,
            updates,
            eframe::egui::Context::default(),
            features,
            MediaPreferences::default(),
        );
        c.test_data_directory = Some(TestDataDirectory(data));
        (c, updates_rx, rx)
    }
    fn accepts(&self, done: &Completed) -> bool {
        done.generation == self.generation
            && done.nav.is_none_or(|n| n == self.nav)
            && (done.action.kind != "__sidebar"
                || done.action.extra.parse::<u64>().unwrap_or(0) == self.sidebar_epoch)
    }
    async fn complete_failed(&mut self, action: &UiAction) {
        if action.kind == "__guild" {self.guild_load.clear();}
        if action.kind == "__status" {
            if let Err(error) = self.finish_presence_change(action, None) { self.error(error); }
        }
        if action.kind == "__guild-threads" { self.thread_catalog.finish_request(&action.id); }
        if action.kind == "__sidebar" {
            self.sidebar_inflight = false;
        }
        if action.kind == "__marked-read" && action.extra == "automatic" {
            self.read_acks.complete(&action.id);
        }
        if action.kind == "__messages" {
            self.loading_channel.clear();
            self.pending_deltas.clear();
            self.pending_delta_bytes = 0;
        }
        if action.kind == "__incoming-dm" {
            self.private_channel_loads.remove(&action.id);
            if self.private_channel_failures.len() >= 128 {
                if let Some(key) = self.private_channel_failures.keys().next().cloned() {
                    self.private_channel_failures.remove(&key);
                }
            }
            self.private_channel_failures
                .insert(action.id.clone(), Instant::now());
        }
        if action.kind == "__credential" {
            self.login_pending = false;
        }
        if action.kind == "__relations" {
            self.friend_pending.remove(&action.extra);
        }
        if action.kind == "__sent" {
            let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
            self.data("send-error",json!({"channelId":action.channel_id,"submissionId":extra["submissionId"],"revision":extra["revision"]}));
        }
        if action.kind.starts_with("__voice-") {
            self.voice_failed(action).await;
        }
    }
    fn known_jump(&self, id: &str) -> bool {
        let channel = &self.state.selected_channel_id;
        self.cache
            .get(channel)
            .iter()
            .chain(&self.state.pinned_messages)
            .chain(&self.state.messages)
            .any(|m| {
                m.channel_id == *channel
                    && (m.id == id
                        || m.reply_to.as_deref() == Some(id)
                            && m.reply_preview
                                .as_ref()
                                .is_none_or(|p| !p.deleted && p.channel_id == *channel))
            })
    }

    fn select_search_jump(&mut self, channel: &str, id: &str) -> Result<(), String> {
        Self::valid(channel)?;
        if self.state.view != "search" || self.state.selected_guild_id.is_empty()
            || !self.state.messages.iter().any(|message|message.id==id&&message.channel_id==channel&&!message.deleted) {
            return Err("El mensaje original no está disponible en esta búsqueda.".into());
        }
        let nav=self.state.channels.iter().find(|nav|nav.id==channel).cloned()
            .ok_or("La conversación del mensaje original ya no está disponible.")?;
        let raw=self.channels.get(channel).ok_or("No se han podido confirmar los permisos de la conversación.")?;
        let access=self.channel_access(raw);
        if !access.can_view||!access.can_read_history||s(raw,"guild_id")!=self.state.selected_guild_id {
            return Err("No tienes permiso para leer la conversación del mensaje original.".into());
        }
        self.reset_navigation();
        self.state.selected_channel_id=channel.into();
        self.last_guild_channels.insert(self.state.selected_guild_id.clone(),channel.into());
        self.state.channel_name=nav.name;
        self.state.selected_channel_kind=nav.kind;
        self.state.can_pin_messages=nav.can_pin;
        self.state.channel_access=access;
        self.state.has_older_messages=true;
        self.state.view="chat".into();
        self.refresh_messages();
        Ok(())
    }

    fn publish(&self) {
        self.snapshot_dirty.store(true, Ordering::Relaxed);
    }
    fn flush_snapshot(&self, force: bool) {
        if !self.snapshot_dirty.load(Ordering::Relaxed)
            || !force
                && self
                    .snapshot_at
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .elapsed()
                    < Duration::from_millis(60)
        {
            return;
        }
        if let Ok(mut slot) = self.latest_snapshot.lock() {
            let mut snapshot=self.state.clone();
            snapshot.upload_limit=self.upload_limit();
            *slot = Some(snapshot);
            self.snapshot_dirty.store(false, Ordering::Relaxed);
            *self.snapshot_at.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
            self.ctx.request_repaint();
        }
    }
    fn next_request(&mut self, name: &str) -> u64 {
        let version = self.request_versions.entry(name.into()).or_default();
        *version = version.wrapping_add(1);
        *version
    }
    fn upload_limit(&self)->notdiscord_core::uploads::UploadLimit {
        let tier=(!self.state.selected_guild_id.is_empty()
            && s(&self.guild_raw,"id")==self.state.selected_guild_id)
            .then(||self.guild_raw["premium_tier"].as_i64()).flatten()
            .and_then(|tier|i32::try_from(tier).ok()).filter(|tier|(0..=3).contains(tier));
        notdiscord_core::uploads::UploadLimit::from_entitlements(self.state.upload_limit.premium_type,tier)
    }
    fn request_current(&self, name: &str, version: u64) -> bool {
        self.request_versions.get(name) == Some(&version)
    }
    fn cancel_view_requests(&mut self) {
        self.invalidate_moderation();
        self.reaction_queries.clear_navigation();
        self.guild_load.clear();
        self.navigation_cancel.cancel();
        self.navigation_cancel = tokio_util::sync::CancellationToken::new();
        self.nav = self.nav.wrapping_add(1);
        self.pending_deltas.clear();
        self.pending_delta_bytes = 0;
        self.loading_channel.clear();
        self.state.focus_message_id.clear();
    }
    fn data(&self, name: &str, value: Value) {
        let _ = self.updates.send(UiUpdate::Data(name.into(), value));
        self.ctx.request_repaint();
    }
    fn error(&mut self, error: String) {
        self.state.status = error.clone();
        self.state.busy = false;
        let _ = self.updates.send(UiUpdate::Error(error));
        self.publish();
    }
    fn completion_error(&mut self, action: &UiAction, error: String) {
        if action.kind=="__group-member-removed" {self.finish_group_member_removal(action);}
        if action.kind=="__private-ring" {self.private_ring_failed(action);}
        if action.kind=="__older" {self.finish_history_page(action);}
        if matches!(action.kind.as_str(), "__voice-preflight" | "__voice-requested") {
            self.report_voice_error(&error);
        }
        if action.kind == "__guild-settings" && self.request_current("guild-settings", action.extra.parse().unwrap_or(0)) {
            if let Some(settings) = &mut self.state.guild_settings { settings.loading=false;settings.saving=false;settings.error=error.clone(); }
            self.publish();
        }
        if action.kind == "__dm_sidebar_profile" {
            self.data(
                "dm-sidebar-profile",
                json!({"channel_id":action.channel_id,"user_id":action.id,"error":error}),
            );
        } else if action.kind == "__forwarded" {
            let total = serde_json::from_str::<Value>(&action.text)
                .ok()
                .and_then(|value| value.as_array().map(Vec::len))
                .or_else(|| {
                    serde_json::from_str::<Value>(&action.extra)
                        .ok()
                        .and_then(|value| {
                            value
                                .as_array()
                                .or_else(|| value["destinations"].as_array())
                                .map(Vec::len)
                        })
                })
                .unwrap_or(0);
            self.data("forward-result",json!({"channel_id":action.channel_id,"message_id":action.id,"sent":0,"total":total,"partial":false,"error":error}));
        } else if matches!(action.kind.as_str(), "__marked-read" | "__guild-read") {
            // Read acknowledgements are independent of navigation. A failed
            // background ACK must not release another request's busy state.
            let _ = self.updates.send(UiUpdate::Error(format!(
                "No se pudo sincronizar la lectura: {error}"
            )));
            self.ctx.request_repaint();
        } else {
            self.error(error);
        }
    }
    fn action_error(&mut self, action: &UiAction, error: String) {
        if action.kind=="older" {self.finish_history_page(action);}
        if matches!(action.kind.as_str(), "voice" | "join-call" | "call-dm" | "call-dm-video" | "video-call") {
            self.report_voice_error(&error);
        }
        if matches!(action.kind.as_str(), "profile-editor-load" | "save-profile-bio" | "custom-status") {
            self.data(if action.kind=="profile-editor-load" {"profile-editor-loaded"} else {"profile-edit-result"},
                json!({"owner":self.me(),"token":action.id,"kind":if action.kind=="profile-editor-load" {"load"} else if action.kind=="save-profile-bio" {"bio"} else {"status"},"error":error}));
        }
        if matches!(action.kind.as_str(), "send" | "send-prepared") {
            let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
            self.data("send-error", json!({"channelId":action.channel_id,"submissionId":extra["submissionId"],"revision":extra["revision"]}));
        }
        let completion_kind = match action.kind.as_str() {
            "forward" | "forward-message" | "forward-many" => Some("__forwarded"),
            "dm-sidebar-profile" => Some("__dm_sidebar_profile"),
            _ => None,
        };
        if let Some(kind) = completion_kind {
            let mut context = action.clone();
            context.kind = kind.into();
            self.completion_error(&context, error);
        } else {
            self.error(error);
        }
    }
    fn status(&mut self, text: &str) {
        self.state.status = text.into();
        self.state.busy = false;
        self.publish();
    }
    fn launch<F>(&self, action: UiAction, nav: Option<u64>, future: F)
    where
        F: std::future::Future<Output = Reply> + Send + 'static,
    {
        let tx = self.completion.clone();
        let generation = self.generation;
        let session_cancel = self.session_cancel.clone();
        let navigation_cancel = self.navigation_cancel.clone();
        tokio::spawn(async move {
            let result = tokio::select! {_=session_cancel.cancelled()=>return,_=navigation_cancel.cancelled(),if nav.is_some()=>return,result=future=>result};
            let _ = tx.send(Completed {
                generation,
                nav,
                action,
                result,
                credential: None,
            });
        });
    }
    fn launch_credential(&self) {
        let data = self.data.clone();
        let tx = self.completion.clone();
        let generation = self.generation;
        let cancel = self.session_cancel.clone();
        tokio::spawn(async move {
            let result = tokio::select! {biased;_=cancel.cancelled()=>return,result=crate::authentication::interactive(&data)=>result};
            let (credential, result) = match result {
                Ok(value) => (Some(value), Ok(Value::Null)),
                Err(error) => (None, Err(error)),
            };
            let _ = tx.send(Completed {
                generation,
                nav: None,
                action: UiAction::new("__credential"),
                result,
                credential,
            });
        });
    }
    fn api(&self) -> Result<Api, String> {
        self.api
            .clone()
            .ok_or("Inicia sesión para continuar.".into())
    }
    fn me(&self) -> String {
        self.state
            .current_user
            .as_ref()
            .map(|p| p.id.clone())
            .unwrap_or_default()
    }
    fn valid(id: &str) -> Result<&str, String> {
        if parsing::valid_id(id) {
            Ok(id)
        } else {
            Err("Identificador no válido.".into())
        }
    }
    fn remember(&mut self, p: Person) {
        if p.id.is_empty() {
            return;
        }
        if self.people.len() >= 3000 && !self.people.contains_key(&p.id) {
            if let Some(key) = self.people.keys().next().cloned() {
                self.people.remove(&key);
            }
        }
        self.people.insert(p.id.clone(), p);
    }
    fn require(&self, what: &str) -> Result<(), String> {
        if self.state.channel_access.allows(what) {
            Ok(())
        } else {
            Err(if self.state.channel_access.reason.is_empty() {
                "No tienes permiso para esta acción.".into()
            } else {
                self.state.channel_access.reason.clone()
            })
        }
    }
    fn reset_session_view(&mut self) {
        self.last_private_channel.clear();
        self.game_detector.reset();
        self.clear_private_call_state();
        self.reaction_queries=Default::default();
        self.features.clear_history();
        let (mute, deaf) = (self.state.self_mute, self.state.self_deaf);
        self.state = AppSnapshot::welcome();
        self.state.notification_generation = self.generation;
        self.state.self_mute = mute;
        self.state.self_deaf = deaf;
    }
    fn session_safety_gate(&self) -> SafetyGate {
        let store = self.safety_store.clone();
        let key = self.safety_credential_key.clone();
        let identity = self.safety_identity.clone();
        let updates = self.updates.clone();
        let ctx = self.ctx.clone();
        SafetyGate::with_observer(move |state| {
            // The closure belongs to the gate, not the actor's current session.
            // A late response still records its original identity after logout.
            let account = identity.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Err(error) = store.record(&key, account.as_deref(), state) {
                let _ = updates.send(UiUpdate::Error(error));
                ctx.request_repaint();
            }
        })
    }
    /// The gate owns the stop; strings returned by legacy actions cannot clear
    /// it. Keep the reason visible separately from dismissible ordinary errors.
    async fn sync_network_safety(&mut self) {
        let Some(api) = &self.api else { return };
        let state = api.safety().state();
        if state == SafetyState::Allowed || state == self.safety_enforced {
            return;
        }
        self.safety_enforced = state;
        let identity = self
            .safety_identity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let account = identity.as_deref().or_else(|| {
            self.state
                .current_user
                .as_ref()
                .map(|user| user.id.as_str())
        });
        let persisted = self
            .safety_store
            .record(&self.safety_credential_key, account, state);
        let mut message = self
            .safety_store
            .unavailable_message()
            .unwrap_or(crate::network_safety::message(state))
            .to_owned();
        if let Err(error) = persisted {
            message.push(' ');
            message.push_str(&error);
        }
        // Closing the native transport also releases media capture. No further
        // HTTP/WS work can leave through the already-blocked shared gate.
        self.game_detector.suspend_rpc();
        self.game_detector.shutdown_rpc().await;
        if let Some(gateway) = self.gateway.take() {
            gateway.stop();
        }
        self.gateway_rx = None;
        self.stop_voice().await;
        self.pending_credential = None;
        self.friend_pending.clear();
        self.state.busy = false;
        self.state.network_safety = state;
        self.state.network_safety_message = message.clone();
        self.state.connection = "Operaciones suspendidas".into();
        self.state.status = message;
        self.publish();
    }
    fn apply_initial_settings(&mut self, value: &Value) -> Option<String> {
        let status = s(value, "presence");
        if notdiscord_core::settings_proto::valid_presence(&status) {
            self.state.self_presence = status.clone();
        }
        if let Some(show) = value["showCurrentGame"].as_bool() {
            self.state.game_activity_enabled = show;
            self.state.game_activity_available = true;
            self.state.game_activity_configured = true;
            self.state.game_activity_label = "Visibilidad de actividad".into();
        }
        notdiscord_core::settings_proto::valid_presence(&status).then_some(status)
    }
    #[cfg(test)]
    fn connect(&mut self, token: String) {
        self.connect_secret(zeroize::Zeroizing::new(token));
    }
    fn connect_secret(&mut self, token: zeroize::Zeroizing<String>) {
        self.presence_changes = Default::default();
        self.game_detector.reset();
        self.notification_settings = Default::default();
        self.session_cancel.cancel();
        self.session_cancel = tokio_util::sync::CancellationToken::new();
        if let Some(gateway) = self.gateway.take() {
            gateway.stop();
        }
        self.gateway_rx = None;
        self.api = None;
        self.reset_navigation();
        self.generation += 1;
        self.reset_session_view();
        self.safety_credential_key = crate::network_safety::credential_key(&token);
        self.safety_identity = Arc::new(Mutex::new(
            self.safety_store
                .account_for_key(&self.safety_credential_key),
        ));
        self.safety_enforced = SafetyState::Allowed;
        self.state.connection = "Conectando".into();
        self.state.status = "Comprobando sesión…".into();
        self.cache.clear();
        self.sidebar_seed = Value::Null;
        self.sidebar_wait = None;
        self.sidebar_inflight = false;
        self.sidebar_deltas.clear();
        self.sidebar_delta_bytes = 0;
        self.sidebar_cancel.cancel();
        self.sidebar_cancel = tokio_util::sync::CancellationToken::new();
        self.sidebar_epoch = self.sidebar_epoch.wrapping_add(1);
        self.read_acks.clear();
        self.member_subscription = None;
        self.people = HashMap::new();
        self.channels = HashMap::new();
        self.guild_acl = None;
        self.guild_raw = Value::Null;
        self.self_member = Value::Null;
        self.muted.clear();
        self.latest.clear();
        self.incoming_latest.clear();
        self.read.clear();
        self.mention.clear();
        self.notice_seen.clear();
        self.gifs_clear();
        self.request_versions.clear();
        self.friend_pending.clear();
        self.restore_channel.clear();
        self.last_guild_channels.clear();
        self.thread_catalog = channel_catalog::ThreadCatalog::default();
        self.guild_load.clear();
        self.reading_channel.clear();
        self.message_seen.clear();
        self.unread_seen.clear();
        self.private_channel_loads.clear();
        self.private_channel_failures.clear();
        self.publish();
        self.pending_credential = Some(token.clone());
        let gate = self.session_safety_gate();
        gate.block(self.safety_store.initial_state(&self.safety_credential_key));
        match Api::new_with_secret(token, gate) {
            Ok(api) => {
                self.api = Some(api.clone());
                if api.safety().state() != SafetyState::Allowed {
                    self.state.network_safety = api.safety().state();
                    self.state.network_safety_message = self
                        .safety_store
                        .unavailable_message()
                        .unwrap_or(crate::network_safety::message(api.safety().state()))
                        .into();
                    self.state.connection = "Operaciones suspendidas".into();
                    self.state.status = self.state.network_safety_message.clone();
                    self.publish();
                    return;
                }
                let store = self.safety_store.clone();
                let credential_key = self.safety_credential_key.clone();
                let identity = self.safety_identity.clone();
                self.launch(UiAction::new("__login"), None, async move {
                    prepare_session(move |path| {
                        let api = api.clone();
                        let store = store.clone();
                        let credential_key = credential_key.clone();
                        let identity = identity.clone();
                        async move {
                            let value = api.get(path).await.map_err(|e| e.to_string())?;
                            if path == "users/@me" {
                                let account = s(&value, "id");
                                if parsing::valid_id(&account) {
                                    *identity.lock().unwrap_or_else(|e| e.into_inner()) =
                                        Some(account.clone());
                                }
                                store.inspect_identity(&credential_key, &account, &api.safety())?;
                            }
                            Ok(value)
                        }
                    })
                    .await
                });
            }
            Err(e) => {
                self.pending_credential = None;
                self.error(e.to_string());
            }
        }
    }
    async fn disconnect(&mut self) {
        self.presence_changes = Default::default();
        self.game_detector.reset();
        self.game_detector.shutdown_rpc().await;
        self.notification_settings = Default::default();
        self.sync_network_safety().await;
        self.login_pending = false;
        self.sidebar_wait = None;
        self.sidebar_inflight = false;
        self.sidebar_deltas.clear();
        self.sidebar_delta_bytes = 0;
        self.sidebar_cancel.cancel();
        self.read_acks.clear();
        self.session_cancel.cancel();
        self.reset_navigation();
        self.generation += 1;
        self.stop_voice().await;
        if let Some(g) = self.gateway.take() {
            g.stop();
        }
        self.gateway_rx = None;
        if let Some(media) = self.media.take() {
            let _ = media.command("shutdown", json!({})).await;
        }
        self.api = None;
        self.pending_credential = None;
        self.safety_credential_key.clear();
        self.safety_identity = Arc::new(Mutex::new(None));
        self.safety_enforced = SafetyState::Allowed;
        self.picker = None;
        self.cache.clear();
        self.people = HashMap::new();
        self.channels = HashMap::new();
        self.gifs_clear();
        self.reset_session_view();
        self.publish();
    }
    fn gifs_clear(&mut self) {
        self.gif_cache.clear();
        self.emoji_cache.clear();
    }
    fn sidebar(&mut self) {
        if self.sidebar_inflight {
            return;
        }
        let Ok(api) = self.api() else { return };
        self.sidebar_wait = None;
        self.sidebar_inflight = true;
        let seed = self.sidebar_seed.clone();
        let cancel = self.sidebar_cancel.clone();
        let mut action = UiAction::new("__sidebar");
        action.extra = self.sidebar_epoch.to_string();
        self.launch(action, None, async move {
            tokio::select! {biased;_=cancel.cancelled()=>Err("El catálogo se sustituyó por una sesión más reciente.".into()),result=request_policy::load_sidebar(seed,move|path|{let api=api.clone();async move{api.get(path).await.map_err(|e|e.to_string())}})=>result}
        });
    }
    fn sidebar_guilds(&self, values: &Value) -> Vec<NavItem> {
        items(values)
            .iter()
            .take(500)
            .map(|value| {
                let mut guild = parsing::guild(value);
                guild.guild_notifications = self.notification_settings.get(&guild.id);
                if let Some(previous) = self
                    .state
                    .guilds
                    .iter()
                    .find(|previous| previous.id == guild.id)
                {
                    guild.unread = previous.unread;
                    guild.mention_count = previous.mention_count;
                    guild.is_muted = previous.is_muted;
                }
                if let Some(muted)=guild.guild_notifications.muted {guild.is_muted=muted;}
                guild
            })
            .collect()
    }
    fn reset_navigation(&mut self) {
        self.resolved_notification_channel = None;
        self.restore_channel.clear();
        self.member_demand = None;
        self.queue_member_subscription();
        self.reading_channel.clear();
        self.cancel_view_requests();
        self.state.member_context_version = self.state.member_context_version.wrapping_add(1);
        self.typing.clear();
        self.state.typing_text.clear();
        self.state.focus_message_id.clear();
        self.state.guild_members.clear();
        self.state.group_members = Vec::new();
        self.state.group_owner_id.clear();
        self.state.group_member_removal_pending_ids.clear();
        self.state.group_recipient_candidates = Vec::new();
        self.state.pinned_messages = Vec::new();
        self.state.guild_member_groups = Vec::new();
        self.state.guild_members_total = 0;
        self.state.guild_members_note.clear();
        self.state.search_people = Vec::new();
        self.guild_member_slots = Vec::new();
        self.member_list_id.clear();
        self.member_range = 99;
    }
    fn invalidate_member_context(&mut self) {
        self.member_demand = None;
        self.queue_member_subscription();
        self.state.guild_members.clear();
        self.guild_member_slots = Vec::new();
        self.member_list_id.clear();
        self.state.guild_member_groups = Vec::new();
        self.state.guild_members_total = 0;
        self.state.guild_members_partial = false;
        self.state.guild_members_has_more = false;
        self.state.guild_members_note.clear();
        self.member_range = 99;
        // This revision is independent of navigation: ACL changes can retire
        // a binding while account, guild and channel IDs remain unchanged.
        self.state.member_context_version = self.state.member_context_version.wrapping_add(1);
    }
    fn select_guild(&mut self, id: &str) -> Result<(), String> {
        self.retain_active_voice_permissions();
        if id.is_empty() {
            // The main Discord button returns from a server to the last
            // private conversation, if it is still in this account's list.
            if !self.state.selected_guild_id.is_empty()
                && self.state.direct_messages.iter().any(|item| {
                    item.id == self.last_private_channel
                        && matches!(item.kind.as_str(), "dm" | "group")
                        && item.access.as_ref().is_none_or(|access| !access.known || access.can_view)
                })
            {
                return self.select_channel_at(&self.last_private_channel.clone(), "restore-private");
            }
            self.state.guild_nicknames.clear();
            self.reset_navigation();
            self.state.selected_guild_id.clear();
            self.state.selected_channel_id.clear();
            self.channels.clear();
            self.guild_acl = None;
            self.state.channels.clear();
            self.state.messages.clear();
            self.state.view = "friends".into();
            self.state.channel_name = "Amigos".into();
            self.publish();
            return Ok(());
        }
        Self::valid(id)?;
        if !self.state.guilds.iter().any(|g| g.id == id) {
            return Err("Servidor no disponible.".into());
        }
        self.state.guild_nicknames.clear();
        self.reset_navigation();
        self.channels.clear();
        self.guild_acl = None;
        self.guild_raw = Value::Null;
        self.self_member = Value::Null;
        self.state.selected_guild_id = id.into();
        self.state.selected_channel_id.clear();
        self.state.messages.clear();
        self.state.channels.clear();
        self.state.view = "chat".into();
        self.state.channel_name = "Selecciona un canal".into();
        self.state.channel_access = ChannelAccess::default();
        self.publish();
        let api = self.api()?;
        let id = id.to_owned();
        let me = self.me();
        self.guild_load.begin(&id);
        self.launch(UiAction::id("__guild", &id), Some(self.nav), async move {
            let guild = api
                .get(&format!("guilds/{id}"))
                .await
                .map_err(|e| e.to_string())?;
            let member = api
                .get(&format!("guilds/{id}/members/{me}"))
                .await
                .map_err(|e| e.to_string())?;
            let channels = api
                .get(&format!("guilds/{id}/channels"))
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({"guild":guild,"member":member,"channels":channels}))
        });
        Ok(())
    }
    fn channel_access(&self, value: &Value) -> ChannelAccess {
        if i(value, "type") == 1 || i(value, "type") == 3 {
            return ChannelAccess::private();
        }
        self.guild_acl
            .as_ref()
            .map(|acl| acl.evaluate(value, self.channels.get(&s(value, "parent_id"))))
            .unwrap_or_default()
    }
    pub(super) fn recover_guild_navigation(&mut self) -> Result<(), String> {
        if self.state.selected_guild_id.is_empty() {
            return Ok(());
        }
        // Gateway deltas may arrive before the initial guild/member ACL reads.
        // Access is still revoked by the caller, but choosing a replacement now
        // would cancel those reads and discard the changes waiting to replay.
        if self.guild_load.is_loading(&self.state.selected_guild_id) {
            return Ok(());
        }
        let candidate =
            preferred_guild_channel(&self.state.channels, &self.state.selected_channel_id);
        if candidate.as_deref() == Some(self.state.selected_channel_id.as_str()) {
            return Ok(());
        }
        if let Some(id) = candidate {
            self.select_channel(&id)
        } else {
            self.reset_navigation();
            self.state.selected_channel_id.clear();
            self.state.messages.clear();
            self.state.view = "chat".into();
            self.state.channel_name = "No hay canales de texto accesibles".into();
            self.state.channel_access = ChannelAccess::default();
            self.publish();
            Ok(())
        }
    }
    fn select_channel(&mut self, id: &str) -> Result<(), String> {
        self.select_channel_at(id, "")
    }
    fn select_channel_at(&mut self, id: &str, anchor_extra: &str) -> Result<(), String> {
        Self::valid(id)?;
        let is_dm = self.state.direct_messages.iter().any(|n| n.id == id);
        let nav = self
            .state
            .channels
            .iter()
            .chain(&self.state.direct_messages)
            .find(|n| n.id == id)
            .cloned();
        if nav.is_none() {
            return Err("Esta conversación no está disponible en la lista actual.".into());
        }
        self.retain_active_voice_permissions();
        self.reset_navigation();
        if is_dm {
            self.state.selected_guild_id.clear();
            self.state.channels.clear();
            self.channels.clear();
            self.guild_acl = None;
        }
        self.state.selected_channel_id = id.into();
        if !is_dm && !self.state.selected_guild_id.is_empty() {
            self.last_guild_channels
                .insert(self.state.selected_guild_id.clone(), id.into());
        }
        self.state.view = "chat".into();
        self.state.channel_name = nav
            .as_ref()
            .map(|n| n.name.clone())
            .unwrap_or("Conversación".into());
        self.state.selected_channel_kind = nav.as_ref().map(|n| n.kind.clone()).unwrap_or_default();
        if is_dm && matches!(self.state.selected_channel_kind.as_str(), "dm" | "group") {
            self.last_private_channel = id.into();
            self.save_navigation();
        }
        self.state.messages = self.features.restore_session(id,self.cache.get(id)).into();
        self.state.pinned_messages.clear();
        self.state.profile_user = None;
        self.state.profile_details = None;
        self.state.channel_access = if is_dm {
            ChannelAccess::private()
        } else {
            self.channels
                .get(id)
                .map(|v| self.channel_access(v))
                .unwrap_or_default()
        };
        self.state.can_pin_messages = nav.as_ref().is_some_and(|n| n.can_pin) || is_dm;
        self.state.has_older_messages = true;
        self.loading_channel = id.into();
        if !self.state.channel_access.can_read_history {
            self.state.messages.clear();
            self.loading_channel.clear();
            self.publish();
            return Ok(());
        }
        self.publish();
        let api = self.api()?;
        let id = id.to_owned();
        let anchor = restore_anchor(anchor_extra);
        let returning_private = is_dm && anchor_extra == "restore-private";
        let mut done = UiAction::id("__messages", &id);
        if returning_private { done.extra = "restore-private".into(); }
        self.launch(
            done,
            Some(self.nav),
            async move {
                let channel = match api.get(&format!("channels/{id}")).await {
                    Ok(channel) => channel,
                    Err(error) if returning_private && matches!(error.status, Some(403 | 404)) => {
                        return Ok(json!({"privateUnavailable":true}));
                    }
                    Err(error) => return Err(error.to_string()),
                };
                let endpoint = message_page_route(&id, anchor.as_deref());
                let loaded = api.get(&endpoint).await;
                let messages = match loaded {
                    Ok(messages)
                        if anchor
                            .as_ref()
                            .is_none_or(|a| items(&messages).iter().any(|m| s(m, "id") == *a)) =>
                    {
                        messages
                    }
                    Ok(_) => api
                        .get(&message_page_route(&id, None))
                        .await
                        .map_err(|e| e.to_string())?,
                    Err(error) if anchor.is_some() && error.status == Some(404) => api
                        .get(&message_page_route(&id, None))
                        .await
                        .map_err(|e| e.to_string())?,
                    Err(error) if returning_private && matches!(error.status, Some(403 | 404)) => {
                        return Ok(json!({"privateUnavailable":true}));
                    }
                    Err(error) => return Err(error.to_string()),
                };
                Ok(json!({"channel":channel,"messages":messages}))
            },
        );
        Ok(())
    }
    fn restore_navigation(&mut self) {
        let saved: Navigation = storage::read_json(&self.data.join("navigation.json"), 8192);
        if saved.account_id != self.me() {
            return;
        }
        self.last_private_channel = [saved.last_private_channel_id.as_str(), saved.channel_id.as_str()]
            .into_iter()
            .find(|id| self.state.direct_messages.iter().any(|item| {
                item.id == *id && matches!(item.kind.as_str(), "dm" | "group")
            }))
            .unwrap_or_default()
            .to_owned();
        if !saved.guild_id.is_empty() && self.state.guilds.iter().any(|g| g.id == saved.guild_id) {
            let _ = self.select_guild(&saved.guild_id);
        } else if self
            .state
            .direct_messages
            .iter()
            .any(|n| n.id == saved.channel_id)
        {
            let _ = self.select_channel(&saved.channel_id);
        }
    }
    fn save_navigation(&self) {
        let _ = storage::save_json(
            &self.data.join("navigation.json"),
            &Navigation {
                account_id: self.me(),
                guild_id: self.state.selected_guild_id.clone(),
                channel_id: if self.state.selected_guild_id.is_empty() {
                    self.state.selected_channel_id.clone()
                } else {
                    String::new()
                },
                last_private_channel_id: self.last_private_channel.clone(),
            },
        );
    }
    fn checked_content(&self, text: &str) -> Result<String, String> {
        pickers::prepare_outgoing(text, |id| {
            self.emoji_by_id_allowed(id)
        })
    }
    fn refresh_messages(&mut self) {
        if self.state.view == "chat" {
            self.state.messages = (if self.state.channel_access.can_read_history
                && self.state.channel_access.can_view
            {
                parsing::context_messages(self.features.restore_session(&self.state.selected_channel_id,self.cache.get(&self.state.selected_channel_id)))
            } else {
                Vec::new()
            })
            .into();
        }
        self.member_list_fallback();
        self.search_people();
    }
    fn finish_history_page(&self,action:&UiAction) {
        self.data("history-page-result",json!({"requestId":action.id,"channelId":action.channel_id}));
    }
    fn older_page_route(&self, channel: &str) -> String {
        // Session tombstones can precede the loaded REST window in the UI.
        // They must not move the server pagination frontier past unseen rows.
        let before=self.cache.get(channel).first().map(|message|message.id.clone()).unwrap_or_default();
        format!("channels/{channel}/messages?limit=50&before={before}")
    }
    fn record_loaded_delta(&mut self,messages:&[ChatMessage],kind:&str,value:&Value) {
        if matches!(kind,"MESSAGE_UPDATE"|"MESSAGE_DELETE") && let Some(previous)=messages.iter().find(|message|message.id==s(value,"id")) {
            if !previous.deleted && (kind=="MESSAGE_DELETE"||value["content"].as_str().is_some_and(|content|content!=previous.content)) {
                if let Err(error)=self.features.record(previous,kind=="MESSAGE_DELETE") {self.error(error);}
            }
        }
    }
    fn apply_message_pin(&mut self, channel: &str, id: &str, pinned: bool) {
        let mut cached = self.cache.get(channel);
        if !cached.is_empty() {
            for message in &mut cached {
                if message.id == id {
                    message.pinned = pinned;
                }
            }
            self.cache.replace(channel, cached);
        }
        let source = self
            .state
            .messages
            .iter()
            .chain(&self.state.pinned_messages)
            .find(|message| message.id == id && message.channel_id == channel)
            .cloned();
        for message in &mut self.state.messages {
            if message.id == id && message.channel_id == channel {
                message.pinned = pinned;
            }
        }
        if self.state.selected_channel_id == channel {
            if pinned {
                if !self
                    .state
                    .pinned_messages
                    .iter()
                    .any(|message| message.id == id && message.channel_id == channel)
                {
                    if let Some(mut message) = source {
                        message.pinned = true;
                        self.state.pinned_messages.insert(0, message);
                    }
                }
            } else {
                self.state
                    .pinned_messages
                    .retain(|message| message.id != id || message.channel_id != channel);
            }
            if self.state.view == "pins" {
                self.state.messages = self.state.pinned_messages.clone().into();
            }
        }
    }
    fn search_people(&mut self) {
        if self.state.selected_channel_id.is_empty() || !self.state.channel_access.can_view {
            self.state.search_people.clear();
            return;
        }
        let mut ids = std::collections::HashSet::new();
        let mut list = Vec::new();
        for p in self
            .state
            .messages
            .iter()
            .map(|m| &m.author)
            .chain(self.state.group_members.iter().map(|m| &m.user))
            .chain(self.state.guild_members.iter().map(|m| &m.user))
            .chain(self.state.current_user.iter())
        {
            if ids.insert(p.id.clone()) {
                list.push(p.clone())
            }
        }
        if self.state.selected_guild_id.is_empty() {
            if let Some(dm) = self
                .state
                .direct_messages
                .iter()
                .find(|n| n.id == self.state.selected_channel_id)
            {
                if let Some(p) = self.people.get(&dm.recipient_id) {
                    if ids.insert(p.id.clone()) {
                        list.push(p.clone())
                    }
                }
            }
        }
        self.state.search_people = list;
    }
    async fn act(&mut self, mut action: UiAction) -> Result<(), String> {
        // Forgetting a local tombstone is never a Discord mutation, nor does it
        // require an account/network request or a change of navigation context.
        if action.kind == "purge-deleted-log" { return self.purge_deleted_log(&action); }
        self.sync_network_safety().await;
        action.kind = match action.kind.as_str() {
            "video-call" => "call-dm-video",
            "toggle-mute" => "mute",
            "toggle-deafen" => "deafen",
            "toggle-game-activity" => "activity-toggle",
            "latest-messages" => "present",
            "add-friend-id" => "add-friend",
            _ => &action.kind,
        }
        .to_owned();
        match action.kind.as_str() {
            "dismiss-voice-error" => {
                self.state.voice_error.clear();
                self.publish();
                return Ok(());
            }
            "notification-open" => {
                self.request_notification_activation(&action)?;
                return Ok(());
            }
            "open-security-help" => {
                self.data("open-official-security", json!({}));
                return Ok(());
            }
            "invite-preview" | "invite-accept" => {
                self.social_action(action).await?;
                return Ok(());
            }
            "login" => {
                if self.login_pending {
                    return Ok(());
                }
                self.login_pending = true;
                self.launch_credential();
                return Ok(());
            }
            "logout" => {
                self.disconnect().await;
                storage::forget_session(&self.data)?;
                return Ok(());
            }
            "ui-settings" => {
                let settings: Value = serde_json::from_str(&action.text).unwrap_or_default();
                if let Some(enabled) = settings.get("SendTyping").and_then(Value::as_bool) {
                    self.send_typing = enabled;
                }
                if let Some(enabled) = settings.get("DetectGames").and_then(Value::as_bool) {
                    self.game_detector.set_enabled(enabled);
                    self.tick_game_activity().await;
                }
                return Ok(());
            }
            _ => (),
        }
        if !self.state.signed_in {
            return Err("Inicia sesión para continuar.".into());
        }
        if is_network_mutation(&action.kind) {
            if let Some(api) = &self.api {
                api.safety().check().map_err(|error| error.to_string())?;
            }
        }
        if !action.channel_id.is_empty()
            && matches!(
                action.kind.as_str(),
                "send"
                    | "send-prepared"
                    | "edit"
                    | "delete"
                    | "react"
                    | "react-toggle"
                    | "pin-message"
                    | "unpin-message"
                    | "pins"
                    | "search"
                    | "forward"
                    | "forward-many"
            )
            && action.channel_id != self.state.selected_channel_id
        {
            return Err("El contexto del chat cambió. La acción se canceló.".into());
        }
        if self.guild_settings_action(&action)? { return Ok(()); }
        if self.member_moderation_action(&action)? { return Ok(()); }
        if self.remove_group_member_action(&action)? { return Ok(()); }
        if action.kind == "reaction-users" { self.request_reaction_users(action); return Ok(()); }
        if self.media_action(&action).await? {
            return Ok(());
        }
        match action.kind.as_str() {
            "guild" => self.select_guild(&action.id)?,
            "reject-call" => self.reject_call(&action)?,
            "channel" => self.select_channel_at(&action.id, &action.extra)?,
            "chat-visibility" => self.report_chat_visibility(&action)?,
            "call-invite" => self.invite_private_call(&action)?,
            "member-demand" => {
                let channel = if action.channel_id.is_empty() {
                    &action.id
                } else {
                    &action.channel_id
                };
                if action.text == "hidden" {
                    if self
                        .member_demand
                        .as_ref()
                        .is_some_and(|(guild, id)| guild == &action.extra && id == channel)
                    {
                        self.member_demand = None;
                        self.queue_member_subscription();
                    }
                } else if action.text == "visible"
                    && self.state.view == "chat"
                    && self.state.selected_channel_id == *channel
                    && self.state.selected_guild_id == action.extra
                    && self.state.channel_access.can_view_members
                    && parsing::valid_id(channel)
                    && parsing::valid_id(&action.extra)
                {
                    self.member_demand = Some((action.extra.clone(), channel.clone()));
                    self.member_list_fallback();
                    self.queue_member_subscription();
                }
            }
            "group-members" => {
                if self.state.selected_channel_kind != "group"
                    || self.state.selected_channel_id.is_empty()
                {
                    return Err("Abre un grupo para consultar sus miembros.".into());
                }
                let channel = self.state.selected_channel_id.clone();
                let api = self.api()?;
                let done=self.group_refresh_action(&channel);
                self.launch(
                    done,
                    Some(self.nav),
                    async move {
                        api.get(&format!("channels/{channel}"))
                            .await
                            .map_err(|e| e.to_string())
                    },
                );
            }
            "notification-navigate" => self.navigate_notification(action)?,
            "view" => {
                self.reset_navigation();
                self.state.selected_guild_id.clear();
                self.state.selected_channel_id.clear();
                self.state.channels.clear();
                self.channels.clear();
                self.guild_acl = None;
                self.guild_raw = Value::Null;
                self.self_member = Value::Null;
                self.state.messages.clear();
                self.state.profile_user = None;
                self.state.profile_details = None;
                self.state.channel_access = ChannelAccess::default();
                self.state.can_pin_messages = false;
                self.state.view = action.id.clone();
                self.state.channel_name = if action.id == "requests" {
                    "Solicitudes"
                } else {
                    "Amigos"
                }
                .into();
                self.publish();
            }
            "return-chat" => {
                self.cancel_view_requests();
                let previous = if self.state.view == "profile"
                    && matches!(
                        self.selected_before_profile.as_str(),
                        "pins" | "chat" | "search" | "history" | "friends" | "requests"
                    ) {
                    self.selected_before_profile.clone()
                } else {
                    "chat".into()
                };
                self.state.view = previous;
                self.state.profile_user = None;
                self.state.profile_details = None;
                if self.state.view == "pins" {
                    self.state.messages = self.state.pinned_messages.clone().into();
                } else if self.state.view == "chat" {
                    self.refresh_messages();
                }
                self.publish();
            }
            "pins" => {
                if self.state.view == "pins" {
                    self.cancel_view_requests();
                    self.state.view = "chat".into();
                    self.refresh_messages();
                    self.publish();
                } else {
                    self.require("history")?;
                    let api = self.api()?;
                    let id = self.state.selected_channel_id.clone();
                    self.cancel_view_requests();
                    self.state.view = "pins".into();
                    self.state.messages.clear();
                    self.state.pinned_messages.clear();
                    self.publish();
                    let mut request = UiAction::id("__pins", &id);
                    request.extra = self.next_request("pins").to_string();
                    self.launch(request, Some(self.nav), async move {
                        api.get(&format!("channels/{id}/messages/pins?limit=50"))
                            .await
                            .map_err(|e| e.to_string())
                    });
                }
            }
            "older" | "present" | "jump-message" => {
                if action.kind == "present" && !action.channel_id.is_empty()
                    && (action.channel_id != self.state.selected_channel_id || self.state.view != "chat") {
                    // A send/latest command can outlive the UI's channel. Do
                    // not redirect a subsequently selected chat or search view.
                    return Ok(());
                }
                self.require("history")?;
                if action.kind=="older" && !action.channel_id.is_empty() && action.channel_id!=self.state.selected_channel_id {
                    return Err("La conversación de la página anterior cambió.".into());
                }
                if action.kind == "older" && (self.state.busy || !self.state.has_older_messages) {
                    self.finish_history_page(&action);
                    return Ok(());
                }
                let crossed_search = action.kind == "jump-message"
                    && !action.channel_id.is_empty()
                    && action.channel_id != self.state.selected_channel_id;
                if crossed_search {
                    self.select_search_jump(&action.channel_id,&action.id)?;
                }
                if action.kind == "jump-message" && !crossed_search && !self.known_jump(&action.id) {
                    return Err(
                        "El mensaje original no está disponible en esta conversación.".into(),
                    );
                }
                if action.kind == "jump-message"
                    && self.state.view == "chat"
                    && self.state.messages.iter().any(|message| {
                        message.id == action.id
                            && message.channel_id == self.state.selected_channel_id
                            && !message.deleted
                    })
                {
                    self.state.focus_message_id = action.id;
                    self.state.focus_message_version =
                        self.state.focus_message_version.wrapping_add(1);
                    self.publish();
                    return Ok(());
                }
                if action.kind != "older" {
                    self.cancel_view_requests();
                }
                let api = self.api()?;
                let channel = self.state.selected_channel_id.clone();
                let id = action.id.clone();
                let endpoint = match action.kind.as_str() {
                    "older" => self.older_page_route(&channel),
                    "jump-message" => format!(
                        "channels/{channel}/messages?limit=50&around={}",
                        Self::valid(&id)?
                    ),
                    _ => format!("channels/{channel}/messages?limit=50"),
                };
                action.kind = format!("__{}", action.kind);
                action.channel_id = channel;
                self.state.busy = true;
                self.publish();
                self.launch(action, Some(self.nav), async move {
                    api.get(&endpoint).await.map_err(|e| e.to_string())
                });
            }
            "send" | "send-prepared" => {
                self.require("send")?;
                let api = self.api()?;
                let channel = if action.channel_id.is_empty() {
                    self.state.selected_channel_id.clone()
                } else {
                    action.channel_id.clone()
                };
                if channel != self.state.selected_channel_id {
                    return Err("El contexto del envío cambió; conserva el borrador.".into());
                }
                let extra: Value = serde_json::from_str(&action.extra).unwrap_or(Value::Null);
                let paths = items(&extra["paths"])
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(PathBuf::from)
                    .collect::<Vec<_>>();
                if action.kind == "send-prepared"
                    && (!extra["paths"].is_array()
                        || items(&extra["paths"]).len() != paths.len()
                        || paths.len() > 10)
                {
                    return Err("El borrador preparado no es válido.".into());
                }
                if !paths.is_empty() {
                    self.require("attach")?;
                }
                let text = self.checked_content(&action.text)?;
                if text.encode_utf16().count() > 4000 || text.trim().is_empty() && paths.is_empty()
                {
                    return Err(
                        "Escribe un mensaje de hasta4000caracteres o prepara un archivo.".into(),
                    );
                }
                let reply = if action.kind == "send" {
                    action.extra.clone()
                } else {
                    s(&extra, "replyId")
                };
                if !reply.is_empty() {
                    Self::valid(&reply)?;
                }
                let nonce = if action.kind == "send-prepared" {
                    prepared_nonce(&extra)?
                } else {
                    uuid::Uuid::new_v4().simple().to_string()[..24].to_owned()
                };
                action.kind = "__sent".into();
                action.channel_id = channel.clone();
                let mention_everyone = notdiscord_core::mentions::everyone_allowed(&self.state.selected_guild_id,&self.state.channel_access);
                let upload_limit=self.upload_limit();
                self.launch(action,None,async move{if paths.is_empty(){let mut body=json!({"content":text,"nonce":nonce,"enforce_nonce":true,"allowed_mentions":notdiscord_core::mentions::allowed_mentions(&text,mention_everyone)});if !reply.is_empty(){body["message_reference"]=json!({"message_id":reply,"channel_id":channel,"fail_if_not_exists":true})}api.request(Method::POST,&format!("channels/{channel}/messages"),Some(body)).await.map_err(|e|e.to_string())}else{api.upload_many_with_limits(&channel,&paths,&text,&reply,&nonce,mention_everyone,upload_limit).await.map_err(|e|e.to_string())}});
            }
            "edit" | "delete" => {
                self.require("send")?;
                let channel = self.state.selected_channel_id.clone();
                let id = Self::valid(&action.id)?.to_owned();
                let original = self
                    .cache
                    .get(&channel)
                    .into_iter()
                    .find(|m| m.id == id)
                    .ok_or("Mensaje no disponible.")?;
                if original.author.id != self.me() {
                    return Err("Sólo puedes modificar tus propios mensajes.".into());
                }
                if action.kind == "edit" && original.forwarded {
                    return Err("Los mensajes reenviados no se pueden editar.".into());
                }
                let api = self.api()?;
                let method = if action.kind == "delete" {
                    Method::DELETE
                } else {
                    Method::PATCH
                };
                let body = if action.kind == "edit" {
                    let everyone = notdiscord_core::mentions::everyone_allowed(&self.state.selected_guild_id,&self.state.channel_access);
                    let content=self.checked_content(&action.text)?; Some(json!({"content":content, "allowed_mentions":notdiscord_core::mentions::allowed_mentions(&content,everyone)}))
                } else {
                    None
                };
                action.kind = "__mutation".into();
                self.launch(action, None, async move {
                    api.request(method, &format!("channels/{channel}/messages/{id}"), body)
                        .await
                        .map_err(|e| e.to_string())
                });
            }
            "pin-dm" => {
                self.features.toggle_pin(&action.id)?;
                self.decorate_dms();
                self.publish();
            }
            "guild-emojis" => {
                let request_version = self.next_request("emojis");
                action.extra = request_version.to_string();
                let id = Self::valid(&action.id)?.to_owned();
                if !self.state.guilds.iter().any(|g| g.id == id) {
                    return Err("Servidor no disponible.".into());
                }
                if let Some((at, list)) = self
                    .emoji_cache
                    .get(&id)
                    .filter(|(at, _)| at.elapsed() < Duration::from_secs(300))
                {
                    let _ = at;
                    let mut list=list.clone();
                    for emoji in &mut list {emoji.usable=self.can_send_emoji(emoji);}
                    if id==self.state.selected_guild_id {self.state.emojis=list.clone();self.publish();}
                    self.data("guild-emojis", serde_json::to_value(list).unwrap());
                } else {
                    let api = self.api()?;
                    action.kind = "__emojis".into();
                    action.text=self.next_request(&format!("emoji-catalog/{id}")).to_string();
                    self.launch(action, None, async move {
                        api.get(&format!("guilds/{id}/emojis"))
                            .await
                            .map_err(|e| e.to_string())
                    });
                }
            }
            "gif-catalog" => {
                let request_version = self.next_request("gifs");
                let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
                let mode = s(&extra, "mode");
                let limit = extra["limit"].as_u64().unwrap_or(20).min(96) as usize;
                let endpoint = pickers::endpoint(&mode, &action.text, limit)?;
                if let Some((_, value)) = self
                    .gif_cache
                    .get(&endpoint)
                    .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
                {
                    self.data("gif-catalog", value.clone());
                } else {
                    let api = self.api()?;
                    action.kind = "__gifs".into();
                    action.id = endpoint.clone();
                    action.extra =
                        json!({"mode":mode,"limit":limit,"requestVersion":request_version})
                            .to_string();
                    self.launch(action, None, async move {
                        api.get(&endpoint).await.map_err(|e| e.to_string())
                    });
                }
            }
            "history" => {
                self.require("history")?;
                self.cancel_view_requests();
                self.state.messages = self
                    .features
                    .history(&self.state.selected_channel_id, 100)?
                    .into();
                self.state.view = "history".into();
                self.publish();
            }
            _ => {
                if !self.social_action(action).await? {
                    return Err("Acción no reconocida por el controlador Rust.".into());
                }
            }
        }
        Ok(())
    }
    async fn complete(&mut self, action: UiAction, value: Value) -> Result<(), String> {
        if action.kind == "__reaction-users" { self.complete_reaction_users(value);return Ok(()); }
        // A completed preflight may already be queued when another request
        // stops the session. Invalidate voice/capture attempts before resuming it.
        self.sync_network_safety().await;
        match action.kind.as_str() {
            "__private-ring" => {self.complete_private_ring(&action)?;}
            "__private-ring-stop" => {self.complete_private_ring_stop(&action);}
            "__private-incoming-expired" => {}
            "__guild-threads" => { self.complete_guild_threads(&action, &value); }
            "__guild-settings" => { self.complete_guild_settings(action, value)?; }
            "__member-moderation" => { self.complete_member_moderation(&action, value)?; }
            "__group-member-removed" => { self.complete_group_member_removal(&action, &value); }
            "__notice-image-ready" => self.complete_notice(&action, value)?,
            "__call-rejected" => {
                self.state.status = "Llamada rechazada".into();
                self.publish();
            }
            "__voice-preflight" | "__voice-requested" | "__voice-camera" | "__voice-share"
            | "__voice-data" => self.voice_complete(action, value).await?,
            "__incoming-dm" => {
                self.private_channel_loads.remove(&action.id);
                self.private_channel_failures.remove(&action.id);
                self.recipients(&value);
                let dm = parsing::channel(&value);
                if !matches!(dm.kind.as_str(), "dm" | "group") {
                    return Ok(());
                }
                self.state.direct_messages.retain(|n| n.id != dm.id);
                self.state.direct_messages.push(dm);
                self.decorate_dms();
                self.publish();
            }
            "__notification-channel" => {
                self.complete_notification_channel(&action, &value)?;
            }
            "__credential" => {
                return Err("El inicio de sesión requiere un resultado secreto tipado.".into());
            }
            "__login" => {
                let api = self.api()?;
                api.safety().check().map_err(|error| error.to_string())?;
                if let Some(token) = self.pending_credential.take() {
                    storage::save_session(&self.data, &token)?;
                }
                self.premium = i(&value["user"], "premium_type");
                self.state.upload_limit.premium_type=value["user"]["premium_type"].as_i64()
                    .and_then(|premium|i32::try_from(premium).ok()).filter(|premium|(0..=3).contains(premium));
                let person = parsing::person(&value["user"]);
                self.remember(person.clone());
                self.state.current_user = Some(person);
                self.state.signed_in = true;
                self.state.status.clear();
                self.state.voice_available = MediaBridge::is_available(&self.root);
                self.state.view = "friends".into();
                self.state.channel_name = "Amigos".into();
                self.picker = Some(PickerHistory::new(&self.data, &self.me()));
                let initial_status = self.apply_initial_settings(&value);
                let (g, rx) = api.gateway(initial_status);
                self.gateway = Some(g);
                self.gateway_rx = Some(rx);
                self.sidebar_wait = Some(Instant::now() + Duration::from_secs(3));
                self.publish();
            }
            "__sidebar" => {
                self.sidebar_inflight = false;
                let mut value = value;
                for (kind, delta) in self.sidebar_deltas.drain(..) {
                    request_policy::overlay_sidebar(&mut value, &kind, &delta);
                }
                let mut partial = false;
                for component in ["guilds", "dms", "relationships"] {
                    if !request_policy::component_complete(component, &value[component]) {
                        value[component] = Value::Null;
                        partial = true;
                    }
                }
                self.sidebar_delta_bytes = 0;
                self.sidebar_seed = value.clone();
                if value["guilds"].is_array() {
                    self.state.guilds = self.sidebar_guilds(&value["guilds"]);
                }
                if value["dms"].is_array() && self.state.selected_guild_id.is_empty() {
                    self.channels
                        .retain(|_, channel| !matches!(i(channel, "type"), 1 | 3));
                }
                for dm in items(&value["dms"]).iter().take(2000) {
                    self.recipients(dm);
                    if self.state.selected_guild_id.is_empty() {
                        self.channels.insert(s(dm, "id"), dm.clone());
                    }
                }
                if value["dms"].is_array() {
                    self.state.direct_messages = items(&value["dms"])
                        .iter()
                        .take(2000)
                        .map(parsing::channel)
                        .collect();
                }
                if value["relationships"].is_array() {
                    self.state.relationships = items(&value["relationships"])
                        .iter()
                        .take(3000)
                        .map(parsing::relationship)
                        .collect();
                }
                if partial {
                    self.sidebar_wait = Some(Instant::now() + Duration::from_millis(250));
                }
                for r in self.state.relationships.clone() {
                    self.remember(r.user);
                }
                self.decorate_dms();
                self.publish();
                if self.state.selected_channel_id.is_empty() && self.restore_channel.is_empty() {
                    self.restore_navigation();
                }
            }
            "__guild" => {
                if self.discard_superseded_notification_guild() { return Ok(()); }
                let value=self.guild_load.reconcile(&action.id,value)?;
                self.guild_raw = value["guild"].clone();
                self.self_member = value["member"].clone();
                self.seed_guild_nicknames();
                self.guild_acl = Some(GuildAccess::from_values(
                    &self.guild_raw,
                    &self.self_member,
                    &self.me(),
                ));
                self.channels = items(&value["channels"])
                    .iter()
                    .take(2000)
                    .map(|v| (s(v, "id"), v.clone()))
                    .collect();
                if value["threads"].is_array() {
                    let threads = channel_catalog::thread_response(&action.id, &value, &self.me());
                    self.thread_catalog.replace(&action.id, &threads);
                }
                if let Some(threads) = self.thread_catalog.get(&action.id) {
                    for thread in threads {
                        let id = s(&thread, "id");
                        if self.channels.len() < 2000 || self.channels.contains_key(&id) { self.channels.insert(id, thread); }
                    }
                }
                let explicit = std::mem::take(&mut self.restore_channel);
                self.restore_resolved_notification_channel(&explicit)?;
                self.rebuild_channels();
                self.publish();
                self.subscribe().await;
                let remembered = if explicit.is_empty() {
                    self.last_guild_channels
                        .get(&self.state.selected_guild_id)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    explicit.clone()
                };
                let target = if !explicit.is_empty() {
                    self.state.channels.iter().find(|channel|channel.id==explicit).map(|channel|channel.id.clone())
                } else { preferred_guild_channel(&self.state.channels, &remembered) };
                if let Some(channel) = target {
                    self.select_channel(&channel)?;
                } else if !explicit.is_empty() {
                    return Err("La conversación exacta de la notificación ya no está disponible.".into());
                } else {
                    self.state.channel_name = "No hay canales de texto accesibles".into();
                    self.state.messages.clear();
                    self.state.channel_access = ChannelAccess::default();
                    self.publish();
                }
                self.request_guild_threads(&action.id);
            }
            "__messages" => {
                if action.extra == "restore-private" && b(&value,"privateUnavailable")
                    && self.state.selected_guild_id.is_empty() && self.state.selected_channel_id == action.id
                {
                    self.last_private_channel.clear();
                    self.select_guild("")?;
                    self.save_navigation();
                    return Ok(());
                }
                let mut channel = value["channel"].clone();
                if !self.state.selected_guild_id.is_empty() {
                    let previous_binding=self.channels.get(&action.id).and_then(|channel|
                        self.guild_acl.as_ref().map(|acl|acl.confirmed_member_list_id(channel))).unwrap_or_default();
                    let previous_member_access=self.state.channel_access.can_view_members;
                    // Message loads can omit channel metadata. Keep the
                    // confirmed overwrite set and member-list binding unless
                    // this response explicitly replaces those fields.
                    if let Some(mut previous)=self.channels.get(&action.id).cloned()
                        && let (Some(previous_fields),Some(fields))=(previous.as_object_mut(),channel.as_object()) {
                        if fields.get("permission_overwrites").is_some_and(|value|previous_fields.get("permission_overwrites")!=Some(value))
                            && !fields.contains_key("member_list_id") {previous_fields.remove("member_list_id");}
                        previous_fields.extend(fields.clone());channel=previous;
                    }
                    self.channels.insert(action.id.clone(), channel.clone());
                    self.state.channel_access = self.channel_access(&channel);
                    let binding=self.guild_acl.as_ref().map(|acl|acl.confirmed_member_list_id(&channel)).unwrap_or_default();
                    if binding!=previous_binding || previous_member_access!=self.state.channel_access.can_view_members {
                        self.invalidate_member_context();
                    }
                }
                if !self.state.channel_access.can_read_history {
                    return Err("No tienes permiso para leer el historial.".into());
                }
                self.recipients(&channel);
                for message in items(&value["messages"]) { self.observe_message_nickname(message); }
                let mut messages = items(&value["messages"])
                    .iter()
                    .map(parsing::message)
                    .collect::<Vec<_>>();
                let deltas=self.pending_deltas.drain(..).collect::<Vec<_>>();
                self.pending_delta_bytes=0;
                for (kind, delta) in deltas {
                    self.record_loaded_delta(&messages,&kind,&delta);
                    apply_delta(&mut messages, &kind, &delta);
                }
                self.features.filter_purged(&mut messages);
                self.cache.replace(&action.id, messages);
                self.loading_channel.clear();
                self.refresh_messages();
                self.state.has_older_messages = items(&value["messages"]).len() == 50;
                self.save_navigation();
                // The channel attached to a message read can predate a confirmed membership change.
                if self.state.selected_channel_kind!="group"||self.state.group_members.is_empty() {self.refresh_group(&channel);}
                self.publish();
                self.subscribe().await;
            }
            "__older" | "__present" | "__jump-message" => {
                if action.channel_id!=self.state.selected_channel_id
                    || action.kind == "__present" && self.state.view != "chat" {
                    if action.kind=="__older" {self.finish_history_page(&action);}
                    return Ok(());
                }
                self.state.busy = false;
                for message in items(&value) { self.observe_message_nickname(message); }
                let mut page = items(&value)
                    .iter()
                    .map(parsing::message)
                    .collect::<Vec<_>>();
                self.features.filter_purged(&mut page);
                if action.kind == "__jump-message"
                    && !page
                        .iter()
                        .any(|message| message.id == action.id && !message.deleted)
                {
                    return Err("El mensaje original ya no está disponible.".into());
                }
                if action.kind == "__older" {
                    page.extend(self.cache.get(&action.channel_id).into_iter().take(50));
                }
                self.cache.replace(&action.channel_id, page);
                self.state.view = "chat".into();
                self.refresh_messages();
                self.state.has_older_messages = items(&value).len() == 50;
                if action.kind == "__jump-message" {
                    self.state.focus_message_id = action.id.clone();
                    self.state.focus_message_version += 1;
                    self.save_navigation();
                }
                if action.kind == "__present" {
                    // Coalesce the scroll request with precisely this page.
                    // A separate Data event could be consumed before its snapshot,
                    // scrolling the old history and then restoring its old anchor.
                    self.state.latest_messages_version =
                        self.state.latest_messages_version.wrapping_add(1).max(1);
                    self.state.latest_messages_channel_id = action.channel_id.clone();
                }
                self.publish();
                if action.kind=="__older" {self.finish_history_page(&action);}
            }
            "__pins" => {
                if self.state.view != "pins"
                    || self.state.selected_channel_id != action.id
                    || !self.request_current("pins", action.extra.parse().unwrap_or(0))
                {
                    return Ok(());
                }
                let values = if value.is_array() {
                    &value
                } else {
                    &value["items"]
                };
                let mut pins = items(values)
                    .iter()
                    .take(50)
                    .map(|v| {
                        let data = if v["message"].is_object() {
                            &v["message"]
                        } else {
                            v
                        };
                        let mut message = parsing::message(data);
                        message.pinned = true;
                        (s(v, "pinned_at"), message)
                    })
                    .collect::<Vec<_>>();
                pins.sort_by(|a, b| {
                    b.0.cmp(&a.0)
                        .then(b.1.timestamp.cmp(&a.1.timestamp))
                        .then(b.1.id.len().cmp(&a.1.id.len()))
                        .then(b.1.id.cmp(&a.1.id))
                });
                self.state.pinned_messages = pins.into_iter().map(|(_, m)| m).collect();
                self.features.filter_purged(&mut self.state.pinned_messages);
                self.state.messages = self.state.pinned_messages.clone().into();
                self.state.view = "pins".into();
                self.data("pins-open", json!(true));
                self.publish();
            }
            "__sent" => {
                let message = parsing::message(&value);
                if !parsing::valid_id(&message.id) || message.channel_id != action.channel_id {
                    return Err("No se pudo confirmar el envío. Conserva el borrador.".into());
                }
                self.observe_message_delivery(&message.channel_id, &message.id, true);
                self.observe_message_preview(&message);
                self.cache.upsert(message);
                self.refresh_messages();
                self.decorate_dms();
                if !self.state.selected_guild_id.is_empty() {
                    self.rebuild_channels();
                }
                let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
                self.data("send-ack",json!({"channelId":action.channel_id,"submissionId":extra["submissionId"],"revision":extra["revision"]}));
                self.status("Mensaje enviado");
            }
            "__mutation" => self.status("Operación completada"),
            "__emojis" => {
                if !self.request_current("emojis", action.extra.parse().unwrap_or(0)) {return Ok(());}
                if !self.request_current(&format!("emoji-catalog/{}",action.id),action.text.parse().unwrap_or(0)) {return Ok(());}
                let list=self.store_emoji_catalog(&action.id,items(&value));
                if action.id==self.state.selected_guild_id {self.publish();}
                if self.request_current("emojis", action.extra.parse().unwrap_or(0)) {
                    self.data("guild-emojis", serde_json::to_value(list).unwrap());
                }
            }
            "__gifs" => {
                let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
                if !self.request_current("gifs", extra["requestVersion"].as_u64().unwrap_or(0)) {
                    return Ok(());
                }
                let page = pickers::parse(
                    &value,
                    &s(&extra, "mode"),
                    extra["limit"].as_u64().unwrap_or(20) as usize,
                )?;
                let value = serde_json::to_value(page).map_err(|_| "Catálogo no válido.")?;
                if self.gif_cache.len() >= 4 {
                    if let Some(key) = self.gif_cache.keys().next().cloned() {
                        self.gif_cache.remove(&key);
                    }
                }
                self.gif_cache
                    .insert(action.id, (Instant::now(), value.clone()));
                self.data("gif-catalog", value);
            }
            "__media" => {
                if action.extra.parse::<u64>().ok() == Some(self.voice_lifecycle.media_epoch) {
                    self.media_event(value).await?;
                }
            }
            _ => self.social_complete(action, value).await?,
        }
        Ok(())
    }
    fn rebuild_channels(&mut self) {
        let values = self
            .channels
            .values()
            .filter(|v| self.channel_access(v).can_view)
            .collect::<Vec<_>>();
        self.state.channels = parsing::ordered_guild_channels(values)
            .into_iter()
            .map(|v| {
                let mut n = self
                    .guild_acl
                    .as_ref()
                    .map(|a| a.decorate(v, self.channels.get(&s(v, "parent_id"))))
                    .unwrap_or_else(|| parsing::channel(v));
                n.access = Some(self.channel_access(v));
                n.is_muted = self.muted.get(&n.id).copied().unwrap_or(false);
                let mut parents = Vec::new();
                let mut parent = s(v, "parent_id");
                for _ in 0..16 {
                    if parent.is_empty() || parents.contains(&parent) { break; }
                    parents.push(parent.clone());
                    parent = self.channels.get(&parent).map(|value| s(value, "parent_id")).unwrap_or_default();
                }
                n.notifications_disabled = self.notification_muted(&n.id, &self.state.selected_guild_id)
                    || self.notification_settings.effective_mode(&self.state.selected_guild_id, &n.id,
                        &parents.iter().map(String::as_str).collect::<Vec<_>>()) == Some(2);
                n.mention_count = self.mention.get(&n.id).copied().unwrap_or(0);
                if let Some(last) = self.latest.get(&n.id) {
                    n.last_message_id = last.clone();
                }
                n.unread = n.mention_count > 0
                    || self.read.get(&n.id).is_some_and(|read| {
                        let incoming = self.incoming_message_id(&n.id, &n.last_message_id);
                        incoming.len() > read.len()
                            || incoming.len() == read.len() && incoming > read.as_str()
                    });
                n
            })
            .collect();
        self.state.guild_roles = self
            .guild_acl
            .as_ref()
            .map(|a| a.roles.clone())
            .unwrap_or_default();
        self.refresh_emoji_access();
    }
    fn queue_member_subscription(&mut self) {
        let visible = self.member_demand.as_ref().filter(|(guild, channel)| {
            self.state.view == "chat"
                && self.state.channel_access.can_view_members
                && guild == &self.state.selected_guild_id
                && channel == &self.state.selected_channel_id
        });
        let (guild, channel) = visible.cloned().unwrap_or_default();
        let range = if guild.is_empty() {
            99
        } else {
            self.member_range as i32
        };
        let desired = (guild, channel, range);
        if let Some(g) = &self.gateway {
            request_policy::queue_member_subscription(
                &mut self.member_subscription,
                desired,
                g.is_ready(),
                |command| g.enqueue(command).is_ok(),
            );
        }
    }
    async fn subscribe(&mut self) {
        self.queue_member_subscription();
    }
    fn flush_read_acks(&mut self, now: Instant) {
        let Some(api) = self
            .api
            .clone()
            .filter(|api| api.safety().state() == SafetyState::Allowed)
        else {
            return;
        };
        for (channel, last) in self.read_acks.ready(now) {
            let api = api.clone();
            let mut action = UiAction::id("__marked-read", &channel);
            action.extra = "automatic".into();
            self.launch(action, None, async move {
                api.request(
                    Method::POST,
                    &format!("channels/{channel}/messages/{last}/ack"),
                    Some(json!({"token":null,"manual":false})),
                )
                .await
                .map_err(|error| error.to_string())?;
                Ok(json!({"last":last}))
            });
        }
    }
    async fn tick(&mut self) {
        self.tick_game_activity().await;
        self.expire_outgoing_ring();
        self.expire_private_calls_at(Instant::now()).await;
        self.expire_voice_join().await;
        self.sync_network_safety().await;
        if self.gateway.as_ref().is_some_and(|g| g.is_ready()) {
            self.queue_member_subscription();
        }
        if self.sidebar_wait.is_some_and(|at| Instant::now() >= at) {
            self.sidebar();
        }
        self.flush_read_acks(Instant::now());
        if let Err(error) = self.resume_pending_views().await {
            self.error(error);
        }
        self.flush_snapshot(false);
        let before = self.typing.len();
        self.typing
            .retain(|_, at| at.elapsed() < Duration::from_secs(8));
        if before != self.typing.len() {
            self.refresh_typing();
            self.publish();
        }
        if !self.voice_channel.is_empty() && self.media_preferences.push_to_talk {
            self.poll_ptt(crate::platform::key_down(self.media_preferences.ptt_virtual_key));
        }
    }
}
fn is_network_mutation(kind: &str) -> bool {
    matches!(
        kind,
        "send"
            | "send-prepared"
            | "edit"
            | "delete"
            | "typing"
            | "react"
            | "react-toggle"
            | "pin-message"
            | "unpin-message"
            | "forward"
            | "forward-message"
            | "forward-many"
            | "add-friend"
            | "accept-friend"
            | "remove-friend"
            | "open-dm"
            | "add-group"
            | "group-add"
            | "mark-read"
            | "close-dm"
            | "mute-channel"
            | "mute-dm"
            | "status"
            | "custom-status"
            | "save-profile-bio"
            | "custom-activity"
            | "activity-toggle"
            | "create-invite"
            | "member-moderation"
            | "remove-group-member"
            | "call-dm"
            | "call-dm-video"
            | "voice"
            | "join-call"
            | "reject-call"
            | "view-incoming-media"
            | "watch-stream"
            | "toggle-camera"
            | "start-camera"
            | "share"
            | "start-share"
    )
}

fn preferred_guild_channel(channels: &[NavItem], remembered: &str) -> Option<String> {
    let accessible = |item: &&NavItem| {
        matches!(item.kind.as_str(), "text" | "thread")
            && item
                .access
                .as_ref()
                .is_some_and(|access| access.can_view && access.can_read_history)
    };
    channels
        .iter()
        .filter(accessible)
        .find(|item| item.id == remembered)
        .or_else(|| {
            channels
                .iter()
                .filter(accessible)
                .find(|item| item.kind == "text")
        })
        .map(|item| item.id.clone())
}
fn apply_delta(messages: &mut Vec<ChatMessage>, kind: &str, value: &Value) {
    let id = s(value, "id");
    match kind {
        "MESSAGE_CREATE" => {
            messages.retain(|m| m.id != id);
            messages.push(parsing::message(value));
        }
        "MESSAGE_UPDATE" => {
            if let Some(m) = messages.iter_mut().find(|m| m.id == id) {
                *m = parsing::update_message(m, value);
            }
        }
        "MESSAGE_DELETE" => {
            if let Some(message) = messages.iter_mut().find(|m| m.id == id) {
                message.deleted = true;
                message.pinned = false;
                message.reactions.clear();
            }
        }
        _ => (),
    }
}

fn restore_anchor(extra: &str) -> Option<String> {
    if extra.len() > 8192 {
        return None;
    }
    let extra: Value = serde_json::from_str(extra).ok()?;
    let id = s(&extra, "anchorMessageId");
    (!b(&extra, "atBottom") && parsing::valid_id(&id)).then_some(id)
}
fn message_page_route(channel: &str, anchor: Option<&str>) -> String {
    format!(
        "channels/{channel}/messages?limit=50{}",
        anchor.map(|a| format!("&around={a}")).unwrap_or_default()
    )
}
fn prepared_nonce(extra: &Value) -> Result<String, String> {
    let id = s(extra, "submissionId");
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("El identificador del envío preparado no es válido.".into());
    }
    Ok(id[..24].into())
}
