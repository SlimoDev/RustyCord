//! Opt-in offline soak: production UI/models, synthetic snapshots, no controller.
//! This diagnostic executable is never copied into the user distribution.
use eframe::egui;
use notdiscord::ui::{NotDiscordUi, UiUpdate};
use notdiscord_core::models::{AppSnapshot, MessageReaction, StatusEmoji};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

const WORKSPACE: &str = r"E:\DESCARGAS\REA\NotDiscordRust";
struct Config {
    fixture: PathBuf,
    data: PathBuf,
    progress: PathBuf,
    png: PathBuf,
    seconds: u64,
}
fn config() -> Result<Config, String> {
    let args: Vec<String> = std::env::args().collect();
    let value = |name: &str| -> Result<String, String> {
        args.iter()
            .position(|item| item == name)
            .and_then(|index| args.get(index + 1))
            .cloned()
            .ok_or_else(|| format!("Missing explicit argument {name}"))
    };
    let root = std::fs::canonicalize(WORKSPACE).map_err(|error| error.to_string())?;
    let existing = |name: &str| -> Result<PathBuf, String> {
        let path = std::fs::canonicalize(value(name)?).map_err(|error| error.to_string())?;
        if !path.starts_with(&root) {
            return Err(format!("{name} must remain in the E: workspace"));
        }
        Ok(path)
    };
    let output = |name: &str| -> Result<PathBuf, String> {
        let path = PathBuf::from(value(name)?);
        let parent = path.parent().ok_or("Output requires an explicit parent")?;
        let parent = std::fs::canonicalize(parent).map_err(|error| error.to_string())?;
        if !path.is_absolute() || !parent.starts_with(&root) || path.exists() {
            return Err(format!("{name} requires a new output inside E:"));
        }
        Ok(parent.join(path.file_name().ok_or("Missing filename")?))
    };
    let seconds = value("--seconds")?
        .parse::<u64>()
        .map_err(|error| error.to_string())?;
    if !(5..=3600).contains(&seconds) {
        return Err("Duration must be 5..3600 seconds".into());
    }
    let data = existing("--data-dir")?;
    // Only a fresh directory, optionally with this run's synthetic UI settings.
    for entry in std::fs::read_dir(&data).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_name() != "ui-settings.json"
            || !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
        {
            return Err(
                "The diagnostic data directory must be fresh, without account files".into(),
            );
        }
    }
    Ok(Config {
        fixture: existing("--fixture")?,
        data,
        progress: output("--progress")?,
        png: output("--png")?,
        seconds,
    })
}
struct SoakUi {
    ui: NotDiscordUi,
    latest_snapshot: Arc<Mutex<Option<AppSnapshot>>>,
    commands: tokio::sync::mpsc::UnboundedReceiver<notdiscord_core::models::UiAction>,
    state: AppSnapshot,
    config: Config,
    fixture_sha256: String,
    started: Instant,
    heartbeat: Instant,
    keepalive: Instant,
    publication: Instant,
    rendered_frames: u64,
    completed_cycles: u64,
    finishing: bool,
}
impl eframe::App for SoakUi {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0., 0., 0., 0.]
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let elapsed = self.started.elapsed();
        if elapsed < Duration::from_secs(self.config.seconds) {
            // Keep the existing short-preview timer alive; no production change.
            if self.keepalive.elapsed() >= Duration::from_secs(2) {
                self.ui.set_preview(self.config.png.clone());
                self.keepalive = Instant::now();
            }
            if self.publication.elapsed() >= Duration::from_secs(1) {
                self.completed_cycles += 1;
                // Exercise copy-on-write and publication, with bounded content.
                if let Some(message) = self.state.messages.last_mut() {
                    message.content = format!(
                        "Offline P2 cycle {:06} · 本文 한글 · 😀 🐈 🎉",
                        self.completed_cycles
                    );
                    let glyphs = ["😀", "🐈", "🎉", "👍🏽", "👩‍💻", "🇵🇪", "❤️", "🏳️‍🌈"];
                    message.reactions = glyphs
                        .iter()
                        .map(|name| MessageReaction {
                            emoji: StatusEmoji {
                                name: (*name).into(),
                                ..Default::default()
                            },
                            count: 1,
                            ..Default::default()
                        })
                        .collect();
                }
                *self.latest_snapshot.lock().expect("Owned snapshot slot") =
                    Some(self.state.clone());
                self.publication = Instant::now();
            }
        } else if !self.finishing {
            self.finishing = true;
            self.ui.set_preview(self.config.png.clone());
            self.ui.set_preview_delay(500);
        }
        // No receiver ever delegates these actions to an actor or network.
        while self.commands.try_recv().is_ok() {}
        eframe::App::update(&mut self.ui, ctx, frame);
        self.rendered_frames += 1;
        if self.heartbeat.elapsed() >= Duration::from_secs(1) || self.finishing {
            let mut progress = self.ui.readiness();
            let object = progress.as_object_mut().expect("Readiness object");
            object.insert("offlineSoak".into(), true.into());
            object.insert("controllerStarted".into(), false.into());
            object.insert("mediaHostStarted".into(), false.into());
            object.insert("networkUrls".into(), 0.into());
            object.insert("renderedFrames".into(), self.rendered_frames.into());
            object.insert("completedCycles".into(), self.completed_cycles.into());
            object.insert("elapsedMs".into(), (elapsed.as_millis() as u64).into());
            object.insert("requestedSeconds".into(), self.config.seconds.into());
            object.insert("fixtureSha256".into(), self.fixture_sha256.clone().into());
            object.insert(
                "phase".into(),
                if self.finishing {
                    "finishing"
                } else {
                    "running"
                }
                .into(),
            );
            notdiscord::storage::save_json(&self.config.progress, &progress)
                .expect("Own progress file on E:");
            self.heartbeat = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
fn run() -> Result<(), String> {
    let config = config()?;
    let bytes = std::fs::read(&config.fixture).map_err(|error| error.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Synthetic fixture exceeds 8MiB".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    if text.to_ascii_lowercase().contains("https://")
        || text.to_ascii_lowercase().contains("http://")
    {
        return Err("The offline fixture must contain no network URLs".into());
    }
    let state: AppSnapshot = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !state.demo
        || !state.signed_in
        || state.view != "chat"
        || state.messages.len() != 200
        || state.guild_members.len() != 500
        || state.direct_messages.len() != 100
        || state.current_user.as_ref().map(|person| person.id.as_str())
            != Some("100000000000000000")
    {
        return Err("Expected the explicit synthetic P2 chat fixture".into());
    }
    let fixture_sha256 = format!("{:X}", Sha256::digest(&bytes));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360., 860.])
            .with_min_inner_size([1000., 650.])
            .with_transparent(true)
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "NotDiscord · P2 offline soak",
        options,
        Box::new(move |cc| {
            let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
            let (updates, snapshots) = mpsc::channel();
            let mut ui = NotDiscordUi::new(cc, config.data.clone(), commands, snapshots);
            ui.set_preview(config.png.clone());
            ui.set_preview_delay(10000);
            let latest_snapshot = Arc::new(Mutex::new(Some(state.clone())));
            updates
                .send(UiUpdate::LatestSnapshot(latest_snapshot.clone()))
                .expect("Owned synthetic snapshot");
            let now = Instant::now();
            Ok(Box::new(SoakUi {
                ui,
                latest_snapshot,
                commands: receiver,
                state,
                config,
                fixture_sha256,
                started: now,
                heartbeat: now - Duration::from_secs(1),
                keepalive: now,
                publication: now,
                rendered_frames: 0,
                completed_cycles: 0,
                finishing: false,
            }))
        }),
    )
    .map_err(|error| error.to_string())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("Offline P2 soak failed: {error}");
        std::process::exit(1);
    }
}
