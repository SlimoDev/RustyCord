use crate::{
    native::{self, ObjectRef},
    protocol::{self, Credentials, Frame, MediaError, Result},
    session::{Session, ShareSelection},
    settings::AudioSettings,
    stats,
};
use napi::{
    Env, JsFunction, JsObject, JsUnknown,
    threadsafe_function::{ErrorStrategy, ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::{Read, Write},
    os::windows::io::AsRawHandle,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::Message;
#[path = "host/dispatch_queue.rs"]
mod dispatch_queue;
#[path = "host/voice_socket.rs"]
mod voice_socket;
#[path = "video.rs"]
pub(crate) mod video;

#[derive(Clone)]
pub enum Tag {
    Inventory { request: Value, part: String },
    Configure { request: Value, part: String },
    Connected(u64),
    Modes(u64, Value, Value),
    Codecs(u64, Value),
    Key(u64, u64),
    Proposals(u64, u64),
    Commit(u64, u64, u16),
    Transition(u64, u64, u16),
    Speaking(u64),

    Video(u64),
    VideoCodecError(u64),
    ConnectionFailed(u64),
    MlsFailure(u64),
    MlsState(u64),
    SourceEnded(u64),
    Soundshare(u64),
    SoundshareFailed(u64),
    MicLevel,
    Stats { request: Value, session: u64 },
}
pub enum Event {
    Request(Value),
    Ready,
    Closed,
    Tick,
    WsOpen(u64),
    WsFrame(u64, Frame),
    WsError(u64, &'static str, MediaError),
    Callback(Tag, Vec<Value>),
    PreviewResult {
        token: u64,
        source: String,
        ok: bool,
    },
}
type Sender = dispatch_queue::Sender;
thread_local! {static HOST:RefCell<Option<Host>>=const{RefCell::new(None)};}
pub struct Host {
    env: Env,
    engine: ObjectRef,
    initialized: bool,
    receiver_only: bool,
    sender: Sender,
    output: mpsc::Sender<Value>,
    closing: Arc<AtomicBool>,
    runtime: tokio::runtime::Runtime,
    sessions: HashMap<u64, Session>,
    connections: HashMap<u64, ObjectRef>,
    sockets: HashMap<u64, voice_socket::Socket>,
    voice: Option<u64>,
    share: Option<u64>,
    sequence: u64,
    settings: AudioSettings,
    inventories: HashMap<String, (Instant, HashSet<String>)>,
    pending: HashMap<String, (Value, Instant)>,
    inventory_parts: HashMap<String, Value>,
    configure_settings: HashMap<String, AudioSettings>,
    stats_parts: HashMap<String, Value>,
    previous_stats: HashMap<u64, (Instant, Value)>,
    mic_test: Option<Instant>,
    mic_stop_failed: bool,
    mic_token: u64,
    share_token: u64,
    mic_last: Option<Instant>,
    data_dir: String,
    video: video::VideoState,
    watches: HashMap<String, u64>,
    watch_tokens: HashMap<String, u64>,
}
fn error_to_napi(e: MediaError) -> napi::Error {
    napi::Error::from_reason(e.message)
}
pub fn start(env: Env, engine: JsObject, pipe: String, data_dir: String) -> napi::Result<()> {
    trace("start");
    if !valid_pipe(&pipe) {
        return Err(napi::Error::from_reason("Invalid private media pipe."));
    }
    let (output, receiver) = mpsc::channel::<Value>();
    let closing = Arc::new(AtomicBool::new(false));
    let mailbox = Arc::new(dispatch_queue::Mailbox::new(closing.clone(), output.clone()));
    let dispatch_mailbox = mailbox.clone();
    let noop = env.create_function_from_closure("dispatch", |ctx| ctx.env.get_undefined())?;
    let wake: ThreadsafeFunction<(), ErrorStrategy::Fatal> = noop.create_threadsafe_function(dispatch_queue::WAKE_CAPACITY, move |ctx| {
        let _: () = ctx.value;
        trace("actor");
        HOST.with(|slot| {
            let current = slot.borrow_mut().take();
            if let Some(mut host) = current {
                for _ in 0..dispatch_queue::BATCH {
                    let Some(delivery) = dispatch_mailbox.pop() else { break; };
                    let bytes = delivery.bytes;
                    host.dispatch(delivery.event);
                    dispatch_mailbox.complete(bytes);
                    if host.closing.load(Ordering::Acquire) { break; }
                }
                let sender = host.sender.clone();
                *slot.borrow_mut() = Some(host);
                sender.finish_dispatch();
            } else {
                trace("actor-empty");
                dispatch_mailbox.actor_unavailable();
            }
        });
        Ok(Vec::<JsUnknown>::new())
    })?;
    let sender = Sender::new(wake, mailbox);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| napi::Error::from_reason("Could not create media runtime."))?;
    let host = Host {
        env,
        engine: ObjectRef::new(env, engine).map_err(error_to_napi)?,
        initialized: false,
        receiver_only: false,
        sender: sender.clone(),
        output,
        closing: closing.clone(),
        runtime,
        sessions: HashMap::new(),
        connections: HashMap::new(),
        sockets: HashMap::new(),
        voice: None,
        share: None,
        sequence: 0,
        settings: AudioSettings::default(),
        inventories: HashMap::new(),
        pending: HashMap::new(),
        inventory_parts: HashMap::new(),
        configure_settings: HashMap::new(),
        stats_parts: HashMap::new(),
        previous_stats: HashMap::new(),
        mic_test: None,
        mic_stop_failed: false,
        mic_token: 0,
        share_token: 0,
        mic_last: None,
        data_dir,
        video: video::VideoState::new(closing.clone()),
        watches: HashMap::new(),
        watch_tokens: HashMap::new(),
    };
    HOST.with(|slot| *slot.borrow_mut() = Some(host));
    let io_sender = sender.clone();
    let io_closing = closing.clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let file = loop {
            match OpenOptions::new().read(true).write(true).open(&pipe) {
                Ok(f) => break f,
                Err(_) => {
                    if Instant::now() >= deadline {
                        let _ =
                            io_sender.call(Event::Closed, ThreadsafeFunctionCallMode::NonBlocking);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
            }
        };
        trace("pipe-connected");
        let _ = io_sender.call(Event::Ready, ThreadsafeFunctionCallMode::NonBlocking);
        // A synchronous duplex Windows handle serializes ReadFile/WriteFile, even
        // after DuplicateHandle. Peek first so a blocked read cannot stall replies.
        let mut input = file;
        let mut frame = Vec::new();
        let mut buf = [0; 2048];
        loop {
            for value in receiver.try_iter() {
                let mut bytes = match serde_json::to_vec(&value) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                bytes.push(b'\n');
                if input.write_all(&bytes).and_then(|_| input.flush()).is_err() {
                    let _ = io_sender.call(Event::Closed, ThreadsafeFunctionCallMode::NonBlocking);
                    return;
                }
            }
            if io_closing.load(Ordering::Relaxed) {
                break;
            }
            let mut available = 0u32;
            let ok = unsafe {
                PeekNamedPipe(
                    input.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                break;
            }
            if available == 0 {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            let count = (available as usize).min(buf.len());
            match input.read(&mut buf[..count]) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for b in &buf[..n] {
                        if *b == b'\n' {
                            if !frame.is_empty() {
                                match serde_json::from_slice::<Value>(&frame) {
                                    Ok(v) => {
                                        let _ = io_sender.call(
                                            Event::Request(v),
                                            ThreadsafeFunctionCallMode::NonBlocking,
                                        );
                                    }
                                    Err(_) => {
                                        let _ = io_sender.call(
                                            Event::Request(Value::Null),
                                            ThreadsafeFunctionCallMode::NonBlocking,
                                        );
                                    }
                                }
                                frame.clear();
                            }
                        } else if *b != b'\r' {
                            frame.push(*b);
                            if frame.len() > protocol::MAX_FRAME {
                                let _ = io_sender
                                    .call(Event::Closed, ThreadsafeFunctionCallMode::NonBlocking);
                                return;
                            }
                        }
                    }
                }
            }
        }
        let _ = io_sender.call(Event::Closed, ThreadsafeFunctionCallMode::NonBlocking);
    });
    std::thread::spawn(move || {
        while !closing.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
            let _ = sender.call(Event::Tick, ThreadsafeFunctionCallMode::NonBlocking);
        }
    });
    Ok(())
}
fn trace(stage: &str) {
    if std::env::var_os("NOTDISCORD_MEDIA_TRACE").is_some() {
        eprintln!("rust-media:{stage}");
    }
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn PeekNamedPipe(
        handle: *mut std::ffi::c_void,
        buffer: *mut std::ffi::c_void,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
}
fn valid_pipe(pipe: &str) -> bool {
    let Some(id) = pipe.strip_prefix(r"\\.\pipe\NotDiscord.Media.") else {
        return false;
    };
    (id.len() == 32 || id.len() == 36) && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
impl Host {
    fn emit(&self, value: Value) {
        let _ = self.output.send(value);
    }
    fn reply(&mut self, id: Value, result: Result<Value>) {
        let key = id.to_string();
        self.pending.remove(&key);
        match result {
            Ok(v) => self.emit(json!({"id":id,"ok":true,"result":v})),
            Err(e) => self.emit(json!({"id":id,"ok":false,"error":e.json()})),
        }
    }
    fn engine(&self) -> Result<JsObject> {
        self.engine.object(self.env)
    }
    fn eng(&self, name: &str, args: Vec<Value>) -> Result<JsUnknown> {
        native::call_json(self.env, &self.engine()?, name, args)
    }
    fn conn(&self, id: u64) -> Result<JsObject> {
        self.connections
            .get(&id)
            .ok_or_else(|| {
                MediaError::new(
                    "INVALID_VOICE_ORDER",
                    "Voice encryption arrived before native transport creation.",
                )
            })?
            .object(self.env)
    }
    fn conn_call(&self, id: u64, name: &str, args: Vec<Value>) -> Result<JsUnknown> {
        let connection=self.conn(id)?;
        if name=="setLocalVolume" {
            let user=protocol::snowflake(args.first().unwrap_or(&Value::Null),"userId")?;
            let gain=args.get(1).and_then(Value::as_f64).ok_or_else(MediaError::internal)?;
            native::participant_gain(self.env,&connection,&user,gain)?;
            return self.env.get_undefined().map(|value|value.into_unknown()).map_err(|_|MediaError::internal());
        }
        if name=="mergeUsers" {return native::merge_participants(self.env,&connection,args.into_iter().next().unwrap_or(Value::Null));}
        native::call_json(self.env,&connection,name,args)
    }
    fn conn_optional(&self, id: u64, name: &str, args: Vec<Value>) -> Result<()> {
        native::optional_json(self.env, &self.conn(id)?, name, args)
    }
    fn callback(&self, tag: Tag) -> Result<JsFunction> {
        let sender = self.sender.clone();
        self.env
            .create_function_from_closure("mediaCallback", move |ctx| {
                if sender.is_closing() { return ctx.env.get_undefined(); }
                let mut values = Vec::new();
                for i in 0..ctx.length.min(8) {
                    let value = ctx.get::<JsUnknown>(i)?;
                    match native::read_value(*ctx.env, value) {
                        Ok(value) => values.push(value),
                        Err(_) => { sender.reject_callback(); return ctx.env.get_undefined(); }
                    }
                }
                let _ = sender.call(
                    Event::Callback(tag.clone(), values),
                    ThreadsafeFunctionCallMode::NonBlocking,
                );
                ctx.env.get_undefined()
            })
            .map_err(|_| MediaError::internal())
    }
    fn eng_callback(&self, name: &str, mut args: Vec<JsUnknown>, tag: Tag) -> Result<()> {
        args.push(self.callback(tag)?.into_unknown());
        native::call(self.env, &self.engine()?, name, &args).map(|_| ())
    }
    fn conn_callback(&self, id: u64, name: &str, mut args: Vec<JsUnknown>, tag: Tag) -> Result<()> {
        args.push(self.callback(tag)?.into_unknown());
        native::call(self.env, &self.conn(id)?, name, &args).map(|_| ())
    }
    fn json(&self, v: Value) -> Result<JsUnknown> {
        native::json_value(self.env, v)
    }
    fn initialize(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }
        let engine = self.engine()?;
        let allocator = self
            .env
            .create_function_from_closure("allocateImage", |ctx| {
                let width = ctx.get::<napi::JsNumber>(0)?.get_uint32()?;
                let height = ctx.get::<napi::JsNumber>(1)?.get_uint32()?;
                if width == 0
                    || height == 0
                    || width > 8192
                    || height > 8192
                    || width as u64 * height as u64 * 4 > 4096 * 2160 * 4
                {
                    return Err(napi::Error::from_reason(
                        "Native video allocation exceeds its bound.",
                    ));
                }
                let mut obj = ctx.env.create_object()?;
                obj.set_named_property("width", width)?;
                obj.set_named_property("height", height)?;
                let length = width as usize * height as usize * 4;
                let buffer = ctx
                    .env
                    .create_arraybuffer(length)?
                    .into_raw()
                    .into_typedarray(napi::TypedArrayType::Uint8Clamped, length, 0)?;
                obj.set_named_property("data", buffer)?;
                Ok(obj)
            })
            .map_err(|_| MediaError::internal())?;
        native::call(
            self.env,
            &engine,
            "setImageDataAllocator",
            &[allocator.into_unknown()],
        )?;
        std::fs::create_dir_all(&self.data_dir).map_err(|_| MediaError::internal())?;
        self.emit(json!({"event":"mediaStage","stage":"initialize-before","kind":"native"}));
        let mut initialization = json!({"audioSubsystem":"experimental","logLevel":-1,"dataDirectory":self.data_dir,"logDirectory":self.data_dir,"maxLogBytes":65536,"offloadAdmControls":true,"asyncVideoInputDeviceInit":true});
        #[cfg(feature = "native-probe")]
        if crate::probe_window::source().is_some() {
            initialization["useFakeAudioCapture"] = json!(true);
            initialization["useFakeVideoCapture"] = json!(true);
        }
        self.eng("initialize", vec![initialization])?;
        self.emit(json!({"event":"mediaStage","stage":"initialize-after","kind":"native"}));
        self.eng("setTransportOptions",vec![json!({"builtInEchoCancellation":true,"echoCancellation":true,"noiseSuppression":true,"automaticGainControl":true,"noiseCancellation":false,"idleJitterBufferFlush":true,"ducking":false})])?;
        self.eng("setInputDevice", vec![json!("disabled")])?;
        #[cfg(feature = "native-probe")]
        let probe = crate::probe_window::source().is_some();
        #[cfg(not(feature = "native-probe"))]
        let probe = false;
        self.eng(
            "setOutputDevice",
            vec![json!(if probe { "disabled" } else { "default" })],
        )?;
        self.eng("setVideoInputDevice", vec![json!("disabled")])?;
        self.eng("setInputVolume", vec![json!(1)])?;
        self.eng("setOutputVolume", vec![json!(1)])?;
        self.initialized = true;
        self.emit(json!({"event":"mediaStage","stage":"audio-configured","kind":"native"}));
        Ok(())
    }
    fn apply_audio(&self, settings: &AudioSettings, capture: bool) -> Result<()> {
        self.eng(
            "setInputDevice",
            vec![json!(if capture { &settings.input } else { "disabled" })],
        )?;
        self.eng("setOutputDevice", vec![json!(settings.output)])?;
        self.eng("setInputVolume", vec![json!(settings.input_volume / 100.)])?;
        self.eng(
            "setOutputVolume",
            vec![json!(self.output_gain(settings))],
        )?;
        self.eng("setTransportOptions", vec![settings.processing()])?;
        Ok(())
    }
    fn output_gain(&self, settings:&AudioSettings)->f64 {
        let stream=if self.receiver_only {settings.stream_volume_percent/100.} else {1.};
        settings.output_volume/100.*stream
    }
    fn input_mode(&self, id: u64, settings: &AudioSettings) -> Result<()> {
        self.conn_call(
            id,
            "setPTTActive",
            vec![json!(false), json!(false), json!(false)],
        )?;
        self.conn_call(id, "setTransportOptions", vec![settings.mode()])?;
        Ok(())
    }
    fn transmit_gate(&mut self,s:&mut Session,wanted:bool)->Result<bool> {
        let active=wanted&&s.connected&&!s.closed&&!s.send_mute()&&self.mic_test.is_none()&&self.settings.input_volume>0.;
        if s.transmit_gate==active {return Ok(active);}
        if let Err(error)=self.conn_call(s.id,"setPTTActive",vec![json!(active),json!(false),json!(false)]) {
            // Preserve incoming audio if the native transmit control failed.
            // A confirmed hard mute closes the input independently; an explicit
            // user unmute must acknowledge a closed PTT gate before reopening.
            self.conn_call(s.id,"setSelfMute",vec![json!(true)])?;
            s.credentials.self_mute=true;s.reset_input_activity();s.ptt_active=false;
            self.emit(json!({"event":"inputGateState","mediaSession":s.id,"muted":true,"reason":error.json(),"retry":"unmute"}));
            return Ok(false);
        }
        s.transmit_gate=active;
        Ok(active)
    }
    fn voice_id(&self) -> Result<u64> {
        self.voice
            .filter(|id| self.sessions.get(id).is_some_and(|s| !s.closed))
            .ok_or_else(|| MediaError::new("NOT_IN_VOICE", "Join a voice channel first."))
    }
    fn dispatch(&mut self, event: Event) {
        if self.closing.load(Ordering::Relaxed) {
            return;
        }
        match event{
  Event::Ready=>self.emit(json!({"event":"ready","protocol":1,"nativeInitialized":false,"backend":"rust-napi-electron-main","electronHost":true,"chromiumRenderer":false,"originalUiLoaded":false,"browserWindows":0})),
  Event::Closed=>self.shutdown(),
  Event::Tick=>self.tick(),
  Event::PreviewResult{token,source,ok}=>self.preview_result(token,&source,ok),
  Event::Request(request)=>{let id=request["id"].clone();let valid=id.as_str().is_some_and(|s|s.len()<=128)||id.as_i64().is_some();if !valid{self.reply(Value::Null,Err(MediaError::new("INVALID_REQUEST","A short request id is required.")));return}if self.pending.len()>=32{self.reply(id,Err(MediaError::new("QUEUE_LIMIT","Too many media requests.")));return}self.pending.insert(id.to_string(),(id.clone(),Instant::now()+Duration::from_secs(30)));let cmd=request["command"].as_str().unwrap_or("");match self.command(id.clone(),cmd,&request["params"]){Ok(Some(value))=>self.reply(id,Ok(value)),Ok(None)=>{},Err(e)=>self.reply(id,Err(e))}},
  Event::WsOpen(id)=>{if let Some(s)=self.sessions.get(&id){let p=&s.credentials;self.stage(s,"websocket-open");self.send(id,0,json!({"server_id":p.server_id,"channel_id":p.channel_id,"user_id":p.user_id,"session_id":p.session_id,"token":p.voice_token,"max_dave_protocol_version":1,"video":true,"streams":[{"type":"video","rid":"100","quality":100,"active":false}]}));}},
  Event::WsFrame(id,frame)=>{if let Some(mut s)=self.sessions.remove(&id){let result=self.frame(&mut s,frame);if let Err(e)=result{self.fail(&mut s,e)}if !s.closed{self.sessions.insert(id,s);}}},
  Event::WsError(id,phase,error)=>{if let Some(mut s)=self.sessions.remove(&id){
    // Fixed phase/code vocabulary and an ephemeral session generation only.
    // Preserve this cause before close removes transport/capture resources.
    self.emit(json!({"event":"transportError","mediaSession":id,"phase":if error.code=="VOICE_HEARTBEAT_TIMEOUT" {"heartbeat"}else{phase},"code":error.code}));
    self.fail(&mut s,error);
  }},
  Event::Callback(tag,values)=>{if let Err(e)=self.native_event(tag.clone(),values){match tag.session(){Some(id)=>{if let Some(mut s)=self.sessions.remove(&id){self.fail(&mut s,e)}},None=>{if let Some(id)=tag.request(){self.reply(id,Err(e));}}}}},
 }
    }
    fn command(&mut self, id: Value, name: &str, p: &Value) -> Result<Option<Value>> {
        match name {
            #[cfg(feature = "native-probe")]
            "probeOwnedSource" => {
                let source = crate::probe_window::source().ok_or_else(|| {
                    MediaError::new(
                        "OWNED_SOURCE_INVALID",
                        "The fixture HWND/PID identity did not match.",
                    )
                })?;
                self.inventories.insert(
                    "sources".into(),
                    (Instant::now(), HashSet::from([source.clone()])),
                );
                Ok(Some(
                    json!({"id":source,"pid":std::process::id(),"identityVerified":true,"nativeCatalogVerified":false}),
                ))
            }
            #[cfg(feature = "native-probe")]
            "probeOwnedCleanup" => {
                self.video_stop_preview()?;
                let videos = self.video_probe_counts();
                let window = crate::probe_window::close();
                let reentrant_detach =
                    video::probe_synthetic_detach(self.env).map_err(|_| MediaError::internal())?;
                Ok(Some(
                    json!({"windowDestroyed":window,"videos":videos,"voiceSessions":self.sessions.len(),"reentrantDetachSafe":reentrant_detach}),
                ))
            }
            "capabilities" => {
                let version = native::property_json(
                    self.env,
                    &self.engine()?,
                    "SupportedSecureFramesProtocolVersion",
                )
                .unwrap_or(json!(0));
                Ok(Some(
                    json!({"backend":"rust-napi-electron-main","electronHost":true,"chromiumRenderer":false,"originalUiLoaded":false,"browserWindows":0,"voice":true,"dave":version,"screenShare":true,"screenReceive":true,"camera":true,"cameraPreview":true,"videoTransport":"private-binary-latest-rgba","maxVideoStreams":4,"maxVideoWidth":1280,"maxVideoHeight":720,"verifiedLive":false,"reason":"Rust owns media state and bounded raw video delivery; native codecs and DAVE run in the compatible Electron host."}),
                ))
            }
            "stats" => {
                self.stats_start(id)?;
                Ok(None)
            }
            "configureReceiver"=>{
                if self.voice.is_some()||self.share.is_some() {return Err(MediaError::new("RECEIVER_ONLY","Receiver settings cannot modify a microphone or outgoing connection."));}
                self.initialize()?;
                self.receiver_only=true;
                let mut settings=AudioSettings::parse(p)?;settings.input="disabled".into();settings.input_volume=0.;
                if settings.stream_volume_revision<self.settings.stream_volume_revision {
                    settings.stream_volume_percent=self.settings.stream_volume_percent;
                    settings.stream_volume_revision=self.settings.stream_volume_revision;
                }
                // Voice participant gains must never scale screen audio.
                settings.user_volumes.clear();settings.user_volumes_revision=0;
                self.apply_audio(&settings,false)?;self.settings=settings;
                if let Some(visible)=p["videoVisible"].as_bool() {self.video_command("setVideoVisible",&json!({"visible":visible}))?;}
                let deaf=p["selfDeaf"].as_bool().unwrap_or(false);
                let volumes=self.settings.user_volumes.clone();
                for session in self.sessions.values_mut().filter(|session|session.receive()) {
                    session.credentials.self_mute=true;session.credentials.self_deaf=deaf;session.volumes=volumes.clone();
                    for (user,member) in &mut session.members {member["volume"]=json!(volumes.get(user).copied().unwrap_or(1.));}
                }
                for session in self.sessions.values().filter(|session|session.receive()&&self.connections.contains_key(&session.id)) {
                    self.conn_call(session.id,"setSelfMute",vec![json!(true)])?;
                    self.conn_call(session.id,"setSelfDeafen",vec![json!(deaf)])?;
                    for user in session.members.keys() {self.conn_call(session.id,"setLocalVolume",vec![json!(user),json!(volumes.get(user).copied().unwrap_or(1.))])?;}
                }
                Ok(Some(json!({"applied":true,"receiverOnly":true})))
            }
            "setStreamVolume"=>{
                if !self.receiver_only {return Err(MediaError::new("RECEIVER_ONLY","Stream volume belongs to the screen receiver."));}
                let value=p["value"].as_f64().filter(|value|value.is_finite()&&(0.0..=100.0).contains(value))
                    .ok_or_else(||MediaError::new("INVALID_STREAM_VOLUME","Stream volume must be between 0 and 100%."))?;
                let revision=p["revision"].as_u64().ok_or_else(||MediaError::new("INVALID_VOLUME_REVISION","Invalid stream volume revision."))?;
                if revision<self.settings.stream_volume_revision {return Ok(Some(json!({"stale":true})));}
                let mut settings=self.settings.clone();settings.stream_volume_percent=value;settings.stream_volume_revision=revision;
                self.eng("setOutputVolume",vec![json!(self.output_gain(&settings))])?;
                self.settings=settings;
                Ok(Some(json!({"applied":true,"value":value,"revision":revision})))
            }
            "setPeerVideoVisible"=>{
                let user=protocol::snowflake(&p["userId"],"userId")?;
                let visible=p["visible"].as_bool().ok_or_else(MediaError::internal)?;
                let voice=self.voice.ok_or_else(||MediaError::new("VOICE_NOT_CONNECTED","The call is no longer active."))?;
                let mut session=self.sessions.remove(&voice).ok_or_else(MediaError::internal)?;
                let result=(|| {
                    if user==session.credentials.user_id {return Err(MediaError::new("REMOTE_VIDEO_ONLY","Select another participant's camera."));}
                    if visible {
                        if session.camera_receive_users.len()>=4&&!session.camera_receive_users.contains(&user) {return Err(MediaError::new("VIDEO_LIMIT","Close another camera before opening this one."));}
                        session.camera_receive_users.insert(user.clone());
                    } else {
                        session.camera_receive_users.remove(&user);
                        self.video_hide_peer(voice,&user);
                    }
                    self.sync_camera_video(&mut session)?;
                    Ok(Some(json!({"userId":user,"visible":visible})))
                })();
                self.sessions.insert(voice,session);result
            }
            "startCameraPreview"
            | "startScreenPreview"
            | "stopMediaPreview"
            | "stopCameraPreview"
            | "setVideoVisible"
            | "setVideoStreamVisible"
            | "setVideoQuality"
            | "focusVideo" => self.video_command(name, p).map(Some),
            "watchStream" => {
                if p["streamOwnerId"] == p["userId"] || p["streamOwnerId"].as_str().is_none() {
                    return Err(MediaError::new(
                        "INVALID_STREAM_OWNER",
                        "Select another participant's active screen stream.",
                    ));
                }
                let key = protocol::string(&p["streamKey"], "streamKey", 128)?;
                if !self.watch_tokens.contains_key(&key) && self.watch_tokens.len() >= 512 {
                    return Err(MediaError::new("WATCH_HISTORY_LIMIT", "Leave this call before opening more different streams."));
                }
                let current = self.watch_tokens.get(&key).copied().unwrap_or(0);
                let token = operation_token(current, p)?;
                if token <= current {
                    return Err(MediaError::new(
                        "WATCH_CANCELLED",
                        "This stream watch was already cancelled.",
                    ));
                }
                self.watch_tokens.insert(key.clone(), token);
                if self.watches.len() >= 2 && !self.watches.contains_key(&key) {
                    return Err(MediaError::new(
                        "WATCH_LIMIT",
                        "Close another screen stream first.",
                    ));
                }
                if let Some(id) = self.watches.remove(&key) {
                    self.close_id(id);
                }
                self.start_session(p, true, None)?;
                Ok(Some(
                    json!({"state":"connecting","session":self.watches.get(&key)}),
                ))
            }
            "stopWatchStream" => {
                let key = protocol::string(&p["streamKey"], "streamKey", 128)?;
                let current = self.watch_tokens.get(&key).copied().unwrap_or(0);
                let token = operation_token(current, p)?;
                if token < current {
                    return Ok(Some(json!({"stale":true})));
                }
                self.watch_tokens.insert(key.clone(), token);
                if let Some(id) = self.watches.remove(&key) {
                    self.close_id(id);
                }
                Ok(Some(json!({"state":"stopped"})))
            }
            "listDevices" | "configureAudio" => {
                self.initialize()?;
                if name == "configureAudio" {
                    self.configure_settings
                        .insert(id.to_string(), AudioSettings::parse(p)?);
                }
                self.inventory_parts.insert(id.to_string(), json!({}));
                for (part, method) in [("input", "getInputDevices"), ("output", "getOutputDevices")]
                {
                    let tag = if name == "configureAudio" {
                        Tag::Configure {
                            request: id.clone(),
                            part: part.into(),
                        }
                    } else {
                        Tag::Inventory {
                            request: id.clone(),
                            part: part.into(),
                        }
                    };
                    self.eng_callback(method, vec![], tag)?;
                }
                Ok(None)
            }
            "listCameras" => {
                self.initialize()?;
                self.eng_callback(
                    "getVideoInputDevices",
                    vec![],
                    Tag::Inventory {
                        request: id,
                        part: "cameras".into(),
                    },
                )?;
                Ok(None)
            }
            "listSources" => {
                self.initialize()?;
                self.inventory_parts.insert(id.to_string(), json!({}));
                for (part, method) in [
                    ("screen", "getScreenPreviews"),
                    ("window", "getWindowPreviews"),
                ] {
                    self.eng_callback(
                        method,
                        vec![self.json(json!(1))?, self.json(json!(1))?],
                        Tag::Inventory {
                            request: id.clone(),
                            part: part.into(),
                        },
                    )?;
                }
                Ok(None)
            }
            "setDevices" => {
                self.initialize()?;
                let mut values = self.settings.json();
                for key in ["inputId", "outputId"] {
                    if let Some(v) = p.get(key) {
                        values[key] = v.clone();
                    }
                }
                self.settings = AudioSettings::parse(&values)?;
                let capture = self
                    .voice
                    .and_then(|i| self.sessions.get(&i))
                    .is_some_and(|s| s.local_ready && !s.closed)
                    || self.mic_test.is_some();
                self.apply_audio(&self.settings, capture)?;
                if let Some(s) = self.voice.and_then(|i| self.sessions.get_mut(&i)) {
                    s.settings = self.settings.clone();
                }
                Ok(Some(json!({"applied":true})))
            }
            "startMicTest" => {
                let token = operation_token(self.mic_token, p)?;
                if token <= self.mic_token {
                    return Err(MediaError::new(
                        "MIC_TEST_CANCELLED",
                        "This microphone test was already cancelled.",
                    ));
                }
                if self.mic_test.is_some() {
                    self.stop_mic_test()?;
                }
                self.mic_token = token;
                let mut settings = if p.get("settings").is_some() {
                    AudioSettings::parse(&p["settings"])?
                } else {
                    self.settings.clone()
                };
                let monitor_only=match p.get("monitorOnly") {None=>false,Some(value)=>value.as_bool().ok_or_else(||MediaError::new("INVALID_AUDIO_SETTING","monitorOnly must be a boolean."))?};
                if monitor_only {settings.output_volume=0.;}
                for (key, device) in [("input", &settings.input), ("output", &settings.output)] {
                    if device != "default"
                        && !self.inventories.get(key).is_some_and(|(at, ids)| {
                            at.elapsed() < Duration::from_secs(600) && ids.contains(device)
                        })
                    {
                        return Err(MediaError::new(
                            "DEVICE_NOT_LISTED",
                            "Select an available audio device again.",
                        ));
                    }
                }
                self.initialize()?;
                // The guard is armed before any capture/loopback operation. All
                // later mute/PTT/readiness paths also consult this guard.
                self.mic_test = Some(Instant::now() + Duration::from_secs(30));
                self.mic_stop_failed=false;
                if let Some(id) = self.share.take() {
                    self.close_id(id);
                }
                let result = (|| {
                if let Some(id) = self.voice.filter(|id| self.connections.contains_key(id)) {
                    self.conn_call(id, "setSelfMute", vec![json!(true)])?;
                    self.conn_call(id, "setSelfDeafen", vec![json!(true)])?;
                    if let Some(mut session) = self.sessions.remove(&id) {
                        if let Some(event) =
                            session.speaking_event(&session.credentials.user_id.clone(), false)
                        {
                            self.emit(event);
                        }
                        self.send(id, 5, json!({"speaking":0,"delay":0,"ssrc":session.ssrc}));
                        self.sessions.insert(id, session);
                    }
                }
                    self.apply_audio(&settings, true)?;
                    self.eng_callback("setOnVoiceCallback", vec![], Tag::MicLevel)?;
                    native::optional_json(
                        self.env,
                        &self.engine()?,
                        "setEmitVADLevel2",
                        vec![json!(true)],
                    )?;
                    // The native engine starts capture for the local meter via
                    // its loopback path. Silent monitoring arms that capture
                    // only after apply_audio has acknowledged output gain 0.
                    self.eng("setLoopback", vec![json!(true), settings.processing()])?;
                    Ok(())
                })();
                if let Err(error) = result {
                    let _ = self.stop_mic_test();
                    return Err(error);
                }
                self.settings = settings;
                if let Some(session) = self.voice.and_then(|id| self.sessions.get_mut(&id)) {
                    session.settings = self.settings.clone();
                }
                self.emit(json!({"event":"micTest","active":true,"pending":false,"level":0,"db":-100,"speaking":false,"token":self.mic_token,"localOnly":true,"temporaryDeaf":self.voice.is_some()}));
                Ok(Some(
                    json!({"active":true,"localOnly":true,"monitorOnly":monitor_only,"maximumSeconds":30,"token":self.mic_token}),
                ))
            }
            "stopMicTest" => {
                let token = operation_token(self.mic_token, p)?;
                if token < self.mic_token {
                    return Ok(Some(
                        json!({"active":self.mic_test.is_some(),"stale":true,"token":self.mic_token}),
                    ));
                }
                self.mic_token = token;
                self.stop_mic_test()?;
                Ok(Some(
                    json!({"active":false,"localOnly":true,"token":self.mic_token}),
                ))
            }
            "connectVoice" => {
                self.stop_mic_test()?;
                self.video_stop_preview()?;
                self.close_all();
                self.start_session(p, false, None)?;
                Ok(Some(
                    json!({"state":"connecting","verifiedLive":false,"session":self.voice}),
                ))
            }
            "disconnect" => {
                self.stop_mic_test()?;
                self.close_all();
                Ok(Some(json!({"state":"disconnected"})))
            }
            "mute" | "deafen" | "ptt" | "startCamera" | "stopCamera" | "setVolume" => {
                self.voice_command(id, name, p)
            }
            "startShare" => {
                let token = operation_token(self.share_token, p)?;
                if token <= self.share_token {
                    return Err(MediaError::new(
                        "SHARE_CANCELLED",
                        "This screen selection was already cancelled.",
                    ));
                }
                if self.mic_test.is_some() {
                    return Err(MediaError::new(
                        "MIC_TEST_ACTIVE",
                        "Stop the local microphone test before sharing screen or audio.",
                    ));
                }
                let voice = self.voice_id()?;
                if !self.sessions[&voice].connected {
                    return Err(MediaError::new(
                        "VOICE_NOT_READY",
                        "Wait for encrypted voice transport before sharing.",
                    ));
                }
                let selection = ShareSelection::parse(p)?;
                if !self.inventories.get("sources").is_some_and(|(at, ids)| {
                    at.elapsed() < Duration::from_secs(600) && ids.contains(&selection.source)
                }) {
                    return Err(MediaError::new(
                        "SOURCE_NOT_LISTED",
                        "Open the source picker again and select an available source.",
                    ));
                }
                if let Some(id) = self.share.take() {
                    self.close_id(id)
                }
                self.share_token = token;
                self.start_session(p, true, Some(selection))?;
                Ok(Some(
                    json!({"state":"connecting","verifiedLive":false,"session":self.share}),
                ))
            }
            "stopShare" => {
                let token = operation_token(self.share_token, p)?;
                if token < self.share_token {
                    return Ok(Some(json!({"state":"unchanged","stale":true})));
                }
                self.share_token = token;
                if let Some(id) = self.share.take() {
                    self.close_id(id)
                }
                Ok(Some(json!({"state":"stopped"})))
            }
            "shutdown" => {
                self.reply(id, Ok(json!({"state":"closed"})));
                self.shutdown();
                Ok(None)
            }
            _ => Err(MediaError::new("UNKNOWN_COMMAND", "Unknown media command.")),
        }
    }
    fn stop_mic_test(&mut self) -> Result<()> {
        if self.mic_test.is_none() {
            return Ok(());
        }
        let result=(|| {
        // Never release network mute unless native loopback was stopped first.
        self.eng("setLoopback", vec![json!(false), json!({})])?;
        let capture=self.voice.and_then(|id|self.sessions.get(&id)).is_some_and(|session|session.local_ready&&!session.closed);
        self.apply_audio(&self.settings,capture)?;
        if let Some(id) = self.voice.filter(|id| self.connections.contains_key(id)) {
            if let Some(session) = self.sessions.get(&id) {
                self.conn_call(
                    id,
                    "setSelfDeafen",
                    vec![json!(session.credentials.self_deaf)],
                )?;
                self.input_mode(id, &session.settings)?;
                self.conn_call(id, "setSelfMute", vec![json!(session.send_mute())])?;
            }
            if let Some(session)=self.sessions.get_mut(&id) {session.reset_input_activity();}
            self.eng_callback("setOnVoiceCallback", vec![], Tag::MicLevel)?;
            native::optional_json(
                self.env,
                &self.engine()?,
                "setEmitVADLevel2",
                vec![json!(true)],
            )?;
        } else {
            self.eng("setInputDevice", vec![json!("disabled")])?;
            native::optional_json(
                self.env,
                &self.engine()?,
                "setEmitVADLevel2",
                vec![json!(false)],
            )?;
        }
        Ok(())
        })();
        if let Err(error)=result {
            // Keep the transport and test guard alive, with capture closed,
            // rather than tearing down voice for a local monitor failure.
            self.mic_stop_failed=true;
            let _=self.eng("setInputDevice",vec![json!("disabled")]);
            if let Some(id)=self.voice.filter(|id|self.connections.contains_key(id)) {
                let _=self.conn_call(id,"setPTTActive",vec![json!(false),json!(false),json!(false)]);
                let _=self.conn_call(id,"setSelfMute",vec![json!(true)]);
                let _=self.conn_call(id,"setSelfDeafen",vec![json!(true)]);
            }
            self.emit(json!({"event":"micTest","active":false,"pending":true,"cleanupFailed":true,"level":0,"db":-100,"token":self.mic_token,"localOnly":true,"temporaryDeaf":self.voice.is_some()}));
            return Err(error);
        }
        self.mic_test=None;self.mic_stop_failed=false;
        self.emit(json!({"event":"micTest","active":false,"pending":false,"level":0,"db":-100,"speaking":false,"token":self.mic_token,"localOnly":true,"temporaryDeaf":false}));
        Ok(())
    }
    fn shutdown(&mut self) {
        let _ = self.stop_mic_test();
        self.close_all();
        #[cfg(feature = "native-probe")]
        crate::probe_window::close();
        self.closing.store(true, Ordering::Relaxed);
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            std::process::exit(0);
        });
    }
    fn stage(&self, s: &Session, name: &str) {
        let mut event=json!({"event":"mediaStage","stage":name,"kind":s.kind()});
        if s.receive() {event["streamKey"]=json!(s.credentials.stream_key);event["token"]=json!(s.operation_token);event["mediaSession"]=json!(s.id);}
        self.emit(event);
    }
    fn send(&self, id: u64, op: u64, data: Value) {
        if let Some(socket) = self.sockets.get(&id) {
            if let Err(error) = socket.send(Message::Text(json!({"op":op,"d":data}).to_string().into())) {
                if socket.fail_once() {
                    let _ = self.sender.call(Event::WsError(id, "enqueue", error), ThreadsafeFunctionCallMode::NonBlocking);
                }
            }
        }
    }
    fn send_binary(&self, id: u64, op: u8, bytes: &[u8]) -> Result<()> {
        let data = protocol::client_binary(op, bytes)?;
        let socket = self.sockets.get(&id).ok_or_else(|| MediaError::new("VOICE_SEND_CLOSED", "La conexión de voz ya se cerró."))?;
        if let Err(error) = socket.send(Message::Binary(data.into())) {
            if socket.fail_once() {
                let _ = self.sender.call(Event::WsError(id, "enqueue", error.clone()), ThreadsafeFunctionCallMode::NonBlocking);
            }
            return Err(error);
        }
        Ok(())
    }
    fn close_id(&mut self, id: u64) {
        if let Some(mut s) = self.sessions.remove(&id) {
            self.close(&mut s, true)
        }
    }
    fn close_all(&mut self) {
        let _ = self.video_stop_preview();
        let ids: Vec<_> = self.sessions.keys().copied().collect();
        for id in ids {
            self.close_id(id)
        }
        self.voice = None;
        self.share = None;
        self.watches.clear();
        self.watch_tokens.clear();
    }
}
impl Tag {
    fn session(&self) -> Option<u64> {
        match self {
            Self::Connected(i)
            | Self::Speaking(i)
            | Self::Video(i)
            | Self::VideoCodecError(i)
            | Self::ConnectionFailed(i)
            | Self::MlsFailure(i)
            | Self::MlsState(i)
            | Self::SourceEnded(i)
            | Self::Soundshare(i)
            | Self::SoundshareFailed(i)
            | Self::Modes(i, ..)
            | Self::Codecs(i, ..)
            | Self::Key(i, ..)
            | Self::Proposals(i, ..)
            | Self::Commit(i, ..)
            | Self::Transition(i, ..) => Some(*i),
            Self::Stats { session, .. } => Some(*session),
            _ => None,
        }
    }
    fn request(&self) -> Option<Value> {
        match self {
            Self::Inventory { request, .. }
            | Self::Configure { request, .. }
            | Self::Stats { request, .. } => Some(request.clone()),
            _ => None,
        }
    }
}

