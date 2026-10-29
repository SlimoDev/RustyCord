//! Bounded, opt-in-to-export operational records. No free-form error messages,
//! account identifiers, URLs, content, command lines or panic payloads enter this API.
use serde::Serialize;
use std::{collections::{HashMap,VecDeque}, fs::{self, File, OpenOptions}, io::{Read,Seek,SeekFrom,Write}, path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc::{self, SyncSender}}, time::{Duration, Instant, SystemTime}};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Component { App, Controller, Voice, Media, Menus, Notifications, Sound, Input, Gifs }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Code { Started, CleanShutdown, PreviousUnclean, Recoverable, TaskFailed, TaskCancelled, TaskTimeout,
    HostExited, TransportFailure, HeartbeatTimeout, QueueSaturated, MenuOpened, MenuDemand,
    VisibilityChanged, FocusChanged, ToastQueued, ToastActivated, SoundQueued, Panic,
    TransportHttp, TransportTls, TransportIo, TransportCapacity, TransportProtocol,
    TransportClosed, TransportAlreadyClosed, TransportUtf8, TransportEndpoint, TransportFormat,
    ControlReadFailed, ControlWriteFailed, ControlTimeout, VideoPipeFailed, AppStartFailed, HostForcedStop, CallSourcesState }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Phase { Start, Stop, Connect, Read, Write, Enqueue, Heartbeat, Dispatch, Present, Persist, Unknown }
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    schema: u8, utc: String, monotonic_ms: u64, sequence: u64, version: String,
    build: Option<&'static str>, component: Component, code: Code, phase: Phase,
    correlation: u64, severity: &'static str, repetitions: u32, dropped_before: u64,
}
struct Recorder { sender: SyncSender<Event>, ring: Mutex<VecDeque<Event>>, start: Instant,
    version: String, sequence: AtomicU64, dropped: AtomicU64, closing: Arc<AtomicBool>,
    emergency: Mutex<File>, panic_written: AtomicBool, disk_failed:AtomicBool, directory: PathBuf }
static RECORDER: OnceLock<Arc<Recorder>> = OnceLock::new();
const PREFIX: &str = "nddiag-";
const FILE_LIMIT: u64 = 1024 * 1024;
const TOTAL_LIMIT: u64 = 16 * FILE_LIMIT;

/// State/counts only: no owner, channel, device, URL or message enters a record.
/// Bits 0..7: local camera/screen, preparation, crypto, voice, known announcements.
/// Bytes 1..5: remote camera, screen, both, unknown camera, unknown screen counts.
/// Byte 6: video descriptors. Bits 56..62: participants; bit 63 is reserved for UI visibility.
pub fn call_sources_summary(state: &crate::models::AppSnapshot) -> u64 {
    let own = state.call_participants.iter().find(|person| person.is_self);
    let flags = [state.camera_enabled, state.screen_sharing, state.camera_preparing,
        state.screen_preparing, state.voice_encryption_ready, state.voice_connected,
        own.is_some_and(|person| person.camera_announcement_known),
        own.is_some_and(|person| person.screen_announcement_known)];
    let mut bits = flags.iter().enumerate().fold(0, |bits, (index, flag)| bits | ((*flag as u64) << index));
    let mut counts = [0u64; 5];
    for person in state.call_participants.iter().filter(|person| !person.is_self) {
        let camera = crate::models::call_source_available(Some(person), &person.user.id, "camera", &state.call_videos);
        let screen = crate::models::call_source_available(Some(person), &person.user.id, "screen", &state.call_videos);
        for (count, present) in counts.iter_mut().zip([camera, screen, camera && screen,
            !person.camera_announcement_known, !person.screen_announcement_known]) {
            *count += present as u64;
        }
    }
    for (index, count) in counts.iter().enumerate() { bits |= (*count).min(255) << (8 * (index + 1)); }
    bits | ((state.call_videos.len().min(255) as u64) << 48)
        | ((state.call_participants.len().min(127) as u64) << 56)
}

