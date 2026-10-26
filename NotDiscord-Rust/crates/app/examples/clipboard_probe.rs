//! Synthetic Windows clipboard roundtrip in a NEW noninteractive window station.
//!
//! `cargo run -p notdiscord --example clipboard_probe` starts a short-lived child.
//! The parent never opens, reads, saves or changes its clipboard. The child must
//! prove station/desktop isolation BEFORE calling any clipboard or COM API.
//! Failure to establish isolation aborts: there is NO interactive fallback.
//!
//! References:
//! https://learn.microsoft.com/en-us/windows/win32/winstation/window-stations
//! https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setprocesswindowstation
//! https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setthreaddesktop
use std::{
    mem::size_of,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, GlobalFree, HANDLE, HGLOBAL, HLOCAL, HWND, LocalFree},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject,
            HBITMAP, HGDIOBJ,
        },
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, IsClipboardFormatAvailable, OpenClipboard,
                RegisterClipboardFormatW, SetClipboardData,
            },
            Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock},
            StationsAndDesktops::*,
            Threading::{GetCurrentProcess, GetCurrentThreadId, OpenProcessToken},
        },
        UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_STYLE},
    },
    core::{PCWSTR, PWSTR, w},
};

type ProbeResult<T> = Result<T, String>;
fn error(stage: &str, error: windows::core::Error) -> String {
    format!("{stage}: {error}")
}

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct SecurityDescriptor(PSECURITY_DESCRIPTOR);
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}
fn private_security() -> ProbeResult<SecurityDescriptor> {
    unsafe {
        let mut token = Token(HANDLE::default());
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token.0)
            .map_err(|e| error("OpenProcessToken", e))?;
        let mut length = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut length);
        if length == 0 || length > 4096 {
            return Err("Invalid token user size".into());
        }
        let mut storage = vec![0u64; (length as usize).div_ceil(8)];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            length,
            &mut length,
        )
        .map_err(|e| error("GetTokenInformation", e))?;
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut sid = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid)
            .map_err(|e| error("ConvertSidToStringSid", e))?;
        let value = sid.to_string();
        let _ = LocalFree(Some(HLOCAL(sid.0.cast())));
        let value = value.map_err(|_| "Invalid token SID")?;
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{value})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = SecurityDescriptor(PSECURITY_DESCRIPTOR::default());
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor.0,
            None,
        )
        .map_err(|e| error("ConvertStringSecurityDescriptor", e))?;
        Ok(descriptor)
    }
}
fn object_name(handle: HANDLE) -> ProbeResult<String> {
    let mut text = [0u16; 256];
    let mut needed = 0;
    unsafe {
        GetUserObjectInformationW(
            handle,
            UOI_NAME,
            Some(text.as_mut_ptr().cast()),
            size_of::<[u16; 256]>() as u32,
            Some(&mut needed),
        )
    }
    .map_err(|e| error("GetUserObjectInformation(name)", e))?;
    let length = text
        .iter()
        .position(|c| *c == 0)
        .ok_or("Invalid object name")?;
    Ok(String::from_utf16_lossy(&text[..length]))
}
struct Isolation {
    original_station: HWINSTA,
    original_desktop: HDESK,
    station: HWINSTA,
    desktop: HDESK,
    switched: bool,
}
impl Isolation {
    fn create() -> ProbeResult<Self> {
        unsafe {
            let original_station =
                GetProcessWindowStation().map_err(|e| error("GetProcessWindowStation", e))?;
            let original_name = object_name(HANDLE(original_station.0))?;
            let original_desktop =
                GetThreadDesktop(GetCurrentThreadId()).map_err(|e| error("GetThreadDesktop", e))?;
            let descriptor = private_security()?;
            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0.0,
                bInheritHandle: false.into(),
            };
            // The automatic logon-derived name can already exist. A fresh UUID
            // name plus CWF_CREATE_ONLY (1) forbids reusing any existing station.
            // If Windows denies named creation, abort without clipboard access.
            // 0x37F is WINSTA_ALL_ACCESS, including ACCESSCLIPBOARD and CREATEDESKTOP.
            let requested_name =
                format!("NotDiscordClipboardProbe-{}", uuid::Uuid::new_v4().simple());
            let station_name: Vec<u16> = requested_name.encode_utf16().chain(Some(0)).collect();
            let station =
                CreateWindowStationW(PCWSTR(station_name.as_ptr()), 1, 0x37F, Some(&attributes))
                    .map_err(|e| error("CreateWindowStation (no clipboard access attempted)", e))?;
            let mut isolated = Self {
                original_station,
                original_desktop,
                station,
                desktop: HDESK::default(),
                switched: false,
            };
            let name = object_name(HANDLE(station.0))?;
            if !name.eq_ignore_ascii_case(&requested_name)
                || name.eq_ignore_ascii_case("WinSta0")
                || name.eq_ignore_ascii_case(&original_name)
            {
                return Err("Refusing an interactive or pre-existing station".into());
            }
            let mut flags = USEROBJECTFLAGS::default();
            GetUserObjectInformationW(
                HANDLE(station.0),
                UOI_FLAGS,
                Some((&mut flags as *mut USEROBJECTFLAGS).cast()),
                size_of::<USEROBJECTFLAGS>() as u32,
                None,
            )
            .map_err(|e| error("GetUserObjectInformation(flags)", e))?;
            if flags.dwFlags & 1 != 0 {
                return Err("Refusing a visible window station".into());
            }
            SetProcessWindowStation(station).map_err(|e| error("SetProcessWindowStation", e))?;
            isolated.switched = true;
            isolated.desktop = CreateDesktopW(
                w!("NotDiscordSyntheticClipboard"),
                PCWSTR::null(),
                None,
                DESKTOP_CONTROL_FLAGS(0),
                DESKTOP_CREATEWINDOW.0 | DESKTOP_READOBJECTS.0 | DESKTOP_WRITEOBJECTS.0,
                Some(&attributes),
            )
            .map_err(|e| error("CreateDesktop", e))?;
            SetThreadDesktop(isolated.desktop).map_err(|e| error("SetThreadDesktop", e))?;
            let current = GetProcessWindowStation().map_err(|e| error("Confirm station", e))?;
            if object_name(HANDLE(current.0))? != name {
                return Err("Station isolation was not confirmed".into());
            }
            let current_desktop =
                GetThreadDesktop(GetCurrentThreadId()).map_err(|e| error("Confirm desktop", e))?;
            if object_name(HANDLE(current_desktop.0))? != "NotDiscordSyntheticClipboard" {
                return Err("Desktop isolation was not confirmed".into());
            }
            Ok(isolated)
        }
    }
    fn restore(&mut self) -> ProbeResult<()> {
        if self.switched {
            unsafe {
                SetProcessWindowStation(self.original_station)
                    .map_err(|e| error("Restore station", e))?;
                SetThreadDesktop(self.original_desktop).map_err(|e| error("Restore desktop", e))?;
            }
            self.switched = false;
        }
        Ok(())
    }
}
impl Drop for Isolation {
    fn drop(&mut self) {
        let _ = self.restore();
        unsafe {
            if !self.desktop.is_invalid() {
                let _ = CloseDesktop(self.desktop);
            }
            if !self.station.is_invalid() {
                let _ = CloseWindowStation(self.station);
            }
        }
    }
}
struct TemporaryOutput {
    root: PathBuf,
    files: Vec<PathBuf>,
}
struct ClipboardOwner(HWND);
impl ClipboardOwner {
    fn create() -> ProbeResult<Self> {
        // A hidden message-only window in the already verified private desktop.
        unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("NotDiscord synthetic clipboard owner"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map(Self)
        .map_err(|e| error("Create isolated clipboard owner", e))
    }
}
impl Drop for ClipboardOwner {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
struct ClipboardOpen;
impl Drop for ClipboardOpen {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
struct GlobalImage(Option<HGLOBAL>);
impl Drop for GlobalImage {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            unsafe {
                let _ = GlobalFree(Some(handle));
            }
        }
    }
}
struct BitmapImage(Option<HBITMAP>);
impl Drop for BitmapImage {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(handle.0));
            }
        }
    }
}
fn set_synthetic_clipboard(owner: &ClipboardOwner, format: u32, handle: HANDLE) -> ProbeResult<()> {
    unsafe {
        OpenClipboard(Some(owner.0)).map_err(|e| error("Open isolated clipboard", e))?;
        let _open = ClipboardOpen;
        EmptyClipboard().map_err(|e| error("Empty isolated clipboard", e))?;
        SetClipboardData(format, Some(handle))
            .map_err(|e| error("Set synthetic clipboard data", e))?;
    }
    Ok(())
}
fn synthetic_header(width: u32, height: u32) -> BITMAPINFOHEADER {
    BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: -(height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: width * height * 4,
        ..Default::default()
    }
}
fn opaque_bgra(pixels: &[u8]) -> Vec<u8> {
    pixels
        .chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect()
}
fn set_dib(owner: &ClipboardOwner, width: u32, height: u32, pixels: &[u8]) -> ProbeResult<()> {
    let header = synthetic_header(width, height);
    let bgra = opaque_bgra(pixels);
    let mut image = GlobalImage(Some(
        unsafe { GlobalAlloc(GHND, size_of::<BITMAPINFOHEADER>() + bgra.len()) }
            .map_err(|e| error("Allocate synthetic DIB", e))?,
    ));
    let handle = image.0.ok_or("Missing synthetic DIB allocation")?;
    unsafe {
        let memory = GlobalLock(handle);
        if memory.is_null() {
            return Err("Cannot lock synthetic DIB".into());
        }
        std::ptr::copy_nonoverlapping(
            (&header as *const BITMAPINFOHEADER).cast::<u8>(),
            memory.cast(),
            size_of::<BITMAPINFOHEADER>(),
        );
        std::ptr::copy_nonoverlapping(
            bgra.as_ptr(),
            memory.cast::<u8>().add(size_of::<BITMAPINFOHEADER>()),
            bgra.len(),
        );
        let _ = GlobalUnlock(handle);
    }
    set_synthetic_clipboard(owner, 8, HANDLE(handle.0))?;
    image.0 = None; // Windows owns the HGLOBAL only after SetClipboardData succeeds.
    Ok(())
}
fn set_bitmap(owner: &ClipboardOwner, width: u32, height: u32, pixels: &[u8]) -> ProbeResult<()> {
    let info = BITMAPINFO {
        bmiHeader: synthetic_header(width, height),
        ..Default::default()
    };
    let mut memory = std::ptr::null_mut();
    let mut image = BitmapImage(Some(
        unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut memory, None, 0) }
            .map_err(|e| error("Create synthetic bitmap", e))?,
    ));
    if memory.is_null() {
        return Err("Missing synthetic bitmap memory".into());
    }
    let bgra = opaque_bgra(pixels);
    unsafe {
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), memory.cast(), bgra.len());
    }
    set_synthetic_clipboard(
        owner,
        2,
        HANDLE(image.0.ok_or("Missing synthetic bitmap")?.0),
    )?;
    image.0 = None; // Windows owns the HBITMAP only after SetClipboardData succeeds.
    Ok(())
}
fn assert_no_png() -> ProbeResult<()> {
    unsafe {
        let png = RegisterClipboardFormatW(w!("PNG"));
        if png != 0 && IsClipboardFormatAvailable(png).is_ok() {
            return Err("PNG remains available; native bitmap fallback was not isolated".into());
        }
    }
    Ok(())
}
fn verify_paste(
    output: &mut TemporaryOutput,
    width: u32,
    height: u32,
    pixels: &[u8],
    preserve_alpha: bool,
) -> ProbeResult<()> {
    let path = notdiscord::platform::paste_image_to_png(&output.root)?
        .ok_or("Synthetic bitmap could not be pasted")?;
    if path.parent() != Some(output.root.as_path()) {
        return Err("Paste returned a path outside synthetic output".into());
    }
    output.files.push(path.clone());
    let metadata = std::fs::metadata(&path).map_err(|_| "Missing synthetic PNG")?;
    if metadata.len() > 25 * 1024 * 1024 {
        return Err("Paste exceeded encoded byte bound".into());
    }
    let decoded = image::open(&path)
        .map_err(|_| "Invalid pasted PNG")?
        .into_rgba8();
    if (decoded.width(), decoded.height()) != (width, height) {
        return Err("Original dimensions changed".into());
    }
    for (expected, actual) in pixels.chunks_exact(4).zip(decoded.as_raw().chunks_exact(4)) {
        let alpha = if preserve_alpha { expected[3] } else { 255 };
        if alpha != actual[3] || (0..3).any(|c| expected[c].abs_diff(actual[c]) > 1) {
            return Err(
                "Roundtrip changed expected alpha or exceeded one-level WIC RGB rounding".into(),
            );
        }
    }
    Ok(())
}
impl TemporaryOutput {
    fn create(id: &str) -> ProbeResult<Self> {
        let root = std::env::temp_dir().join(format!("notdiscord-clipboard-probe-{id}"));
        std::fs::create_dir(&root)
            .map_err(|_| "Cannot create unique synthetic output directory")?;
        Ok(Self {
            root,
            files: Vec::new(),
        })
    }
}
impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        // Delete only exact files returned by the paste API inside this unique
        // directory; never enumerate or recursively remove a computed path.
        for file in &self.files {
            if file.parent() == Some(self.root.as_path()) {
                let _ = std::fs::remove_file(file);
            }
        }
        let _ = std::fs::remove_dir(&self.root);
    }
}
fn isolated_child(id: &str) -> ProbeResult<()> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid synthetic job id".into());
    }
    let mut isolation = Isolation::create()?;
    // All clipboard calls, including emptiness checks, occur ONLY after this gate.
    if notdiscord::platform::clipboard_has_image() {
        return Err("A fresh isolated clipboard unexpectedly contains an image".into());
    }
    let mut output = TemporaryOutput::create(id)?;
    let (width, height) = (7u32, 5u32);
    let pixels: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            [
                (i * 5) as u8,
                (i * 3) as u8,
                (i * 7) as u8,
                if i % 2 == 0 { 255 } else { 128 },
            ]
        })
        .collect();
    notdiscord::platform::copy_image(width, height, &pixels)?;
    if !notdiscord::platform::clipboard_has_image() {
        return Err("Copied bitmap was not advertised".into());
    }
    verify_paste(&mut output, width, height, &pixels, true)?;
    let owner = ClipboardOwner::create()?;
    set_dib(&owner, width, height, &pixels)?;
    assert_no_png()?;
    verify_paste(&mut output, width, height, &pixels, false)?;
    set_bitmap(&owner, width, height, &pixels)?;
    assert_no_png()?;
    verify_paste(&mut output, width, height, &pixels, false)?;
    if notdiscord::platform::valid_image_dimensions(8193, 1)
        || notdiscord::platform::valid_image_dimensions(6000, 6000)
        || notdiscord::platform::copy_image(0, 1, &[]).is_ok()
    {
        return Err("Invalid dimensions were accepted".into());
    }
    drop(owner); // No windows may remain when restoring the thread desktop.
    drop(output);
    isolation.restore()?;
    println!(
        "{}",
        serde_json::json!({"isolated":true,"roundtrip":true,"formats":["PNG","CF_DIB","CF_BITMAP"],"width":width,"height":height,"alphaPreserved":true,"dimensionBounds":true,"interactiveClipboardAccessed":false})
    );
    Ok(())
}
fn parent() -> ProbeResult<()> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let exe = std::env::current_exe().map_err(|_| "Cannot locate probe executable")?;
    let mut child = Command::new(exe)
        .arg("--isolated-child")
        .arg(id)
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Cannot start isolated probe child")?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child
            .try_wait()
            .map_err(|_| "Cannot inspect probe child")?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "Isolated probe timed out; only its exact child process was stopped".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let result = child
        .wait_with_output()
        .map_err(|_| "Cannot read isolated probe result")?;
    if result.stdout.len() > 4096 || result.stderr.len() > 4096 {
        return Err("Unexpected probe output size".into());
    }
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().into());
    }
    let result: serde_json::Value =
        serde_json::from_slice(&result.stdout).map_err(|_| "Invalid isolated probe result")?;
    if result["isolated"] != true
        || result["interactiveClipboardAccessed"] != false
        || result["roundtrip"] != true
    {
        return Err("The child did not confirm isolated success".into());
    }
    println!("{result}");
    Ok(())
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = if args.get(1).is_some_and(|arg| arg == "--isolated-child") {
        args.get(2)
            .ok_or_else(|| "Missing synthetic job id".into())
            .and_then(|id| isolated_child(id))
    } else {
        parent()
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
