//! One selected desktop thumbnail at a time. Native URLs remain inside the
//! helper; only bounded decoded RGBA enters the binary frame lane.
use super::*;
use base64::Engine;
use std::io::Cursor;
pub(super) struct ScreenPreview {
    token: u64,
    source: String,
    native: String,
    alive: Arc<AtomicBool>,
}
#[derive(Default)]
pub(super) struct Pipeline {
    demand: Option<Demand>,
    lease: Option<Query>,
    sequence: u64,
}
struct Demand { token: u64, next: Instant, failed: bool }
struct Query { id: u64, token: u64, started: Instant, expired: bool }
#[derive(Debug, PartialEq, Eq)]
enum PreviewAction { Idle, Query { id: u64, token: u64 }, Expired { token: u64 } }
struct Completion { token: u64, deliver: bool }
impl Pipeline {
    fn select(&mut self, token: u64, now: Instant) {
        self.demand = Some(Demand { token, next: now, failed: false });
    }
    fn cancel(&mut self) { self.demand = None; }
    fn tick(&mut self, now: Instant) -> PreviewAction {
        if let Some(query) = self.lease.as_mut() {
            if !query.expired && now.saturating_duration_since(query.started) >= Duration::from_secs(5) {
                query.expired = true;
                if let Some(demand) = self.demand.as_mut().filter(|demand| demand.token == query.token) { demand.failed = true; }
                return PreviewAction::Expired { token: query.token };
            }
            return PreviewAction::Idle;
        }
        let Some(demand) = self.demand.as_mut().filter(|demand| !demand.failed && now >= demand.next) else { return PreviewAction::Idle; };
        let Some(id) = self.sequence.checked_add(1) else {
            demand.failed = true;
            return PreviewAction::Expired { token: demand.token };
        };
        self.sequence = id;
        self.lease = Some(Query { id, token: demand.token, started: now, expired: false });
        PreviewAction::Query { id, token: demand.token }
    }
    fn can_deliver(&self, id: u64, now: Instant) -> bool {
        self.lease.as_ref().is_some_and(|query| query.id == id && !query.expired
            && now.saturating_duration_since(query.started) < Duration::from_secs(5)
            && self.demand.as_ref().is_some_and(|demand| demand.token == query.token && !demand.failed))
    }
    /// Only a real callback/Promise settlement releases its exact lease. A
    /// timeout, cancellation or a newer selection never asserts completion.
    fn complete(&mut self, id: u64, now: Instant, ok: bool) -> Option<Completion> {
        if !self.lease.as_ref().is_some_and(|query| query.id == id) { return None; }
        let query = self.lease.take().unwrap();
        let current = self.demand.as_ref().is_some_and(|demand| demand.token == query.token);
        let deliver = ok && current && !query.expired && now.saturating_duration_since(query.started) < Duration::from_secs(5)
            && self.demand.as_ref().is_some_and(|demand| !demand.failed);
        if let Some(demand) = self.demand.as_mut().filter(|demand| demand.token == query.token) {
            demand.failed |= query.expired || now.saturating_duration_since(query.started) >= Duration::from_secs(5);
            demand.next = now + Duration::from_millis(750);
        }
        Some(Completion { token: query.token, deliver })
    }
    fn fail_unknown(&mut self, id: u64) -> Option<u64> {
        let query = self.lease.as_mut().filter(|query| query.id == id)?;
        query.expired = true;
        if let Some(demand) = self.demand.as_mut().filter(|demand| demand.token == query.token) { demand.failed = true; }
        Some(query.token)
    }
    fn failed(&self, token: u64) -> bool { self.demand.as_ref().is_some_and(|demand| demand.token == token && demand.failed) }
}
fn preview_pixels(url: &str) -> Option<(u32, u32, Vec<u8>)> {
    let data = url
        .strip_prefix("data:image/png;base64,")
        .or_else(|| url.strip_prefix("data:image/jpeg;base64,"))
        .or_else(|| url.strip_prefix("data:image/bmp;base64,"))?;
    if data.len() > 2 * 1024 * 1024 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1280);
    limits.max_image_height = Some(720);
    limits.max_alloc = Some(8 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    copy_scaled_rgba(&image, image.width(), image.height(), 640, 360)
}
fn selected_url(env: Env, list: JsObject, source: &str) -> Option<String> {
    let length = list.get_array_length().ok()?.min(128);
    for index in 0..length {
        let item = list.get_element::<JsObject>(index).ok()?;
        let id = item
            .get_named_property::<napi::JsString>("id")
            .ok()?
            .into_utf8()
            .ok()?
            .as_str()
            .ok()?
            .to_owned();
        if id == source {
            return item
                .get_named_property::<napi::JsString>("url")
                .ok()?
                .into_utf8()
                .ok()?
                .as_str()
                .ok()
                .map(str::to_owned);
        }
    }
    let _ = env;
    None
}
impl Host {
    pub(crate) fn stop_screen_preview(&mut self) {
        self.video.preview_pipeline.borrow_mut().cancel();
        if let Some(preview) = self.video.screen_preview.take() {
            preview.alive.store(false, Ordering::Release);
            let _ = self.video_detach(&preview.native, true);
        }
    }
    pub(super) fn start_screen_preview(&mut self, p: &Value) -> Result<Value> {
        #[cfg(feature = "native-probe")]
        trace("preview-start");
        let token = operation_token(self.video.preview_token, p)?;
        if token <= self.video.preview_token {
            return Err(MediaError::new(
                "PREVIEW_CANCELLED",
                "This selection was cancelled.",
            ));
        }
        let selection = ShareSelection::parse(p)?;
        if !self.inventories.get("sources").is_some_and(|(at, ids)| {
            at.elapsed() < Duration::from_secs(600) && ids.contains(&selection.source)
        }) {
            return Err(MediaError::new(
                "SOURCE_NOT_LISTED",
                "Choose an available source again.",
            ));
        }
        if !self.video.visible || !self.video.output.available.load(Ordering::Acquire) {
            return Err(MediaError::new(
                "VIDEO_PIPE_UNAVAILABLE",
                "Open the preview before selecting a source.",
            ));
        }
        // Desktop preview methods dereference native state created by
        // initialize(). A source catalog may also be supplied by a trusted
        // caller; never rely on listSources having initialized it earlier.
        self.initialize()?;
        self.video_stop_preview()?;
        #[cfg(feature = "native-probe")]
        trace("preview-old-stopped");
        self.video.preview_token = token;
        self.video.sequence += 1;
        let native = format!("preview:{}", selection.source);
        let source = Source {
            id: self.video.sequence,
            native: native.clone(),
            session: 0,
            user: p["userId"].as_str().unwrap_or("").into(),
            kind: "screen",
            local: true,
            stream_key: String::new(),
            ready: true,
            visible: true,
            preview: true,
            source_id: selection.source.clone(),
            selection_token: token,
            operation_token:0,
            sequence: Arc::new(AtomicU64::new(0)),
        };
        if !self.video.output.frames.lock().unwrap().activate(source.id) {
            return Err(MediaError::new(
                "VIDEO_LIMIT",
                "Close another video before previewing.",
            ));
        }
        #[cfg(feature = "native-probe")]
        trace("preview-slot-active");
        self.emit(metadata(&source, true, "loading"));
        self.video.sources.insert(native.clone(), source);
        self.video.screen_preview = Some(ScreenPreview {
            token,
            source: selection.source,
            native,
            alive: Arc::new(AtomicBool::new(true)),
        });
        self.video.preview_pipeline.borrow_mut().select(token, Instant::now());
        self.preview_tick();
        #[cfg(feature = "native-probe")]
        trace("preview-start-return");
        Ok(json!({"state":"loading","localOnly":true,"token":token}))
    }
    pub(crate) fn preview_result(&mut self, token: u64, source: &str, ok: bool) {
        let Some(preview) = self
            .video
            .screen_preview
            .as_mut()
            .filter(|preview| preview.token == token && preview.source == source)
        else {
            return;
        };
        if !ok && self.video.preview_pipeline.borrow().failed(token) { preview.alive.store(false, Ordering::Release); }
        let native = preview.native.clone();
        if let Some(source) = self.video.sources.get(&native) {
            let mut value = metadata(source, ok, if ok { "active" } else { "error" });
            if !ok {
                value["message"] =
                    json!("La fuente seleccionada no produjo una vista previa compatible.");
            }
            self.emit(value);
        }
    }
    pub(crate) fn preview_tick(&mut self) {
        self.preview_tick_at(Instant::now());
    }
    pub(crate) fn preview_tick_at(&mut self, now: Instant) {
        let action = self.video.preview_pipeline.borrow_mut().tick(now);
        if let PreviewAction::Expired { token } = action {
            if let Some(preview) = self.video.screen_preview.as_ref().filter(|preview| preview.token == token) {
                let source = preview.source.clone();
                self.preview_result(token, &source, false);
            }
            return;
        }
        let PreviewAction::Query { id: query, token } = action else { return; };
        let Some(preview) = self.video.screen_preview.as_mut().filter(|preview| preview.token == token) else {
            self.video.preview_pipeline.borrow_mut().complete(query, now, false);
            return;
        };
        let source_id = preview.source.clone();
        let alive = preview.alive.clone();
        let Some(source) = self.video.sources.get(&preview.native).cloned() else {
            let mut pipeline = self.video.preview_pipeline.borrow_mut();
            pipeline.fail_unknown(query);
            pipeline.complete(query, now, false);
            return;
        };
        let output = self.video.output.clone();
        let sender = self.sender.clone();
        let result_source = source_id.clone();
        let pipeline = self.video.preview_pipeline.clone();
        #[cfg(feature = "native-probe")]
        trace("preview-callback-before");
        let callback =
            self.env
                .create_function_from_closure("selectedDesktopPreview", move |ctx| {
                    #[cfg(feature = "native-probe")]
                    if std::env::var("NOTDISCORD_MEDIA_PROBE").as_deref() == Ok("OWNED_SCREEN") {
                        let list = ctx.get::<JsObject>(0).ok();
                        let length = list.as_ref().and_then(|list| list.get_array_length().ok());
                        eprintln!("rust-preview:result:arguments={}:length={length:?}", ctx.length);
                        if let Some(item) = list.and_then(|list| list.get_element::<JsObject>(0).ok()) {
                            let id = item.get_named_property::<napi::JsString>("id").ok()
                                .and_then(|value| value.into_utf8().ok())
                                .and_then(|value| value.as_str().ok().map(str::to_owned));
                            let url_type = item.get_named_property::<JsUnknown>("url").ok()
                                .and_then(|value| value.get_type().ok());
                            let url = item.get_named_property::<napi::JsString>("url").ok()
                                .and_then(|value| value.into_utf8().ok())
                                .and_then(|value| value.as_str().ok().map(str::to_owned));
                            eprintln!("rust-preview:item:id={id:?}:urltype={url_type:?}:urllen={:?}:embedded={}",
                                url.as_ref().map(String::len),url.as_ref().is_some_and(|value|value.starts_with("data:image/")));
                            if let Some((mime,data))=url.as_ref().and_then(|url|url.split_once(',')) {
                                eprintln!("rust-preview:mime={:?}",mime.chars().take(100).collect::<String>());
                                if data.len()<=2*1024*1024 {
                                    if let Ok(bytes)=base64::engine::general_purpose::STANDARD.decode(data) {
                                        let format=image::guess_format(&bytes).ok();
                                        let dimensions=if bytes.starts_with(b"BM")&&bytes.len()>=26 {
                                            Some((i32::from_le_bytes(bytes[18..22].try_into().unwrap()) as i64,i32::from_le_bytes(bytes[22..26].try_into().unwrap()) as i64))
                                        }else{
                                            image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format().ok().and_then(|reader|reader.into_dimensions().ok()).map(|(width,height)|(width as i64,height as i64))
                                        };
                                        eprintln!("rust-preview:encoded:format={format:?}:dimensions={dimensions:?}");
                                    }
                                }
                            }
                        }
                    }
                    let pixels = if alive.load(Ordering::Acquire) && pipeline.borrow().can_deliver(query, Instant::now()) {
                        ctx.get::<JsObject>(0)
                            .ok()
                            .and_then(|list| selected_url(*ctx.env, list, &source.source_id))
                            .and_then(|url| preview_pixels(&url))
                    } else {
                        None
                    };
                    let completion = pipeline.borrow_mut().complete(query, Instant::now(), pixels.is_some());
                    if let Some(completion) = completion {
                        if completion.deliver && alive.load(Ordering::Acquire) && let Some((width, height, rgba)) = pixels {
                            output.publish(VideoFrame {
                                subscription_id: source.id,
                                sequence: source.sequence.fetch_add(1, Ordering::Relaxed) + 1,
                                width,
                                height,
                                rgba,
                            });
                        }
                        let _ = sender.call(
                            Event::PreviewResult { token: completion.token, source: result_source.clone(), ok: completion.deliver },
                            ThreadsafeFunctionCallMode::NonBlocking,
                        );
                    }
                    ctx.env.get_undefined()
                });
        #[cfg(feature = "native-probe")]
        trace("preview-callback-after");
        let capture_invoked = std::cell::Cell::new(false);
        let result = callback
            .map_err(|_| MediaError::internal())
            .and_then(|callback| {
                let engine = self.engine()?;
                #[cfg(feature = "native-probe")]
                trace("preview-engine-ready");
                // The shipped media wrapper configures this before every
                // desktop preview query. Use the regular window path here;
                // selecting a preview never enables outgoing capture.
                native::optional_json(self.env, &engine, "setPreviewsUseWgc", vec![json!(false)])?;
                #[cfg(feature = "native-probe")]
                if std::env::var("NOTDISCORD_MEDIA_PROBE").as_deref() == Ok("OWNED_SCREEN")
                    && (crate::probe_window::source().as_deref() != Some(source_id.as_str())
                        || !engine
                            .has_named_property("getSingleWindowPreview")
                            .unwrap_or(false))
                {
                    return Err(MediaError::new(
                        "OWNED_PREVIEW_ONLY",
                        "Only this process's single window may be captured by the probe.",
                    ));
                }
                if source_id.starts_with("window:")
                    && engine
                        .has_named_property("getSingleWindowPreview")
                        .unwrap_or(false)
                {
                    // The shipped engine exposes this as a Promise of source[];
                    // only the explicitly selected window is captured here.
                    let handle = source_id.split(':').nth(1).unwrap_or("");
                    let args = [self.json(json!(handle))?, self.json(json!(640))?, self.json(json!(360))?];
                    capture_invoked.set(true);
                    let promise = native::call(
                        self.env,
                        &engine,
                        "getSingleWindowPreview",
                        &args,
                    )?;
                    let promise = promise
                        .coerce_to_object()
                        .map_err(|_| MediaError::internal())?;
                    let sender = self.sender.clone();
                    let failed_source = source_id.clone();
                    let failure_pipeline = self.video.preview_pipeline.clone();
                    let failure = self
                        .env
                        .create_function_from_closure("selectedPreviewRejected", move |ctx| {
                            #[cfg(feature = "native-probe")]
                            if std::env::var("NOTDISCORD_MEDIA_PROBE").as_deref()
                                == Ok("OWNED_SCREEN")
                            {
                                let reason = ctx
                                    .get::<JsUnknown>(0)
                                    .ok()
                                    .and_then(|value| value.coerce_to_string().ok())
                                    .and_then(|value| value.into_utf8().ok())
                                    .and_then(|value| {
                                        value.as_str().ok().map(|value| {
                                            value.chars().take(240).collect::<String>()
                                        })
                                    });
                                eprintln!("rust-preview:rejected:reason={reason:?}");
                            }
                            if let Some(completion) = failure_pipeline.borrow_mut().complete(query, Instant::now(), false) {
                                let _ = sender.call(
                                    Event::PreviewResult { token: completion.token, source: failed_source.clone(), ok: false },
                                    ThreadsafeFunctionCallMode::NonBlocking,
                                );
                            }
                            ctx.env.get_undefined()
                        })
                        .map_err(|_| MediaError::internal())?;
                    native::call(
                        self.env,
                        &promise,
                        "then",
                        &[callback.into_unknown(), failure.into_unknown()],
                    )
                    .map(|_| ())
                } else {
                    let method = if source_id.starts_with("window:") {
                        "getWindowPreviews"
                    } else {
                        "getScreenPreviews"
                    };
                    let args = [self.json(json!(640))?, self.json(json!(360))?, callback.into_unknown()];
                    capture_invoked.set(true);
                    native::call(
                        self.env,
                        &engine,
                        method,
                        &args,
                    )
                    .map(|_| ())
                }
            });
        if result.is_err() {
            let failed = if capture_invoked.get() {
                self.video.preview_pipeline.borrow_mut().fail_unknown(query).is_some()
            } else {
                self.video.preview_pipeline.borrow_mut().complete(query, Instant::now(), false).is_some()
            };
            if failed { self.preview_result(token, &source_id, false); }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn query(pipeline: &mut Pipeline, now: Instant) -> (u64, u64) {
        match pipeline.tick(now) { PreviewAction::Query { id, token } => (id, token), other => panic!("Expected query, got {other:?}") }
    }
    #[test]
    fn hung_preview_times_out_once_and_replacement_waits_for_exact_old_completion() {
        let now = Instant::now(); let mut pipeline = Pipeline::default();
        pipeline.select(1, now); let (old, _) = query(&mut pipeline, now);
        assert_eq!(pipeline.tick(now + Duration::from_millis(4999)), PreviewAction::Idle);
        assert_eq!(pipeline.tick(now + Duration::from_secs(5)), PreviewAction::Expired { token: 1 });
        for elapsed in [6, 10, 60, 600] { assert_eq!(pipeline.tick(now + Duration::from_secs(elapsed)), PreviewAction::Idle); }
        pipeline.select(2, now + Duration::from_secs(600));
        assert_eq!(pipeline.tick(now + Duration::from_secs(600)), PreviewAction::Idle);
        assert!(pipeline.complete(old + 1, now + Duration::from_secs(601), true).is_none());
        assert!(!pipeline.can_deliver(old, now + Duration::from_secs(601)));
        assert!(!pipeline.complete(old, now + Duration::from_secs(601), true).unwrap().deliver);
        let (second, token) = query(&mut pipeline, now + Duration::from_secs(601)); assert_eq!(token, 2);
        assert!(pipeline.complete(old, now + Duration::from_secs(601), true).is_none());
        pipeline.cancel(); pipeline.select(3, now + Duration::from_secs(601));
        assert_eq!(pipeline.tick(now + Duration::from_secs(601)), PreviewAction::Idle);
        assert!(!pipeline.complete(second, now + Duration::from_secs(601), true).unwrap().deliver);
        let (current, token) = query(&mut pipeline, now + Duration::from_secs(601)); assert_eq!(token, 3);
        assert!(pipeline.complete(current, now + Duration::from_secs(601), true).unwrap().deliver);
        assert_eq!(pipeline.tick(now + Duration::from_secs(601) + Duration::from_millis(749)), PreviewAction::Idle);
        assert!(matches!(pipeline.tick(now + Duration::from_secs(601) + Duration::from_millis(750)), PreviewAction::Query { token: 3, .. }));
    }
    #[test]
    fn late_completion_without_a_tick_is_terminal_but_settled_empty_frames_keep_normal_refresh() {
        let now = Instant::now(); let mut pipeline = Pipeline::default(); pipeline.select(1, now);
        let (id, _) = query(&mut pipeline, now);
        assert!(!pipeline.complete(id, now + Duration::from_secs(5), true).unwrap().deliver);
        assert!(pipeline.failed(1)); assert_eq!(pipeline.tick(now + Duration::from_secs(60)), PreviewAction::Idle);
        pipeline.select(2, now + Duration::from_secs(60)); let (id, _) = query(&mut pipeline, now + Duration::from_secs(60));
        assert!(!pipeline.complete(id, now + Duration::from_secs(60), false).unwrap().deliver);
        assert!(!pipeline.failed(2));
        assert_eq!(pipeline.tick(now + Duration::from_secs(60) + Duration::from_millis(749)), PreviewAction::Idle);
        let (id, _) = query(&mut pipeline, now + Duration::from_secs(60) + Duration::from_millis(750));
        pipeline.fail_unknown(id);
        assert_eq!(pipeline.tick(now + Duration::from_secs(90)), PreviewAction::Idle);
        assert!(!pipeline.complete(id, now + Duration::from_secs(90), true).unwrap().deliver);
        assert_eq!(pipeline.tick(now + Duration::from_secs(91)), PreviewAction::Idle);
    }
    #[test]
    fn selected_preview_accepts_only_bounded_embedded_images() {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 128])
            .unwrap()
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
        );
        let (width, height, rgba) = preview_pixels(&url).unwrap();
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba, [255, 0, 0, 255, 0, 255, 0, 128]);
        assert!(preview_pixels("https://example.invalid/private.png").is_none());
        assert!(preview_pixels("file:///private.png").is_none());
        assert!(preview_pixels("data:image/png;base64,not base64").is_none());
        assert!(
            preview_pixels(&format!(
                "data:image/png;base64,{}",
                "A".repeat(2 * 1024 * 1024 + 1)
            ))
            .is_none()
        );
    }
    #[test]
    fn selected_preview_decodes_native_bmp_and_rejects_excessive_dimensions() {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbImage::from_raw(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
            .unwrap()
            .write_to(&mut bytes, image::ImageFormat::Bmp)
            .unwrap();
        let mut bytes = bytes.into_inner();
        let make_url = |bytes: &[u8]| {
            format!(
                "data:image/bmp;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            )
        };
        let (width, height, rgba) = preview_pixels(&make_url(&bytes)).unwrap();
        assert_eq!((width, height), (2, 2));
        assert_eq!(
            rgba,
            [
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255
            ]
        );
        bytes[18..22].copy_from_slice(&i32::MAX.to_le_bytes());
        bytes[22..26].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(preview_pixels(&make_url(&bytes)).is_none());
    }
}
