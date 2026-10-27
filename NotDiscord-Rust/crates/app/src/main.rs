#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use notdiscord::{authentication, controller, ui};

use std::{path::PathBuf, sync::mpsc};
struct DesktopApp {
    ui: ui::NotDiscordUi,
    readiness: Option<PathBuf>,
    last: std::time::Instant,
    instance_signal: Option<std::sync::Arc<notdiscord::instance::ShutdownSignal>>,
    replacement_notice: bool,
    closing_for_replacement: bool,
    replacement_close_error: Option<String>,
    shutdown: tokio::sync::mpsc::UnboundedSender<notdiscord_core::models::UiAction>,
}
impl eframe::App for DesktopApp {
    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0., 0., 0., 0.]
    }
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        let focused=ctx.input(|input|input.focused);
        let previous_focus=ctx.data_mut(|data|{
            let key=eframe::egui::Id::new("diagnostic-root-focus");
            let previous=data.get_temp::<bool>(key);data.insert_temp(key,focused);previous
        });
        if previous_focus!=Some(focused){notdiscord_core::diagnostics::record(notdiscord_core::diagnostics::Component::Input,notdiscord_core::diagnostics::Code::FocusChanged,notdiscord_core::diagnostics::Phase::Present,u64::from(focused));}
        if !self.closing_for_replacement
            && self.replacement_close_error.is_none()
            && self.instance_signal.as_ref().is_some_and(|signal| signal.requested()) {
            match self.ui.prepare_orderly_close() {
                Ok(())=>{
                    self.closing_for_replacement = true;
                    let _ = self.shutdown.send(notdiscord_core::models::UiAction::new("shutdown"));
                    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                }
                Err(error)=>self.replacement_close_error=Some(error),
            }
        }
        if self.instance_signal.is_some() {
            // Remains responsive even while the root viewport is minimized.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        eframe::App::update(&mut self.ui, ctx, frame);
        if let Some(error)=self.replacement_close_error.clone() {
            eframe::egui::Window::new("No se pudo guardar antes de cambiar de versión")
                .id(eframe::egui::Id::new("instance-close-error"))
                .collapsible(false).resizable(false)
                .anchor(eframe::egui::Align2::CENTER_CENTER,[0.,0.])
                .show(ctx,|ui|{
                    ui.label(error);
                    ui.label("Esta ventana se mantendrá abierta para conservar los ajustes. La nueva instancia espera a que termine el cierre.");
                    if ui.button("Reintentar cierre").clicked() { self.replacement_close_error=None; }
                });
        }
        if self.replacement_notice {
            eframe::egui::Window::new("Cambio de versión de NotDiscord")
                .id(eframe::egui::Id::new("instance-replaced-notice"))
                .collapsible(false).resizable(false)
                .anchor(eframe::egui::Align2::CENTER_TOP, [0., 64.])
                .show(ctx, |ui| {
                    ui.label("La instancia anterior se cerró porque abriste otra versión de NotDiscord.");
                    ui.label("La configuración y la sesión se comparten entre ambas versiones.");
                    if ui.button("Entendido").clicked() { self.replacement_notice = false; }
                });
        }
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
    notdiscord_core::diagnostics::install_panic_hook();
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
    if args.iter().any(|a| a == "--login") {
        let data = absolute(arg("--data-dir").unwrap_or_else(|| root.join(notdiscord::brand::DATA_DIRECTORY)));
        authentication::login_process(&data);
        return Ok(());
    }
    let preview = arg("--preview").map(absolute);
    let legacy_sources = notdiscord::shared_state::legacy_directories(&root, arg("--data-dir").map(absolute));
    let (data, desktop_instance) = if preview.is_some() {
        // Explicit offline fixtures keep their own files and never migrate or
        // replace a running signed-in desktop application.
        (absolute(arg("--data-dir").unwrap_or_else(|| root.join(notdiscord::brand::DATA_DIRECTORY))), None)
    } else {
        let initialized = (|| -> Result<_, String> {
            let data = notdiscord::shared_state::directory(arg("--shared-data-dir").map(absolute))?;
            let instance = notdiscord::instance::InstanceGuard::acquire(&data, &root)?;
            notdiscord::shared_state::initialize(&data, &legacy_sources)?;
            let data = std::fs::canonicalize(&data).map_err(|_| "No se pudo abrir la carpeta compartida.")?;
            Ok((data, Some(instance)))
        })();
        match initialized {
            Ok(value) => value,
            Err(error) => { notdiscord::instance::show_startup_error(&error); return Ok(()); }
        }
    };
    let instance_signal = desktop_instance.as_ref().map(|instance| instance.signal());
    let diagnostic_session=notdiscord_core::diagnostics::initialise(&data,&notdiscord::updates::installed_version());
    let replacement_notice = desktop_instance.as_ref().is_some_and(|instance| instance.replaced_previous());
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
    let fixture = arg("--fixture").map(absolute);
    let shutdown = commands.clone();
    let app_shutdown = commands.clone();
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
    let ui_ready=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ui_created=ui_ready.clone();
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
            let desktop=DesktopApp {
                ui: app,
                readiness,
                last: std::time::Instant::now(),
                instance_signal,
                replacement_notice,
                closing_for_replacement: false,
                replacement_close_error: None,
                shutdown: app_shutdown,
            };
            ui_created.store(true,std::sync::atomic::Ordering::Release);
            Ok(Box::new(desktop))
        }),
    );
    let _ = shutdown.send(notdiscord_core::models::UiAction::new("shutdown"));
    if result.is_err(){
        use notdiscord_core::diagnostics::{record,Component,Code,Phase};
        if ui_ready.load(std::sync::atomic::Ordering::Acquire){record(Component::App,Code::Recoverable,Phase::Present,0);}
        else{record(Component::App,Code::AppStartFailed,Phase::Start,0);}
    }
    if let Some(task) = task.lock().unwrap().take() {
        runtime.block_on(async {
            use notdiscord_core::diagnostics::{record,Component,Code,Phase};
            match tokio::time::timeout(std::time::Duration::from_secs(30), task).await {
                Ok(Ok(()))=>{},Ok(Err(error))=>record(Component::Controller,if error.is_cancelled(){Code::TaskCancelled}else{Code::TaskFailed},Phase::Stop,0),
                Err(_)=>record(Component::Controller,Code::TaskTimeout,Phase::Stop,0),
            }
        });
    }
    // Releasing ownership before controller/runtime teardown would let the new
    // edition read stale preferences or contend for still-open media files.
    drop(runtime);
    drop(desktop_instance);
    drop(diagnostic_session);
    result
}
