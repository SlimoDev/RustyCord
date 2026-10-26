//! Integrated viewer lifecycle with owned synthetic attachments. No account, network
//! or clipboard; file fixtures use the production asynchronous WIC worker.
#[cfg(any(debug_assertions, feature = "offline-probes"))]
mod probe {
    use eframe::egui;
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };
    const CASES: usize = 8;
    #[derive(Clone, Default)]
    struct Observation {
        root_pass: bool,
        title: bool,
        fit_control: bool,
        original_control: bool,
        ready: bool,
        error: bool,
        retry: bool,
        texture_size: Option<[usize; 2]>,
    }
    struct Report {
        current: Option<usize>,
        observations: Vec<Observation>,
        expected_sizes: Vec<Option<[usize; 2]>>,
        opens: usize,
        closes: usize,
        completed: bool,
    }
    impl Default for Report {
        fn default() -> Self {
            Self {
                current: None,
                observations: vec![Observation::default(); CASES],
                expected_sizes: (0..CASES).map(|index|match index%4 {
                    0=>Some([2,2]),1=>Some([1600,800]),2=>Some([1600,1200]),_=>None,
                }).collect(),
                opens: 0,
                closes: 0,
                completed: false,
            }
        }
    }
    struct Fixtures {
        root: PathBuf,
        files: Vec<PathBuf>,
        png_size: [usize; 2],
    }
    impl Fixtures {
        fn create() -> Result<Self, Box<dyn std::error::Error>> {
            let base = std::env::var_os("NOTDISCORD_VIEWER_PROBE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir);
            let base = std::fs::canonicalize(base)?;
            let project =
                std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
            if !base.starts_with(project)
                || !base
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(r"\\?\e:\")
            {
                return Err("Set NOTDISCORD_VIEWER_PROBE_DIR to an existing directory inside the project on E:.".into());
            }
            let root = base.join(format!(
                "notdiscord-synthetic-viewer-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir(&root)?;
            let mut fixtures = Self {
                root,
                files: Vec::new(),
                png_size: [1600,800],
            };
            // Register exact owned filenames before writing, including partial failures.
            let png = fixtures.root.join("synthetic-attachment-2048x1024.png");
            fixtures.files.push(png.clone());
            let arguments: Vec<_> = std::env::args().collect();
            if let Some(index) = arguments
                .iter()
                .position(|argument| argument == "--local-image")
            {
                let input = arguments
                    .get(index + 1)
                    .ok_or("--local-image needs an explicit local path on E:")?;
                let input = std::fs::canonicalize(input)?;
                if !input
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(r"\\?\e:\")
                    || !input.is_file()
                    || std::fs::metadata(&input)?.len() > 25 * 1024 * 1024
                {
                    return Err(
                        "The local attachment must be a file on E: within the 25MiB limit.".into(),
                    );
                }
                std::fs::copy(&input, &png)?;
                println!(
                    "{}",
                    serde_json::json!({"event":"viewer-owned-local-fixture","bytes":std::fs::metadata(&png)?.len(),"network":false})
                );
            } else {
                image::RgbaImage::from_fn(2048, 1024, |x, y| {
                    image::Rgba([
                        ((x / 8) % 256) as u8,
                        ((y / 4) % 256) as u8,
                        if (x / 128 + y / 128) % 2 == 0 {
                            220
                        } else {
                            50
                        },
                        255,
                    ])
                })
                .save_with_format(&png, image::ImageFormat::Png)?;
            }
            let (width,height)=image::ImageReader::open(&png)?.with_guessed_format()?.into_dimensions()?;
            let scale=(1600_f64/width.max(height) as f64).min(1.);
            fixtures.png_size=[(width as f64*scale).max(1.) as usize,(height as f64*scale).max(1.) as usize];
            let jpeg = fixtures.root.join("synthetic-attachment-4000x3000.jpg");
            fixtures.files.push(jpeg.clone());
            image::RgbImage::from_fn(4000, 3000, |x, y| {
                image::Rgb([((x / 16) % 256) as u8, ((y / 12) % 256) as u8, 140])
            })
            .save_with_format(&jpeg, image::ImageFormat::Jpeg)?;
            let bad = fixtures.root.join("synthetic-corrupt-attachment.png");
            fixtures.files.push(bad.clone());
            std::fs::write(
                &bad,
                b"\x89PNG\r\n\x1a\nsynthetic truncated PNG without IHDR",
            )?;
            Ok(fixtures)
        }
        fn case_file(&self, index: usize) -> Option<PathBuf> {
            match index % 4 {
                0 => None,
                kind => Some(self.files[kind - 1].clone()),
            }
        }
    }
    impl Drop for Fixtures {
        fn drop(&mut self) {
            for path in &self.files {
                if path.parent() == Some(self.root.as_path()) {
                    if let Err(error) = std::fs::remove_file(path) {
                        if error.kind() != std::io::ErrorKind::NotFound {
                            eprintln!("viewer_probe cleanup_file_failed: {error}");
                        }
                    }
                }
            }
            // Non-recursive: unknown files are deliberately left untouched.
            if let Err(error) = std::fs::remove_dir(&self.root) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("viewer_probe cleanup_directory_retained: {error}");
                }
            }
        }
    }
    fn case_name(index: usize) -> &'static str {
        match index % 4 {
            0 => "preseeded-2x2",
            1 => "attachment-image-worker",
            2 => "attachment-jpeg-4000x3000",
            _ => "attachment-corrupt-png",
        }
    }
    fn expected(observation: &Observation, index: usize) -> bool {
        let chrome=observation.root_pass&&observation.title&&observation.fit_control&&observation.original_control;
        if index % 4 == 3 {
            chrome && observation.error && observation.retry
        } else {
            chrome && observation.ready && observation.texture_size.is_some()
        }
    }
    fn inspect_shape(
        shape: &egui::Shape,
        observation: &mut Observation,
        textures: &mut Vec<egui::TextureId>,
    ) {
        match shape {
            egui::Shape::Text(text) => {
                let text = &text.galley.job.text;
                observation.ready |= text.contains("arrastra para desplazar");
                observation.title |= text == "Imagen / GIF";
                observation.fit_control |= text == "Ajustar";
                observation.original_control |= text == "100 %";
                observation.error |= text.contains("Imagen no disponible");
                observation.retry |= text == "Reintentar";
            }
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::Managed(0) => {
                textures.push(mesh.texture_id)
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    inspect_shape(shape, observation, textures);
                }
            }
            _ => {}
        }
    }
    fn install_observer(ctx: &egui::Context, report: Arc<Mutex<Report>>) {
        ctx.on_end_pass("viewer-attachment-lifecycle-probe",Arc::new(move|ctx|{
            if ctx.viewport_id()!=egui::ViewportId::ROOT{return;}
            let (index,expected_size)={let report=report.lock().unwrap();let Some(index)=report.current else{return;};(index,report.expected_sizes[index])};
            let mut layer_ids=ctx.memory(|memory|memory.layer_ids().collect::<Vec<_>>());
            if !layer_ids.contains(&egui::LayerId::background()){layer_ids.push(egui::LayerId::background());}
            let mut observation=Observation{root_pass:true,..Default::default()};let mut textures=Vec::new();
            ctx.graphics(|graphics|{for layer in &layer_ids{if let Some(list)=graphics.get(*layer){for entry in list.all_entries(){inspect_shape(&entry.shape,&mut observation,&mut textures);}}}});
            let manager=ctx.tex_manager();let manager=manager.read();
            // The ROOT pass also paints fonts, logos and avatars. Accept only a
            // mesh referencing the exact decoded fixture dimensions, never the
            // first arbitrary non-font texture from the surrounding application.
            observation.texture_size=textures.iter().filter_map(|texture|manager.meta(*texture).map(|meta|meta.size))
                .find(|size|Some(*size)==expected_size);
            let mut report=report.lock().unwrap();let previous=&mut report.observations[index];let already_expected=expected(previous,index);
            previous.root_pass|=observation.root_pass;previous.title|=observation.title;previous.fit_control|=observation.fit_control;previous.original_control|=observation.original_control;
            previous.ready|=observation.ready;previous.error|=observation.error;previous.retry|=observation.retry;
            if observation.texture_size.is_some(){previous.texture_size=observation.texture_size;}
            if !already_expected&&expected(previous,index){println!("{}",serde_json::json!({"event":"viewer-observed","case":case_name(index),"cycle":index/4+1,"rootPass":previous.root_pass,"textureSize":previous.texture_size,"errorUi":previous.error,"retryUi":previous.retry}));}
        }));
    }
    struct Probe {
        ui: notdiscord::ui::NotDiscordUi,
        report: Arc<Mutex<Report>>,
        cases: Vec<Option<PathBuf>>,
        index: usize,
        open: bool,
        phase_started: Instant,
        started: Instant,
        initialized: bool,
        _rx: tokio::sync::mpsc::UnboundedReceiver<notdiscord_core::models::UiAction>,
    }
    impl eframe::App for Probe {
        fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
            [0.; 4]
        }
        fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
            if self.report.lock().unwrap().completed {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            eframe::App::update(&mut self.ui, ctx, frame);
            if self.started.elapsed() > Duration::from_secs(22) {
                eprintln!("viewer_probe hard_timeout");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            if !self.initialized {
                assert_eq!(self.ui.readiness()["signedIn"], false);
                self.initialized = true;
                self.open = true;
                self.open_case();
            } else if self.open {
                let ready = expected(
                    &self.report.lock().unwrap().observations[self.index],
                    self.index,
                );
                if self.phase_started.elapsed() >= Duration::from_millis(650)
                    && (ready || self.phase_started.elapsed() >= Duration::from_secs(2))
                {
                    self.ui.set_fixture_viewer(false);
                    let mut report = self.report.lock().unwrap();
                    report.current = None;
                    report.closes += 1;
                    println!(
                        "{}",
                        serde_json::json!({"event":"viewer-close","case":case_name(self.index),"cycle":self.index/4+1,"expectedUiObserved":ready})
                    );
                    self.open = false;
                    self.phase_started = Instant::now();
                }
            } else if self.phase_started.elapsed() >= Duration::from_millis(350) {
                self.index += 1;
                if self.index == CASES {
                    self.report.lock().unwrap().completed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                self.open = true;
                self.open_case();
            }
            ctx.request_repaint_after(Duration::from_millis(25));
        }
    }
    impl Probe {
        fn open_case(&mut self) {
            let mut report = self.report.lock().unwrap();
            report.current = Some(self.index);
            report.opens += 1;
            drop(report);
            if let Some(path) = &self.cases[self.index] {
                self.ui.set_fixture_viewer_file(path.clone());
            } else {
                self.ui.set_fixture_viewer(true);
            }
            self.phase_started = Instant::now();
            println!(
                "{}",
                serde_json::json!({"event":"viewer-open","case":case_name(self.index),"cycle":self.index/4+1,"pathWorker":self.cases[self.index].is_some()})
            );
        }
    }
    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let fixtures = Fixtures::create()?;
        let panic_path = fixtures.root.join("panic.txt");
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |information| {
            let detail = format!("{information}\n");
            let mut end = detail.len().min(16 * 1024);
            while !detail.is_char_boundary(end) {
                end -= 1;
            }
            let _ = std::fs::write(&panic_path, &detail[..end]);
            previous_hook(information);
        }));
        let cases = (0..CASES).map(|index| fixtures.case_file(index)).collect();
        let data = fixtures.root.clone();
        let mut initial=Report::default();
        for index in [1,5] {initial.expected_sizes[index]=Some(fixtures.png_size);}
        let report = Arc::new(Mutex::new(initial));
        let observed = report.clone();
        let result = eframe::run_native(
            "NotDiscord · synthetic attachment viewer lifecycle",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([1100., 740.])
                    .with_transparent(true),
                ..Default::default()
            },
            Box::new(move |cc| {
                install_observer(&cc.egui_ctx, observed.clone());
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let (send, updates) = std::sync::mpsc::channel();
                let mut state = notdiscord_core::models::AppSnapshot::welcome();
                state.demo = true;
                state.signed_in = false;
                send.send(notdiscord::ui::UiUpdate::Snapshot(state))
                    .unwrap();
                Ok(Box::new(Probe {
                    ui: notdiscord::ui::NotDiscordUi::new(cc, data, tx, updates),
                    report: observed,
                    cases,
                    index: 0,
                    open: false,
                    phase_started: Instant::now(),
                    started: Instant::now(),
                    initialized: false,
                    _rx: rx,
                }))
            }),
        );
        let report = report.lock().unwrap();
        let success = result.is_ok()
            && report.completed
            && report.opens == CASES
            && report.closes == CASES
            && report
                .observations
                .iter()
                .enumerate()
                .all(|(index, observation)| expected(observation, index));
        println!(
            "{}",
            serde_json::json!({"event":"viewer-probe-result","success":success,"opens":report.opens,"closes":report.closes,"cycles":2,"account":false,"network":false,"clipboard":false,"pngWorkerDecoded":report.observations[1].ready&&report.observations[5].ready,"jpegWorkerDecoded":report.observations[2].ready&&report.observations[6].ready,"corruptErrorUi":expected(&report.observations[3],3)&&expected(&report.observations[7],7)})
        );
        drop(report);
        drop(fixtures);
        result?;
        if !success {
            return Err(
                "The synthetic viewer did not complete every decode/error/reopen observation."
                    .into(),
            );
        }
        Ok(())
    }
}
#[cfg(any(debug_assertions, feature = "offline-probes"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    probe::run()
}
#[cfg(not(any(debug_assertions, feature = "offline-probes")))]
fn main() {
    eprintln!(
        "Offline viewer probe was NOT executed. Build with --features offline-probes or use Debug."
    );
    std::process::exit(2);
}
