//! A fixture window owned by this isolated probe process. Never enumerates or
//! captures another application's windows or the full desktop.
use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicIsize, Ordering},
    },
    time::Duration,
};
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static CLASS: OnceLock<String> = OnceLock::new();
static THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Paint {
    dc: isize,
    erase: i32,
    rect: Rect,
    restore: i32,
    update: i32,
    reserved: [u8; 32],
}
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct Message {
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: Point,
    private: u32,
}
#[repr(C)]
struct Class {
    style: u32,
    proc: Option<unsafe extern "system" fn(isize, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    background: isize,
    menu: *const u16,
    name: *const u16,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(class: *const Class) -> u16;
    fn UnregisterClassW(name: *const u16, instance: isize) -> i32;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        param: *const std::ffi::c_void,
    ) -> isize;
    fn DefWindowProcW(hwnd: isize, message: u32, wparam: usize, lparam: isize) -> isize;
    fn DestroyWindow(hwnd: isize) -> i32;
    fn IsWindow(hwnd: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
    fn GetClassNameW(hwnd: isize, name: *mut u16, count: i32) -> i32;
    fn PostMessageW(hwnd: isize, message: u32, wparam: usize, lparam: isize) -> i32;
    fn PostQuitMessage(code: i32);
    fn GetMessageW(message: *mut Message, hwnd: isize, min: u32, max: u32) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn BeginPaint(hwnd: isize, paint: *mut Paint) -> isize;
    fn EndPaint(hwnd: isize, paint: *const Paint) -> i32;
    fn GetClientRect(hwnd: isize, rect: *mut Rect) -> i32;
    fn FillRect(dc: isize, rect: *const Rect, brush: isize) -> i32;
    fn UpdateWindow(hwnd: isize) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> isize;
    fn DeleteObject(object: isize) -> i32;
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
unsafe extern "system" fn window_proc(
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    unsafe {
        match message {
            0x000f => {
                let mut paint: Paint = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut paint);
                let mut client = Rect::default();
                GetClientRect(hwnd, &mut client);
                let x = client.right / 2;
                let y = client.bottom / 2;
                for (rect, color) in [
                    (
                        Rect {
                            left: 0,
                            top: 0,
                            right: x,
                            bottom: y,
                        },
                        0x0000ff,
                    ),
                    (
                        Rect {
                            left: x,
                            top: 0,
                            right: client.right,
                            bottom: y,
                        },
                        0x00ff00,
                    ),
                    (
                        Rect {
                            left: 0,
                            top: y,
                            right: x,
                            bottom: client.bottom,
                        },
                        0xff0000,
                    ),
                    (
                        Rect {
                            left: x,
                            top: y,
                            right: client.right,
                            bottom: client.bottom,
                        },
                        0xffffff,
                    ),
                ] {
                    let brush = CreateSolidBrush(color);
                    if brush != 0 {
                        FillRect(dc, &rect, brush);
                        DeleteObject(brush);
                    }
                }
                EndPaint(hwnd, &paint);
                0
            }
            0x0010 => {
                DestroyWindow(hwnd);
                0
            }
            0x0002 => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}
pub fn prepare() -> Result<String, String> {
    if std::env::var("NOTDISCORD_MEDIA_PROBE").as_deref() != Ok("OWNED_SCREEN") {
        return Err("Wrong probe mode".into());
    }
    let unique = format!(
        "NotDiscord.OwnedPreview.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    CLASS
        .set(unique.clone())
        .map_err(|_| "Fixture already exists")?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || unsafe {
        let name = wide(&unique);
        let instance = GetModuleHandleW(std::ptr::null());
        let class = Class {
            style: 3,
            proc: Some(window_proc),
            class_extra: 0,
            window_extra: 0,
            instance,
            icon: 0,
            cursor: 0,
            background: 0,
            menu: std::ptr::null(),
            name: name.as_ptr(),
        };
        if RegisterClassW(&class) == 0 {
            let _ = tx.send(0);
            return;
        }
        let hwnd = CreateWindowExW(
            0x08000080,
            name.as_ptr(),
            name.as_ptr(),
            0x10cf0000,
            80,
            80,
            400,
            300,
            0,
            0,
            instance,
            std::ptr::null(),
        );
        WINDOW.store(hwnd, Ordering::Release);
        if hwnd != 0 {
            UpdateWindow(hwnd);
        }
        let _ = tx.send(hwnd);
        if hwnd != 0 {
            let mut message: Message = std::mem::zeroed();
            while GetMessageW(&mut message, 0, 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        WINDOW.store(0, Ordering::Release);
        UnregisterClassW(name.as_ptr(), instance);
    });
    *THREAD.lock().unwrap() = Some(thread);
    let hwnd = rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "Fixture window timed out")?;
    if hwnd == 0 {
        return Err("Fixture window creation failed".into());
    }
    source().ok_or_else(|| "Fixture identity verification failed".into())
}
pub fn source() -> Option<String> {
    let hwnd = WINDOW.load(Ordering::Acquire);
    if hwnd == 0 {
        return None;
    }
    let mut pid = 0;
    let mut class = [0_u16; 256];
    unsafe {
        if IsWindow(hwnd) == 0
            || GetWindowThreadProcessId(hwnd, &mut pid) == 0
            || pid != std::process::id()
        {
            return None;
        }
        let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if len <= 0 || CLASS.get()? != &String::from_utf16_lossy(&class[..len as usize]) {
            return None;
        }
    }
    Some(format!("window:{hwnd}"))
}
pub fn close() -> bool {
    let hwnd = WINDOW.load(Ordering::Acquire);
    if hwnd != 0 {
        unsafe {
            PostMessageW(hwnd, 0x0010, 0, 0);
        }
    }
    let thread = THREAD.lock().unwrap().take();
    if let Some(thread) = thread {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !thread.is_finished() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if thread.is_finished() {
            let _ = thread.join();
        }
    }
    WINDOW.load(Ordering::Acquire) == 0
}
