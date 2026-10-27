//! Private, bounded IPC. Operational rejections never terminate a working call.
use notdiscord_core::video::{
    HEADER_LEN, LatestFrames, SharedVideoFrames, VideoFrame, decode_header,
};
use serde_json::{Value, json};
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
        BufReader,
    },
    net::windows::named_pipe::ServerOptions,
    sync::{mpsc, oneshot},
};
static NEXT_VIDEO_NAMESPACE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
const MAX_RESPONSE: usize = 262144;
const MAX_REQUEST: usize = 1024 * 1024;
struct Request {
    kind: String,
    args: Value,
    done: oneshot::Sender<Result<Value, String>>,
}
struct ControlExit {
    code: &'static str,
    message: String,
    intentional: bool,
    pending: Vec<oneshot::Sender<Result<Value,String>>>,
}
impl ControlExit {
    fn new(code:&'static str,message:impl Into<String>,intentional:bool)->Self {
        Self {code,message:message.into(),intentional,pending:Vec::new()}
    }
    fn with_pending(mut self,pending:oneshot::Sender<Result<Value,String>>)->Self {
        self.pending.push(pending);self
    }
}
fn early_host_exit(status:std::io::Result<std::process::ExitStatus>)->String {
    let exit_code=status.ok().and_then(|status|status.code());
    notdiscord_core::diagnostics::record(notdiscord_core::diagnostics::Component::Media,notdiscord_core::diagnostics::Code::HostExited,notdiscord_core::diagnostics::Phase::Start,exit_code.unwrap_or(0) as u64);
    match exit_code {
        Some(code)=>format!("El proceso multimedia terminó durante el inicio con código 0x{:08X}.",code as u32),
        None=>"El proceso multimedia terminó durante el inicio sin confirmar su estado.".into(),
    }
}
#[derive(Clone)]
pub struct MediaBridge {
    commands: Arc<mpsc::Sender<Request>>,
    close: tokio_util::sync::CancellationToken,
    video_frames: SharedVideoFrames,
    video_namespace: u64,
}
impl MediaBridge {
    pub fn is_available(root: &Path) -> bool {
        let config: Value = crate::storage::read_json(&root.join("runtime.json"), 16384);
        let exe = resolve(
            root,
            config["MediaHostExe"]
                .as_str()
                .unwrap_or("media-host/runtime/NotDiscord.Media.exe"),
        );
        let native = resolve(
            root,
            config["NativeRoot"]
                .as_str()
                .unwrap_or("media-host/runtime"),
        );
        exe.is_file()
            && root.join("NotDiscord.Media.node").is_file()
            && native
                .join("resources/standalone_modules/discord_voice/discord_voice.node")
                .is_file()
    }
    pub fn close(&self) {
        self.close.cancel();
        clear_video_namespace(&self.video_frames,self.video_namespace);
    }
    pub fn video_frames(&self) -> SharedVideoFrames {
        self.video_frames.clone()
    }
    pub fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.commands, &other.commands)
    }
    pub fn owns_video_id(&self,id:u64)->bool { id>>32==self.video_namespace>>32 }
    pub fn clear_video_frames(&self) {clear_video_namespace(&self.video_frames,self.video_namespace);}
    pub async fn start(
        root: &Path,
        data: &Path,
        events: mpsc::UnboundedSender<Value>,
        ctx: eframe::egui::Context,
    ) -> Result<Self, String> {
        Self::start_scoped(root,data,events,ctx,None).await
    }
    pub async fn start_receiver(root:&Path,data:&Path,events:mpsc::UnboundedSender<Value>,ctx:eframe::egui::Context,frames:SharedVideoFrames)->Result<Self,String> {
        let sequence=NEXT_VIDEO_NAMESPACE.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        if sequence>1_000_000 {return Err("Se agotaron los identificadores de vídeo de esta sesión.".into());}
        Self::start_scoped(root,data,events,ctx,Some((sequence<<32,frames))).await
    }
    async fn start_scoped(root:&Path,data:&Path,events:mpsc::UnboundedSender<Value>,ctx:eframe::egui::Context,scope:Option<(u64,SharedVideoFrames)>)->Result<Self,String> {
        let normal_root = normal_path(root);
        let root = normal_root.as_path();
        let normal_data = normal_path(data);
        let data = normal_data.as_path();
        let config: Value = crate::storage::read_json(&root.join("runtime.json"), 16384);
        let executable = resolve(
            root,
            config["MediaHostExe"]
                .as_str()
                .unwrap_or("media-host/runtime/NotDiscord.Media.exe"),
        );
        let native = resolve(
            root,
            config["NativeRoot"]
                .as_str()
                .unwrap_or("media-host/runtime"),
        );
        let addon = root.join("NotDiscord.Media.node");
        if !executable.is_file() || !addon.is_file() || !native.is_dir() {
            return Err(
                "No se encontraron el host y motor multimedia de esta distribución.".into(),
            );
        }
        let name = format!(
            r"\\.\pipe\NotDiscord.Media.{}",
            uuid::Uuid::new_v4().simple()
        );
        let server = private_pipe(&name)?;
        let video_name = format!("{name}.Frames");
        let video_server = private_pipe(&video_name)?;
        let (video_namespace,video_frames)=scope.unwrap_or_else(||(0,Arc::new(std::sync::Mutex::new(LatestFrames::default()))));
        let mut process = tokio::process::Command::new(&executable)
            .env_remove("ELECTRON_RUN_AS_NODE")
            .env_remove("NODE_OPTIONS")
            .env_remove("NOTDISCORD_MEDIA_PROBE")
            .env("NOTDISCORD_NATIVE_ROOT", native)
            .env("NOTDISCORD_RUST_ADDON", addon)
            .env("NOTDISCORD_MEDIA_PIPE", &name)
            .env("NOTDISCORD_VIDEO_PIPE", &video_name)
            .env("NOTDISCORD_VIDEO_NAMESPACE",video_namespace.to_string())
            .env("NOTDISCORD_MEDIA_DATA", data.join("media"))
            .current_dir(root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x08000000)
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "No se pudo abrir el host multimedia.")?;
        let expected = process.id().ok_or("El host multimedia terminó.")?;
        let job = ProcessJob::attach(expected)?;
        tokio::select! {
            status=process.wait()=>return Err(early_host_exit(status)),
            connected=tokio::time::timeout(Duration::from_secs(15),server.connect())=>{
                connected.map_err(|_|"El host multimedia no respondió.")?.map_err(|_|"Se cerró el canal multimedia.")?;
            }
        }
        let mut client = 0;
        if unsafe { GetNamedPipeClientProcessId(server.as_raw_handle(), &mut client) } == 0
            || client != expected
        {
            let _ = process.kill().await;
            return Err("El proceso multimedia no corresponde a esta sesión.".into());
        }
        let (read, write) = tokio::io::split(server);
        let mut reader = BufReader::new(read);
        let ready=tokio::select! {
            status=process.wait()=>return Err(early_host_exit(status)),
            ready=tokio::time::timeout(Duration::from_secs(10),read_frame(&mut reader))=>{
                ready.map_err(|_|"El host multimedia no confirmó el inicio.")??.ok_or("El host multimedia se cerró.")?
            }
        };
        validate_ready(&ready)?;
        let _ = events.send(ready);
        let (tx, rx) = mpsc::channel::<Request>(16);
        let close = tokio_util::sync::CancellationToken::new();
        let video_close = close.clone();
        let frames = video_frames.clone();
        let video_events = events.clone();
        let (video_failure_tx,video_failure_rx)=oneshot::channel::<String>();
        tokio::spawn(async move {
            let result = tokio::select! {_=video_close.cancelled()=>Ok(()),result=read_video_lane_scoped(video_server,expected,frames.clone(),ctx,video_namespace)=>result};
            clear_video_namespace(&frames,video_namespace);
            if let Err(message)=result && !video_close.is_cancelled() {
                if video_namespace!=0 {let _=video_failure_tx.send(message);}
                else {let _=video_events.send(json!({"event":"videoError","message":message,"error":{"code":"VIDEO_PIPE_FAILED"}}));}
            }
        });
        let closed = close.clone();
        let frames = video_frames.clone();
        tokio::spawn(async move {
            let mut reason=tokio::select! {
                _=closed.cancelled()=>ControlExit::new("HOST_STOP_REQUESTED","Se cerró el receptor solicitado.",true),
                reason=serve_scoped(reader,write,rx,events.clone(),Some(frames.clone()),video_namespace,(video_namespace!=0).then_some(video_failure_rx))=>reason,
            };
            closed.cancel();
            clear_video_namespace(&frames,video_namespace);
            let mut forced=false;
            let status=match tokio::time::timeout(Duration::from_secs(3),process.wait()).await {
                Ok(Ok(status))=>Some(status),
                Ok(Err(_))=>None,
                Err(_)=>{forced=true;let _=process.kill().await;None},
            };
            drop(job);
            if !reason.intentional {
                let exit_code=status.and_then(|status|status.code());
                let code=if exit_code.is_some_and(|code|code!=0) {"HOST_PROCESS_EXIT"}else{reason.code};
                let fatal=json!({"event":"fatal","error":{"code":code,"message":reason.message,"controlCode":reason.code,"exitCode":exit_code,"forcedStop":forced}});
                crate::diagnostics::media_event(&fatal);
                let _=events.send(fatal);
                // A request failure and its fatal event travel through separate
                // actor tasks. Return the observed exit reason through both so
                // whichever wins cannot replace it with a generic IPC error.
                if !reason.pending.is_empty() {
                    let message=if code=="HOST_PROCESS_EXIT" {
                        format!("El proceso receptor terminó con código 0x{:08X}.",exit_code.unwrap_or(0) as u32)
                    } else {reason.message};
                    for done in reason.pending.drain(..) {let _=done.send(Err(message.clone()));}
                }
            }
        });
        Ok(Self {
            commands: Arc::new(tx),
            close,
            video_frames,
            video_namespace,
        })
    }
    pub async fn command(&self, kind: &str, args: Value) -> Result<Value, String> {
        if self.close.is_cancelled() {
            return Err("El host multimedia se cerró.".into());
        }
        prepare_video_request(kind,&args,Some(&self.video_frames),self.video_namespace)?;
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(Request {
                kind: kind.into(),
                args,
                done: tx,
            })
            .await
            .map_err(|_| "El host multimedia no está disponible.")?;
        rx.await
            .map_err(|_| "El host multimedia se cerró.".to_owned())?
    }
    #[cfg(test)]
    pub(crate) fn fake_receiver<F>(respond:F,frames:SharedVideoFrames)->(Self,Arc<std::sync::Mutex<Vec<String>>>)
    where F:FnMut(&str,&Value)->Result<Value,String>+Send+'static {
        let (mut bridge,calls)=Self::fake(respond);bridge.video_frames=frames;
        bridge.video_namespace=NEXT_VIDEO_NAMESPACE.fetch_add(1,std::sync::atomic::Ordering::Relaxed)<<32;(bridge,calls)
    }
    #[cfg(test)]
    pub(crate) fn fake_video_id(&self)->u64 {self.video_namespace+1}
    #[cfg(test)]
    pub(crate) fn fake<F>(mut respond: F) -> (Self, Arc<std::sync::Mutex<Vec<String>>>)
    where
        F: FnMut(&str, &Value) -> Result<Value, String> + Send + 'static,
    {
        let (tx, mut rx) = mpsc::channel::<Request>(16);
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = calls.clone();
        tokio::spawn(async move {
            while let Some(r) = rx.recv().await {
                record.lock().unwrap().push(r.kind.clone());
                let value = respond(&r.kind, &r.args);
                let _ = r.done.send(value);
            }
        });
        (
            Self {
                commands: Arc::new(tx),
                close: tokio_util::sync::CancellationToken::new(),
                video_frames: Arc::new(std::sync::Mutex::new(LatestFrames::default())),
                video_namespace:0,
            },
            calls,
        )
    }
}
#[cfg(test)]
async fn read_video_lane(
    server: tokio::net::windows::named_pipe::NamedPipeServer,
    expected: u32,
    frames: SharedVideoFrames,
    ctx: eframe::egui::Context,
) -> Result<(), String> {
    read_video_lane_scoped(server,expected,frames,ctx,0).await
}
async fn read_video_lane_scoped(
    mut server: tokio::net::windows::named_pipe::NamedPipeServer,
    expected:u32,frames:SharedVideoFrames,ctx:eframe::egui::Context,video_namespace:u64,
)->Result<(),String> {
    tokio::time::timeout(Duration::from_secs(15), server.connect())
        .await
        .map_err(|_| "No respondió el canal de vídeo.")?
        .map_err(|_| "Se cerró el canal de vídeo.")?;
    let mut client = 0;
    if unsafe { GetNamedPipeClientProcessId(server.as_raw_handle(), &mut client) } == 0
        || client != expected
    {
        return Err("El proceso del canal de vídeo no corresponde a esta sesión.".into());
    }
    loop {
        let mut bytes = [0; HEADER_LEN];
        server
            .read_exact(&mut bytes)
            .await
            .map_err(|_| "Se cerró el canal de vídeo.")?;
        let header = decode_header(&bytes)?;
        let allowed = header.subscription_id>>32==video_namespace>>32 && frames
            .lock()
            .unwrap()
            .active_ids()
            .contains(&header.subscription_id);
        if !allowed {
            let copied = tokio::io::copy(
                &mut (&mut server).take(header.len as u64),
                &mut tokio::io::sink(),
            )
            .await
            .map_err(|_| "Fotograma incompleto.")?;
            if copied != header.len as u64 {
                return Err("Fotograma incompleto.".into());
            }
            continue;
        }
        let mut rgba = vec![0; header.len as usize];
        server
            .read_exact(&mut rgba)
            .await
            .map_err(|_| "Fotograma incompleto.")?;
        let accepted = frames.lock().unwrap().publish(VideoFrame {
            subscription_id: header.subscription_id,
            sequence: header.sequence,
            width: header.width,
            height: header.height,
            rgba,
        });
        if accepted {
            ctx.request_repaint();
        }
    }
}
#[cfg(test)]
fn emit_event(
    value: Value,
    events: &mpsc::UnboundedSender<Value>,
    frames: Option<&SharedVideoFrames>,
) {
    emit_scoped_event(value,events,frames,0);
}
fn clear_video_namespace(frames:&SharedVideoFrames,namespace:u64) {
    let mut frames=frames.lock().unwrap();
    for id in frames.active_ids().into_iter().filter(|id|id>>32==namespace>>32) {frames.remove(id);}
}
fn emit_scoped_event(value:Value,events:&mpsc::UnboundedSender<Value>,frames:Option<&SharedVideoFrames>,namespace:u64) {
    crate::diagnostics::media_event(&value);
    if value["event"] == "videoStream" {
        if let (Some(frames), Some(id)) = (frames, value["id"].as_u64()) {
            if id>>32!=namespace>>32 {return;}
            let mut frames = frames.lock().unwrap();
            if value["active"] == true || matches!(value["state"].as_str(),Some("loading"|"waiting"|"starting")) {
                frames.activate(id);
            } else if value["state"]!="paused" {
                frames.remove(id);
            }
            // A temporary native/DAVE suspension can race an explicit enable.
            // Keep its registration; explicit hide removes it before the command,
            // and removed/error metadata or receiver teardown releases it.
        }
    }
    let _ = events.send(value);
}
fn prepare_video_request(kind:&str,args:&Value,frames:Option<&SharedVideoFrames>,namespace:u64)->Result<(),String> {
    if kind!="setVideoStreamVisible" {return Ok(());}
    let id=args["id"].as_u64().filter(|id|*id>0&&*id>>32==namespace>>32).ok_or("La fuente de vídeo ya no pertenece a este receptor.")?;
    let visible=args["visible"].as_bool().ok_or("La visibilidad de vídeo no es válida.")?;
    if let Some(frames)=frames {
        let mut frames=frames.lock().unwrap();
        if visible {
            if !frames.activate(id) {return Err("Cierra otro vídeo antes de abrir esta fuente.".into());}
        } else {frames.remove(id);}
    }
    Ok(())
}
fn resolve(root: &Path, path: &str) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(path);
    if path.is_relative() {
        root.join(path)
    } else {
        path
    }
}
fn normal_path(path: &Path) -> std::path::PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        std::path::PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        std::path::PathBuf::from(rest)
    } else {
        path.to_owned()
    }
}
fn validate_ready(v: &Value) -> Result<(), String> {
    if v["event"] != "ready" || v["protocol"] != 1 || v["nativeInitialized"] != false {
        return Err("Contrato multimedia incompatible.".into());
    }
    Ok(())
}
async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Value>, String> {
    let mut bytes = Vec::with_capacity(2048);
    read_frame_buffered(reader, &mut bytes).await
}
async fn read_frame_buffered<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    bytes: &mut Vec<u8>,
) -> Result<Option<Value>, String> {
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|_| "El host multimedia se cerró.")?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err("Trama multimedia incompleta.".into())
            };
        }
        let newline = available.iter().position(|b| *b == b'\n');
        let length = newline.unwrap_or(available.len());
        if bytes.len() + length > MAX_RESPONSE {
            return Err("Respuesta multimedia demasiado grande.".into());
        }
        bytes.extend_from_slice(&available[..length]);
        reader.consume(length + usize::from(newline.is_some()));
        if newline.is_some() {
            let result = serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| "Respuesta multimedia no válida.".into());
            bytes.clear();
            return result;
        }
    }
}
#[cfg(test)]
async fn serve<R, W>(
    reader: BufReader<R>,
    writer: W,
    requests: mpsc::Receiver<Request>,
    events: mpsc::UnboundedSender<Value>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    serve_with_video(reader, writer, requests, events, None).await
}
#[cfg(test)]
async fn serve_with_video<R, W>(
    reader: BufReader<R>,
    writer: W,
    requests: mpsc::Receiver<Request>,
    events: mpsc::UnboundedSender<Value>,
    frames: Option<SharedVideoFrames>,
) where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reason=serve_scoped(reader,writer,requests,events,frames,0,None).await;
    for done in reason.pending.drain(..) {let _=done.send(Err(reason.message.clone()));}
}
async fn read_video_failure(receiver:&mut Option<oneshot::Receiver<String>>)->String {
    if let Some(receiver)=receiver.as_mut() && let Ok(message)=receiver.await {return message;}
    *receiver=None;
    std::future::pending().await
}
async fn serve_scoped<R,W>(mut reader:BufReader<R>,mut writer:W,mut requests:mpsc::Receiver<Request>,events:mpsc::UnboundedSender<Value>,frames:Option<SharedVideoFrames>,video_namespace:u64,mut video_failure:Option<oneshot::Receiver<String>>)->ControlExit
where R:AsyncRead+Unpin,W:AsyncWrite+Unpin {
    let mut sequence = 0u64;
    let mut pending = Vec::with_capacity(2048);
    let mut reason='serve:loop {
        tokio::select! {
         message=read_video_failure(&mut video_failure)=>break ControlExit::new("VIDEO_PIPE_FAILED",message,false),
         request=requests.recv()=>{let Some(request)=request else{break ControlExit::new("HOST_COMMANDS_CLOSED","El canal de órdenes se cerró.",true)};sequence+=1;let id=sequence.to_string();let mut bytes=json!({"id":id,"command":request.kind,"params":request.args}).to_string().into_bytes();if bytes.len()>MAX_REQUEST{let _=request.done.send(Err("Solicitud multimedia demasiado grande.".into()));continue}
          // Authorize immediately before writing the enable command. The first
          // raw frame may precede its JSON response on the independent lane.
          if let Err(error)=prepare_video_request(&request.kind,&request.args,frames.as_ref(),video_namespace){let _=request.done.send(Err(error));continue}
          bytes.push(b'\n');let written=tokio::select! {
            written=writer.write_all(&bytes)=>written,
            message=read_video_failure(&mut video_failure)=>break 'serve ControlExit::new("VIDEO_PIPE_FAILED",message,false).with_pending(request.done),
          };
          if written.is_err(){break ControlExit::new("HOST_CONTROL_WRITE_FAILED","No se pudo entregar la orden al receptor.",false).with_pending(request.done)}
          // Outer errors mean damaged transport. Inner errors are valid helper replies.
          let response=tokio::select! {
            response=tokio::time::timeout(Duration::from_secs(35),async{loop{let value=read_frame_buffered(&mut reader,&mut pending).await?.ok_or("El host multimedia se cerró.".to_owned())?;if value["id"].as_str()==Some(&id){return Ok::<_,String>(if value["ok"]==true{Ok(value["result"].clone())}else if value["ok"]==false{Err("No se pudo completar la operación multimedia.".into())}else{return Err("Respuesta multimedia no válida.".into())})}if value["event"].is_string(){emit_scoped_event(value,&events,frames.as_ref(),video_namespace);}}})=>response,
            message=read_video_failure(&mut video_failure)=>break 'serve ControlExit::new("VIDEO_PIPE_FAILED",message,false).with_pending(request.done),
          };
          let(terminal,result)=match response {
            Ok(Ok(result))=>(None,result),
            Ok(Err(error))=>(Some(ControlExit::new("HOST_CONTROL_READ_FAILED",error.clone(),false)),Err(error)),
            Err(_)=>(Some(ControlExit::new("HOST_CONTROL_TIMEOUT","El receptor no confirmó una orden a tiempo.",false)),Err("El host multimedia agotó el tiempo de espera.".into())),
          };
          let shutdown=request.kind=="shutdown";
          if let Some(reason)=terminal {break reason.with_pending(request.done)}
          let _=request.done.send(result);
          if shutdown {break ControlExit::new("HOST_SHUTDOWN","El receptor confirmó su cierre.",true)}
         },
         frame=read_frame_buffered(&mut reader,&mut pending)=>match frame {
            Ok(Some(value))if value["event"].is_string()=>{emit_scoped_event(value,&events,frames.as_ref(),video_namespace);},
            Ok(Some(_))=>{},
            Ok(None)=>break ControlExit::new("HOST_CONTROL_CLOSED","El receptor cerró el canal de control sin solicitarlo.",false),
            Err(error)=>break ControlExit::new("HOST_CONTROL_READ_FAILED",error,false),
         },
        }
    };
    requests.close();
    while let Ok(request)=requests.try_recv() {reason.pending.push(request.done);}
    let _ = writer.shutdown().await;
    reason
}
#[repr(C)]
struct SecurityAttributes {
    length: u32,
    descriptor: *mut std::ffi::c_void,
    inherit: i32,
}
#[repr(C)]
struct TokenUser {
    sid: *mut std::ffi::c_void,
    attributes: u32,
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        s: *const u16,
        version: u32,
        descriptor: *mut *mut std::ffi::c_void,
        size: *mut u32,
    ) -> i32;
    fn OpenProcessToken(
        process: *mut std::ffi::c_void,
        access: u32,
        token: *mut *mut std::ffi::c_void,
    ) -> i32;
    fn GetTokenInformation(
        token: *mut std::ffi::c_void,
        class: u32,
        information: *mut std::ffi::c_void,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *mut std::ffi::c_void, string: *mut *mut u16) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(p: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn OpenProcess(access: u32, inherit: i32, id: u32) -> *mut std::ffi::c_void;
    fn GetNamedPipeClientProcessId(pipe: *mut std::ffi::c_void, id: *mut u32) -> i32;
    fn CreateJobObjectW(
        attributes: *mut std::ffi::c_void,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        class: u32,
        information: *const std::ffi::c_void,
        size: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
}
fn current_user_sid() -> Result<String, String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), 8, &mut token) == 0 {
            return Err("No se pudo identificar al usuario del canal multimedia.".into());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut needed = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            1,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
        if needed < std::mem::size_of::<TokenUser>() as u32 || needed > 65536 {
            return Err("Identidad local no válida.".into());
        }
        let mut buffer = vec![0u8; needed as usize];
        if GetTokenInformation(
            token.as_raw_handle(),
            1,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err("No se pudo proteger el canal multimedia.".into());
        }
        let user = std::ptr::read_unaligned(buffer.as_ptr().cast::<TokenUser>());
        let mut sid = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.sid, &mut sid) == 0 {
            return Err("Identidad local no válida.".into());
        }
        let mut len = 0;
        while len < 256 && *sid.add(len) != 0 {
            len += 1
        }
        let value = String::from_utf16(std::slice::from_raw_parts(sid, len));
        LocalFree(sid.cast());
        value.map_err(|_| "Identidad local no válida.".into())
    }
}
fn private_pipe(name: &str) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, String> {
    let sid = current_user_sid()?;
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err("No se pudo proteger el canal multimedia.".into());
    }
    let mut security = SecurityAttributes {
        length: std::mem::size_of::<SecurityAttributes>() as u32,
        descriptor,
        inherit: 0,
    };
    let server = unsafe {
        ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                name,
                (&mut security as *mut SecurityAttributes).cast(),
            )
    };
    unsafe {
        LocalFree(descriptor);
    }
    server.map_err(|_| "No se pudo crear el canal multimedia privado.".into())
}
#[repr(C)]
#[derive(Default)]
struct BasicLimit {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min_working_set: usize,
    max_working_set: usize,
    active_processes: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimit {
    basic: BasicLimit,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process_memory: usize,
    peak_job_memory: usize,
}
struct ProcessJob {
    _handle: OwnedHandle,
}
impl ProcessJob {
    fn attach(pid: u32) -> Result<Self, String> {
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if handle.is_null() {
                return Err("No se pudo proteger el proceso multimedia.".into());
            }
            let job = OwnedHandle::from_raw_handle(handle);
            let mut limit = ExtendedLimit::default();
            limit.basic.flags = 0x2000;
            if SetInformationJobObject(
                job.as_raw_handle(),
                9,
                (&limit as *const ExtendedLimit).cast(),
                std::mem::size_of::<ExtendedLimit>() as u32,
            ) == 0
            {
                return Err("No se pudo proteger el proceso multimedia.".into());
            }
            let process = OpenProcess(0x101, 0, pid);
            if process.is_null() {
                return Err("El proceso multimedia no está disponible.".into());
            }
            let process = OwnedHandle::from_raw_handle(process);
            if AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) == 0 {
                return Err("No se pudo asociar el proceso multimedia a su sesión.".into());
            }
            Ok(Self { _handle: job })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn receiver_shutdown_and_wrong_scope_metadata_never_remove_main_call_frames() {
        let (main,_)=MediaBridge::fake(|_,_|Ok(json!({})));
        let frames=main.video_frames();let (receiver,_)=MediaBridge::fake_receiver(|_,_|Ok(json!({})),frames.clone());
        let receive_id=receiver.fake_video_id();
        {let mut frames=frames.lock().unwrap();assert!(frames.activate(1));assert!(frames.activate(receive_id));}
        let (events,mut values)=mpsc::unbounded_channel();
        emit_scoped_event(json!({"event":"videoStream","id":1,"state":"removed","active":false}),&events,Some(&frames),receiver.video_namespace);
        assert!(values.try_recv().is_err());assert!(frames.lock().unwrap().active_ids().contains(&1));
        receiver.close();assert_eq!(frames.lock().unwrap().active_ids(),vec![1]);
        assert!(!main.close.is_cancelled());assert!(receiver.close.is_cancelled());
        main.command("stats",json!({})).await.unwrap();
        let (reopened,_)=MediaBridge::fake_receiver(|_,_|Ok(json!({})),frames.clone());
        assert_ne!(receive_id,reopened.fake_video_id());assert!(frames.lock().unwrap().activate(reopened.fake_video_id()));
        receiver.close();assert!(frames.lock().unwrap().active_ids().contains(&reopened.fake_video_id()));
    }
    #[tokio::test]
    async fn binary_video_lane_checks_pid_allowlist_latest_frame_and_detach() {
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = format!(
            r"\\.\pipe\NotDiscord.Media.{}.Frames",
            uuid::Uuid::new_v4().simple()
        );
        let server = private_pipe(&name).unwrap();
        let mut client = ClientOptions::new().open(&name).unwrap();
        let frames: SharedVideoFrames = Default::default();
        frames.lock().unwrap().activate(7);
        let target = frames.clone();
        let task = tokio::spawn(read_video_lane(
            server,
            std::process::id(),
            target,
            eframe::egui::Context::default(),
        ));
        for (id, sequence, color) in [(8, 1, 9), (7, 1, 1), (7, 2, 2)] {
            let frame = VideoFrame {
                subscription_id: id,
                sequence,
                width: 2,
                height: 1,
                rgba: vec![color; 8],
            };
            client
                .write_all(&frame.header().unwrap().encode_header())
                .await
                .unwrap();
            client.write_all(&frame.rgba).await.unwrap();
        }
        let delivered = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(frame) = frames
                    .lock()
                    .unwrap()
                    .drain()
                    .into_iter()
                    .find(|frame| frame.sequence == 2)
                {
                    break frame;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(delivered.subscription_id, 7);
        assert_eq!(delivered.rgba, [2; 8]);
        let (events, _) = mpsc::unbounded_channel();
        emit_event(
            json!({"event":"videoStream","id":7,"active":false,"state":"removed"}),
            &events,
            Some(&frames),
        );
        let frame = VideoFrame {
            subscription_id: 7,
            sequence: 3,
            width: 2,
            height: 1,
            rgba: vec![3; 8],
        };
        client
            .write_all(&frame.header().unwrap().encode_header())
            .await
            .unwrap();
        client.write_all(&frame.rgba).await.unwrap();
        drop(client);
        assert!(task.await.unwrap().is_err());
        assert_eq!(frames.lock().unwrap().buffered_bytes(), 0);
        assert!(frames.lock().unwrap().active_ids().is_empty());
    }
    #[test]
    fn ready_requires_protocol_and_no_init() {
        assert!(
            validate_ready(&json!({"event":"ready","protocol":1,"nativeInitialized":false}))
                .is_ok()
        );
        assert!(
            validate_ready(&json!({"event":"ready","protocol":1,"nativeInitialized":true}))
                .is_err()
        );
        assert!(
            validate_ready(&json!({"event":"ready","protocol":2,"nativeInitialized":false}))
                .is_err()
        );
    }
    #[tokio::test]
    async fn bounds_are_checked_before_line_allocation() {
        let (data, mut tx) = tokio::io::duplex(1024);
        tokio::spawn(async move {
            let bytes = vec![b'x'; MAX_RESPONSE + 1];
            let _ = tx.write_all(&bytes).await;
        });
        assert!(
            read_frame(&mut BufReader::new(data))
                .await
                .unwrap_err()
                .contains("grande")
        );
    }
    #[tokio::test]
    async fn incomplete_or_invalid_utf8_frame_rejected() {
        for bytes in [b"{\"id\":1}".to_vec(), vec![255, b'\n']] {
            let mut input = BufReader::new(&bytes[..]);
            assert!(read_frame(&mut input).await.is_err());
        }
    }
    #[tokio::test]
    async fn operation_rejection_preserves_next_command_and_events() {
        let (client, server) = tokio::io::duplex(4096);
        let (read, write) = tokio::io::split(client);
        let (tx, rx) = mpsc::channel(16);
        let (events, mut notices) = mpsc::unbounded_channel();
        let work = tokio::spawn(serve(BufReader::new(read), write, rx, events));
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(server);
            let mut read = BufReader::new(read);
            for fail in [true, false] {
                let request = read_frame(&mut read).await.unwrap().unwrap();
                let frame = if fail {
                    json!({"id":request["id"],"ok":false,"error":{"code":"CAMERA_NOT_LISTED"}})
                } else {
                    json!({"id":request["id"],"ok":true,"result":{"muted":true}})
                };
                write
                    .write_all(b"{\"event\":\"speaking\",\"active\":false}\n")
                    .await
                    .unwrap();
                write
                    .write_all(format!("{frame}\n").as_bytes())
                    .await
                    .unwrap();
            }
        });
        let bridge = MediaBridge {
            commands: Arc::new(tx),
            close: tokio_util::sync::CancellationToken::new(),
            video_frames: Default::default(),
            video_namespace:0,
        };
        assert!(bridge.command("startCamera", json!({})).await.is_err());
        assert_eq!(
            bridge.command("mute", json!({"value":true})).await.unwrap()["muted"],
            true
        );
        assert_eq!(notices.recv().await.unwrap()["event"], "speaking");
        drop(bridge);
        work.await.unwrap();
    }
    #[tokio::test]
    async fn malformed_reply_closes_transport_and_pending_commands() {
        let (client, server) = tokio::io::duplex(4096);
        let (read, write) = tokio::io::split(client);
        let (tx, rx) = mpsc::channel(16);
        let (events, _) = mpsc::unbounded_channel();
        let task = tokio::spawn(serve(BufReader::new(read), write, rx, events));
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(server);
            let mut read = BufReader::new(read);
            let _ = read_frame(&mut read).await;
            write.write_all(b"bad-json\n").await.unwrap();
        });
        let bridge = MediaBridge {
            commands: Arc::new(tx),
            close: tokio_util::sync::CancellationToken::new(),
            video_frames: Default::default(),
            video_namespace:0,
        };
        assert!(bridge.command("stats", json!({})).await.is_err());
        task.await.unwrap();
        assert!(bridge.command("stats", json!({})).await.is_err());
    }
    #[tokio::test]
    async fn private_pipe_accepts_current_process_and_excludes_remote() {
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = format!(
            r"\\.\pipe\NotDiscord.Media.{}",
            uuid::Uuid::new_v4().simple()
        );
        let server = private_pipe(&name).unwrap();
        let client = ClientOptions::new().open(&name).unwrap();
        server.connect().await.unwrap();
        let mut id = 0;
        assert_ne!(
            unsafe { GetNamedPipeClientProcessId(server.as_raw_handle(), &mut id) },
            0
        );
        assert_eq!(id, std::process::id());
        drop(client);
    }
    #[test]
    fn relative_distribution_paths_resolve_under_root() {
        let root = std::path::PathBuf::from(r"C:\NotDiscord");
        assert_eq!(
            resolve(&root, "media-runtime/NotDiscord.Media.exe"),
            root.join("media-runtime/NotDiscord.Media.exe")
        );
        assert_eq!(
            resolve(&root, r"D:\Runtime\Media.exe"),
            std::path::PathBuf::from(r"D:\Runtime\Media.exe")
        );
    }
    #[tokio::test]
    async fn fragmented_event_survives_cancelled_read_when_command_arrives() {
        let (client, server) = tokio::io::duplex(4096);
        let (read, write) = tokio::io::split(client);
        let (tx, rx) = mpsc::channel(16);
        let (events, mut notices) = mpsc::unbounded_channel();
        let task = tokio::spawn(serve(BufReader::new(read), write, rx, events));
        let (ready, fragment) = oneshot::channel();
        tokio::spawn(async move {
            let (read, mut write) = tokio::io::split(server);
            write.write_all(b"{\"event\":\"speak").await.unwrap();
            let _ = ready.send(());
            let mut read = BufReader::new(read);
            let request = read_frame(&mut read).await.unwrap().unwrap();
            write.write_all(b"ing\",\"active\":false}\n").await.unwrap();
            write
                .write_all(
                    format!(
                        "{}\n",
                        json!({"id":request["id"],"ok":true,"result":{"ok":true}})
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        fragment.await.unwrap();
        tokio::task::yield_now().await;
        let bridge = MediaBridge {
            commands: Arc::new(tx),
            close: tokio_util::sync::CancellationToken::new(),
            video_frames: Default::default(),
            video_namespace:0,
        };
        assert_eq!(
            bridge.command("stats", json!({})).await.unwrap()["ok"],
            true
        );
        assert_eq!(notices.recv().await.unwrap()["event"], "speaking");
        drop(bridge);
        task.await.unwrap();
    }
    #[tokio::test]
    #[ignore = "requires prepared third-party Electron runtime; no devices or network"]
    async fn packaged_rust_host_idle_gate() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        let data = temporary.join(format!(
            "notdiscord-host-rust-gate-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&data).unwrap();
        let (events, mut rx) = mpsc::unbounded_channel();
        let bridge = MediaBridge::start(&root, &data, events, eframe::egui::Context::default())
            .await
            .unwrap();
        let capabilities = bridge.command("capabilities", json!({})).await.unwrap();
        assert!(capabilities["voice"] == true && capabilities["dave"].as_u64().unwrap_or(0) >= 1);
        let stats = bridge.command("stats", json!({})).await.unwrap();
        assert!(stats["voice"].is_null() && stats["share"].is_null());
        assert_eq!(
            bridge.command("shutdown", json!({})).await.unwrap()["state"],
            "closed"
        );
        while let Ok(value) = rx.try_recv() {
            assert_ne!(value["event"], "mediaStage");
        }
        drop(bridge);
        tokio::time::sleep(Duration::from_millis(150)).await;
        let resolved = data.canonicalize().unwrap();
        if resolved.parent() == Some(temporary.as_path())
            && resolved.file_name().is_some_and(|n| {
                n.to_string_lossy()
                    .starts_with("notdiscord-host-rust-gate-")
            })
        {
            let _ = std::fs::remove_dir_all(&resolved);
        }
    }
}
