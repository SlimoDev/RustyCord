//! One compatible preference file and one per-user DPAPI session for both editions.
//! Migration runs only after the desktop instance lease has been acquired. Source
//! files are retained, and opaque backups are made before importing preferences.
use crate::storage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

const CONFIG_NAME: &str = "configuration.json";
const MAX_CONFIG: u64 = 4 * 1024 * 1024;
const SECTIONS: [&str; 5] = ["ui-settings.json", "media-settings.json", "sound-settings.json", "audio-settings.json", "navigation.json"];
static ACTIVE_ROOT: OnceLock<PathBuf> = OnceLock::new();
static CONFIG_IO: Mutex<()> = Mutex::new(());

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Configuration {
    schema_version: u32,
    sections: BTreeMap<String, Value>,
    migrated_sources: BTreeSet<String>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}
fn read_configuration(root: &Path) -> Result<Configuration, String> {
    let path = root.join(CONFIG_NAME);
    if !path.exists() { return Ok(Configuration { schema_version: 1, ..Default::default() }); }
    let bytes = storage::read_bounded(&path, MAX_CONFIG).ok_or("No se pudo leer la configuración compartida; se conservará sin sustituirla.")?;
    let value: Configuration = serde_json::from_slice(&bytes).map_err(|_| "La configuración compartida no es válida; se conservará sin sustituirla.")?;
    if value.schema_version != 1 { return Err("Esta versión no puede abrir el formato de configuración compartida. Actualiza la aplicación.".into()); }
    Ok(value)
}
fn write_configuration(root: &Path, value: &Configuration) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| "La configuración compartida no es válida.")?;
    if bytes.len() as u64 > MAX_CONFIG { return Err("La configuración compartida supera el límite local.".into()); }
    storage::atomic_write(&root.join(CONFIG_NAME), &bytes)
}
fn key(path: &Path) -> Option<(PathBuf, String)> {
    let root = ACTIVE_ROOT.get()?;
    let relative = path.strip_prefix(root).ok()?;
    let pieces = relative.iter().map(|part| part.to_str()).collect::<Option<Vec<_>>>()?;
    let allowed = match pieces.as_slice() {
        [name] => SECTIONS.contains(name),
        [folder, name] => matches!(*folder, "pickers" | "notification-views")
            && name.ends_with(".json") && name.len() <= 80
            && name.trim_end_matches(".json").bytes().all(|c| c.is_ascii_hexdigit()),
        _ => false,
    };
    allowed.then(|| (root.clone(), pieces.join("/")))
}
/// `None` means an ordinary file outside the active shared preferences store.
pub(crate) fn read_section(path: &Path, limit: u64) -> Option<Option<Vec<u8>>> {
    let (root, key) = key(path)?;
    let _guard = CONFIG_IO.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(mut config) = read_configuration(&root) else { return Some(None); };
    let bytes = config.sections.remove(&key)
        .and_then(|value| serde_json::to_vec(&value).ok())
        .filter(|bytes| bytes.len() as u64 <= limit);
    Some(bytes)
}
pub(crate) fn write_section(path: &Path, incoming: Value) -> Option<Result<(), String>> {
    let (root, key) = key(path)?;
    let _guard = CONFIG_IO.lock().unwrap_or_else(|e| e.into_inner());
    Some((|| {
        let mut config = read_configuration(&root)?;
        // Preserve fields written by a newer compatible edition. Individual
        // field values (including maps) are replaced, so removals stay removed.
        let value = match (config.sections.remove(&key), incoming) {
            (Some(Value::Object(mut previous)), Value::Object(next)) => {
                previous.extend(next); Value::Object(previous)
            }
            (_, value) => value,
        };
        config.sections.insert(key, value);
        write_configuration(&root, &config)
    })())
}

pub fn directory(override_root: Option<PathBuf>) -> Result<PathBuf, String> {
    let root = match override_root {
        Some(root) => root,
        None => PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("Windows no indicó la carpeta local del usuario.")?)
            .join("NotDiscord").join("shared"),
    };
    let root = std::path::absolute(root).map_err(|_| "No se pudo localizar la carpeta compartida.")?;
    fs::create_dir_all(&root).map_err(|_| "No se pudo crear la carpeta compartida.")?;
    fs::canonicalize(root).map_err(|_| "No se pudo localizar la carpeta compartida.".into())
}

