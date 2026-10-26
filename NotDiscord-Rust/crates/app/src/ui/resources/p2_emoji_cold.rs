//! Initial emoji attribution in a fresh process, not a cold Windows system.
//! Never starts an account, Gateway, media host, capture device or HTTP request.
use super::*;
use serde::Serialize;
use serde_json::json;
use std::{fs, fs::File, io::BufWriter, os::windows::fs::MetadataExt};
use windows::Win32::System::{
    ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX},
    Threading::GetCurrentProcess,
};

type Ui = super::super::NotDiscordUi;
const PHASES: [&str; 10] = [
    "process-before-ui",
    "ui-fonts-workers-no-emoji-frame",
    "catalog-parse-only",
    "catalog-released-before-native",
    "first-native-emoji-worker-only",
    "first-unicode-picker-render",
    "unicode-picker-warm-reuse",
    "unicode-picker-closed",
    "ui-dropped-context-alive",
    "context-dropped",
];
#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct State {
    catalog_entries: usize,
    catalog_capacity: usize,
    catalog_string_capacity_bytes: usize,
    catalog_metadata_capacity_bytes: usize,
    emoji_texture_bytes: usize,
    emoji_textures: usize,
    emoji_pending: usize,
    emoji_failures: usize,
    context_texture_bytes: usize,
    context_textures: usize,
    configured_font_files: usize,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct Sample {
    phase: usize,
    sample: usize,
    elapsed_ms: u64,
    private_usage_bytes: usize,
    working_set_bytes: usize,
    peak_working_set_bytes: usize,
    page_faults: u32,
    state: State,
}
struct Probe {
    path: PathBuf,
    samples_path: PathBuf,
    output: BufWriter<File>,
    samples: Vec<Sample>,
    start: Instant,
}
impl Probe {
    fn progress(&self, phase: usize, completed: bool) {
        crate::storage::save_json(
            &self.path,
            &json!({
                "version":1,"synthetic":true,"authenticated":false,"gatewayStarted":false,
                "externalHttpRequests":0,"mediaHostStarted":false,"devicesOpened":false,
                "freshProcess":true,"windowsSystemCold":false,"completedCycles":phase,
                "completed":completed,"phase":PHASES[phase.min(PHASES.len()-1)],
                "elapsedMs":self.start.elapsed().as_millis() as u64,"metrics":self.samples_path,
            }),
        )
        .unwrap();
    }
    fn phase(&mut self, phase: usize, state: State) {
        self.progress(phase, false);
        // Stable fixed sample storage was reserved before the first process
        // sample. Native counters include the small harness itself in every phase.
        for index in 0..10 {
            std::thread::sleep(Duration::from_millis(300));
            let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
            memory.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            unsafe {
                GetProcessMemoryInfo(
                    GetCurrentProcess(),
                    (&mut memory as *mut PROCESS_MEMORY_COUNTERS_EX)
                        .cast::<PROCESS_MEMORY_COUNTERS>(),
                    std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                )
                .expect("own Windows process memory counters");
            }
            let row = Sample {
                phase,
                sample: index,
                elapsed_ms: self.start.elapsed().as_millis() as u64,
                private_usage_bytes: memory.PrivateUsage,
                working_set_bytes: memory.WorkingSetSize,
                peak_working_set_bytes: memory.PeakWorkingSetSize,
                page_faults: memory.PageFaultCount,
                state,
            };
            self.samples.push(row);
            serde_json::to_writer(&mut self.output, &row).unwrap();
            self.output.write_all(b"\n").unwrap();
            self.output.flush().unwrap();
            if index % 3 == 0 {
                self.progress(phase, false);
            }
        }
    }
}
fn state(app: Option<&Ui>, ctx: Option<&egui::Context>) -> State {
    let mut result = State::default();
    if let Some(app) = app {
        result.catalog_entries = app.emojis.len();
        result.catalog_capacity = app.emojis.capacity();
        // Entries borrow the binary's static TSV; only their vector is owned.
        result.catalog_string_capacity_bytes = 0;
        result.catalog_metadata_capacity_bytes = app.emojis.capacity()
            * std::mem::size_of::<(&str, &str, &str)>();
        result.emoji_texture_bytes = app.emoji_weight;
        result.emoji_textures = app.emoji_textures.len();
        result.emoji_pending = app.emoji_pending.len();
        result.emoji_failures = app.emoji_failed.len();
        result.configured_font_files = app.fonts.font_data.len();
    }
    if let Some(ctx) = ctx {
        let textures = ctx.tex_manager();
        let manager = textures.read();
        result.context_texture_bytes = manager.allocated().map(|(_, meta)| meta.bytes_used()).sum();
        result.context_textures = manager.num_allocated();
    }
    result
}
fn consume_updates(ctx: &egui::Context) {
    drop(ctx.tex_manager().write().take_delta());
}
fn draw_picker(app: &mut Ui, ctx: &egui::Context) {
    drop(ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 800.),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |_| {});
            app.draw_picker(ctx, &notdiscord_core::models::AppSnapshot::welcome());
        },
    ));
    consume_updates(ctx);
}
fn drain(app: &mut Ui, ctx: &egui::Context) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        app.process_updates(ctx);
        consume_updates(ctx);
        assert!(app.emoji_pending.len() <= 67);
        assert!(app.emoji_weight <= 2 * 1024 * 1024);
        if app.emoji_pending.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native Windows emoji worker stalled"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "Initial Windows emoji phase attribution; explicit E: synthetic directory and separate process required"]
