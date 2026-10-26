//! Explicit, opt-in, prolonged exercise of production image and raw-video paths.
//! This module never requests a URL, initializes a media host, or opens a device.
use super::*;
use notdiscord_core::video::{LatestFrames, MAX_BUFFERED_BYTES, MAX_FRAME_BYTES, VideoFrame};
use serde_json::json;
use std::{fs, fs::File, io::BufWriter, os::windows::fs::MetadataExt};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    fn GetLastError() -> u32;
}
fn move_progress_file(source: &Path, destination: &Path) -> Result<(), u32> {
    use std::os::windows::ffi::OsStrExt;
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 1 | 8) } != 0 {
        Ok(())
    } else {
        // Capture immediately: cleanup and other filesystem operations can
        // overwrite the useful native error exposed by this exact operation.
        Err(unsafe { GetLastError() })
    }
}
fn publish_progress(path: &Path, value: &serde_json::Value) -> Result<usize, String> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<usize, String> {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("synthetic progress temporary: {error}"))?;
        serde_json::to_writer(&mut output, value)
            .map_err(|error| format!("synthetic progress JSON: {error}"))?;
        output
            .flush()
            .and_then(|_| output.sync_all())
            .map_err(|error| format!("synthetic progress flush: {error}"))?;
        drop(output);
        // A telemetry reader/AV can temporarily retain the destination even
        // when delete sharing was requested. Retry only this owned JSON's
        // atomic publication, never the workload, requests or measured cycle.
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut attempts = 0;
        loop {
            attempts += 1;
            match move_progress_file(&temporary, path) {
                Ok(()) => return Ok(attempts),
                Err(code) if matches!(code, 5 | 32 | 33) && Instant::now() < deadline => {
                    let pause = Duration::from_millis(5)
                        .min(deadline.saturating_duration_since(Instant::now()));
                    if attempts == 1 {
                        eprintln!(
                            "synthetic progress contention: MoveFileExW win32={code}; retry <=500ms"
                        );
                    }
                    std::thread::sleep(pause);
                }
                Err(code) => {
                    return Err(format!(
                        "synthetic progress MoveFileExW win32={code}, attempts={attempts}, boundedWaitMs=500"
                    ));
                }
            }
        }
    })();
    let _ = fs::remove_file(temporary);
    result
}