impl Host {
    fn start_session(
        &mut self,
        p: &Value,
        stream: bool,
        selection: Option<ShareSelection>,
    ) -> Result<()> {
        self.initialize()?;
        let engine = self.engine()?;
        let version =
            native::property_json(self.env, &engine, "SupportedSecureFramesProtocolVersion")?
                .as_u64()
                .unwrap_or(0);
        if version < 1 {
            return Err(MediaError::new(
                "DAVE_UNAVAILABLE",
                "The media engine does not support required DAVE encryption.",
            ));
        }
        let credentials = Credentials::parse(p, stream)?;
        self.sequence += 1;
        let id = self.sequence;
        let endpoint = credentials.endpoint.clone();
        let mut s = Session::new(id, credentials, self.settings.clone());
        if !s.receive() && let Some(volumes)=p.get("participantVolumes") {
            let revision=p["userVolumesRevision"].as_u64().unwrap_or(0);
            if revision>=self.settings.user_volumes_revision {s.volumes=protocol::user_volumes(volumes)?;}
        }
        if s.receive() {s.volumes.clear();}
        s.volumes.remove(&s.credentials.user_id);
        s.operation_token = p["token"].as_u64().unwrap_or(0);
        s.suspended_share = selection;
        self.emit(s.state_event("connecting", None));
        self.sockets.insert(id, voice_socket::spawn(self.runtime.handle(), endpoint, id, self.sender.clone()));
        if s.receive() {
            self.watches.insert(s.credentials.stream_key.clone(), id);
        } else if stream {
            self.share = Some(id)
        } else {
            self.voice = Some(id)
        }
        self.sessions.insert(id, s);
        Ok(())
    }
    fn frame(&mut self, s: &mut Session, frame: Frame) -> Result<()> {
        if s.closed {
            return Ok(());
        }
        if let Some(seq) = frame.seq {
            s.seq = seq as i64;
        }
        let d = frame.data;
        match frame.op {
            8 => {
                let interval = d["heartbeat_interval"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 1000.)
                    .ok_or_else(|| {
                        MediaError::new("INVALID_HELLO", "Invalid voice heartbeat interval.")
                    })?;
                s.heartbeat_interval = Some(Duration::from_millis(interval.min(15000.) as u64));
                self.beat(s)?;
            }
            3 => self.beat(s)?,
            6 => s.heartbeat_pending = None,
            2 => self.ready(s, d)?,
            4 => self.describe(s, d)?,
            5 => {
                let id = d["user_id"].as_str().unwrap_or("");
                self.add_user(s, id, d["ssrc"].as_u64().unwrap_or(0))?;
                let flags = d["speaking"].as_u64().unwrap_or(0);
                self.conn_call(
                    s.id,
                    "setRemoteUserSpeakingStatus",
                    vec![json!(id), json!(flags)],
                )?;
            }
            11 => {
                if let Some(ids) = d["user_ids"].as_array() {
                    for id in ids {
                        self.add_user(s, id.as_str().unwrap_or(""), 0)?;
                    }
                }
            }
            12 => {
                let id = d["user_id"].as_str().unwrap_or("");
                self.add_user(s, id, d["audio_ssrc"].as_u64().unwrap_or(0))?;
                s.video_user(
                    id,
                    d["audio_ssrc"].as_u64().unwrap_or(0),
                    d["video_ssrc"].as_u64().unwrap_or(0),
                    d["rtx_ssrc"].as_u64().unwrap_or(0),
                    &d["streams"],
                );
                if let Some(user) = s.members.get(id) {
                    if self.connections.contains_key(&s.id) {
                        if let Err(error) = self.conn_call(s.id, "mergeUsers", vec![json!([user])])
                        {
                            self.video_error(s.id, id, error);
                        }
                    }
                }
                let has_video = s.members.get(id).is_some_and(|user| {
                    user["videoSsrcs"]
                        .as_array()
                        .is_some_and(|streams| !streams.is_empty())
                });
                if !has_video {
                    self.video_remove_user(s.id, id);
                }
                self.sync_receive_video(s)?;
                self.update_camera_subscription(s);
                if !s.stream() {
                    self.emit(
                        json!({"event":"participant","userId":id,"present":true,"video":has_video}),
                    );
                }
            }
            13 => {
                if let Some(id) = d["user_id"].as_str() {
                    s.members.remove(id);
                    self.video_remove_user(s.id, id);
                    s.speaking.remove(id);
                    self.conn_call(s.id, "destroyUser", vec![json!(id)])?;
                    self.sync_receive_video(s)?;
                    s.camera_receive_users.remove(id);
                    self.update_camera_subscription(s);
                    if !s.stream() {
                        self.emit(json!({"event":"participant","userId":id,"present":false}));
                    }
                    self.update_readiness(s)?;
                }
            }
            14 => {
                if d["audio_codec"].as_str().filter(|codec|!codec.is_empty()).is_some_and(|codec|codec!="opus") {
                    return Err(MediaError::new(
                        "UNSUPPORTED_CODEC",
                        "The voice server selected an unsupported audio codec.",
                    ));
                }
            }
            15 => {
                if s.stream() && self.connections.contains_key(&s.id) {
                    let fps = s
                        .share_selection
                        .as_ref()
                        .or(s.suspended_share.as_ref())
                        .map(|v| v.fps)
                        .unwrap_or(30);
                    self.conn_call(
                        s.id,
                        "setTransportOptions",
                        vec![json!({"remoteSinkWantsMaxFramerate":fps})],
                    )?;
                }
            }
            21 => {
                if d["protocol_version"] != 1 {
                    return Err(MediaError::new(
                        "DAVE_REQUIRED",
                        "An unencrypted protocol transition was refused.",
                    ));
                }
                let transition = d["transition_id"]
                    .as_u64()
                    .filter(|n| *n <= 65535)
                    .ok_or_else(|| {
                        MediaError::new("INVALID_TRANSITION", "Invalid encryption transition.")
                    })? as u16;
                self.conn_callback(
                    s.id,
                    "prepareSecureFramesTransition",
                    vec![self.json(json!(transition))?, self.json(json!(1))?],
                    Tag::Transition(s.id, s.epoch, transition),
                )?;
            }
            22 => {
                let transition = d["transition_id"]
                    .as_u64()
                    .filter(|n| *n <= 65535)
                    .ok_or_else(|| {
                        MediaError::new("INVALID_TRANSITION", "Invalid encryption transition.")
                    })? as u16;
                self.execute_transition(s, transition)?;
            }
            24 => {
                let epoch = if let Some(v) = d["epoch"].as_str() {
                    v.to_owned()
                } else {
                    d["epoch"].to_string()
                };
                self.prepare_epoch(s, &epoch, d["protocol_version"].as_u64().unwrap_or(0))?;
            }
            25 => {
                native::call(
                    self.env,
                    &self.conn(s.id)?,
                    "updateMLSExternalSender",
                    &[native::bytes_value(self.env, frame.bytes)?],
                )?;
            }
            27 => {
                self.conn_callback(
                    s.id,
                    "processMLSProposals",
                    vec![native::bytes_value(self.env, frame.bytes)?],
                    Tag::Proposals(s.id, s.epoch),
                )?;
            }
            29 | 30 => {
                let (id, bytes) = protocol::transition(&frame.bytes)?;
                self.conn_callback(
                    s.id,
                    if frame.op == 29 {
                        "prepareMLSCommitTransition"
                    } else {
                        "processMLSWelcome"
                    },
                    vec![self.json(json!(id))?, native::bytes_value(self.env, bytes)?],
                    Tag::Commit(s.id, s.epoch, id),
                )?;
            }
            _ => {}
        }
        Ok(())
    }
    fn ready(&mut self, s: &mut Session, d: Value) -> Result<()> {
        self.stage(s, "ready-received");
        if self.connections.contains_key(&s.id) {
            return Err(MediaError::new(
                "DUPLICATE_READY",
                "Unexpected duplicate voice transport.",
            ));
        }
        let port = d["port"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= 65535)
            .ok_or_else(|| MediaError::new("INVALID_READY", "Invalid transport description."))?;
        let ssrc = d["ssrc"]
            .as_u64()
            .filter(|ssrc|*ssrc<=u32::MAX as u64)
            .ok_or_else(|| MediaError::new("INVALID_READY", "Invalid transport description."))?;
        let ip = d["ip"]
            .as_str()
            .ok_or_else(|| MediaError::new("INVALID_READY", "Invalid transport description."))?;
        if !d["modes"].is_array() {
            return Err(MediaError::new(
                "INVALID_READY",
                "Invalid transport description.",
            ));
        }
        s.ssrc = ssrc;
        s.udp_deadline = Some(Instant::now() + Duration::from_secs(15));
        s.native_ready = d.clone();
        s.streams=d["streams"].as_array().map(|streams|streams.iter().filter_map(|stream| {
            if matches!(stream["type"].as_str(),Some("audio"|"test")) {return None;}
            let video=stream["ssrc"].as_u64().filter(|ssrc|*ssrc>0&&*ssrc<u32::MAX as u64)?;
            let rtx=stream["rtx_ssrc"].as_u64().filter(|ssrc|*ssrc>0&&*ssrc<=u32::MAX as u64).unwrap_or(video+1);
            Some(json!({"type":"video","rid":stream["rid"].as_str().filter(|rid|rid.len()<=64&&!rid.chars().any(char::is_control)).unwrap_or("100"),"ssrc":video,"rtxSsrc":rtx,"quality":stream["quality"].as_u64().unwrap_or(100),"active":false}))
        }).take(8).collect()).unwrap_or_default();
        // The original client supplies an inactive video/RTX pair for every
        // connection when READY omits it, including receivers and voice/camera.
        // This describes transport capacity; publication still requires its
        // separate explicit setVideoBroadcast/startShare action.
        if s.streams.is_empty() && ssrc<=u32::MAX as u64-2 {
            s.streams.push(json!({"type":"video","rid":"100","ssrc":ssrc+1,"rtxSsrc":ssrc+2,"quality":100,"active":false}));
        }
        self.emit(s.state_event("negotiating", None));
        let mut streams = vec![
            json!({"type":"audio","ssrc":ssrc,"rid":"","maxBitrate":64000,"soundshare":s.stream()}),
        ];
        streams.extend(s.streams.clone());
        let mut options = json!({"ssrc":ssrc,"address":ip,"port":port,"context":if s.stream(){"stream"}else{"default"},"experiments":[],"qosEnabled":true,"streamParameters":streams});
        if s.stream() {
            options["streamUserId"] = json!(s.credentials.stream_owner);
        }
        self.stage(s, "native-connection-before");
        let obj = native::create_connection(
            self.env,
            &self.engine()?,
            &s.credentials.user_id,
            options,
            self.callback(Tag::Connected(s.id))?,
            s.stream() && !s.receive(),
        )?;
        self.connections
            .insert(s.id, ObjectRef::new(self.env, obj)?);
        self.stage(s, "native-connection-created");
        Ok(())
    }
    fn native_connected(&mut self, s: &mut Session, values: &[Value]) -> Result<()> {
        if !transport_success(values) {
            return Err(MediaError::new(
                "UDP_FAILED",
                "The native voice transport could not connect.",
            ));
        }
        self.stage(s, "udp-ready");
        s.udp_deadline = None;
        self.conn_call(s.id, "setSelfMute", vec![json!(true)])?;
        self.conn_call(
            s.id,
            "setSelfDeafen",
            vec![json!(s.credentials.self_deaf || self.mic_test.is_some())],
        )?;
        let mut transport=json!({"selfMute":true,"fec":true,"packetLossRate":0.3,"qos":true,"encodingVoiceBitRate":64000,"callBitRate":600000,"callMinBitRate":16000,"callMaxBitRate":10000000,"attenuation":false,"minimumJitterBufferLevel":0,"postponeDecodeLevel":100});
        let mode=if s.stream() {json!({"inputMode":1,"inputModeOptions":{"vadThreshold":-40,"vadAutoThreshold":3,"vadUseKrisp":false,"vadLeading":5,"vadTrailing":25}})} else {s.settings.mode()};
        for (key,value) in mode.as_object().unwrap() {transport[key]=value.clone();}
        self.conn_call(s.id,"setTransportOptions",vec![transport])?;
        self.stage(s, "transport-configured");
        if !s.stream() {
            self.input_mode(s.id, &s.settings)?;
            s.reset_input_activity();
        }
        for (name, tag, required) in [
            ("setOnSpeakingCallback", Tag::Speaking(s.id), true),
            ("setOnVideoCallback", Tag::Video(s.id), false),
            ("setVideoCodecErrorCallback", Tag::VideoCodecError(s.id), false),
            (
                "setOnConnectionFailedCallback",
                Tag::ConnectionFailed(s.id),
                false,
            ),
            ("setOnMLSFailureCallback", Tag::MlsFailure(s.id), false),
            (
                "setSecureFramesStateUpdateCallback",
                Tag::MlsState(s.id),
                false,
            ),
            ("setOnDesktopSourceEnded", Tag::SourceEnded(s.id), false),
            ("setOnSoundshare", Tag::Soundshare(s.id), false),
            ("setOnSoundshareFailed", Tag::SoundshareFailed(s.id), false),
        ] {
            let conn = self.conn(s.id)?;
            if required || conn.has_named_property(name).unwrap_or(false) {
                let video = matches!(tag, Tag::Video(_) | Tag::VideoCodecError(_));
                if let Err(error) =
                    native::call(self.env, &conn, name, &[self.callback(tag)?.into_unknown()])
                {
                    if video {
                        self.video_error(s.id, "", error);
                    } else {
                        return Err(error);
                    }
                }
            }
        }
        self.conn_optional(s.id, "setPingInterval", vec![json!(5000)])?;
        for id in s.members.keys().cloned().collect::<Vec<_>>() {
            self.merge_audio_user(s,&id)?;
        }
        self.conn_callback(
            s.id,
            "getEncryptionModes",
            vec![],
            Tag::Modes(s.id, s.native_ready["modes"].clone(), values[1].clone()),
        )?;
        Ok(())
    }
    fn select_protocol(&self, s: &Session, info: Value) {
        self.send(s.id,1,json!({"protocol":info["protocol"].as_str().unwrap_or("udp"),"data":{"address":info["address"],"port":info["port"],"mode":s.mode},"codecs":s.codecs}));
    }
    fn describe(&mut self, s: &mut Session, d: Value) -> Result<()> {
        self.stage(s, "session-description");
        let key = d["secret_key"].as_array();
        if d["mode"] != s.mode
            || !key.is_some_and(|a| {
                a.len() == 32 && a.iter().all(|n| n.as_u64().is_some_and(|n| n <= 255))
            })
        {
            return Err(MediaError::new(
                "INVALID_SESSION",
                "The voice server returned an invalid encryption session.",
            ));
        }
        if d["dave_protocol_version"] != 1 {
            return Err(MediaError::new(
                "DAVE_REQUIRED",
                "This client requires DAVE v1 for calls. Unencrypted downgrade is refused.",
            ));
        }
        if d["audio_codec"].as_str().filter(|codec|!codec.is_empty()).is_some_and(|codec|codec!="opus") {
            return Err(MediaError::new(
                "UNSUPPORTED_CODEC",
                "The selected audio codec is unsupported.",
            ));
        }
        self.conn_call(s.id,"setTransportOptions",vec![json!({"encryptionSettings":{"mode":d["mode"],"secretKey":d["secret_key"]},"audioEncoder":{"type":120,"name":"opus","freq":48000,"pacsize":960,"channels":if s.stream(){2}else{1},"rate":64000},"audioDecoders":[{"type":120,"name":"opus","freq":48000,"channels":2,"params":{"stereo":"1"}}]})])?;
        if s.stream() || !s.streams.is_empty() || d.get("video_codec").is_some() {
            let selected=d["video_codec"].as_str().filter(|codec|!codec.is_empty()).unwrap_or("H264");
            let codec = s
                .codecs
                .iter()
                .find(|v| v["type"] == "video" && v["name"] == selected)
                .cloned();
            if codec.is_none() && s.stream() {
                return Err(MediaError::new(
                    "UNSUPPORTED_CODEC",
                    "The selected screen sharing codec is unsupported.",
                ));
            }
            if let Some(codec) = codec {
                let params = if codec["name"] == "H264" {
                    json!({"level-asymmetry-allowed":"1","packetization-mode":"1","profile-level-id":"4d0033"})
                } else {
                    json!({})
                };
                let decoders:Vec<_>=s.codecs.iter()
                    .filter(|codec|codec["type"]=="video"&&codec["decode"]==true)
                    .map(|codec| {
                        let mut params=if codec["name"]=="H264" {
                            json!({"level-asymmetry-allowed":"1","packetization-mode":"1","profile-level-id":"4d0033"})
                        } else {json!({})};
                        // This main-process host has no renderer video factory.
                        // Original getCodecOptions (web.4d4b2f75fdea5470.js,
                        // byte 10826366) sets this to 0 without Electron video.
                        params["hardware-h264"]=json!("0");
                        json!({"name":codec["name"],"type":codec["payload_type"],"rtxType":codec["rtx_payload_type"],"params":params})
                    }).collect();
                let mut options = json!({"videoDecoders":decoders});
                if codec["encode"] == true {
                    options["videoEncoder"] = json!({"name":codec["name"],"type":codec["payload_type"],"rtxType":codec["rtx_payload_type"],"params":params});
                }
                match self.conn_call(s.id, "setTransportOptions", vec![options]) {
                    Ok(_) => s.video_codec = Some(codec),
                    Err(error) if !s.stream() => {
                        s.video_codec = None;
                        self.video_error(s.id, "", error);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        s.session_description = true;
        self.prepare_epoch(s, "1", 1)?;
        s.local_ready = true;
        s.connect_deadline = None;
        self.sync_receive_video(s)?;
        self.update_camera_subscription(s);
        self.update_readiness(s)?;
        Ok(())
    }
    fn prepare_epoch(&mut self, s: &mut Session, epoch: &str, version: u64) -> Result<()> {
        if version != 1 {
            return Err(MediaError::new(
                "DAVE_REQUIRED",
                "An unsupported encryption transition was refused.",
            ));
        }
        self.stage(s, "mls-epoch");
        if epoch == "1" {
            self.conn_call(s.id, "setSelfMute", vec![json!(true)])?;
            self.video_suspend(s.id);
            if s.camera_requested {
                self.stop_camera(s)?;
            }
            let paused = s.share_selection.is_some();
            s.begin_epoch();
            if paused {
                self.clear_share(s)?;
                self.emit(s.state_event("paused", None));
            }
        }
        self.conn_call(
            s.id,
            "prepareSecureFramesEpoch",
            vec![
                json!(epoch),
                json!(version),
                json!(s.credentials.channel_id),
            ],
        )?;
        if epoch == "1" {
            self.stage(s, "mls-key-package-before");
            self.conn_callback(s.id, "getMLSKeyPackage", vec![], Tag::Key(s.id, s.epoch))?;
            self.update_readiness(s)?;
        }
        Ok(())
    }
    fn execute_transition(&mut self, s: &mut Session, id: u16) -> Result<()> {
        if !s.transitions.remove(&id) {
            return Err(MediaError::new(
                "INVALID_TRANSITION",
                "An unprepared encryption transition was refused.",
            ));
        }
        self.conn_call(s.id, "executeSecureFramesTransition", vec![json!(id)])?;
        s.dave_ready = true;
        self.check_connected(s)
    }
    fn update_readiness(&mut self, s: &mut Session) -> Result<()> {
        if s.closed || !s.local_ready || s.connected {
            return Ok(());
        }
        self.conn_call(s.id, "setSelfMute", vec![json!(true)])?;
        if !s.stream() && !s.local_vad {
            self.eng("setInputDevice", vec![json!(s.settings.input)])?;
            s.local_vad = true;
            self.eng_callback("setOnVoiceCallback", vec![], Tag::MicLevel)?;
            native::optional_json(
                self.env,
                &self.engine()?,
                "setEmitVADLevel2",
                vec![json!(true)],
            )?;
        }
        s.update_wait(Instant::now());
        self.emit(s.state_event(s.state(), None));
        Ok(())
    }
    fn check_connected(&mut self, s: &mut Session) -> Result<()> {
        if !s.establish() {
            return Ok(());
        }
        if !s.stream() {
            self.input_mode(s.id,&s.settings)?;
            s.reset_input_activity();
            self.eng("setInputDevice", vec![json!(s.settings.input)])?;
            native::optional_json(
                self.env,
                &self.engine()?,
                "setEmitVADLevel2",
                vec![json!(true)],
            )?;
            self.eng_callback("setOnVoiceCallback", vec![], Tag::MicLevel)?;
            if let Some(v) = s.speaking_event(&s.credentials.user_id.clone(), false) {
                self.emit(v);
            }
        }
        self.conn_call(
            s.id,
            "setSelfMute",
            vec![json!(s.send_mute() || self.mic_test.is_some())],
        )?;
        self.stage(s, "connected");
        self.emit(s.state_event("connected", None));
        self.sync_receive_video(s)?;
        self.update_camera_subscription(s);
        self.video_resume(s.id);
        if let Some(selection) = s.suspended_share.take() {
            self.share_capture(s, selection)?;
            self.emit(s.state_event("capture-requested", None));
        }
        Ok(())
    }
    // GoLive is an SFU subscription. Registering as a viewer and merging a
    // decoder do not request packets: the receiver must announce its chosen
    // active SSRC through voice opcode 15, independently of the main call.
    fn sync_receive_video(&mut self, s:&mut Session)->Result<()> {
        if !s.receive() || !s.local_ready || !s.session_description {return Ok(());}
        let selected=if self.video_receive_enabled() {
            s.members.get(&s.credentials.stream_owner)
                .and_then(|user|user["videoSsrcs"].as_array())
                .and_then(|streams|streams.iter().filter_map(Value::as_u64).find(|ssrc|*ssrc>0&&*ssrc<=u32::MAX as u64))
        } else {None};
        if selected==s.receive_requested_ssrc {return Ok(());}
        let mut wants=json!({});
        if let Some(previous)=s.receive_requested_ssrc {wants[previous.to_string()]=json!(0);}
        if let Some(ssrc)=selected {
            wants[ssrc.to_string()]=json!(100);
            wants["pixelCounts"]=json!({});
            wants["pixelCounts"][ssrc.to_string()]=json!(640*360);
        }
        if s.members.contains_key(&s.credentials.stream_owner) {
            self.conn_call(s.id,"setDisableLocalVideo",vec![json!(s.credentials.stream_owner),json!(selected.is_none())])?;
        }
        self.send(s.id,15,wants);
        s.receive_requested_ssrc=selected;
        if selected.is_some() {self.stage(s,"video-subscription-requested");}
        Ok(())
    }
    fn sync_camera_video(&mut self,s:&mut Session)->Result<()> {
        if s.stream()||!s.local_ready||!s.session_description {return Ok(());}
        let visible=self.video_receive_enabled();
        let mut selected=std::collections::HashSet::new();
        for (user,member) in &s.members {
            let requested=visible&&s.camera_receive_users.contains(user);
            let streams=member["videoSsrcs"].as_array();
            if requested && let Some(ssrc)=streams.and_then(|streams|streams.iter().filter_map(Value::as_u64).find(|ssrc|*ssrc>0&&*ssrc<=u32::MAX as u64)) {selected.insert(ssrc);}
        }
        // Opcode 15 applies to audio as well as video. The native original
        // starts with {any:100} (web.4d4b2f75fdea5470.js, byte 2407471),
        // then overrides disabled sources. A video-only {any:0} silences
        // unknown audio SSRCs too, including while no camera is selected.
        let mut wants=json!({"any":100,"pixelCounts":{}});
        for ssrc in &s.camera_requested_ssrcs {wants[ssrc.to_string()]=json!(0);}
        for member in s.members.values() {
            if let Some(streams)=member["videoSsrcs"].as_array() {
                for ssrc in streams.iter().filter_map(Value::as_u64).filter(|ssrc|*ssrc>0&&*ssrc<=u32::MAX as u64) {
                    wants[ssrc.to_string()]=json!(0);
                }
            }
            if member["mute"]==true&&let Some(ssrc)=member["ssrc"].as_u64().filter(|ssrc|*ssrc>0&&*ssrc<=u32::MAX as u64) {
                wants[ssrc.to_string()]=json!(0);
            }
        }
        for ssrc in &selected {wants[ssrc.to_string()]=json!(100);wants["pixelCounts"][ssrc.to_string()]=json!(640*360);}
        if s.camera_subscription_sent&&wants==s.camera_sink_wants {return Ok(());}
        for (user,member) in &s.members {
            if member["videoSsrcs"].as_array().is_some_and(|streams|!streams.is_empty()) {
                self.conn_call(s.id,"setDisableLocalVideo",vec![json!(user),json!(!visible||!s.camera_receive_users.contains(user))])?;
            }
        }
        self.send(s.id,15,wants.clone());
        s.camera_sink_wants=wants;
        s.camera_requested_ssrcs=selected;s.camera_subscription_sent=true;
        Ok(())
    }
    fn update_camera_subscription(&mut self,s:&mut Session) {
        if let Err(error)=self.sync_camera_video(s) {self.video_error(s.id,"",error);}
    }
    fn add_user(&mut self, s: &mut Session, id: &str, ssrc: u64) -> Result<()> {
        let previous = s.members.get(id).and_then(|user| user["ssrc"].as_u64());
        let fresh = s.add_user(id, ssrc);
        if s.members.contains_key(id) {
            if self.connections.contains_key(&s.id) && (fresh || ssrc > 0 && previous != Some(ssrc))
            {
                self.merge_audio_user(s,id)?;
            }
            if fresh && !s.stream() {
                self.emit(json!({"event":"participant","userId":id,"present":true}));
            }
        }
        self.update_camera_subscription(s);
        self.update_readiness(s)
    }
    fn merge_audio_user(&mut self,s:&mut Session,id:&str)->Result<()> {
        let Some(user)=s.members.get(id).cloned() else {return Ok(());};
        if let Err(error)=self.conn_call(s.id,"mergeUsers",vec![json!([user])]) {
            let has_video=user["videoSsrc"].as_u64().is_some_and(|ssrc|ssrc>0)
                ||user["videoSsrcs"].as_array().is_some_and(|streams|!streams.is_empty());
            if !has_video {return Err(error);}
            // A rejected video decoder description must not poison the next
            // audio SSRC update. Retry just the audio member, retaining gain.
            let mut audio=user;audio["videoSsrc"]=json!(0);audio["videoSsrcs"]=json!([]);audio["rtxSsrc"]=json!(0);
            self.conn_call(s.id,"mergeUsers",vec![json!([audio])])?;
            s.members.insert(id.into(),audio);
            self.video_error(s.id,id,error);
        }
        Ok(())
    }
    fn beat(&self, s: &mut Session) -> Result<()> {
        let now = Instant::now();
        let Some(interval) = s.heartbeat_interval else {
            return Ok(());
        };
        let socket = self.sockets.get(&s.id).ok_or_else(|| MediaError::new("VOICE_SEND_CLOSED", "La conexión de voz ya se cerró."))?;
        // The first pending timestamp is not renewed by retransmission. Its
        // watchdog also lives in the receiver task, independent of JS ticks.
        socket.heartbeat.sent(now, interval)?;
        s.heartbeat_pending = socket.heartbeat.first_pending();
        s.next_heartbeat = Some(now + interval);
        self.send(
            s.id,
            3,
            json!({"t":chrono::Utc::now().timestamp_millis(),"seq_ack":s.seq}),
        );
        Ok(())
    }
    fn fail(&mut self, s: &mut Session, error: MediaError) {
        self.emit(s.state_event("error", Some(error)));
        self.close(s, false);
    }
    fn close(&mut self, s: &mut Session, notify: bool) {
        if s.closed {
            return;
        }
        if s.receive() && let Some(ssrc)=s.receive_requested_ssrc.take() {
            let mut wants=json!({});wants[ssrc.to_string()]=json!(0);self.send(s.id,15,wants);
        }
        if !s.stream()&&s.camera_subscription_sent {self.send(s.id,15,json!({"any":0}));}
        let _ = self.stop_camera(s);
        self.video_remove_session(s.id);
        if s.stream() && !s.receive() {
            s.cancel_share();
            let _ = self.clear_share(s);
        }
        if s.local_vad && !s.stream() {
            let _ = self.eng("setInputDevice", vec![json!("disabled")]);
            if let Ok(engine) = self.engine() {
                let _ = native::optional_json(
                    self.env,
                    &engine,
                    "setEmitVADLevel2",
                    vec![json!(false)],
                );
            }
        }
        if !s.stream() {
            let _ = self.eng("setInputDevice", vec![json!("disabled")]);
        }
        s.closed = true;
        s.heartbeat_pending = None;
        s.next_heartbeat = None;
        s.heartbeat_interval = None;
        s.local_ready = false;
        s.connected = false;
        s.credentials.voice_token.clear();
        s.credentials.session_id.clear();
        if let Some(socket) = self.sockets.remove(&s.id) {
            // Cancellation and AbortHandle bypass both data and Pong backlog;
            // dropping the split socket releases the transport immediately.
            socket.cancel();
        }
        if let Some(conn) = self.connections.remove(&s.id) {
            if let Ok(obj) = conn.object(self.env) {
                let _ = native::call_json(self.env, &obj, "destroy", vec![]);
            }
            conn.release(self.env);
        }
        if self.voice == Some(s.id) {
            self.voice = None
        }
        if self.share == Some(s.id) {
            self.share = None
        }
        self.watches.retain(|_, session| *session != s.id);
        self.previous_stats.remove(&s.id);
        if notify {
            self.emit(s.state_event("disconnected", None));
        }
    }
}

impl Host {
    fn native_event(&mut self, tag: Tag, values: Vec<Value>) -> Result<()> {
        let value = values.first().cloned().unwrap_or(Value::Null);
        match tag.clone() {
            Tag::Inventory { request, part } | Tag::Configure { request, part } => {
                return self.inventory_result(
                    request,
                    &part,
                    value,
                    matches!(tag, Tag::Configure { .. }),
                );
            }
            Tag::Stats { request, session } => {
                let report = if let Some(s) = self.sessions.get(&session) {
                    let parsed = if let Some(raw) = value.as_str() {
                        if raw.len() > protocol::MAX_FRAME {
                            None
                        } else {
                            serde_json::from_str(raw).ok()
                        }
                    } else {
                        Some(value)
                    };
                    if let Some(raw) = parsed.filter(Value::is_object) {
                        let mut r = stats::sanitize(&raw, s.state(), s.dave_ready);
                        let now = Instant::now();
                        if let Some((at, previous)) = self.previous_stats.get(&session) {
                            stats::add_interval(
                                &mut r,
                                previous,
                                now.duration_since(*at).as_secs_f64(),
                            );
                        }
                        self.previous_stats
                            .insert(session, (now, stats::counters(&r)));
                        r
                    } else {
                        stats::unavailable(s.state(), "INVALID_NATIVE_STATS")
                    }
                } else {
                    stats::unavailable("disconnected", "CONNECTION_CHANGED")
                };
                self.stats_result(request, session, report);
                return Ok(());
            }
            Tag::MicLevel => {
                if self
                    .mic_last
                    .is_some_and(|at| at.elapsed() < Duration::from_millis(75))
                {
                    return Ok(());
                }
                self.mic_last = Some(Instant::now());
                let (level, db) = input_level(input_sample(&values).unwrap_or(-100.));
                let detected = manual_activity(&self.settings,level,db);
                let mut activity=detected;
                let mut transmitting=false;
                if let Some(id) = self.voice {
                    if let Some(mut session) = self.sessions.remove(&id) {
                        let eligible=self.mic_test.is_none()&&session.local_ready&&!session.closed&&!session.credentials.self_mute&&!session.credentials.self_deaf&&self.settings.input_volume>0.;
                        let wanted=session.input_activity(Instant::now(),detected,level>0.,eligible);
                        let active=match self.transmit_gate(&mut session,wanted) {
                            Ok(active)=>if session.connected {active} else {wanted&&!session.credentials.self_mute&&!session.credentials.self_deaf&&self.mic_test.is_none()},
                            Err(error)=>{self.fail(&mut session,error);false},
                        };
                        transmitting=session.transmit_gate&&session.connected&&!session.send_mute()&&self.mic_test.is_none();
                        activity=if self.mic_test.is_some() {detected} else {active};
                        if let Some(event) =
                            session.speaking_event(&session.credentials.user_id.clone(), active)
                        {
                            if session.connected {
                                self.send(id,5,json!({"speaking":if active{1}else{0},"delay":0,"ssrc":session.ssrc}));
                            }
                            self.emit(event);
                        }
                        if !session.closed {self.sessions.insert(id, session);}
                    }
                }
                if self.mic_test.is_some() || self.voice.is_some() {
                    self.emit(json!({"event":if self.mic_test.is_some(){"micTest"}else{"inputLevel"},"active":self.mic_test.is_some(),"pending":false,"monitoring":self.voice.is_some(),"level":level,"db":db,"speaking":activity,"detected":detected,"transmitting":transmitting,"thresholdDb":self.settings.sensitivity_db,"manual":!self.settings.ptt,"token":self.mic_token,"localOnly":true,"temporaryDeaf":self.mic_test.is_some()&&self.voice.is_some()}));
                }
                return Ok(());
            }
            _ => {}
        }
        let Some(id) = tag.session() else {
            return Ok(());
        };
        let Some(mut s) = self.sessions.remove(&id) else {
            return Ok(());
        };
        let result = (|| {
            match tag {
                Tag::Connected(_) => self.native_connected(&mut s, &values)?,
                Tag::Modes(_, server, info) => {
                    self.stage(&s, "encryption-modes-ready");
                    s.mode = protocol::choose_encryption(&server, &value)?;
                    s.codecs = vec![
                        json!({"type":"audio","name":"opus","priority":1000,"payload_type":120}),
                    ];
                    if self
                        .engine()?
                        .has_named_property("getCodecCapabilities")
                        .unwrap_or(false)
                    {
                        self.eng_callback("getCodecCapabilities", vec![], Tag::Codecs(id, info))?;
                    } else {
                        self.select_protocol(&s, info);
                    }
                }
                Tag::Codecs(_, info) => {
                    let available = if let Some(v) = value.as_str() {
                        serde_json::from_str(v).unwrap_or(Value::Null)
                    } else {
                        value
                    };
                    let codecs = available
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter(|v| {
                                    (v["decode"].as_bool().unwrap_or(false)
                                        || !s.receive() && v["encode"].as_bool().unwrap_or(false))
                                        && matches!(
                                            v.get("codec")
                                                .or_else(|| v.get("name"))
                                                .and_then(Value::as_str),
                                            Some("H264" | "VP8" | "VP9")
                                        )
                                })
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if codecs.is_empty() {
                        if s.stream() {
                            return Err(MediaError::new(
                                if s.receive(){"NO_VIDEO_DECODER"}else{"NO_VIDEO_ENCODER"},
                                if s.receive(){"No supported video decoder was found."}else{"No supported video encoder was found."},
                            ));
                        }
                        s.streams.clear();
                    } else {
                        for (i, c) in codecs.iter().enumerate() {
                            s.codecs.push(json!({"type":"video","name":c.get("codec").or_else(||c.get("name")),"priority":(i+2)*1000,"payload_type":101+i*2,"rtx_payload_type":102+i*2,"encode":!s.receive()&&c["encode"].as_bool().unwrap_or(false),"decode":c["decode"].as_bool().unwrap_or(false)}));
                        }
                    }
                    self.select_protocol(&s, info);
                }
                Tag::Key(_, generation) => {
                    if generation == s.epoch {
                        self.stage(&s, "mls-key-package-created");
                        self.send_binary(id, 26, &bytes(&value)?)?;
                    }
                }
                Tag::Proposals(_, generation) => {
                    if generation == s.epoch {
                        let out = bytes(&value)?;
                        if !out.is_empty() {
                            self.send_binary(id, 28, &out)?;
                        }
                    }
                }
                Tag::Commit(_, generation, transition) => {
                    if generation != s.epoch {
                        return Ok(());
                    }
                    let ok = value.as_bool().unwrap_or(false);
                    let version = values.get(1).and_then(Value::as_u64).unwrap_or(0);
                    if !ok {
                        s.failed_mls += 1;
                        if s.failed_mls >= 5 {
                            return Err(MediaError::new(
                                "MLS_VALIDATION_FAILED",
                                "Repeated encrypted group validation failure.",
                            ));
                        }
                        self.send(id, 31, json!({"transition_id":transition}));
                        self.prepare_epoch(&mut s, "1", 1)?;
                    } else {
                        if version != 1 {
                            return Err(MediaError::new(
                                "DAVE_VERSION_UNSUPPORTED",
                                "The encrypted group selected an unsupported protocol.",
                            ));
                        }
                        s.failed_mls = 0;
                        if transition == 0 {
                            s.dave_ready = true;
                            self.check_connected(&mut s)?;
                        } else {
                            s.transitions.insert(transition);
                            self.send(id, 23, json!({"transition_id":transition}));
                        }
                    }
                }
                Tag::Transition(_, generation, transition) => {
                    if generation != s.epoch {
                        return Ok(());
                    }
                    s.transitions.insert(transition);
                    if transition == 0 {
                        self.execute_transition(&mut s, transition)?;
                    } else {
                        self.send(id, 23, json!({"transition_id":transition}));
                    }
                }
                Tag::Speaking(_) => {
                    if s.connected {
                        let user = value.as_str().unwrap_or("");
                        if user != s.credentials.user_id {
                            if let Some(event) =
                                s.speaking_event(user, values.get(1).is_some_and(truthy))
                            {
                                self.emit(event);
                            }
                        }
                    }
                }
                Tag::Video(_) => {
                    if let Err(error) = self.video(&mut s, &values) {
                        self.video_error(
                            s.id,
                            values.first().and_then(Value::as_str).unwrap_or(""),
                            error,
                        );
                    }
                }
                Tag::VideoCodecError(_) => {
                    let owner=if s.receive() {s.credentials.stream_owner.as_str()}else{""};
                    self.video_error(s.id,owner,MediaError::new("VIDEO_CODEC_FAILED","El motor no pudo descodificar el vídeo. El audio continúa; vuelve a abrir la pantalla para reintentar."));
                }
                Tag::ConnectionFailed(_) => {
                    return Err(MediaError::new("UDP_FAILED", "The media transport failed."));
                }
                Tag::MlsFailure(_) => {
                    return Err(MediaError::new(
                        "MLS_FAILED",
                        "The media engine reported an encryption failure.",
                    ));
                }
                Tag::MlsState(_) => self.stage(&s, "mls-state-updated"),
                Tag::SourceEnded(_) => {
                    if s.stream() && s.capture_active && !s.clearing_source {
                        s.suspended_share = None;
                        return Err(MediaError::new(
                            "SOURCE_ENDED",
                            "The selected screen or window is no longer available.",
                        ));
                    }
                }
                Tag::Soundshare(_) => {
                    if s.stream() && s.soundshare_requested && truthy(&value) {
                        s.soundshare_attached = true;
                        self.emit(s.state_event("audio-attached", None));
                    }
                }
                Tag::SoundshareFailed(_) => {
                    if s.stream() && s.soundshare_requested {
                        return Err(MediaError::new(
                            "SCREEN_AUDIO_FAILED",
                            "The requested screen audio source could not be attached.",
                        ));
                    }
                }
                _ => {}
            }
            Ok(())
        })();
        if let Err(e) = result {
            self.fail(&mut s, e)
        }
        if !s.closed {
            self.sessions.insert(id, s);
        }
        Ok(())
    }
    fn inventory_result(
        &mut self,
        id: Value,
        part: &str,
        value: Value,
        configure: bool,
    ) -> Result<()> {
        if !self.pending.contains_key(&id.to_string()) {
            return Ok(());
        }
        let values = value.as_array().cloned().unwrap_or_default();
        let compact:Vec<Value>=values.iter().enumerate().map(|(i,v)|match part{
  "input"|"output"=>json!({"type":if part=="input"{"audioinput"}else{"audiooutput"},"id":v.get("guid").or_else(||v.get("id")).map(|v|v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string())).unwrap_or_else(||i.to_string()),"name":v["name"].as_str().unwrap_or("Device"),"index":v["index"].as_i64().unwrap_or(i as i64)}),
  "cameras"=>json!({"id":v["guid"].as_str().filter(|s|!s.is_empty()).or_else(||v["name"].as_str()).unwrap_or(""),"name":v["name"].as_str().map(str::to_owned).unwrap_or_else(||format!("Camera {}",i+1))}),
  _=>json!({"id":v["id"].as_str().map(str::to_owned).or_else(||v["id"].as_i64().map(|n|n.to_string())).unwrap_or_default(),"name":v["name"].as_str().unwrap_or(if part=="screen"{"Screen"}else{"Window"}),"type":part}),
 }).filter(|v|matches!(part,"input"|"output")||v["id"].as_str().is_some_and(|s|!s.is_empty()&&s!="disabled")).collect();
        if matches!(part, "input" | "output") {
            // startMicTest validates explicit devices against the same native
            // catalog shown in the picker. Keep both listDevices and
            // configureAudio results; returning the list alone left every
            // non-default selection permanently unlisted.
            self.inventories.insert(
                part.into(),
                (
                    Instant::now(),
                    compact.iter().filter_map(|device| {
                        device["id"].as_str().filter(|id| !id.is_empty() && *id != "disabled").map(str::to_owned)
                    }).collect(),
                ),
            );
        }
        if part == "cameras" {
            self.inventories.insert(
                "cameras".into(),
                (
                    Instant::now(),
                    compact
                        .iter()
                        .filter_map(|v| v["id"].as_str().map(str::to_owned))
                        .collect(),
                ),
            );
            self.reply(id, Ok(json!(compact)));
            return Ok(());
        }
        let key = id.to_string();
        let entry = self.inventory_parts.entry(key.clone()).or_insert(json!({}));
        entry[part] = json!(compact);
        let result = entry.clone();
        let ready = if matches!(part, "input" | "output") {
            result.get("input").is_some() && result.get("output").is_some()
        } else {
            result.get("screen").is_some() && result.get("window").is_some()
        };
        if !ready {
            return Ok(());
        }
        self.inventory_parts.remove(&key);
        if matches!(part, "screen" | "window") {
            let mut sources = result["screen"].as_array().cloned().unwrap_or_default();
            sources.extend(result["window"].as_array().cloned().unwrap_or_default());
            self.inventories.insert(
                "sources".into(),
                (
                    Instant::now(),
                    sources
                        .iter()
                        .filter_map(|v| v["id"].as_str().map(str::to_owned))
                        .collect(),
                ),
            );
            self.reply(id, Ok(json!(sources)));
        } else if configure {
            let mut settings = self
                .configure_settings
                .remove(&key)
                .ok_or_else(MediaError::internal)?;
            // An audio-device form can complete after an individual gain ACK.
            // During a live call only setVolume owns participant gains.
            if self.voice.is_some()||settings.user_volumes_revision<self.settings.user_volumes_revision {
                settings.user_volumes=self.settings.user_volumes.clone();
                settings.user_volumes_revision=self.settings.user_volumes_revision;
            }
            let mut fallbacks = vec![];
            for (name, device, list) in [
                ("inputId", &mut settings.input, "input"),
                ("outputId", &mut settings.output, "output"),
            ] {
                if device != "default"
                    && !result[list]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v["id"] == *device))
                {
                    *device = "default".into();
                    fallbacks.push(name);
                }
            }
            let capture = self
                .voice
                .and_then(|id| self.sessions.get(&id))
                .is_some_and(|s| s.local_ready && !s.closed)
                || self.mic_test.is_some();
            self.apply_audio(&settings, capture)?;
            if let Some(id) = self.voice {
                if self.connections.contains_key(&id) {
                    self.input_mode(id, &settings)?;
                }
                if let Some(s) = self.sessions.get_mut(&id) {
                    s.settings = settings.clone();
                    s.reset_input_activity();s.ptt_active=false;
                }
            }
            self.settings = settings;
            self.reply(id, Ok(json!({"applied":true,"fallbacks":fallbacks})));
        } else {
            self.reply(id, Ok(result));
        }
        Ok(())
    }
    fn voice_command(&mut self, request: Value, name: &str, p: &Value) -> Result<Option<Value>> {
        if name == "setVolume" && p["target"] != "user" {
            self.initialize()?;
            let value = protocol::volume(&p["value"])?;
            let target = p["target"].as_str().unwrap_or("");
            if target=="input"&&value>2. {return Err(MediaError::new("INVALID_VOLUME","Input volume must not exceed 200%."));}
            self.eng(
                match target {
                    "input" => "setInputVolume",
                    "output" => "setOutputVolume",
                    _ => {
                        return Err(MediaError::new(
                            "INVALID_VOLUME_TARGET",
                            "Volume target must be input, output or user.",
                        ));
                    }
                },
                vec![json!(value)],
            )?;
            match target {"input"=>self.settings.input_volume=value*100.,"output"=>self.settings.output_volume=value*100.,_=>{}}
            if let Some(id)=self.voice&&let Some(mut session)=self.sessions.remove(&id) {
                session.settings=self.settings.clone();
                if target=="input"&&value==0. {
                    if let Err(error)=self.transmit_gate(&mut session,false) {self.fail(&mut session,error.clone());return Err(error);}
                    session.reset_input_activity();
                    if let Some(event)=session.speaking_event(&session.credentials.user_id.clone(),false) {
                        if session.connected {self.send(id,5,json!({"speaking":0,"delay":0,"ssrc":session.ssrc}));}
                        self.emit(event);
                    }
                }
                if !session.closed {self.sessions.insert(id,session);}
            }
            return Ok(Some(json!({"target":target,"value":p["value"]})));
        }
        let id = self.voice_id()?;
        let mut s = self.sessions.remove(&id).ok_or_else(MediaError::internal)?;
        let mut camera_started = false;
        let result = (|| match name {
            "mute" => {
                if !truthy(&p["value"]) {
                    self.conn_call(id,"setPTTActive",vec![json!(false),json!(false),json!(false)])?;
                    s.reset_input_activity();
                }
                s.credentials.self_mute = truthy(&p["value"]);
                self.conn_call(
                    id,
                    "setSelfMute",
                    vec![json!(s.send_mute() || self.mic_test.is_some())],
                )?;
                if s.credentials.self_mute && !s.connected {
                    if let Some(v) = s.speaking_event(&s.credentials.user_id.clone(), false) {
                        self.emit(v);
                    }
                }
                if s.credentials.self_mute {self.transmit_gate(&mut s,false)?;s.reset_input_activity();}
                Ok(Some(json!({"muted":s.credentials.self_mute})))
            }
            "deafen" => {
                s.credentials.self_deaf = truthy(&p["value"]);
                self.conn_call(
                    id,
                    "setSelfDeafen",
                    vec![json!(s.credentials.self_deaf || self.mic_test.is_some())],
                )?;
                if s.credentials.self_deaf {self.transmit_gate(&mut s,false)?;s.reset_input_activity();}
                self.conn_call(
                    id,
                    "setSelfMute",
                    vec![json!(s.send_mute() || self.mic_test.is_some())],
                )?;
                if s.credentials.self_deaf && !s.connected {
                    if let Some(v) = s.speaking_event(&s.credentials.user_id.clone(), false) {
                        self.emit(v);
                    }
                }
                Ok(Some(json!({"deafened":s.credentials.self_deaf})))
            }
            "ptt" => {
                let active = p["active"].as_bool().ok_or_else(|| {
                    MediaError::new("INVALID_PTT", "PTT active must be a boolean.")
                })?;
                if !self.settings.ptt {
                    return Err(MediaError::new(
                        "PTT_DISABLED",
                        "Enable push to talk first.",
                    ));
                }
                s.ptt_active = active;
                if s.local_ready {self.transmit_gate(&mut s,active)?;}
                if !active && !s.connected {
                    if let Some(v) = s.speaking_event(&s.credentials.user_id.clone(), false) {
                        self.emit(v);
                    }
                }
                Ok(Some(json!({"active":active})))
            }
            "setVolume" => {
                let user = protocol::snowflake(&p["userId"], "userId")?;
                if user==s.credentials.user_id || !s.members.contains_key(&user) {
                    return Err(MediaError::new("INVALID_VOLUME_TARGET","Select another participant in this call."));
                }
                let scale = protocol::user_volume(&p["value"])?;
                let revision=p["volumeRevision"].as_u64().unwrap_or(0);
                if revision>0&&s.volume_requests.get(&user).is_some_and(|current|*current>revision) {
                    return Ok(Some(json!({"target":"user","userId":user,"value":s.volumes.get(&user).copied().unwrap_or(1.)*100.,"stale":true})));
                }
                self.conn_call(id, "setLocalVolume", vec![json!(user), json!(scale)])?;
                s.volumes.insert(user.clone(), scale);
                if revision>0 {s.volume_requests.insert(user.clone(),revision);}
                self.settings.user_volumes.insert(user.clone(),scale);
                self.settings.user_volumes_revision=self.settings.user_volumes_revision.max(revision);
                if let Some(u) = s.members.get_mut(&user) {
                    u["volume"] = json!(scale);
                }
                Ok(Some(json!({"target":"user","userId":user,"value":p["value"]})))
            }
            "startCamera" => {
                let token = operation_token(s.camera_token, p)?;
                if token <= s.camera_token {
                    return Err(MediaError::new(
                        "CAMERA_CANCELLED",
                        "This camera selection was already cancelled.",
                    ));
                }
                let device = protocol::string(&p["deviceId"], "deviceId", 1024)?;
                if !self.inventories.get("cameras").is_some_and(|(at, ids)| {
                    at.elapsed() < Duration::from_secs(600) && ids.contains(&device)
                }) {
                    return Err(MediaError::new(
                        "CAMERA_NOT_LISTED",
                        "Select an available camera again.",
                    ));
                }
                if !s.connected
                    || !s
                        .video_codec
                        .as_ref()
                        .is_some_and(|codec| codec["encode"] == true)
                    || s.streams.is_empty()
                {
                    return Err(MediaError::new(
                        "CAMERA_UNAVAILABLE",
                        "This encrypted voice session did not negotiate camera video. Rejoin the call.",
                    ));
                }
                if s.camera_requested {
                    return Err(MediaError::new(
                        "CAMERA_BUSY",
                        "The camera is already active or preparing.",
                    ));
                }
                s.camera_requested = true;
                camera_started = true;
                s.camera_token = token;
                s.camera_request = Some(request);
                s.camera_deadline = Some(Instant::now() + Duration::from_secs(10));
                self.emit(json!({"event":"cameraState","token":s.camera_token,"mediaSession":s.id,"state":"preparing"}));
                let streams: Vec<Value> = s
                    .streams
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let mut v = v.clone();
                        v["active"] = json!(i == 0);
                        v["maxResolution"] = json!({"type":"fixed","width":1280,"height":720});
                        v["maxFrameRate"] = json!(30);
                        v["maxBitrate"] = json!(2500000);
                        v
                    })
                    .collect();
                self.conn_call(id,"setTransportOptions",vec![json!({"streamParameters":streams,"encodingVideoWidth":1280,"encodingVideoHeight":720,"encodingVideoFrameRate":30,"captureVideoFrameRate":30,"encodingVideoBitRate":600000,"encodingVideoMinBitRate":200000,"encodingVideoMaxBitRate":2500000})])?;
                let _ = self.video_source(
                    s.id,
                    &s.credentials.user_id,
                    &device,
                    "camera",
                    true,
                    "",
                    true,
                );
                self.video.preview = None;
                self.eng("setVideoInputDevice", vec![json!(device)])?;
                self.conn_call(id, "setVideoBroadcast", vec![json!(true)])?;
                Ok(None)
            }
            "stopCamera" => {
                let token = operation_token(s.camera_token, p)?;
                if token < s.camera_token {
                    return Ok(Some(json!({"state":"unchanged","stale":true})));
                }
                s.camera_token = token;
                self.stop_camera(&mut s)?;
                Ok(Some(json!({"state":"stopped"})))
            }
            _ => Err(MediaError::internal()),
        })();
        if result.is_err() && name == "startCamera" && camera_started {
            let _ = self.stop_camera(&mut s);
            self.emit(json!({"event":"cameraState","token":s.camera_token,"mediaSession":s.id,"state":"error","reason":result.as_ref().err().map(|e|e.json())}));
        }
        self.sessions.insert(id, s);
        result
    }
    fn stop_camera(&mut self, s: &mut Session) -> Result<()> {
        if s.stream() {
            return Ok(());
        }
        let requested = s.camera_requested;
        s.camera_requested = false;
        s.camera_active = false;
        s.camera_deadline = None;
        if let Some(request) = s.camera_request.take() {
            self.reply(
                request,
                Err(MediaError::new("CAMERA_STOPPED", "Camera stopped.")),
            );
        }
        if requested {
            self.video_remove_local(s.id, "camera");
            let broadcast = self.conn_call(s.id, "setVideoBroadcast", vec![json!(false)]);
            let input = self.eng("setVideoInputDevice", vec![json!("disabled")]);
            self.send(s.id,12,json!({"audio_ssrc":s.ssrc,"video_ssrc":0,"rtx_ssrc":0,"streams":s.streams.iter().map(|v|json!({"type":v["type"],"rid":v["rid"],"ssrc":v["ssrc"],"rtx_ssrc":v["rtxSsrc"],"quality":v["quality"],"active":false})).collect::<Vec<_>>()}));
            self.emit(json!({"event":"cameraState","token":s.camera_token,"mediaSession":s.id,"state":"stopped"}));
            if let Err(error) = broadcast.and(input) {
                self.video_error(s.id, &s.credentials.user_id, error);
            }
        }
        Ok(())
    }
    fn video(&mut self, s: &mut Session, values: &[Value]) -> Result<()> {
        protocol::snowflake(values.first().unwrap_or(&Value::Null),"videoUserId")?;
        let id = values.first().and_then(Value::as_str).unwrap_or("");
        if s.receive() && id!=s.credentials.stream_owner {return Ok(());}
        let ssrc = values.get(1).and_then(Value::as_u64).unwrap_or(0);
        if id != s.credentials.user_id {
            let native_stream = values.get(2).and_then(Value::as_str).unwrap_or("");
            if ssrc > 0 && !native_stream.is_empty() {
                if let Err(error) = self.video_source(
                    s.id,
                    id,
                    native_stream,
                    if s.receive() { "screen" } else { "camera" },
                    false,
                    &s.credentials.stream_key,
                    s.connected,
                ) {
                    self.video_error(s.id, id, error.clone());
                    return Err(error);
                }
            } else {
                self.video_remove_user(s.id, id);
            }
            if !s.stream() {
                self.emit(json!({"event":"participant","userId":id,"present":true,"video":ssrc>0}));
            }
            return Ok(());
        }
        let active = ssrc > 0 && (s.stream() || s.camera_requested);
        if s.stream() && !s.receive() {
            if let Some(native_stream) = values
                .get(2)
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            {
                if active {
                    let _ = self.video_source(
                        s.id,
                        &s.credentials.user_id,
                        native_stream,
                        "screen",
                        true,
                        &s.credentials.stream_key,
                        s.connected,
                    );
                    self.video_local_source_id(
                        native_stream,
                        s.share_selection
                            .as_ref()
                            .or(s.suspended_share.as_ref())
                            .map(|v| v.source.as_str())
                            .unwrap_or(""),
                    );
                    self.stop_screen_preview();
                } else {
                    self.video_remove_local(s.id, "screen");
                }
            }
        }
        if !s.stream() && s.camera_requested && s.camera_active && !active {
            self.stop_camera(s)?;
            self.emit(json!({"event":"cameraState","token":s.camera_token,"mediaSession":s.id,"state":"error","reason":{"code":"CAMERA_SOURCE_ENDED","message":"The camera stopped delivering video. Select it again to reconnect."}}));
            return Ok(());
        }
        let streams = values.get(3).and_then(Value::as_array);
        for (i, p) in s.streams.iter_mut().enumerate() {
            let actual = streams.and_then(|a| a.iter().find(|v| v["rid"] == p["rid"]));
            if let Some(v) = actual {
                if v.get("ssrc").is_some() {
                    p["ssrc"] = v["ssrc"].clone();
                }
                if v.get("rtxSsrc").is_some() {
                    p["rtxSsrc"] = v["rtxSsrc"].clone();
                }
            } else if i == 0 && active {
                p["ssrc"] = json!(ssrc);
            }
            p["active"] = json!(active && actual.map(|v| truthy(&v["active"])).unwrap_or(i == 0));
        }
        self.send(s.id,12,json!({"audio_ssrc":s.ssrc,"video_ssrc":if active{ssrc}else{0},"rtx_ssrc":if active{s.streams.first().map(|v|v["rtxSsrc"].clone()).unwrap_or(json!(0))}else{json!(0)},"streams":s.streams.iter().map(|v|json!({"type":v["type"],"rid":v["rid"],"ssrc":v["ssrc"],"rtx_ssrc":v["rtxSsrc"],"quality":v["quality"],"active":v["active"]})).collect::<Vec<_>>()}));
        s.camera_active = active;
        self.emit(json!({"event":"cameraState","token":s.camera_token,"mediaSession":s.id,"state":if active{"active"}else if s.camera_requested{"preparing"}else{"stopped"}}));
        if active {
            s.camera_deadline = None;
            if let Some(request) = s.camera_request.take() {
                self.reply(
                    request,
                    Ok(json!({"state":"active","verifiedReceiver":false,"preview":false})),
                );
            }
        }
        Ok(())
    }
    fn share_capture(&mut self, s: &mut Session, selection: ShareSelection) -> Result<()> {
        if !s.stream() || !s.connected {
            return Err(MediaError::new(
                "SHARE_NOT_READY",
                "The separate Go Live media session is not encrypted and connected yet.",
            ));
        }
        let width = (selection.height as f64 * 16. / 9.).round() as u32;
        let bitrate = if selection.height > 720 || selection.fps > 30 {
            9000000
        } else {
            3500000
        };
        if selection.audio {
            s.soundshare_requested = true;
            self.conn_call(s.id,"setTransportOptions",vec![json!({"soundsharePid":selection.pid,"soundshareEventDriven":true,"soundshareLoopback":selection.loopback,"encodingVoiceBitRate":128000})])?;
        }
        let streams: Vec<Value> = s
            .streams
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let mut v = v.clone();
                v["active"] = json!(i == 0);
                v["maxResolution"] =
                    json!({"type":"fixed","width":width,"height":selection.height});
                v["maxFrameRate"] = json!(selection.fps);
                v["maxBitrate"] = json!(bitrate);
                v
            })
            .collect();
        let degradation = native::property_json(self.env, &self.engine()?, "DegradationPreference")
            .ok()
            .and_then(|v| v.get("MAINTAIN_FRAMERATE").cloned())
            .unwrap_or(json!(1));
        self.conn_call(s.id,"setTransportOptions",vec![json!({"streamParameters":streams,"encodingVideoWidth":width,"encodingVideoHeight":selection.height,"encodingVideoFrameRate":selection.fps,"captureVideoFrameRate":selection.fps,"encodingVideoBitRate":600000,"encodingVideoMinBitRate":500000,"encodingVideoMaxBitRate":bitrate,"encodingVideoDegradationPreference":degradation,"remoteSinkWantsMaxFramerate":selection.fps,"remoteSinkWantsPixelCount":width*selection.height})])?;
        let parts: Vec<_> = selection.source.split(':').collect();
        s.capture_active = true;
        self.conn_call(s.id,"setDesktopSourceWithOptions",vec![json!({"type":parts[0],"sourceId":parts[1],"useVideoHook":false,"useGraphicsCapture":true,"useGraphicsCaptureApiLevel":0,"useCaptureDeviceForEncode":true})])?;
        self.conn_call(s.id, "setVideoBroadcast", vec![json!(true)])?;
        self.send(s.id,12,json!({"audio_ssrc":s.ssrc,"video_ssrc":streams.first().map(|v|v["ssrc"].clone()).unwrap_or(json!(0)),"rtx_ssrc":streams.first().map(|v|v["rtxSsrc"].clone()).unwrap_or(json!(0)),"streams":streams.iter().map(|v|json!({"type":v["type"],"rid":v["rid"],"ssrc":v["ssrc"],"rtx_ssrc":v["rtxSsrc"],"quality":v["quality"],"active":v["active"],"max_bitrate":v["maxBitrate"],"max_framerate":v["maxFrameRate"],"max_resolution":v["maxResolution"]})).collect::<Vec<_>>()}));
        s.share_selection = Some(selection);
        Ok(())
    }
    fn clear_share(&mut self, s: &mut Session) -> Result<()> {
        s.capture_active = false;
        s.soundshare_requested = false;
        s.soundshare_attached = false;
        s.clearing_source = true;
        let result = (|| {
            self.conn_call(s.id, "setVideoBroadcast", vec![json!(false)])?;
            self.conn_call(s.id,"setTransportOptions",vec![json!({"soundsharePid":0,"soundshareEventDriven":true,"soundshareLoopback":false})])?;
            self.conn_call(s.id, "clearDesktopSource", vec![])?;
            Ok(())
        })();
        s.clearing_source = false;
        result
    }
    fn tick(&mut self) {
        self.preview_tick();
        let now = Instant::now();
        if !self.mic_stop_failed&&self.mic_test.is_some_and(|at| now >= at) {
            let _=self.stop_mic_test();
        }
        let ids: Vec<_> = self.sessions.keys().copied().collect();
        for id in ids {
            if let Some(mut s) = self.sessions.remove(&id) {
                let result = (|| {
                    if !s.stream()&&(s.input_expired(now)||s.credentials.self_mute||s.credentials.self_deaf||self.mic_test.is_some()||self.settings.input_volume==0.) {
                        self.transmit_gate(&mut s,false)?;
                        if let Some(event)=s.speaking_event(&s.credentials.user_id.clone(),false) {
                            if s.connected {self.send(id,5,json!({"speaking":0,"delay":0,"ssrc":s.ssrc}));}
                            self.emit(event);
                        }
                    }
                    if s.connect_deadline.is_some_and(|at| now >= at) {
                        return Err(MediaError::new(
                            "VOICE_CONNECT_TIMEOUT",
                            "The voice transport did not finish in time.",
                        ));
                    }
                    if s.udp_deadline.is_some_and(|at| now >= at) {
                        return Err(MediaError::new(
                            "UDP_TIMEOUT",
                            "The native UDP transport did not connect.",
                        ));
                    }
                    if s.dave_deadline.is_some_and(|at| now >= at) {
                        return Err(MediaError::new(
                            "VOICE_ENCRYPTION_TIMEOUT",
                            "The encrypted participant group did not finish in time.",
                        ));
                    }
                    if s.next_heartbeat.is_some_and(|at| now >= at) {
                        self.beat(&mut s)?;
                    }
                    if s.camera_deadline.is_some_and(|at| now >= at) {
                        if let Some(req) = s.camera_request.take() {
                            self.reply(
                                req,
                                Err(MediaError::new(
                                    "CAMERA_TIMEOUT",
                                    "The camera did not begin delivering native video in time.",
                                )),
                            );
                        }
                        self.stop_camera(&mut s)?;
                        self.emit(json!({"event":"cameraState","token":s.camera_token,"mediaSession":s.id,"state":"error","reason":{"code":"CAMERA_TIMEOUT","message":"The camera did not begin delivering native video in time."}}));
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    self.fail(&mut s, e)
                }
                if !s.closed {
                    self.sessions.insert(id, s);
                }
            }
        }
        let expired: Vec<Value> = self
            .pending
            .values()
            .filter(|(_, at)| now >= *at)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            let key = id.to_string();
            if let Some(parts) = self.stats_parts.remove(&key) {
                let mut result = parts;
                for part in ["voice", "share"] {
                    if result[part].is_number() {
                        result[part] = stats::unavailable("unknown", "NATIVE_STATS_FAILED");
                    }
                }
                result["sanitized"] = json!(true);
                result["sampledAtUtc"] = json!(chrono::Utc::now().to_rfc3339());
                self.reply(id, Ok(result));
            } else {
                self.reply(
                    id.clone(),
                    Err(MediaError::new(
                        "NATIVE_TIMEOUT",
                        "The native media engine did not reply in time.",
                    )),
                );
                self.inventory_parts.remove(&key);
                self.configure_settings.remove(&key);
            }
        }
    }
    fn stats_start(&mut self, id: Value) -> Result<()> {
        let mut parts = json!({"voice":null,"share":null});
        for (part, session) in [("voice", self.voice), ("share", self.share)] {
            if let Some(session) = session {
                if let Some(s) = self.sessions.get(&session) {
                    if self.connections.contains_key(&session) && !s.closed {
                        parts[part] = json!(session);
                    } else {
                        parts[part] = stats::unavailable(s.state(), "NO_NATIVE_CONNECTION");
                    }
                }
            }
        }
        let pending = parts["voice"].is_number() || parts["share"].is_number();
        if !pending {
            parts["sanitized"] = json!(true);
            parts["sampledAtUtc"] = json!(chrono::Utc::now().to_rfc3339());
            self.reply(id, Ok(parts));
            return Ok(());
        }
        self.stats_parts.insert(id.to_string(), parts.clone());
        self.pending.insert(
            id.to_string(),
            (id.clone(), Instant::now() + Duration::from_secs(2)),
        );
        for part in ["voice", "share"] {
            if let Some(session) = parts[part].as_u64() {
                let conn = self.conn(session)?;
                let tag = Tag::Stats {
                    request: id.clone(),
                    session,
                };
                let result = if conn.has_named_property("getFilteredStats").unwrap_or(false) {
                    self.conn_callback(session, "getFilteredStats", vec![self.json(json!(7))?], tag)
                } else {
                    self.conn_callback(session, "getStats", vec![], tag)
                };
                if result.is_err() {
                    let state = self
                        .sessions
                        .get(&session)
                        .map(|s| s.state())
                        .unwrap_or("disconnected");
                    self.stats_result(
                        id.clone(),
                        session,
                        stats::unavailable(state, "NATIVE_STATS_FAILED"),
                    );
                }
            }
        }
        Ok(())
    }
    fn stats_result(&mut self, id: Value, session: u64, report: Value) {
        let key = id.to_string();
        let Some(parts) = self.stats_parts.get_mut(&key) else {
            return;
        };
        for part in ["voice", "share"] {
            if parts[part].as_u64() == Some(session) {
                parts[part] = report.clone();
            }
        }
        if !parts["voice"].is_number() && !parts["share"].is_number() {
            let mut result = self.stats_parts.remove(&key).unwrap();
            result["sanitized"] = json!(true);
            result["sampledAtUtc"] = json!(chrono::Utc::now().to_rfc3339());
            self.reply(id, Ok(result));
        }
    }
}
fn bytes(v: &Value) -> Result<Vec<u8>> {
    v.as_array()
        .filter(|a| a.len() <= protocol::MAX_FRAME)
        .and_then(|a| {
            a.iter()
                .map(|v| v.as_u64().filter(|n| *n <= 255).map(|n| n as u8))
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(MediaError::internal)
}
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().is_some_and(|n| n != 0.),
        Value::String(v) => !v.is_empty(),
        _ => true,
    }
}
fn socket_closed(code: Option<u16>) -> MediaError {
    let Some(code) = code else {
        return MediaError::new("VOICE_SOCKET_CLOSED", "Se cerró la conexión con el servidor de voz.");
    };
    let reason = match code {
        4004 => "la autorización de voz fue rechazada",
        4006 | 4009 => "la sesión de voz ya no es válida",
        4011 => "el servidor de voz ya no está disponible",
        4012 => "el servidor rechazó el protocolo de voz",
        4014 => "el servidor terminó tu conexión al canal",
        4015 => "el servidor de voz se reinició",
        4016 => "el servidor rechazó el cifrado de transporte",
        4017 => "el servidor requiere otro protocolo de cifrado de extremo a extremo",
        _ => "el servidor cerró la conexión",
    };
    // Only the numeric code and fixed text cross IPC, never a server-provided
    // reason string that could contain credentials or unrelated account data.
    MediaError::new("VOICE_SOCKET_CLOSED", format!("Se cerró la conexión de voz (código {code}): {reason}."))
}
fn socket_failure(error: &tokio_tungstenite::tungstenite::Error) -> MediaError {
    use tokio_tungstenite::tungstenite::Error;
    match error {
        Error::Http(response) => MediaError::new("VOICE_SOCKET_HTTP", format!("El servidor de voz rechazó la conexión (HTTP {}).", response.status().as_u16())),
        Error::Tls(_) => MediaError::new("VOICE_SOCKET_TLS", "No se pudo establecer la conexión segura con el servidor de voz."),
        Error::Io(_) => MediaError::new("VOICE_SOCKET_NETWORK", "La conexión de red con el servidor de voz falló o se interrumpió."),
        Error::Capacity(_) | Error::WriteBufferFull(_) => MediaError::new("VOICE_SOCKET_LIMIT", "La señalización de voz superó los límites de tamaño del transporte."),
        Error::Protocol(_) => MediaError::new("VOICE_SOCKET_PROTOCOL", "El transporte de voz recibió una secuencia WebSocket no válida."),
        Error::ConnectionClosed => MediaError::new("VOICE_SOCKET_CLOSED", "El servidor completó el cierre de la conexión de voz."),
        Error::AlreadyClosed => MediaError::new("VOICE_SOCKET_ALREADY_CLOSED", "Se intentó usar un transporte de voz que ya estaba cerrado."),
        Error::Utf8(_) => MediaError::new("VOICE_SOCKET_UTF8", "La señalización de voz contenía texto no válido."),
        Error::AttackAttempt => MediaError::new("VOICE_SOCKET_HANDSHAKE_LIMIT", "El transporte rechazó un intercambio WebSocket no permitido."),
        Error::Url(_) => MediaError::new("VOICE_SOCKET_ENDPOINT", "No se pudo preparar la dirección del transporte de voz."),
        Error::HttpFormat(_) => MediaError::new("VOICE_SOCKET_HTTP_FORMAT", "La respuesta del transporte no tenía un formato HTTP válido."),
    }
}
fn transport_success(values: &[Value]) -> bool {
    !values.first().is_some_and(truthy) && values.get(1).is_some_and(Value::is_object)
}
fn input_level(value: f64) -> (f64, f64) {
    // setEmitVADLevel2 reports dB. The original input meter treats -100 dB
    // and lower as silence; 0 dB is full scale, never a missing sample.
    if !value.is_finite() || value <= -100. {
        return (0., -100.);
    }
    let db=value.min(0.);
    (10f64.powf(db/20.),db)
}
fn input_sample(values:&[Value])->Option<f64> {
    values.first().and_then(Value::as_f64).or_else(|| {
        values.first().filter(|value|value.is_boolean()).and_then(|_|values.get(1)).and_then(Value::as_f64)
    }).filter(|value|value.is_finite())
}
fn manual_activity(settings:&AudioSettings,level:f64,db:f64)->bool {
    settings.input_volume>0.&&level>0.&&level.is_finite()&&db.is_finite()&&db>=settings.sensitivity_db
}
fn operation_token(current: u64, params: &Value) -> Result<u64> {
    match params.get("token") {
        Some(value) => value.as_u64().filter(|v| *v > 0).ok_or_else(|| {
            MediaError::new(
                "INVALID_OPERATION_TOKEN",
                "A positive operation token is required.",
            )
        }),
        None => current.checked_add(1).ok_or_else(MediaError::internal),
    }
}
#[cfg(test)]
mod callback_tests {
    use super::*;
    #[test]
    fn voice_close_preserves_only_safe_numeric_server_reason() {
        for (code, reason) in [(4004, "autorización"), (4009, "sesión"), (4014, "canal"), (4016, "cifrado"), (4017, "extremo a extremo")] {
            let error = socket_closed(Some(code));
            assert!(error.message.contains(&code.to_string()) && error.message.contains(reason));
        }
        assert!(!socket_closed(None).message.contains("código"));
        assert!(socket_closed(Some(4999)).message.contains("4999"));
    }
    #[test]
    fn successful_native_error_false_is_not_a_failure() {
        assert!(transport_success(&[
            json!(false),
            json!({"protocol":"udp"})
        ]));
        assert!(transport_success(&[Value::Null, json!({"protocol":"udp"})]));
        assert!(!transport_success(&[json!(true), json!({})]));
        assert!(!transport_success(&[json!(false), Value::Null]));
    }
    #[test]
    fn native_input_meter_uses_db_full_scale_silence_and_invalid_samples() {
        for (sample, linear, db) in [
            (-100., 0., -100.),
            (-120., 0., -100.),
            (0., 1., 0.),
            (-20., 0.1, -20.),
            (-40., 0.01, -40.),
            (9., 1., 0.),
        ] {
            let actual = input_level(sample);
            assert!((actual.0 - linear).abs() < 0.000001);
            assert!((actual.1 - db).abs() < 0.000001);
        }
        for sample in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(input_level(sample), (0., -100.));
        }
        for missing in [vec![],vec![Value::Null],vec![json!(false)]] {
            assert_eq!(input_level(input_sample(&missing).unwrap_or(-100.)),(0.,-100.));
        }
    }
    #[test]
    fn selected_manual_threshold_controls_activity_without_a_second_automatic_gate() {
        let mut settings=AudioSettings::default();
        let (level,db)=input_level(-40.);
        settings.sensitivity_db=-60.;assert!(manual_activity(&settings,level,db));
        settings.sensitivity_db=-40.;assert!(manual_activity(&settings,level,db));
        settings.sensitivity_db=-39.;assert!(!manual_activity(&settings,level,db));
        settings.sensitivity_db=-100.;assert!(!manual_activity(&settings,0.,-100.));
        settings.input_volume=0.;assert!(!manual_activity(&settings,level,db));
        settings.input_volume=100.;assert!(!manual_activity(&settings,f64::NAN,db));
    }
    #[test]
    fn native_meter_accepts_level_first_and_boolean_first_callback_layouts() {
        assert_eq!(input_sample(&[json!(-20.),json!(false)]),Some(-20.));
        assert_eq!(input_sample(&[json!(false),json!(-40.)]),Some(-40.));
        assert_eq!(input_sample(&[json!(false)]),None);
        assert_eq!(input_sample(&[json!("0.1"),json!(true)]),None);
    }
    #[test]
    fn operation_tokens_preserve_legacy_increment_and_reject_invalid_generations() {
        assert_eq!(operation_token(8, &json!({})).unwrap(), 9);
        assert_eq!(operation_token(8, &json!({"token":7})).unwrap(), 7);
        for token in [json!(0), json!(-1), json!("9"), Value::Null] {
            assert!(operation_token(8, &json!({"token":token})).is_err());
        }
        assert!(operation_token(u64::MAX, &json!({})).is_err());
    }
}
