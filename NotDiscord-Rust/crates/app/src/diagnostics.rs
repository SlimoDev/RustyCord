//! Parent-side, typed adapters for operational diagnostics. Never record the
//! raw IPC payload, message, endpoint, caller/account ID or exception text.
use eframe::egui;
use notdiscord_core::diagnostics::{self as log, Component, Code, Phase};
use serde_json::Value;
use std::{path::PathBuf, sync::{Arc, Mutex}};

pub fn call_sources(ctx: &egui::Context, state: &notdiscord_core::models::AppSnapshot, panel_visible: bool) {
    let bits = log::call_sources_summary(state) | ((panel_visible as u64) << 63);
    let key = egui::Id::new("call-sources-diagnostic-state");
    let changed = ctx.data_mut(|data| {
        if data.get_temp::<u64>(key) == Some(bits) { return false; }
        data.insert_temp(key, bits);
        true
    });
    if changed { log::record(Component::App, Code::CallSourcesState, Phase::Present, bits); }
}

pub fn media_event(value: &Value) {
    let correlation=value["mediaSession"].as_u64().unwrap_or(0);
    let phase=match value["phase"].as_str().unwrap_or_default() {
        "connect"=>Phase::Connect,"read"=>Phase::Read,"write"=>Phase::Write,
        "enqueue"=>Phase::Enqueue,"heartbeat"=>Phase::Heartbeat,"dispatch"=>Phase::Dispatch,_=>Phase::Unknown,
    };
    match value["event"].as_str().unwrap_or_default() {
        "transportError"=>log::record(Component::Voice,match value["code"].as_str().unwrap_or_default() {
            "VOICE_HEARTBEAT_TIMEOUT"|"HEARTBEAT_TIMEOUT"=>Code::HeartbeatTimeout,
            "VOICE_SOCKET_HTTP"=>Code::TransportHttp,"VOICE_SOCKET_TLS"=>Code::TransportTls,
            "VOICE_SOCKET_NETWORK"=>Code::TransportIo,"VOICE_SOCKET_LIMIT"|"VOICE_SOCKET_HANDSHAKE_LIMIT"=>Code::TransportCapacity,
            "VOICE_SOCKET_PROTOCOL"=>Code::TransportProtocol,"VOICE_SOCKET_CLOSED"=>Code::TransportClosed,
            "VOICE_SOCKET_ALREADY_CLOSED"=>Code::TransportAlreadyClosed,"VOICE_SOCKET_UTF8"=>Code::TransportUtf8,
            "VOICE_SOCKET_ENDPOINT"=>Code::TransportEndpoint,"VOICE_SOCKET_HTTP_FORMAT"=>Code::TransportFormat,
            "VOICE_QUEUE_FULL"|"NAPI_QUEUE_FULL"|"VOICE_SEND_QUEUE_FULL"|"VOICE_CONTROL_QUEUE_FULL"|"MEDIA_DISPATCH_QUEUE_FULL"|"MEDIA_CONTROL_QUEUE_FULL"|"MEDIA_CALLBACK_UNAVAILABLE"=>Code::QueueSaturated,
            "VOICE_SEND_TOO_LARGE"=>Code::TransportCapacity,
            "VOICE_SOCKET_TIMEOUT"|"VOICE_WRITE_TIMEOUT"=>Code::ControlTimeout,_=>Code::TransportFailure,
        },phase,correlation),
        "fatal"=>{
            let code=match value["error"]["controlCode"].as_str().unwrap_or_default(){
                "HOST_CONTROL_READ_FAILED"=>Code::ControlReadFailed,"HOST_CONTROL_WRITE_FAILED"=>Code::ControlWriteFailed,
                "HOST_CONTROL_TIMEOUT"=>Code::ControlTimeout,"VIDEO_PIPE_FAILED"=>Code::VideoPipeFailed,_=>Code::TransportClosed,
            };
            log::record(Component::Media,code,Phase::Stop,0);
            log::record(Component::Media,Code::HostExited,Phase::Stop,value["error"]["exitCode"].as_i64().unwrap_or(0) as u64);
            if value["error"]["forcedStop"].as_bool()==Some(true){log::record(Component::Media,Code::HostForcedStop,Phase::Stop,0);}
        },
        "videoError"=>log::record(Component::Media,Code::Recoverable,Phase::Read,correlation),
        _=>{}
    }
}
pub fn open(ctx: &egui::Context) {ctx.data_mut(|data|data.insert_temp(egui::Id::new("diagnostic-panel"),true));}
pub fn close(ctx: &egui::Context) {ctx.data_mut(|data|data.insert_temp(egui::Id::new("diagnostic-panel"),false));}
pub fn is_open(ctx:&egui::Context)->bool{ctx.data(|data|data.get_temp::<bool>(egui::Id::new("diagnostic-panel")).unwrap_or(false))}
/// Everything shown, copied and exported comes from one captured snapshot.
#[derive(Default)]
struct PanelState {
    events: Arc<Vec<log::Event>>, rows: Vec<String>, revision: Option<(usize, u64, u32)>,
    valid: bool, busy: bool, notice: String, error: String,
    exported: Option<PathBuf>, show_session: bool, exporting: bool,
}
impl PanelState {
    fn accept(&mut self, events: Vec<log::Event>) {
        self.valid = true;
        self.busy = false;
        let revision = events.last().map(|event| { let (sequence, repetitions) = event.revision(); (events.len(), sequence, repetitions) });
        if revision == self.revision { return; }
        self.revision = revision;
        self.rows = events.iter().rev().filter_map(|event| serde_json::to_string(event).ok()).collect();
        self.events = Arc::new(events);
    }
    fn target(&self) -> Option<PathBuf> {
        self.exported.clone().filter(|_| !self.show_session).or_else(log::session_log)
    }
}
#[derive(Clone, Default)]
struct Panel(Arc<Mutex<PanelState>>);
impl Panel {
    fn state(&self) -> std::sync::MutexGuard<'_, PanelState> { self.0.lock().unwrap_or_else(|error| error.into_inner()) }
    fn reveal(&self, ctx: &egui::Context, path: PathBuf) {
        let (panel, ctx) = (self.clone(), ctx.clone());
        let started = std::thread::Builder::new().name("diagnostic-reveal".into()).spawn(move || {
            if let Err(error) = crate::platform::reveal_file(&path) {
                panel.state().error = error;
                ctx.request_repaint();
            }
        });
        if started.is_err() { self.state().error = "No se pudo abrir la carpeta de diagnóstico.".into(); }
    }
    fn export(&self, ctx: &egui::Context, events: Arc<Vec<log::Event>>) {
        // Set before the thread exists: it may finish first and clear this.
        self.state().exporting = true;
        let (panel, ctx) = (self.clone(), ctx.clone());
        let started = std::thread::Builder::new().name("diagnostic-export".into()).spawn(move || {
            let chosen = rfd::FileDialog::new().add_filter("JSON", &["json"]).set_file_name("NotDiscord-diagnostico.json").save_file();
            let result = chosen.map(|path| log::render(&events).and_then(|text| log::export(&path, &text)).map(|()| path));
            let mut state = panel.state();
            state.exporting = false;
            match result {
                Some(Ok(path)) => {
                    state.error.clear();
                    state.notice = format!("Exportados {} eventos.", events.len());
                    state.exported = Some(path);
                    state.show_session = false;
                }
                Some(Err(error)) => state.error = error.into(),
                None => {}
            }
            drop(state);
            ctx.request_repaint();
        });
        if started.is_err() {
            let mut state = self.state();
            state.exporting = false;
            state.error = "No se pudo iniciar la exportación.".into();
        }
    }
}
pub fn draw(ctx: &egui::Context) {
    let id=egui::Id::new("diagnostic-panel");
    let mut shown=ctx.data(|data|data.get_temp::<bool>(id).unwrap_or(false));
    if !shown {return;}
    let panel=ctx.data_mut(|data|data.get_temp_mut_or_default::<Panel>(id.with("panel")).clone());
    let snapshot=log::snapshot();
    let (mut reveal,mut export)=(None,None);
    let mut state=panel.state();
    let unavailable=matches!(snapshot,Err(log::SnapshotError::Unavailable));
    match snapshot {
        Ok(events)=>state.accept(events),
        Err(log::SnapshotError::Busy)=>{state.busy=true;ctx.request_repaint_after(std::time::Duration::from_millis(200));}
        Err(log::SnapshotError::Unavailable)=>{}
    }
    egui::Window::new("Diagnóstico local").id(id).order(egui::Order::Foreground).open(&mut shown).default_width(680.).show(ctx,|ui|{
        ui.label("Registros locales acotados: clases de error y etapas. No incluyen mensajes, credenciales ni direcciones privadas. No se envían automáticamente.");
        if log::disk_failed(){ui.colored_label(egui::Color32::LIGHT_RED,"No se pudo seguir escribiendo el registro local. El resumen en memoria sigue disponible para exportar.");}
        ui.horizontal_wrapped(|ui|{
            if ui.button("Abrir carpeta").clicked(){
                state.error.clear();
                match state.target() {
                    Some(path)=>reveal=Some(path),
                    None=>state.error="El registro local no está disponible; comprueba el espacio y los permisos de la carpeta de datos.".into(),
                }
            }
            if ui.add_enabled(state.valid,egui::Button::new("Copiar todo")).clicked(){
                match log::render(&state.events) {
                    Ok(text)=>{
                        ui.ctx().copy_text(text);
                        state.error.clear();
                        state.notice=format!("Copiados {} eventos.",state.events.len());
                    }
                    Err(error)=>state.error=error.into(),
                }
            }
            if ui.add_enabled(state.valid&&!state.exporting,egui::Button::new(if state.exporting {"Exportando…"} else {"Exportar resumen…"})).clicked(){
                state.error.clear();
                export=Some(state.events.clone());
            }
        });
        if let Some(path)=state.target() {
            let exported=state.exported.is_some()&&!state.show_session;
            let name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            ui.horizontal_wrapped(|ui|{
                ui.label(egui::RichText::new(format!("{}: {name}",if exported {"Archivo exportado"} else {"Registro de esta ejecución"})).small());
                if state.exported.is_some() {
                    let label=if exported {"Ver registro de la ejecución"} else {"Ver archivo exportado"};
                    if ui.small_button(label).clicked(){state.show_session=exported;}
                }
            });
        }
        ui.label(egui::RichText::new("Vista: recientes primero. Copiar y exportar usan orden cronológico. El registro en disco puede tener más eventos que este resumen.").small().weak());
        if !state.error.is_empty(){ui.colored_label(egui::Color32::LIGHT_RED,&state.error);}
        else if !state.notice.is_empty(){ui.label(egui::RichText::new(&state.notice).small());}
        if state.busy&&state.valid {ui.label(egui::RichText::new("El registro está ocupado; se muestra el último resumen válido.").small().weak());}
        ui.separator();
        crate::ui::autoscroll::ScrollArea::vertical().id_salt("diagnostic-events").max_height(360.).show(ui,|ui|{
            if unavailable&&!state.valid {ui.label("El registro no está disponible en esta ejecución.");}
            else if !state.valid {ui.label("El registro está ocupado; volviendo a intentarlo…");}
            else if state.rows.is_empty() {ui.label("Sin eventos disponibles en esta ejecución.");}
            for row in &state.rows {ui.monospace(row);}
        });
    });
    drop(state);
    if let Some(path)=reveal {panel.reveal(ctx,path);}
    if let Some(events)=export {panel.export(ctx,events);}
    ctx.data_mut(|data|data.insert_temp(id,shown));
}