struct Progress {
    path: PathBuf,
    metrics_path: PathBuf,
    metrics: BufWriter<File>,
    start: Instant,
    cycles: u64,
    seconds: u64,
}
impl Progress {
    fn report(&mut self, phase: &str, completed: bool, data: serde_json::Value) {
        let row = json!({
            "version":1,"synthetic":true,"authenticated":false,"gatewayStarted":false,
            "mediaHostStarted":false,"devicesOpened":false,"externalHttpRequests":0,
            "elapsedMs":self.start.elapsed().as_millis() as u64,
            "durationSeconds":self.seconds,"completedCycles":self.cycles,
            "completed":completed,"phase":phase,"metrics":self.metrics_path,"data":data,
        });
        serde_json::to_writer(&mut self.metrics, &row).unwrap();
        self.metrics.write_all(b"\n").unwrap();
        self.metrics.flush().unwrap();
        publish_progress(&self.path, &row).expect("atomic synthetic progress");
    }
}
fn consume_texture_updates(ctx: &egui::Context) {
    // A renderer consumes these payloads every frame. Keeping every old texture
    // delta in this headless test would manufacture a retention absent in UI.
    drop(ctx.tex_manager().write().take_delta());
}
fn texture_bytes(ctx: &egui::Context) -> usize {
    ctx.tex_manager()
        .read()
        .allocated()
        .map(|(_, meta)| meta.bytes_used())
        .sum()
}
fn wait_texture(pictures: &mut Pictures, ctx: &egui::Context, path: &Path, side: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        pictures.poll(ctx);
        if let Some(texture) = pictures.texture_path(path, side) {
            assert!(texture.size()[0] <= side as usize && texture.size()[1] <= side as usize);
            drop(texture);
            consume_texture_updates(ctx);
            return;
        }
        assert!(
            pictures.path_error(path, side).is_none(),
            "synthetic PNG decode failed"
        );
        assert!(
            Instant::now() < deadline,
            "local decoder failed to make progress"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn png(width: u32, height: u32) -> Vec<u8> {
    let source = image::RgbaImage::from_pixel(width, height, image::Rgba([25, 50, 75, 128]));
    let mut output = Cursor::new(Vec::new());
    source
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}
fn animation() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut bytes, 120, 80, &[255, 0, 0, 0, 0, 255]).unwrap();
        encoder.set_repeat(gif::Repeat::Infinite).unwrap();
        for index in 0..24 {
            let frame = gif::Frame {
                width: 120,
                height: 80,
                delay: 4,
                buffer: std::borrow::Cow::Owned(vec![(index % 2) as u8; 120 * 80]),
                ..Default::default()
            };
            encoder.write_frame(&frame).unwrap();
        }
    }
    bytes
}
fn disk_usage(path: &Path) -> (u64, usize) {
    let mut bytes = 0;
    let mut files = 0;
    for item in fs::read_dir(path).unwrap() {
        let item = item.unwrap();
        let metadata = fs::symlink_metadata(item.path()).unwrap();
        assert_eq!(
            metadata.file_attributes() & 0x400,
            0,
            "test tree contains a reparse point"
        );
        if metadata.is_dir() {
            let (n, count) = disk_usage(&item.path());
            bytes += n;
            files += count;
        } else {
            bytes += metadata.len();
            files += 1;
        }
    }
    (bytes, files)
}

