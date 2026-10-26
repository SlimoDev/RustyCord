#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use notdiscord::{authentication, controller, ui};

use std::{path::PathBuf, sync::mpsc};
struct DesktopApp {
    ui: ui::NotDiscordUi,
    readiness: Option<PathBuf>,
    last: std::time::Instant,
}
impl eframe::App for DesktopApp {
    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0., 0., 0., 0.]
    }
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        eframe::App::update(&mut self.ui, ctx, frame);
        if let Some(path) = &self.readiness {
            if self.last.elapsed() >= std::time::Duration::from_secs(1) {
                let _ = notdiscord::storage::save_json(path, &self.ui.readiness());
                self.last = std::time::Instant::now();
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }
}
fn main() -> eframe::Result {
    let args = std::env::args().collect::<Vec<_>>();
    let root = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    notdiscord::notification_identity::set_app_id();
    let arg = |key: &str| {
        args.iter()
            .position(|a| a == key)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let absolute = |p: PathBuf| std::path::absolute(&p).unwrap_or(p);
    let data = absolute(arg("--data-dir").unwrap_or_else(|| root.join(notdiscord::brand::DATA_DIRECTORY)));
    if args.iter().any(|a| a == "--login") {
        authentication::login_process(&data);
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(3)
        .enable_all()
        .build()
        .expect("Runtime Rust no disponible");
    let (commands, commands_rx) = tokio::sync::mpsc::unbounded_channel();
    let (updates_tx, updates) = mpsc::channel();
    let controller_data = data.clone();
    let controller_root = absolute(arg("--runtime-root").unwrap_or(root.clone()));
    let runtime_handle = runtime.handle().clone();
    let preview = arg("--preview").map(absolute);
    let fixture = arg("--fixture").map(absolute);
    let shutdown = commands.clone();
    let task = std::sync::Arc::new(std::sync::Mutex::new(None));
    let task_created = task.clone();
    let readiness = arg("--readiness-file").map(absolute);
    if preview.is_none() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = notdiscord::notification_identity::register(&exe, &controller_root, &data);
        }
    }
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([1000.0, 650.0])
        .with_maximized(preview.is_none())
        .with_transparent(true)
        .with_decorations(false);
    if let Ok(icon) = image::load_from_memory(include_bytes!("../../../assets/discord.ico")) {
        let icon = icon.to_rgba8();
        viewport = viewport.with_icon(eframe::egui::IconData {
            width: icon.width(),
            height: icon.height(),
            rgba: icon.into_raw(),
        });
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        notdiscord::brand::NAME,
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            if preview.is_none() {
                let running = runtime_handle.spawn(controller::run(
                    controller_root,
                    controller_data,
                    commands_rx,
                    updates_tx,
                    ctx,
                ));
                *task_created.lock().unwrap() = Some(running);
            } else {
                let snapshot = fixture
                    .and_then(|p| std::fs::read(p).ok())
                    .and_then(|bytes| {
                        serde_json::from_slice::<notdiscord_core::models::AppSnapshot>(&bytes).ok()
                    })
                    .unwrap_or_else(notdiscord_core::models::AppSnapshot::welcome);
                let _ = updates_tx.send(ui::UiUpdate::Snapshot(snapshot));
            }
            let mut app = ui::NotDiscordUi::new(cc, data, commands, updates);
            if let Some(path) = preview {
                app.set_preview(path);
                if let Some(delay) = args
                    .iter()
                    .position(|arg| arg == "--preview-duration-ms")
                    .and_then(|index| args.get(index + 1))
                    .and_then(|arg| arg.parse::<u64>().ok())
                {
                    app.set_preview_delay(delay);
                }
            }
            Ok(Box::new(DesktopApp {
                ui: app,
                readiness,
                last: std::time::Instant::now(),
            }))
        }),
    );
    let _ = shutdown.send(notdiscord_core::models::UiAction::new("shutdown"));
    if let Some(task) = task.lock().unwrap().take() {
        runtime.block_on(async {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(8), task).await;
        });
    }
    result
}
