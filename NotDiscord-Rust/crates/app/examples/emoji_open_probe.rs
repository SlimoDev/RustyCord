//! Native first-open measurement. Only this window receives synthetic egui
//! events; no controller, credentials, remote URLs, device or global input.
use eframe::egui;
use notdiscord::ui::{NotDiscordUi, UiUpdate};
use notdiscord_core::models::{AppSnapshot, ChannelAccess, ChatMessage, Person, UiAction};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use windows::Win32::System::{
    ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        PROCESS_MEMORY_COUNTERS_EX2,
    },
    Threading::GetCurrentProcess,
};

#[derive(Default)]
struct Seen {
    button: Option<egui::Pos2>,
    title: Option<egui::Pos2>,
    open: bool,
    unicode: bool,
}
fn see_shape(shape: &egui::Shape, seen: &mut Seen) {
    match shape {
        egui::Shape::Text(text) => {
            let label = &text.galley.job.text;
            let center = text.pos + text.galley.size() / 2.;
            if label == "☺" && seen.button.is_none_or(|old| center.y > old.y) {
                seen.button = Some(center);
            }
            if label == "Emojis" {
                seen.title = Some(center);
            }
            seen.open |= label == "Elige un emoji";
            seen.unicode |= label.contains("Windows") && label.contains("Unicode");
        }
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                see_shape(shape, seen);
            }
        }
        _ => {}
    }
}
fn observe(ctx: &egui::Context) -> Seen {
    let mut seen = Seen::default();
    let mut layers = ctx.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
    if !layers.contains(&egui::LayerId::background()) {
        layers.push(egui::LayerId::background());
    }
    ctx.graphics(|graphics| {
        for layer in layers {
            if let Some(list) = graphics.get(layer) {
                for entry in list.all_entries() {
                    see_shape(&entry.shape, &mut seen);
                }
            }
        }
    });
    seen
}
fn pointer(point: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}
fn escape(pressed: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}
struct Probe {
    ui: NotDiscordUi,
    commands: tokio::sync::mpsc::UnboundedReceiver<UiAction>,
    root: PathBuf,
    started: Instant,
    phase_started: Instant,
    last_sample: Instant,
    stage: u8,
    queued: Vec<egui::Event>,
    click: egui::Pos2,
    samples: Vec<serde_json::Value>,
    actions: Vec<String>,
    close_method: &'static str,
    opened: bool,
    closed: bool,
    qa: bool,
    screenshot_size: Option<[usize; 2]>,
    outcome: Arc<Mutex<Option<bool>>>,
}
impl Probe {
    fn begin(&mut self, stage: u8) {
        self.stage = stage;
        self.phase_started = Instant::now();
        self.last_sample = Instant::now() - Duration::from_millis(100);
    }
    fn fail(&mut self, ctx: &egui::Context, error: &str) {
        self.finish(ctx, false, Some(error));
    }
    fn finish(&mut self, ctx: &egui::Context, valid: bool, error: Option<&str>) {
        if self.stage == 255 {
            return;
        }
        let summaries:Vec<_>=["baseline","first-open","closed"].iter().map(|name| {
            let rows:Vec<_>=self.samples.iter().filter(|sample|sample["phase"]==*name).collect();
            let stats=|field:&str|{
                let mut values:Vec<_>=rows.iter().filter_map(|row|row[field].as_u64()).collect();values.sort_unstable();
                if values.is_empty(){return json!(null);}
                json!({"minimum":values[0],"median":(values[(values.len()-1)/2]+values[values.len()/2])/2,"maximum":values[values.len()-1]})
            };
            json!({"phase":name,"samples":rows.len(),"privateUsageBytes":stats("privateUsageBytes"),"workingSetBytes":stats("workingSetBytes"),"privateWorkingSetBytes":stats("privateWorkingSetBytes"),"privateWorkingSetSamples":rows.iter().filter(|row|row["privateWorkingSetBytes"].is_u64()).count(),"textureMetadataBytes":stats("textureMetadataBytes")})
        }).collect();
        let covered = summaries
            .iter()
            .all(|phase| phase["samples"].as_u64().is_some_and(|count| count >= 20));
        let valid = valid && covered && self.opened && self.closed && self.qa;
        let error =
            error.or_else(|| (!covered).then_some("phase sampling coverage below twenty samples"));
        let report = json!({"version":1,"valid":valid,"failure":error,"synthetic":true,
            "authenticated":false,"controllerStarted":false,"gatewayStarted":false,"externalHttpRequests":0,
            "mediaHostStarted":false,"globalInputInjected":false,"openedConfirmed":self.opened,
            "closedConfirmed":self.closed,"closeMethod":self.close_method,"screenshotCaptured":self.qa,
            "screenshotSize":self.screenshot_size,"elapsedMs":self.started.elapsed().as_millis(),
            "phases":summaries,"samples":self.samples,"discardedUiActions":self.actions,
            "method":"Native eframe renderer; baseline/open/closed each >=3 seconds. Screenshot uses a second opening after all measured phases.",
            "limitations":["Fresh process, not cold Windows font/driver caches.",
                "PrivateUsage is committed private bytes; WorkingSet is total resident process pages, not private working set.",
                "PrivateWorkingSetBytes is optional via PROCESS_MEMORY_COUNTERS_EX2; unsupported systems use null, never a fabricated zero. Task Manager can sample or aggregate differently.",
                "Texture metadata and a real native GPU rendering path are exercised; GPU VRAM residency is not directly measured.",
                "Synthetic fixture only; no account, actor, Gateway, remote URLs or actual message sends."]});
        let written = fs::write(
            self.root.join("result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .is_ok();
        *self.outcome.lock().unwrap() = Some(valid && written);
        println!(
            "{}",
            json!({"event":"emoji-open-probe-result","valid":valid&&written,"root":self.root,"failure":error})
        );
        self.stage = 255;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn sample(&mut self, ctx: &egui::Context) {
        if self.last_sample.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_sample = Instant::now();
        // EX2 is available on recent Windows versions. Its unsupported result
        // must not turn an absent private-working-set metric into zero.
        let (private_usage, working_set, peak_working_set, private_working_set, counter_version) = unsafe {
            let mut ex2 = PROCESS_MEMORY_COUNTERS_EX2::default();
            ex2.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
            if GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut ex2 as *mut PROCESS_MEMORY_COUNTERS_EX2).cast::<PROCESS_MEMORY_COUNTERS>(),
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
            )
            .is_ok()
            {
                (
                    ex2.PrivateUsage,
                    ex2.WorkingSetSize,
                    ex2.PeakWorkingSetSize,
                    Some(ex2.PrivateWorkingSetSize),
                    "EX2",
                )
            } else {
                let mut ex = PROCESS_MEMORY_COUNTERS_EX::default();
                ex.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
                GetProcessMemoryInfo(
                    GetCurrentProcess(),
                    (&mut ex as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
                    std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
                )
                .expect("own process counters EX fallback");
                (
                    ex.PrivateUsage,
                    ex.WorkingSetSize,
                    ex.PeakWorkingSetSize,
                    None,
                    "EX",
                )
            }
        };
        let manager = ctx.tex_manager();
        let manager = manager.read();
        self.samples.push(json!({"phase":match self.stage{0=>"baseline",3=>"first-open",7=>"closed",_=>"transition"},
            "elapsedMs":self.started.elapsed().as_millis(),"phaseElapsedMs":self.phase_started.elapsed().as_millis(),
            "privateUsageBytes":private_usage,"workingSetBytes":working_set,"privateWorkingSetBytes":private_working_set,
            "memoryCounterVersion":counter_version,"peakWorkingSetBytes":peak_working_set,"textures":manager.num_allocated(),
            "textureMetadataBytes":manager.allocated().map(|(_,meta)|meta.bytes_used()).sum::<usize>()}));
    }
}
impl eframe::App for Probe {
    fn raw_input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        input
            .events
            .retain(|event| matches!(event, egui::Event::Screenshot { .. }));
        input.events.append(&mut self.queued);
    }
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.stage == 255 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let capture = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = capture {
            let rgba: Vec<u8> = image
                .pixels
                .iter()
                .flat_map(|pixel| pixel.to_array())
                .collect();
            if image::save_buffer(
                self.root.join("open-qa.png"),
                &rgba,
                image.width() as u32,
                image.height() as u32,
                image::ColorType::Rgba8,
            )
            .is_err()
            {
                self.fail(ctx, "own screenshot could not be saved");
                return;
            }
            self.screenshot_size = Some(image.size);
            self.qa = true;
        }
        eframe::App::update(&mut self.ui, ctx, frame);
        while let Ok(action) = self.commands.try_recv() {
            if matches!(
                action.kind.as_str(),
                "send"
                    | "send-message"
                    | "voice"
                    | "video-call"
                    | "login"
                    | "start-share"
                    | "start-camera"
                    | "gif-catalog"
            ) {
                self.fail(
                    ctx,
                    "unexpected nonlocal UI action (no actor exists to execute it)",
                );
                return;
            }
            if self.actions.len() < 128 {
                self.actions.push(action.kind);
            }
        }
        if self.started.elapsed() > Duration::from_secs(30) {
            self.fail(ctx, "native probe timed out");
            return;
        }
        let seen = observe(ctx);
        match self.stage {
            0 => {
                self.sample(ctx);
                if self.phase_started.elapsed() >= Duration::from_secs(3) {
                    let Some(point) = seen.button else {
                        self.fail(ctx, "composer emoji button was not observed");
                        return;
                    };
                    self.click = point;
                    self.queued = pointer(point, true);
                    self.begin(1);
                }
            }
            1 => {
                self.queued = pointer(self.click, false);
                self.begin(2);
            }
            2 => {
                if seen.open && seen.unicode {
                    self.opened = true;
                    self.begin(3);
                } else if self.phase_started.elapsed() > Duration::from_secs(3) {
                    self.fail(ctx, "Unicode picker opening was not confirmed");
                }
            }
            3 => {
                if !seen.open {
                    self.fail(ctx, "picker disappeared during first-open sampling");
                    return;
                }
                self.sample(ctx);
                if self.phase_started.elapsed() >= Duration::from_secs(3) {
                    self.queued.push(escape(true));
                    self.close_method = "escape";
                    self.begin(4);
                }
            }
            4 => {
                self.queued.push(escape(false));
                if !seen.open {
                    self.closed = true;
                    self.begin(7);
                } else if self.phase_started.elapsed() > Duration::from_millis(350) {
                    // P2's Unicode Window has no Escape binding. Use its own
                    // title close control and report that fact explicitly.
                    let rect =
                        ctx.memory(|memory| memory.area_rect(egui::Id::new("content-picker")));
                    let (Some(rect), Some(title)) = (rect, seen.title) else {
                        self.fail(ctx, "picker close control was not observable");
                        return;
                    };
                    self.click = egui::pos2(rect.right() - 16., title.y);
                    self.queued.extend(pointer(self.click, true));
                    self.close_method = "titlebar-after-escape";
                    self.begin(5);
                }
            }
            5 => {
                self.queued = pointer(self.click, false);
                self.begin(6);
            }
            6 => {
                if !seen.open {
                    self.closed = true;
                    self.begin(7);
                } else if self.phase_started.elapsed() > Duration::from_secs(2) {
                    self.fail(ctx, "picker did not close after its titlebar control");
                }
            }
            7 => {
                if seen.open {
                    self.fail(ctx, "picker reappeared during closed sampling");
                    return;
                }
                self.sample(ctx);
                if self.phase_started.elapsed() >= Duration::from_secs(3) {
                    let Some(point) = seen.button else {
                        self.fail(ctx, "composer button missing after close");
                        return;
                    };
                    self.click = point;
                    self.queued = pointer(point, true);
                    self.begin(8);
                }
            }
            8 => {
                self.queued = pointer(self.click, false);
                self.begin(9);
            }
            9 => {
                if seen.open && seen.unicode && self.phase_started.elapsed() >= Duration::from_millis(500) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                    self.begin(10);
                } else if self.phase_started.elapsed() > Duration::from_secs(3) {
                    self.fail(ctx, "QA reopening was not confirmed");
                }
            }
            10 => {
                if self.qa {
                    self.finish(ctx, true, None);
                } else if self.phase_started.elapsed() > Duration::from_secs(5) {
                    self.fail(ctx, "native screenshot did not arrive");
                }
            }
            _ => {}
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
fn run() -> Result<(), String> {
    let arguments: Vec<_> = std::env::args().collect();
    let value = arguments
        .iter()
        .position(|value| value == "--run-dir")
        .and_then(|index| arguments.get(index + 1))
        .ok_or("--run-dir requires an explicit fresh E: directory")?;
    let root = fs::canonicalize(value).map_err(|error| error.to_string())?;
    let workspace =
        fs::canonicalize(r"E:\DESCARGAS\REA\NotDiscordRust").map_err(|error| error.to_string())?;
    if !root.starts_with(workspace) {
        return Err("All outputs must remain inside the E: workspace".into());
    }
    for file in fs::read_dir(&root).map_err(|error| error.to_string())? {
        let file = file.map_err(|error| error.to_string())?;
        if !matches!(file.file_name().to_str(), Some("stdout.log" | "stderr.log")) {
            return Err("Run directory must be fresh, except redirected stdout/stderr".into());
        }
    }
    let data = root.join("data");
    fs::create_dir(&data).map_err(|error| error.to_string())?;
    let person = Person {
        id: "100000000000000001".into(),
        name: "Synthetic local user".into(),
        ..Default::default()
    };
    let snapshot = AppSnapshot {
        demo: true,
        signed_in: true,
        view: "chat".into(),
        selected_channel_id: "200000000000000001".into(),
        selected_channel_kind: "dm".into(),
        channel_name: "Offline emoji first-open".into(),
        channel_access: ChannelAccess::private(),
        current_user: Some(person.clone()),
        messages: vec![ChatMessage {
            id: "300000000000000001".into(),
            channel_id: "200000000000000001".into(),
            author: person,
            content: "Synthetic local message; no network or account.".into(),
            ..Default::default()
        }]
        .into(),
        ..AppSnapshot::welcome()
    };
    let outcome = Arc::new(Mutex::new(None));
    let result = outcome.clone();
    eframe::run_native(
        "NotDiscord · offline native emoji probe",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1360., 860.])
                .with_min_inner_size([1360., 860.])
                .with_resizable(false)
                .with_transparent(true)
                .with_decorations(false),
            ..Default::default()
        },
        Box::new(move |cc| {
            let (command, commands) = tokio::sync::mpsc::unbounded_channel();
            let (updates, receiver) = mpsc::channel();
            let ui = NotDiscordUi::new(cc, data, command, receiver);
            updates.send(UiUpdate::Snapshot(snapshot)).unwrap();
            let now = Instant::now();
            Ok(Box::new(Probe {
                ui,
                commands,
                root,
                started: now,
                phase_started: now,
                last_sample: now - Duration::from_millis(100),
                stage: 0,
                queued: Vec::new(),
                click: egui::Pos2::ZERO,
                samples: Vec::with_capacity(120),
                actions: Vec::new(),
                close_method: "not-closed",
                opened: false,
                closed: false,
                qa: false,
                screenshot_size: None,
                outcome: result,
            }))
        }),
    )
    .map_err(|error| error.to_string())?;
    if *outcome.lock().unwrap() != Some(true) {
        return Err("native emoji probe did not complete all verified phases".into());
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("Native emoji probe failed: {error}");
        std::process::exit(1);
    }
}