#[test]
#[ignore = "Prolonged offline native-memory exercise; explicit directory/duration and process monitor required"]
fn offline_multimedia_resource_soak() {
    let requested = PathBuf::from(
        std::env::var_os("NOTDISCORD_P2_SOAK_DIR").expect("explicit E: test directory"),
    );
    let root = fs::canonicalize(&requested).expect("existing test directory");
    let project = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert!(
        root.starts_with(&project),
        "test artifacts must stay inside this project on E:"
    );
    assert!(
        root.to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(r"\\?\e:\"),
        "explicit E: path required"
    );
    assert_eq!(
        fs::symlink_metadata(&requested).unwrap().file_attributes() & 0x400,
        0
    );
    let progress_path = PathBuf::from(
        std::env::var_os("NOTDISCORD_P2_SOAK_PROGRESS").expect("explicit progress file"),
    );
    assert_eq!(
        fs::canonicalize(progress_path.parent().unwrap()).unwrap(),
        root
    );
    let seconds: u64 = std::env::var("NOTDISCORD_P2_SOAK_SECONDS")
        .unwrap_or_else(|_| "900".into())
        .parse()
        .unwrap();
    assert!(
        (10..=3600).contains(&seconds),
        "duration must be 10..3600 seconds"
    );
    let metrics_path = root.join("multimedia-metrics.jsonl");
    let metrics = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&metrics_path)
            .unwrap(),
    );
    let mut progress = Progress {
        path: progress_path,
        metrics_path,
        metrics,
        start: Instant::now(),
        cycles: 0,
        seconds,
    };
    progress.report("initializing",false,json!({"textureBudget":BUDGET,"videoBufferBudget":MAX_BUFFERED_BYTES,"avatarDiskBudget":32*1024*1024}));

    // Everything the harness reads or writes below was created by this run.
    let payload = root.join(format!(
        "synthetic-payload-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&payload).unwrap();
    let empty_data = payload.join("empty-ui-data");
    fs::create_dir(&empty_data).unwrap();
    let large_png = png(1600, 1200);
    let large_paths: Vec<_> = (0..3)
        .map(|index| {
            let path = payload.join(format!("large-{index}.png"));
            fs::write(&path, &large_png).unwrap();
            path
        })
        .collect();
    drop(large_png);
    let medium_png = png(800, 600);
    let medium_paths: Vec<_> = (0..8)
        .map(|index| {
            let path = payload.join(format!("medium-{index}.png"));
            fs::write(&path, &medium_png).unwrap();
            path
        })
        .collect();
    let bad = payload.join("malformed.png");
    fs::write(&bad, b"synthetic, deliberately not an image").unwrap();
    let gif = animation();
    let avatar_base = payload.join("avatar-cache");
    fs::create_dir(&avatar_base).unwrap();
    let cache = AvatarCache::open(&avatar_base, "synthetic-offline-account").unwrap();
    let padded = payload.join("padded-avatar.png");
    {
        let mut bytes = medium_png.clone();
        bytes.resize(3 * 1024 * 1024, 0);
        fs::write(&padded, bytes).unwrap();
    }
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::CACHE_CONTROL,
        "public, max-age=3600".parse().unwrap(),
    );
    // Exceed the encoded cache quota once, then replace entries throughout the
    // soak. URLs below are inert cache keys; no HTTP function is ever called.
    for index in 0..12 {
        let key = format!("https://cdn.discordapp.com/avatars/42/synthetic-{index}.png?size=256");
        assert!(cache.store(&key, &headers, &padded, unix_now()).unwrap());
    }
    assert!(disk_usage(&avatar_base).0 <= 32 * 1024 * 1024);
    let ctx = egui::Context::default();
    let (commands, _command_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (_updates, updates_receiver) = mpsc::channel();
    let mut app =
        super::super::NotDiscordUi::from_context(&ctx, empty_data, commands, updates_receiver);
    let video = Arc::new(Mutex::new(LatestFrames::default()));
    app.video_frames = Some(video.clone());
    // The actual GIF decoder and publication path are used, while the download
    // worker remains idle. This is the only test substitution in Pictures.
    let (animation_send, animation_receive) = mpsc::channel();
    app.pictures.animation_ready = animation_receive;
    consume_texture_updates(&ctx);
    let retained_baseline = texture_bytes(&ctx);
    progress.report(
        "ready",
        false,
        json!({"retainedTextureBaseline":retained_baseline}),
    );

    while progress.start.elapsed() < Duration::from_secs(seconds) || progress.cycles < 3 {
        progress.report("image-backpressure", false, json!({}));
        for path in &large_paths {
            assert!(app.pictures.texture_path(path, 1600).is_none());
        }
        let held = app
            .pictures
            .ready
            .recv_timeout(Duration::from_secs(10))
            .expect("first WIC image");
        assert_eq!(held.2.as_ref().unwrap().2.len(), 1600 * 1200 * 4);
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.pictures.decode_gate.waiting() < 2 {
            assert!(
                Instant::now() < deadline,
                "all other image workers must wait before decoding"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            app.pictures.ready.try_recv().is_err(),
            "no additional decoded bitmap can accumulate"
        );
        // Hold the completed image while more UI requests arrive, then cancel
        // that entire generation. This exercises the real bounded work queue.
        for index in 0..96 {
            app.pictures.texture_source(
                format!("queued-{index}"),
                PictureSource::Local(medium_paths[0].clone()),
                384,
            );
        }
        assert!(app.pictures.pending.len() <= 64);
        std::thread::sleep(Duration::from_millis(120));
        app.pictures.clear_account();
        drop(held);
        assert_eq!(app.pictures.textures.capacity(), 0);
        assert_eq!(app.pictures.pending.capacity(), 0);
        assert_eq!(app.pictures.failed.capacity(), 0);
        assert_eq!(app.pictures.local_sources.capacity(), 0);
        assert_eq!(app.pictures.avatar_sources.capacity(), 0);

        progress.report("wic-texture-cache", false, json!({}));
        for path in &medium_paths {
            wait_texture(&mut app.pictures, &ctx, path, 768);
            assert!(app.pictures.used <= BUDGET);
            assert!(app.pictures.textures.len() <= MAX_TEXTURES);
        }
        assert!(
            app.pictures.textures.len() < medium_paths.len(),
            "byte quota must evict older image textures"
        );
        wait_texture(&mut app.pictures, &ctx, &large_paths[0], 1600);
        assert!(app.pictures.large.is_some());
        app.pictures.texture_path(&bad, 128);
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.pictures.path_error(&bad, 128).is_none() {
            app.pictures.poll(&ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let spool = EncodedSpool::copy_from(
            Cursor::new(&medium_png),
            Some(medium_png.len() as u64),
            MAX_ENCODED,
            &payload,
        )
        .unwrap();
        let spool_path = spool.path().to_owned();
        assert_eq!(decode_file(spool.path(), 128).unwrap().0, 128);
        drop(spool);
        assert!(!spool_path.exists());
        assert!(
            EncodedSpool::copy_cancellable(
                Cursor::new(&medium_png),
                None,
                MAX_ENCODED,
                &payload,
                || true
            )
            .is_err()
        );
        assert!(
            EncodedSpool::copy_from(
                Cursor::new([]),
                Some(MAX_ENCODED + 1),
                MAX_ENCODED,
                &payload
            )
            .is_err()
        );

        progress.report("gif-and-avatar-cache", false, json!({}));
        assert!(decode_animation_bytes(&gif, || true).is_empty());
        let decoded = decode_animation_bytes(&gif, || false);
        assert_eq!(decoded.len(), 24);
        let gif_bytes = decoded.iter().map(|frame| frame.2.len()).sum::<usize>();
        assert!(gif_bytes <= BUDGET);
        let generation = app.pictures.animation_generation.load(Ordering::Acquire);
        app.pictures.animation_pending = Some((generation, "synthetic-gif".into()));
        animation_send
            .send((generation, "synthetic-gif".into(), Ok(decoded)))
            .unwrap();
        app.pictures.poll(&ctx);
        assert_eq!(app.pictures.animation.as_ref().unwrap().frames.len(), 24);
        app.pictures.begin_frame();
        assert!(app.pictures.animated(&ctx, "synthetic-gif").is_some());
        app.pictures.finish_frame(false);
        consume_texture_updates(&ctx);
        let cache_key = format!(
            "https://cdn.discordapp.com/avatars/42/rotation-{}.png?size=256",
            progress.cycles
        );
        assert!(
            cache
                .store(&cache_key, &headers, &padded, unix_now())
                .unwrap()
        );
        let cached = cache
            .lookup(&cache_key, unix_now())
            .expect("fresh synthetic avatar");
        assert_eq!(cached.len(), 3 * 1024 * 1024);
        let decoded = decode_prepared(
            PreparedImage::Cached(cached, cache_key.clone()),
            64,
            Some(&cache),
        )
        .unwrap();
        assert_eq!((decoded.0, decoded.1), (64, 48));
        drop(decoded);
        let (cache_bytes, cache_files) = disk_usage(&avatar_base);
        assert!(cache_bytes <= 32 * 1024 * 1024);
        assert!(
            cache.lookup(&cache_key, unix_now() + 3601).is_none(),
            "expired avatar must not be reused"
        );

        progress.report("video-latest-frames", false, json!({}));
        let mut texture_ids = HashMap::new();
        for sequence in 1..=3 {
            let mut store = video.lock().unwrap();
            for id in 1..=4 {
                assert!(store.activate(id));
            }
            assert!(!store.activate(5));
            for id in 1..=4 {
                let (width, height) = if id <= 2 { (1280, 720) } else { (640, 360) };
                let frame = VideoFrame {
                    subscription_id: id,
                    sequence,
                    width,
                    height,
                    rgba: vec![sequence as u8; width as usize * height as usize * 4],
                };
                assert!(frame.rgba.len() <= MAX_FRAME_BYTES);
                let accepted = store.publish(frame);
                assert_eq!(
                    accepted,
                    id <= 3,
                    "total byte quota must refuse the fourth raw frame"
                );
            }
            assert!(store.buffered_bytes() <= MAX_BUFFERED_BYTES);
            drop(store);
            app.poll_video_frames(&ctx);
            assert_eq!(video.lock().unwrap().buffered_bytes(), 0);
            assert_eq!(app.video_textures.len(), 3);
            for (&id, texture) in &app.video_textures {
                if sequence == 1 {
                    texture_ids.insert(id, texture.id());
                } else {
                    assert_eq!(texture_ids[&id], texture.id());
                }
            }
            consume_texture_updates(&ctx);
            std::thread::sleep(Duration::from_millis(80));
        }
        progress.report("loaded",false,json!({
            "imageTextureBytes":app.pictures.used,"imageTextures":app.pictures.textures.len(),
            "largeImage":app.pictures.large.is_some(),"gifDecodedBytes":gif_bytes,
            "videoTextures":app.video_textures.len(),"rawVideoBufferedBytes":video.lock().unwrap().buffered_bytes(),
            "retainedTextureBytes":texture_bytes(&ctx),"avatarDiskBytes":cache_bytes,"avatarFiles":cache_files,
        }));
        std::thread::sleep(Duration::from_millis(160));
        // Repeatedly release all UI owners, reject stale animation publication,
        // and prove that retained texture metadata returns to the same baseline.
        video.lock().unwrap().clear();
        app.poll_video_frames(&ctx);
        assert!(app.video_textures.is_empty());
        let old = app.pictures.animation_generation.load(Ordering::Acquire);
        app.pictures.clear_account();
        animation_send
            .send((
                old,
                "synthetic-gif".into(),
                Ok(vec![(1, 1, vec![0, 0, 0, 255], Duration::from_millis(40))]),
            ))
            .unwrap();
        app.pictures.poll(&ctx);
        assert!(app.pictures.animation.is_none());
        assert!(app.pictures.large.is_none());
        assert_eq!(app.pictures.used, 0);
        consume_texture_updates(&ctx);
        assert_eq!(
            texture_bytes(&ctx),
            retained_baseline,
            "image/video textures retained after release"
        );
        assert_eq!(video.lock().unwrap().buffered_bytes(), 0);
        progress.cycles += 1;
        progress.report("released",false,json!({"retainedTextureBytes":texture_bytes(&ctx),"imageTextureBytes":0,"rawVideoBufferedBytes":0}));
        std::thread::sleep(Duration::from_millis(500));
    }
    drop(app);
    drop(cache);
    drop(animation_send);
    consume_texture_updates(&ctx);
    assert_eq!(
        fs::canonicalize(&payload).unwrap().parent(),
        Some(root.as_path())
    );
    disk_usage(&payload); // Refuse a reparse point anywhere before cleanup.
    fs::remove_dir_all(&payload).unwrap();
    progress.report(
        "completed",
        true,
        json!({"allCyclesReleased":true,"syntheticPayloadRemoved":true}),
    );
}

fn emoji_frame(
    app: &mut super::super::NotDiscordUi,
    ctx: &egui::Context,
    value: Option<(&str, f32)>,
) {
    let state = notdiscord_core::models::AppSnapshot::welcome();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 800.),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some((text, size)) = value {
                    app.emoji_button(ui, text, size);
                }
            });
            app.draw_picker(ctx, &state);
        },
    );
    // Real GUI backends upload these once and discard the CPU payload. This
    // harness exercises the same creation/replacement/free protocol headlessly.
    drop(output);
    consume_texture_updates(ctx);
}
fn drain_emojis(
    app: &mut super::super::NotDiscordUi,
    ctx: &egui::Context,
    progress: &mut Progress,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut last_report = Instant::now();
    loop {
        app.process_updates(ctx);
        consume_texture_updates(ctx);
        assert!(
            app.emoji_pending.len() <= 67,
            "one worker + two completed + sixty-four queued requests"
        );
        assert!(app.emoji_weight <= 2 * 1024 * 1024);
        assert!(app.emoji_failed.len() <= 256);
        if app.emoji_pending.is_empty() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "DirectWrite emoji worker stalled"
        );
        if last_report.elapsed() >= Duration::from_secs(1) {
            progress.report(
                "emoji-draining",
                false,
                json!({"pending":app.emoji_pending.len(),"emojiTextureBytes":app.emoji_weight}),
            );
            last_report = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn emoji_metrics(app: &super::super::NotDiscordUi, ctx: &egui::Context) -> serde_json::Value {
    json!({
        "emojiTextureBytes":app.emoji_weight,"emojiTextures":app.emoji_textures.len(),
        "pending":app.emoji_pending.len(),"failed":app.emoji_failed.len(),
        "catalogEntries":app.emojis.len(),"catalogCapacity":app.emojis.capacity(),
        "catalogStringBytes":0,"catalogMetadataCapacityBytes":app.emojis.capacity()*std::mem::size_of::<(&str,&str,&str)>(),
        "retainedTextureBytes":texture_bytes(ctx),
    })
}

fn install_emoji_panic_evidence(root: &Path) {
    // This opt-in ignored test runs alone in its own process. Preserve stderr
    // and retain the panic even when a monitor does not capture console output.
    // The explicit E: root was validated before installing this hook.
    let destination = root.join("panic.txt");
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |information| {
        let detail = format!("{information}\n");
        let mut end = detail.len().min(16 * 1024);
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        let _ = fs::write(&destination, &detail[..end]);
        previous(information);
    }));
}