fn own_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix(PREFIX) else { return false; };
    let Some((id, suffix)) = rest.split_once('.') else { return false; };
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        && matches!(suffix, "jsonl" | "run" | "panic.jsonl")
}
fn no_links(path: &Path) -> bool {
    path.ancestors().all(|p| fs::symlink_metadata(p).map_or(true, |m| {
        #[cfg(windows)] { use std::os::windows::fs::MetadataExt; m.file_attributes() & 0x400 == 0 }
        #[cfg(not(windows))] { !m.file_type().is_symlink() }
    }))
}
fn create_private(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new(); options.create_new(true).write(true);
    #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.share_mode(1); }
    options.open(path)
}
const OPEN_MARKER:&[u8]=b"NotDiscord.Standard.Diagnostics/1\nopen\n";
const CLOSED_MARKER:&[u8]=b"NotDiscord.Standard.Diagnostics/1\nclosed\n";
fn own_marker(marker:&Path)->Option<bool>{
    if !no_links(marker) || fs::metadata(marker).ok()?.len()>96{return None;}
    let mut bytes=Vec::new();File::open(marker).ok()?.take(97).read_to_end(&mut bytes).ok()?;
    if bytes==OPEN_MARKER{Some(false)}else if bytes==CLOSED_MARKER{Some(true)}else{None}
}
fn claim(marker: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new(); options.read(true).write(true);
    #[cfg(windows)] { use std::os::windows::fs::OpenOptionsExt; options.share_mode(4); }
    options.open(marker)
}
fn prepare(directory: &Path) -> std::io::Result<bool> {
    if !no_links(directory) { return Err(std::io::ErrorKind::PermissionDenied.into()); }
    fs::create_dir_all(directory)?;
    let mut groups:HashMap<String,Vec<(PathBuf,u64,SystemTime)>>=HashMap::new();let mut scanned=0;
    for entry in fs::read_dir(directory)?.take(1025) {
        scanned+=1;if scanned>1024{return Err(std::io::ErrorKind::StorageFull.into());}
        let entry = entry?; let name = entry.file_name(); let Some(name) = name.to_str() else { continue; };
        if !own_name(name) || !no_links(&entry.path()) { continue; }
        let meta = entry.metadata()?; if !meta.is_file() { continue; }
        let stem = name.strip_prefix(PREFIX).unwrap().split('.').next().unwrap();
        groups.entry(stem.into()).or_default().push((entry.path(),meta.len(),meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
    }
    if groups.len()>=256 {return Err(std::io::ErrorKind::StorageFull.into());}
    let mut owned=Vec::new();let mut total=0u64;let mut unclean=false;
    for (id,files) in groups{
        let marker=directory.join(format!("{PREFIX}{id}.run"));
        let Some(closed)=own_marker(&marker)else{continue;};
        let active=claim(&marker).is_err();
        if !closed && !active{unclean=true;}
        let bytes=files.iter().fold(0u64,|sum,file|sum.saturating_add(file.1));
        // Reserve the remaining writer budget of active executions, not just
        // the bytes already on disk, even for overlapping explicit previews.
        let bytes=if active{bytes.max(FILE_LIMIT+4096+96)}else{bytes};total=total.saturating_add(bytes);
        let modified=files.iter().map(|file|file.2).max().unwrap_or(SystemTime::UNIX_EPOCH);
        owned.push((modified,marker,files,bytes,active));
    }
    owned.sort_by_key(|group|group.0);let mut count=owned.len();
    for (modified, marker, files, bytes, active) in owned {
        let old = modified.elapsed().unwrap_or_default() > Duration::from_secs(14*24*60*60);
        if !active && (old || count>=128 || total.saturating_add(2*FILE_LIMIT)>TOTAL_LIMIT){
            let Ok(mut lease)=claim(&marker)else{continue;};
            let mut marker_bytes=Vec::new();
            if (&mut lease).take(97).read_to_end(&mut marker_bytes).is_err() || marker_bytes!=OPEN_MARKER && marker_bytes!=CLOSED_MARKER{continue;}
            let mut removed=true;
            for (path,_,_) in files.iter().filter(|file|file.0!=marker){
                if !no_links(path) || fs::remove_file(path).is_err(){removed=false;break;}
            }
            if removed && fs::remove_file(&marker).is_ok(){total=total.saturating_sub(bytes);count=count.saturating_sub(1);}
            drop(lease);
        }
    }
    if count>=128 || total.saturating_add(2*FILE_LIMIT) > TOTAL_LIMIT { return Err(std::io::ErrorKind::StorageFull.into()); }
    Ok(unclean)
}
/// Initialisation is best effort: failure never prevents application startup.
/// The caller holds the returned guard until after controller/runtime teardown.
pub fn initialise(data: &Path, version: &str) -> Option<Session> {
    if RECORDER.get().is_some() { return None; }
    let directory = data.join("diagnostics"); let unclean = prepare(&directory).ok()?;
    let id = format!("{:032x}", rand::random::<u128>());
    let marker_path = directory.join(format!("{PREFIX}{id}.run"));
    let mut marker = create_private(&marker_path).ok()?;
    marker.write_all(OPEN_MARKER).ok()?;
    let file = create_private(&directory.join(format!("{PREFIX}{id}.jsonl"))).ok()?;
    let emergency = create_private(&directory.join(format!("{PREFIX}{id}.panic.jsonl"))).ok()?;
    let (sender, receiver) = mpsc::sync_channel(256);
    let closing = Arc::new(AtomicBool::new(false)); let closed = closing.clone();
    let (done_tx, done) = mpsc::sync_channel(1);
    let recorder = Arc::new(Recorder {sender, ring: Mutex::new(VecDeque::with_capacity(256)), start:Instant::now(),
        version:version.chars().filter(|ch|ch.is_ascii_alphanumeric() || matches!(ch,'.'|'-'|'+')).take(64).collect(),
        sequence:AtomicU64::new(0),dropped:AtomicU64::new(0),closing,emergency:Mutex::new(emergency),panic_written:AtomicBool::new(false),disk_failed:AtomicBool::new(false),directory});
    if RECORDER.set(recorder).is_err() { return None; }
    std::thread::Builder::new().name("diagnostic-writer".into()).spawn(move || {
        let mut file=file; let mut bytes=0u64; let mut failed=false;
        loop {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => if let Ok(mut line) = serde_json::to_vec(&event) {
                    line.push(b'\n');
                    if !failed && line.len() <= 4096 && bytes.saturating_add(line.len() as u64) <= FILE_LIMIT {
                        if file.write_all(&line).is_ok() { bytes+=line.len() as u64; } else {
                            failed=true;if let Some(recorder)=RECORDER.get(){recorder.disk_failed.store(true,Ordering::Relaxed);}
                        }
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) if !closed.load(Ordering::Acquire) => continue,
                Err(_) => break,
            }
        }
        let _=file.flush(); drop(file);
        if closed.load(Ordering::Acquire) {
            let _=marker.seek(SeekFrom::Start(0));let _=marker.write_all(CLOSED_MARKER);
            let _=marker.set_len(CLOSED_MARKER.len() as u64);let _=marker.flush();
        }
        drop(marker);
        let _=done_tx.try_send(());
    }).ok()?;
    record(Component::App,Code::Started,Phase::Start,0);
    if unclean { record(Component::App,Code::PreviousUnclean,Phase::Start,0); }
    Some(Session { done })
}
pub struct Session { done: mpsc::Receiver<()> }
impl Drop for Session {
    fn drop(&mut self) {
        if std::thread::panicking(){return;}
        record(Component::App,Code::CleanShutdown,Phase::Stop,0);
        if let Some(recorder)=RECORDER.get() { recorder.closing.store(true,Ordering::Release); }
        let _=self.done.recv_timeout(Duration::from_millis(250));
    }
}
pub fn record(component: Component, code: Code, phase: Phase, correlation: u64) {
    let Some(recorder)=RECORDER.get() else { return; };
    let event=Event {schema:1,utc:chrono::Utc::now().to_rfc3339(),monotonic_ms:recorder.start.elapsed().as_millis().min(u64::MAX as u128) as u64,
        sequence:recorder.sequence.fetch_add(1,Ordering::Relaxed),version:recorder.version.clone(),build:option_env!("NOTDISCORD_BUILD_ID").filter(|id|matches!(id.len(),40|64)&&id.bytes().all(|b|b.is_ascii_hexdigit())),component,code,phase,correlation,
        severity:match code {Code::Panic|Code::HostExited|Code::TaskFailed|Code::AppStartFailed=>"error",Code::Started|Code::CleanShutdown|Code::MenuOpened|Code::MenuDemand|Code::VisibilityChanged|Code::FocusChanged|Code::ToastQueued|Code::ToastActivated|Code::SoundQueued|Code::CallSourcesState=>"info",_=>"warning"},
        repetitions:1,dropped_before:recorder.dropped.swap(0,Ordering::Relaxed)};
    if let Ok(mut ring)=recorder.ring.try_lock() {
        if let Some(last)=ring.back_mut() && last.component==component && last.code==code && last.phase==phase && last.correlation==correlation && event.monotonic_ms.saturating_sub(last.monotonic_ms)<1000 {
            last.repetitions=last.repetitions.saturating_add(1);last.dropped_before=last.dropped_before.saturating_add(event.dropped_before);return;
        }
        if ring.len()==256 {ring.pop_front();} ring.push_back(event.clone());
    }
    if recorder.sender.try_send(event).is_err() {recorder.dropped.fetch_add(1,Ordering::Relaxed);}
}
pub fn summary() -> Vec<Event> { RECORDER.get().and_then(|r|r.ring.try_lock().ok().map(|ring|ring.iter().cloned().collect())).unwrap_or_default() }
pub fn directory() -> Option<PathBuf> { RECORDER.get().map(|r|r.directory.clone()) }
pub fn disk_failed()->bool {RECORDER.get().is_some_and(|r|r.disk_failed.load(Ordering::Relaxed))}
/// Voluntary export serialises only the typed in-memory ring; never bundles
/// arbitrary files placed in the diagnostics directory by another program.
pub fn export(destination: &Path) -> Result<(), &'static str> {
    if !no_links(destination) { return Err("La salida de diagnóstico no admite enlaces."); }
    let mut file=create_private(destination).map_err(|_|"No se pudo crear el archivo de diagnóstico; utiliza un nombre nuevo.")?;
    serde_json::to_writer_pretty(&mut file,&summary()).map_err(|_|"No se pudo exportar el resumen de diagnóstico.")
}
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location=info.location().map_or(0,|location|((location.line() as u64)<<32)|location.column() as u64);
        record(Component::App,Code::Panic,Phase::Unknown,location);
        if let Some(recorder)=RECORDER.get() && !recorder.panic_written.swap(true,Ordering::Relaxed) {
            if let Ok(mut emergency)=recorder.emergency.try_lock() {
                // No arbitrary payload, backtrace, path or formatting recursion.
                let _=writeln!(emergency,"{{\"schema\":1,\"code\":\"Panic\",\"location\":{location}}}");
                let _=emergency.flush();
            }
        }
    }));
}
