use super::*;
use std::sync::{Arc, Mutex};
#[path = "private_calls.rs"]
mod private_calls;
struct OpenedMedia {
    media: MediaBridge,
    capabilities: Value,
}
type OpenSlot = Arc<Mutex<Option<Result<OpenedMedia, String>>>>;
#[derive(Default)]
pub(super) struct VoiceLifecycle {
    pub(super) media_epoch: u64,
    opening: Option<OpenSlot>,
    resume: VecDeque<UiAction>,
    request_pending: bool,
    request_channel: String,
    request_guild: String,
    request_instance: u64,
    join_only: bool,
    pending_ring: String,
    pending_ring_attempt: u64,
    share_attempt: u64,
    camera_attempt: u64,
    pending_camera: Option<(String, Instant)>,
    pending_share: Option<(UiAction, Instant)>,
    mic_attempt: u64,
    mic_prior_deaf: Option<bool>,
    mic_active: bool,
    mic_error: Option<String>,
    preview_attempt: u64,
    video_visible: Option<bool>,
    watch_attempt: u64,
    watches: HashMap<String, Value>,
    watch_stops: HashMap<String, WatchStop>,
    // Active-call RTC identity only; credentials are kept solely in an explicit watch.
    stream_identities: HashMap<String, StreamIdentity>,
    early_watch_errors: HashMap<u64, Value>,
    stream_viewers: HashMap<String, HashSet<String>>,
    native_voice: Option<u64>,
    native_share: Option<u64>,
    early_voice: Option<Value>,
    early_share: Option<Value>,
    pub(super) incoming_sequence: u64,
    pending_views: HashMap<String, PendingView>,
    inventory_request: u64,
    inventory_pending: bool,
    // Retain only the active channel's ACL if the user navigates elsewhere.
    active_acl: Option<GuildAccess>,
    active_channel: Value,
    active_acl_scope: Option<VoicePermissionScope>,
    observed_guild_voice: HashMap<(String, String), Value>,
    acl_checked_at: Option<Instant>,
    last_permissions: Option<(bool, bool, bool)>,
    ptt_effective: bool,
    volume_sequence: u64,
    user_volumes_revision: u64,
    stream_volume_revision: u64,
    pending_volumes: HashMap<String,u64>,
    video_sessions: HashMap<u64,u64>,
    switching: Option<VoiceSwitch>,
    leaving: Option<VoiceLeave>,
    join_wait: Option<VoiceJoinWait>,
    receiver: Option<MediaBridge>,
    receiver_opening: Option<OpenSlot>,
    receiver_epoch: u64,
    private_calls: private_calls::Calls,
    #[cfg(test)]
    preflight_fixture: Option<(String,Value)>,
}
impl VoiceLifecycle {
    pub(super) fn needs_fast_maintenance(&self)->bool {
        self.opening.is_some() || self.receiver_opening.is_some() || self.request_pending
            || !self.resume.is_empty() || !self.pending_views.is_empty()
            || self.pending_camera.is_some() || self.pending_share.is_some()
            || self.switching.is_some() || self.leaving.is_some() || self.join_wait.is_some()
            || !self.watch_stops.is_empty() || self.private_calls.needs_fast_maintenance()
    }
}
struct VoiceLeave {
    guild: String,
    requested_at: Instant,
}
struct VoicePermissionScope {account:String,guild:String,channel:String,attempt:u64}
struct VoiceJoinWait {
    phase: VoiceJoinPhase,
    deadline: Instant,
}
#[derive(Clone, Copy)]
enum VoiceJoinPhase { Preflight, MediaHost, Gateway, Transport }
impl VoiceJoinPhase {
    fn timeout_message(self, has_session: bool, has_server: bool) -> &'static str {
        match self {
            Self::Preflight => "No se pudo comprobar el acceso a la llamada a tiempo.",
            Self::MediaHost => "El motor local de voz no terminó de prepararse a tiempo.",
            Self::Gateway if !has_session && !has_server => "Discord no confirmó tu sesión ni asignó un servidor de voz a tiempo.",
            Self::Gateway if !has_session => "Discord asignó un servidor, pero no confirmó tu sesión de voz a tiempo.",
            Self::Gateway => "Discord confirmó tu sesión, pero no asignó un servidor de voz a tiempo.",
            Self::Transport => "El motor local no confirmó la conexión con el servidor de voz a tiempo.",
        }
    }
}
struct VoiceSwitch {
    channel: String,
    video: bool,
    join_only: bool,
    instance: u64,
    deadline: Instant,
}
#[derive(Clone)]
struct PendingView {
    channel: String,
    owner: String,
    kind: String,
    instance: u64,
    attempt: u64,
    selected: Instant,
    requested: bool,
}
#[derive(Clone, Default)]
struct StreamIdentity { server: String, channel: Option<String> }
struct WatchStop { attempt:u64, written:bool, pending:u64 }
impl VoiceLifecycle {
    fn request_current(&self, attempt: u64, current: u64, id: &str) -> bool {
        attempt == current && self.request_pending && self.request_channel == id
    }
    fn can_join(&self, exists: bool) -> bool {
        !self.join_only || exists
    }
    fn take_ring(&mut self, channel: &str, attempt: u64) -> bool {
        if self.pending_ring == channel && self.pending_ring_attempt == attempt {
            self.pending_ring.clear();
            true
        } else {
            false
        }
    }
    fn same_share(&self, meta: &Value, voice: u64) -> bool {
        meta["media"].as_u64() == Some(self.media_epoch)
            && meta["voice"].as_u64() == Some(voice)
            && meta["share"].as_u64() == Some(self.share_attempt)
    }
}
fn wrapped(result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => json!({"ok":true,"result":value}),
        Err(error) => json!({"ok":false,"error":error}),
    }
}
fn unwrapped(value: &Value) -> Result<Value, String> {
    if value["ok"] == true {
        Ok(value["result"].clone())
    } else {
        Err(value["error"]
            .as_str()
            .unwrap_or("No se pudo completar la operación multimedia.")
            .to_owned())
    }
}
impl Controller {
    // Form drafts and delayed settings replies cannot replace independently
    // confirmed participant volumes. The stored map is the single authority.
    fn current_media_preferences(&self, preferences: MediaPreferences) -> MediaPreferences {
        let mut preferences = preferences.normalize();
        preferences.user_volumes = self.media_preferences.user_volumes.clone();
        preferences.stream_volume_percent = self.media_preferences.stream_volume_percent;
        preferences
    }
    fn media_parameters_for(&self, preferences: &MediaPreferences) -> Value {
        let mut settings = preferences.parameters();
        settings["userVolumes"] = json!(self.media_preferences.user_volumes);
        settings["userVolumesRevision"] = json!(self.voice_lifecycle.user_volumes_revision);
        settings["streamVolumePercent"] = json!(self.media_preferences.stream_volume_percent);
        settings["streamVolumeRevision"] = json!(self.voice_lifecycle.stream_volume_revision);
        settings
    }
    fn receiver_parameters(&self)->Value {
        let mut settings=self.media_parameters_for(&self.media_preferences);
        settings["inputId"]=json!("disabled");settings["inputVolume"]=json!(0);
        settings["userVolumes"]=json!({});settings["userVolumesRevision"]=json!(0);
        settings["selfDeaf"]=json!(self.state.self_deaf||self.voice_lifecycle.mic_prior_deaf.is_some());
        settings["videoVisible"]=json!(self.voice_lifecycle.video_visible!=Some(false));
        settings
    }
    fn close_receiver(&mut self) {
        self.voice_lifecycle.receiver_epoch=self.voice_lifecycle.receiver_epoch.wrapping_add(1);
        self.voice_lifecycle.receiver_opening=None;
        self.voice_lifecycle.early_watch_errors.clear();
        if let Some(receiver)=self.voice_lifecycle.receiver.take() {receiver.close();}
    }
    fn receiver_command(&self,id:&str,command:&str,args:Value)->Result<(),String> {
        let receiver=self.voice_lifecycle.receiver.clone().ok_or("El receptor de vídeo no está disponible.")?;
        let mut done=UiAction::id("__voice-data",id);let mut meta=self.meta();meta["receiver"]=json!(self.voice_lifecycle.receiver_epoch);
        if id=="stream-volume" {meta["streamVolumeRevision"]=args["revision"].clone();}
        done.extra=meta.to_string();
        if matches!(id,"video-stream-visible"|"video-stream-hidden")
            && let Some(video)=args["id"].as_u64().and_then(|id|self.state.call_videos.iter().find(|video|video.id==id))
            && let Some(watch)=self.voice_lifecycle.watches.get(&video.stream_key) {
            done.channel_id=video.stream_key.clone();done.text=watch["attempt"].as_u64().unwrap_or(0).to_string();
        }
        let command=command.to_owned();self.launch(done,None,async move {Ok(wrapped(receiver.command(&command,args).await))});Ok(())
    }
    fn sync_receiver_settings(&self) {
        if self.voice_lifecycle.receiver.is_some() {let _=self.receiver_command("receiver-settings","configureReceiver",self.receiver_parameters());}
    }
    fn video_command(&self,id:&str,command:&str,args:Value)->Result<(),String> {
        if args["id"].as_u64().is_some_and(|id|self.voice_lifecycle.receiver.as_ref().is_some_and(|receiver|receiver.owns_video_id(id))) {
            self.receiver_command(id,command,args)
        } else {self.launch_media("__voice-data",id,command,args)}
    }
    async fn probe_receiver(&mut self)->Result<(),String> {
        if self.voice_lifecycle.receiver.is_some()||self.voice_lifecycle.receiver_opening.is_some() {return Ok(());}
        let frames=self.media.as_ref().ok_or("La llamada de voz terminó.")?.video_frames();
        self.voice_lifecycle.receiver_epoch=self.voice_lifecycle.receiver_epoch.wrapping_add(1);
        let epoch=self.voice_lifecycle.receiver_epoch;let media_epoch=self.voice_lifecycle.media_epoch;
        let tx=self.completion.clone();let generation=self.generation;let (events,mut rx)=mpsc::unbounded_channel();
        tokio::spawn(async move {while let Some(value)=rx.recv().await {let mut action=UiAction::new("__media");action.extra=media_epoch.to_string();let _=tx.send(Completed {generation,nav:None,action,result:Ok(json!({"event":"receiver-host","epoch":epoch,"value":value})),credential:None});}});
        let slot:OpenSlot=Arc::new(Mutex::new(None));self.voice_lifecycle.receiver_opening=Some(slot.clone());
        let root=self.root.clone();let data=self.data.join("receive-host");let ctx=self.ctx.clone();let settings=self.receiver_parameters();
        let mut action=UiAction::id("__voice-data","receiver-opened");let mut meta=self.meta();meta["receiver"]=json!(epoch);action.extra=meta.to_string();
        self.launch(action,None,async move {
            let result=async {let media=MediaBridge::start_receiver(&root,&data,events,ctx,frames).await?;
                let capabilities=match media.command("capabilities",json!({})).await {Ok(value)=>value,Err(error)=>{media.close();return Err(error)}};
                if !b(&capabilities,"screenReceive")||i(&capabilities,"dave")<1 {media.close();return Err("El receptor no admite pantallas cifradas.".into());}
                if let Err(error)=media.command("configureReceiver",settings).await {media.close();return Err(error);}
                Ok(OpenedMedia {media,capabilities})}.await;
            *slot.lock().unwrap()=Some(result);Ok(json!({"opened":true}))
        });Ok(())
    }
    async fn receiver_failed(&mut self,message:&str)->Result<(),String> {
        self.close_receiver();
        let keys=self.voice_lifecycle.watches.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            let phase=self.voice_lifecycle.watches.get(&key).and_then(|watch|watch["phase"].as_str()).unwrap_or("");
            let label=match phase {
                "receiver"=>"inicio del receptor",
                "stream-identity"|"stream-server"=>"datos de la transmisión",
                "session-description"|"waiting-peer"=>"cifrado de la pantalla",
                phase if phase.starts_with("mls-")=>"cifrado DAVE",
                "native-source"=>"fuente de vídeo registrada",
                "first-frame"=>"espera del primer fotograma del motor nativo",
                "frames-received"=>"fotogramas recibidos del motor nativo",
                "video-subscription-requested"=>"recepción del vídeo solicitado",
                "connected"=>"espera de la fuente de vídeo",
                _=>"conexión del receptor",
            };
            let error=format!("{message} Última etapa confirmada: {label}.");
            self.fail_watch(&key,&error).await?;
        }
        self.publish();Ok(())
    }
    fn receiver_failure_message(event:&Value)->String {
        let code=s(&event["error"],"code");
        let detail=match code.as_str() {
            "HOST_PROCESS_EXIT"=>event["error"]["exitCode"].as_i64().map(|code|format!("El proceso receptor terminó con código 0x{:08X}.",code as u32)).unwrap_or_else(||"El proceso receptor terminó de forma inesperada.".into()),
            "HOST_CONTROL_TIMEOUT"=>"El receptor dejó de confirmar las órdenes de control.".into(),
            "HOST_CONTROL_WRITE_FAILED"=>"Se perdió la escritura al canal de control del receptor.".into(),
            "HOST_CONTROL_READ_FAILED"=>format!("No se pudo leer el control del receptor: {}",parsing::limited(event["error"]["message"].as_str().unwrap_or("respuesta no válida"),180)),
            "HOST_CONTROL_CLOSED"=>"El receptor cerró inesperadamente su canal de control.".into(),
            "VIDEO_PIPE_FAILED"=>format!("Falló el canal de fotogramas: {}",parsing::limited(event["message"].as_str().or_else(||event["error"]["message"].as_str()).unwrap_or("conexión de vídeo cerrada"),180)),
            _=>"El receptor de pantalla se interrumpió.".into(),
        };
        format!("{detail} El audio de la llamada continúa; vuelve a abrir la pantalla para reintentar.")
    }
    pub(super) async fn probe_media(&mut self) {
        if self.media.is_some() || self.voice_lifecycle.opening.is_some() {
            return;
        }
        self.voice_lifecycle.media_epoch += 1;
        let epoch = self.voice_lifecycle.media_epoch;
        let tx = self.completion.clone();
        let generation = self.generation;
        let (events, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(value) = rx.recv().await {
                let mut action = UiAction::new("__media");
                action.extra = epoch.to_string();
                let _ = tx.send(Completed {
                    generation,
                    nav: None,
                    action,
                    result: Ok(value),
                    credential: None,
                });
            }
        });
        let slot: OpenSlot = Arc::new(Mutex::new(None));
        self.voice_lifecycle.opening = Some(slot.clone());
        let root = self.root.clone();
        let data = self.data.clone();
        let settings = self.media_parameters_for(&self.media_preferences);
        let ctx = self.ctx.clone();
        let mut action = UiAction::id("__voice-data", "opened");
        action.extra = epoch.to_string();
        self.launch(action, None, async move {
            let result = async {
                let media = MediaBridge::start(&root, &data, events, ctx).await?;
                let capabilities = media.command("capabilities", json!({})).await?;
                if !b(&capabilities, "voice")
                    || !b(&capabilities, "electronHost")
                    || i(&capabilities, "dave") < 1
                {
                    media.close();
                    return Err("El motor no admite voz cifrada en este host.".into());
                }
                media.command("configureAudio", settings).await?;
                Ok(OpenedMedia {
                    media,
                    capabilities,
                })
            }
            .await;
            *slot.lock().unwrap() = Some(result);
            Ok(json!({"opened":true}))
        });
    }
    async fn queue_media(&mut self, action: UiAction) -> Result<(), String> {
        if self.voice_lifecycle.resume.len() >= 8 {
            return Err("El servicio multimedia todavía se está preparando.".into());
        }
        self.voice_lifecycle.resume.push_back(action);
        self.probe_media().await;
        Ok(())
    }
    pub(super) async fn join_voice(&mut self, id: &str, video: bool) -> Result<(), String> {
        self.switch_voice(id, video, false, 0).await
    }
    pub(super) fn report_voice_error(&mut self, message: &str) {
        self.state.voice_error=parsing::limited(message,512);
        self.status(message);
    }
    pub(super) fn expire_outgoing_ring(&mut self) {
        if self.voice_lifecycle.switching.as_ref().is_some_and(|next|Instant::now()>=next.deadline) {
            self.voice_lifecycle.switching=None;
            self.state.voice_connecting=false;
            self.state.call_channel_name.clear();
            self.state.call_status_label.clear();
            self.refresh_call();
            self.report_voice_error("No se confirmó la salida de la llamada anterior. El audio local está cerrado; vuelve a seleccionar el destino para reintentar.");
        }
    }
    pub(super) async fn expire_voice_join(&mut self) {
        if self.voice_lifecycle.join_wait.as_ref().is_some_and(|wait| Instant::now() >= wait.deadline) {
            let wait = self.voice_lifecycle.join_wait.take().unwrap();
            let message = wait.phase.timeout_message(!self.voice_session.is_empty(), !self.voice_endpoint.is_empty() && !self.voice_token.is_empty());
            self.stop_voice().await;
            self.report_voice_error(&format!("{message} Se cerró el audio local. Puedes volver a unirte para reintentar."));
        }
    }
    fn wait_voice_join(&mut self, phase: VoiceJoinPhase, seconds: u64) {
        self.voice_lifecycle.join_wait = Some(VoiceJoinWait { phase, deadline: Instant::now() + Duration::from_secs(seconds) });
    }
    fn voice_permissions(&self) -> (bool, bool, bool) {
        if self.voice_guild.is_empty() { return (true, true, true); }
        let current_catalog=self.state.selected_guild_id==self.voice_guild
            && !self.guild_load.is_loading(&self.voice_guild) && self.guild_acl.is_some();
        let retained_current=self.voice_lifecycle.active_acl_scope.as_ref().is_some_and(|scope|
            scope.account==self.state.current_user.as_ref().map(|user|user.id.as_str()).unwrap_or("")
                &&scope.guild==self.voice_guild&&scope.channel==self.voice_channel&&scope.attempt==self.voice_attempt);
        let (acl, raw) = if current_catalog {
            (self.guild_acl.as_ref(), self.channels.get(&self.voice_channel))
        } else if retained_current {
            (self.voice_lifecycle.active_acl.as_ref(), Some(&self.voice_lifecycle.active_channel))
        } else {return (false,false,false);};
        let Some((acl, raw)) = acl.zip(raw) else { return (false, false, false); };
        if s(raw,"id")!=self.voice_channel||!matches!(i(raw,"type"),2|13)
            ||!s(raw,"guild_id").is_empty()&&s(raw,"guild_id")!=self.voice_guild {return (false,false,false);}
        let bits = acl.channel_permissions(raw);
        let connect = acl.evaluate(raw, None).can_view && bits & (1 << 20) != 0;
        (connect, connect && bits & (1 << 21) != 0, connect && bits & (1 << 9) != 0)
    }
    pub(super) fn retain_active_voice_permissions(&mut self) {
        if self.voice_channel.is_empty()||self.voice_guild.is_empty()||self.state.selected_guild_id!=self.voice_guild
            ||self.guild_load.is_loading(&self.voice_guild) {return;}
        let Some(acl)=self.guild_acl.as_ref() else {return;};
        // Navigation clears the selected catalog while its replacement loads.
        // That absence is not a revocation of the already confirmed call ACL.
        self.voice_lifecycle.active_acl=Some(acl.clone());
        self.voice_lifecycle.active_channel=self.channels.get(&self.voice_channel).cloned().unwrap_or(Value::Null);
        self.voice_lifecycle.active_acl_scope=Some(VoicePermissionScope {
            account:self.me(),guild:self.voice_guild.clone(),channel:self.voice_channel.clone(),attempt:self.voice_attempt,
        });
    }
    fn effective_voice_mute(&self) -> bool {
        self.state.self_mute || self.state.self_deaf || !self.voice_permissions().1
    }
    fn require_voice_video(&self) -> Result<(), String> {
        if self.voice_permissions().2 { Ok(()) } else {
            Err("No tienes permiso para usar cámara o compartir pantalla en este canal.".into())
        }
    }
    fn remember_guild_voice(&mut self, value: &Value) {
        self.remember_guild_voice_state(value);
        let guild = s(value, "guild_id");
        let user = s(value, "user_id");
        if !parsing::valid_id(&guild) || !parsing::valid_id(&user) { return; }
        let key = (guild, user);
        // Bounded, session-only presence. Store no transport credentials.
        if self.voice_lifecycle.observed_guild_voice.len() >= 1024
            && !self.voice_lifecycle.observed_guild_voice.contains_key(&key) { return; }
        let mut compact = self.voice_lifecycle.observed_guild_voice.get(&key).cloned().unwrap_or_else(||json!({"user_id":value["user_id"]}));
        if value.get("channel_id").is_some_and(Value::is_null) { compact["channel_id"]=Value::Null; }
        else if parsing::valid_id(&s(value,"channel_id")) { compact["channel_id"]=json!(s(value,"channel_id")); }
        for field in ["self_mute","self_deaf","mute","deaf","self_video","self_stream"] {
            if let Some(value)=value.get(field).and_then(Value::as_bool) { compact[field]=json!(value); }
        }
        let user=if value["member"]["user"].is_object(){&value["member"]["user"]}else{&value["user"]};
        if user.is_object() {
            compact["user"]=json!({"id":s(value,"user_id"),"username":parsing::limited(&s(user,"username"),128),"global_name":parsing::limited(&s(user,"global_name"),128),"avatar":parsing::limited(&s(user,"avatar"),128),"bot":b(user,"bot")});
        }
        self.voice_lifecycle.observed_guild_voice.insert(key, compact);
    }
    pub(super) fn observe_guild_voice_metadata(&mut self, value: &Value) {
        let mut snapshots = items(&value["guild_voice_states"]).to_vec();
        if value["voice_states"].is_array() && parsing::valid_id(&s(value,"id")) {
            let source = items(&value["voice_states"]);
            snapshots.push(json!({"guild_id":s(value,"id"),"partial":source.len()>1024,"states":source.iter().take(1024).cloned().collect::<Vec<_>>()}));
        }
        for snapshot in snapshots.iter().take(500) {
            let guild = s(snapshot,"guild_id");
            if !parsing::valid_id(&guild) { continue; }
            if snapshot["partial"] == false {
                self.voice_lifecycle.observed_guild_voice.retain(|(known,_),_|known!=&guild);
                self.reset_guild_voice_snapshot(&guild);
            }
            for state in items(&snapshot["states"]).iter().take(1024) {
                let mut state=state.clone(); state["guild_id"]=json!(guild);
                self.remember_guild_voice(&state);
            }
        }
    }
    fn seed_guild_call_people(&mut self) {
        if self.voice_guild.is_empty() { return; }
        let mut values: HashMap<String, Value> = items(&self.guild_raw["voice_states"])
            .iter().take(512).map(|value| (s(value, "user_id"), value.clone())).collect();
        for ((guild, user), value) in &self.voice_lifecycle.observed_guild_voice {
            if guild == &self.voice_guild { values.insert(user.clone(), value.clone()); }
        }
        for value in values.values() {
            // Joining adds the local participant immediately afterwards.
            if self.call_people.len() >= 255 { break; }
            if s(value, "channel_id") == self.voice_channel {
                if let Some(person) = self.incoming_participant(value, None) {
                    self.call_people.insert(person.user.id.clone(), person);
                }
            }
        }
    }
    fn voice_request_item(&self, id: &str, join_only: bool) -> Option<(NavItem, bool)> {
        if !parsing::valid_id(id) {
            return None;
        }
        if let Some(item) = self
            .state
            .channels
            .iter()
            .chain(&self.state.direct_messages)
            .find(|item| item.id == id)
        {
            return Some((
                item.clone(),
                self.state.direct_messages.iter().any(|item| item.id == id),
            ));
        }
        let call = self.state.incoming_calls.iter().find(|call| {
            join_only && call.channel_id == id && matches!(call.kind.as_str(), "dm" | "group")
        })?;
        Some((
            NavItem {
                id: id.into(),
                name: call.channel_name.clone(),
                kind: call.kind.clone(),
                icon_url: call.icon_url.clone(),
                ..Default::default()
            },
            true,
        ))
    }
    #[cfg(test)]
    fn request_voice(&mut self, id: &str, video: bool, join_only: bool) -> Result<(), String> {
        self.request_voice_scoped(id, video, join_only, 0)
    }
    fn incoming_join_current(&self) -> bool {
        !self.voice_lifecycle.join_only
            || self.state.incoming_calls.iter().any(|call| {
                call.channel_id == self.voice_lifecycle.request_channel
                    && (self.voice_lifecycle.request_instance == 0
                        || call.instance_id == self.voice_lifecycle.request_instance)
            })
    }
    fn checked_voice_destination(&self,id:&str,video:bool,join_only:bool,instance:u64)->Result<(NavItem,bool),String> {
        if self.voice_lifecycle.mic_prior_deaf.is_some() {
            return Err("Detén la prueba de micrófono antes de iniciar otra llamada.".into());
        }
        Self::valid(id)?;
        if instance>0&&!self.state.incoming_calls.iter().any(|call|call.channel_id==id&&call.instance_id==instance) {
            return Err("La llamada entrante cambió o terminó.".into());
        }
        if join_only&&!self.state.incoming_calls.iter().any(|call|call.channel_id==id) {
            return Err("La llamada ya terminó. No se inició una llamada nueva.".into());
        }
        self.gateway.as_ref().ok_or("La conexión aún no está preparada.")?;
        self.api()?;
        let (item,is_dm)=self.voice_request_item(id,join_only).ok_or("Canal no disponible.")?;
        if item.kind=="stage" {return Err("Los escenarios siguen pendientes en la versión de referencia.".into());}
        if !is_dm&&item.kind!="voice" {return Err("Selecciona un canal de voz.".into());}
        if !is_dm {
            if !self.channels.get(id).is_some_and(|raw|self.channel_access(raw).can_view&&self.guild_acl.as_ref().is_some_and(|acl|acl.channel_permissions(raw)&(1<<20)!=0)) {
                return Err("No tienes permiso para conectar a este canal.".into());
            }
            if video&&self.channels.get(id).is_none_or(|raw|self.guild_acl.as_ref().is_none_or(|acl|acl.channel_permissions(raw)&(1<<9)==0)) {
                return Err("No tienes permiso para usar vídeo en este canal.".into());
            }
        }
        Ok((item,is_dm))
    }
    async fn switch_voice(&mut self,id:&str,video:bool,join_only:bool,instance:u64)->Result<(),String> {
        // Validate before leaving a working call. No destination acquires audio
        // until the Gateway confirms the old membership has been removed.
        let (item,is_dm)=self.checked_voice_destination(id,video,join_only,instance)?;
        // Explicit acceptance owns the invitation before any asynchronous
        // teardown/preflight. Its ten-second no-answer task must not race a
        // user who is switching from an existing call.
        if join_only && is_dm { self.end_incoming_invitation(id,instance,true); }
        if self.voice_channel==id {
            if video {self.data("camera-request",json!(true));}
            return Ok(());
        }
        if let Some(next)=self.voice_lifecycle.switching.as_mut() {
            if next.channel!=id {self.voice_lifecycle.pending_views.clear();}
            next.channel=id.into();next.video=video;next.join_only=join_only;next.instance=instance;
            self.state.call_channel_name=item.name;
            self.refresh_call();
            return Ok(());
        }
        if let Some(previous)=&self.voice_lifecycle.leaving {
            // A manual Leave followed by Join has the same ordering hazard as
            // a direct switch. Never let an old channel:null cancel a new join.
            let previous_guild=previous.guild.clone();
            let mut requested_at=previous.requested_at;
            if requested_at.elapsed() >= Duration::from_secs(8) {
                self.gateway.as_ref().ok_or("Gateway no disponible.")?.send(GatewayCommand::VoiceState {
                    guild_id: (!previous_guild.is_empty()).then(||previous_guild.clone()),
                    channel_id: None,
                    mute: self.state.self_mute || self.state.self_deaf,
                    deaf: self.state.self_deaf,
                    video: false,
                }).await.map_err(|error|error.to_string())?;
                requested_at=Instant::now();
                self.voice_lifecycle.leaving.as_mut().unwrap().requested_at=requested_at;
            }
            self.voice_lifecycle.switching=Some(VoiceSwitch {channel:id.into(),video,join_only,instance,deadline:requested_at+Duration::from_secs(8)});
            self.state.voice_connecting=true;
            self.state.call_channel_name=item.name;
            self.state.call_status_label="Esperando confirmación de salida de la llamada anterior…".into();
            self.refresh_call();
            return Ok(());
        }
        if self.voice_channel.is_empty() {
            if self.voice_lifecycle.request_pending {
                if self.voice_lifecycle.request_channel==id {return Ok(());}
                self.stop_voice_internal(false).await;
            }
            return self.request_voice_scoped(id,video,join_only,instance);
        }
        if let Some(media)=self.media.clone() {
            // Prefer a native teardown acknowledgement before another host can
            // open the microphone. A wedged helper is still force-closed below.
            let _=tokio::time::timeout(Duration::from_secs(2),media.command("disconnect",json!({}))).await;
        }
        self.stop_voice().await;
        self.voice_lifecycle.switching=Some(VoiceSwitch {channel:id.into(),video,join_only,instance,deadline:Instant::now()+Duration::from_secs(8)});
        self.state.voice_connecting=true;
        self.state.call_channel_name=item.name;
        self.state.call_status_label="Cerrando la llamada anterior…".into();
        self.refresh_call();
        Ok(())
    }
    fn request_voice_scoped(
        &mut self,
        id: &str,
        video: bool,
        join_only: bool,
        instance: u64,
    ) -> Result<(), String> {
        if self.voice_lifecycle.mic_prior_deaf.is_some() {
            return Err("Detén la prueba de micrófono antes de iniciar otra llamada.".into());
        }
        Self::valid(id)?;
        if instance > 0
            && !self
                .state
                .incoming_calls
                .iter()
                .any(|call| call.channel_id == id && call.instance_id == instance)
        {
            return Err("La llamada entrante cambió o terminó.".into());
        }
        if join_only && !self.state.incoming_calls.iter().any(|c| c.channel_id == id) {
            return Err("La llamada ya terminó. No se inició una llamada nueva.".into());
        }
        if !self.voice_channel.is_empty() {
            if self.voice_channel == id && video {
                self.data("camera-request", json!(true));
            }
            return Err("Ya hay una llamada abierta.".into());
        }
        if self.voice_lifecycle.request_pending {
            return Err("La llamada todavía se está preparando…".into());
        }
        let _ = self
            .gateway
            .as_ref()
            .ok_or("La conexión aún no está preparada.")?;
        let api = self.api()?;
        let (item, is_dm) = self
            .voice_request_item(id, join_only)
            .ok_or("Canal no disponible.")?;
        if item.kind == "stage" {
            return Err("Los escenarios siguen pendientes en la versión de referencia.".into());
        }
        if !is_dm && item.kind != "voice" {
            return Err("Selecciona un canal de voz.".into());
        }
        #[cfg(test)]
        if is_dm&&self.voice_lifecycle.preflight_fixture.as_ref().is_none_or(|(channel,_)|channel!=id) {
            return Err("La prueba privada requiere un preflight sintético explícito para este canal; no se permite HTTP real.".into());
        }
        if !is_dm {
            let permitted = self.channels.get(id).is_some_and(|v| {
                self.channel_access(v).can_view
                    && self
                        .guild_acl
                        .as_ref()
                        .is_some_and(|a| a.channel_permissions(v) & (1u64 << 20) != 0)
            });
            if !permitted {
                return Err("No tienes permiso para conectar a este canal.".into());
            }
            if video && self.channels.get(id).is_none_or(|v| {
                self.guild_acl.as_ref().is_none_or(|acl| acl.channel_permissions(v) & (1 << 9) == 0)
            }) { return Err("No tienes permiso para usar vídeo en este canal.".into()); }
        }
        self.voice_attempt += 1;
        self.state.voice_error.clear();
        self.wait_voice_join(VoiceJoinPhase::Preflight, 20);
        self.voice_lifecycle.request_pending = true;
        self.voice_lifecycle.request_channel = id.into();
        self.voice_lifecycle.request_guild = if is_dm {String::new()} else {self.state.selected_guild_id.clone()};
        self.voice_lifecycle.join_only = join_only;
        self.voice_lifecycle.request_instance = if join_only {
            self.state
                .incoming_calls
                .iter()
                .find(|call| call.channel_id == id)
                .map(|call| call.instance_id)
                .unwrap_or(instance)
        } else {
            0
        };
        if is_dm&&join_only {self.end_incoming_invitation(id,self.voice_lifecycle.request_instance,true);}
        self.pending_video = video;
        self.state.voice_connecting = true;
        self.state.call_channel_name = item.name.clone();
        self.state.call_status_label = "Solicitando acceso a la llamada…".into();
        self.refresh_call();
        let mut action = UiAction::id("__voice-preflight", id);
        action.extra = self.voice_attempt.to_string();
        action.text=json!({"isDm":is_dm,"guild":if is_dm{String::new()}else{self.state.selected_guild_id.clone()},"name":item.name,"joinOnly":join_only}).to_string();
        let channel = id.to_owned();
        #[cfg(test)]
        if let Some((fixture_channel,result))=self.voice_lifecycle.preflight_fixture.take() {
            assert_eq!(fixture_channel,channel,"Synthetic preflight belongs to another destination");
            self.launch(action,None,async move {Ok(wrapped(Ok(result)))});
            return Ok(());
        }
        self.launch(action, None, async move {
            Ok(wrapped(
                async {
                    let mut call=if is_dm && !join_only {
                        api.get(&format!("channels/{channel}/call"))
                            .await
                            .map_err(|e| e.to_string())?
                    } else {
                        json!({"ringable":false})
                    };
                    if is_dm {
                        let metadata=api.get(&format!("channels/{channel}")).await.map_err(|error|error.to_string())?;
                        if s(&metadata,"id")!=channel||!matches!(i(&metadata,"type"),1|3){return Err("La conversación de la llamada privada cambió.".into());}
                        call["recipients"]=metadata["recipients"].clone();
                    }
                    Ok(call)
                }
                .await,
            ))
        });
        Ok(())
    }
    fn meta(&self) -> Value {
        json!({"media":self.voice_lifecycle.media_epoch,"voice":self.voice_attempt,"share":self.voice_lifecycle.share_attempt,"camera":self.voice_lifecycle.camera_attempt,"mic":self.voice_lifecycle.mic_attempt,"preview":self.voice_lifecycle.preview_attempt,"watch":self.voice_lifecycle.watch_attempt})
    }
    fn launch_media(&self, kind: &str, id: &str, command: &str, args: Value) -> Result<(), String> {
        let media = self
            .media
            .clone()
            .ok_or("El host multimedia no está disponible.")?;
        let command = command.to_owned();
        let mut action = UiAction::id(kind, id);
        action.extra = self.meta().to_string();
        self.launch(action, None, async move {
            Ok(wrapped(media.command(&command, args).await))
        });
        Ok(())
    }
    pub(super) fn poll_ptt(&mut self, pressed: bool) {
        let active = self.state.voice_connected && pressed;
        if active != self.last_ptt {
            self.last_ptt = active;
            self.update_ptt(active);
        }
    }
    pub(super) fn update_ptt(&mut self, active: bool) {
        let active = active && self.voice_permissions().1 && !self.state.self_mute && !self.state.self_deaf;
        if active == self.voice_lifecycle.ptt_effective { return; }
        self.voice_lifecycle.ptt_effective = active;
        let _ = self.launch_media("__voice-data", "ptt", "ptt", json!({"active":active}));
        if self.state.voice_connected && self.voice_permissions().1 && !self.state.self_mute && !self.state.self_deaf {
            self.data("sound", json!({"event":if active {"ptt_activate"} else {"ptt_deactivate"}}));
        }
    }
    pub(super) async fn voice_failed(&mut self, action: &UiAction) {
        if action.kind == "__voice-preflight"
            && action.extra.parse::<u64>().ok() == Some(self.voice_attempt)
        {
            self.voice_lifecycle.request_pending = false;
            self.voice_lifecycle.join_wait = None;
            self.voice_lifecycle.pending_views.clear();
            self.voice_lifecycle.request_channel.clear();
            self.state.voice_connecting = false;
            self.pending_video = false;
            self.refresh_call();
        } else {
            let meta: Value = serde_json::from_str(&action.extra).unwrap_or_default();
            if meta["media"].as_u64() == Some(self.voice_lifecycle.media_epoch)
                && meta["voice"].as_u64() == Some(self.voice_attempt)
            {
                if action.kind == "__voice-data" && matches!(action.id.as_str(), "permission-mute" | "ptt") {
                    // A lost release/mute confirmation must never leave a
                    // previously active input transmitting indefinitely.
                    self.stop_voice().await;
                    return;
                }
                if action.kind=="__voice-data" && matches!(action.id.as_str(),"watch-stop"|"video-stream-hidden"|"video-stream-visible"|"stream-volume") {
                    // Receiver cleanup/decoder errors cannot be classified as
                    // failure to release a microphone. Preserve the voice session.
                    self.publish();
                    return;
                }
                if action.kind == "__voice-data"
                    && matches!(action.id.as_str(), "mic-start" | "mic-stop")
                {
                    if meta["mic"].as_u64() == Some(self.voice_lifecycle.mic_attempt) {
                        // A local loopback failure must not leave the call. Only
                        // restore its original state after native cleanup succeeds.
                        let cleaned=match self.media.clone() {
                            Some(media)=>media.command("stopMicTest",json!({"token":self.voice_lifecycle.mic_attempt})).await.is_ok(),
                            None=>false,
                        };
                        if cleaned {let _=self.finish_mic_test().await;}
                        else {self.hold_failed_mic_test("No se pudo cerrar la prueba local. Sigues conectado con el audio silenciado; pulsa Detener para reintentar.");}
                    }
                    return;
                }
                if action.kind == "__voice-data" && action.id == "watch-start" {
                    if self
                        .voice_lifecycle
                        .watches
                        .get(&action.channel_id)
                        .is_some_and(|watch| {
                            watch["attempt"].as_u64() == action.text.parse::<u64>().ok()
                        })
                    {
                        let _ = self
                            .fail_watch(
                                &action.channel_id,
                                "No se pudo abrir esta transmisión. Puedes volver a intentarlo.",
                            )
                            .await;
                    }
                    return;
                }
                if action.kind == "__voice-data"
                    && action.id == "preview-start"
                    && meta["preview"].as_u64() == Some(self.voice_lifecycle.preview_attempt)
                {
                    for video in self
                        .state
                        .call_videos
                        .iter_mut()
                        .filter(|video| video.preview)
                    {
                        video.active = false;
                        video.status = "error".into();
                        video.error = "No se pudo cargar esta vista previa.".into();
                    }
                    self.publish();
                    return;
                }
                if action.kind == "__voice-share" {
                    if self.voice_lifecycle.same_share(&meta, self.voice_attempt) {
                        let _ = self.stop_share().await;
                    }
                } else if action.kind == "__voice-data"
                    && action.id == "settings"
                    && !self.voice_channel.is_empty()
                {
                    self.state.self_mute = true;
                    let _ = storage::save_json(
                        &self.data.join("audio-settings.json"),
                        &json!({"mute":true,"deaf":self.state.self_deaf}),
                    );
                    self.stop_voice().await;
                } else if action.kind == "__voice-camera" {
                    if meta["camera"].as_u64() == Some(self.voice_lifecycle.camera_attempt) {
                        self.state.camera_preparing = false;
                    }
                } else if action.kind == "__voice-requested" {
                    self.stop_voice().await;
                }
            }
        }
    }
    pub(super) async fn voice_complete(
        &mut self,
        action: UiAction,
        value: Value,
    ) -> Result<(), String> {
        if action.kind=="__voice-data" && action.id=="receiver-opened" {
            let meta:Value=serde_json::from_str(&action.extra).unwrap_or_default();
            if meta["media"].as_u64()!=Some(self.voice_lifecycle.media_epoch)||meta["voice"].as_u64()!=Some(self.voice_attempt)||meta["receiver"].as_u64()!=Some(self.voice_lifecycle.receiver_epoch) {return Ok(());}
            let opened=self.voice_lifecycle.receiver_opening.take().and_then(|slot|slot.lock().unwrap().take());
            match opened {
                Some(Ok(opened))=>{
                    if let Err(error)=opened.media.command("setVideoVisible",json!({"visible":self.voice_lifecycle.video_visible!=Some(false)})).await {opened.media.close();self.receiver_failed(&error).await?;return Ok(());}
                    // Opening is asynchronous: a global stream volume change
                    // may have arrived after its initial settings were captured.
                    if let Err(error)=opened.media.command("configureReceiver",self.receiver_parameters()).await {opened.media.close();self.receiver_failed(&error).await?;return Ok(());}
                    self.voice_lifecycle.receiver=Some(opened.media);let keys=self.voice_lifecycle.watches.keys().cloned().collect::<Vec<_>>();for key in keys {self.try_watch(&key).await?;}
                }
                Some(Err(error))=>self.receiver_failed(&error).await?,
                None=>self.receiver_failed("El receptor de pantalla no confirmó su inicio. Puedes volver a abrir la pantalla.").await?,
            }return Ok(());
        }
        if action.kind == "__voice-data" && action.id == "opened" {
            if action.extra.parse::<u64>().ok() != Some(self.voice_lifecycle.media_epoch) {
                return Ok(());
            }
            let opened = self
                .voice_lifecycle
                .opening
                .take()
                .and_then(|slot| slot.lock().unwrap().take())
                .ok_or("El servicio multimedia no confirmó su inicio.")?;
            let opened=match opened {
                Ok(opened)=>match opened.media.command("setVideoVisible",json!({"visible":self.voice_lifecycle.video_visible!=Some(false)})).await {
                    Ok(_)=>Ok(opened),
                    Err(error)=>{opened.media.close();Err(error)},
                },
                Err(error)=>Err(error),
            };
            match opened {
                Ok(opened) => {
                    self.state.voice_available = b(&opened.capabilities, "voice");
                    let _ = self
                        .updates
                        .send(UiUpdate::VideoFrames(opened.media.video_frames()));
                    self.media = Some(opened.media);
                }
                Err(error) => {
                    let joining_voice=self.voice_lifecycle.request_pending || self.voice_lifecycle.resume.iter().any(|pending|pending.kind=="__voice-requested");
                    if joining_voice { self.report_voice_error(&error); }
                    if self.voice_lifecycle.inventory_pending || self.voice_lifecycle.resume.iter().any(|pending| matches!(pending.kind.as_str(), "media-settings" | "list-devices")) {
                        self.voice_lifecycle.inventory_pending = false;
                        self.data("devices", inventory_failure(&error));
                    }
                    self.voice_lifecycle.resume.clear();
                    self.voice_lifecycle.request_pending = false;
                    self.voice_lifecycle.join_wait = None;
                    self.voice_lifecycle.request_channel.clear();
                    self.state.voice_connecting = false;
                    self.state.voice_available = false;
                    self.refresh_call();
                    return Err(error);
                }
            }
            let actions = self.voice_lifecycle.resume.drain(..).collect::<Vec<_>>();
            for next in actions {
                if next.kind == "__voice-requested" {
                    Box::pin(self.voice_complete(next, json!({"ok":true,"result":{}}))).await?;
                } else {
                    Box::pin(self.media_action(&next)).await?;
                }
            }
            return Ok(());
        }
        // Enumeration is scoped to its own request and media host, not to a
        // call attempt: joining a call must not strand settings in Loading.
        if action.kind == "__voice-data" && action.id == "devices" {
            let meta: Value = serde_json::from_str(&action.extra).unwrap_or_default();
            if meta["media"].as_u64() != Some(self.voice_lifecycle.media_epoch)
                || action.channel_id.parse::<u64>().ok() != Some(self.voice_lifecycle.inventory_request) { return Ok(()); }
            self.voice_lifecycle.inventory_pending = false;
            match unwrapped(&value) {
                Ok(result) => self.data("devices", result),
                Err(error) => self.data("devices", inventory_failure(&error)),
            }
            // The editor already owns its loaded settings. Enumeration can be
            // slow; never replace a newer draft when its result arrives.
            return Ok(());
        }
        if action.kind == "__voice-preflight" {
            let attempt = action.extra.parse::<u64>().unwrap_or(u64::MAX);
            if !self
                .voice_lifecycle
                .request_current(attempt, self.voice_attempt, &action.id)
            {
                return Ok(());
            }
            let exists = self
                .state
                .incoming_calls
                .iter()
                .any(|c| c.channel_id == action.id);
            if !self.voice_lifecycle.can_join(exists) || !self.incoming_join_current() {
                self.voice_lifecycle.request_pending = false;
                self.voice_lifecycle.join_wait = None;
                self.voice_lifecycle.request_channel.clear();
                self.state.voice_connecting = false;
                self.pending_video = false;
                self.refresh_call();
                return Err(
                    "La llamada terminó mientras se comprobaba. No se inició otra llamada.".into(),
                );
            }
            let call = match unwrapped(&value) {
                Ok(v) => v,
                Err(e) => {
                    self.voice_failed(&action).await;
                    return Err(e);
                }
            };
            let mut next = UiAction::id("__voice-requested", &action.id);
            next.extra = attempt.to_string();
            let mut info: Value = serde_json::from_str(&action.text).unwrap_or_default();
            info["ringable"] = json!(b(&call, "ringable"));
            info["recipients"] = call["recipients"].clone();
            next.text = info.to_string();
            if self.media.is_none() {
                self.wait_voice_join(VoiceJoinPhase::MediaHost, 20);
                self.state.call_status_label="Preparando el motor local de voz…".into();
                self.refresh_call();
                self.queue_media(next).await?;
            } else {
                Box::pin(self.voice_complete(next, json!({"ok":true,"result":{}}))).await?;
            }
            return Ok(());
        }
        if action.kind == "__voice-requested" && action.extra.parse::<u64>().is_ok() {
            let attempt = action.extra.parse::<u64>().unwrap();
            if !self
                .voice_lifecycle
                .request_current(attempt, self.voice_attempt, &action.id)
            {
                return Ok(());
            }
            if !self.voice_lifecycle.can_join(
                self.state
                    .incoming_calls
                    .iter()
                    .any(|c| c.channel_id == action.id),
            ) || !self.incoming_join_current()
            {
                self.voice_lifecycle.request_pending = false;
                self.voice_lifecycle.join_wait = None;
                self.voice_lifecycle.request_channel.clear();
                self.state.voice_connecting = false;
                if let Some(media) = self.media.take() {
                    media.close();
                    self.voice_lifecycle.media_epoch += 1;
                }
                self.refresh_call();
                return Err(
                    "La llamada terminó antes de conectar. No se inició otra llamada.".into(),
                );
            }
            let info: Value = serde_json::from_str(&action.text).unwrap_or_default();
            if !s(&info, "guild").is_empty() && !self.channels.get(&action.id).is_some_and(|raw| {
                self.state.selected_guild_id == s(&info, "guild")
                    && self.channel_access(raw).can_view
                    && self.guild_acl.as_ref().is_some_and(|acl| acl.channel_permissions(raw) & (1 << 20) != 0)
            }) {
                self.voice_lifecycle.request_pending = false;
                self.voice_lifecycle.join_wait = None;
                self.voice_lifecycle.request_channel.clear();
                self.state.voice_connecting = false;
                self.refresh_call();
                return Err("Los permisos o el contexto del canal de voz cambiaron antes de conectar.".into());
            }
            self.voice_channel = action.id.clone();
            self.wait_voice_join(VoiceJoinPhase::Gateway, 15);
            self.voice_guild = s(&info, "guild");
            self.voice_lifecycle.active_acl=None;
            self.voice_lifecycle.active_channel=Value::Null;
            self.voice_lifecycle.active_acl_scope=None;
            self.retain_active_voice_permissions();
            self.voice_lifecycle.acl_checked_at = None;
            self.voice_lifecycle.last_permissions = None;
            self.voice_session.clear();
            self.voice_endpoint.clear();
            self.voice_token.clear();
            self.voice_started = false;
            self.state.call_channel_name = s(&info, "name");
            self.state.call_status_label = "Esperando confirmación del canal y servidor de voz…".into();
            self.voice_lifecycle.pending_ring =
                if !self.voice_lifecycle.join_only && b(&info, "ringable") {
                    action.id.clone()
                } else {
                    String::new()
                };
            self.voice_lifecycle.pending_ring_attempt = attempt;
            self.voice_lifecycle.request_pending = false;
            self.voice_lifecycle.request_channel.clear();
            self.call_people.clear();
            self.seed_guild_call_people();
            if let Some(call) = self
                .state
                .incoming_calls
                .iter()
                .find(|call| call.channel_id == self.voice_channel)
            {
                self.call_people.extend(
                    call.call_participants
                        .iter()
                        .cloned()
                        .map(|p| (p.user.id.clone(), p)),
                );
            }
            if let Some(me) = self.state.current_user.clone() {
                self.call_people.insert(
                    me.id.clone(),
                    CallParticipant {
                        user: me,
                        is_self: true,
                        ..Default::default()
                    },
                );
            }
            self.prepare_private_call(&info["recipients"]);
            self.refresh_call();
            let gateway = self.gateway.clone().ok_or("Gateway no disponible.")?;
            let command = self.voice_state_update(false);
            let mut done = UiAction::id("__voice-requested", "gateway");
            done.extra = self.meta().to_string();
            self.launch(done, None, async move {
                Ok(wrapped(
                    gateway
                        .send(command)
                        .await
                        .map(|_| json!({}))
                        .map_err(|e| e.to_string()),
                ))
            });
            return Ok(());
        }
        let meta: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        if meta["media"].as_u64() != Some(self.voice_lifecycle.media_epoch)
            || meta["voice"].as_u64() != Some(self.voice_attempt)
        {
            return Ok(());
        }
        if meta.get("receiver").is_some() && meta["receiver"].as_u64()!=Some(self.voice_lifecycle.receiver_epoch) {return Ok(());}
        if action.kind=="__voice-data"&&action.id=="stream-volume"
            && meta["streamVolumeRevision"].as_u64()!=Some(self.voice_lifecycle.stream_volume_revision) {return Ok(());}
        if action.kind=="__voice-data"&&action.id=="watch-gateway-stop" {
            if !self.voice_lifecycle.watch_stops.get(&action.channel_id).is_some_and(|stop|Some(stop.attempt)==action.text.parse::<u64>().ok()) {return Ok(());}
            match unwrapped(&value) {
                Ok(_)=>{
                    self.voice_lifecycle.watch_stops.get_mut(&action.channel_id).unwrap().written=true;
                    if self.voice_lifecycle.watches.get(&action.channel_id).is_some_and(|watch|watch["requested"]!=true) {Box::pin(self.request_watch(&action.channel_id)).await?;}
                }
                Err(error)=>{
                    if let Some(stop)=self.voice_lifecycle.watch_stops.get_mut(&action.channel_id) {stop.pending=stop.pending.saturating_sub(1);stop.written=true;}
                    if self.voice_lifecycle.watch_stops.get(&action.channel_id).is_some_and(|stop|stop.pending==0) {self.voice_lifecycle.watch_stops.remove(&action.channel_id);}
                    if self.voice_lifecycle.watches.contains_key(&action.channel_id) {self.fail_watch(&action.channel_id,&format!("No se pudo cerrar la suscripción anterior. Puedes reintentar. {error}")).await?;}
                }
            }
            return Ok(());
        }
        if action.kind == "__voice-share"
            && !self.voice_lifecycle.same_share(&meta, self.voice_attempt)
        {
            return Ok(());
        }
        if action.kind == "__voice-camera"
            && meta["camera"].as_u64() != Some(self.voice_lifecycle.camera_attempt)
        {
            return Ok(());
        }
        if action.kind == "__voice-data"
            && matches!(action.id.as_str(), "preview-start" | "preview-stop")
            && meta["preview"].as_u64() != Some(self.voice_lifecycle.preview_attempt)
        {
            return Ok(());
        }
        if action.kind == "__voice-data"
            && matches!(action.id.as_str(), "mic-start" | "mic-stop")
            && meta["mic"].as_u64() != Some(self.voice_lifecycle.mic_attempt)
        {
            return Ok(());
        }
        let result = match unwrapped(&value) {
            Ok(v) => v,
            Err(error) => {
                if meta.get("receiver").is_some() && action.id=="stream-volume" {
                    self.status(&format!("El volumen seleccionado no se pudo aplicar al receptor: {error}. Vuelve a ajustar el volumen para reintentar."));
                    return Ok(());
                }
                if meta.get("receiver").is_some() && matches!(action.id.as_str(),"video-stream-visible"|"video-stream-hidden") {
                    if self.voice_lifecycle.watches.get(&action.channel_id).is_some_and(|watch|watch["attempt"].as_u64()==action.text.parse::<u64>().ok()) {
                        self.fail_watch(&action.channel_id,&format!("No se pudo representar esta pantalla. El audio continúa; puedes reintentar. {error}")).await?;
                    }
                    return Ok(());
                }
                if meta.get("receiver").is_some() && action.id!="watch-start" {
                    self.receiver_failed(&format!("{error} El audio de la llamada continúa; vuelve a abrir la pantalla para reintentar.")).await?;
                    return Ok(());
                }
                if meta.get("receiver").is_some() && action.id=="watch-start" {
                    if self.voice_lifecycle.watches.get(&action.channel_id).is_some_and(|watch|watch["attempt"].as_u64()==action.text.parse::<u64>().ok()) {
                        self.fail_watch(&action.channel_id,&format!("{error} El audio de la llamada continúa; puedes reintentar esta pantalla.")).await?;
                    }
                    return Ok(());
                }
                if action.kind=="__voice-data" && matches!(action.id.as_str(),"volume-user"|"volume-input") {
                    if self.voice_lifecycle.pending_volumes.get(&action.channel_id).copied()==meta["volumeRequest"].as_u64() {
                        self.voice_lifecycle.pending_volumes.remove(&action.channel_id);
                    }
                }
                if action.kind == "__voice-data" && matches!(action.id.as_str(), "mic-start" | "mic-stop") {
                    self.voice_lifecycle.mic_error = Some(format!("No se pudo {} la prueba local: {error}", if action.id == "mic-start" { "iniciar" } else { "detener" }));
                }
                self.voice_failed(&action).await;
                return Err(error);
            }
        };
        match action.kind.as_str() {
            "__voice-requested" => {
                if action.id == "transport" {
                    self.voice_lifecycle.native_voice = result["session"].as_u64();
                    if let Some(early) = self.voice_lifecycle.early_voice.take() {
                        self.media_event(early).await?;
                    }
                }
            }
            "__voice-share" => {
                self.voice_lifecycle.native_share = result["session"].as_u64();
                if !self.state.voice_encryption_ready {
                    self.stop_share().await?;
                } else {
                    self.state.screen_preparing = !self.state.screen_sharing;
                    if let Some(early) = self.voice_lifecycle.early_share.take() {
                        self.media_event(early).await?;
                    }
                    if let Some(g) = &self.gateway {
                        let _ = g
                            .send(GatewayCommand::StreamSetPaused {
                                stream_key: self.stream_key.clone(),
                                paused: false,
                            })
                            .await;
                    }
                }
            }
            "__voice-camera" => {
                self.state.camera_preparing = false;
                if action.id == "stop" {
                    self.state.camera_enabled = false;
                } else if self.state.voice_encryption_ready && s(&result, "state") == "active" {
                    self.state.camera_enabled = true;
                    self.media_preferences.camera_id = action.id;
                    let _ = storage::save_json(
                        &self.data.join("media-settings.json"),
                        &self.media_preferences,
                    );
                } else {
                    if let Some(media) = &self.media {
                        let media = media.clone();
                        let token = self.voice_lifecycle.camera_attempt;
                        tokio::spawn(async move {
                            let _ = media.command("stopCamera", json!({"token":token})).await;
                        });
                    }
                    self.state.camera_enabled = false;
                }
                self.voice_gateway().await?;
            }
            "__voice-data" => match action.id.as_str() {
                "watch-start"=>{
                    let mut early=None;
                    if let Some(watch)=self.voice_lifecycle.watches.get_mut(&action.channel_id) && watch["attempt"].as_u64()==action.text.parse::<u64>().ok() {
                        watch["nativeSession"]=result["session"].clone();
                        if let Some(session)=result["session"].as_u64() {early=self.voice_lifecycle.early_watch_errors.remove(&session);}
                    }
                    if let Some(event)=early {Box::pin(self.media_event(json!({"event":"receiver-host","epoch":self.voice_lifecycle.receiver_epoch,"value":event}))).await?;}
                    if self.voice_lifecycle.watches.values().all(|watch|watch["started"]!=true||watch["nativeSession"].as_u64().is_some()) {self.voice_lifecycle.early_watch_errors.clear();}
                }
                "volume-input" => {
                    if self.voice_lifecycle.pending_volumes.get(&action.channel_id).copied()!=meta["volumeRequest"].as_u64() {return Ok(());}
                    self.voice_lifecycle.pending_volumes.remove(&action.channel_id);
                    let value=action.text.parse::<i32>().map_err(|_|"Volumen de entrada no válido.")?;
                    if action.channel_id!=self.me()||result["target"]!="input"||result["value"].as_i64()!=Some(value as i64) {
                        return Err("El motor no confirmó el volumen de entrada.".into());
                    }
                    self.media_preferences.input_volume=value;
                    storage::save_json(&self.data.join("media-settings.json"),&self.media_preferences)?;
                    self.data("media-settings",serde_json::to_value(&self.media_preferences).unwrap());
                }
                "volume-user" => {
                    if self.voice_lifecycle.pending_volumes.get(&action.channel_id).copied()!=meta["volumeRequest"].as_u64() {return Ok(());}
                    self.voice_lifecycle.pending_volumes.remove(&action.channel_id);
                    let value=action.text.parse::<f32>().map_err(|_|"Volumen del participante no válido.")?;
                    if !parsing::valid_id(&action.channel_id) || !value.is_finite() || !(0.0..=500.0).contains(&value)
                        || result["target"]!="user" || result["userId"]!=action.channel_id || result["value"].as_f64()!=Some(value as f64) {
                        return Err("El motor no confirmó el volumen del participante.".into());
                    }
                    self.media_preferences.user_volumes.insert(action.channel_id.clone(),value);
                    self.voice_lifecycle.volume_sequence=self.voice_lifecycle.volume_sequence.wrapping_add(1).max(1);
                    self.voice_lifecycle.user_volumes_revision=self.voice_lifecycle.volume_sequence;
                    // Native audio already accepted this value. Preserve that
                    // fact in controls even when the disk is no longer writable.
                    let saved=storage::save_json(&self.data.join("media-settings.json"),&self.media_preferences);
                    self.data("media-settings",serde_json::to_value(&self.media_preferences).unwrap());
                    self.refresh_call();
                    saved.map_err(|error|format!("Volumen aplicado, pero no se pudo guardar para la próxima sesión: {error}"))?;
                }
                "watch-timeout" => {
                    if self
                        .voice_lifecycle
                        .watches
                        .get(&action.channel_id)
                        .is_some_and(|watch| {
                            watch["attempt"].as_u64() == action.text.parse::<u64>().ok()
                                && watch["waitEpoch"].as_u64()==meta["watchWait"].as_u64()
                        })
                        && !self
                            .state
                            .call_videos
                            .iter()
                            .any(|video| video.stream_key == action.channel_id && video.active)
                        && self.voice_lifecycle.video_visible!=Some(false)
                    {
                        self.fail_watch(
                            &action.channel_id,
                            Self::watch_timeout_message(self.voice_lifecycle.watches.get(&action.channel_id).unwrap()),
                        )
                        .await?;
                    }
                }
                "preview-cameras" => self.data("cameras-preview", result),
                "mic-start" => {
                    let already_active = self.voice_lifecycle.mic_active;
                    if !self.activate_mic_test().await? { return Ok(()); }
                    if let Ok(preferences) = serde_json::from_str::<MediaPreferences>(&action.text)
                    {
                        self.media_preferences = self.current_media_preferences(preferences);
                        storage::save_json(
                            &self.data.join("media-settings.json"),
                            &self.media_preferences,
                        )?;
                    }
                    if !already_active {
                        self.data("mic-test",json!({"active":true,"pending":false,"level":0,"db":-100,"token":self.voice_lifecycle.mic_attempt,"localOnly":true,"temporaryDeaf":!self.voice_channel.is_empty()}));
                    }
                }
                "mic-stop" => self.finish_mic_test().await?,
                "devices" => {
                    self.data("devices", result);
                    self.data(
                        "media-settings",
                        serde_json::to_value(&self.media_preferences).unwrap(),
                    );
                }
                "cameras" => {
                    if !self.voice_channel.is_empty() {
                        self.data("cameras", result);
                    }
                }
                "sources" => {
                    if !self.voice_channel.is_empty() {
                        self.data("sources", result);
                    }
                }
                "settings" => {
                    let preferences: MediaPreferences =
                        serde_json::from_str(&action.text).map_err(|_| "Ajustes no válidos.")?;
                    let preferences = self.current_media_preferences(preferences);
                    let restart = self.state.camera_enabled
                        && self.state.voice_encryption_ready
                        && !preferences.camera_id.is_empty()
                        && preferences.camera_id != self.media_preferences.camera_id;
                    self.media_preferences = preferences;
                    storage::save_json(
                        &self.data.join("media-settings.json"),
                        &self.media_preferences,
                    )?;
                    self.last_ptt = false;
                    self.voice_lifecycle.ptt_effective = false;
                    if restart {
                        self.voice_lifecycle.camera_attempt += 1;
                        self.state.camera_preparing = true;
                        self.state.camera_enabled = false;
                        let media = self
                            .media
                            .clone()
                            .ok_or("El host multimedia no está disponible.")?;
                        let device = self.media_preferences.camera_id.clone();
                        let token = self.voice_lifecycle.camera_attempt;
                        let mut done = UiAction::id("__voice-camera", &device);
                        done.extra = self.meta().to_string();
                        self.launch(done, None, async move {
                            Ok(wrapped(
                                async {
                                    media
                                        .command(
                                            "stopCamera",
                                            json!({"token":token.saturating_sub(1)}),
                                        )
                                        .await?;
                                    media.command("listCameras", json!({})).await?;
                                    media
                                        .command(
                                            "startCamera",
                                            json!({"deviceId":device,"token":token}),
                                        )
                                        .await
                                }
                                .await,
                            ))
                        });
                    }
                    self.status("Dispositivos y ajustes guardados");
                    self.sync_receiver_settings();
                }
                _ => {}
            },
            _ => {}
        }
        self.refresh_call();
        Ok(())
    }
    pub(super) async fn voice_call_dispatch(
        &mut self,
        kind: &str,
        value: &Value,
    ) -> Result<(), String> {
        let channel = s(value, "channel_id");
        self.observe_private_call_event(kind,value);
        if matches!(kind, "CALL_CREATE" | "CALL_UPDATE") && channel == self.voice_channel {
            if value["voice_states"].is_array() {
                let present=items(&value["voice_states"]).iter().map(|state|s(state,"user_id")).filter(|id|parsing::valid_id(id)).collect::<HashSet<_>>();
                let me=self.me();let removed=self.call_people.keys().filter(|id|*id!=&me&&!present.contains(*id)).cloned().collect::<HashSet<_>>();
                self.call_people.retain(|id,_|id==&me||present.contains(id));
                self.voice_lifecycle.pending_views.retain(|_,view|!removed.contains(&view.owner));
                let watches=self.voice_lifecycle.watches.iter().filter(|(_,watch)|removed.contains(&s(watch,"owner"))).map(|(key,_)|key.clone()).collect::<Vec<_>>();
                for key in watches {self.stop_watch(&key).await?;}
            }
            if let Some(call) = self
                .state
                .incoming_calls
                .iter()
                .find(|call| call.channel_id == channel)
            {
                for person in &call.call_participants {
                    if !self.call_people.contains_key(&person.user.id) && self.call_people.len() >= 256 { continue; }
                    let mut person = person.clone();
                    if let Some(old) = self.call_people.get(&person.user.id) {
                        person.speaking = old.speaking;
                    }
                    self.call_people.insert(person.user.id.clone(), person);
                }
            }
            self.resume_pending_views().await?;
        }
        if kind == "CALL_DELETE" {
            self.voice_lifecycle
                .pending_views
                .retain(|_, view| view.channel != channel);
            if self.voice_lifecycle.request_pending
                && self.voice_lifecycle.request_channel == channel
                && self.voice_lifecycle.join_only
            {
                self.stop_voice_internal(false).await;
            }
            self.refresh_call();
            return Ok(());
        }
        if kind == "CALL_CREATE"
            && channel == self.voice_channel
            && self.voice_lifecycle.take_ring(&channel, self.voice_attempt)
        {
            self.send_private_invites(None,false)?;
        }
        self.refresh_call();
        Ok(())
    }
    pub(super) async fn stop_voice(&mut self) {
        self.stop_voice_internal(true).await;
    }
    async fn stop_voice_internal(&mut self, leave: bool) {
        self.clear_private_outgoing();
        self.close_receiver();
        self.voice_lifecycle.switching=None;
        self.voice_lifecycle.join_wait=None;
        self.voice_lifecycle.pending_views.clear();
        self.state.call_videos.clear();
        self.voice_lifecycle.video_sessions.clear();
        self.voice_lifecycle.watches.clear();
        self.voice_lifecycle.watch_stops.clear();
        self.voice_lifecycle.stream_identities.clear();
        self.voice_lifecycle.stream_viewers.clear();
        self.data("stream-viewers", json!({"channelId":"","voiceAttempt":0,"counts":{}}));
        self.voice_lifecycle.preview_attempt += 1;
        self.voice_lifecycle.watch_attempt += 1;
        if let Some(previous) = self.voice_lifecycle.mic_prior_deaf.take() {
            self.state.self_deaf = previous;
        }
        self.voice_lifecycle.mic_active = false;
        self.voice_lifecycle.mic_error = None;
        self.voice_lifecycle.ptt_effective = false;
        self.voice_lifecycle.mic_attempt += 1;
        self.data(
            "mic-test",
            json!({"active":false,"pending":false,"level":0,"db":-100,"localOnly":true}),
        );
        self.voice_lifecycle.pending_camera = None;
        self.voice_lifecycle.pending_share = None;
        self.voice_attempt += 1;
        self.voice_lifecycle.media_epoch += 1;
        self.voice_lifecycle.inventory_pending = false;
        self.data("devices-loading", json!(false));
        self.voice_lifecycle.share_attempt += 1;
        self.voice_lifecycle.camera_attempt += 1;
        self.voice_lifecycle.request_pending = false;
        self.voice_lifecycle.request_channel.clear();
        self.voice_lifecycle.request_guild.clear();
        self.voice_lifecycle.pending_ring.clear();
        self.voice_lifecycle.resume.clear();
        self.voice_lifecycle.opening = None;
        self.voice_lifecycle.native_voice = None;
        self.voice_lifecycle.native_share = None;
        self.voice_lifecycle.active_acl=None;
        self.voice_lifecycle.active_channel=Value::Null;
        self.voice_lifecycle.active_acl_scope=None;
        self.voice_lifecycle.early_voice = None;
        self.voice_lifecycle.early_share = None;
        let had = !self.voice_channel.is_empty();
        let guild = std::mem::take(&mut self.voice_guild);
        let stream = std::mem::take(&mut self.stream_key);
        self.voice_channel.clear();
        self.voice_session.clear();
        self.voice_endpoint.clear();
        use zeroize::Zeroize;
        self.voice_token.zeroize();
        self.stream_token.zeroize();
        self.voice_started = false;
        self.stream_started = false;
        self.stream_source.clear();
        self.stream_server.clear();
        self.stream_endpoint.clear();
        self.pending_video = false;
        if let Some(media) = self.media.take() {
            media.close();
        }
        self.state.voice_connected = false;
        self.state.voice_encryption_ready = false;
        self.state.voice_connecting = false;
        self.state.screen_sharing = false;
        self.state.screen_paused = false;
        self.state.screen_preparing = false;
        self.state.camera_enabled = false;
        self.state.camera_available = false;
        self.state.camera_preparing = false;
        self.state.screen_available = false;
        self.state.call_started_at = None;
        self.state.call_status_label.clear();
        self.state.call_channel_name.clear();
        self.call_people.clear();
        self.voice_lifecycle.pending_volumes.clear();
        self.refresh_call();
        if leave && had {
            self.voice_lifecycle.leaving=Some(VoiceLeave {guild:guild.clone(), requested_at:Instant::now()});
            if let Some(g) = &self.gateway {
                if !stream.is_empty() {
                    let _ = g
                        .send(GatewayCommand::StreamDelete { stream_key: stream })
                        .await;
                }
                let _ = g
                    .send(GatewayCommand::VoiceState {
                        guild_id: (!guild.is_empty()).then_some(guild),
                        channel_id: None,
                        mute: self.state.self_mute || self.state.self_deaf,
                        deaf: self.state.self_deaf,
                        video: false,
                    })
                    .await;
            }
        }
    }
    fn view_source_available(&self, channel: &str, owner: &str, kind: &str) -> bool {
        let person = if self.voice_channel == channel {
            self.call_people.get(owner)
        } else {
            None
        }
        .or_else(|| {
            self.state
                .incoming_calls
                .iter()
                .find(|call| call.channel_id == channel)
                .and_then(|call| {
                    call.call_participants
                        .iter()
                        .find(|person| person.user.id == owner)
                })
        });
        person.is_some_and(|person| {
            if kind == "camera" {
                person.self_video
            } else {
                person.screen_sharing
            }
        })
    }
    async fn set_peer_video_visible(
        &mut self,
        owner: &str,
        kind: &str,
        visible: bool,
    ) -> Result<(), String> {
        if kind=="camera"&&owner!=self.me()&&self.voice_lifecycle.native_voice.is_some() {
            self.launch_media("__voice-data",if visible {"video-stream-visible"}else{"video-stream-hidden"},"setPeerVideoVisible",json!({"userId":owner,"visible":visible}))?;
        }
        let ids = self
            .state
            .call_videos
            .iter()
            .filter(|video| video.user_id == owner && video.kind == kind && !video.preview)
            .map(|video| video.id)
            .collect::<Vec<_>>();
        for id in ids {
            self.video_command(
                if visible {"video-stream-visible"} else {"video-stream-hidden"},
                "setVideoStreamVisible",
                json!({"id":id,"visible":visible}),
            )?;
            if let Some(video)=self.state.call_videos.iter_mut().find(|video|video.id==id) {video.view_requested=visible;}
        }
        Ok(())
    }
    async fn view_incoming_media(&mut self, action: &UiAction) -> Result<(), String> {
        Self::valid(&action.channel_id)?;
        Self::valid(&action.id)?;
        let kind = action.text.as_str();
        if !matches!(kind, "camera" | "screen") {
            return Err("Selecciona cámara o pantalla.".into());
        }
        let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
        let visible = extra["visible"].as_bool().unwrap_or(true);
        let instance = extra["instanceId"].as_u64().unwrap_or(0);
        if instance > 0
            && !self
                .state
                .incoming_calls
                .iter()
                .any(|call| call.channel_id == action.channel_id && call.instance_id == instance)
        {
            return Err("Esta llamada ya cambió o terminó.".into());
        }
        if extra["voiceAttempt"].as_u64().is_some_and(|attempt| {
            attempt != self.voice_attempt || self.voice_channel != action.channel_id
        }) {
            return Err("La llamada seleccionada ya no está activa.".into());
        }
        let key = format!("{}:{}", action.channel_id, action.id);
        if !visible {
            if self
                .voice_lifecycle
                .pending_views
                .get(&key)
                .is_some_and(|view| view.kind == kind)
            {
                self.voice_lifecycle.pending_views.remove(&key);
            }
            if self.voice_channel == action.channel_id && self.media.is_some() {
                if kind == "screen" {
                    let keys = self
                        .voice_lifecycle
                        .watches
                        .iter()
                        .filter(|(_, watch)| s(watch, "owner") == action.id)
                        .map(|(key, _)| key.clone())
                        .collect::<Vec<_>>();
                    for key in keys {
                        self.stop_watch(&key).await?;
                    }
                }
                self.set_peer_video_visible(&action.id, kind, false).await?;
            }
            return Ok(());
        }
        if !self.view_source_available(&action.channel_id, &action.id, kind) {
            return Err("Esa persona ya no ofrece esa cámara o pantalla.".into());
        }
        if !self.voice_channel.is_empty() && self.voice_channel != action.channel_id
            ||self.voice_lifecycle.request_pending&&self.voice_lifecycle.request_channel!=action.channel_id
            ||self.voice_lifecycle.switching.as_ref().is_some_and(|next|next.channel!=action.channel_id) {
            self.switch_voice(&action.channel_id,false,true,instance).await?;
        }
        if !self.voice_lifecycle.pending_views.contains_key(&key)
            && self.voice_lifecycle.pending_views.len() >= 4
        {
            return Err("Cierra otro vídeo antes de abrir este.".into());
        }
        if self.voice_channel.is_empty() && !self.voice_lifecycle.request_pending&&self.voice_lifecycle.switching.is_none() {
            // A user click explicitly joins the real call. request_voice keeps
            // the user's existing mute/deaf preferences and never auto-rings.
            self.request_voice_scoped(&action.channel_id, false, true, instance)?;
        }
        self.voice_lifecycle.pending_views.insert(
            key,
            PendingView {
                channel: action.channel_id.clone(),
                owner: action.id.clone(),
                kind: kind.into(),
                instance,
                attempt: self.voice_attempt,
                selected: Instant::now(),
                requested: false,
            },
        );
        self.resume_pending_views().await
    }
    pub(super) async fn resume_pending_views(&mut self) -> Result<(), String> {
        if !self.voice_channel.is_empty() && !self.voice_guild.is_empty()
            && self.voice_lifecycle.acl_checked_at.is_none_or(|at|at.elapsed() >= Duration::from_secs(1)) {
            self.voice_lifecycle.acl_checked_at = Some(Instant::now());
            let permissions = self.voice_permissions();
            if !permissions.0 { self.stop_voice().await; return Err("Se perdió el permiso para conectar al canal de voz.".into()); }
            // connectVoice establishes the initial mute before opening audio.
            // Sending a mute command before that native session exists would
            // return NOT_IN_VOICE and incorrectly cancel every guild join.
            if self.state.voice_connected && self.voice_lifecycle.last_permissions != Some(permissions) {
                self.voice_lifecycle.last_permissions = Some(permissions);
                // The host already hard-mutes an active local test. Keep its
                // saved post-test mute independent of temporary UI deafen.
                let mute=self.state.self_mute||self.voice_lifecycle.mic_prior_deaf.unwrap_or(self.state.self_deaf)||!permissions.1;
                self.launch_media("__voice-data","permission-mute","mute",json!({"value":mute}))?;
                if !permissions.2 {
                    if self.state.camera_enabled || self.state.camera_preparing {
                        Box::pin(self.media_action(&UiAction::new("toggle-camera"))).await?;
                    }
                    if self.state.screen_sharing || self.state.screen_preparing || self.state.screen_paused {
                        self.stop_share().await?;
                    }
                }
                self.voice_gateway().await?;
                self.refresh_call();
            }
        }
        if self.voice_lifecycle.video_visible==Some(false)||self.voice_lifecycle.pending_views.is_empty() {
            return Ok(());
        }
        let selected = self
            .voice_lifecycle
            .pending_views
            .iter()
            .map(|(key, view)| (key.clone(), view.clone()))
            .collect::<Vec<_>>();
        for (key, view) in selected {
            let stale = view.attempt != self.voice_attempt
                || view.instance > 0
                    && !self.state.incoming_calls.iter().any(|call| {
                        call.channel_id == view.channel && call.instance_id == view.instance
                    });
            let expired = view.selected.elapsed() > Duration::from_secs(35);
            if stale
                || expired
                || !self.view_source_available(&view.channel, &view.owner, &view.kind)
            {
                self.voice_lifecycle.pending_views.remove(&key);
                if view.requested&&view.kind=="camera"&&view.channel==self.voice_channel {self.set_peer_video_visible(&view.owner,"camera",false).await?;}
                if !stale {
                    self.state.status = if expired {
                        "El vídeo no estuvo disponible a tiempo. Puedes volver a mostrarlo."
                    } else {
                        "La persona dejó de ofrecer la cámara o pantalla seleccionada."
                    }
                    .into();
                    self.publish();
                }
                continue;
            }
            if !self.state.voice_encryption_ready || self.voice_channel != view.channel {
                continue;
            }
            if view.kind == "camera" {
                if !self.state.call_videos.iter().any(|video| {
                    video.user_id == view.owner && video.kind == "camera" && !video.preview
                }) {
                    if !view.requested&&view.owner!=self.me() {
                        self.set_peer_video_visible(&view.owner,"camera",true).await?;
                        if let Some(pending)=self.voice_lifecycle.pending_views.get_mut(&key) {pending.requested=true;}
                    }
                    continue;
                }
                self.voice_lifecycle.pending_views.remove(&key);
                let keys = self
                    .voice_lifecycle
                    .watches
                    .iter()
                    .filter(|(_, watch)| s(watch, "owner") == view.owner)
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                for key in keys {
                    self.stop_watch(&key).await?;
                }
                self.set_peer_video_visible(&view.owner, "screen", false)
                    .await?;
                self.set_peer_video_visible(&view.owner, "camera", true)
                    .await?;
            } else {
                self.voice_lifecycle.pending_views.remove(&key);
                self.set_peer_video_visible(&view.owner, "camera", false)
                    .await?;
                if view.owner == self.me() {
                    self.set_peer_video_visible(&view.owner, "screen", true)
                        .await?;
                } else {
                    let mut action = UiAction::id("watch-stream", &view.owner);
                    action.channel_id = view.channel;
                    Box::pin(self.media_action(&action)).await?;
                }
            }
        }
        Ok(())
    }
    pub(super) async fn media_action(&mut self, action: &UiAction) -> Result<bool, String> {
        match action.kind.as_str() {
            "preview-camera"
            | "start-camera-preview"
            | "start-screen-preview"
            | "stop-camera-preview"
            | "stop-media-preview" => {
                if self.media.is_none() {
                    if !matches!(
                        action.kind.as_str(),
                        "stop-camera-preview" | "stop-media-preview"
                    ) {
                        self.queue_media(action.clone()).await?;
                    } else {
                        self.voice_lifecycle.resume.retain(|pending| {
                            !matches!(
                                pending.kind.as_str(),
                                "preview-camera" | "start-camera-preview" | "start-screen-preview"
                            )
                        });
                    }
                    return Ok(true);
                }
                match action.kind.as_str() {
                    "preview-camera" => self.launch_media(
                        "__voice-data",
                        "preview-cameras",
                        "listCameras",
                        json!({}),
                    )?,
                    "start-camera-preview" | "start-screen-preview" => {
                        if action.id.is_empty()
                            || action.id.len() > 1024
                            || action.id.chars().any(char::is_control)
                        {
                            return Err("Selecciona una cámara disponible.".into());
                        }
                        self.voice_lifecycle.preview_attempt += 1;
                        let camera = action.kind == "start-camera-preview";
                        self.state.call_videos.retain(|video| !video.preview);
                        self.state.call_videos.push(CallVideo {
                            id: u64::MAX / 2 + self.voice_lifecycle.preview_attempt,
                            user_id: self.me(),
                            label: "Vista previa".into(),
                            kind: if camera { "camera" } else { "screen" }.into(),
                            local: true,
                            preview: true,
                            source_id: action.id.clone(),
                            status: "loading".into(),
                            ..Default::default()
                        });
                        self.publish();
                        self.launch_media("__voice-data","preview-start",if camera{"startCameraPreview"}else{"startScreenPreview"},json!({"deviceId":action.id,"sourceId":action.id,"userId":self.me(),"token":self.voice_lifecycle.preview_attempt}))?;
                    }
                    _ => {
                        self.voice_lifecycle.resume.retain(|pending| {
                            !matches!(
                                pending.kind.as_str(),
                                "preview-camera" | "start-camera-preview" | "start-screen-preview"
                            )
                        });
                        self.voice_lifecycle.preview_attempt += 1;
                        self.state.call_videos.retain(|video| !video.preview);
                        self.publish();
                        self.launch_media(
                            "__voice-data",
                            "preview-stop",
                            "stopMediaPreview",
                            json!({"token":self.voice_lifecycle.preview_attempt}),
                        )?;
                    }
                }
            }
            "view-incoming-media" => {
                self.view_incoming_media(action).await?;
            }
            "video-visibility" => {
                if !action.channel_id.is_empty() && action.channel_id != self.voice_channel {
                    return Err("La llamada seleccionada ya no está activa.".into());
                }
                let id = action
                    .id
                    .parse::<u64>()
                    .map_err(|_| "Vídeo no disponible.")?;
                if !self
                    .state
                    .call_videos
                    .iter()
                    .any(|video| video.id == id && !video.preview)
                {
                    return Err("La fuente de vídeo ya no está disponible.".into());
                }
                self.video_command(
                    if action.text=="true" {"video-stream-visible"} else {"video-stream-hidden"},
                    "setVideoStreamVisible",
                    json!({"id":id,"visible":action.text=="true"}),
                )?;
            }
            "video-visible" | "video-quality" | "video-focus" => {
                if action.kind=="video-visible" {
                    let visible=action.text=="true";let previous=self.voice_lifecycle.video_visible!=Some(false);
                    self.voice_lifecycle.video_visible=Some(visible);
                    if previous!=visible {
                        if visible {for pending in self.voice_lifecycle.pending_views.values_mut() {pending.selected=Instant::now();}}
                        let keys=self.voice_lifecycle.watches.keys().cloned().collect::<Vec<_>>();
                        for key in keys {self.wait_watch_frame(&key);}
                    }
                }
                if self.media.is_some() {
                    if action.kind == "video-visible" {
                        self.launch_media(
                            "__voice-data",
                            "video-visible",
                            "setVideoVisible",
                            json!({"visible":action.text=="true"}),
                        )?;
                        if self.voice_lifecycle.receiver.is_some() {self.receiver_command("receiver-visible","setVideoVisible",json!({"visible":action.text=="true"}))?;}
                    } else {
                        let id = action
                            .id
                            .parse::<u64>()
                            .map_err(|_| "Vídeo no disponible.")?;
                        self.video_command(
                            "video-quality",
                            if action.kind == "video-focus" {
                                "focusVideo"
                            } else {
                                "setVideoQuality"
                            },
                            json!({"id":id,"large":action.text=="true"}),
                        )?;
                    }
                }
            }
            "watch-stream" => {
                if !action.channel_id.is_empty() && action.channel_id != self.voice_channel {
                    return Err("La llamada seleccionada ya no está activa.".into());
                }
                if !self.state.voice_encryption_ready {
                    return Err("Espera la conexión cifrada para ver la pantalla.".into());
                }
                let owner = &action.id;
                Self::valid(owner)?;
                if owner == &self.me()
                    || !self
                        .call_people
                        .get(owner)
                        .is_some_and(|person| person.screen_sharing)
                {
                    return Err("Esa persona no está compartiendo pantalla en esta llamada.".into());
                }
                let key = self.stream_key_for(owner);
                if self.voice_lifecycle.watches.contains_key(&key) {
                    return Ok(true);
                }
                if self.voice_lifecycle.watches.len() >= 2 {
                    return Err("Cierra otra pantalla compartida antes de abrir esta.".into());
                }
                if self.voice_lifecycle.watch_stops.len()>=256&&!self.voice_lifecycle.watch_stops.contains_key(&key) {
                    return Err("Hay demasiados cierres de pantalla pendientes de confirmación. Puedes reintentar esta pantalla cuando Discord los confirme.".into());
                }
                self.voice_lifecycle.watch_stops.entry(key.clone()).or_insert(WatchStop {attempt:0,written:true,pending:0});
                self.voice_lifecycle.watch_attempt += 1;
                let identity=self.voice_lifecycle.stream_identities.get(&key).cloned().unwrap_or_default();
                self.voice_lifecycle.watches.insert(key.clone(),json!({"owner":owner,"started":false,"requested":false,"attempt":self.voice_lifecycle.watch_attempt,"server":identity.server,"channel":identity.channel,"phase":"stream-server"}));
                self.state
                    .call_videos
                    .retain(|video| video.stream_key != key);
                self.state.call_videos.push(CallVideo {
                    view_requested: true,
                    id: u64::MAX - self.voice_lifecycle.watch_attempt,
                    user_id: owner.clone(),
                    label: self
                        .people
                        .get(owner)
                        .map(|person| person.name.clone())
                        .unwrap_or_else(|| "Pantalla compartida".into()),
                    kind: "screen".into(),
                    stream_key: key.clone(),
                    source_id: format!("{key}:{}",self.voice_lifecycle.watch_attempt),
                    status: "connecting".into(),
                    ..Default::default()
                });
                self.publish();
                self.request_watch(&key).await?;
            }
            "stop-watch-stream" => {
                let key = if self.voice_lifecycle.watches.contains_key(&action.id)
                    || self
                        .state
                        .call_videos
                        .iter()
                        .any(|video| video.stream_key == action.id)
                {
                    action.id.clone()
                } else {
                    self.state
                        .call_videos
                        .iter()
                        .find(|video| {
                            video.user_id == action.id && video.kind == "screen" && !video.local
                        })
                        .map(|video| video.stream_key.clone())
                        .unwrap_or_default()
                };
                if !key.is_empty() {
                    self.stop_watch(&key).await?;
                }
            }
            "call-dm" | "call-dm-video" | "voice" => {
                let id = if action.id.is_empty() {
                    self.state.selected_channel_id.clone()
                } else {
                    action.id.clone()
                };
                self.join_voice(&id, action.kind == "call-dm-video").await?;
            }
            "join-call" => {
                let extra: Value = serde_json::from_str(&action.extra).unwrap_or_default();
                self.switch_voice(
                    &action.id,
                    false,
                    true,
                    extra["instanceId"].as_u64().unwrap_or(0),
                ).await?;
            }
            "disconnect-voice" => self.stop_voice().await,
            "mute" | "deafen" => {
                if self.voice_lifecycle.mic_prior_deaf.is_some() {
                    return Err(
                        "Detén la prueba de micrófono para cambiar silencio o ensordecimiento."
                            .into(),
                    );
                }
                if action.kind == "mute" {
                    if self.state.self_deaf {
                        self.state.self_deaf = false;
                        self.state.self_mute = false;
                    } else {
                        self.state.self_mute = !self.state.self_mute;
                    }
                } else {
                    self.state.self_deaf = !self.state.self_deaf;
                }
                let _ = storage::save_json(
                    &self.data.join("audio-settings.json"),
                    &json!({"mute":self.state.self_mute,"deaf":self.state.self_deaf}),
                );
                if !self.voice_channel.is_empty() {
                    if let Some(media) = self.media.clone() {
                        let mute = self.effective_voice_mute();
                        let deaf = self.state.self_deaf;
                        let mut done = UiAction::id("__voice-requested", "audio");
                        done.extra = self.meta().to_string();
                        self.launch(done, None, async move {
                            Ok(wrapped(
                                async {
                                    media.command("mute", json!({"value":mute})).await?;
                                    media.command("deafen", json!({"value":deaf})).await?;
                                    Ok(json!({}))
                                }
                                .await,
                            ))
                        });
                    }
                    self.voice_gateway().await?;
                    self.sync_receiver_settings();
                }
                self.refresh_call();
                self.sync_receiver_settings();
            }
            "media-settings" | "list-devices" => {
                if self.voice_lifecycle.inventory_pending && self.media.is_some() { return Ok(true); }
                if self.media.is_none() {
                    if self.voice_lifecycle.resume.iter().any(|pending| matches!(pending.kind.as_str(),"media-settings"|"list-devices")) { return Ok(true); }
                    self.data("devices-loading", json!(true));
                    self.queue_media(action.clone()).await?;
                    return Ok(true);
                }
                self.voice_lifecycle.inventory_pending = true;
                self.voice_lifecycle.inventory_request = self.voice_lifecycle.inventory_request.wrapping_add(1);
                self.data("devices-loading", json!(true));
                let media = self.media.clone().unwrap();
                let mut done = UiAction::id("__voice-data", "devices");
                done.extra = self.meta().to_string();
                done.channel_id = self.voice_lifecycle.inventory_request.to_string();
                done.text = action.kind.clone();
                self.launch(done, None, async move {
                    // Keep independent failures separate; a denied camera does
                    // not hide working audio devices or window sources.
                    let audio = media.command("listDevices", json!({})).await;
                    let cameras = media.command("listCameras", json!({})).await;
                    let sources = media.command("listSources", json!({})).await;
                    let mut result = json!({});
                    match audio {
                        Ok(audio) => { inventory_part(&mut result,"input",Ok(audio["input"].clone())); inventory_part(&mut result,"output",Ok(audio["output"].clone())); }
                        Err(error) => { inventory_part(&mut result,"input",Err(error.clone())); inventory_part(&mut result,"output",Err(error)); }
                    }
                    inventory_part(&mut result,"cameras",cameras);
                    inventory_part(&mut result,"sources",sources);
                    Ok(wrapped(Ok(result)))
                });
            }
            "save-media-settings" => {
                let preferences: MediaPreferences = serde_json::from_str(&action.text)
                    .map_err(|_| "Ajustes multimedia no válidos.")?;
                let preferences = self.current_media_preferences(preferences);
                if self.media.is_none() {
                    storage::save_json(&self.data.join("media-settings.json"), &preferences)?;
                    self.media_preferences = preferences;
                } else {
                    let media = self.media.clone().unwrap();
                    let settings = self.media_parameters_for(&preferences);
                    let mut done = UiAction::id("__voice-data", "settings");
                    done.extra = self.meta().to_string();
                    done.text = serde_json::to_string(&preferences).unwrap();
                    self.launch(done, None, async move {
                        Ok(wrapped(
                            media
                                .command("configureAudio", settings)
                                .await,
                        ))
                    });
                }
            }
            "test-mic" | "stop-mic-test" => {
                if self.media.is_none() {
                    if action.kind == "stop-mic-test" {
                        self.voice_lifecycle
                            .resume
                            .retain(|pending| pending.kind != "test-mic");
                        return Ok(true);
                    }
                    self.queue_media(action.clone()).await?;
                    return Ok(true);
                }
                if action.kind == "test-mic" && self.voice_lifecycle.mic_prior_deaf.is_some() {
                    return Err(
                        "La prueba de micrófono ya se está preparando o está activa.".into(),
                    );
                }
                self.voice_lifecycle.mic_attempt += 1;
                let token = self.voice_lifecycle.mic_attempt;
                if action.kind == "test-mic" {
                    self.voice_lifecycle.mic_error = None;
                    let preferences = if action.text.is_empty() {
                        self.media_preferences.clone()
                    } else {
                        serde_json::from_str::<MediaPreferences>(&action.text)
                            .map_err(|_| "Ajustes de prueba no válidos.")?
                            .normalize()
                    };
                    let preferences = self.current_media_preferences(preferences);
                    let settings = self.media_parameters_for(&preferences);
                    if self.state.screen_sharing
                        || self.state.screen_preparing
                        || self.state.screen_paused
                    {
                        self.stop_share().await?;
                    }
                    self.voice_lifecycle.mic_prior_deaf = Some(self.state.self_deaf);
                    if !self.voice_channel.is_empty() {
                        self.state.self_deaf = true;
                    }
                    self.data("mic-test",json!({"active":false,"pending":true,"level":0,"db":-100,"token":token,"localOnly":true,"temporaryDeaf":!self.voice_channel.is_empty()}));
                    let media = self.media.clone().unwrap();
                    let mut done = UiAction::id("__voice-data", "mic-start");
                    done.extra = self.meta().to_string();
                    done.text = serde_json::to_string(&preferences).unwrap();
                    self.launch(done, None, async move {
                        Ok(wrapped(
                            media
                                .command(
                                    "startMicTest",
                                    json!({"token":token,"settings":settings}),
                                )
                                .await,
                        ))
                    });
                } else {
                    self.voice_lifecycle.mic_error = None;
                    self.launch_media(
                        "__voice-data",
                        "mic-stop",
                        "stopMicTest",
                        json!({"token":token}),
                    )?;
                }
                self.refresh_call();
            }
            "toggle-camera" => {
                if self.state.camera_enabled || self.state.camera_preparing {
                    if self.voice_lifecycle.pending_camera.is_some()
                        && self
                            .state
                            .call_videos
                            .iter()
                            .any(|video| video.preview && video.kind == "camera")
                    {
                        Box::pin(self.media_action(&UiAction::new("stop-media-preview"))).await?;
                    }
                    self.voice_lifecycle.pending_camera = None;
                    self.voice_lifecycle.camera_attempt += 1;
                    self.state.camera_preparing = false;
                    self.state.camera_enabled = false;
                    self.launch_media(
                        "__voice-camera",
                        "stop",
                        "stopCamera",
                        json!({"token":self.voice_lifecycle.camera_attempt}),
                    )?;
                } else {
                    self.require_voice_video()?;
                    if self.voice_channel.is_empty() {
                        return Err("Entra a una llamada para seleccionar una cámara.".into());
                    }
                    self.launch_media("__voice-data", "cameras", "listCameras", json!({}))?;
                }
            }
            "start-camera" => {
                self.require_voice_video()?;
                if self.voice_channel.is_empty() {
                    return Err("La llamada terminó. No se activó la cámara.".into());
                }
                if action.id.is_empty()
                    || action.id.len() > 1024
                    || action.id.chars().any(char::is_control)
                {
                    return Err("Selecciona una cámara disponible.".into());
                }
                if !self.state.voice_encryption_ready {
                    self.voice_lifecycle.camera_attempt += 1;
                    self.voice_lifecycle.pending_camera = Some((action.id.clone(), Instant::now()));
                    self.state.camera_preparing = true;
                    Box::pin(self.media_action(&UiAction::id("start-camera-preview", &action.id)))
                        .await?;
                    self.status("Vista previa local · la cámara sólo se enviará al terminar la conexión cifrada.");
                    self.refresh_call();
                    return Ok(true);
                }
                if !self.state.camera_available {
                    return Err(
                        "Esta llamada no negoció vídeo de cámara. Vuelve a entrar para intentarlo."
                            .into(),
                    );
                }
                if self.state.camera_preparing {
                    return Err("La cámara todavía se está preparando.".into());
                }
                self.voice_lifecycle.camera_attempt += 1;
                self.state.camera_preparing = true;
                self.refresh_call();
                self.launch_media(
                    "__voice-camera",
                    &action.id,
                    "startCamera",
                    json!({"deviceId":action.id,"token":self.voice_lifecycle.camera_attempt}),
                )?;
            }
            "share" | "stop-share" => {
                if self.state.screen_sharing
                    || self.state.screen_paused
                    || self.state.screen_preparing
                    || action.kind == "stop-share"
                {
                    self.stop_share().await?;
                } else {
                    self.require_voice_video()?;
                    if self.voice_channel.is_empty() {
                        return Err("Entra a una llamada para seleccionar una pantalla.".into());
                    }
                    self.launch_media("__voice-data", "sources", "listSources", json!({}))?;
                }
            }
            "start-share" => {
                self.require_voice_video()?;
                if self.voice_channel.is_empty() {
                    return Err("La llamada terminó. No se compartió la pantalla.".into());
                }
                if self.voice_lifecycle.mic_prior_deaf.is_some() {
                    return Err("Detén la prueba local antes de compartir pantalla o audio.".into());
                }
                let quality: Value = serde_json::from_str(&action.text)
                    .map_err(|_| "Selección de pantalla no válida.")?;
                let height = i(&quality, "height");
                let fps = i(&quality, "fps");
                if ![480, 720, 1080, 1440].contains(&height)
                    || ![15, 30, 60].contains(&fps)
                    || action.id.len() > 512
                    || action.id.is_empty()
                {
                    return Err("Selecciona una fuente y calidad disponibles.".into());
                }
                if !self.state.voice_encryption_ready {
                    self.voice_lifecycle.share_attempt += 1;
                    self.voice_lifecycle.pending_share = Some((action.clone(), Instant::now()));
                    self.state.screen_preparing = true;
                    Box::pin(self.media_action(&UiAction::id("start-screen-preview", &action.id)))
                        .await?;
                    self.status("Vista previa local · la pantalla sólo se enviará al terminar la conexión cifrada.");
                    self.refresh_call();
                    return Ok(true);
                }
                self.voice_lifecycle.share_attempt += 1;
                self.voice_lifecycle.native_share = None;
                self.voice_lifecycle.early_share = None;
                self.stream_source = action.id.clone();
                self.stream_quality = quality;
                self.stream_key = self.expected_stream();
                self.stream_server.clear();
                self.stream_endpoint.clear();
                self.stream_token.clear();
                self.stream_started = false;
                self.state.screen_preparing = true;
                self.refresh_call();
                let gateway = self.gateway.clone().ok_or("Gateway no disponible.")?;
                gateway
                    .send(GatewayCommand::StreamCreate {
                        guild_id: (!self.voice_guild.is_empty()).then(|| self.voice_guild.clone()),
                        channel_id: self.voice_channel.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
            }
            "set-volume" => {
                let args: Value =
                    serde_json::from_str(&action.text).map_err(|_| "Volumen no válido.")?;
                self.launch_media("__voice-data", "volume", "setVolume", args)?;
            }
            "stream-volume" => {
                let value=action.text.parse::<f32>().ok().filter(|value|value.is_finite()&&(0.0..=100.0).contains(value))
                    .ok_or("El volumen de las transmisiones admite de 0 a 100 %.")?;
                self.media_preferences.stream_volume_percent=value;
                self.voice_lifecycle.stream_volume_revision=self.voice_lifecycle.stream_volume_revision.wrapping_add(1).max(1);
                let saved=storage::save_json(&self.data.join("media-settings.json"),&self.media_preferences);
                self.data("media-settings",serde_json::to_value(&self.media_preferences).unwrap());
                self.refresh_call();
                if self.voice_lifecycle.receiver.is_some() {
                    self.receiver_command("stream-volume","setStreamVolume",json!({"value":value,"revision":self.voice_lifecycle.stream_volume_revision}))?;
                }
                saved.map_err(|error|format!("El volumen se conservó en esta sesión, pero no se pudo guardar: {error}"))?;
            }
            "participant-volume" => {
                Self::valid(&action.id)?;
                let context:Value=serde_json::from_str(&action.extra).map_err(|_|"La llamada cambió.")?;
                if action.channel_id!=self.voice_channel || context["voiceAttempt"].as_u64()!=Some(self.voice_attempt)
                    || !self.state.voice_connected || self.voice_channel.is_empty() || action.id==self.me()
                    || !self.call_people.contains_key(&action.id) {return Err("Selecciona otro participante de la llamada actual.".into());}
                let value=action.text.parse::<f32>().ok().filter(|value|value.is_finite()&&(0.0..=500.0).contains(value)).ok_or("El volumen admite de 0 a 500 %.")?;
                if self.media_preferences.user_volumes.len()>=256 && !self.media_preferences.user_volumes.contains_key(&action.id) {return Err("Demasiados ajustes de participantes.".into());}
                let media=self.media.clone().ok_or("El host multimedia no está disponible.")?;
                self.voice_lifecycle.volume_sequence=self.voice_lifecycle.volume_sequence.wrapping_add(1).max(1);
                let token=self.voice_lifecycle.volume_sequence;
                self.voice_lifecycle.pending_volumes.insert(action.id.clone(),token);
                let mut done=UiAction::id("__voice-data","volume-user");done.channel_id=action.id.clone();done.text=value.to_string();
                let mut meta=self.meta();meta["volumeRequest"]=json!(token);done.extra=meta.to_string();
                let args=json!({"target":"user","userId":action.id,"value":value,"volumeRevision":token});
                self.launch(done,None,async move {Ok(wrapped(media.command("setVolume",args).await))});
            }
            "participant-input-volume" => {
                let context:Value=serde_json::from_str(&action.extra).map_err(|_|"La llamada cambió.")?;
                if action.id!=self.me()||action.channel_id!=self.voice_channel||context["voiceAttempt"].as_u64()!=Some(self.voice_attempt)
                    ||!self.state.voice_connected||self.voice_channel.is_empty() {return Err("Selecciona tu micrófono en la llamada actual.".into());}
                let value=action.text.parse::<i32>().ok().filter(|value|(0..=200).contains(value)).ok_or("El micrófono admite de 0 a 200 %.")?;
                let media=self.media.clone().ok_or("El host multimedia no está disponible.")?;
                self.voice_lifecycle.volume_sequence=self.voice_lifecycle.volume_sequence.wrapping_add(1).max(1);
                let token=self.voice_lifecycle.volume_sequence;
                self.voice_lifecycle.pending_volumes.insert(action.id.clone(),token);
                let mut done=UiAction::id("__voice-data","volume-input");done.channel_id=action.id.clone();done.text=value.to_string();
                let mut meta=self.meta();meta["volumeRequest"]=json!(token);done.extra=meta.to_string();
                self.launch(done,None,async move {Ok(wrapped(media.command("setVolume",json!({"target":"input","value":value})).await))});
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
    fn voice_state_update(&self, video: bool) -> GatewayCommand {
        GatewayCommand::VoiceState {
            guild_id: (!self.voice_guild.is_empty()).then(|| self.voice_guild.clone()),
            channel_id: (!self.voice_channel.is_empty()).then(|| self.voice_channel.clone()),
            mute: self.effective_voice_mute(),
            deaf: self.state.self_deaf,
            video,
        }
    }
    async fn voice_gateway(&self) -> Result<(), String> {
        if let Some(g) = &self.gateway {
            g.send(self.voice_state_update(self.state.camera_enabled))
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub(super) async fn voice_dispatch(&mut self, kind: &str, value: &Value) -> Result<(), String> {
        self.incoming_media_dispatch(kind, value);
        match kind {
            "VOICE_STATE_UPDATE" => {
                self.remember_guild_voice(value);
                if s(value,"user_id")==self.me()&&self.voice_lifecycle.leaving.as_ref().is_some_and(|previous|s(value,"guild_id")==previous.guild) {
                    if value.get("channel_id").is_some_and(Value::is_null) {
                        self.voice_lifecycle.leaving=None;
                        self.state.voice_connecting=false;
                        if let Some(next)=self.voice_lifecycle.switching.take() {
                            if let Err(error)=self.request_voice_scoped(&next.channel,next.video,next.join_only,next.instance) {
                                self.state.call_channel_name.clear();self.state.call_status_label.clear();self.refresh_call();return Err(error);
                            }
                            for view in self.voice_lifecycle.pending_views.values_mut().filter(|view|view.channel==next.channel) {view.attempt=self.voice_attempt;}
                        }
                    }
                    return Ok(());
                }
                let event_guild = s(value, "guild_id");
                if !event_guild.is_empty() && event_guild != self.voice_guild {
                    // Keep the observed seed, but a different guild must not
                    // remove participants from the current private/guild call.
                    return Ok(());
                }
                let id = s(value, "user_id");
                let channel = if value.get("channel_id").is_none() && (id == self.me() || self.call_people.contains_key(&id)) {
                    // Partial metadata for a known participant is not a leave.
                    // Only explicit channel:null removes current membership.
                    self.voice_channel.clone()
                } else { s(value, "channel_id") };
                if id == self.me() && !self.voice_channel.is_empty()
                    && !channel.is_empty() && channel != self.voice_channel {
                    // A remote move changes the server-side channel. Keep no
                    // old transport/capture alive and do not broadcast into
                    // the destination until the user explicitly joins it.
                    let connected = self.state.voice_connected;
                    let destination = self.channels.get(&channel).filter(|raw|self.channel_access(raw).can_view)
                        .and_then(|_|self.state.channels.iter().find(|item|item.id==channel))
                        .map(|item|item.name.clone());
                    self.state.call_move_version=self.state.call_move_version.wrapping_add(1);
                    if connected { self.data("sound",json!({"event":"user_move"})); }
                    self.stop_voice_internal(false).await;
                    self.status(&format!("Te movieron a {}. Se cerró la conexión anterior; selecciona el destino y pulsa Unirse para comprobar sus permisos y conectar de nuevo.",destination.map(|name|format!("«{name}»")).unwrap_or_else(||"otro canal".into())));
                    return Ok(());
                }
                if id != self.me() && self.call_people.contains_key(&id)
                    && self.state.voice_connected && !channel.is_empty() && channel != self.voice_channel {
                    self.state.call_move_version=self.state.call_move_version.wrapping_add(1);
                    self.data("sound", json!({"event":"user_move"}));
                }
                if id != self.me()
                    && (channel != self.voice_channel
                        || value.get("self_stream") == Some(&Value::Bool(false)))
                {
                    let keys: Vec<_> = self
                        .state
                        .call_videos
                        .iter()
                        .filter(|video| {
                            video.user_id == id && video.kind == "screen" && !video.local
                        })
                        .map(|video| video.stream_key.clone())
                        .filter(|key| !key.is_empty())
                        .collect();
                    for key in keys {
                        self.stop_watch(&key).await?;
                    }
                }
                if id!=self.me()&&(channel!=self.voice_channel||value.get("self_stream")==Some(&Value::Bool(false))) {
                    self.voice_lifecycle.stream_identities.retain(|key,_|key.rsplit(':').next()!=Some(id.as_str()));
                }
                if id!=self.me()&&(channel!=self.voice_channel||value.get("self_video")==Some(&Value::Bool(false))) {
                    self.voice_lifecycle.pending_views.retain(|_,view|view.owner!=id||view.kind!="camera");
                    self.state.call_videos.retain(|video|video.local||video.user_id!=id||video.kind!="camera");
                    self.voice_lifecycle.video_sessions.retain(|video_id,_|self.state.call_videos.iter().any(|video|video.id==*video_id));
                }
                if channel == self.voice_channel && !self.voice_channel.is_empty() {
                    let previous = self.call_people.get(&id).cloned();
                    if (previous.is_some() || self.call_people.len() < 256)
                        && let Some(person) = self.incoming_participant(value, previous.as_ref()) {
                        self.call_people.insert(id.clone(), person);
                    }
                } else {
                    self.call_people.remove(&id);
                }
                if id == self.me() {
                    if channel.is_empty() {
                        if !self.voice_channel.is_empty() {
                            self.stop_voice_internal(false).await;
                            self.report_voice_error("Discord confirmó la salida del canal de voz. Puedes volver a unirte para abrir una sesión nueva.");
                        }
                        return Ok(());
                    }
                    if channel == self.voice_channel {
                        if let Some(session)=value.get("session_id").and_then(Value::as_str).filter(|session|!session.is_empty() && session.len()<=512) {
                            self.voice_session = session.to_owned();
                        }
                        self.try_voice().await?;
                    }
                }
                self.refresh_call();
            }
            "VOICE_SERVER_UPDATE" => {
                if self.voice_channel.is_empty() {
                    return Ok(());
                }
                if !self.voice_guild.is_empty() && s(value, "guild_id") != self.voice_guild {
                    return Ok(());
                }
                if self.voice_guild.is_empty()
                    && !s(value, "channel_id").is_empty()
                    && s(value, "channel_id") != self.voice_channel
                {
                    return Ok(());
                }
                self.voice_endpoint = s(value, "endpoint");
                self.voice_token = s(value, "token");
                self.try_voice().await?;
            }
            "STREAM_UPDATE" => {
                self.remember_stream_identity(value);
                self.observe_stream_viewers(value);
                let key=s(value,"stream_key");
                if let Some(watch)=self.voice_lifecycle.watches.get_mut(&key) {
                    if let Some(identity)=self.voice_lifecycle.stream_identities.get(&key) {
                        watch["server"]=json!(identity.server);watch["channel"]=json!(identity.channel);
                    }
                    self.try_watch(&key).await?;
                }
                if let Some((channel, owner)) =
                    super::incoming::incoming_stream_identity(&s(value, "stream_key"))
                {
                    if channel == self.voice_channel {
                        if let Some(person) = self.call_people.get_mut(&owner) {
                            person.screen_sharing = true;
                        }
                    }
                }
            }
            "STREAM_CREATE" => {
                self.remember_stream_identity(value);
                self.observe_stream_viewers(value);
                let key = s(value, "stream_key");
                if let Some((channel, owner)) = super::incoming::incoming_stream_identity(&key) {
                    if channel == self.voice_channel {
                        if let Some(person) = self.call_people.get_mut(&owner) {
                            person.screen_sharing = true;
                        }
                    }
                }
                if let Some(watch) = self.voice_lifecycle.watches.get_mut(&key) {
                    if let Some(identity)=self.voice_lifecycle.stream_identities.get(&key) {
                        watch["server"]=json!(identity.server);watch["channel"]=json!(identity.channel);
                    }
                    self.try_watch(&key).await?;
                }
                if s(value, "stream_key") == self.expected_stream()
                    && !self.stream_source.is_empty()
                {
                    self.stream_key = s(value, "stream_key");
                    self.stream_server = s(value, "rtc_server_id");
                    self.stream_quality["streamChannelId"] = value
                        .get("rtc_channel_id")
                        .or_else(|| value.get("stream_channel_id"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    self.try_share().await?;
                }
            }
            "STREAM_SERVER_UPDATE" => {
                let key = s(value, "stream_key");
                if let Some(watch) = self.voice_lifecycle.watches.get_mut(&key) {
                    watch["endpoint"] = value["endpoint"].clone();
                    watch["token"] = value["token"].clone();
                    self.try_watch(&key).await?;
                }
                if s(value, "stream_key") == self.expected_stream()
                    && !self.stream_source.is_empty()
                {
                    self.stream_endpoint = s(value, "endpoint");
                    self.stream_token = s(value, "token");
                    self.try_share().await?;
                }
            }
            "STREAM_DELETE" => {
                let key = s(value, "stream_key");
                let expected=value["reason"]=="user_requested"&&self.voice_lifecycle.watch_stops.get(&key).is_some_and(|stop|stop.pending>0);
                if expected {
                    let stop=self.voice_lifecycle.watch_stops.get_mut(&key).unwrap();stop.pending-=1;
                    if stop.pending==0 {self.voice_lifecycle.watch_stops.remove(&key);}
                    // A viewer's STREAM_STOP receives the same dispatch as a
                    // publisher. Its delayed reply cannot cancel a fresh watch.
                    if self.voice_lifecycle.watches.get(&key).is_some_and(|watch|watch["requested"]!=true) {Box::pin(self.request_watch(&key)).await?;}
                } else if self.voice_lifecycle.watches.contains_key(&key) {
                    self.voice_lifecycle.watches.get_mut(&key).unwrap()["requested"]=json!(false);
                    match value["reason"].as_str() {
                        Some("stream_full")=>self.fail_watch(&key,"La transmisión alcanzó su límite de espectadores. Puedes volver a intentarlo.").await?,
                        Some("unauthorized")=>self.fail_watch(&key,"Discord no autorizó esta suscripción de pantalla.").await?,
                        _=>self.finish_watch(&key).await?,
                    }
                }
                // The publisher's VOICE_STATE self_stream, not a viewer's
                // subscription teardown, controls whether its screen is offered.
                if s(value, "stream_key") == self.stream_key {
                    self.stop_share().await?;
                }
            }
            _ => (),
        }
        self.resume_pending_views().await?;
        if kind.starts_with("STREAM_") {
            self.refresh_call();
        }
        Ok(())
    }
    async fn try_voice(&mut self) -> Result<(), String> {
        if self.voice_started
            || self.voice_session.is_empty()
            || self.voice_endpoint.is_empty()
            || self.voice_token.is_empty()
        {
            return Ok(());
        }
        let args = json!({"userId":self.me(),"guildId":if self.voice_guild.is_empty(){Value::Null}else{json!(self.voice_guild)},"channelId":self.voice_channel,"sessionId":self.voice_session,"voiceToken":self.voice_token,"endpoint":self.voice_endpoint,"selfMute":self.effective_voice_mute(),"selfDeaf":self.state.self_deaf,"participantVolumes":self.media_preferences.user_volumes,"userVolumesRevision":self.voice_lifecycle.user_volumes_revision});
        self.launch_media("__voice-requested", "transport", "connectVoice", args)?;
        self.voice_started = true;
        self.wait_voice_join(VoiceJoinPhase::Transport, 45);
        self.state.call_status_label="Conectando con el servidor de voz…".into();
        use zeroize::Zeroize;
        self.voice_token.zeroize();
        Ok(())
    }
    fn expected_stream(&self) -> String {
        self.stream_key_for(&self.me())
    }
    fn publish_stream_viewers(&self) {
        let counts:serde_json::Map<String,Value> = self.voice_lifecycle.stream_viewers.iter()
            .map(|(key,viewers)|(key.clone(),json!(viewers.len()))).collect();
        self.data("stream-viewers",json!({"channelId":self.voice_channel,"voiceAttempt":self.voice_attempt,"counts":counts}));
    }
    fn observe_stream_viewers(&mut self, value: &Value) {
        let Some(values)=value["viewer_ids"].as_array() else {return;};
        let key=s(value,"stream_key");
        let Some((channel,owner))=super::incoming::incoming_stream_identity(&key) else {return;};
        if channel!=self.voice_channel || self.voice_channel.is_empty() {return;}
        if !self.voice_lifecycle.stream_viewers.contains_key(&key) && self.voice_lifecycle.stream_viewers.len()>=32 {return;}
        let viewers:HashSet<String>=values.iter().filter_map(Value::as_str).filter(|id|parsing::valid_id(id))
            .take(256).map(str::to_owned).collect();
        if let Some(previous)=self.voice_lifecycle.stream_viewers.get(&key) {
            if previous==&viewers {return;}
            if owner==self.me() && self.state.screen_sharing && self.state.self_presence!="dnd" {
                if viewers.difference(previous).any(|id|id!=&owner) {
                    self.data("sound",json!({"event":"stream_viewer_joined"}));
                }
                if previous.difference(&viewers).any(|id|id!=&owner) {
                    self.data("sound",json!({"event":"stream_viewer_left"}));
                }
            }
        }
        self.voice_lifecycle.stream_viewers.insert(key,viewers);
        self.publish_stream_viewers();
    }
    fn stream_key_for(&self, owner: &str) -> String {
        if self.voice_guild.is_empty() {
            format!("call:{}:{}", self.voice_channel, owner)
        } else {
            format!(
                "guild:{}:{}:{}",
                self.voice_guild, self.voice_channel, owner
            )
        }
    }
    fn remember_stream_identity(&mut self,value:&Value) {
        let key=s(value,"stream_key");
        let parts=key.split(':').collect::<Vec<_>>();
        let current=match parts.as_slice() {
            ["call",channel,owner]=>self.voice_guild.is_empty()&&*channel==self.voice_channel&&parsing::valid_id(owner),
            ["guild",guild,channel,owner]=>*guild==self.voice_guild&&*channel==self.voice_channel&&parsing::valid_id(owner),
            _=>false,
        };
        if !current || self.voice_channel.is_empty() {return;}
        let server=s(value,"rtc_server_id");
        let channel=value.get("rtc_channel_id").or_else(||value.get("stream_channel_id")).and_then(Value::as_str).filter(|id|parsing::valid_id(id));
        if !parsing::valid_id(&server)&&channel.is_none() {return;}
        if self.voice_lifecycle.stream_identities.len()>=256&&!self.voice_lifecycle.stream_identities.contains_key(&key) {return;}
        let identity=self.voice_lifecycle.stream_identities.entry(key).or_default();
        if parsing::valid_id(&server)&&server!=identity.server {identity.server=server;identity.channel=None;}
        if let Some(channel)=channel {identity.channel=Some(channel.into());}
    }
    fn wait_watch_frame(&mut self,key:&str) {
        let Some(watch)=self.voice_lifecycle.watches.get_mut(key) else{return;};
        let epoch=watch["waitEpoch"].as_u64().unwrap_or(0).wrapping_add(1);
        watch["waitEpoch"]=json!(epoch);
        if self.voice_lifecycle.video_visible==Some(false) {return;}
        let attempt=watch["attempt"].as_u64().unwrap_or(0);
        let mut timeout=UiAction::id("__voice-data","watch-timeout");timeout.channel_id=key.into();timeout.text=attempt.to_string();
        let mut meta=self.meta();meta["watchWait"]=json!(epoch);timeout.extra=meta.to_string();
        self.launch(timeout,None,async{tokio::time::sleep(Duration::from_secs(35)).await;Ok(wrapped(Ok(json!({}))))});
    }
    fn set_watch_phase(&mut self,key:&str,phase:&str) {
        let changed=self.voice_lifecycle.watches.get_mut(key).is_some_and(|watch| {
            if watch["phase"].as_str()==Some(phase) {false} else {watch["phase"]=json!(phase);true}
        });
        if changed {self.wait_watch_frame(key);}
    }
    fn watch_timeout_message(watch:&Value)->&'static str {
        match watch["phase"].as_str().unwrap_or("") {
            "stream-stop"=>"Discord no confirmó el cierre de la suscripción anterior. Puedes reintentar sin salir de voz.",
            "stream-identity"=>"Discord no entregó la identidad RTC de esta pantalla. El audio continúa; puedes reintentar.",
            "stream-server"=>"Discord no entregó el servidor de esta transmisión. El audio continúa; puedes reintentar.",
            "receiver"=>"El receptor de pantalla no terminó de abrirse. El audio continúa; puedes reintentar.",
            "websocket-open"|"transport"=>"No se completó la conexión al servidor de pantalla. El audio continúa; puedes reintentar.",
            "ready-received"|"native-connection-before"|"native-connection-created"|"udp-ready"|"transport-configured"|"encryption-modes-ready"=>"No se completó la negociación de transporte del vídeo. El audio continúa; puedes reintentar.",
            "session-description"|"mls-epoch"|"mls-key-package-before"|"mls-key-package-created"|"mls-state-updated"|"waiting-peer"=>"No se confirmó el cifrado DAVE de esta pantalla. El audio continúa; puedes reintentar.",
            "connected"=>"El receptor se conectó, pero no llegó la fuente de vídeo de la transmisión. El audio continúa; puedes reintentar.",
            "video-subscription-requested"=>"Se solicitó el vídeo al servidor, pero no llegó su primer fotograma. El audio continúa; puedes reintentar.",
            "native-source"=>"Se registró la fuente de pantalla, pero no comenzó la entrega de vídeo. El audio continúa; puedes reintentar.",
            "first-frame"=>"Se instaló el receptor de fotogramas, pero el motor nativo no entregó el primero. El audio continúa; puedes reintentar.",
            _=>"La transmisión no respondió. El audio continúa; puedes volver a intentarlo.",
        }
    }
    async fn try_share(&mut self) -> Result<(), String> {
        if self.stream_source.is_empty()
            || self.stream_started
            || self.stream_server.is_empty()
            || self.stream_endpoint.is_empty()
            || self.stream_token.is_empty()
        {
            return Ok(());
        }
        if !self.state.voice_encryption_ready {
            self.stop_share().await?;
            return Err("La pantalla se canceló al perder el cifrado.".into());
        }
        self.stream_started = true;
        let args = json!({"userId":self.me(),"channelId":self.voice_channel,"streamKey":self.stream_key,"rtcServerId":self.stream_server,"streamChannelId":self.stream_quality["streamChannelId"],"sessionId":self.voice_session,"voiceToken":self.stream_token,"endpoint":self.stream_endpoint,"sourceId":self.stream_source,"quality":{"height":self.stream_quality["height"],"fps":self.stream_quality["fps"]},"audio":{"enabled":b(&self.stream_quality,"audio"),"loopback":b(&self.stream_quality,"loopback"),"pid":i(&self.stream_quality,"pid")}});
        let mut args = args;
        args["token"] = json!(self.voice_lifecycle.share_attempt);
        self.launch_media("__voice-share", "start", "startShare", args)?;
        use zeroize::Zeroize;
        self.stream_token.zeroize();
        Ok(())
    }
    async fn request_watch(&mut self,key:&str)->Result<(),String> {
        if !self.voice_lifecycle.watches.get(key).is_some_and(|watch|watch["requested"]!=true) {return Ok(());}
        if self.voice_lifecycle.watch_stops.get(key).is_some_and(|stop|!stop.written) {
            self.set_watch_phase(key,"stream-stop");return Ok(());
        }
        // Replies received while the prior subscription was closing are not
        // credentials for this new STREAM_WATCH request.
        if let Some(watch)=self.voice_lifecycle.watches.get_mut(key) {
            use zeroize::Zeroize;
            if let Some(Value::String(token))=watch.get_mut("token") {token.zeroize();}
            watch["endpoint"]=json!("");watch["token"]=json!("");
        }
        let requested=match self.gateway.as_ref() {
            Some(gateway)=>gateway.send(GatewayCommand::StreamWatch {stream_key:key.into()}).await.map_err(|error|error.to_string()),
            None=>Err("Gateway no disponible.".into()),
        };
        if let Err(error)=requested {
            self.fail_watch(key,&format!("No se pudo solicitar esta transmisión. Puedes volver a intentarlo. {error}")).await?;
            return Ok(());
        }
        if let Some(watch)=self.voice_lifecycle.watches.get_mut(key) {watch["requested"]=json!(true);}
        self.set_watch_phase(key,"stream-server");
        self.wait_watch_frame(key);
        Ok(())
    }
    async fn try_watch(&mut self, key: &str) -> Result<(), String> {
        let Some(watch) = self.voice_lifecycle.watches.get(key) else {
            return Ok(());
        };
        if watch["requested"]==false||watch["started"]==true||!self.state.voice_encryption_ready {return Ok(());}
        if s(watch,"server").is_empty() {self.set_watch_phase(key,"stream-identity");return Ok(());}
        if s(watch,"endpoint").is_empty()||s(watch,"token").is_empty() {self.set_watch_phase(key,"stream-server");return Ok(());}
        let args = json!({"userId":self.me(),"streamOwnerId":watch["owner"],"streamKey":key,"channelId":self.voice_channel,"rtcServerId":watch["server"],"streamChannelId":watch["channel"],"sessionId":self.voice_session,"voiceToken":watch["token"],"endpoint":watch["endpoint"],"selfMute":true,"selfDeaf":self.state.self_deaf,"participantVolumes":self.media_preferences.user_volumes,"userVolumesRevision":self.voice_lifecycle.user_volumes_revision,"token":watch["attempt"]});
        if self.voice_lifecycle.receiver.is_none() {self.set_watch_phase(key,"receiver");self.probe_receiver().await?;return Ok(());}
        self.voice_lifecycle.watches.get_mut(key).unwrap()["started"] = json!(true);
        self.set_watch_phase(key,"transport");
        let media = self
            .voice_lifecycle.receiver
            .clone()
            .ok_or("El receptor de pantalla no está disponible.")?;
        let mut done = UiAction::id("__voice-data", "watch-start");
        let mut meta=self.meta();meta["receiver"]=json!(self.voice_lifecycle.receiver_epoch);done.extra = meta.to_string();
        done.channel_id = key.into();
        done.text = self.voice_lifecycle.watches[key]["attempt"]
            .as_u64()
            .unwrap_or(0)
            .to_string();
        self.launch(done, None, async move {
            Ok(wrapped(media.command("watchStream", args).await))
        });
        self.voice_lifecycle.watches.get_mut(key).unwrap()["token"] = Value::Null;
        Ok(())
    }
    async fn stop_watch(&mut self, key: &str) -> Result<(), String> {
        let removed=self.voice_lifecycle.watches.remove(key);
        let existed=removed.is_some();
        let subscribed=removed.as_ref().is_some_and(|watch|watch["requested"]==true);
        self.state
            .call_videos
            .retain(|video| video.stream_key != key);
        self.voice_lifecycle.video_sessions.retain(|id,_|self.state.call_videos.iter().any(|video|video.id==*id));
        self.voice_lifecycle.watch_attempt += 1;
        if self.voice_lifecycle.watches.is_empty() {
            self.voice_lifecycle.early_watch_errors.clear();
            self.voice_lifecycle.receiver_epoch=self.voice_lifecycle.receiver_epoch.wrapping_add(1);
            self.voice_lifecycle.receiver_opening=None;
            if let Some(receiver)=self.voice_lifecycle.receiver.take() {
                receiver.clear_video_frames();let key=key.to_owned();let token=self.voice_lifecycle.watch_attempt;
                tokio::spawn(async move {
                    if existed {let _=tokio::time::timeout(Duration::from_secs(2),receiver.command("stopWatchStream",json!({"streamKey":key,"token":token}))).await;}
                    let _=tokio::time::timeout(Duration::from_secs(1),receiver.command("shutdown",json!({}))).await;receiver.close();
                });
            }
        } else if existed && self.voice_lifecycle.receiver.is_some() {
            self.receiver_command(
                "watch-stop",
                "stopWatchStream",
                json!({"streamKey":key,"token":self.voice_lifecycle.watch_attempt}),
            )?;
        }
        if !subscribed&&self.voice_lifecycle.watch_stops.get(key).is_some_and(|stop|stop.pending==0) {self.voice_lifecycle.watch_stops.remove(key);}
        if subscribed && let Some(gateway)=self.gateway.clone() {
            let attempt=self.voice_lifecycle.watch_attempt;
            // One bounded counter retains all outstanding USER_REQUESTED
            // replies; a second close cannot overwrite the first reply's guard.
            let stop=self.voice_lifecycle.watch_stops.entry(key.into()).or_insert(WatchStop {attempt,written:false,pending:0});
            stop.attempt=attempt;stop.written=false;stop.pending=stop.pending.saturating_add(1);
            let mut done=UiAction::id("__voice-data","watch-gateway-stop");done.channel_id=key.into();done.text=attempt.to_string();done.extra=self.meta().to_string();
            let key=key.to_owned();
            // Original GatewayConnectionStore STREAM_STOP calls streamDelete
            // for viewers too (web.4d4b2f75fdea5470.js bytes 4951222/4955300).
            self.launch(done,None,async move {Ok(wrapped(gateway.send(GatewayCommand::StreamDelete {stream_key:key}).await.map(|_|json!({})).map_err(|error|error.to_string())))});
        }
        self.publish();
        Ok(())
    }
    async fn fail_watch(&mut self, key: &str, message: &str) -> Result<(), String> {
        let placeholder = self
            .state
            .call_videos
            .iter()
            .find(|video| video.stream_key == key)
            .cloned();
        self.stop_watch(key).await?;
        if let Some(mut video) = placeholder {
            video.active = false;
            video.status = "error".into();
            video.error = message.into();
            self.state.call_videos.push(video);
        }
        self.status(message);
        self.publish();
        Ok(())
    }
    async fn finish_watch(&mut self, key: &str) -> Result<(), String> {
        // A normal terminal subscription is neither a decoder failure nor a
        // publisher announcement. Remove only this viewer's resources. The
        // publisher's VOICE_STATE controls its independently offered buttons.
        if let Some(watch)=self.voice_lifecycle.watches.get_mut(key) {
            watch["requested"]=json!(false);
            let owner=s(watch,"owner");
            self.voice_lifecycle.pending_views.retain(|_,view|view.owner!=owner||view.kind!="screen");
        }
        self.stop_watch(key).await?;
        self.refresh_call();
        self.publish();
        Ok(())
    }
    async fn stop_share(&mut self) -> Result<(), String> {
        if self.voice_lifecycle.pending_share.is_some()
            && self
                .state
                .call_videos
                .iter()
                .any(|video| video.preview && video.kind == "screen")
        {
            Box::pin(self.media_action(&UiAction::new("stop-media-preview"))).await?;
        }
        self.voice_lifecycle.pending_share = None;
        self.voice_lifecycle.share_attempt += 1;
        self.voice_lifecycle.native_share = None;
        self.voice_lifecycle.early_share = None;
        let key = std::mem::take(&mut self.stream_key);
        self.voice_lifecycle.stream_viewers.remove(&key);
        self.publish_stream_viewers();
        self.stream_source.clear();
        self.stream_server.clear();
        self.stream_endpoint.clear();
        use zeroize::Zeroize;
        self.stream_token.zeroize();
        self.stream_started = false;
        self.state.screen_sharing = false;
        self.state.screen_paused = false;
        self.state.screen_preparing = false;
        if let Some(media) = self.media.clone() {
            let token = self.voice_lifecycle.share_attempt;
            tokio::spawn(async move {
                let _ = media.command("stopShare", json!({"token":token})).await;
            });
        }
        if !key.is_empty() {
            if let Some(g) = &self.gateway {
                let _ = g
                    .send(GatewayCommand::StreamDelete { stream_key: key })
                    .await;
            }
        }
        self.refresh_call();
        self.sync_receiver_settings();
        Ok(())
    }
    pub(super) async fn media_event(&mut self, value: Value) -> Result<(), String> {
        match s(&value, "event").as_str() {
            "receiver-host"=>{
                if value["epoch"].as_u64()!=Some(self.voice_lifecycle.receiver_epoch) {return Ok(());}
                let event=value["value"].clone();let kind=s(&event,"event");
                if kind=="mediaStage" {
                    let key=s(&event,"streamKey");
                    let mut progressed=false;
                    if let Some(watch)=self.voice_lifecycle.watches.get_mut(&key)
                        && event["token"]==watch["attempt"] {
                        let stage=s(&event,"stage");progressed=watch["phase"].as_str()!=Some(stage.as_str());watch["phase"]=json!(stage);
                        let status=if stage=="video-subscription-requested" {"loading"}else if stage.starts_with("mls-")||stage=="session-description" {"waiting"}else{"connecting"};
                        for video in self.state.call_videos.iter_mut().filter(|video|video.stream_key==key&&!video.active) {video.status=status.into();}
                        self.publish();
                    }
                    if progressed {self.wait_watch_frame(&key);}
                    return Ok(());
                }
                if kind=="fatal" || kind=="videoError" && s(&event,"scope")!="stream" {
                    return self.receiver_failed(&Self::receiver_failure_message(&event)).await;
                }
                if kind=="videoStream" {
                    let key=s(&event,"streamKey");
                    if !self.voice_lifecycle.watches.get(&key).is_some_and(|watch|event["token"]==watch["attempt"]) {return Ok(());}
                }
                if kind=="videoError" || kind=="videoStream" && s(&event,"state")=="error" {
                    let user=s(&event,"userId");let stream=s(&event,"streamKey");
                    let keys=self.voice_lifecycle.watches.iter().filter(|(key,watch)|
                        (stream.is_empty()||stream==key.as_str())&&(user.is_empty()||s(watch,"owner")==user)
                            && (kind!="videoError"||event["mediaSession"].as_u64().is_some_and(|session|watch["nativeSession"].as_u64()==Some(session)))
                    ).map(|(key,_)|key.clone()).collect::<Vec<_>>();
                    let message=if s(&event,"message").is_empty() {"El vídeo no pudo abrirse. El audio continúa; puedes reintentar la pantalla.".to_owned()}else{s(&event,"message")};
                    if keys.is_empty()&&kind=="videoError"
                        && let Some(session)=event["mediaSession"].as_u64().filter(|session|*session>0)
                        && self.voice_lifecycle.early_watch_errors.len()<4
                        && self.voice_lifecycle.watches.values().any(|watch|watch["started"]==true&&watch["nativeSession"].as_u64().is_none()) {
                        // A native callback can precede the watch-start reply.
                        // Defer it until that reply confirms the session identity.
                        self.voice_lifecycle.early_watch_errors.insert(session,event);return Ok(());
                    }
                    for key in keys {self.fail_watch(&key,&message).await?;}return Ok(());
                }
                if matches!(kind.as_str(),"videoStream"|"videoError"|"watchState") {Box::pin(self.media_event(event)).await?;}
            }
            "videoStream" => {
                if b(&value, "preview")
                    && value["previewToken"].as_u64() != Some(self.voice_lifecycle.preview_attempt)
                {
                    return Ok(());
                }
                let id = value["id"].as_u64().unwrap_or(0);
                if id == 0 {
                    return Ok(());
                }
                if s(&value, "kind") == "screen"
                    && !b(&value, "local")
                    && !self
                        .voice_lifecycle
                        .watches
                        .contains_key(&s(&value, "streamKey"))
                {
                    return Ok(());
                }
                let mut status = s(&value, "state");
                let key=s(&value,"streamKey");
                if self.voice_lifecycle.watches.contains_key(&key) {
                    // These are confirmed metadata/callback milestones, not a
                    // native decoder stack trace or a diagnosis of its failure.
                    if status=="waiting" {self.set_watch_phase(&key,"native-source");}
                    else if status=="loading" {self.set_watch_phase(&key,"first-frame");}
                    else if status=="playing" {
                        let watch=self.voice_lifecycle.watches.get_mut(&key).unwrap();watch["phase"]=json!("frames-received");watch["waitEpoch"]=json!(watch["waitEpoch"].as_u64().unwrap_or(0).wrapping_add(1));
                    }
                }
                if status == "removed" {
                    let key=s(&value,"streamKey");
                    if !b(&value,"local")&&s(&value,"kind")=="screen"&&self.voice_lifecycle.watches.contains_key(&key) {
                        // A native SSRC/sink replacement does not end the user's
                        // authorized GoLive watch. Keep its stable loading tile.
                        if let Some(video)=self.state.call_videos.iter_mut().find(|video|video.id==id) {
                            video.active=false;video.status="waiting".into();video.error.clear();
                        }
                        self.voice_lifecycle.video_sessions.remove(&id);
                        self.wait_watch_frame(&key);
                    } else {
                        let camera_replacement=self.state.call_videos.iter().any(|video|video.id==id&&!video.local&&video.kind=="camera"&&video.view_requested
                            && self.call_people.get(&video.user_id).is_some_and(|person|person.self_video));
                        if camera_replacement {
                            // Decoder teardown while hiding/replacing a camera
                            // is not withdrawal of the publisher's announcement.
                            if let Some(video)=self.state.call_videos.iter_mut().find(|video|video.id==id) {
                                video.active=false;video.status="waiting".into();video.error.clear();
                            }
                            self.voice_lifecycle.video_sessions.remove(&id);
                        } else {self.state.call_videos.retain(|video| video.id != id);}
                    }
                } else {
                    let person = s(&value, "userId");
                    let local = b(&value, "local");
                    let name = if local && b(&value, "preview") {
                        "Vista previa".into()
                    } else {
                        self.people
                            .get(&person)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| s(&value, "label"))
                    };
                    let remote_screen=!local&&s(&value,"kind")=="screen";
                    let watch=self.voice_lifecycle.watches.get(&s(&value,"streamKey"));
                    let requested_screen=remote_screen&&watch.is_some();
                    let requested=self.state.call_videos.iter().any(|old|old.id==id&&old.user_id==person&&old.kind==s(&value,"kind")&&old.source_id==s(&value,"sourceId")&&old.view_requested
                        && (local&&!old.preview||value["mediaSession"].as_u64().is_none_or(|session|self.voice_lifecycle.video_sessions.get(&id)==Some(&session))))
                        || !local&&s(&value,"kind")=="camera"
                            && value["mediaSession"].as_u64().is_none_or(|session|self.voice_lifecycle.native_voice==Some(session))
                            && self.call_people.get(&person).is_some_and(|peer|peer.self_video)
                            && self.state.call_videos.iter().any(|old|old.user_id==person&&old.kind=="camera"&&!old.local&&old.view_requested)
                        || requested_screen;
                    // Suspending a selected sink is not an end of its source.
                    // Retain that selection until the window restores reception.
                    if status=="paused" && (requested_screen || requested&&!b(&value,"preview")&&(!local||self.voice_lifecycle.video_visible==Some(false))) {status="waiting".into();}
                    let video = CallVideo {
                        view_requested: requested,
                        id,
                        user_id: person,
                        label: name,
                        kind: s(&value, "kind"),
                        local,
                        active: b(&value, "active"),
                        stream_key: s(&value, "streamKey"),
                        status,
                        preview: b(&value, "preview"),
                        source_id: if requested_screen {format!("{}:{}",s(&value,"streamKey"),watch.and_then(|watch|watch["attempt"].as_u64()).unwrap_or(0))}else{s(&value,"sourceId")},
                        error: s(&value, "message"),
                    };
                    self.state.call_videos.retain(|old| {
                        old.id == id
                            || !((video.preview && old.preview)
                                || (video.kind == "screen"
                                    && !video.stream_key.is_empty()
                                    && old.stream_key == video.stream_key)
                                || (video.kind=="camera"&&!video.local&&!old.local&&old.kind=="camera"
                                    && old.user_id==video.user_id&&!old.active&&old.status=="waiting"))
                    });
                    if let Some(existing) = self
                        .state
                        .call_videos
                        .iter_mut()
                        .find(|video| video.id == id)
                    {
                        *existing = video;
                    } else if self.state.call_videos.len() < 32 {
                        self.state.call_videos.push(video);
                    }
                }
                self.voice_lifecycle.video_sessions.retain(|id,_|self.state.call_videos.iter().any(|video|video.id==*id));
                if s(&value,"state")!="removed"&&self.state.call_videos.iter().any(|video|video.id==id) {
                    if let Some(session)=value["mediaSession"].as_u64() {self.voice_lifecycle.video_sessions.insert(id,session);}
                }
                if self.voice_lifecycle.video_visible!=Some(false)&&self.state.call_videos.iter().any(|video|video.id==id&&video.local&&!video.preview&&video.view_requested&&video.status=="paused") {
                    self.video_command("video-stream-visible","setVideoStreamVisible",json!({"id":id,"visible":true}))?;
                }
                if self.voice_lifecycle.video_visible!=Some(false)&&value["visible"]==false&&s(&value,"state")=="waiting"
                    && self.state.call_videos.iter().any(|video|video.id==id&&!video.local&&video.kind=="screen"&&video.view_requested) {
                    // Registration has crossed the control lane and authorized
                    // its frame id before the native sink may emit frame one.
                    self.video_command("video-stream-visible","setVideoStreamVisible",json!({"id":id,"visible":true}))?;
                }
                self.resume_pending_views().await?;
                self.publish();
            }
            "videoError" => {
                let scoped = s(&value, "scope") == "stream";
                let source_id=value["id"].as_u64().filter(|id|*id>0);
                let session=value["mediaSession"].as_u64();
                let stream_key=s(&value,"streamKey");
                let user=s(&value,"userId");
                if !scoped && self.media.is_some() {
                    let _ = self.launch_media(
                        "__voice-data",
                        "video-visible",
                        "setVideoVisible",
                        json!({"visible":false}),
                    );
                }
                for video in self.state.call_videos.iter_mut().filter(|video| {
                    if !scoped {return true;}
                    // A participant may have a camera and an independent GoLive
                    // transport. A user id alone cannot identify the failed one.
                    let identity=source_id.map(|id|video.id==id)
                        .or_else(||(!stream_key.is_empty()).then(||video.stream_key==stream_key))
                        .unwrap_or(session.is_some());
                    identity && (user.is_empty()||video.user_id==user)
                        && session.is_none_or(|session|self.voice_lifecycle.video_sessions.get(&video.id)==Some(&session))
                }) {
                    video.active = false;
                    video.status = "error".into();
                    video.error = s(&value, "message");
                }
                self.status(&s(&value, "message"));
                self.publish();
            }
            "watchState" => {
                let key = s(&value, "streamKey");
                if !self
                    .voice_lifecycle
                    .watches
                    .get(&key)
                    .is_some_and(|watch| value["token"] == watch["attempt"])
                {
                    return Ok(());
                }
                if s(&value,"state")=="disconnected" && value["reason"]["code"].as_str().is_none()
                    && value["error"]["code"].as_str().is_none() {
                    self.finish_watch(&key).await?;
                } else if matches!(s(&value, "state").as_str(), "error" | "disconnected") {
                    let message = value["reason"]["message"]
                        .as_str()
                        .or_else(|| value["error"]["message"].as_str())
                        .unwrap_or("La transmisión se interrumpió. Puedes volver a intentarlo.");
                    self.fail_watch(&key, message).await?;
                } else {
                    let status=if s(&value,"state")=="connected" {"loading"} else {"connecting"};
                    for video in self.state.call_videos.iter_mut().filter(|video|video.stream_key==key && !video.active) {
                        video.status=status.into();
                    }
                    self.publish();
                }
            }
            "voiceState" => {
                if let Some(session) = value["mediaSession"].as_u64() {
                    if let Some(current) = self.voice_lifecycle.native_voice {
                        if session != current {
                            return Ok(());
                        }
                    } else {
                        self.voice_lifecycle.early_voice = Some(value);
                        return Ok(());
                    }
                }
                if self.voice_channel.is_empty() {
                    return Ok(());
                }
                let state = s(&value, "state");
                apply_readiness(&mut self.state, &value);
                if self.state.voice_encryption_ready {
                    self.voice_lifecycle.join_wait=None;self.state.voice_error.clear();
                } else if self.state.voice_connected {
                    let has_peer=self.call_people.keys().any(|id|id!=&self.me());
                    if has_peer {
                        if self.voice_lifecycle.join_wait.is_none() {self.wait_voice_join(VoiceJoinPhase::Transport,45);}
                    } else {self.voice_lifecycle.join_wait=None;}
                }
                if matches!(state.as_str(), "error" | "disconnected") {
                    self.stop_voice().await;
                    let reason=parsing::limited(value["reason"]["message"].as_str().unwrap_or("La conexión de voz terminó."),256);
                    self.report_voice_error(&format!("{reason} Puedes volver a unirte para abrir una sesión nueva."));
                } else if self.pending_video && self.state.voice_connected {
                    self.pending_video = false;
                    self.launch_media("__voice-data", "cameras", "listCameras", json!({}))?;
                }
                if self.state.voice_encryption_ready {
                    self.resume_explicit_capture().await?;
                    self.resume_pending_views().await?;
                }
                self.refresh_call();
            }
            "shareState" => {
                if let Some(session) = value["mediaSession"].as_u64() {
                    if let Some(current) = self.voice_lifecycle.native_share {
                        if session != current {
                            return Ok(());
                        }
                    } else {
                        self.voice_lifecycle.early_share = Some(value);
                        return Ok(());
                    }
                }
                if self.stream_source.is_empty() {
                    return Ok(());
                }
                let state = s(&value, "state");
                self.state.screen_paused = state == "paused";
                self.state.screen_sharing = matches!(
                    state.as_str(),
                    "capture-requested" | "audio-attached" | "sharing" | "paused" | "started"
                );
                self.state.screen_preparing = matches!(
                    state.as_str(),
                    "connecting" | "negotiating" | "waiting-peer" | "connected"
                );
                if matches!(state.as_str(), "error" | "stopped" | "disconnected") {
                    self.stop_share().await?;
                }
                self.refresh_call();
            }
            "cameraState" => {
                if self.voice_channel.is_empty() {
                    return Ok(());
                }
                if value["token"]
                    .as_u64()
                    .is_some_and(|token| token != self.voice_lifecycle.camera_attempt)
                    || value["mediaSession"]
                        .as_u64()
                        .is_some_and(|session| Some(session) != self.voice_lifecycle.native_voice)
                {
                    return Ok(());
                }
                if self.voice_lifecycle.pending_camera.is_some()
                    && !self.state.voice_encryption_ready
                {
                    return Ok(());
                }
                let state = s(&value, "state");
                self.state.camera_enabled = self.state.voice_encryption_ready
                    && matches!(state.as_str(), "started" | "active");
                self.state.camera_preparing = state == "preparing";
                if matches!(state.as_str(), "error" | "stopped") {
                    self.state.camera_enabled = false;
                    self.voice_gateway().await?;
                }
                self.refresh_call();
            }
            "speaking" | "participant" => {
                if self.voice_channel.is_empty() {
                    return Ok(());
                }
                let id = s(&value, "userId");
                if !parsing::valid_id(&id) {
                    return Ok(());
                }
                if s(&value, "event") == "participant" && value["present"] == false {
                    self.call_people.remove(&id);
                } else {
                    if !self.call_people.contains_key(&id) && self.call_people.len() < 256 {
                        let user = self.people.get(&id).cloned().unwrap_or(Person {
                            id: id.clone(),
                            name: "Usuario".into(),
                            ..Default::default()
                        });
                        self.call_people.insert(
                            id.clone(),
                            CallParticipant {
                                user,
                                is_self: id == self.me(),
                                ..Default::default()
                            },
                        );
                    }
                    if let Some(p) = self.call_people.get_mut(&id) {
                        if s(&value, "event") == "speaking" {
                            p.speaking = b(&value, "active");
                        }
                        if value.get("video").is_some() {
                            p.self_video = b(&value, "video");
                        }
                    }
                }
                self.refresh_call();
            }
            "micTest" => {
                let mut value = value;
                if value["token"]
                    .as_u64()
                    .is_some_and(|token| token != self.voice_lifecycle.mic_attempt)
                {
                    return Ok(());
                }
                if value["cleanupFailed"]==true {
                    self.hold_failed_mic_test("No se pudo cerrar la prueba local. Sigues conectado con el audio silenciado; pulsa Detener para reintentar.");
                    return Ok(());
                }
                if value["active"] == false {
                    self.finish_mic_test().await?;
                    value["error"] = json!(self.voice_lifecycle.mic_error);
                } else {
                    if !self.activate_mic_test().await? { return Ok(()); }
                }
                self.data("mic-test", value);
            }
            "inputLevel" => {
                if self.voice_lifecycle.mic_prior_deaf.is_none() {
                    let mut value = value;
                    value["error"] = json!(self.voice_lifecycle.mic_error);
                    self.data("mic-test", value);
                }
            }
            "inputGateState" => {
                let Some(session)=value["mediaSession"].as_u64() else {return Ok(());};
                if self.voice_channel.is_empty()||self.voice_lifecycle.native_voice!=Some(session) {return Ok(());}
                if value["muted"]==true {
                    self.state.self_mute=true;
                    let _=storage::save_json(&self.data.join("audio-settings.json"),&json!({"mute":true,"deaf":self.state.self_deaf}));
                    let _=self.voice_gateway().await;
                    self.refresh_call();
                    self.status("El control de entrada falló y el micrófono quedó silenciado. El audio recibido continúa; puedes activar el micrófono para reintentar.");
                }
            }
            "fatal" => {
                self.stop_voice().await;
                self.state.voice_available = MediaBridge::is_available(&self.root);
                self.report_voice_error("El servicio multimedia se detuvo. Puedes volver a unirte para abrir una sesión nueva.");
                self.publish();
            }
            _ => (),
        }
        Ok(())
    }
    async fn resume_explicit_capture(&mut self) -> Result<(), String> {
        if let Some((device, selected)) = self.voice_lifecycle.pending_camera.take() {
            self.state.camera_preparing = false;
            if selected.elapsed() <= Duration::from_secs(90) {
                if let Err(error) =
                    Box::pin(self.media_action(&UiAction::id("start-camera", &device))).await
                {
                    self.status(&error);
                }
            } else {
                if self.state.call_videos.iter().any(|video| {
                    video.preview && video.kind == "camera" && video.source_id == device
                }) {
                    Box::pin(self.media_action(&UiAction::new("stop-media-preview"))).await?;
                }
                self.status("La selección de cámara caducó. Vuelve a elegirla para activarla.");
            }
        }
        if let Some((action, selected)) = self.voice_lifecycle.pending_share.take() {
            self.state.screen_preparing = false;
            if selected.elapsed() <= Duration::from_secs(90) {
                if let Err(error) = Box::pin(self.media_action(&action)).await {
                    self.status(&error);
                }
            } else {
                if self.state.call_videos.iter().any(|video| {
                    video.preview && video.kind == "screen" && video.source_id == action.id
                }) {
                    Box::pin(self.media_action(&UiAction::new("stop-media-preview"))).await?;
                }
                self.status("La selección de pantalla caducó. Vuelve a elegirla para compartir.");
            }
        }
        Ok(())
    }
    async fn activate_mic_test(&mut self) -> Result<bool, String> {
        if self.voice_lifecycle.mic_prior_deaf.is_none() { return Ok(false); }
        let started = !self.voice_lifecycle.mic_active;
        self.voice_lifecycle.mic_active = true;
        if started {
            // The start reply and the first meter callback can arrive in
            // either order. Only the state transition updates the Gateway;
            // subsequent level samples are local UI data, not voice joins.
            self.voice_gateway().await?;
            self.sync_receiver_settings();
        }
        Ok(true)
    }
    async fn finish_mic_test(&mut self) -> Result<(), String> {
        let previous = self.voice_lifecycle.mic_prior_deaf.take();
        self.voice_lifecycle.mic_active = false;
        if let Some(previous) = previous {
            // Native stopMicTest resets setPTTActive. A no-op stop when no
            // test existed must keep the edge state until the real release.
            self.last_ptt = false;
            self.voice_lifecycle.ptt_effective = false;
            self.state.self_deaf = previous;
        }
        self.data("mic-test",json!({"active":false,"pending":false,"error":self.voice_lifecycle.mic_error,"level":0,"db":-100,"token":self.voice_lifecycle.mic_attempt,"localOnly":true,"temporaryDeaf":false}));
        self.refresh_call();
        self.sync_receiver_settings();
        if previous.is_some() {self.voice_gateway().await?;}
        Ok(())
    }
    fn hold_failed_mic_test(&mut self,message:&str) {
        self.voice_lifecycle.mic_active=false;
        if !self.voice_channel.is_empty() {self.state.self_deaf=true;}
        self.data("mic-test",json!({"active":false,"pending":true,"cleanupFailed":true,"error":message,"level":0,"db":-100,"token":self.voice_lifecycle.mic_attempt,"localOnly":true,"temporaryDeaf":!self.voice_channel.is_empty()}));
        self.status(message);self.refresh_call();self.sync_receiver_settings();
    }
    fn refresh_call(&mut self) {
        self.state.stream_volume_percent=self.media_preferences.stream_volume_percent;
        self.voice_lifecycle.video_sessions.retain(|id,_|self.state.call_videos.iter().any(|video|video.id==*id));
        self.sync_private_call_snapshot(Instant::now());
        if self.state.voice_connected {
            self.state.call_status_label=if self.state.voice_encryption_ready {"Conectado"}
                else if self.call_people.keys().any(|id|id!=&self.me()) {"Transporte conectado · preparando cifrado de audio…"}
                else {"Transporte conectado · esperando participantes"}.into();
            if self.state.voice_encryption_ready&&!self.state.call_pending_invitees.is_empty(){self.state.call_status_label.push_str(" · Esperando participantes");}
        }
        let (connect, speak, video) = self.voice_permissions();
        self.data("voice-permissions", json!({"connect":connect,"speak":speak,"video":video}));
        self.state.camera_available &= video;
        self.state.screen_available &= video;
        self.state.call_channel_id = if let Some(next)=&self.voice_lifecycle.switching {next.channel.clone()} else if self.voice_channel.is_empty() && self.voice_lifecycle.request_pending {self.voice_lifecycle.request_channel.clone()} else {self.voice_channel.clone()};
        self.state.call_guild_id = if let Some(next)=&self.voice_lifecycle.switching {self.state.channels.iter().find(|item|item.id==next.channel).map(|_|self.state.selected_guild_id.clone()).unwrap_or_default()} else if self.voice_channel.is_empty() && self.voice_lifecycle.request_pending {self.voice_lifecycle.request_guild.clone()} else {self.voice_guild.clone()};
        self.state.call_instance_id = if self.state.call_channel_id.is_empty() {
            0
        } else {
            self.voice_attempt
        };
        let me = self.me();
        for (id, person) in &mut self.call_people {
            person.volume_percent = self.media_preferences.user_volumes.get(id).copied().unwrap_or(100.);
        }
        if let Some(p) = self.call_people.get_mut(&me) {
            p.volume_percent=self.media_preferences.input_volume as f32;
            p.self_mute = self.state.self_mute || self.state.self_deaf || !speak;
            p.self_deaf = self.state.self_deaf;
            p.self_video = self.state.camera_enabled;
            p.screen_sharing = self.state.screen_sharing;
            if p.self_mute {
                p.speaking = false;
            }
        }
        self.state.call_participants = self.call_people.values().cloned().collect();
        self.state.call_participants.sort_by(|a, b| {
            b.is_self
                .cmp(&a.is_self)
                .then(a.user.name.cmp(&b.user.name))
        });
        self.decorate_dms();
        self.publish();
    }
}
fn inventory_status(error: &str) -> &'static str {
    let error = error.to_ascii_lowercase();
    if ["access_denied", "access denied", "permission", "e_accessdenied", "0x80070005", "denegado"].iter().any(|part| error.contains(part)) {
        "permission_denied"
    } else { "error" }
}
fn inventory_part(result: &mut Value, part: &str, value: Result<Value, String>) {
    match value {
        Ok(values) if values.is_array() => {
            result[part] = values;
            result["states"][part] = json!({"state":if result[part].as_array().is_some_and(|v|v.is_empty()) {"empty"} else {"ready"}});
        }
        Ok(_) => { result[part] = json!([]); result["states"][part] = json!({"state":"error","error":"El host devolvió un inventario no válido."}); }
        Err(error) => { result[part] = json!([]); result["states"][part] = json!({"state":inventory_status(&error),"error":error}); }
    }
}
fn inventory_failure(error: &str) -> Value {
    let mut result = json!({});
    for part in ["input", "output", "cameras", "sources"] { inventory_part(&mut result, part, Err(error.into())); }
    result
}
fn apply_readiness(state: &mut AppSnapshot, value: &Value) {
    let kind = s(value, "state");
    state.voice_encryption_ready = kind == "connected" && b(value, "encryptionReady");
    state.voice_connected = state.voice_encryption_ready
        || b(value, "transportReady") && matches!(kind.as_str(), "waiting-peer" | "negotiating");
    state.voice_connecting = !state.voice_connected;
    state.screen_available = state.voice_encryption_ready;
    state.camera_available = state.voice_encryption_ready && b(value, "cameraAvailable");
    state.call_status_label = match kind.as_str() {
        "connected" if state.voice_encryption_ready => "Conectado",
        "waiting-peer" => "Conectado",
        "negotiating" if b(value, "transportReady") => "Conectado · preparando conexión cifrada…",
        "negotiating" | "connecting" => "Conectando voz…",
        "error" => "Error de conexión",
        _ => "Desconectada",
    }
    .into();
    if state.voice_connected && state.call_started_at.is_none() {
        state.call_started_at = Some(chrono::Utc::now().fixed_offset());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn guild_join_and_receive_controls_cross_gateway_and_fake_bridge_without_real_account() {
        let (mut c,_,mut replies)=target();
        c.voice_channel.clear();c.voice_session.clear();
        guild_permissions_fixture(&mut c,(1<<10)|(1<<20)|(1<<21)|(1<<9));
        c.state.channels.push(NavItem {id:"200".into(),name:"Offline voice".into(),kind:"voice".into(),..Default::default()});
        c.api=Some(Api::new("SYNTHETIC-OFFLINE-ONLY-NOT-A-USER-ACCOUNT".into()).unwrap());
        let (gateway,mut wire,task)=GatewayHandle::offline_recorder(c.api.as_ref().unwrap().safety());
        c.gateway=Some(gateway.clone());
        let active=Arc::new(Mutex::new(HashSet::<String>::new()));let resources=active.clone();
        let (bridge,calls)=MediaBridge::fake(move|command,args| {
            match command {
                "connectVoice"=> {assert_eq!(args["guildId"],"500");assert_eq!(args["channelId"],"200");Ok(json!({"session":10,"state":"connecting"}))},
                "mute"=>Ok(json!({})),
                "setVideoStreamVisible"=> {if args["visible"]==true {resources.lock().unwrap().insert("camera".into());}else{resources.lock().unwrap().remove("camera");}Ok(json!({}))},
                "watchStream"=> {assert_eq!(args["streamOwnerId"],"300");assert_eq!(args["selfMute"],true);resources.lock().unwrap().insert("screen".into());Ok(json!({"state":"connecting"}))},
                "stopWatchStream"=>{resources.lock().unwrap().remove("screen");Ok(json!({"state":"stopped"}))},
                _=>panic!("Synthetic call attempted an unexpected native operation: {command}"),
            }
        });c.media=Some(bridge);
        c.act(UiAction::id("voice","200")).await.unwrap();
        reply(&mut c,&mut replies).await.unwrap();reply(&mut c,&mut replies).await.unwrap();
        let join=tokio::time::timeout(Duration::from_secs(2),wire.recv()).await.unwrap().unwrap();assert_eq!(join["op"],4);assert_eq!(join["d"]["guild_id"],"500");assert_eq!(join["d"]["channel_id"],"200");
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"500","user_id":"100","channel_id":"200","session_id":"SYNTHETIC-SESSION"})).await.unwrap();
        c.voice_dispatch("VOICE_SERVER_UPDATE",&json!({"guild_id":"500","endpoint":"offline.discord.media","token":"SYNTHETIC-NOT-REAL"})).await.unwrap();
        reply(&mut c,&mut replies).await.unwrap();
        assert_eq!(calls.lock().unwrap().iter().filter(|command|command.as_str()=="connectVoice").count(),1);
        c.media_event(json!({"event":"voiceState","mediaSession":10,"state":"connected","encryptionReady":true,"transportReady":true})).await.unwrap();
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"500","user_id":"300","channel_id":"200","self_video":true,"self_stream":true,"member":{"user":{"id":"300","username":"Synthetic peer"}}})).await.unwrap();
        c.state.call_videos.push(CallVideo{id:20,user_id:"300".into(),kind:"camera".into(),status:"paused".into(),..Default::default()});
        let view=|kind:&str,visible:bool,attempt|UiAction {kind:"view-incoming-media".into(),id:"300".into(),channel_id:"200".into(),text:kind.into(),extra:json!({"visible":visible,"voiceAttempt":attempt}).to_string()};
        let attempt=c.voice_attempt;
        c.act(view("camera",true,attempt)).await.unwrap();
        wait_named_calls(&calls,"setVideoStreamVisible",1).await;
        while let Ok(done)=replies.try_recv(){c.complete(done.action,done.result.unwrap()).await.unwrap();}
        assert!(active.lock().unwrap().contains("camera"));
        c.act(view("camera",false,attempt)).await.unwrap();wait_named_calls(&calls,"setVideoStreamVisible",2).await;
        while let Ok(done)=replies.try_recv(){c.complete(done.action,done.result.unwrap()).await.unwrap();}
        assert!(active.lock().unwrap().is_empty());
        let receive_resources=active.clone();
        let (receiver,receive_calls)=MediaBridge::fake_receiver(move|command,args|match command {
            "watchStream"=>{assert_eq!(args["streamOwnerId"],"300");receive_resources.lock().unwrap().insert("screen".into());Ok(json!({"session":50}))},
            "stopWatchStream"=>{receive_resources.lock().unwrap().remove("screen");Ok(json!({}))},
            "shutdown"=>Ok(json!({})),_=>panic!("Unexpected receiver operation: {command}"),
        },c.media.as_ref().unwrap().video_frames());c.voice_lifecycle.receiver=Some(receiver);
        c.act(view("screen",true,attempt)).await.unwrap();
        let watch=loop {let payload=tokio::time::timeout(Duration::from_secs(2),wire.recv()).await.unwrap().unwrap();if payload["op"]==20 {break payload;}};
        let key="guild:500:200:300";assert_eq!(watch["d"]["stream_key"],key);
        c.voice_dispatch("STREAM_CREATE",&json!({"stream_key":key,"rtc_server_id":"600","rtc_channel_id":"601"})).await.unwrap();
        c.voice_dispatch("STREAM_SERVER_UPDATE",&json!({"stream_key":key,"endpoint":"offline.discord.media","token":"SYNTHETIC-NOT-REAL"})).await.unwrap();
        wait_named_calls(&receive_calls,"watchStream",1).await;
        while let Ok(done)=replies.try_recv(){c.complete(done.action,done.result.unwrap()).await.unwrap();}
        assert!(active.lock().unwrap().contains("screen"));
        c.act(view("screen",false,attempt)).await.unwrap();wait_named_calls(&receive_calls,"stopWatchStream",1).await;
        while let Ok(done)=replies.try_recv(){c.complete(done.action,done.result.unwrap()).await.unwrap();}
        assert!(active.lock().unwrap().is_empty());assert!(c.voice_lifecycle.watches.is_empty());
        assert!(c.state.voice_connected);assert!(!c.state.screen_sharing);
        assert!(!calls.lock().unwrap().iter().any(|command|matches!(command.as_str(),"watchStream"|"stopWatchStream"|"shutdown")));
        while let Ok(payload)=wire.try_recv(){assert_ne!(payload["op"],19,"Stopping a receiver must not delete the sender's stream");}
        c.stop_voice().await;assert!(c.state.call_videos.is_empty()&&c.media.is_none());
        gateway.stop();tokio::time::timeout(Duration::from_secs(1),task).await.unwrap().unwrap();
    }
    #[test]
    fn stream_viewer_metadata_is_bounded_deduplicated_silent_on_seed_and_respects_dnd() {
        let (mut c,updates,_)=target();c.state.screen_sharing=true;
        let key="call:200:100";
        for viewers in [json!(["300"]),json!(["300"]),json!(["300","400"]),json!(["400"])] {
            c.observe_stream_viewers(&json!({"stream_key":key,"viewer_ids":viewers}));
        }
        let sounds:Vec<_>=updates.try_iter().filter_map(|update|match update {UiUpdate::Data(kind,value) if kind=="sound"=>Some(s(&value,"event")),_=>None}).collect();
        assert_eq!(sounds,["stream_viewer_joined","stream_viewer_left"]);
        c.state.self_presence="dnd".into();c.observe_stream_viewers(&json!({"stream_key":key,"viewer_ids":[]}));
        assert!(!updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,_) if kind=="sound")));
        c.observe_stream_viewers(&json!({"stream_key":key}));assert!(c.voice_lifecycle.stream_viewers[key].is_empty());
        c.observe_stream_viewers(&json!({"stream_key":key,"viewer_ids":(1..=400).map(|n|json!(n.to_string())).collect::<Vec<_>>()}));
        assert_eq!(c.voice_lifecycle.stream_viewers[key].len(),256);
    }
    fn guild_permissions_fixture(c: &mut Controller, bits: u64) {
        c.voice_guild = "500".into();
        c.state.selected_guild_id = "500".into();
        c.guild_raw = json!({"id":"500","roles":[{"id":"500","permissions":bits.to_string()}]});
        c.guild_acl = Some(GuildAccess::from_values(&c.guild_raw,&json!({}),&c.me()));
        c.channels.insert("200".into(),json!({"id":"200","guild_id":"500","type":2,"permission_overwrites":[]}));
    }
    #[test]
    fn guild_voice_permissions_gate_connect_speak_video_without_changing_user_mute() {
        let (mut c,_,_) = target();
        for (bits,expected) in [(0,(false,false,false)),((1<<10)|(1<<21)|(1<<9),(false,false,false)),((1<<10)|(1<<20),(true,false,false)),((1<<10)|(1<<20)|(1<<21),(true,true,false)),((1<<10)|(1<<20)|(1<<21)|(1<<9),(true,true,true))] {
            guild_permissions_fixture(&mut c,bits);
            assert_eq!(c.voice_permissions(),expected);
            assert_eq!(c.effective_voice_mute(),!expected.1);
            assert!(!c.state.self_mute);
            assert_eq!(c.require_voice_video().is_ok(),expected.2);
        }
        c.retain_active_voice_permissions();
        c.state.selected_guild_id = "900".into(); c.guild_acl = None; c.channels.clear();
        assert_eq!(c.voice_permissions(),(true,true,true));
    }
    #[test]
    fn retained_guild_voice_acl_is_scoped_and_navigation_keeps_the_latest_confirmed_permissions() {
        let (mut c,_,_)=target();let allowed=(1<<10)|(1<<20)|(1<<21)|(1<<9);
        guild_permissions_fixture(&mut c,allowed);c.retain_active_voice_permissions();
        c.state.selected_guild_id="900".into();c.guild_acl=None;c.channels.clear();
        assert_eq!(c.voice_permissions(),(true,true,true));
        c.voice_attempt+=1;assert_eq!(c.voice_permissions(),(false,false,false));c.voice_attempt-=1;
        c.state.current_user.as_mut().unwrap().id="101".into();assert_eq!(c.voice_permissions(),(false,false,false));
        c.state.current_user.as_mut().unwrap().id="100".into();
        c.voice_guild="900".into();assert_eq!(c.voice_permissions(),(false,false,false));c.voice_guild="500".into();
        c.voice_channel="201".into();assert_eq!(c.voice_permissions(),(false,false,false));c.voice_channel="200".into();
        // A later confirmed revocation must replace the original join ACL before
        // leaving the guild, rather than resurfacing the old grant from cache.
        guild_permissions_fixture(&mut c,1<<10);c.retain_active_voice_permissions();
        c.state.selected_guild_id.clear();c.guild_acl=None;c.channels.clear();
        assert_eq!(c.voice_permissions(),(false,false,false));
    }
    #[tokio::test]
    async fn leaving_for_dm_group_or_another_guild_and_returning_does_not_rejoin_or_close_active_voice() {
        for destination in ["dm","group","guild"] {
            let (mut c,_,_)=target();let allowed=(1<<10)|(1<<20)|(1<<21)|(1<<9);
            guild_permissions_fixture(&mut c,allowed);c.state.voice_connected=true;c.state.voice_encryption_ready=true;c.voice_started=true;
            c.voice_lifecycle.native_voice=Some(77);c.voice_lifecycle.last_permissions=Some((true,true,true));
            c.state.guilds=vec![NavItem{id:"500".into(),..Default::default()},NavItem{id:"900".into(),..Default::default()}];
            c.state.direct_messages=vec![NavItem{id:"700".into(),kind:"dm".into(),..Default::default()},NavItem{id:"800".into(),kind:"group".into(),..Default::default()}];
            // Navigation uses its real mutation path, while its unrelated HTTP
            // reads are prohibited before transport by this fixture-only gate.
            let api=Api::new("SYNTHETIC-OFFLINE-ONLY-NOT-A-USER-ACCOUNT".into()).unwrap();
            api.safety().block(SafetyState::SynchronizationRequired);c.api=Some(api);
            let (gateway,mut wire,task)=GatewayHandle::offline_recorder(notdiscord_core::safety::SafetyGate::default());c.gateway=Some(gateway.clone());
            let (media,calls)=MediaBridge::fake(|command,_|panic!("Navigation touched native media: {command}"));c.media=Some(media.clone());
            match destination {"dm"=>c.select_channel("700").unwrap(),"group"=>c.select_channel("800").unwrap(),_=>c.select_guild("900").unwrap()}
            c.voice_lifecycle.acl_checked_at=None;c.resume_pending_views().await.unwrap();
            c.select_guild("500").unwrap();assert!(c.guild_load.is_loading("500"));
            assert!(c.guild_acl.is_none()&&c.channels.is_empty());
            c.voice_lifecycle.acl_checked_at=None;c.resume_pending_views().await.unwrap();
            // An unsuccessful catalog read also is not an explicit revocation.
            c.complete_failed(&UiAction::id("__guild","500")).await;
            c.voice_lifecycle.acl_checked_at=None;c.resume_pending_views().await.unwrap();
            c.guild_load.clear();guild_permissions_fixture(&mut c,allowed);
            c.voice_lifecycle.acl_checked_at=None;c.resume_pending_views().await.unwrap();
            assert!(c.state.voice_connected&&c.state.voice_encryption_ready&&c.voice_started,"{destination}");
            assert_eq!((c.voice_channel.as_str(),c.voice_guild.as_str(),c.voice_attempt,c.voice_lifecycle.native_voice),("200","500",7,Some(77)));
            assert!(c.media.as_ref().is_some_and(|active|active.same_instance(&media)));
            assert!(calls.lock().unwrap().is_empty());
            while let Ok(frame)=wire.try_recv() {assert_ne!(frame["op"],4,"navigation must not leave or rejoin the voice channel");}
            c.gateway=None;gateway.stop();task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn a_fresh_voice_catalog_revocation_still_closes_the_retained_session() {
        for revoke in ["connect","deleted","text","other-guild"] {
            let (mut c,_,_)=target();let allowed=(1<<10)|(1<<20)|(1<<21)|(1<<9);
            guild_permissions_fixture(&mut c,allowed);c.retain_active_voice_permissions();
            c.state.voice_connected=true;c.state.voice_encryption_ready=true;c.voice_started=true;
            c.voice_lifecycle.native_voice=Some(77);c.voice_lifecycle.last_permissions=Some((true,true,true));
            let (gateway,mut wire,task)=GatewayHandle::offline_recorder(notdiscord_core::safety::SafetyGate::default());c.gateway=Some(gateway.clone());
            let (media,calls)=MediaBridge::fake(|command,_|panic!("Revocation cannot start new media: {command}"));c.media=Some(media.clone());
            c.guild_acl=None;c.channels.clear();c.guild_load.begin("500");
            c.resume_pending_views().await.unwrap();assert!(c.state.voice_connected);
            c.guild_load.clear();guild_permissions_fixture(&mut c,if revoke=="connect" {1<<10}else{allowed});
            match revoke {"deleted"=>{c.channels.remove("200");},"text"=>c.channels.get_mut("200").unwrap()["type"]=json!(0),
                "other-guild"=>c.channels.get_mut("200").unwrap()["guild_id"]=json!("900"),_=>{}}
            c.voice_lifecycle.acl_checked_at=None;
            assert!(c.resume_pending_views().await.is_err());
            assert!(!c.state.voice_connected&&c.voice_channel.is_empty()&&c.media.is_none());
            let leave=tokio::time::timeout(Duration::from_secs(1),wire.recv()).await.unwrap().unwrap();
            assert_eq!(leave["op"],4);assert!(leave["d"]["channel_id"].is_null());
            assert!(media.command("must-remain-closed",json!({})).await.is_err());assert!(calls.lock().unwrap().is_empty());
            c.gateway=None;gateway.stop();task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn guild_voice_participants_seed_update_leave_and_move_without_hardware() {
        let (mut c,_,_) = target();
        guild_permissions_fixture(&mut c,(1<<10)|(1<<20)|(1<<21)|(1<<9));
        c.voice_lifecycle.acl_checked_at = Some(Instant::now());
        c.guild_raw["voice_states"] = json!([{"user_id":"300","channel_id":"200","self_video":true}]);
        c.seed_guild_call_people(); assert!(c.call_people["300"].self_video);
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"500","user_id":"301","channel_id":"200","self_stream":true,"member":{"user":{"id":"301","username":"Peer"}}})).await.unwrap();
        assert_eq!(c.call_people["301"].user.name,"Peer");
        assert!(c.call_people["301"].screen_sharing);
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"900","user_id":"301","channel_id":"901"})).await.unwrap();
        assert!(c.call_people.contains_key("301"),"unrelated guild cannot remove current participant");
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"500","user_id":"301","channel_id":"201"})).await.unwrap();
        assert!(!c.call_people.contains_key("301"));
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"500","user_id":"300","channel_id":null})).await.unwrap();
        c.call_people.clear(); c.seed_guild_call_people();
        assert!(c.call_people.is_empty(),"observed leaves/moves supersede old guild snapshot");
        assert!(c.media.is_none());
    }
    #[tokio::test]
    async fn device_inventory_is_partial_and_refresh_does_not_reset_settings_or_start_capture() {
        let (mut c,updates,mut replies) = target();
        let (bridge,calls)=MediaBridge::fake(|command,_| match command {
            "listDevices"=>Ok(json!({"input":[{"id":"i","name":"Mic"}],"output":[]})),
            "listCameras"=>Err("E_ACCESSDENIED 0x80070005".into()),
            "listSources"=>Ok(json!([{"id":"window:1","name":"Fixture","type":"window"}])),
            _=>panic!("unexpected capture command {command}"),
        });
        c.media=Some(bridge);
        c.media_action(&UiAction::new("list-devices")).await.unwrap();
        c.media_action(&UiAction::new("list-devices")).await.unwrap();
        let done=tokio::time::timeout(Duration::from_secs(1),replies.recv()).await.unwrap().unwrap();
        // A concurrent join no longer invalidates an inventory request.
        c.voice_attempt+=1;
        c.voice_complete(done.action,done.result.unwrap()).await.unwrap();
        let values: Vec<_> = updates.try_iter().collect();
        let devices=values.iter().find_map(|update| if let UiUpdate::Data(kind,value)=update { (kind=="devices").then_some(value) } else {None}).unwrap();
        assert_eq!(devices["states"]["input"]["state"],"ready");
        assert_eq!(devices["states"]["output"]["state"],"empty");
        assert_eq!(devices["states"]["cameras"]["state"],"permission_denied");
        assert_eq!(devices["sources"][0]["id"],"window:1");
        assert!(!values.iter().any(|update|matches!(update,UiUpdate::Data(kind,_) if kind=="media-settings")));
        assert_eq!(calls.lock().unwrap().len(),3);
    }
    #[test]
    fn inventory_distinguishes_invalid_payload_and_absent_devices() {
        let mut result=json!({});
        inventory_part(&mut result,"cameras",Ok(json!([])));
        assert_eq!(result["states"]["cameras"]["state"],"empty");
        inventory_part(&mut result,"cameras",Ok(json!({})));
        assert_eq!(result["states"]["cameras"]["state"],"error");
        assert_eq!(inventory_failure("timeout")["states"]["sources"]["state"],"error");
    }
    #[test]
    fn projected_voice_metadata_updates_seed_without_transport_secrets() {
        let (mut c,_,_) = target();
        guild_permissions_fixture(&mut c,(1<<10)|(1<<20));
        c.observe_guild_voice_metadata(&json!({"guild_voice_states":[{"guild_id":"500","partial":false,"states":[{"user_id":"300","channel_id":"200","self_video":true,"session_id":"DO-NOT-KEEP"}]}]}));
        assert_eq!(c.voice_lifecycle.observed_guild_voice.len(),1);
        assert!(!c.voice_lifecycle.observed_guild_voice.values().next().unwrap().to_string().contains("DO-NOT-KEEP"));
        c.seed_guild_call_people(); assert!(c.call_people["300"].self_video);
        c.observe_guild_voice_metadata(&json!({"guild_voice_states":[{"guild_id":"500","partial":false,"states":[]}]}));
        assert!(c.voice_lifecycle.observed_guild_voice.is_empty());
    }
    #[tokio::test]
    async fn ptt_sounds_and_commands_only_follow_effective_transitions() {
        let (mut c,updates,_) = target();
        c.state.voice_connected=true;
        let (bridge,calls)=MediaBridge::fake(|_,_|Ok(json!({})));
        c.media=Some(bridge);
        c.update_ptt(false); c.update_ptt(true); c.update_ptt(true);
        c.update_ptt(false); c.update_ptt(false);
        wait_calls(&calls,2).await;
        assert_eq!(calls.lock().unwrap().len(),2);
        let sounds=updates.try_iter().filter(|value|matches!(value,UiUpdate::Data(kind,_) if kind=="sound")).count();
        assert_eq!(sounds,2);
        c.state.self_mute=true; c.update_ptt(true);
        assert!(!c.voice_lifecycle.ptt_effective);
    }
    #[tokio::test]
    async fn slow_initial_inventory_never_replaces_an_open_settings_draft() {
        let (mut c,updates,_) = target();
        c.voice_lifecycle.inventory_request=3;
        c.voice_lifecycle.inventory_pending=true;
        let mut action=UiAction::id("__voice-data","devices");
        action.extra=c.meta().to_string(); action.channel_id="3".into(); action.text="media-settings".into();
        c.voice_complete(action,wrapped(Ok(json!({"cameras":[]})))).await.unwrap();
        assert!(!c.voice_lifecycle.inventory_pending);
        let values:Vec<_>=updates.try_iter().collect();
        assert!(values.iter().any(|value|matches!(value,UiUpdate::Data(kind,_) if kind=="devices")));
        assert!(!values.iter().any(|value|matches!(value,UiUpdate::Data(kind,_) if kind=="media-settings")));
    }
    #[tokio::test]
    async fn participant_updates_honor_global_gallery_cap_and_keep_existing_people_fresh() {
        let (mut c,_,_) = target();
        for id in 1..=256 { c.call_people.insert(id.to_string(),CallParticipant{user:Person{id:id.to_string(),..Default::default()},..Default::default()}); }
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"999","channel_id":"200"})).await.unwrap();
        assert_eq!(c.call_people.len(),256); assert!(!c.call_people.contains_key("999"));
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"1","channel_id":"200","self_video":true})).await.unwrap();
        assert!(c.call_people["1"].self_video);
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"2","channel_id":null})).await.unwrap();
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"999","channel_id":"200"})).await.unwrap();
        assert_eq!(c.call_people.len(),256); assert!(c.call_people.contains_key("999"));
    }
    #[tokio::test]
    async fn held_ptt_before_connection_only_activates_after_transport_ready() {
        let (mut c,_,_) = target();
        let (bridge,calls)=MediaBridge::fake(|_,_|Ok(json!({})));
        c.media=Some(bridge);
        c.poll_ptt(true); assert!(!c.last_ptt && !c.voice_lifecycle.ptt_effective);
        assert!(calls.lock().unwrap().is_empty());
        c.state.voice_connected=true; c.poll_ptt(true);
        wait_calls(&calls,1).await;
        assert!(c.last_ptt && c.voice_lifecycle.ptt_effective);
        c.poll_ptt(true); assert_eq!(calls.lock().unwrap().len(),1);
    }
    #[tokio::test]
    async fn failed_ptt_release_or_permission_mute_closes_transport_without_changing_preference() {
        for id in ["ptt","permission-mute"] {
            let (mut c,_,_) = target();
            let (bridge,_)=MediaBridge::fake(|_,_|Ok(json!({})));
            c.media=Some(bridge); c.state.voice_connected=true;
            c.voice_lifecycle.ptt_effective=true;
            let mut action=UiAction::id("__voice-data",id); action.extra=c.meta().to_string();
            c.voice_complete(action,wrapped(Err("Synthetic native failure".into()))).await.unwrap_err();
            assert!(c.media.is_none() && c.voice_channel.is_empty() && !c.state.voice_connected);
            assert!(!c.state.self_mute,"do not overwrite a personal preference when closing transport");
            assert!(!c.voice_lifecycle.ptt_effective);
        }
    }
    #[tokio::test]
    async fn guild_acl_does_not_send_mute_before_native_voice_exists() {
        let (mut c,_,_) = target();
        guild_permissions_fixture(&mut c,(1<<10)|(1<<20));
        let (bridge,calls)=MediaBridge::fake(|_,_|Ok(json!({})));
        c.media=Some(bridge);
        c.resume_pending_views().await.unwrap();
        assert!(calls.lock().unwrap().is_empty());
        assert!(c.voice_lifecycle.last_permissions.is_none());
        assert!(!c.voice_channel.is_empty());
        c.state.voice_connected=true; c.voice_lifecycle.acl_checked_at=None;
        c.resume_pending_views().await.unwrap();
        wait_calls(&calls,1).await;
        assert_eq!(c.voice_lifecycle.last_permissions,Some((true,false,false)));
    }
    #[tokio::test]
    async fn stopping_or_closing_mic_settings_never_loses_a_ptt_release_or_resume() {
        let (mut c,_,_) = target(); c.state.voice_connected=true;
        let (bridge,calls)=MediaBridge::fake(|_,_|Ok(json!({}))); c.media=Some(bridge);
        c.poll_ptt(true); wait_calls(&calls,1).await;
        c.finish_mic_test().await.unwrap();
        assert!(c.last_ptt && c.voice_lifecycle.ptt_effective,"closing an idle mic test must retain the held key edge");
        c.poll_ptt(false); wait_calls(&calls,2).await;
        assert!(!c.last_ptt && !c.voice_lifecycle.ptt_effective);
        c.last_ptt=true; c.voice_lifecycle.ptt_effective=true;
        c.voice_lifecycle.mic_prior_deaf=Some(false); c.state.self_deaf=true;
        c.finish_mic_test().await.unwrap();
        assert!(!c.last_ptt && !c.voice_lifecycle.ptt_effective,"an actual test resets native PTT mode");
        c.poll_ptt(true); wait_calls(&calls,3).await;
        assert!(c.last_ptt && c.voice_lifecycle.ptt_effective);
    }
    #[tokio::test]
    async fn forced_own_voice_move_closes_old_transport_and_capture_without_auto_join_or_duplicates() {
        let (mut c,updates,_) = target();
        guild_permissions_fixture(&mut c,(1<<10)|(1<<20)|(1<<21)|(1<<9));
        c.state.voice_connected=true; c.state.voice_encryption_ready=true;
        c.state.camera_enabled=true; c.state.screen_sharing=true;
        c.state.self_mute=true; c.state.self_deaf=false;
        let (bridge,calls)=MediaBridge::fake(|command,_| {
            assert!(!matches!(command,"connectVoice"|"startCamera"|"startShare"),"a remote move cannot auto-join or broadcast");
            Ok(json!({}))
        });
        c.media=Some(bridge);
        let moved=json!({"guild_id":"500","user_id":"100","channel_id":"201","session_id":"synthetic-new-session"});
        c.voice_dispatch("VOICE_STATE_UPDATE",&moved).await.unwrap();
        assert!(c.voice_channel.is_empty() && c.media.is_none());
        assert!(!c.state.voice_connected && !c.state.voice_encryption_ready && !c.state.voice_connecting);
        assert!(!c.state.camera_enabled && !c.state.screen_sharing);
        assert!(c.state.self_mute && !c.state.self_deaf);
        assert!(c.state.status.contains("Unirse") && c.state.status.contains("permisos"));
        assert_eq!(c.state.call_move_version,1);
        c.voice_dispatch("VOICE_STATE_UPDATE",&moved).await.unwrap();
        assert_eq!(c.state.call_move_version,1);
        let sounds=updates.try_iter().filter(|value|matches!(value,UiUpdate::Data(kind,payload) if kind=="sound" && payload["event"]=="user_move")).count();
        assert_eq!(sounds,1);
        assert!(calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn accepting_an_old_instance_or_replaced_preflight_never_starts_another_call() {
        let (mut c, _, _) = target();
        incoming_view_fixture(&mut c);
        c.voice_channel.clear();
        let mut action = UiAction::id("join-call", "200");
        action.extra = json!({"instanceId":11}).to_string();
        assert!(c.act(action).await.unwrap_err().contains("cambió"));
        assert!(!c.voice_lifecycle.request_pending && c.media.is_none());
        c.voice_lifecycle.request_pending = true;
        c.voice_lifecycle.request_channel = "200".into();
        c.voice_lifecycle.join_only = true;
        c.voice_lifecycle.request_instance = 11;
        let mut action = UiAction::id("__voice-preflight", "200");
        action.extra = c.voice_attempt.to_string();
        assert!(
            c.voice_complete(action, wrapped(Ok(json!({"ringable":false}))))
                .await
                .unwrap_err()
                .contains("terminó")
        );
        assert!(
            !c.voice_lifecycle.request_pending && c.media.is_none() && c.voice_channel.is_empty()
        );
    }
    #[tokio::test]
    async fn private_incoming_call_can_resolve_cold_navigation_without_granting_guild_access() {
        let (mut c, _) = Controller::test_fixture();
        c.state.incoming_calls.push(IncomingCall {
            channel_id: "200".into(),
            channel_name: "Synthetic group".into(),
            kind: "group".into(),
            ..Default::default()
        });
        let (item, dm) = c.voice_request_item("200", true).unwrap();
        assert!(dm && item.access.is_none());
        assert_eq!(item.kind, "group");
        assert!(c.voice_request_item("200", false).is_none());
        c.state.incoming_calls[0].kind = "voice".into();
        assert!(c.voice_request_item("200", true).is_none());
        c.state.incoming_calls[0].kind.clear();
        assert!(c.voice_request_item("200", true).is_none());
        c.state.incoming_calls.clear();
        assert!(c.voice_request_item("200", true).is_none());
        assert!(
            c.request_voice("200", false, true)
                .unwrap_err()
                .contains("ya terminó")
        );
        assert!(c.media.is_none() && c.voice_channel.is_empty());
    }
    fn view_action(kind: &str, visible: bool, instance: u64) -> UiAction {
        UiAction {
            kind: "view-incoming-media".into(),
            id: "300".into(),
            text: kind.into(),
            channel_id: "200".into(),
            extra: json!({"visible":visible,"instanceId":instance}).to_string(),
        }
    }
    fn incoming_view_fixture(c: &mut Controller) {
        let person = CallParticipant {
            user: Person {
                id: "300".into(),
                name: "Synthetic peer".into(),
                ..Default::default()
            },
            self_video: true,
            screen_sharing: true,
            ..Default::default()
        };
        c.state.incoming_calls.push(IncomingCall {
            channel_id: "200".into(),
            instance_id: 12,
            participants: vec![person.user.clone()],
            call_participants: vec![person.clone()],
            ..Default::default()
        });
        c.call_people.insert("300".into(), person);
    }
    #[tokio::test]
    async fn viewing_during_join_preserves_current_audio_choices_and_hiding_does_not_leave() {
        for mute in [false, true] {
            for deaf in [false, true] {
                let (mut c, _, _) = target();
                incoming_view_fixture(&mut c);
                c.voice_channel.clear();
                c.state.self_mute = mute;
                c.state.self_deaf = deaf;
                c.voice_lifecycle.request_pending = true;
                c.voice_lifecycle.request_channel = "200".into();
                c.voice_lifecycle.join_only = true;
                let before = std::fs::read(c.data.join("audio-settings.json")).ok();
                let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!({})));
                c.media = Some(bridge);
                c.act(view_action("camera", true, 12)).await.unwrap();
                assert!(c.voice_lifecycle.pending_views.contains_key("200:300"));
                assert_eq!((c.state.self_mute, c.state.self_deaf), (mute, deaf));
                c.state.self_mute = !mute;
                c.state.self_deaf = !deaf; // Changes while connecting stay authoritative.
                c.act(view_action("camera", false, 12)).await.unwrap();
                assert!(
                    c.voice_lifecycle.pending_views.is_empty() && c.voice_lifecycle.request_pending
                );
                assert!(c.voice_channel.is_empty() && calls.lock().unwrap().is_empty());
                assert_eq!((c.state.self_mute, c.state.self_deaf), (!mute, !deaf));
                assert_eq!(
                    std::fs::read(c.data.join("audio-settings.json")).ok(),
                    before
                );
                c.voice_channel = "200".into();
                match c.voice_state_update(false) {
                    GatewayCommand::VoiceState {
                        channel_id,
                        mute: wire_mute,
                        deaf: wire_deaf,
                        video,
                        ..
                    } => {
                        assert_eq!(channel_id.as_deref(), Some("200"));
                        assert_eq!(wire_mute, !mute || !deaf);
                        assert_eq!(wire_deaf, !deaf);
                        assert!(!video);
                    }
                    _ => panic!("Expected real join command"),
                }
            }
        }
    }
    #[tokio::test]
    async fn explicit_camera_waits_for_dave_then_toggles_only_its_sink() {
        let (mut c, _, mut rx) = target();
        incoming_view_fixture(&mut c);
        let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
        let recorded = seen.clone();
        let (bridge, calls) = MediaBridge::fake(move |command, args| {
            assert_eq!(command, "setVideoStreamVisible");
            recorded.lock().unwrap().push(args.clone());
            Ok(json!({}))
        });
        c.media = Some(bridge);
        c.state.call_videos = vec![
            CallVideo {
                id: 10,
                user_id: "300".into(),
                kind: "camera".into(),
                status: "paused".into(),
                ..Default::default()
            },
            CallVideo {
                id: 11,
                user_id: "400".into(),
                kind: "camera".into(),
                active: true,
                ..Default::default()
            },
        ];
        c.act(view_action("camera", true, 12)).await.unwrap();
        assert!(calls.lock().unwrap().is_empty());
        c.state.voice_encryption_ready = true;
        c.resume_pending_views().await.unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        assert_eq!(*seen.lock().unwrap(), [json!({"id":10,"visible":true})]);
        c.act(view_action("camera", false, 12)).await.unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        assert_eq!(seen.lock().unwrap()[1], json!({"id":10,"visible":false}));
        assert_eq!(c.voice_channel, "200");
        assert!(c.state.call_videos[1].active);
    }
    #[tokio::test]
    async fn pending_media_rejects_old_instances_and_removed_sources_without_capturing() {
        let (mut c, _, _) = target();
        incoming_view_fixture(&mut c);
        c.voice_channel.clear();
        c.voice_lifecycle.request_pending = true;
        c.voice_lifecycle.request_channel = "200".into();
        c.act(view_action("screen", true, 12)).await.unwrap();
        c.act(view_action("camera", false, 12)).await.unwrap(); // Late hide for previous kind cannot cancel screen.
        assert_eq!(c.voice_lifecycle.pending_views["200:300"].kind, "screen");
        c.state.incoming_calls[0].call_participants[0].screen_sharing = false;
        c.resume_pending_views().await.unwrap();
        assert!(c.voice_lifecycle.pending_views.is_empty());
        assert!(c.state.status.contains("dejó"));
        c.state.incoming_calls[0].instance_id = 13;
        assert!(
            c.act(view_action("camera", true, 12))
                .await
                .unwrap_err()
                .contains("cambió")
        );
        assert!(c.media.is_none() && c.voice_channel.is_empty());
    }
    #[tokio::test]
    async fn failed_watch_releases_only_matching_attempt_and_can_start_again() {
        let (mut c, _, mut rx) = target();
        c.state.voice_encryption_ready = true;
        let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = starts.clone();
        let (main,_)=MediaBridge::fake(|_,_|Ok(json!({})));c.media=Some(main.clone());
        let (bridge, calls) = MediaBridge::fake_receiver(move |command, _| {
            if command == "watchStream"
                && count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0
            {
                Err("synthetic watch rejection".into())
            } else {
                Ok(json!({}))
            }
        },main.video_frames());
        c.voice_lifecycle.receiver = Some(bridge);
        let key = "call:200:300";
        let mut info = json!({"owner":"300","server":"400","channel":"401","endpoint":"synthetic.discord.media","token":"SYNTHETIC-NOT-REAL","attempt":1,"started":false});
        c.voice_lifecycle.watches.insert(key.into(), info.clone());
        c.try_watch(key).await.unwrap();
        assert!(reply(&mut c, &mut rx).await.is_err());
        wait_calls(&calls, 2).await;
        assert!(!c.voice_lifecycle.watches.contains_key(key));
        info["attempt"] = json!(3);
        let count=starts.clone();let (next,_)=MediaBridge::fake_receiver(move|command,_| {if command=="watchStream" {count.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}Ok(json!({}))},main.video_frames());c.voice_lifecycle.receiver=Some(next);
        c.voice_lifecycle.watches.insert(key.into(), info);
        c.try_watch(key).await.unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert!(c.voice_lifecycle.watches[key]["started"] == true);
        let mut old = UiAction::id("__voice-data", "watch-start");
        old.channel_id = key.into();
        old.text = "1".into();
        old.extra = c.meta().to_string();
        c.voice_failed(&old).await;
        assert!(c.voice_lifecycle.watches.contains_key(key));
    }
    #[tokio::test]
    async fn local_preview_requires_explicit_command_and_never_starts_broadcast_before_dave() {
        let (mut c, _, mut rx) = target();
        let seen = Arc::new(Mutex::new(Value::Null));
        let capture = seen.clone();
        let (bridge, calls) = MediaBridge::fake(move |command, args| {
            assert_eq!(command, "startCameraPreview");
            *capture.lock().unwrap() = args.clone();
            Ok(json!({"localOnly":true}))
        });
        c.media = Some(bridge);
        assert!(!c.state.voice_encryption_ready);
        assert!(calls.lock().unwrap().is_empty());
        c.media_action(&UiAction::id("start-camera-preview", "explicit-camera"))
            .await
            .unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        assert_eq!(*calls.lock().unwrap(), ["startCameraPreview"]);
        assert_eq!(seen.lock().unwrap()["deviceId"], "explicit-camera");
        assert!(!c.state.camera_enabled && !c.state.self_deaf);
    }
    #[tokio::test]
    async fn stop_preview_cancels_an_explicit_selection_queued_before_host_start() {
        let (mut c, _, _) = target();
        c.voice_lifecycle
            .resume
            .push_back(UiAction::id("start-camera-preview", "explicit-camera"));
        c.media_action(&UiAction::new("stop-camera-preview"))
            .await
            .unwrap();
        assert!(c.voice_lifecycle.resume.is_empty());
        assert!(c.media.is_none());
    }
    #[tokio::test]
    async fn watching_remote_screen_never_reuses_outgoing_capture_or_accepts_stale_stop() {
        let (mut c, _, mut rx) = target();
        c.state.voice_encryption_ready = true;
        let args = Arc::new(Mutex::new(Value::Null));
        let capture = args.clone();
        let (main,_)=MediaBridge::fake(|_,_|Ok(json!({})));c.media=Some(main.clone());
        let (bridge, calls) = MediaBridge::fake_receiver(move |command, value| {
            assert_eq!(command, "watchStream");
            *capture.lock().unwrap() = value.clone();
            Ok(json!({"session":11}))
        },main.video_frames());
        c.voice_lifecycle.receiver = Some(bridge);
        let key = "call:200:300";
        c.voice_lifecycle.watches.insert(key.into(),json!({"owner":"300","server":"400","channel":"401","endpoint":"synthetic.discord.media","token":"SYNTHETIC-NOT-REAL","attempt":4,"started":false}));
        c.try_watch(key).await.unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        assert_eq!(*calls.lock().unwrap(), ["watchStream"]);
        assert_eq!(args.lock().unwrap()["streamOwnerId"], "300");
        assert_eq!(args.lock().unwrap()["selfMute"], true);
        assert!(!c.state.screen_sharing && c.stream_source.is_empty());
        c.media_event(
            json!({"event":"watchState","streamKey":key,"token":3,"state":"disconnected"}),
        )
        .await
        .unwrap();
        assert!(c.voice_lifecycle.watches.contains_key(key));
    }
    fn target() -> (
        Controller,
        std::sync::mpsc::Receiver<UiUpdate>,
        mpsc::UnboundedReceiver<Completed>,
    ) {
        let (mut c, u, r) = Controller::test_fixture_with_replies();
        c.state.current_user = Some(Person {
            id: "100".into(),
            name: "Synthetic self".into(),
            ..Default::default()
        });
        c.state.signed_in = true;
        c.voice_channel = "200".into();
        c.voice_session = "synthetic-session".into();
        c.voice_attempt = 7;
        c.voice_lifecycle.media_epoch = 3;
        (c, u, r)
    }
    fn share(c: &mut Controller) {
        c.state.voice_encryption_ready = true;
        c.state.voice_connected = true;
        c.state.screen_preparing = true;
        c.stream_source = "window:123".into();
        c.stream_key = "call:200:100".into();
        c.stream_server = "300".into();
        c.stream_endpoint = "synthetic.invalid".into();
        c.stream_token = "SYNTHETIC-NOT-A-REAL-CREDENTIAL".into();
        c.stream_quality = json!({"height":720,"fps":30});
        c.voice_lifecycle.share_attempt = 1;
    }
    async fn wait_named_calls(calls: &Arc<Mutex<Vec<String>>>, command: &str, count: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if calls.lock().unwrap().iter().filter(|seen|seen.as_str()==command).count()>=count {break;}
                tokio::task::yield_now().await;
            }
        }).await.unwrap_or_else(|_|panic!("Expected {count} {command} calls, saw {:?}",calls.lock().unwrap()));
    }
    async fn wait_calls(calls: &Arc<Mutex<Vec<String>>>, count: usize) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if calls.lock().unwrap().len() >= count {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    async fn reply(
        c: &mut Controller,
        rx: &mut mpsc::UnboundedReceiver<Completed>,
    ) -> Result<(), String> {
        let done = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        c.complete(done.action, done.result.unwrap()).await
    }
    #[test]
    fn solo_and_peer_readiness_preserve_crypto_boundary_and_clock() {
        let mut state = AppSnapshot::default();
        apply_readiness(
            &mut state,
            &json!({"state":"waiting-peer","transportReady":true,"encryptionReady":false,"cameraAvailable":true}),
        );
        assert!(state.voice_connected);
        assert!(!state.voice_connecting);
        assert!(!state.voice_encryption_ready);
        assert!(!state.camera_available);
        assert!(!state.screen_available);
        assert_eq!(state.call_status_label,"Conectado");
        let started = state.call_started_at;
        assert!(started.is_some());
        apply_readiness(
            &mut state,
            &json!({"state":"negotiating","transportReady":true,"encryptionReady":false,"cameraAvailable":true}),
        );
        assert!(state.voice_connected);
        assert_eq!(state.call_started_at, started);
        assert!(!state.camera_available);
        apply_readiness(
            &mut state,
            &json!({"state":"connected","transportReady":true,"encryptionReady":true,"cameraAvailable":true}),
        );
        assert!(state.voice_encryption_ready && state.camera_available && state.screen_available);
        assert_eq!(state.call_started_at, started);
    }
    #[test]
    fn literal_connected_is_insufficient_and_reset_keeps_explicit_choices() {
        let mut state = AppSnapshot::default();
        state.self_mute = true;
        state.self_deaf = true;
        state.screen_sharing = true;
        state.screen_paused = true;
        apply_readiness(
            &mut state,
            &json!({"state":"waiting-peer","transportReady":true,"encryptionReady":false}),
        );
        assert!(state.self_mute && state.self_deaf && state.screen_sharing && state.screen_paused);
        assert!(!state.screen_available && !state.camera_available);
        apply_readiness(
            &mut state,
            &json!({"state":"connected","transportReady":true,"cameraAvailable":true}),
        );
        assert!(
            !state.voice_connected
                && !state.voice_encryption_ready
                && !state.camera_available
                && !state.screen_available
        );
        apply_readiness(
            &mut state,
            &json!({"state":"negotiating","transportReady":false,"encryptionReady":false}),
        );
        assert!(!state.voice_connected && state.voice_connecting);
        apply_readiness(&mut state, &json!({"state":"disconnected"}));
        assert!(!state.voice_connected && !state.voice_encryption_ready);
    }
    #[tokio::test]
    async fn terminated_join_does_not_even_consult_api_or_start_media() {
        let (mut c, _) = Controller::test_fixture();
        let error = c.request_voice("200", false, true).unwrap_err();
        assert!(error.contains("ya terminó"));
        assert!(
            c.api.is_none()
                && c.media.is_none()
                && c.voice_channel.is_empty()
                && !c.voice_lifecycle.request_pending
        );
    }
    #[tokio::test]
    async fn private_test_preflight_without_a_matching_fixture_fails_before_launching_any_http_or_gateway_work() {
        let (mut c,_,mut replies)=target();c.voice_channel.clear();
        c.api=Some(Api::new("SYNTHETIC-OFFLINE-ONLY-NOT-A-USER-ACCOUNT".into()).unwrap());
        let (gateway,mut wire,task)=GatewayHandle::offline_recorder(c.api.as_ref().unwrap().safety());c.gateway=Some(gateway.clone());
        c.state.direct_messages.push(NavItem{id:"201".into(),kind:"group".into(),..Default::default()});
        c.state.incoming_calls.push(IncomingCall{channel_id:"201".into(),instance_id:12,kind:"group".into(),ringing:true,..Default::default()});
        for fixture in [None,Some(("999".into(),json!({"ringable":false,"recipients":[]})))] {
            c.voice_lifecycle.preflight_fixture=fixture;
            let error=c.request_voice_scoped("201",false,true,12).unwrap_err();assert!(error.contains("no se permite HTTP real"));
            assert!(!c.voice_lifecycle.request_pending&&!c.state.voice_connecting&&c.media.is_none());
            assert_eq!(c.voice_attempt,7);assert!(wire.try_recv().is_err()&&replies.try_recv().is_err());
            assert!(c.state.incoming_calls[0].ringing,"failed test setup must not accept the invitation");
        }
        gateway.stop();task.await.unwrap();
    }
    #[tokio::test]
    async fn call_disappearing_during_preflight_cannot_create_replacement() {
        let (mut c, _, _) = target();
        c.voice_channel.clear();
        c.voice_lifecycle.request_pending = true;
        c.voice_lifecycle.request_channel = "200".into();
        c.voice_lifecycle.join_only = true;
        let mut action = UiAction::id("__voice-preflight", "200");
        action.extra = c.voice_attempt.to_string();
        action.text = json!({"isDm":true,"joinOnly":true}).to_string();
        assert!(
            c.voice_complete(action, wrapped(Ok(json!({"ringable":true}))))
                .await
                .unwrap_err()
                .contains("terminó")
        );
        assert!(c.media.is_none() && c.voice_channel.is_empty());
        assert!(!c.voice_lifecycle.request_pending && c.voice_lifecycle.pending_ring.is_empty());
    }
    #[tokio::test]
    async fn call_delete_invalidates_pending_open_and_completion() {
        let (mut c, _, _) = target();
        c.voice_channel.clear();
        c.voice_lifecycle.request_pending = true;
        c.voice_lifecycle.request_channel = "200".into();
        c.voice_lifecycle.join_only = true;
        let epoch = c.voice_lifecycle.media_epoch;
        let attempt = c.voice_attempt;
        c.voice_call_dispatch("CALL_DELETE", &json!({"channel_id":"200"}))
            .await
            .unwrap();
        assert!(c.voice_attempt > attempt && c.voice_lifecycle.media_epoch > epoch);
        let mut stale = UiAction::id("__voice-requested", "200");
        stale.extra = attempt.to_string();
        c.voice_complete(stale, wrapped(Ok(json!({}))))
            .await
            .unwrap();
        assert!(
            c.voice_channel.is_empty() && c.media.is_none() && !c.voice_lifecycle.request_pending
        );
    }
    #[test]
    fn ring_is_armed_only_for_matching_explicit_attempt_and_consumed_once() {
        let mut life = VoiceLifecycle::default();
        assert!(!life.take_ring("200", 1));
        life.pending_ring = "200".into();
        life.pending_ring_attempt = 2;
        assert!(!life.take_ring("200", 1));
        assert!(!life.take_ring("201", 2));
        assert!(life.take_ring("200", 2));
        assert!(!life.take_ring("200", 2));
        life.join_only = true;
        assert!(!life.can_join(false));
        assert!(life.can_join(true));
    }
    #[tokio::test]
    async fn stale_helper_event_cannot_disconnect_replacement() {
        let (mut c, _, _) = target();
        c.state.voice_connected = true;
        c.state.voice_encryption_ready = true;
        let mut action = UiAction::new("__media");
        action.extra = (c.voice_lifecycle.media_epoch - 1).to_string();
        c.complete(action, json!({"event":"voiceState","state":"disconnected"}))
            .await
            .unwrap();
        assert!(c.state.voice_connected && c.state.voice_encryption_ready);
        assert_eq!(c.voice_channel, "200");
    }
    #[tokio::test]
    async fn share_loss_before_dispatch_never_starts_native_capture() {
        let (mut c, _, _) = target();
        share(&mut c);
        let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        c.state.voice_encryption_ready = false;
        assert!(c.try_share().await.is_err());
        wait_calls(&calls, 1).await;
        assert_eq!(*calls.lock().unwrap(), ["stopShare"]);
        assert!(!c.state.screen_preparing && !c.state.screen_sharing && c.stream_source.is_empty());
    }
    #[tokio::test]
    async fn share_loss_during_start_cleans_same_attempt() {
        let (mut c, _, mut rx) = target();
        share(&mut c);
        let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!({"state":"connecting"})));
        c.media = Some(bridge);
        c.try_share().await.unwrap();
        c.media_event(json!({"event":"voiceState","state":"waiting-peer","transportReady":true,"encryptionReady":false})).await.unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        wait_calls(&calls, 2).await;
        assert_eq!(*calls.lock().unwrap(), ["startShare", "stopShare"]);
        assert!(!c.state.screen_preparing && !c.state.screen_sharing && c.stream_source.is_empty());
    }
    #[tokio::test]
    async fn late_share_completion_never_stops_replacement() {
        let (mut c, _, mut rx) = target();
        share(&mut c);
        let (bridge, old) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        c.try_share().await.unwrap();
        wait_calls(&old, 1).await;
        let (replacement, new_calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(replacement);
        c.voice_lifecycle.media_epoch += 1;
        c.voice_lifecycle.share_attempt = 2;
        reply(&mut c, &mut rx).await.unwrap();
        assert_eq!(*old.lock().unwrap(), ["startShare"]);
        assert!(new_calls.lock().unwrap().is_empty());
        assert_eq!(c.voice_lifecycle.share_attempt, 2);
    }
    #[tokio::test]
    async fn active_capture_stop_remains_available_without_encrypted_readiness() {
        let (mut c, _, _) = target();
        share(&mut c);
        let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        c.state.voice_encryption_ready = false;
        c.state.screen_preparing = false;
        c.state.screen_sharing = true;
        c.state.screen_paused = true;
        c.media_action(&UiAction::new("share")).await.unwrap();
        wait_calls(&calls, 1).await;
        assert_eq!(*calls.lock().unwrap(), ["stopShare"]);
        assert!(c.stream_source.is_empty() && !c.state.screen_sharing && !c.state.screen_paused);
    }
    #[tokio::test]
    async fn stale_camera_completion_never_marks_new_attempt_active() {
        let (mut c, _, _) = target();
        c.state.voice_encryption_ready = true;
        let mut action = UiAction::id("__voice-camera", "synthetic-camera");
        action.extra = c.meta().to_string();
        c.voice_lifecycle.camera_attempt += 1;
        c.voice_complete(action, wrapped(Ok(json!({"state":"active"}))))
            .await
            .unwrap();
        assert!(!c.state.camera_enabled);
    }
    #[tokio::test]
    async fn failed_audio_configuration_cannot_leave_ptt_transmitting_as_vad() {
        let (mut c, _, _) = target();
        let (bridge, _) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        let mut action = UiAction::id("__voice-data", "settings");
        action.extra = c.meta().to_string();
        assert!(
            c.voice_complete(action, wrapped(Err("Synthetic failure".into())))
                .await
                .is_err()
        );
        assert!(c.state.self_mute && c.voice_channel.is_empty() && c.media.is_none());
    }
    #[tokio::test]
    async fn ptt_command_uses_active_without_blocking_actor() {
        let (mut c, _, _) = target();
        let seen = Arc::new(Mutex::new(Value::Null));
        let record = seen.clone();
        let (bridge, calls) = MediaBridge::fake(move |_, value| {
            *record.lock().unwrap() = value.clone();
            Ok(json!({}))
        });
        c.media = Some(bridge);
        c.update_ptt(true);
        wait_calls(&calls, 1).await;
        assert_eq!(*seen.lock().unwrap(), json!({"active":true}));
    }
    #[tokio::test]
    async fn local_readiness_opens_explicit_video_picker_without_starting_camera() {
        let (mut c, _, _) = target();
        let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!([])));
        c.media = Some(bridge);
        c.pending_video = true;
        c.media_event(json!({"event":"voiceState","state":"waiting-peer","transportReady":true,"encryptionReady":false,"cameraAvailable":true})).await.unwrap();
        wait_calls(&calls, 1).await;
        assert!(!c.pending_video);
        assert_eq!(*calls.lock().unwrap(), ["listCameras"]);
        assert!(c.state.voice_connected && !c.state.camera_available);
    }
    #[tokio::test]
    async fn in_call_mic_test_applies_selected_devices_and_restores_previous_deafen() {
        for previous_deaf in [false, true] {
            let (mut c, _, mut rx) = target();
            c.state.voice_connected = true;
            c.state.voice_encryption_ready = true;
            c.state.self_deaf = previous_deaf;
            c.state.self_mute = false;
            let seen = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
            let record = seen.clone();
            let (bridge, calls) = MediaBridge::fake(move |kind, args| {
                record.lock().unwrap().push((kind.into(), args.clone()));
                Ok(json!({}))
            });
            c.media = Some(bridge);
            let mut preferences = c.media_preferences.clone();
            preferences.input_id = "synthetic-input".into();
            preferences.output_id = "synthetic-output".into();
            preferences.input_sensitivity_db = -26;
            let mut start = UiAction::new("test-mic");
            start.text = serde_json::to_string(&preferences).unwrap();
            c.media_action(&start).await.unwrap();
            assert!(c.state.self_deaf && !c.state.self_mute);
            assert_eq!(c.voice_lifecycle.mic_prior_deaf, Some(previous_deaf));
            assert!(c.media_action(&UiAction::new("mute")).await.is_err());
            assert!(c.media_action(&UiAction::new("deafen")).await.is_err());
            reply(&mut c, &mut rx).await.unwrap();
            assert!(c.voice_lifecycle.mic_active);
            let values = seen.lock().unwrap().clone();
            assert_eq!(values[0].0, "startMicTest");
            assert_eq!(values[0].1["settings"]["inputId"], "synthetic-input");
            assert_eq!(values[0].1["settings"]["outputId"], "synthetic-output");
            assert_eq!(values[0].1["settings"]["inputSensitivityDb"], -26);
            c.media_action(&UiAction::new("stop-mic-test"))
                .await
                .unwrap();
            reply(&mut c, &mut rx).await.unwrap();
            assert_eq!(*calls.lock().unwrap(), ["startMicTest", "stopMicTest"]);
            assert_eq!(c.state.self_deaf, previous_deaf);
            assert!(!c.state.self_mute && !c.voice_lifecycle.mic_active);
            assert_eq!(c.voice_lifecycle.mic_prior_deaf, None);
        }
    }
    #[tokio::test]
    async fn late_mic_start_and_stop_cannot_replace_new_test() {
        let (mut c, _, _) = target();
        let (bridge, _) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        c.voice_lifecycle.mic_attempt = 3;
        c.voice_lifecycle.mic_prior_deaf = Some(false);
        c.state.self_deaf = true;
        let mut old = UiAction::id("__voice-data", "mic-start");
        let mut meta = c.meta();
        meta["mic"] = json!(1);
        old.extra = meta.to_string();
        c.voice_complete(old, wrapped(Ok(json!({})))).await.unwrap();
        assert!(!c.voice_lifecycle.mic_active);
        c.media_event(json!({"event":"micTest","token":2,"active":false}))
            .await
            .unwrap();
        assert!(c.state.self_deaf);
        assert_eq!(c.voice_lifecycle.mic_prior_deaf, Some(false));
        c.media_event(json!({"event":"micTest","token":3,"active":true}))
            .await
            .unwrap();
        assert!(c.voice_lifecycle.mic_active);
        c.media_event(json!({"event":"micTest","token":3,"active":false}))
            .await
            .unwrap();
        assert!(!c.state.self_deaf);
    }
    #[tokio::test]
    async fn failed_mic_start_keeps_transport_and_restores_original_deafen() {
        let (mut c, _, _) = target();
        let (bridge, _) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        c.voice_lifecycle.mic_attempt = 1;
        c.voice_lifecycle.mic_prior_deaf = Some(false);
        c.state.self_deaf = true;
        let mut action = UiAction::id("__voice-data", "mic-start");
        action.extra = c.meta().to_string();
        assert!(
            c.voice_complete(action, wrapped(Err("synthetic loopback failure".into())))
                .await
                .is_err()
        );
        assert!(c.media.is_some() && !c.voice_channel.is_empty() && !c.state.self_deaf);
        assert!(c.voice_lifecycle.mic_prior_deaf.is_none());
    }
    #[tokio::test]
    async fn solo_selections_preview_locally_and_cancel_without_transmitting() {
        let (mut c, _, _) = target();
        let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        c.state.voice_connected = true;
        c.media_action(&UiAction::id("start-camera", "synthetic-camera"))
            .await
            .unwrap();
        let mut screen = UiAction::id("start-share", "window:123");
        screen.text = json!({"height":720,"fps":30}).to_string();
        c.media_action(&screen).await.unwrap();
        assert!(c.state.camera_preparing && c.state.screen_preparing);
        wait_calls(&calls, 2).await;
        assert!(
            calls
                .lock()
                .unwrap()
                .iter()
                .all(|kind| matches!(kind.as_str(), "startCameraPreview" | "startScreenPreview"))
        );
        c.media_action(&UiAction::new("toggle-camera"))
            .await
            .unwrap();
        c.media_action(&UiAction::new("stop-share")).await.unwrap();
        wait_calls(&calls, 5).await;
        c.media_event(json!({"event":"voiceState","state":"connected","transportReady":true,"encryptionReady":true,"cameraAvailable":true})).await.unwrap();
        assert!(calls.lock().unwrap().iter().all(|kind| matches!(
            kind.as_str(),
            "stopCamera"
                | "stopShare"
                | "stopMediaPreview"
                | "startCameraPreview"
                | "startScreenPreview"
        )));
        assert!(!c.state.camera_preparing && !c.state.screen_preparing);
    }
    #[tokio::test]
    async fn only_fresh_explicit_camera_selection_can_start_when_dave_is_ready() {
        for expired in [false, true] {
            let (mut c, _, mut rx) = target();
            let (bridge, calls) = MediaBridge::fake(|kind, args| {
                assert!(matches!(
                    kind,
                    "startCamera" | "startCameraPreview" | "stopMediaPreview"
                ));
                if kind == "stopMediaPreview" {
                    return Ok(json!({}));
                }
                assert_eq!(args["deviceId"], "synthetic-camera");
                Ok(json!({"state":"active"}))
            });
            c.media = Some(bridge);
            c.state.voice_connected = true;
            c.media_action(&UiAction::id("start-camera", "synthetic-camera"))
                .await
                .unwrap();
            reply(&mut c, &mut rx).await.unwrap();
            if expired {
                c.voice_lifecycle.pending_camera.as_mut().unwrap().1 =
                    Instant::now() - Duration::from_secs(91);
            }
            c.media_event(json!({"event":"voiceState","state":"connected","transportReady":true,"encryptionReady":true,"cameraAvailable":true})).await.unwrap();
            if expired {
                wait_calls(&calls, 2).await;
                assert_eq!(
                    *calls.lock().unwrap(),
                    ["startCameraPreview", "stopMediaPreview"]
                );
                assert!(!c.state.camera_enabled && !c.state.camera_preparing);
            } else {
                reply(&mut c, &mut rx).await.unwrap();
                assert_eq!(
                    *calls.lock().unwrap(),
                    ["startCameraPreview", "startCamera"]
                );
                assert!(c.state.camera_enabled);
            }
        }
    }
    #[tokio::test]
    async fn old_camera_event_cannot_clear_replacement_visual_state() {
        let (mut c, _, _) = target();
        c.voice_lifecycle.camera_attempt = 3;
        c.voice_lifecycle.native_voice = Some(9);
        c.state.camera_preparing = true;
        for event in [
            json!({"event":"cameraState","token":2,"mediaSession":9,"state":"stopped"}),
            json!({"event":"cameraState","token":3,"mediaSession":8,"state":"stopped"}),
        ] {
            c.media_event(event).await.unwrap();
            assert!(c.state.camera_preparing);
        }
    }
    #[tokio::test]
    async fn early_session_readiness_replayed_after_matching_response() {
        let (mut c, _, _) = target();
        c.media_event(json!({"event":"voiceState","mediaSession":11,"state":"waiting-peer","transportReady":true,"encryptionReady":false})).await.unwrap();
        assert!(!c.state.voice_connected);
        let mut action = UiAction::id("__voice-requested", "transport");
        action.extra = c.meta().to_string();
        c.voice_complete(action, wrapped(Ok(json!({"session":11}))))
            .await
            .unwrap();
        assert!(c.state.voice_connected && !c.state.voice_encryption_ready);
        assert!(c.voice_lifecycle.early_voice.is_none());
    }
    #[tokio::test]
    async fn old_share_session_event_cannot_cancel_replacement() {
        let (mut c, _, _) = target();
        share(&mut c);
        c.voice_lifecycle.native_share = Some(8);
        c.state.screen_sharing = true;
        c.media_event(json!({"event":"shareState","mediaSession":7,"state":"disconnected"}))
            .await
            .unwrap();
        assert_eq!(c.stream_source, "window:123");
        assert!(c.state.screen_sharing);
        c.media_event(json!({"event":"shareState","mediaSession":8,"state":"capture-requested"}))
            .await
            .unwrap();
        assert!(c.state.screen_sharing && !c.state.screen_preparing);
    }
    #[tokio::test]
    async fn changing_selected_camera_restarts_only_explicit_device() {
        let (mut c, _, mut rx) = target();
        let (bridge, calls) = MediaBridge::fake(|cmd, args| {
            if cmd == "startCamera" {
                assert_eq!(args["deviceId"], "new-explicit-camera");
                Ok(json!({"state":"active"}))
            } else {
                Ok(json!([]))
            }
        });
        c.media = Some(bridge);
        c.state.voice_encryption_ready = true;
        c.state.camera_enabled = true;
        c.media_preferences.camera_id = "old-camera".into();
        let mut preferences = c.media_preferences.clone();
        preferences.camera_id = "new-explicit-camera".into();
        let mut action = UiAction::id("__voice-data", "settings");
        action.extra = c.meta().to_string();
        action.text = serde_json::to_string(&preferences).unwrap();
        c.voice_complete(action, wrapped(Ok(json!({"applied":true}))))
            .await
            .unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            ["stopCamera", "listCameras", "startCamera"]
        );
        assert!(c.state.camera_enabled && !c.state.camera_preparing);
    }
    #[tokio::test]
    async fn stale_capture_failure_cannot_reset_replacement() {
        let (mut c, _, _) = target();
        share(&mut c);
        let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        let mut screen = UiAction::id("__voice-share", "start");
        screen.extra = c.meta().to_string();
        let mut camera = UiAction::id("__voice-camera", "old");
        camera.extra = c.meta().to_string();
        c.voice_lifecycle.share_attempt += 1;
        c.voice_lifecycle.camera_attempt += 1;
        c.state.camera_preparing = true;
        c.voice_failed(&screen).await;
        c.voice_failed(&camera).await;
        assert!(calls.lock().unwrap().is_empty());
        assert!(c.state.screen_preparing && c.state.camera_preparing);
        assert_eq!(c.stream_source, "window:123");
    }
    #[tokio::test]
    async fn selector_replacement_ignores_stale_preview_and_cleanup_preserves_outgoing_camera() {
        let (mut c, _, mut rx) = target();
        let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge);
        c.media_action(&UiAction::id("start-camera-preview", "camera-one"))
            .await
            .unwrap();
        c.media_action(&UiAction::id("start-screen-preview", "window:123"))
            .await
            .unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        let current = c.voice_lifecycle.preview_attempt;
        c.media_event(json!({"event":"videoStream","id":10,"kind":"screen","local":true,"preview":true,"previewToken":current,"sourceId":"window:123","active":true,"state":"active","userId":"100"})).await.unwrap();
        c.media_event(json!({"event":"videoStream","id":9,"kind":"camera","local":true,"preview":true,"previewToken":current-1,"sourceId":"camera-one","active":true,"state":"active","userId":"100"})).await.unwrap();
        assert_eq!(c.state.call_videos.len(), 1);
        assert_eq!(c.state.call_videos[0].source_id, "window:123");
        c.media_event(json!({"event":"videoStream","id":11,"kind":"camera","local":true,"preview":false,"sourceId":"camera-one","active":true,"state":"active","userId":"100"})).await.unwrap();
        c.media_action(&UiAction::new("stop-media-preview"))
            .await
            .unwrap();
        reply(&mut c, &mut rx).await.unwrap();
        assert_eq!(c.state.call_videos.len(), 1);
        assert_eq!(c.state.call_videos[0].id, 11);
        assert!(!c.state.call_videos[0].preview);
        assert_eq!(
            *calls.lock().unwrap(),
            [
                "startCameraPreview",
                "startScreenPreview",
                "stopMediaPreview"
            ]
        );
    }
    #[tokio::test]
    async fn scoped_remote_video_error_keeps_audio_and_other_video_alive() {
        let (mut c, _, _) = target();
        let (bridge, calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(bridge.clone());
        c.state.voice_connected = true;
        c.state.voice_encryption_ready = true;
        c.state.call_videos = vec![
            CallVideo {
                id: 1,
                user_id: "300".into(),
                active: true,
                kind: "camera".into(),
                ..Default::default()
            },
            CallVideo {
                id: 2,
                user_id: "400".into(),
                active: true,
                kind: "camera".into(),
                ..Default::default()
            },
        ];
        c.media_event(json!({"event":"videoError","scope":"stream","id":1,"userId":"300","message":"Synthetic decoder failure"})).await.unwrap();
        assert!(c.state.voice_connected && c.state.voice_encryption_ready);
        assert_eq!(c.voice_channel, "200");
        assert!(c.media.as_ref().unwrap().same_instance(&bridge));
        assert_eq!(c.state.call_videos[0].status, "error");
        assert!(c.state.call_videos[1].active);
        assert!(calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn video_errors_are_isolated_by_source_session_even_for_the_same_owner() {
        let (mut c,_,_)=target();c.state.voice_connected=true;c.state.voice_encryption_ready=true;
        let (bridge,calls)=MediaBridge::fake(|command,_|panic!("Scoped error must not affect native audio: {command}"));c.media=Some(bridge);
        let key="call:200:300";c.voice_lifecycle.watches.insert(key.into(),json!({"owner":"300","attempt":1}));
        let camera=json!({"event":"videoStream","id":1,"mediaSession":10,"userId":"300","kind":"camera","active":true,"state":"playing"});
        let screen=json!({"event":"videoStream","id":2,"mediaSession":20,"userId":"300","kind":"screen","streamKey":key,"active":true,"state":"playing"});
        c.media_event(camera.clone()).await.unwrap();c.media_event(screen.clone()).await.unwrap();
        c.media_event(json!({"event":"videoStream","id":3,"mediaSession":10,"userId":"400","kind":"camera","active":true,"state":"playing"})).await.unwrap();
        for event in [json!({"event":"videoError","scope":"stream","mediaSession":999,"userId":"","message":"Unknown connection"}),json!({"event":"videoError","scope":"stream","userId":"300","message":"Ambiguous identity"})] {
            c.media_event(event).await.unwrap();assert!(c.state.call_videos.iter().all(|video|video.active));
        }
        c.media_event(json!({"event":"videoError","scope":"stream","mediaSession":20,"userId":"300","message":"GoLive failed"})).await.unwrap();
        assert!(c.state.call_videos[0].active&&!c.state.call_videos[1].active&&c.state.call_videos[2].active);
        c.media_event(screen).await.unwrap();
        c.media_event(json!({"event":"videoError","scope":"stream","mediaSession":10,"userId":"300","message":"Camera failed"})).await.unwrap();
        assert!(!c.state.call_videos[0].active&&c.state.call_videos[1].active&&c.state.call_videos[2].active);
        c.media_event(json!({"event":"videoError","scope":"stream","id":3,"mediaSession":20,"message":"Mismatched source/session"})).await.unwrap();assert!(c.state.call_videos[2].active);
        c.media_event(json!({"event":"videoStream","id":2,"mediaSession":20,"userId":"300","kind":"screen","streamKey":key,"state":"removed"})).await.unwrap();
        assert_eq!(c.voice_lifecycle.video_sessions.len(),2);assert!(!c.voice_lifecycle.video_sessions.contains_key(&2));
        c.media_event(camera).await.unwrap();
        assert!(c.state.voice_connected&&c.state.voice_encryption_ready);assert_eq!(c.voice_channel,"200");assert!(calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn watch_failure_keeps_retry_error_and_cancel_by_owner_only_closes_receiver() {
        let (mut c, _, _) = target();
        let (bridge, voice_calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        let (receiver,calls)=MediaBridge::fake_receiver(|_,_|Ok(json!({})),bridge.video_frames());
        c.media = Some(bridge.clone());c.voice_lifecycle.receiver=Some(receiver);
        c.state.voice_connected = true;
        c.state.voice_encryption_ready = true;
        let key = "call:200:300";
        c.voice_lifecycle
            .watches
            .insert(key.into(), json!({"owner":"300","attempt":3}));
        c.state.call_videos.push(CallVideo {
            id: u64::MAX - 3,
            user_id: "300".into(),
            kind: "screen".into(),
            stream_key: key.into(),
            status: "connecting".into(),
            ..Default::default()
        });
        c.media_event(json!({"event":"watchState","streamKey":key,"token":3,"state":"error","reason":{"message":"Synthetic receiver failed"}})).await.unwrap();
        wait_named_calls(&calls,"shutdown",1).await;
        assert!(!c.voice_lifecycle.watches.contains_key(key));
        assert_eq!(c.state.call_videos[0].status, "error");
        assert_eq!(c.state.call_videos[0].error, "Synthetic receiver failed");
        assert!(c.state.voice_connected);
        c.media_action(&UiAction::id("stop-watch-stream", "300"))
            .await
            .unwrap();
        assert!(c.state.call_videos.is_empty());
        assert_eq!(*calls.lock().unwrap(), ["stopWatchStream","shutdown"]);
        assert!(voice_calls.lock().unwrap().is_empty()&&c.media.as_ref().unwrap().same_instance(&bridge));
        assert!(c.voice_lifecycle.receiver.is_none());
    }
    #[tokio::test]
    async fn fatal_helper_releases_old_bridge_and_leaves_media_runtime_retryable() {
        let (mut c, _, _) = target();
        let (bridge, _) = MediaBridge::fake(|_, _| Ok(json!({})));
        let root = c.data.join("synthetic-runtime");
        std::fs::create_dir_all(
            root.join("media-host/runtime/resources/standalone_modules/discord_voice"),
        )
        .unwrap();
        for path in [
            root.join("NotDiscord.Media.node"),
            root.join("media-host/runtime/NotDiscord.Media.exe"),
            root.join(
                "media-host/runtime/resources/standalone_modules/discord_voice/discord_voice.node",
            ),
        ] {
            std::fs::write(path, []).unwrap();
        }
        c.root = root;
        c.media = Some(bridge.clone());
        c.state.voice_connected = true;
        c.voice_lifecycle.request_pending = true;
        c.media_event(json!({"event":"fatal"})).await.unwrap();
        assert!(c.media.is_none());
        assert!(bridge.command("stats", json!({})).await.is_err());
        assert!(c.voice_channel.is_empty());
        assert!(!c.voice_lifecycle.request_pending);
        assert!(!c.state.voice_connected);
        assert!(c.state.voice_available);
    }
    #[tokio::test]
    async fn receive_cleanup_failure_keeps_voice_and_another_participant_then_allows_reentry() {
        let (mut c,_,mut rx)=target();
        c.state.voice_connected=true;c.state.voice_encryption_ready=true;
        let (bridge,voice_calls)=MediaBridge::fake(|command,_|panic!("Receiver cleanup touched voice: {command}"));
        c.media=Some(bridge.clone());
        let (receiver,calls)=MediaBridge::fake_receiver(|command,_|if command=="stopWatchStream" {Err("Synthetic receiver shutdown failure".into())}else{Ok(json!({}))},bridge.video_frames());
        c.voice_lifecycle.receiver=Some(receiver);
        let key="call:200:300";
        c.voice_lifecycle.watches.insert(key.into(),json!({"owner":"300","attempt":3,"started":true}));
        c.state.call_videos=vec![CallVideo{id:10,user_id:"300".into(),kind:"screen".into(),stream_key:key.into(),status:"playing".into(),active:true,..Default::default()},CallVideo{id:11,user_id:"400".into(),kind:"camera".into(),active:true,..Default::default()}];
        c.stop_watch(key).await.unwrap();
        wait_named_calls(&calls,"stopWatchStream",1).await;
        assert!(c.state.voice_connected&&c.state.voice_encryption_ready&&c.media.as_ref().unwrap().same_instance(&bridge));
        assert_eq!(c.voice_channel,"200");assert!(c.voice_lifecycle.watches.is_empty());
        assert_eq!(c.state.call_videos.len(),1);assert!(c.state.call_videos[0].active);
        // A new receive request on the same voice connection has a fresh token.
        let (receiver,reopened_calls)=MediaBridge::fake_receiver(|_,_|Ok(json!({})),bridge.video_frames());c.voice_lifecycle.receiver=Some(receiver);
        c.voice_lifecycle.watches.insert(key.into(),json!({"owner":"300","attempt":9,"server":"500","channel":"501","endpoint":"offline.discord.media","token":"SYNTHETIC"}));
        c.try_watch(key).await.unwrap();reply(&mut c,&mut rx).await.unwrap();
        assert_eq!(reopened_calls.lock().unwrap().as_slice(),["watchStream"]);
        assert!(c.voice_lifecycle.watches[key]["started"]==true&&c.state.voice_connected);
        c.stop_watch(key).await.unwrap();wait_named_calls(&reopened_calls,"stopWatchStream",1).await;
        assert!(c.voice_lifecycle.watches.is_empty()&&c.state.voice_connected);
        assert!(voice_calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn individual_volume_uses_real_bridge_ack_and_never_changes_global_or_other_peer_volume() {
        let (mut c,_,mut rx)=target();c.state.voice_connected=true;
        for id in ["300","400"] {c.call_people.insert(id.into(),CallParticipant{user:Person{id:id.into(),..Default::default()},..Default::default()});}
        let captured=Arc::new(Mutex::new(Vec::new()));let seen=captured.clone();
        let (bridge,calls)=MediaBridge::fake(move|command,args| {assert_eq!(command,"setVolume");seen.lock().unwrap().push(args.clone());Ok(args.clone())});
        c.media=Some(bridge);let original=c.media_preferences.clone();
        let mut action=UiAction::id("participant-volume","300");action.channel_id="200".into();action.extra=json!({"voiceAttempt":7}).to_string();action.text="500".into();
        c.media_action(&action).await.unwrap();assert!(!c.media_preferences.user_volumes.contains_key("300"));
        reply(&mut c,&mut rx).await.unwrap();
        assert_eq!(c.media_preferences.user_volumes["300"],500.);assert_eq!(c.call_people["400"].volume_percent,100.);
        assert_eq!(c.call_people["300"].volume_percent,500.);
        let mut expected=original;expected.user_volumes.insert("300".into(),500.);
        assert_eq!(serde_json::to_value(&c.media_preferences).unwrap(),serde_json::to_value(&expected).unwrap());
        assert_eq!(captured.lock().unwrap().as_slice(),[json!({"target":"user","userId":"300","value":500.0,"volumeRevision":1})]);
        for value in [0.,25.,99.,100.,125.5] {
            action.text=value.to_string();c.media_action(&action).await.unwrap();reply(&mut c,&mut rx).await.unwrap();
            assert_eq!(c.call_people["300"].volume_percent,value);assert_eq!(c.call_people["400"].volume_percent,100.);
        }
        for (user,value,attempt) in [("100","500",7),("999","500",7),("300","-1",7),("300","501",7),("300","500",6)] {
            action.id=user.into();action.text=value.into();action.extra=json!({"voiceAttempt":attempt}).to_string();assert!(c.media_action(&action).await.is_err());
        }
        assert_eq!(calls.lock().unwrap().len(),6);assert!(c.voice_lifecycle.pending_volumes.is_empty());
    }
    #[tokio::test]
    async fn individual_volume_persists_across_peer_leave_stop_and_controller_restart() {
        let (mut c,_,mut replies)=target();c.state.voice_connected=true;
        c.call_people.insert("300".into(),CallParticipant{user:Person{id:"300".into(),name:"Old name".into(),..Default::default()},..Default::default()});
        let (bridge,_)=MediaBridge::fake(|command,args| {assert_eq!(command,"setVolume");Ok(args.clone())});c.media=Some(bridge);
        let mut action=UiAction::id("participant-volume","300");action.channel_id="200".into();action.text="175".into();action.extra=json!({"voiceAttempt":7}).to_string();
        c.media_action(&action).await.unwrap();
        // A departing peer does not invalidate an ACK for this same call.
        c.media_event(json!({"event":"participant","userId":"300","present":false})).await.unwrap();
        assert!(!c.call_people.contains_key("300"));
        reply(&mut c,&mut replies).await.unwrap();
        assert_eq!(c.media_preferences.user_volumes["300"],175.);
        c.media_event(json!({"event":"participant","userId":"300","present":true})).await.unwrap();
        assert_eq!(c.call_people["300"].volume_percent,175.);
        c.stop_voice_internal(false).await;
        assert_eq!(c.media_preferences.user_volumes["300"],175.);
        let loaded:MediaPreferences=storage::read_json(&c.data.join("media-settings.json"),32768);
        let (tx,mut rx)=mpsc::unbounded_channel();let (updates,_)=std::sync::mpsc::channel();
        let mut restarted=Controller::new(c.root.clone(),c.data.clone(),AppSnapshot::welcome(),tx,updates,eframe::egui::Context::default(),Features::new(c.data.clone()).unwrap(),loaded);
        restarted.state.current_user=Some(Person{id:"100".into(),..Default::default()});restarted.state.signed_in=true;
        restarted.voice_channel="200".into();restarted.voice_session="synthetic-restarted".into();restarted.voice_endpoint="synthetic.invalid".into();restarted.voice_token="SYNTHETIC-LOCAL-ONLY".into();
        for (id,name) in [("300","Renamed person"),("400","Same old name, different ID")] {restarted.call_people.insert(id.into(),CallParticipant{user:Person{id:id.into(),name:name.into(),..Default::default()},..Default::default()});}
        restarted.refresh_call();
        assert_eq!(restarted.call_people["300"].volume_percent,175.);assert_eq!(restarted.call_people["400"].volume_percent,100.);
        assert_eq!(restarted.media_parameters_for(&restarted.media_preferences)["userVolumes"]["300"],175.);
        assert_eq!(restarted.receiver_parameters()["userVolumes"]["300"],175.);
        let (bridge,calls)=MediaBridge::fake(|command,args| {assert_eq!(command,"connectVoice");assert_eq!(args["participantVolumes"]["300"],175.);assert_eq!(args["userVolumesRevision"],0);assert!(args["participantVolumes"].get("400").is_none());Ok(json!({"session":42}))});
        restarted.media=Some(bridge);restarted.try_voice().await.unwrap();reply(&mut restarted,&mut rx).await.unwrap();
        assert_eq!(calls.lock().unwrap().as_slice(),["connectVoice"]);
    }
    #[tokio::test]
    async fn individual_volume_newest_ack_survives_stale_settings_without_changing_other_audio() {
        let (mut c,_,mut replies)=target();c.state.voice_connected=true;
        c.call_people.insert("300".into(),CallParticipant{user:Person{id:"300".into(),..Default::default()},..Default::default()});
        let old_preferences=c.media_preferences.clone();
        let (bridge,_)=MediaBridge::fake(|command,args| {assert_eq!(command,"setVolume");Ok(args.clone())});c.media=Some(bridge);
        let mut action=UiAction::id("participant-volume","300");action.channel_id="200".into();action.extra=json!({"voiceAttempt":7}).to_string();
        for value in [25.,250.] {action.text=value.to_string();c.media_action(&action).await.unwrap();}
        let first=tokio::time::timeout(Duration::from_secs(1),replies.recv()).await.unwrap().unwrap();
        let second=tokio::time::timeout(Duration::from_secs(1),replies.recv()).await.unwrap().unwrap();
        let (older,newer)=if first.action.text=="25" {(first,second)}else{(second,first)};
        c.complete(newer.action,newer.result.unwrap()).await.unwrap();
        let accepted_revision=c.voice_lifecycle.user_volumes_revision;
        c.complete(older.action,older.result.unwrap()).await.unwrap();
        assert_eq!(c.call_people["300"].volume_percent,250.);assert_eq!(c.voice_lifecycle.user_volumes_revision,accepted_revision);
        let mut stale=UiAction::id("__voice-data","settings");stale.extra=c.meta().to_string();stale.text=serde_json::to_string(&old_preferences).unwrap();
        c.voice_complete(stale,wrapped(Ok(json!({})))).await.unwrap();
        let saved:MediaPreferences=storage::read_json(&c.data.join("media-settings.json"),32768);
        assert_eq!(saved.user_volumes["300"],250.);assert_eq!((saved.input_volume,saved.output_volume),(100,100));
        assert_eq!(c.receiver_parameters()["userVolumesRevision"],accepted_revision);
        assert_eq!(c.receiver_parameters()["userVolumes"]["300"],250.);
    }
    #[tokio::test]
    async fn individual_volume_failed_ack_or_persistence_error_keeps_controls_honest() {
        let (mut c,updates,mut replies)=target();c.state.voice_connected=true;
        c.call_people.insert("300".into(),CallParticipant{user:Person{id:"300".into(),..Default::default()},..Default::default()});
        let (bridge,_)=MediaBridge::fake(|_,_|Err("Synthetic native rejection".into()));c.media=Some(bridge);
        let mut action=UiAction::id("participant-volume","300");action.channel_id="200".into();action.text="500".into();action.extra=json!({"voiceAttempt":7}).to_string();
        c.media_action(&action).await.unwrap();assert!(reply(&mut c,&mut replies).await.is_err());
        assert!(c.media_preferences.user_volumes.is_empty());assert_eq!(c.call_people["300"].volume_percent,100.);
        assert!(!c.data.join("media-settings.json").exists());
        let (bridge,_)=MediaBridge::fake(|_,args|Ok(args.clone()));c.media=Some(bridge);
        // An obstructing directory makes the synthetic fixture unwritable
        // without changing permissions or touching the real data directory.
        std::fs::create_dir(c.data.join("media-settings.json")).unwrap();
        c.media_action(&action).await.unwrap();
        let done=tokio::time::timeout(Duration::from_secs(1),replies.recv()).await.unwrap().unwrap();
        let error=c.complete(done.action.clone(),done.result.unwrap()).await.unwrap_err();
        assert!(error.contains("Volumen aplicado")&&error.contains("no se pudo guardar"));
        c.completion_error(&done.action,error);
        assert_eq!(c.call_people["300"].volume_percent,500.);assert_eq!(c.media_preferences.user_volumes["300"],500.);
        assert!(updates.try_iter().any(|update|matches!(update,UiUpdate::Error(error) if error.contains("Volumen aplicado"))));
        assert!(c.state.voice_connected);
    }
    #[tokio::test]
    async fn own_tile_volume_updates_only_input_after_native_ack_and_persists_the_shared_setting() {
        let (mut c,updates,mut replies)=target();c.state.voice_connected=true;
        c.call_people.insert("100".into(),CallParticipant{user:c.state.current_user.clone().unwrap(),is_self:true,..Default::default()});
        let (bridge,calls)=MediaBridge::fake(|command,args| {assert_eq!(command,"setVolume");assert_eq!(args["target"],"input");assert!(args.get("userId").is_none());Ok(args.clone())});
        c.media=Some(bridge);
        let mut action=UiAction::id("participant-input-volume","100");action.channel_id="200".into();action.extra=json!({"voiceAttempt":7}).to_string();
        for value in [0,25,125,200] {
            action.text=value.to_string();let before=c.media_preferences.input_volume;c.media_action(&action).await.unwrap();assert_eq!(c.media_preferences.input_volume,before);
            reply(&mut c,&mut replies).await.unwrap();
            assert_eq!(c.media_preferences.input_volume,value);assert_eq!(c.call_people["100"].volume_percent,value as f32);
            let saved:MediaPreferences=storage::read_json(&c.data.join("media-settings.json"),16384);
            assert_eq!(saved.input_volume,value);assert_eq!(saved.output_volume,100);
        }
        assert_eq!(calls.lock().unwrap().len(),4);
        assert!(updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,value) if kind=="media-settings"&&value["InputVolume"]==200)));
        for value in ["-1","201","NaN"] {action.text=value.into();assert!(c.media_action(&action).await.is_err());}
        action.id="300".into();action.text="100".into();assert!(c.media_action(&action).await.is_err());
        assert_eq!(calls.lock().unwrap().len(),4);assert!(!c.voice_lifecycle.mic_active&&c.voice_lifecycle.mic_prior_deaf.is_none());
    }
    #[tokio::test]
    async fn native_gate_failure_marks_actual_input_muted_without_ending_receive_audio_or_a_newer_session() {
        let (mut c,_,_)=target();c.state.voice_connected=true;c.state.voice_encryption_ready=true;c.voice_lifecycle.native_voice=Some(30);
        let (bridge,calls)=MediaBridge::fake(|command,_|panic!("Input fault must not touch receive connection: {command}"));c.media=Some(bridge.clone());
        c.call_people.insert("100".into(),CallParticipant{user:c.state.current_user.clone().unwrap(),is_self:true,speaking:true,..Default::default()});
        for event in [json!({"event":"inputGateState","muted":true}),json!({"event":"inputGateState","mediaSession":29,"muted":true})] {
            c.media_event(event).await.unwrap();assert!(!c.state.self_mute);
        }
        c.media_event(json!({"event":"inputGateState","mediaSession":30,"muted":true})).await.unwrap();
        assert!(c.state.self_mute&&c.state.voice_connected&&c.state.voice_encryption_ready);
        assert!(c.call_people["100"].self_mute&&!c.call_people["100"].speaking);
        assert!(c.media.as_ref().unwrap().same_instance(&bridge)&&calls.lock().unwrap().is_empty());assert_eq!(c.voice_channel,"200");
    }
    #[tokio::test]
    async fn switching_guild_private_and_group_calls_waits_for_leave_then_starts_one_fresh_transport() {
        for (previous_guild,destination_guild,destination_kind) in [("500","500","voice"),("500","600","voice"),("500","","dm"),("","","group"),("","600","voice"),("","","dm")] {
            let (mut c,_,mut replies)=target();c.voice_guild=previous_guild.into();c.state.voice_connected=true;c.state.voice_encryption_ready=true;
            c.state.self_mute=true;c.state.self_deaf=false;
            c.api=Some(Api::new("SYNTHETIC-OFFLINE-ONLY-NOT-A-USER-ACCOUNT".into()).unwrap());
            let (gateway,mut wire,task)=GatewayHandle::offline_recorder(c.api.as_ref().unwrap().safety());c.gateway=Some(gateway.clone());
            if destination_guild.is_empty() {
                // This scenario tests transport switching, not HTTP. Private
                // joins now read real conversation recipients in production;
                // inject that complete result before any request is launched.
                c.voice_lifecycle.preflight_fixture=Some(("201".into(),json!({"ringable":false,"recipients":[{"id":"300","username":"Synthetic destination member"}]})));
                c.state.direct_messages.push(NavItem{id:"201".into(),name:"Synthetic private destination".into(),kind:destination_kind.into(),..Default::default()});
                c.state.incoming_calls.push(IncomingCall{channel_id:"201".into(),instance_id:12,kind:destination_kind.into(),..Default::default()});
            } else {
                c.state.selected_guild_id=destination_guild.into();
                c.guild_raw=json!({"id":destination_guild,"roles":[{"id":destination_guild,"permissions":((1u64<<10)|(1<<20)|(1<<21)|(1<<9)).to_string()}]});
                c.guild_acl=Some(GuildAccess::from_values(&c.guild_raw,&json!({}),&c.me()));
                c.channels.insert("201".into(),json!({"id":"201","guild_id":destination_guild,"type":2,"permission_overwrites":[]}));
                c.state.channels.push(NavItem{id:"201".into(),name:"Synthetic guild destination".into(),kind:"voice".into(),..Default::default()});
            }
            let (old,old_calls)=MediaBridge::fake(|command,_|{assert_eq!(command,"disconnect");Ok(json!({"state":"disconnected"}))});c.media=Some(old.clone());
            c.call_people.insert("300".into(),CallParticipant::default());c.state.call_videos.push(CallVideo{id:1,active:true,..Default::default()});
            c.voice_lifecycle.watches.insert("old".into(),json!({"owner":"300"}));
            let old_epoch=c.voice_lifecycle.media_epoch;
            let join_only=destination_guild.is_empty();
            c.switch_voice("201",false,join_only,if join_only {12} else {0}).await.unwrap();
            let leave=wire.recv().await.unwrap();assert_eq!(leave["op"],4);assert!(leave["d"]["channel_id"].is_null());
            assert_eq!(leave["d"]["guild_id"].as_str(),(!previous_guild.is_empty()).then_some(previous_guild));
            assert!(!c.state.voice_connected&&c.state.voice_connecting&&c.media.is_none()&&c.voice_channel.is_empty());
            assert!(c.call_people.is_empty()&&c.state.call_videos.is_empty()&&c.voice_lifecycle.watches.is_empty());
            assert!(old.command("stats",json!({})).await.is_err());assert_eq!(old_calls.lock().unwrap().as_slice(),["disconnect"]);
            c.switch_voice("201",false,join_only,if join_only {12} else {0}).await.unwrap();assert!(wire.try_recv().is_err());
            let mut stale=UiAction::new("__media");stale.extra=old_epoch.to_string();c.complete(stale,json!({"event":"fatal"})).await.unwrap();assert!(c.voice_lifecycle.switching.is_some());
            c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":previous_guild,"user_id":"100","channel_id":null})).await.unwrap();
            assert!(c.voice_lifecycle.switching.is_none()&&c.voice_lifecycle.request_pending);
            let (replacement,native_calls)=MediaBridge::fake(|command,_|match command {"connectVoice"=>Ok(json!({"session":30})),"mute"=>Ok(json!({})),_=>panic!("Unexpected destination operation: {command}")});c.media=Some(replacement);
            reply(&mut c,&mut replies).await.unwrap();reply(&mut c,&mut replies).await.unwrap();
            assert!(c.voice_lifecycle.preflight_fixture.is_none());
            let join=wire.recv().await.unwrap();assert_eq!(join["op"],4);assert_eq!(join["d"]["channel_id"],"201");
            assert_eq!(join["d"]["guild_id"].as_str(),(!destination_guild.is_empty()).then_some(destination_guild));
            c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":destination_guild,"user_id":"100","channel_id":"201","session_id":"SYNTHETIC-NEW"})).await.unwrap();
            c.voice_dispatch("VOICE_SERVER_UPDATE",&json!({"guild_id":destination_guild,"channel_id":"201","endpoint":"offline.discord.media","token":"SYNTHETIC-ONLY"})).await.unwrap();
            reply(&mut c,&mut replies).await.unwrap();
            c.media_event(json!({"event":"voiceState","mediaSession":30,"state":"connected","encryptionReady":true,"transportReady":true})).await.unwrap();
            assert!(c.state.voice_connected&&c.state.voice_encryption_ready);assert_eq!(c.state.call_channel_id,"201");
            assert!(c.state.self_mute&&!c.state.self_deaf);assert_eq!(native_calls.lock().unwrap().iter().filter(|kind|kind.as_str()=="connectVoice").count(),1);
            c.stop_voice().await;gateway.stop();task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn switch_timeout_and_changed_destination_permissions_clear_connecting_and_allow_retry() {
        let (mut c,_,_)=target();c.state.voice_connected=true;
        c.api=Some(Api::new("SYNTHETIC-OFFLINE-ONLY-NOT-A-USER-ACCOUNT".into()).unwrap());
        let (gateway,_wire,task)=GatewayHandle::offline_recorder(c.api.as_ref().unwrap().safety());c.gateway=Some(gateway.clone());
        c.state.direct_messages.push(NavItem{id:"201".into(),kind:"group".into(),..Default::default()});
        c.state.incoming_calls.push(IncomingCall{channel_id:"201".into(),instance_id:12,kind:"group".into(),..Default::default()});
        c.switch_voice("201",false,true,12).await.unwrap();c.voice_lifecycle.switching.as_mut().unwrap().deadline=Instant::now()-Duration::from_millis(1);
        c.expire_outgoing_ring();assert!(c.voice_lifecycle.switching.is_none()&&!c.state.voice_connecting&&c.state.call_channel_id.is_empty());
        c.state.incoming_calls.clear();assert!(c.switch_voice("201",false,true,12).await.is_err());assert!(!c.state.voice_connecting);
        c.stop_voice().await;gateway.stop();task.await.unwrap();
    }
    #[tokio::test]
    async fn server_disconnect_before_session_assignment_clears_connecting_and_allows_a_fresh_attempt() {
        let (mut c,_,_)=target();c.voice_session.clear();c.voice_started=false;c.state.voice_connecting=true;
        c.voice_lifecycle.request_pending=true;c.voice_lifecycle.request_channel="200".into();
        c.voice_lifecycle.request_guild="500".into();c.refresh_call();assert_eq!(c.state.call_channel_id,"200");
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"user_id":"100","channel_id":null})).await.unwrap();
        assert!(c.voice_channel.is_empty()&&!c.state.voice_connecting&&!c.voice_lifecycle.request_pending);
        assert!(c.state.call_channel_id.is_empty()&&c.state.call_guild_id.is_empty());
        // This reaches the preflight dependency check instead of the old busy-call rejection.
        let error=c.request_voice("200",false,false).unwrap_err();assert_eq!(error,"La conexión aún no está preparada.");
    }
    #[tokio::test]
    async fn manual_leave_and_immediate_rejoin_wait_for_ack_without_duplicate_leave_or_old_join() {
        let (mut c,_,mut replies)=target();
        guild_permissions_fixture(&mut c,(1<<10)|(1<<20)|(1<<21)|(1<<9));
        c.voice_guild="500".into();c.state.voice_connected=true;c.state.voice_encryption_ready=true;
        c.state.channels.push(NavItem{id:"200".into(),name:"Offline voice".into(),kind:"voice".into(),..Default::default()});
        c.api=Some(Api::new("SYNTHETIC-OFFLINE-ONLY-NOT-A-USER-ACCOUNT".into()).unwrap());
        let (gateway,mut wire,task)=GatewayHandle::offline_recorder(c.api.as_ref().unwrap().safety());c.gateway=Some(gateway.clone());
        c.stop_voice().await;
        assert!(wire.recv().await.unwrap()["d"]["channel_id"].is_null());
        c.join_voice("200",false).await.unwrap();
        assert!(c.voice_lifecycle.switching.is_some()&&!c.voice_lifecycle.request_pending&&c.voice_channel.is_empty());
        assert!(wire.try_recv().is_err(),"joining before the first leave ACK must not send duplicate leaves");
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"500","user_id":"100","channel_id":"200","session_id":"SYNTHETIC-OLD"})).await.unwrap();
        assert!(c.voice_session.is_empty()&&c.voice_channel.is_empty()&&c.voice_lifecycle.switching.is_some());
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"600","user_id":"100","channel_id":null})).await.unwrap();
        assert!(c.voice_lifecycle.switching.is_some());
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"500","user_id":"100","channel_id":null})).await.unwrap();
        assert!(c.voice_lifecycle.leaving.is_none()&&c.voice_lifecycle.switching.is_none()&&c.voice_lifecycle.request_pending);
        let (replacement,calls)=MediaBridge::fake(|command,_|match command {"connectVoice"=>Ok(json!({"session":30})),"mute"=>Ok(json!({})),_=>panic!("Unexpected operation: {command}")});c.media=Some(replacement);
        reply(&mut c,&mut replies).await.unwrap();reply(&mut c,&mut replies).await.unwrap();
        assert_eq!(wire.recv().await.unwrap()["d"]["channel_id"],"200");
        c.voice_dispatch("VOICE_SERVER_UPDATE",&json!({"guild_id":"500","endpoint":"offline.discord.media","token":"SYNTHETIC-NEW-ONLY"})).await.unwrap();
        assert!(!c.voice_started);
        c.voice_dispatch("VOICE_STATE_UPDATE",&json!({"guild_id":"500","user_id":"100","channel_id":"200","session_id":"SYNTHETIC-NEW"})).await.unwrap();
        reply(&mut c,&mut replies).await.unwrap();
        assert_eq!(calls.lock().unwrap().iter().filter(|name|name.as_str()=="connectVoice").count(),1);
        c.media_event(json!({"event":"voiceState","mediaSession":30,"state":"waiting-peer","transportReady":true,"encryptionReady":false})).await.unwrap();
        assert!(c.state.voice_connected&&!c.state.voice_connecting&&c.voice_lifecycle.join_wait.is_none());
        c.stop_voice().await;gateway.stop();task.await.unwrap();
    }
    #[tokio::test]
    async fn missing_join_parts_expire_with_specific_reason_close_host_and_ignore_late_completion() {
        for (phase,session,server,expected) in [
            (VoiceJoinPhase::Preflight,false,false,"comprobar"),
            (VoiceJoinPhase::MediaHost,false,false,"motor local"),
            (VoiceJoinPhase::Gateway,false,false,"sesión ni asignó"),
            (VoiceJoinPhase::Gateway,true,false,"no asignó"),
            (VoiceJoinPhase::Gateway,false,true,"no confirmó"),
            (VoiceJoinPhase::Transport,true,true,"motor local"),
        ] {
            let (mut c,_,_)=target();c.state.voice_connecting=true;
            c.voice_session=if session {"SYNTHETIC-ONLY".into()}else{String::new()};
            c.voice_endpoint=if server {"offline.discord.media".into()}else{String::new()};
            c.voice_token=if server {"SYNTHETIC-ONLY".into()}else{String::new()};
            c.voice_lifecycle.request_pending=true;c.voice_lifecycle.request_channel="200".into();
            let (bridge,_)=MediaBridge::fake(|_,_|Ok(json!({})));c.media=Some(bridge.clone());
            c.wait_voice_join(phase,15);c.voice_lifecycle.join_wait.as_mut().unwrap().deadline=Instant::now()-Duration::from_millis(1);
            let mut old=UiAction::id("__voice-requested","transport");old.extra=c.meta().to_string();
            c.expire_voice_join().await;
            assert!(c.state.status.contains(expected),"unexpected timeout reason: {}",c.state.status);
            assert_eq!(c.state.voice_error,c.state.status);
            assert!(!c.state.voice_connecting&&!c.state.voice_connected&&c.voice_channel.is_empty()&&c.media.is_none()&&!c.voice_lifecycle.request_pending);
            assert!(bridge.command("stats",json!({})).await.is_err());
            c.voice_complete(old,json!({"ok":true,"result":{"session":999}})).await.unwrap();
            assert!(c.voice_lifecycle.native_voice.is_none()&&c.voice_channel.is_empty());
        }
    }
    #[tokio::test]
    async fn complete_actor_join_keeps_gateway_credentials_in_both_event_orders_and_late_send_ack() {
        for state_first in [false,true] {
            for ack_first in [false,true] {
                let (mut c,_,mut replies)=target();c.voice_channel.clear();c.voice_session.clear();
                guild_permissions_fixture(&mut c,(1<<10)|(1<<20)|(1<<21)|(1<<9));
                c.state.channels.push(NavItem{id:"200".into(),name:"Offline voice".into(),kind:"voice".into(),..Default::default()});
                c.api=Some(Api::new("SYNTHETIC-OFFLINE-ONLY-NOT-A-USER-ACCOUNT".into()).unwrap());
                let (gateway,mut wire,task)=GatewayHandle::offline_recorder(c.api.as_ref().unwrap().safety());c.gateway=Some(gateway.clone());
                let params=Arc::new(Mutex::new(Vec::<Value>::new()));let native_params=params.clone();
                let (media,calls)=MediaBridge::fake(move|command,args|match command {
                    "connectVoice"=>{native_params.lock().unwrap().push(args.clone());Ok(json!({"session":40,"state":"connecting"}))},
                    "mute"=>Ok(json!({})),_=>panic!("Unexpected native operation: {command}"),
                });c.media=Some(media);
                c.act(UiAction::id("voice","200")).await.unwrap();
                reply(&mut c,&mut replies).await.unwrap();
                assert_eq!(wire.recv().await.unwrap()["d"]["channel_id"],"200");
                let ack=tokio::time::timeout(Duration::from_secs(1),replies.recv()).await.unwrap().unwrap();
                assert_eq!(ack.action.id,"gateway");
                if ack_first {c.complete(ack.action.clone(),ack.result.clone().unwrap()).await.unwrap();}
                let state=("VOICE_STATE_UPDATE",json!({"guild_id":"500","user_id":"100","channel_id":"200","session_id":"SYNTHETIC-ACTOR-SESSION"}));
                let server=("VOICE_SERVER_UPDATE",json!({"guild_id":"500","endpoint":"offline.discord.media","token":"SYNTHETIC-ACTOR-TOKEN"}));
                let events=if state_first {[state,server]}else{[server,state]};
                for (kind,payload) in events {c.dispatch(kind,payload).await.unwrap();}
                assert!(c.voice_started);
                // A server reply can beat the local write acknowledgement; the
                // acknowledgement must not reset fields already received.
                if !ack_first {c.complete(ack.action,ack.result.unwrap()).await.unwrap();}
                assert_eq!(c.voice_session,"SYNTHETIC-ACTOR-SESSION");
                assert_eq!(c.voice_endpoint,"offline.discord.media");
                wait_named_calls(&calls,"connectVoice",1).await;
                let passed=params.lock().unwrap().clone();assert_eq!(passed.len(),1);
                assert_eq!(passed[0]["sessionId"],"SYNTHETIC-ACTOR-SESSION");assert_eq!(passed[0]["voiceToken"],"SYNTHETIC-ACTOR-TOKEN");assert_eq!(passed[0]["guildId"],"500");
                c.media_event(json!({"event":"voiceState","mediaSession":40,"state":"waiting-peer","transportReady":true,"encryptionReady":false})).await.unwrap();
                reply(&mut c,&mut replies).await.unwrap();
                assert!(c.state.voice_connected&&!c.state.voice_connecting&&c.voice_lifecycle.join_wait.is_none());
                assert_eq!(calls.lock().unwrap().iter().filter(|command|command.as_str()=="connectVoice").count(),1);
                c.stop_voice().await;gateway.stop();task.await.unwrap();
            }
        }
    }
    #[tokio::test]
    async fn partial_self_state_does_not_erase_session_or_disconnect_before_server_assignment() {
        for omit_channel in [false,true] {
            let (mut c,_,mut replies)=target();c.voice_guild="500".into();c.voice_started=false;
            guild_permissions_fixture(&mut c,(1<<10)|(1<<20)|(1<<21)|(1<<9));
            c.state.voice_connecting=true;
            let (media,calls)=MediaBridge::fake(|command,args|match command {
                "connectVoice"=>{assert_eq!(args["sessionId"],"synthetic-session");Ok(json!({"session":41}))},
                _=>panic!("Unexpected native operation: {command}"),
            });c.media=Some(media);
            let mut delta=json!({"guild_id":"500","user_id":"100","self_mute":true});
            if !omit_channel {delta["channel_id"]=json!("200");}
            c.dispatch("VOICE_STATE_UPDATE",delta).await.unwrap();
            assert_eq!(c.voice_channel,"200");assert_eq!(c.voice_session,"synthetic-session");assert!(!c.voice_started);
            c.dispatch("VOICE_SERVER_UPDATE",json!({"guild_id":"500","endpoint":"offline.discord.media","token":"SYNTHETIC-ONLY"})).await.unwrap();
            reply(&mut c,&mut replies).await.unwrap();
            assert_eq!(calls.lock().unwrap().as_slice(),["connectVoice"]);assert!(c.voice_started);
            c.dispatch("VOICE_STATE_UPDATE",json!({"guild_id":"500","user_id":"100","channel_id":null})).await.unwrap();
            assert!(c.voice_channel.is_empty()&&c.media.is_none()&&!c.state.voice_connecting);
        }
    }
    #[tokio::test]
    async fn voice_failure_remains_visible_after_unrelated_status_and_manual_teardown() {
        let (mut c,_,_)=target();c.voice_lifecycle.native_voice=Some(90);c.state.voice_connecting=true;
        c.media_event(json!({"event":"voiceState","mediaSession":90,"state":"error","reason":{"code":"UDP_TIMEOUT","message":"Synthetic UDP did not connect"}})).await.unwrap();
        assert!(c.voice_channel.is_empty()&&c.state.voice_error.contains("Synthetic UDP"));
        let error=c.state.voice_error.clone();
        c.status("Otro ajuste guardado");c.stop_voice().await;
        assert_eq!(c.state.voice_error,error);
        c.report_voice_error(&"x".repeat(2000));assert_eq!(c.state.voice_error.chars().count(),512);
    }
    #[tokio::test]
    async fn a_joining_media_host_open_failure_remains_visible_after_request_cleanup() {
        let (mut c,_,_)=target();c.voice_channel.clear();c.state.voice_connecting=true;
        c.voice_lifecycle.request_pending=true;c.voice_lifecycle.request_channel="200".into();
        c.voice_lifecycle.opening=Some(Arc::new(Mutex::new(Some(Err("Synthetic media host startup failed".into())))));
        let mut opened=UiAction::id("__voice-data","opened");opened.extra=c.voice_lifecycle.media_epoch.to_string();
        assert!(c.voice_complete(opened,json!({"ok":true,"result":{}})).await.is_err());
        assert!(!c.state.voice_connecting&&!c.voice_lifecycle.request_pending&&c.voice_lifecycle.request_channel.is_empty());
        assert_eq!(c.state.voice_error,"Synthetic media host startup failed");
    }
    #[tokio::test]
    async fn receiver_process_failure_preserves_two_participant_voice_and_reopens_with_fresh_events() {
        let (mut c,_,mut replies)=target();c.state.voice_connected=true;c.state.voice_encryption_ready=true;
        for user in ["100","300"] {c.call_people.insert(user.into(),CallParticipant{user:Person{id:user.into(),..Default::default()},is_self:user=="100",screen_sharing:user=="300",..Default::default()});}
        let (main,main_calls)=MediaBridge::fake(|command,_|panic!("A receive failure touched the microphone/voice bridge: {command}"));c.media=Some(main.clone());
        let key="call:200:300";
        for cycle in 0..3 {
            let (receiver,calls)=MediaBridge::fake_receiver(|command,args|match command {
                "watchStream"=>{assert!(args["selfMute"]==true&&args["streamOwnerId"]=="300");Ok(json!({"session":1}))},
                "stopWatchStream"|"shutdown"|"configureReceiver"=>Ok(json!({})),_=>panic!("Unexpected receiver command: {command}"),
            },main.video_frames());
            let id=receiver.fake_video_id();c.voice_lifecycle.receiver_epoch+=1;let epoch=c.voice_lifecycle.receiver_epoch;c.voice_lifecycle.receiver=Some(receiver.clone());
            let token=20+cycle;c.voice_lifecycle.watches.insert(key.into(),json!({"owner":"300","server":"400","channel":"401","endpoint":"offline.discord.media","token":"SYNTHETIC","attempt":token,"started":false}));
            c.state.call_videos.retain(|video|video.stream_key!=key);
            c.state.call_videos.push(CallVideo{id,user_id:"300".into(),kind:"screen".into(),stream_key:key.into(),status:"connecting".into(),..Default::default()});
            c.try_watch(key).await.unwrap();reply(&mut c,&mut replies).await.unwrap();
            c.media_event(json!({"event":"receiver-host","epoch":epoch,"value":{"event":"videoStream","id":id,"mediaSession":1,"streamKey":key,"userId":"300","kind":"screen","active":true,"state":"playing","token":token}})).await.unwrap();
            assert!(c.state.call_videos.iter().any(|video|video.id==id&&video.active));
            if cycle==0 {c.stop_watch(key).await.unwrap();wait_named_calls(&calls,"stopWatchStream",1).await;}
            else {
                let failed=if cycle==1 {json!({"event":"fatal"})}else{json!({"event":"videoError","scope":"stream","userId":"300","mediaSession":1,"message":"Synthetic decoder failure"})};
                c.media_event(json!({"event":"receiver-host","epoch":epoch,"value":failed})).await.unwrap();
                assert!(c.state.call_videos.iter().any(|video|video.stream_key==key&&video.status=="error"));
            }
            assert!(c.state.voice_connected&&c.state.voice_encryption_ready&&c.voice_channel=="200");
            assert_eq!(c.call_people.len(),2);assert!(c.media.as_ref().unwrap().same_instance(&main));
            assert!(c.voice_lifecycle.receiver.is_none()&&c.voice_lifecycle.watches.is_empty());
            c.media_event(json!({"event":"receiver-host","epoch":epoch,"value":{"event":"fatal"}})).await.unwrap();
            assert!(c.state.voice_connected&&main_calls.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn known_peer_partial_mute_and_deaf_preserve_screen_watch_until_explicit_leave() {
        let (mut c,_,_)=target();c.state.voice_connected=true;c.state.voice_encryption_ready=true;
        let (main,main_calls)=MediaBridge::fake(|command,_|panic!("Peer metadata touched main audio: {command}"));c.media=Some(main.clone());
        let (receiver,receiver_calls)=MediaBridge::fake_receiver(|_,_|Ok(json!({})),main.video_frames());c.voice_lifecycle.receiver=Some(receiver);
        c.call_people.insert("300".into(),CallParticipant {user:Person {id:"300".into(),name:"Synthetic peer".into(),..Default::default()},self_video:true,screen_sharing:true,..Default::default()});
        let key="call:200:300";c.voice_lifecycle.watches.insert(key.into(),json!({"owner":"300","attempt":3,"started":true}));
        c.state.call_videos.push(CallVideo {id:10,user_id:"300".into(),kind:"screen".into(),stream_key:key.into(),active:true,..Default::default()});
        for payload in [json!({"user_id":"300","self_mute":true}),json!({"user_id":"300","self_deaf":true})] {
            c.dispatch("VOICE_STATE_UPDATE",payload).await.unwrap();
            let peer=&c.call_people["300"];assert!(peer.self_mute&&peer.self_video&&peer.screen_sharing);assert_eq!(peer.user.name,"Synthetic peer");
            assert!(c.voice_lifecycle.watches.contains_key(key)&&c.state.call_videos.iter().any(|video|video.active));
            assert!(c.state.voice_connected&&receiver_calls.lock().unwrap().is_empty());
        }
        assert!(c.call_people["300"].self_deaf);
        c.dispatch("VOICE_STATE_UPDATE",json!({"user_id":"300","channel_id":null})).await.unwrap();
        wait_named_calls(&receiver_calls,"shutdown",1).await;
        assert!(!c.call_people.contains_key("300")&&c.voice_lifecycle.watches.is_empty()&&c.state.call_videos.is_empty());
        assert!(c.state.voice_connected&&c.media.as_ref().unwrap().same_instance(&main)&&main_calls.lock().unwrap().is_empty());
    }
}
