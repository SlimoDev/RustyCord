//! Compatible per-user DPAPI preferences and session-only change history. No token logging.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroize;

#[repr(C)]
struct Blob {
    len: u32,
    data: *mut u8,
}
#[link(name = "crypt32")]
unsafe extern "system" {
    fn CryptProtectData(
        input: *const Blob,
        description: *const u16,
        entropy: *const Blob,
        reserved: *mut std::ffi::c_void,
        prompt: *mut std::ffi::c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *const Blob,
        description: *mut *mut u16,
        entropy: *const Blob,
        reserved: *mut std::ffi::c_void,
        prompt: *mut std::ffi::c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
}
const ENTROPY: &[u8] = b"NotDiscord.Terminal.Features.v1";
pub fn protect(data: &[u8], feature: bool, encrypt: bool) -> Result<Vec<u8>, String> {
    if data.len() > 2 * 1024 * 1024 {
        return Err("El registro supera el límite cifrado.".into());
    }
    let input = Blob {
        len: data.len() as u32,
        data: data.as_ptr() as *mut u8,
    };
    let entropy = Blob {
        len: ENTROPY.len() as u32,
        data: ENTROPY.as_ptr() as *mut u8,
    };
    let ep = if feature {
        &entropy as *const Blob
    } else {
        std::ptr::null()
    };
    let mut output = Blob {
        len: 0,
        data: std::ptr::null_mut(),
    };
    let mut description = std::ptr::null_mut();
    let label: Vec<u16> = if feature {
        "NotDiscord feature data"
    } else {
        "NotDiscord Terminal"
    }
    .encode_utf16()
    .chain(Some(0))
    .collect();
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                label.as_ptr(),
                ep,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                &mut description,
                ep,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut output,
            )
        };
        if !description.is_null() {
            LocalFree(description.cast());
        }
        if ok == 0 {
            return Err("Windows no pudo abrir el almacén cifrado de este usuario.".into());
        }
        let result = if output.len == 0 {
            Vec::new()
        } else if output.data.is_null() {
            return Err("El almacén cifrado devolvió datos no válidos.".into());
        } else {
            std::slice::from_raw_parts(output.data, output.len as usize).to_vec()
        };
        if !output.data.is_null() {
            std::slice::from_raw_parts_mut(output.data, output.len as usize).zeroize();
            LocalFree(output.data.cast());
        }
        Ok(result)
    }
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| "No se pudo crear la carpeta de datos.")?;
    }
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| "No se pudo preparar el archivo.")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "No se pudo guardar el archivo.")?;
        drop(file);
        use std::os::windows::ffi::OsStrExt;
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
            return Err("No se pudo sustituir el archivo de datos.".into());
        }
        Ok(())
    })();
    let _ = fs::remove_file(temp);
    result
}
pub(super) fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > limit {
        return None;
    }
    let mut data = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut data)
        .ok()?;
    (data.len() as u64 <= limit).then_some(data)
}
pub fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path, limit: u64) -> T {
    crate::shared_state::read_section(path, limit).unwrap_or_else(|| read_bounded(path, limit))
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let value = serde_json::to_value(value).map_err(|_| "Datos no válidos.")?;
    if let Some(result) = crate::shared_state::write_section(path, value.clone()) { return result; }
    atomic_write(
        path,
        &serde_json::to_vec(&value).map_err(|_| "Datos no válidos.")?,
    )
}
pub fn load_session(dir: &Path) -> Option<zeroize::Zeroizing<String>> {
    let bytes = read_bounded(&dir.join("session.dpapi"), 65536)?;
    let plain = zeroize::Zeroizing::new(protect(&bytes, false, false).ok()?);
    let token = std::str::from_utf8(&plain).ok()?;
    if token.len() < 20 || token.len() > 4096 || token.contains(['\r', '\n']) {
        return None;
    }
    Some(zeroize::Zeroizing::new(token.to_owned()))
}
pub fn save_session(dir: &Path, token: &str) -> Result<(), String> {
    if token.len() < 20 || token.len() > 4096 || token.contains(['\r', '\n']) {
        return Err("La credencial no es válida.".into());
    }
    let encrypted = protect(token.as_bytes(), false, true)?;
    atomic_write(&dir.join("session.dpapi"), &encrypted)
}
/// Forget only the application session file. A missing file is already forgotten;
/// permission, sharing and path errors must remain visible to the caller.
pub fn forget_session(dir: &Path) -> Result<(), String> {
    match fs::remove_file(dir.join("session.dpapi")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("No se pudo eliminar la sesión guardada. La credencial local puede seguir en disco; comprueba sus permisos antes de volver a iniciar NotDiscord.".into()),
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
pub struct Navigation {
    pub account_id: String,
    pub guild_id: String,
    pub channel_id: String,
    pub last_private_channel_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct MediaPreferences {
    pub input_id: String,
    pub output_id: String,
    pub camera_id: String,
    pub input_volume: i32,
    pub input_sensitivity_db: i32,
    pub output_volume: i32,
    pub stream_volume_percent: f32,
    pub user_volumes: BTreeMap<String, f32>,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub push_to_talk: bool,
    pub ptt_virtual_key: i32,
}
impl Default for MediaPreferences {
    fn default() -> Self {
        Self {
            input_id: "default".into(),
            output_id: "default".into(),
            camera_id: String::new(),
            input_volume: 100,
            input_sensitivity_db: -40,
            output_volume: 100,
            stream_volume_percent: 100.,
            user_volumes: BTreeMap::new(),
            noise_suppression: true,
            echo_cancellation: true,
            push_to_talk: false,
            ptt_virtual_key: 0xA3,
        }
    }
}
impl MediaPreferences {
    pub fn normalize(mut self) -> Self {
        for (s, default, max) in [
            (&mut self.input_id, "default", 512),
            (&mut self.output_id, "default", 512),
            (&mut self.camera_id, "", 1024),
        ] {
            if s.trim().is_empty() || s.len() > max || s.chars().any(char::is_control) {
                *s = default.into();
            }
        }
        self.input_volume = self.input_volume.clamp(0, 200);
        self.input_sensitivity_db = self.input_sensitivity_db.clamp(-100, 0);
        self.output_volume = self.output_volume.clamp(0, 400);
        self.stream_volume_percent = if self.stream_volume_percent.is_finite() {
            self.stream_volume_percent.clamp(0., 100.)
        } else {100.};
        self.user_volumes.retain(|id, value| {
            notdiscord_core::parsing::valid_id(id) && value.is_finite()
        });
        for value in self.user_volumes.values_mut() {
            *value = value.clamp(0., 500.);
        }
        while self.user_volumes.len() > 256 {
            self.user_volumes.pop_last();
        }
        // GetAsyncKeyState accepts all assigned keyboard and mouse virtual keys.
        // 0 and 0xff are not keys; unknown/unassigned keys are harmless until
        // Windows actually reports a press. Do not restrict this to four keys.
        if !(1..=254).contains(&self.ptt_virtual_key) {
            self.ptt_virtual_key = 0xA3;
        }
        self
    }
    pub fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"inputId":self.input_id,"outputId":self.output_id,"cameraId":self.camera_id,"inputVolume":self.input_volume,"outputVolume":self.output_volume,"streamVolumePercent":self.stream_volume_percent,"userVolumes":self.user_volumes,"inputSensitivityDb":self.input_sensitivity_db,"noiseSuppression":self.noise_suppression,"echoCancellation":self.echo_cancellation,"pushToTalk":self.push_to_talk})
    }
}
#[path = "picker_history.rs"]
mod picker_history;
pub use picker_history::{GifImportResult, PickerHistory, PickerValue, PickerValues};
#[path = "feature_history.rs"]
mod feature_history;
pub use feature_history::Features;
#[cfg(test)]
use feature_history::validate_tail;
#[path = "message_cache.rs"]
mod message_cache;
pub use message_cache::MessageCache;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forgetting_a_session_reports_sharing_failures_and_preserves_other_files() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!(
            "notdiscord-forget-session-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&dir).unwrap();
        let session = dir.join("session.dpapi");
        fs::write(&session, b"synthetic encrypted session only").unwrap();
        fs::write(dir.join("preferences.json"), b"{}").unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&session)
            .unwrap();
        assert!(forget_session(&dir).is_err());
        assert!(session.exists());
        drop(held);
        forget_session(&dir).unwrap();
        forget_session(&dir).unwrap();
        assert!(!session.exists() && dir.join("preferences.json").exists());
        let resolved = fs::canonicalize(&dir).unwrap();
        let temporary = fs::canonicalize(std::env::temp_dir()).unwrap();
        assert_eq!(resolved.parent(), Some(temporary.as_path()));
        assert!(
            resolved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("notdiscord-forget-session-")
        );
        fs::remove_dir_all(resolved).unwrap();
    }
    #[test]
    fn dpapi_legacy_namespaces() {
        let secret = b"synthetic-never-a-token";
        let a = protect(secret, false, true).unwrap();
        assert_eq!(protect(&a, false, false).unwrap(), secret);
        assert!(protect(&a, true, false).is_err());
        let b = protect(secret, true, true).unwrap();
        assert_eq!(protect(&b, true, false).unwrap(), secret);
    }
    #[test]
    fn incomplete_tail_only_repaired() {
        let path = std::env::temp_dir().join(format!("nd-rust-{}.ndj", uuid::Uuid::new_v4()));
        let mut f = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        f.write_all(&[3, 0, 0, 0, 1, 2, 3, 5, 0]).unwrap();
        validate_tail(&mut f, true).unwrap();
        assert_eq!(f.metadata().unwrap().len(), 7);
        drop(f);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn preferences_clamp_and_legacy() {
        let p: MediaPreferences =
            serde_json::from_str(r#"{"InputVolume":999,"OutputVolume":-10,"PttVirtualKey":4}"#)
                .unwrap();
        let p = p.normalize();
        assert_eq!(p.input_volume, 200);
        assert_eq!(p.output_volume, 0);
        assert_eq!(p.ptt_virtual_key, 4);
        for key in [1, 6, 0x20, 0x41, 0x7b, 0xa3, 0xfe] {
            let mut p = MediaPreferences::default();
            p.ptt_virtual_key = key;
            assert_eq!(p.normalize().ptt_virtual_key, key);
        }
        for key in [-1, 0, 255, 500] {
            let mut p = MediaPreferences::default();
            p.ptt_virtual_key = key;
            assert_eq!(p.normalize().ptt_virtual_key, 0xa3);
        }
    }
    #[test]
    fn individual_volume_preferences_round_trip_stable_ids_and_legacy_defaults() {
        let legacy: MediaPreferences = serde_json::from_str(r#"{"OutputVolume":72}"#).unwrap();
        assert!(legacy.user_volumes.is_empty());
        assert_eq!(legacy.output_volume, 72);
        let mut preferences = legacy;
        preferences.user_volumes.insert("300".into(), 0.);
        preferences.user_volumes.insert("400".into(), 125.5);
        preferences.user_volumes.insert("500".into(), 500.);
        preferences.user_volumes.insert("bad-id".into(), 200.);
        preferences.user_volumes.insert("600".into(), f32::NAN);
        let preferences = preferences.normalize();
        assert_eq!(preferences.user_volumes.len(), 3);
        let file = std::env::temp_dir().join(format!("notdiscord-volume-{}.json", uuid::Uuid::new_v4()));
        save_json(&file, &preferences).unwrap();
        let loaded: MediaPreferences = read_json(&file, 32768);
        fs::remove_file(&file).unwrap();
        assert_eq!(loaded.user_volumes, preferences.user_volumes);
        assert_eq!(loaded.user_volumes.get("999").copied().unwrap_or(100.), 100.);
        assert_eq!(loaded.parameters()["userVolumes"]["400"], 125.5);
        assert_eq!((loaded.input_volume, loaded.output_volume), (100, 72));
        let mut bounded = MediaPreferences::default();
        for id in 1..=300 { bounded.user_volumes.insert(id.to_string(), 100.); }
        assert_eq!(bounded.normalize().user_volumes.len(), 256);
    }
}