#[test]
fn progress_atomic_replace_reports_native_reader_sharing_behaviour() {
    use std::os::windows::fs::OpenOptionsExt;
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence");
    let root = evidence.join(format!(
        "p2-progress-sharing-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let path = root.join("progress.json");
    fs::write(&path, b"before").unwrap();
    let temporary = root.join("native-replacement.tmp");
    fs::write(&temporary, b"after").unwrap();
    let deny_delete = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&path)
        .unwrap();
    let denied = move_progress_file(&temporary, &path);
    println!("MoveFileExW denied-delete reader: {denied:?}");
    assert!(
        denied.is_err(),
        "a reader denying delete must block replacement"
    );
    assert_eq!(fs::read(&path).unwrap(), b"before");
    drop(deny_delete);
    let mut share_delete = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2 | 4)
        .open(&path)
        .unwrap();
    let shared = move_progress_file(&temporary, &path);
    println!("MoveFileExW read/write/delete-sharing reader: {shared:?}");
    let mut previous = Vec::new();
    share_delete.read_to_end(&mut previous).unwrap();
    assert_eq!(
        previous, b"before",
        "the reader keeps its immutable old file handle"
    );
    drop(share_delete);
    if shared.is_err() {
        assert_eq!(fs::read(&path).unwrap(), b"before");
        move_progress_file(&temporary, &path).expect("replacement after closing every reader");
    }
    assert_eq!(fs::read(&path).unwrap(), b"after");
    fs::remove_file(path).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn progress_retry_publishes_complete_json_and_bounds_a_persistent_reader_stall() {
    use std::os::windows::fs::OpenOptionsExt;
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evidence");
    let root = evidence.join(format!(
        "p2-progress-retry-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let path = root.join("progress.json");
    fs::write(&path, br#"{"before":true}"#).unwrap();
    let reader = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&path)
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        drop(reader);
    });
    let expected = json!({"completedCycles":42,"synthetic":true,"authenticated":false});
    publish_progress(&path, &expected).expect("publication after the temporary reader releases");
    release.join().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
        expected
    );
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&path)
        .unwrap();
    let started = Instant::now();
    let failure = publish_progress(&path, &json!({"mustNotAppear":true}))
        .expect_err("persistent contention must stop");
    let elapsed = started.elapsed();
    println!("bounded progress failure: {failure}; elapsed={elapsed:?}");
    assert!(elapsed >= Duration::from_millis(450));
    assert!(
        elapsed < Duration::from_secs(2),
        "500ms retry loop must remain bounded with filesystem overhead"
    );
    assert!(failure.contains("MoveFileExW win32="));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
        expected
    );
    drop(held);
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "failed publication removes only its temporary"
    );
    fs::remove_file(path).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
