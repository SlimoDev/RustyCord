//! Cross-edition, per-user desktop ownership. Kernel mutex ownership survives
//! neither a crash nor process exit, so no stale lock file can block a restart.
//! The new process waits for the old process' ordinary shutdown before it opens
//! configuration, the session, the controller, or the media runtime.
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    ffi::c_void,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
type Handle = *mut c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateMutexW(attributes: *const c_void, initial: i32, name: *const u16) -> Handle;
    fn ReleaseMutex(handle: Handle) -> i32;
    fn CreateEventW(attributes: *const c_void, manual: i32, initial: i32, name: *const u16) -> Handle;
    fn SetEvent(handle: Handle) -> i32;
    fn ResetEvent(handle: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn QueryFullProcessImageNameW(process: Handle, flags: u32, path: *mut u16, length: *mut u32) -> i32;
}
#[link(name = "user32")]
unsafe extern "system" {
    fn EnumWindows(callback: unsafe extern "system" fn(Handle, isize) -> i32, context: isize) -> i32;
    fn GetWindowThreadProcessId(window: Handle, pid: *mut u32) -> u32;
    fn GetWindowTextW(window: Handle, text: *mut u16, length: i32) -> i32;
    fn PostMessageW(window: Handle, message: u32, wparam: usize, lparam: isize) -> i32;
    fn MessageBoxW(window: Handle, text: *const u16, caption: *const u16, flags: u32) -> i32;
}
const WAIT_OBJECT: u32 = 0;
const WAIT_ABANDONED: u32 = 0x80;
const WAIT_TIMEOUT: u32 = 258;
fn wide(value: &str) -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() }
fn owned(handle: Handle) -> Result<OwnedHandle, String> {
    if handle.is_null() { Err("Windows no pudo coordinar las instancias de NotDiscord.".into()) }
    else { Ok(unsafe { OwnedHandle::from_raw_handle(handle) }) }
}
pub struct ShutdownSignal { event: OwnedHandle }
impl ShutdownSignal {
    pub fn requested(&self) -> bool { unsafe { WaitForSingleObject(self.event.as_raw_handle(), 0) == WAIT_OBJECT } }
}
struct HeldMutex(OwnedHandle);
impl Drop for HeldMutex {
    fn drop(&mut self) { unsafe { ReleaseMutex(self.0.as_raw_handle()); } }
}
pub struct InstanceGuard {
    _ownership: HeldMutex,
    signal: Arc<ShutdownSignal>,
    replaced: bool,
}
fn namespace() -> Result<String, String> {
    let user_root = std::env::var_os("LOCALAPPDATA").ok_or("Windows no indicó la carpeta local del usuario.")?;
    let digest = format!("{:x}", Sha256::digest(user_root.to_string_lossy().to_lowercase().as_bytes()));
    Ok(format!("Local\\NotDiscord.Shared.{}", &digest[..24]))
}
/// The installer joins the same ownership protocol before replacing binaries.
/// It takes Admission before requesting application shutdown, waits Owner after
/// that process exits, then releases both before relaunching the application.
pub fn coordination_names() -> Result<(String, String), String> {
    let prefix = namespace()?;
    Ok((format!("{prefix}.Admission"), format!("{prefix}.Owner")))
}
impl InstanceGuard {
    pub fn signal(&self) -> Arc<ShutdownSignal> { self.signal.clone() }
    pub fn replaced_previous(&self) -> bool { self.replaced }

