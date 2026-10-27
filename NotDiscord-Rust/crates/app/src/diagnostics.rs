//! Parent-side, typed adapters for operational diagnostics. Never record the
//! raw IPC payload, message, endpoint, caller/account ID or exception text.
use eframe::egui;
use notdiscord_core::diagnostics::{self as log, Component, Code, Phase};
use serde_json::Value;

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
pub fn is_open(ctx:&egui::Context)->bool{ctx.data(|data|data.get_temp::<bool>(egui::Id::new("diagnostic-panel")).unwrap_or(false))}
pub fn draw(ctx: &egui::Context) {
    let id=egui::Id::new("diagnostic-panel");
    let mut shown=ctx.data(|data|data.get_temp::<bool>(id).unwrap_or(false));
    if !shown {return;}
    let mut error=ctx.data(|data|data.get_temp::<String>(id.with("error"))).unwrap_or_default();
    egui::Window::new("Diagnóstico local").id(id).order(egui::Order::Foreground).open(&mut shown).default_width(680.).show(ctx,|ui|{
        ui.label("Registros locales acotados: clases de error y etapas. No incluyen mensajes, credenciales ni direcciones privadas. No se envían automáticamente.");
        if log::disk_failed(){ui.colored_label(egui::Color32::LIGHT_RED,"No se pudo seguir escribiendo el registro local. El resumen en memoria sigue disponible para exportar.");}
        ui.horizontal_wrapped(|ui|{
            if ui.button("Abrir carpeta").clicked(){
                if let Some(path)=log::directory(){
                    use std::os::windows::process::CommandExt;
                    if std::process::Command::new("explorer.exe").arg(path).creation_flags(0x08000000).spawn().is_err(){error="No se pudo abrir la carpeta de diagnóstico.".into();}
                }else{error="El registro local no está disponible; comprueba el espacio y los permisos de la carpeta de datos.".into();}
            }
            if ui.button("Exportar resumen…").clicked(){
                if let Some(path)=rfd::FileDialog::new().add_filter("JSON",&["json"]).set_file_name("NotDiscord-diagnostico.json").save_file(){
                    match log::export(&path){Ok(())=>error.clear(),Err(message)=>error=message.into()}
                }
            }
        });
        if !error.is_empty(){ui.colored_label(egui::Color32::LIGHT_RED,&error);}
        ui.separator();
        crate::ui::autoscroll::ScrollArea::vertical().id_salt("diagnostic-events").max_height(360.).show(ui,|ui|{
            let events=log::summary();
            if events.is_empty(){ui.label("Sin eventos disponibles en esta ejecución.");}
            for event in events.iter().rev(){if let Ok(text)=serde_json::to_string(event){ui.monospace(text);}}
        });
    });
    ctx.data_mut(|data|{data.insert_temp(id,shown);data.insert_temp(id.with("error"),error);});
}
