//! Owned, bounded update downloads. Legacy/unknown folders are never adopted.
//! The open lease denies cleanup while either the client or helper is active.
use super::*;
use std::{fs::File, os::windows::fs::{MetadataExt, OpenOptionsExt}, time::SystemTime};

const OWNER: &str = "NotDiscord.Standard.UpdateStaging";
const MAX_DIRS: usize = 6;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_AGE: u64 = 7 * 24 * 60 * 60;
const MIN_AGE: u64 = 60;
const FILES: &[&str] = &["staging.json", "lease.lock", "setup.exe", "plan.json", "install-update.ps1",
    "prepared", "installer-ready", "commit-install", "cancel-install", "installer-lease.ini", "installed", "error.txt"];

#[derive(Serialize, Deserialize)]
struct Marker { schema: u32, owner: String, created: u64, state: String }
fn now() -> u64 { SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() }
fn marker(path: &Path) -> Option<Marker> {
    let file = File::open(path.join("staging.json")).ok()?;
    let mut bytes = Vec::new(); file.take(4097).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 4096 { return None; }
    let marker: Marker = serde_json::from_slice(&bytes).ok()?;
    (marker.schema == 1 && marker.owner == OWNER && marker.created <= now().saturating_add(300)
        && ["downloading", "active", "failed", "cancelled", "installed"].contains(&marker.state.as_str())).then_some(marker)
}
fn flat_files(path: &Path) -> Option<(Vec<PathBuf>, u64)> {
    let mut paths = Vec::new(); let mut bytes = 0u64;
    for entry in fs::read_dir(path).ok()? {
        let entry = entry.ok()?; let meta = fs::symlink_metadata(entry.path()).ok()?;
        if meta.file_attributes() & 0x400 != 0 || !meta.is_file() || !FILES.contains(&entry.file_name().to_str()?) { return None; }
        bytes = bytes.checked_add(meta.len())?; paths.push(entry.path());
        if paths.len() > FILES.len() { return None; }
    }
    Some((paths, bytes))
}
fn cleanup(base: &Path, incoming: u64) -> Result<(), String> {
    let mut owned = Vec::new();
    for entry in fs::read_dir(base).map_err(|_| "No se pudo revisar el espacio de actualizaciones")? {
        let entry = entry.map_err(|_| "No se pudo revisar una actualización anterior")?;
        let name = entry.file_name(); let Some(name) = name.to_str() else { continue; };
        if name.len() != 32 || !name.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) { continue; }
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| "No se pudo comprobar la carpeta temporal")?;
        if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 { continue; }
        let Some((paths, bytes)) = flat_files(&entry.path()) else { continue; };
        let Some(marker) = marker(&entry.path()) else { continue; };
        owned.push((marker.created, entry.path(), paths, bytes));
        if owned.len() > 256 { return Err("Hay demasiadas preparaciones propias de actualización; no se descargó otro instalador".into()); }
    }
    owned.sort_by_key(|item| item.0);
    let mut count = owned.len(); let mut total = owned.iter().fold(0u64, |sum, item| sum.saturating_add(item.3));
    for (created, path, paths, bytes) in owned {
        let age = now().saturating_sub(created);
        if age < MAX_AGE && count < MAX_DIRS && total.saturating_add(incoming) <= MAX_BYTES { continue; }
        if age < MIN_AGE { continue; }
        // FILE_SHARE_DELETE permits unlinking our own lease while retaining an
        // exclusive claim against any new helper reader. Active leases fail open.
        let Ok(lease) = OpenOptions::new().read(true).write(true).share_mode(4).open(path.join("lease.lock")) else { continue; };
        reject_reparse_ancestors(&path)?;
        // Revalidate the flat allowlist after claiming the lease. No recursive
        // delete, symlink traversal, arbitrary child directory or foreign file.
        let Some((current, _)) = flat_files(&path) else { continue; };
        if current.len() != paths.len() { continue; }
        for file in current.iter().filter(|file| file.file_name().is_none_or(|name| name != "lease.lock" && name != "staging.json")) {
            fs::remove_file(file).map_err(|_| "No se pudo retirar una preparación anterior propia")?;
        }
        // Preserve ownership until the large payload and diagnostics are gone,
        // so an interrupted cleanup can be safely resumed on the next attempt.
        fs::remove_file(path.join("staging.json")).map_err(|_| "No se pudo retirar el marcador anterior")?;
        fs::remove_file(path.join("lease.lock")).map_err(|_| "No se pudo retirar el lease anterior")?;
        drop(lease);
        fs::remove_dir(&path).map_err(|_| "No se pudo retirar la carpeta temporal propia")?;
        count = count.saturating_sub(1); total = total.saturating_sub(bytes);
    }
    if count >= MAX_DIRS || total.saturating_add(incoming) > MAX_BYTES {
        return Err("Las preparaciones recientes o activas alcanzan la cuota de actualizaciones. Espera antes de reintentar; no se borraron carpetas ajenas ni activas".into());
    }
    Ok(())
}
pub(super) struct Staging { path: PathBuf, _lease: File, created: u64, handed_off: bool }
impl Staging {
    pub(super) fn create(base: &Path, expected: u64) -> Result<Self, String> {
        cleanup(base, expected)?;
        let path = base.join(uuid::Uuid::new_v4().simple().to_string());
        fs::create_dir(&path).map_err(|_| "No se pudo crear el staging de actualización")?;
        let lease = OpenOptions::new().read(true).write(true).create_new(true).share_mode(1 | 4)
            .open(path.join("lease.lock")).map_err(|_| "No se pudo reservar la actualización")?;
        let staging = Self { path, _lease: lease, created: now(), handed_off: false };
        staging.write_state("downloading")?; Ok(staging)
    }
    pub(super) fn path(&self) -> &Path { &self.path }
    fn write_state(&self, state: &str) -> Result<(), String> {
        let marker = Marker { schema: 1, owner: OWNER.into(), created: self.created, state: state.into() };
        fs::write(self.path.join("staging.json"), serde_json::to_vec(&marker).map_err(|_| "Estado temporal inválido")?)
            .map_err(|_| "No se pudo guardar el estado de actualización".into())
    }
    pub(super) fn hand_off(&mut self) -> Result<(), String> { self.write_state("active")?; self.handed_off = true; Ok(()) }
}
impl Drop for Staging {
    fn drop(&mut self) { if !self.handed_off { let _ = self.write_state("failed"); } }
}
