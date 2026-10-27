//! One bounded local worker. Reading and UTF-8 validation never run on the UI
//! thread, and a canceled/replaced operation cannot update another picker.
use super::PickerValue;
use eframe::egui;
use std::{fs::File, io::Read, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}, mpsc::{self, SyncSender}}};

const MAX_BYTES: usize = 256 * 1024;
#[derive(Clone, PartialEq, Eq)]
struct Stamp { operation: u64, owner: String, session: u64 }
struct Job { stamp: Stamp, path: PathBuf }
struct Completed { stamp: Stamp, result: Result<Vec<PickerValue>,String> }
pub(super) struct Importer {
    sender: SyncSender<Job>,
    generation: Arc<AtomicU64>,
    result: Arc<Mutex<Option<Completed>>>,
    pending: Option<Stamp>,
}
impl Importer {
    pub(super) fn new(ctx: egui::Context) -> Result<Self,String> {
        let (sender, receiver) = mpsc::sync_channel::<Job>(1);
        let generation = Arc::new(AtomicU64::new(0));
        let result = Arc::new(Mutex::new(None));
        let worker_generation = generation.clone();
        let worker_result = result.clone();
        std::thread::Builder::new().name("gif-txt-import".into()).spawn(move || {
            while let Ok(job) = receiver.recv() {
                let current = || worker_generation.load(Ordering::Acquire) == job.stamp.operation;
                if !current() { continue; }
                let imported = read(&job.path, current);
                if current() {
                    *worker_result.lock().unwrap_or_else(|error|error.into_inner()) = Some(Completed { stamp: job.stamp, result: imported });
                    ctx.request_repaint();
                }
            }
        }).map_err(|_|"No se pudo iniciar la importación. Los favoritos se conservan.".to_owned())?;
        Ok(Self { sender, generation, result, pending: None })
    }
    pub(super) fn cancel(&mut self) {
        self.generation.fetch_add(1,Ordering::AcqRel);
        self.pending = None;
        self.result.lock().unwrap_or_else(|error|error.into_inner()).take();
    }
    pub(super) fn pending(&self) -> bool { self.pending.is_some() }
    pub(super) fn start(&mut self, path: PathBuf, owner: &str, session: u64) -> Result<(),String> {
        self.cancel();
        let stamp = Stamp { operation: self.generation.load(Ordering::Acquire), owner: owner.into(), session };
        self.sender.try_send(Job { stamp: stamp.clone(), path })
            .map_err(|_|"La importación anterior aún se está cerrando. Vuelve a intentarlo; los favoritos se conservan.".to_owned())?;
        self.pending = Some(stamp);
        Ok(())
    }
    pub(super) fn poll(&mut self, owner: &str, session: u64, picker_visible: bool) -> Option<Result<Vec<PickerValue>,String>> {
        if self.pending.as_ref().is_some_and(|stamp|stamp.owner != owner || stamp.session != session || !picker_visible) {
            self.cancel();
            return None;
        }
        let completed = self.result.lock().unwrap_or_else(|error|error.into_inner()).take()?;
        if self.pending.as_ref() != Some(&completed.stamp)
            || self.generation.load(Ordering::Acquire) != completed.stamp.operation { return None; }
        self.pending = None;
        Some(completed.result)
    }
}
impl Drop for Importer { fn drop(&mut self) { self.cancel(); } }

fn read(path: &std::path::Path, current: impl Fn() -> bool) -> Result<Vec<PickerValue>,String> {
    let mut file = File::open(path).map_err(|_|"No se pudo abrir la lista de enlaces. Los favoritos se conservan.".to_owned())?;
    let metadata = file.metadata().map_err(|_|"No se pudo comprobar la lista de enlaces. Los favoritos se conservan.".to_owned())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
        return Err("La lista debe ser un archivo TXT de hasta 256 KiB. Los favoritos se conservan.".into());
    }
    // Metadata is only an early filter: the descriptor may grow afterwards.
    // Read at most limit+1 bytes, checking cancellation between small chunks.
    let mut bytes = Vec::with_capacity((metadata.len() as usize).min(MAX_BYTES));
    let mut chunk = [0_u8;8192];
    while bytes.len() <= MAX_BYTES {
        if !current() { return Err("Importación cancelada.".into()); }
        let remaining = (MAX_BYTES + 1 - bytes.len()).min(chunk.len());
        let count = file.read(&mut chunk[..remaining]).map_err(|_|"No se pudo leer la lista de enlaces. Los favoritos se conservan.".to_owned())?;
        if count == 0 { break; }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.len() > MAX_BYTES { return Err("La lista supera 256 KiB. Los favoritos se conservan.".into()); }
    let text = String::from_utf8(bytes).map_err(|_|"La lista TXT debe usar UTF-8. Los favoritos se conservan.".to_owned())?;
    let mut values = Vec::<PickerValue>::new();
    for line in text.trim_start_matches('\u{feff}').lines().take(500).map(str::trim) {
        if !current() { return Err("Importación cancelada.".into()); }
        let Ok(url) = url::Url::parse(line) else { continue; };
        let host = url.host_str().unwrap_or("");
        let provider = ["tenor.com","giphy.com","klipy.com"].iter().any(|provider|
            host == *provider || host.strip_suffix(*provider).is_some_and(|prefix|prefix.ends_with('.')));
        if url.scheme() == "https" && (provider || url.path().to_ascii_lowercase().ends_with(".gif"))
            && values.len() < 100 && !values.iter().any(|value|value.value == line) {
            values.push(PickerValue { value: line.into(), name: "GIF importado".into(), image_url: line.into(), ..Default::default() });
        }
    }
    Ok(values)
}