    pub fn acquire(_data: &Path, executable_root: &Path) -> Result<Self, String> {
        // Both editions and explicit portable data roots use the same per-user
        // ownership namespace; offline preview processes never acquire it.
        let prefix = namespace()?;
        let admission = owned(unsafe { CreateMutexW(std::ptr::null(), 0, wide(&format!("{prefix}.Admission")).as_ptr()) })?;
        wait_mutex(&admission, None)?;
        let _admission = HeldMutex(admission);
        let ownership = owned(unsafe { CreateMutexW(std::ptr::null(), 0, wide(&format!("{prefix}.Owner")).as_ptr()) })?;
        let event = owned(unsafe { CreateEventW(std::ptr::null(), 1, 0, wide(&format!("{prefix}.Close")).as_ptr()) })?;
        let status = unsafe { WaitForSingleObject(ownership.as_raw_handle(), 0) };
        let mut replaced = false;
        match status {
            WAIT_OBJECT | WAIT_ABANDONED => {},
            WAIT_TIMEOUT => {
                replaced = true;
                if unsafe { SetEvent(event.as_raw_handle()) } == 0 { return Err("No se pudo solicitar el cierre de la instancia anterior.".into()); }
                wait_mutex(&ownership, Some("La instancia anterior está guardando sus cambios y cerrando las conexiones. Espera a que termine. Si no responde, ciérrala normalmente y pulsa Reintentar."))?;
            },
            _ => return Err("No se pudo comprobar la instancia anterior.".into()),
        }
        let ownership = HeldMutex(ownership);
        // Old published builds predate the event protocol. Request WM_CLOSE
        // only for matching executable+window+installation paths, never kill a
        // process or scan a credential/browser store.
        let legacy = legacy_windows(executable_root);
        if !legacy.is_empty() {
            replaced = true;
            for (window, _) in &legacy { unsafe { PostMessageW(*window, 0x0010, 0, 0); } }
            for (_, process) in legacy {
                wait_process(&process)?;
            }
        }
        if unsafe { ResetEvent(event.as_raw_handle()) } == 0 { return Err("No se pudo preparar el cierre coordinado.".into()); }
        Ok(Self { _ownership: ownership, signal: Arc::new(ShutdownSignal { event }), replaced })
    }
}
fn ask_retry(text: &str) -> bool {
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide("NotDiscord · Cambio de instancia").as_ptr(), 0x00000005 | 0x00000040) == 4 }
}
fn wait_mutex(handle: &OwnedHandle, message: Option<&str>) -> Result<(), String> {
    let mut last_notice = Instant::now();
    loop {
        match unsafe { WaitForSingleObject(handle.as_raw_handle(), 250) } {
            WAIT_OBJECT | WAIT_ABANDONED => return Ok(()),
            WAIT_TIMEOUT => {
                if last_notice.elapsed() >= Duration::from_secs(20) {
                    let message = message.unwrap_or("Otra ventana de NotDiscord se está iniciando. Espera a que termine o ciérrala normalmente y pulsa Reintentar.");
                    if !ask_retry(message) { return Err("Inicio cancelado mientras se esperaba el cierre de otra instancia.".into()); }
                    last_notice = Instant::now();
                }
            },
            _ => return Err("Windows no pudo esperar el cierre de la instancia anterior.".into()),
        }
    }
}
fn wait_process(handle: &OwnedHandle) -> Result<(), String> {
    let mut last_notice = Instant::now();
    loop {
        match unsafe { WaitForSingleObject(handle.as_raw_handle(), 250) } {
            WAIT_OBJECT => return Ok(()),
            WAIT_TIMEOUT => {
                if last_notice.elapsed() >= Duration::from_secs(20) {
                    if !ask_retry("Una versión anterior de NotDiscord continúa abierta. Ciérrala normalmente para conservar sus datos y pulsa Reintentar.") {
                        return Err("Inicio cancelado mientras se esperaba el cierre de una versión anterior.".into());
                    }
                    last_notice = Instant::now();
                }
            },
            _ => return Err("No se pudo confirmar el cierre de la versión anterior.".into()),
        }
    }
}
unsafe extern "system" fn collect(window: Handle, context: isize) -> i32 {
    let windows = unsafe { &mut *(context as *mut Vec<(Handle, u32)>) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid); }
    if pid == 0 || pid == std::process::id() { return 1; }
    let mut title = [0u16; 256];
    let count = unsafe { GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32) };
    let title = String::from_utf16_lossy(&title[..count.max(0) as usize]);
    if count > 0 && title.starts_with("NotDiscord")
        && !title.contains("Llamada entrante") && !title.contains("Inicio de sesión") {
        windows.push((window, pid));
    }
    1
}
fn legacy_windows(executable_root: &Path) -> Vec<(Handle, OwnedHandle)> {
    let roots = executable_root.ancestors().take(3).filter_map(|path| std::fs::canonicalize(path).ok()).collect::<Vec<PathBuf>>();
    let mut windows = Vec::<(Handle, u32)>::new();
    unsafe { EnumWindows(collect, &mut windows as *mut _ as isize); }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for (window, pid) in windows {
        if !seen.insert(pid) { continue; }
        let Ok(process) = owned(unsafe { OpenProcess(0x1000 | 0x00100000, 0, pid) }) else { continue };
        let mut path = vec![0u16; 32768];
        let mut count = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(process.as_raw_handle(), 0, path.as_mut_ptr(), &mut count) } == 0 { continue; }
        let path = PathBuf::from(String::from_utf16_lossy(&path[..count as usize]));
        let Ok(path) = std::fs::canonicalize(path) else { continue };
        if path.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("NotDiscord.exe"))
            && roots.iter().any(|root| path.starts_with(root)) {
            result.push((window, process));
        }
    }
    result
}

pub fn show_startup_error(message: &str) {
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(message).as_ptr(), wide("NotDiscord · Inicio").as_ptr(), 0x00000010); }
}