pub fn legacy_directories(executable_root: &Path, requested: Option<PathBuf>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Some(path) = requested { result.push(path); }
    // Cover the old portable package layout and the checked-out project layout;
    // no recursive drive scan, browser profile search, or account discovery.
    for ancestor in executable_root.ancestors().take(4) {
        result.push(ancestor.join("data"));
        result.push(ancestor.join("data-plus"));
    }
    result.sort(); result.dedup(); result
}
fn plain_file(path: &Path, limit: u64) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.file_attributes() & 0x400 == 0 && meta.len() <= limit)
}
fn stamp(path: &Path) -> SystemTime {
    fs::metadata(path).and_then(|meta| meta.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}
fn backup(root: &Path, source: &Path, relative: &Path, limit: u64) -> Result<(), String> {
    let path = source.join(relative);
    if !plain_file(&path, limit) { return Ok(()); }
    let source_key = format!("{:x}", Sha256::digest(source.to_string_lossy().as_bytes()));
    let dest = root.join("migration-backups").join(&source_key[..24]).join(relative);
    if !dest.exists() {
        let bytes = storage::read_bounded(&path, limit).ok_or("No se pudo respaldar un ajuste anterior; la migración se detuvo.")?;
        storage::atomic_write(&dest, &bytes)?;
    }
    Ok(())
}
fn preference_files(source: &Path) -> Vec<PathBuf> {
    let mut result = SECTIONS.iter().map(PathBuf::from).collect::<Vec<_>>();
    for folder in ["pickers", "notification-views"] {
        if let Ok(entries) = fs::read_dir(source.join(folder)) {
            for entry in entries.take(256).flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name.ends_with(".json") && name.len() <= 80
                    && name.trim_end_matches(".json").bytes().all(|c| c.is_ascii_hexdigit()) {
                    result.push(PathBuf::from(folder).join(name));
                }
            }
        }
    }
    result
}
fn migrate_sound_paths(value: &mut Value, source: &Path, root: &Path) -> Result<(), String> {
    let Some(events) = value.get_mut("events").and_then(Value::as_object_mut) else { return Ok(()) };
    for setting in events.values_mut() {
        let Some(file) = setting.get_mut("file") else { continue };
        let Some(text) = file.as_str() else { continue };
        let path = PathBuf::from(text);
        let path = if path.is_absolute() { path } else { source.join(path) };
        if !plain_file(&path, 8 * 1024 * 1024) { continue; }
        let bytes = storage::read_bounded(&path, 8 * 1024 * 1024).ok_or("No se pudo conservar un sonido importado.")?;
        let digest = format!("{:x}", Sha256::digest(&bytes));
        // Files imported by the existing decoder are already normalized WAVs;
        // copying preserves samples and never reconverts or plays the sound.
        if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" { continue; }
        let dest = root.join("custom-sounds-v1").join(format!("{digest}.wav"));
        if !dest.exists() { storage::atomic_write(&dest, &bytes)?; }
        *file = Value::String(dest.to_string_lossy().into_owned());
    }
    Ok(())
}
fn merge_missing(target: &mut Value, older: Value) {
    if let (Value::Object(target), Value::Object(older)) = (target, older) {
        for (key, value) in older {
            match target.get_mut(&key) {
                Some(existing) if matches!(key.as_str(), "UserVolumes" | "events") => merge_missing(existing, value),
                Some(_) => {},
                None => { target.insert(key, value); },
            }
        }
    }
}