fn windows_emoji_cold_phase_probe() {
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
        std::env::var_os("NOTDISCORD_P2_SOAK_PROGRESS").expect("explicit E: progress file"),
    );
    assert_eq!(fs::canonicalize(path.parent().unwrap()).unwrap(), root);
    let samples_path = root.join("emoji-cold-phase-samples.jsonl");
    let output = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&samples_path)
            .unwrap(),
    );
    let mut probe = Probe {
        path,
        samples_path,
        output,
        samples: Vec::with_capacity(100),
        start: Instant::now(),
    };
    probe.phase(0, State::default());

    // No context/frame/font/glyph construction was performed before phase zero.
    let data = root.join(format!(
        "synthetic-emoji-cold-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&data).unwrap();
    let ctx = egui::Context::default();
    let (command, _commands) = tokio::sync::mpsc::unbounded_channel();
    let (_updates, receiver) = mpsc::channel();
    let mut app = Ui::from_context(&ctx, data.clone(), command, receiver);
    consume_updates(&ctx);
    assert_eq!(app.emojis.capacity(), 0);
    assert_eq!(app.emoji_weight, 0);
    probe.phase(1, state(Some(&app), Some(&ctx)));
    app.emojis = super::super::load_emoji_catalog();
    assert!(app.emojis.len() > 1000);
    assert!(app.emoji_pending.is_empty());
    probe.phase(2, state(Some(&app), Some(&ctx)));
    app.emojis = Vec::new();
    assert_eq!(app.emojis.capacity(), 0);
    probe.phase(3, state(Some(&app), Some(&ctx)));

    // Isolate DirectWrite/D2D/WIC from a first egui font-atlas/layout frame.
    // This is the production worker and production result consumer, with one
    // synthetic UI demand queued explicitly instead of drawing a fallback label.
    let key = ("😀".to_owned(), 32);
    let generation = app.emoji_generation.load(std::sync::atomic::Ordering::Acquire);
    app.emoji_request.try_send((generation, key.clone())).unwrap();
    app.emoji_pending.insert(key.clone(), generation);
    drain(&mut app, &ctx);
    assert!(app.emoji_textures.contains_key(&key));
    probe.phase(4, state(Some(&app), Some(&ctx)));
    app.open_picker(false, &notdiscord_core::models::AppSnapshot::welcome());
    for _ in 0..4 {
        draw_picker(&mut app, &ctx);
        drain(&mut app, &ctx);
    }
    assert!(app.emojis.len() > 1000);
    probe.phase(5, state(Some(&app), Some(&ctx)));
    let pointer = app.emojis.as_ptr();
    for _ in 0..10 {
        draw_picker(&mut app, &ctx);
        drain(&mut app, &ctx);
        assert_eq!(app.emojis.as_ptr(), pointer);
    }
    probe.phase(6, state(Some(&app), Some(&ctx)));
    app.picker = None;
    draw_picker(&mut app, &ctx);
    drain(&mut app, &ctx);
    assert_eq!(app.emojis.capacity(), 0);
    probe.phase(7, state(Some(&app), Some(&ctx)));
    let emoji_ids: Vec<_> = app
        .emoji_textures
        .values()
        .map(|texture| texture.id())
        .collect();
    drop(app);
    consume_updates(&ctx);
    for id in emoji_ids {
        assert!(ctx.tex_manager().read().meta(id).is_none());
    }
    probe.phase(8, state(None, Some(&ctx)));
    drop(ctx);
    probe.phase(9, State::default());

    assert_eq!(probe.samples.len(), 100);
    let summaries:Vec<_>=PHASES.iter().enumerate().map(|(index,name)| {
        let rows:Vec<_>=probe.samples.iter().filter(|row|row.phase==index).collect();
        let mut private:Vec<_>=rows.iter().map(|row|row.private_usage_bytes).collect();private.sort_unstable();
        let mut working:Vec<_>=rows.iter().map(|row|row.working_set_bytes).collect();working.sort_unstable();
        json!({"phase":index,"name":name,"samples":rows.len(),
            "privateUsageBytes":{"minimum":private[0],"median":(private[4]+private[5])/2,"maximum":private[9]},
            "workingSetBytes":{"minimum":working[0],"median":(working[4]+working[5])/2,"maximum":working[9]},
            "state":rows[0].state})
    }).collect();
    crate::storage::save_json(&root.join("emoji-cold-phase-summary.json"),&json!({
        "version":1,"valid":true,"synthetic":true,"authenticated":false,"gatewayStarted":false,
        "freshProcess":true,"windowsSystemCold":false,"externalHttpRequests":0,
        "sampleIntervalMs":300,"samplesPerPhase":10,"phases":summaries,
        "limitations":["Headless egui texture ownership and CPU upload payloads; no GPU residency measurement.",
            "GetProcessMemoryInfo PrivateUsage is committed private bytes, not Task Manager private working set.",
            "Phases are ordered within one fresh process; Windows font/native allocator caches can survive UI release.",
            "Small fixed sample/progress overhead is included; no working-set trimming or forced collection."],
    })).unwrap();
    assert_eq!(
        fs::canonicalize(&data).unwrap().parent(),
        Some(root.as_path())
    );
    assert!(fs::read_dir(&data).unwrap().next().is_none());
    fs::remove_dir(&data).unwrap();
    probe.progress(PHASES.len(), true);
}
