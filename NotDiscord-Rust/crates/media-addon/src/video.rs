//! Native RGBA callbacks have their own bounded binary lane, never the actor/JSON queue.
use super::*;
use crate::frames::{LatestFrames, SharedVideoFrames, VideoFrame};
use napi::{JsNumber, JsTypedArray, NapiRaw, NapiValue};
use std::{
    rc::Rc,
    sync::{Mutex, atomic::AtomicU64},
};
#[path = "preview.rs"]
mod preview;
#[cfg(feature = "native-probe")]
#[path = "receive_probe.rs"]
mod receive_probe;
#[cfg(feature = "native-probe")]
pub(crate) use receive_probe::run as probe_receive;

const MAX_NATIVE_BYTES: usize = 4096 * 2160 * 4;
#[derive(Clone)]
struct FrameOutput {
    frames: SharedVideoFrames,
    wake: mpsc::SyncSender<()>,
    available: Arc<AtomicBool>,
}
impl FrameOutput {
    fn start(pipe: Option<String>, closing: Arc<AtomicBool>) -> Self {
        let frames = Arc::new(Mutex::new(LatestFrames::default()));
        let (wake, rx) = mpsc::sync_channel(1);
        let available = Arc::new(AtomicBool::new(false));
        let result = Self {
            frames: frames.clone(),
            wake,
            available: available.clone(),
        };
        let Some(pipe) = pipe.filter(|pipe| {
            super::valid_pipe(pipe.trim_end_matches(".Frames")) && pipe.ends_with(".Frames")
        }) else {
            return result;
        };
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(12);
            let mut output = loop {
                match OpenOptions::new().write(true).open(&pipe) {
                    Ok(file) => break file,
                    Err(_) => {
                        if closing.load(Ordering::Relaxed) || Instant::now() >= deadline {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(40));
                    }
                }
            };
            available.store(true, Ordering::Release);
            while !closing.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(()) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                }
                let batch = frames.lock().unwrap().drain();
                for frame in batch {
                    // A removed subscription may still have an in-flight frame;
                    // the reader also checks its independent allowlist.
                    let Ok(header) = frame.header() else { continue };
                    if output
                        .write_all(&header.encode_header())
                        .and_then(|_| output.write_all(&frame.rgba))
                        .is_err()
                    {
                        available.store(false, Ordering::Release);
                        frames.lock().unwrap().clear();
                        return;
                    }
                }
            }
            available.store(false, Ordering::Release);
            frames.lock().unwrap().clear();
        });
        result
    }
    fn publish(&self, frame: VideoFrame) -> bool {
        if let Ok(mut frames) = self.frames.try_lock() {
            if frames.publish(frame) {
                let _ = self.wake.try_send(());
                return true;
            }
        }
        false
    }
}
struct Sink {
    alive: Arc<AtomicBool>,
    delivered: Arc<AtomicBool>,
    engine: Rc<RefCell<Option<ObjectRef>>>,
    quality: Arc<Mutex<(u32, u32, u32)>>,
}
impl Sink {
    fn detach(self, env: Env, native_stream: &str) {
        self.alive.store(false, Ordering::Release);
        // Clearing a sink may synchronously deliver its final native callback.
        // Keep its reference available for the final readiness acknowledgement,
        // but never hold the RefCell guard while entering native code.
        let engine = self
            .engine
            .borrow()
            .as_ref()
            .and_then(|value| value.object(env).ok());
        if let Some(engine) = engine {
            let _ = native::call_json(
                env,
                &engine,
                "setVideoOutputSink",
                vec![json!(native_stream)],
            );
        }
        let reference = self.engine.borrow_mut().take();
        if let Some(reference) = reference {
            reference.release(env);
        }
    }
}
#[cfg(feature = "native-probe")]
pub(crate) fn probe_synthetic_detach(env: Env) -> napi::Result<bool> {
    let cleared = Arc::new(AtomicBool::new(false));
    let seen = cleared.clone();
    let acknowledged = Arc::new(AtomicBool::new(false));
    let ack = acknowledged.clone();
    let reference = Rc::new(RefCell::new(None));
    let reentrant = reference.clone();
    let clear = env.create_function_from_closure("fakeDetachSink", move |ctx| {
        let engine = reentrant.try_borrow().ok().and_then(|value| {
            value
                .as_ref()
                .and_then(|value: &ObjectRef| value.object(*ctx.env).ok())
        });
        seen.store(ctx.length == 1 && engine.is_some(), Ordering::SeqCst);
        if let Some(engine) = engine {
            native::call_json(
                *ctx.env,
                &engine,
                "signalVideoOutputSinkReady",
                vec![json!("synthetic-stream")],
            )
            .map_err(|_| {
                napi::Error::from_reason("Reentrant final-frame acknowledgement failed.")
            })?;
        }
        ctx.env.get_undefined()
    })?;
    let ready = env.create_function_from_closure("fakeFinalFrameReady", move |ctx| {
        ack.store(ctx.length == 1, Ordering::SeqCst);
        ctx.env.get_undefined()
    })?;
    let mut engine = env.create_object()?;
    engine.set_named_property("setVideoOutputSink", clear)?;
    engine.set_named_property("signalVideoOutputSinkReady", ready)?;
    let own = ObjectRef::new(env, engine)
        .map_err(|_| napi::Error::from_reason("Fake engine reference failed."))?;
    *reference.borrow_mut() = Some(own);
    let observed = reference.clone();
    let alive = Arc::new(AtomicBool::new(true));
    let enabled = alive.clone();
    Sink {
        alive,
        delivered: Arc::new(AtomicBool::new(false)),
        engine: reference,
        quality: Arc::new(Mutex::new((640, 360, 12))),
    }
    .detach(env, "synthetic-stream");
    Ok(cleared.load(Ordering::SeqCst)
        && acknowledged.load(Ordering::SeqCst)
        && !enabled.load(Ordering::Acquire)
        && observed.borrow().is_none())
}
#[derive(Clone)]
struct Source {
    id: u64,
    native: String,
    session: u64,
    user: String,
    kind: &'static str,
    local: bool,
    stream_key: String,
    ready: bool,
    visible: bool,
    preview: bool,
    source_id: String,
    selection_token: u64,
    operation_token: u64,
    sequence: Arc<AtomicU64>,
}
pub(super) struct VideoState {
    output: FrameOutput,
    sources: HashMap<String, Source>,
    sinks: HashMap<String, Sink>,
    sequence: u64,
    visible: bool,
    pub(super) preview: Option<String>,
    preview_token: u64,
    screen_preview: Option<preview::ScreenPreview>,
}
impl VideoState {
    pub(super) fn new(closing: Arc<AtomicBool>) -> Self {
        Self {
            output: FrameOutput::start(std::env::var("NOTDISCORD_VIDEO_PIPE").ok(), closing),
            sources: HashMap::new(),
            sinks: HashMap::new(),
            sequence: std::env::var("NOTDISCORD_VIDEO_NAMESPACE").ok().and_then(|value|value.parse::<u64>().ok()).filter(|base|base & 0xffff_ffff==0 && *base<=(1_000_000u64<<32)).unwrap_or(0),
            visible: true,
            preview: None,
            preview_token: 0,
            screen_preview: None,
        }
    }
}
fn metadata(source: &Source, active: bool, state: &str) -> Value {
    json!({"event":"videoStream","id":source.id,"userId":source.user,"kind":source.kind,"local":source.local,"preview":source.preview,"previewToken":source.selection_token,"token":source.operation_token,"sourceId":source.source_id,"label":if source.preview{"Vista previa"}else if source.kind=="screen"{"Pantalla compartida"}else{"Cámara"},"active":active,"visible":source.visible,"state":state,"streamKey":source.stream_key,"mediaSession":source.session})
}
fn fitted_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let factor = (max_width as f64 / width as f64)
        .min(max_height as f64 / height as f64)
        .min(1.);
    (
        ((width as f64 * factor).floor() as u32).max(1),
        ((height as f64 * factor).floor() as u32).max(1),
    )
}
fn copy_scaled_rgba(
    bytes: &[u8],
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return None;
    }
    let required = width as usize * height as usize * 4;
    if width == 0 || height == 0 || required > MAX_NATIVE_BYTES || bytes.len() != required {
        return None;
    }
    let (w, h) = fitted_size(
        width,
        height,
        max_width.clamp(1, 1280),
        max_height.clamp(1, 720),
    );
    let mut output = vec![0; w as usize * h as usize * 4];
    if (w, h) == (width, height) {
        output.copy_from_slice(bytes);
    } else {
        for y in 0..h {
            for x in 0..w {
                let from = ((y as u64 * height as u64 / h as u64) * width as u64
                    + x as u64 * width as u64 / w as u64) as usize
                    * 4;
                let to = (y as usize * w as usize + x as usize) * 4;
                output[to..to + 4].copy_from_slice(&bytes[from..from + 4]);
            }
        }
    }
    Some((w, h, output))
}
pub(crate) fn read_image(
    env: Env,
    image: JsObject,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    let width = image
        .get_named_property::<JsNumber>("width")
        .ok()?
        .get_uint32()
        .ok()?;
    let height = image
        .get_named_property::<JsNumber>("height")
        .ok()?
        .get_uint32()
        .ok()?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return None;
    }
    let length = width as u64 * height as u64 * 4;
    if width == 0 || height == 0 || length > MAX_NATIVE_BYTES as u64 {
        return None;
    }
    let data = image.get_named_property::<JsUnknown>("data").ok()?;
    if !data.is_typedarray().ok()? {
        return None;
    }
    let array = unsafe { JsTypedArray::from_raw_unchecked(env.raw(), data.raw()) }
        .into_value()
        .ok()?;
    if !matches!(
        array.typedarray_type,
        napi::TypedArrayType::Uint8 | napi::TypedArrayType::Uint8Clamped
    ) || array.length != length as usize
    {
        return None;
    }
    let offset = array.byte_offset;
    let buffer = array.arraybuffer.into_value().ok()?;
    let bytes = buffer.as_ref();
    let data = bytes.get(offset..offset.checked_add(length as usize)?)?;
    copy_scaled_rgba(data, width, height, max_width, max_height)
}
impl Host {
    pub(super) fn video_receive_enabled(&self)->bool {self.video.visible}
    #[cfg(feature = "native-probe")]
    pub(super) fn video_probe_counts(&self) -> Value {
        json!({"sources":self.video.sources.len(),"sinks":self.video.sinks.len(),"active":self.video.output.frames.lock().unwrap().active_ids().len()})
    }
    pub(super) fn video_attach(&mut self, native_stream: &str) -> Result<()> {
        if !self.video.visible || self.video.sinks.contains_key(native_stream) {
            return Ok(());
        }
        let source = self
            .video
            .sources
            .get(native_stream)
            .cloned()
            .ok_or_else(MediaError::internal)?;
        if !source.visible {
            return Ok(());
        }
        if source.preview && source.kind == "screen" {
            return Ok(());
        }
        if !source.ready {
            return Ok(());
        }
        if !self.video.output.available.load(Ordering::Acquire) {
            return Err(MediaError::new(
                "VIDEO_PIPE_UNAVAILABLE",
                "The private video lane is not connected.",
            ));
        }
        if !self.video.output.frames.lock().unwrap().activate(source.id) {
            let error=MediaError::new("VIDEO_SINK_LIMIT","Cierra otro vídeo antes de abrir esta fuente.");
            let mut event=metadata(&source,false,"error");event["message"]=json!(error.message);self.emit(event);
            return Err(error);
        }
        let engine = Rc::new(RefCell::new(Some(ObjectRef::new(
            self.env,
            self.engine()?,
        )?)));
        let own_engine = engine.clone();
        let stream = native_stream.to_owned();
        let alive = Arc::new(AtomicBool::new(true));
        let active = alive.clone();
        let quality = Arc::new(Mutex::new((640, 360, 12)));
        let size = quality.clone();
        let frames = self.video.output.clone();
        let sequence = source.sequence.clone();
        let first_frame = Arc::new(AtomicBool::new(false));
        let delivered = first_frame.clone();
        let event_output = self.output.clone();
        let playing = metadata(&source, true, "playing");
        let last = RefCell::new(None::<Instant>);
        let callback = self
            .env
            .create_function_from_closure("rawVideoFrame", move |ctx| {
                let render = (|| {
                    if !active.load(Ordering::Acquire) || !frames.available.load(Ordering::Acquire)
                    {
                        return None;
                    }
                    let (w, h, fps) = *size.lock().ok()?;
                    if last.borrow().is_some_and(|time| {
                        time.elapsed() < Duration::from_millis(1000 / fps as u64)
                    }) {
                        return None;
                    }
                    *last.borrow_mut() = Some(Instant::now());
                    let image = ctx.get::<JsObject>(0).ok()?;
                    let (width, height, rgba) = read_image(*ctx.env, image, w, h)?;
                    let sequence = sequence.fetch_add(1, Ordering::Relaxed) + 1;
                    let accepted = frames.publish(VideoFrame {
                        subscription_id: source.id,
                        sequence,
                        width,
                        height,
                        rgba,
                    });
                    if accepted && !delivered.swap(true, Ordering::AcqRel) {
                        let _ = event_output.send(playing.clone());
                    }
                    Some(())
                })();
                let _ = render;
                // The native producer waits for this acknowledgement even when a
                // malformed, throttled or superseded frame was intentionally dropped.
                if let Some(engine) = own_engine
                    .borrow()
                    .as_ref()
                    .and_then(|reference| reference.object(*ctx.env).ok())
                {
                    let _ = native::call_json(
                        *ctx.env,
                        &engine,
                        "signalVideoOutputSinkReady",
                        vec![json!(stream)],
                    );
                }
                ctx.env.get_undefined()
            })
            .map_err(|_| MediaError::internal())?;
        let result = native::call(
            self.env,
            &self.engine()?,
            "setVideoOutputSink",
            &[
                self.json(json!(native_stream))?,
                callback.into_unknown(),
                self.json(json!(true))?,
            ],
        );
        if result.is_err() {
            self.video.output.frames.lock().unwrap().remove(source.id);
            if let Some(reference) = engine.borrow_mut().take() {
                reference.release(self.env);
            }
            return Err(MediaError::internal());
        }
        self.video.sinks.insert(
            native_stream.into(),
            Sink {
                alive,
                delivered: first_frame.clone(),
                engine,
                quality,
            },
        );
        if !source.local {
            if let Err(error) = self.conn_call(
                source.session,
                "setDisableLocalVideo",
                vec![json!(source.user), json!(false)],
            ) {
                let _ = self.video_detach(native_stream, false);
                let mut failed = metadata(&source, false, "error");
                failed["message"] = json!(error.message);
                self.emit(failed);
                return Err(error);
            }
        }
        self.emit(metadata(&source, first_frame.load(Ordering::Acquire), if first_frame.load(Ordering::Acquire) {"playing"} else {"loading"}));
        Ok(())
    }
    pub(super) fn video_detach(&mut self, native_stream: &str, remove: bool) -> Result<()> {
        let mut result = Ok(());
        if let Some(sink) = self.video.sinks.remove(native_stream) {
            sink.detach(self.env, native_stream);
        }
        if let Some(source) = self.video.sources.get(native_stream).cloned() {
            if !source.local {
                result = self.conn_call(
                    source.session,
                    "setDisableLocalVideo",
                    vec![json!(source.user), json!(true)],
                ).map(|_| ());
            }
            self.video.output.frames.lock().unwrap().remove(source.id);
            self.emit(metadata(
                &source,
                false,
                if remove { "removed" } else { "paused" },
            ));
        }
        if remove {
            self.video.sources.remove(native_stream);
        }
        result
    }
    pub(super) fn video_source(
        &mut self,
        session: u64,
        user: &str,
        native_stream: &str,
        kind: &'static str,
        local: bool,
        stream_key: &str,
        ready: bool,
    ) -> Result<()> {
        if native_stream.is_empty()
            || native_stream.len() > 1024
            || native_stream.chars().any(char::is_control)
        {
            return Ok(());
        }
        let old: Vec<_> = self
            .video
            .sources
            .values()
            .filter(|source| {
                source.session == session
                    && source.user == user
                    && source.kind == kind
                    && source.local == local
                    && source.native != native_stream
            })
            .map(|source| source.native.clone())
            .collect();
        // A device selector is an explicit local preview. Publishing camera
        // or screen is independent of viewing it in the call grid. A remote
        // screen first registers its id with the app; the app then explicitly
        // enables the sink after authorizing the independent binary frame lane.
        let visible = local && session == 0;
        let promoted = self.video.sources.get(native_stream)
            .is_some_and(|source| source.preview && local && session != 0);
        for stream in old {
            let _ = self.video_detach(&stream, true);
        }
        if !self.video.sources.contains_key(native_stream) {
            if self.video.sources.len() >= 32 {
                return Ok(());
            }
            self.video.sequence += 1;
            let source = Source {
                id: self.video.sequence,
                native: native_stream.into(),
                session,
                user: user.into(),
                kind,
                local,
                stream_key: stream_key.into(),
                ready,
                visible,
                preview: local && session == 0,
                source_id: if local {
                    native_stream.into()
                } else {
                    String::new()
                },
                selection_token: self.video.preview_token,
                operation_token:self.watch_tokens.get(stream_key).copied().unwrap_or(0),
                sequence: Arc::new(AtomicU64::new(0)),
            };
            self.emit(metadata(
                &source,
                false,
                if source.visible || !local&&kind=="screen" { "waiting" } else { "paused" },
            ));
            self.video.sources.insert(native_stream.into(), source);
        } else if let Some(source) = self.video.sources.get_mut(native_stream) {
            // Confirming a selector promotes the very same camera sink to
            // outgoing state. A later selector cleanup cannot remove it.
            source.session = session;
            source.preview = local && session == 0;
            source.stream_key = stream_key.into();
            source.ready = ready;
            if promoted { source.visible = false; }
            let updated = source.clone();
            let playing = !promoted && self.video.sinks.get(native_stream).is_some_and(|sink|sink.delivered.load(Ordering::Acquire));
            self.emit(metadata(
                &updated,
                playing,
                if playing {"playing"} else if updated.visible {"loading"} else {"paused"},
            ));
        }
        if promoted { self.video_detach(native_stream, false)?; }
        let result = self.video_attach(native_stream);
        // Incoming video registers before attaching its sink.
        // Disabling its decoder here can invalidate that new registration
        // before the app's enable command arrives on the control lane.
        // Per-user native reception is controlled by the session's explicit
        // SFU interest, independently of whether its app sink exists yet.
        result
    }
    pub(super) fn video_hide_peer(&mut self,session:u64,user:&str) {
        let streams=self.video.sources.values_mut().filter(|source|source.session==session&&!source.local&&source.user==user).map(|source|{source.visible=false;source.native.clone()}).collect::<Vec<_>>();
        for stream in streams {let _=self.video_detach(&stream,false);}
    }
    pub(super) fn video_remove_user(&mut self, session: u64, user: &str) {
        let streams: Vec<_> = self
            .video
            .sources
            .values()
            .filter(|source| source.session == session && source.user == user && !source.local)
            .map(|source| source.native.clone())
            .collect();
        for stream in streams {
            let _ = self.video_detach(&stream, true);
        }
    }
    pub(super) fn video_remove_session(&mut self, session: u64) {
        let streams: Vec<_> = self
            .video
            .sources
            .values()
            .filter(|source| source.session == session)
            .map(|source| source.native.clone())
            .collect();
        for stream in streams {
            let _ = self.video_detach(&stream, true);
        }
    }
    pub(super) fn video_remove_local(&mut self, session: u64, kind: &str) {
        let streams: Vec<_> = self
            .video
            .sources
            .values()
            .filter(|source| source.session == session && source.local && source.kind == kind)
            .map(|source| source.native.clone())
            .collect();
        for stream in streams {
            let _ = self.video_detach(&stream, true);
        }
    }
    pub(super) fn video_local_source_id(&mut self, native: &str, source_id: &str) {
        if let Some(source) = self.video.sources.get_mut(native) {
            source.source_id = source_id.into();
            let source = source.clone();
            self.emit(metadata(
                &source,
                self.video.sinks.contains_key(native),
                "active",
            ));
        }
    }
    pub(super) fn video_error(&mut self, session: u64, user: &str, error: MediaError) {
        let affected: Vec<_> = self
            .video
            .sources
            .values()
            .filter(|source| source.session == session && (user.is_empty() || source.user == user))
            .cloned()
            .collect();
        if affected.is_empty() {
            self.emit(json!({"event":"videoError","scope":"stream","mediaSession":session,"userId":user,"message":error.message,"reason":error.json()}));
        }
        for source in affected {
            let _ = self.video_detach(&source.native, false);
            let mut event = metadata(&source, false, "error");
            event["message"] = json!(error.message);
            self.emit(event);
        }
    }
    pub(super) fn video_resume(&mut self, session: u64) {
        if session != 0 {
            for source in self
                .video
                .sources
                .values_mut()
                .filter(|source| source.session == session)
            {
                source.ready = true;
            }
        }
        let streams: Vec<_> = self
            .video
            .sources
            .values()
            .filter(|source| source.session == session || session == 0)
            .map(|source| source.native.clone())
            .collect();
        for stream in streams {
            if let Err(error)=self.video_attach(&stream)
                && let Some(source)=self.video.sources.get(&stream).cloned() {
                self.video_error(source.session,&source.user,error);
            }
        }
    }
    pub(super) fn video_suspend(&mut self, session: u64) {
        let streams: Vec<_> = self
            .video
            .sources
            .values_mut()
            .filter(|source| source.session == session)
            .map(|source| {
                source.ready = false;
                source.native.clone()
            })
            .collect();
        for stream in streams {
            let _ = self.video_detach(&stream, false);
        }
    }
    pub(super) fn video_stop_preview(&mut self) -> Result<()> {
        self.stop_screen_preview();
        if let Some(device) = self.video.preview.take() {
            let _ = self.video_detach(&device, true);
            if !self
                .sessions
                .values()
                .any(|session| session.camera_requested && !session.closed)
            {
                self.eng("setVideoInputDevice", vec![json!("disabled")])?;
            }
        }
        Ok(())
    }
    pub(super) fn video_start_preview(&mut self, p: &Value) -> Result<Value> {
        let token = operation_token(self.video.preview_token, p)?;
        if token <= self.video.preview_token {
            return Err(MediaError::new(
                "PREVIEW_CANCELLED",
                "This preview was already cancelled.",
            ));
        }
        let device = protocol::string(&p["deviceId"], "deviceId", 1024)?;
        if !self.inventories.get("cameras").is_some_and(|(time, ids)| {
            time.elapsed() < Duration::from_secs(600) && ids.contains(&device)
        }) {
            return Err(MediaError::new(
                "CAMERA_NOT_LISTED",
                "Select an available camera again.",
            ));
        }
        if self
            .sessions
            .values()
            .any(|session| session.camera_requested)
            && self.video.preview.as_deref() != Some(&device)
        {
            return Err(MediaError::new(
                "CAMERA_BUSY",
                "Stop the outgoing camera before previewing another device.",
            ));
        }
        if !self.video.visible {
            return Err(MediaError::new(
                "VIDEO_NOT_VISIBLE",
                "Open the video view before starting a local preview.",
            ));
        }
        self.initialize()?;
        self.video_stop_preview()?;
        self.video.preview_token = token;
        let user = p["userId"].as_str().unwrap_or("");
        if let Err(error) = self.video_source(0, user, &device, "camera", true, "", true) {
            let _ = self.video_detach(&device, true);
            return Err(error);
        }
        if !self.video.sinks.contains_key(&device) {
            let _ = self.video_detach(&device, true);
            return Err(MediaError::new(
                "VIDEO_LIMIT",
                "Close another video before starting preview.",
            ));
        }
        self.video.preview = Some(device.clone());
        if self
            .eng("setVideoInputDevice", vec![json!(device)])
            .is_err()
        {
            let _ = self.video_stop_preview();
            return Err(MediaError::internal());
        }
        Ok(json!({"state":"active","localOnly":true,"token":token}))
    }
    pub(super) fn video_command(&mut self, name: &str, p: &Value) -> Result<Value> {
        match name {
            "startCameraPreview" => self.video_start_preview(p),
            "startScreenPreview" => self.start_screen_preview(p),
            "stopCameraPreview" | "stopMediaPreview" => {
                let token = operation_token(self.video.preview_token, p)?;
                if token >= self.video.preview_token {
                    self.video.preview_token = token;
                    self.video_stop_preview()?;
                }
                Ok(json!({"state":"stopped"}))
            }
            "setVideoVisible" => {
                self.video.visible = p["visible"].as_bool().ok_or_else(MediaError::internal)?;
                if self.video.visible {
                    self.video_resume(0);
                } else {
                    self.stop_screen_preview();
                    let streams: Vec<_> = self.video.sinks.keys().cloned().collect();
                    for stream in streams {
                        let session = self.video.sources.get(&stream).filter(|source|!source.local).map(|source|(source.session,source.user.clone()));
                        if let Err(error)=self.video_detach(&stream, false) && let Some((session,user))=session {
                            if self.sessions.get(&session).is_some_and(|session|session.receive()) {self.close_id(session);}
                            else {self.video_error(session,&user,error);}
                        }
                    }
                    if !self
                        .sessions
                        .values()
                        .any(|session| session.camera_requested)
                    {
                        self.video_stop_preview()?;
                    }
                }
                let receivers=self.sessions.iter().filter(|(_,session)|session.receive()||!session.stream()).map(|(id,_)|*id).collect::<Vec<_>>();
                for id in receivers {
                    if let Some(mut session)=self.sessions.remove(&id) {
                        let result=self.sync_receive_video(&mut session).and_then(|_|self.sync_camera_video(&mut session));
                        if let Err(error)=result {
                            if session.receive() {self.fail(&mut session,error);}
                            else {self.video_error(session.id,"",error);}
                        }
                        if !session.closed {self.sessions.insert(id,session);}
                    }
                }
                Ok(json!({"visible":self.video.visible}))
            }
            "setVideoStreamVisible" => {
                let id = p["id"].as_u64().ok_or_else(MediaError::internal)?;
                let visible = p["visible"].as_bool().ok_or_else(MediaError::internal)?;
                let (native, session, local, user) = self
                    .video
                    .sources
                    .values_mut()
                    .find(|source| source.id == id)
                    .map(|source| {
                        source.visible = visible;
                        (source.native.clone(), source.session, source.local, source.user.clone())
                    })
                    .ok_or_else(|| {
                        MediaError::new(
                            "VIDEO_NOT_FOUND",
                            "This video source is no longer available.",
                        )
                    })?;
                if visible {
                    self.video_attach(&native)?;
                } else {
                    // A GoLive receiver owns a separate connection. A camera
                    // shares the main voice transport: report decoder failure
                    // with the sink released, without destroying working audio.
                    if let Err(error) = self.video_detach(&native, false) {
                        if !local && self.sessions.get(&session).is_some_and(|session|session.receive()) { self.close_id(session); }
                        else {self.video_error(session,&user,error.clone());}
                        return Err(error);
                    }
                }
                Ok(json!({"id":id,"visible":visible}))
            }
            "setVideoQuality" => {
                let id = p["id"].as_u64().ok_or_else(MediaError::internal)?;
                let large = p["large"] == true;
                for (native, sink) in &self.video.sinks {
                    let selected = large && self.video.sources[native].id == id;
                    *sink.quality.lock().unwrap() = if selected {
                        (1280, 720, 24)
                    } else {
                        (640, 360, 12)
                    };
                }
                Ok(json!({"applied":true}))
            }
            "focusVideo" => {
                let id = p["id"].as_u64().ok_or_else(MediaError::internal)?;
                let stream = self
                    .video
                    .sources
                    .values()
                    .find(|source| source.id == id)
                    .map(|source| source.native.clone())
                    .ok_or_else(MediaError::internal)?;
                if self.video.sinks.len() >= 4 && !self.video.sinks.contains_key(&stream) {
                    if let Some(old) = self
                        .video
                        .sinks
                        .keys()
                        .find(|key| key.as_str() != stream)
                        .cloned()
                    {
                        let _ = self.video_detach(&old, false);
                    }
                }
                self.video_attach(&stream)?;
                Ok(json!({"applied":true}))
            }
            _ => Err(MediaError::internal()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rgba_downsample_preserves_channels_alpha_aspect_and_bounds() {
        let pixels = vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 255, 0,
        ];
        let (w, h, small) = copy_scaled_rgba(&pixels, 2, 2, 1, 1).unwrap();
        assert_eq!((w, h), (1, 1));
        assert_eq!(small, [255, 0, 0, 255]);
        assert_eq!(
            copy_scaled_rgba(&pixels, 2, 2, 1280, 720).unwrap().2,
            pixels
        );
        assert!(copy_scaled_rgba(&pixels[..15], 2, 2, 1, 1).is_none());
        assert_eq!(fitted_size(1920, 1080, 640, 360), (640, 360));
        assert_eq!(fitted_size(1080, 1920, 640, 360), (202, 360));
        assert!(copy_scaled_rgba(&[], u32::MAX, u32::MAX, 640, 360).is_none());
        assert!(copy_scaled_rgba(&[], 1 << 31, 1 << 31, 640, 360).is_none());
    }
}