#[ignore = "Prolonged Windows DirectWrite/UI emoji exercise; explicit E: directory and process monitor required"]
fn offline_windows_emoji_soak() {
    let requested = PathBuf::from(
        std::env::var_os("NOTDISCORD_P2_SOAK_DIR").expect("explicit E: test directory"),
    );
    let root = fs::canonicalize(&requested).expect("existing test directory");
    let project = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert!(root.starts_with(&project));
    assert!(
        root.to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(r"\\?\e:\")
    );
    assert_eq!(
        fs::symlink_metadata(&requested).unwrap().file_attributes() & 0x400,
        0
    );
    let path = PathBuf::from(
        std::env::var_os("NOTDISCORD_P2_SOAK_PROGRESS").expect("explicit progress file"),
    );
    assert_eq!(fs::canonicalize(path.parent().unwrap()).unwrap(), root);
    install_emoji_panic_evidence(&root);
    // Diagnostic replay chooses the repertoire immediately without claiming
    // that earlier cycles have completed in this fresh process.
    let rotation_offset: u64 = std::env::var("NOTDISCORD_P2_EMOJI_START_CYCLE")
        .unwrap_or_else(|_| "0".into())
        .parse()
        .expect("nonnegative replay cycle");
    assert!(rotation_offset <= 100_000);
    let seconds: u64 = std::env::var("NOTDISCORD_P2_SOAK_SECONDS")
        .unwrap_or_else(|_| "900".into())
        .parse()
        .unwrap();
    assert!((10..=3600).contains(&seconds));
    let metrics_path = root.join("emoji-metrics.jsonl");
    let metrics = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&metrics_path)
            .unwrap(),
    );
    let mut progress = Progress {
        path,
        metrics_path,
        metrics,
        start: Instant::now(),
        cycles: 0,
        seconds,
    };
    progress.report("emoji-initializing",false,json!({"emojiTextureBudget":2*1024*1024,"pendingLimit":67,"renderer":"Windows DirectWrite/D2D/WIC software"}));
    let payload = root.join(format!(
        "synthetic-emoji-payload-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&payload).unwrap();
    let ctx = egui::Context::default();
    let (command, _command_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (_updates, receiver) = mpsc::channel();
    let mut app =
        super::super::NotDiscordUi::from_context(&ctx, payload.clone(), command, receiver);
    consume_texture_updates(&ctx);
    assert_eq!(app.emojis.capacity(), 0);
    assert_eq!(app.emoji_weight, 0);
    progress.report("emoji-cold", false, emoji_metrics(&app, &ctx));
    if rotation_offset != 0 {
        progress.report(
            "emoji-diagnostic-replay",
            false,
            json!({"catalogRotationOffset":rotation_offset}),
        );
    }

    // Include ZWJ families/professions, VS16, flags, modifiers and a changing
    // Unicode sample, instead of repeatedly rasterizing only one happy face.
    let fixed = [
        "😀",
        "🤣",
        "🙂",
        "❤️",
        "👍",
        "👍🏻",
        "👍🏽",
        "👍🏿",
        "👩‍💻",
        "👨🏾‍🚀",
        "👩🏻‍⚕️",
        "👨‍👩‍👧‍👦",
        "🏳️‍🌈",
        "🏳️‍⚧️",
        "🇵🇪",
        "🇯🇵",
        "🏴‍☠️",
        "🫶🏽",
        "🐈‍⬛",
        "🐦‍🔥",
        "🧑‍🦽",
        "🧑🏽‍🤝‍🧑🏻",
        "👩‍❤️‍💋‍👩",
        "1️⃣",
    ];
    while progress.start.elapsed() < Duration::from_secs(seconds) || progress.cycles < 3 {
        progress.report(
            if progress.cycles == 0 {
                "emoji-cold-picker-open"
            } else {
                "emoji-warm-picker-open"
            },
            false,
            emoji_metrics(&app, &ctx),
        );
        app.open_picker(false, &notdiscord_core::models::AppSnapshot::welcome());
        emoji_frame(&mut app, &ctx, None);
        assert!(app.emojis.len() > 1000);
        let catalog_pointer = app.emojis.as_ptr();
        emoji_frame(&mut app, &ctx, None);
        assert_eq!(
            app.emojis.as_ptr(),
            catalog_pointer,
            "warm repaint must reuse the catalog"
        );
        let count = app.emojis.len();
        let mut repertoire: Vec<String> = fixed.iter().map(|s| (*s).to_owned()).collect();
        repertoire.extend((0..12).map(|i| {
            app.emojis[((((progress.cycles + rotation_offset) as usize) * 12 + i) * 37) % count]
                .0
                .to_owned()
        }));
        progress.report("emoji-catalog-loaded", false, emoji_metrics(&app, &ctx));
        app.picker = None;
        emoji_frame(&mut app, &ctx, None);
        assert_eq!(
            app.emojis.capacity(),
            0,
            "closing Unicode picker must release catalog storage"
        );
        drain_emojis(&mut app, &ctx, &mut progress);
        for size in [16., 32., 64., 192.] {
            for (index, text) in repertoire.iter().enumerate() {
                emoji_frame(&mut app, &ctx, Some((text, size)));
                app.process_updates(&ctx);
                consume_texture_updates(&ctx);
                assert!(app.emoji_weight <= 2 * 1024 * 1024);
                assert!(app.emoji_pending.len() <= 67);
                if index % 12 == 0 {
                    progress.report("emoji-rasterizing",false,json!({"physicalSize":size,"repertoire":repertoire.len(),"pending":app.emoji_pending.len(),"emojiTextureBytes":app.emoji_weight,"repertoireIndex":index,"catalogRotationOffset":rotation_offset}));
                }
                if app.emoji_pending.len() >= 60 {
                    drain_emojis(&mut app, &ctx, &mut progress);
                }
            }
            drain_emojis(&mut app, &ctx, &mut progress);
        }
        // Isolate cache hits after broad churn: exactly the same Windows glyph
        // and physical size must keep its texture and enqueue no new work.
        emoji_frame(&mut app, &ctx, Some(("😀", 32.)));
        drain_emojis(&mut app, &ctx, &mut progress);
        let key = ("😀".to_owned(), 32);
        let texture_id = app
            .emoji_textures
            .get(&key)
            .expect("Windows emoji rasterization")
            .id();
        for _ in 0..8 {
            emoji_frame(&mut app, &ctx, Some(("😀", 32.)));
            assert_eq!(app.emoji_textures[&key].id(), texture_id);
            assert!(app.emoji_pending.is_empty());
        }
        progress.report("emoji-reuse", false, emoji_metrics(&app, &ctx));
        assert_eq!(app.emojis.capacity(), 0);
        assert!(app.emoji_weight <= 2 * 1024 * 1024);
        progress.cycles += 1;
        progress.report("emoji-cycle-completed", false, emoji_metrics(&app, &ctx));
        std::thread::sleep(Duration::from_millis(500));
    }
    let emoji_ids: Vec<_> = app
        .emoji_textures
        .values()
        .map(|texture| texture.id())
        .collect();
    drop(app);
    consume_texture_updates(&ctx);
    for id in emoji_ids {
        assert!(
            ctx.tex_manager().read().meta(id).is_none(),
            "dropping UI must free emoji textures"
        );
    }
    assert_eq!(
        fs::canonicalize(&payload).unwrap().parent(),
        Some(root.as_path())
    );
    disk_usage(&payload);
    fs::remove_dir_all(&payload).unwrap();
    progress.report(
        "completed",
        true,
        json!({"allEmojiTextureOwnersReleased":true,"syntheticPayloadRemoved":true}),
    );
}