/// Called at desktop startup only, while holding the cross-edition instance
/// lease. This function never obtains a token in plaintext.
pub fn initialize(root: &Path, sources: &[PathBuf]) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|_| "No se pudo crear la carpeta compartida.")?;
    let root = fs::canonicalize(root).map_err(|_| "No se pudo abrir la carpeta compartida.")?;
    let mut config = read_configuration(&root)?;
    let mut additions = BTreeMap::<String, Vec<(SystemTime, Value)>>::new();
    let mut imported = Vec::new();
    let mut sources = sources.to_vec();
    // An explicitly selected shared directory can itself contain the former
    // separate files. Import them once as well, without deleting the originals.
    sources.push(root.clone());
    sources.sort(); sources.dedup();
    for source in &sources {
        let Ok(source) = fs::canonicalize(source) else { continue };
        let source_id = source.to_string_lossy().to_lowercase();
        if config.migrated_sources.contains(&source_id) { continue; }
        let files = preference_files(&source);
        if !files.iter().any(|path| plain_file(&source.join(path), 256 * 1024))
            && !plain_file(&source.join("session.dpapi"), 65536)
            && !plain_file(&source.join("pins.dpapi"), 2 * 1024 * 1024)
            && !plain_file(&source.join("network-safety.dpapi"), 65536)
            && !plain_file(&source.join("network-safety.pending"), 65536) { continue; }
        for relative in files {
            let path = source.join(&relative);
            if !plain_file(&path, 256 * 1024) { continue; }
            backup(&root, &source, &relative, 256 * 1024)?;
            let bytes = storage::read_bounded(&path, 256 * 1024).ok_or("No se pudo leer un ajuste anterior.")?;
            let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else { continue };
            if relative.parent().is_none_or(|parent| parent.as_os_str().is_empty()) && !value.is_object() { continue; }
            if relative == Path::new("sound-settings.json") { migrate_sound_paths(&mut value, &source, &root)?; }
            let key = relative.to_string_lossy().replace('\\', "/");
            additions.entry(key).or_default().push((stamp(&path), value));
        }
        for name in ["session.dpapi", "network-safety.dpapi", "network-safety.pending"] {
            backup(&root, &source, Path::new(name), 65536)?;
        }
        backup(&root, &source, Path::new("pins.dpapi"), 2 * 1024 * 1024)?;
        imported.push(source);
        config.migrated_sources.insert(source_id);
    }
    for (key, mut candidates) in additions {
        candidates.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
        if config.sections.contains_key(&key) { continue; }
        let mut candidates = candidates.into_iter().map(|(_, value)| value);
        let Some(mut value) = candidates.next() else { continue };
        for older in candidates { merge_missing(&mut value, older); }
        config.sections.insert(key, value);
    }
    // Session migration copies the original Windows-protected bytes unchanged.
    // An established shared session (including explicit logout) wins forever.
    if !root.join("session.dpapi").exists() && !root.join(".session-migrated").exists() {
        if let Some(source) = imported.iter().map(|dir| dir.join("session.dpapi"))
            .filter(|path| plain_file(path, 65536)).max_by_key(|path| stamp(path)) {
            let bytes = storage::read_bounded(&source, 65536).ok_or("No se pudo trasladar la sesión cifrada.")?;
            storage::atomic_write(&root.join("session.dpapi"), &bytes)?;
        }
        storage::atomic_write(&root.join(".session-migrated"), b"1")?;
    }
    // Conversation pins use the existing feature-DPAPI format, separately from
    // plain preferences. Keep their ordering/removals from the newest source;
    // conflicting older encrypted copies remain in migration-backups.
    if !root.join("pins.dpapi").exists() && !root.join(".pins-migrated").exists() {
        if let Some(source) = imported.iter().map(|dir| dir.join("pins.dpapi"))
            .filter(|path| plain_file(path, 2 * 1024 * 1024)).max_by_key(|path| stamp(path)) {
            let bytes = storage::read_bounded(&source, 2 * 1024 * 1024).ok_or("No se pudieron trasladar las conversaciones fijadas.")?;
            storage::atomic_write(&root.join("pins.dpapi"), &bytes)?;
        }
        storage::atomic_write(&root.join(".pins-migrated"), b"1")?;
    }
    crate::network_safety::migrate_shared_markers(&root, &imported)?;
    write_configuration(&root, &config)?;
    ACTIVE_ROOT.set(root).map_err(|_| "La configuración compartida ya está inicializada.".to_string())
}
