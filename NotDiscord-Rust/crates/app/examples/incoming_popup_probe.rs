//! Synthetic native call window only: no controller/account/media or capture.
//! Minimizes its own root, verifies DND, X=Ignore, rearm and cancellation.
use eframe::egui;
use notdiscord::{
    storage,
    ui::{NotDiscordUi, UiUpdate},
};
use notdiscord_core::models::{AppSnapshot, IncomingCall, Person, UiAction};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            EnumThreadWindows, GWL_EXSTYLE, GetForegroundWindow, GetWindowLongPtrW, GetWindowTextW,
            GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW, WM_CLOSE,
            WS_EX_TOPMOST,
        },
    },
    core::BOOL,
};
fn own(hwnd: HWND) -> bool {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}
unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
    if own(hwnd) {
        let mut text = [0u16; 96];
        let n = unsafe { GetWindowTextW(hwnd, &mut text) };
        if n > 0 && String::from_utf16_lossy(&text[..n as usize]) == "NotDiscord · Llamada entrante"
        {
            unsafe {
                (*(data.0 as *mut Vec<HWND>)).push(hwnd);
            }
        }
    }
    BOOL(1)
}
fn popups() -> Result<Vec<HWND>, String> {
    let mut values = Vec::new();
    unsafe {
        if !EnumThreadWindows(
            GetCurrentThreadId(),
            Some(collect),
            LPARAM(&mut values as *mut _ as isize),
        )
        .as_bool()
        {
            return Err(windows::core::Error::from_win32().to_string());
        }
    }
    Ok(values)
}
fn state(instance: u64, dnd: bool, ringing: bool) -> AppSnapshot {
    let caller = Person {
        id: "42".into(),
        name: "Participante sintético".into(),
        ..Default::default()
    };
    AppSnapshot {
        signed_in: true,
        demo: true,
        connection: "Demo".into(),
        self_presence: if dnd { "dnd".into() } else { "online".into() },
        current_user: Some(Person {
            id: "1".into(),
            name: "Cuenta sintética".into(),
            ..Default::default()
        }),
        incoming_calls: if ringing {
            vec![IncomingCall {
                channel_id: "50".into(),
                channel_name: "Grupo de prueba local".into(),
                kind: "group".into(),
                instance_id: instance,
                ringing: true,
                caller: Some(caller.clone()),
                participants: vec![caller],
                ..Default::default()
            }]
        } else {
            vec![]
        },
        ..AppSnapshot::welcome()
    }
}
struct Probe {
    ui: NotDiscordUi,
    updates: mpsc::Sender<UiUpdate>,
    commands: tokio::sync::mpsc::UnboundedReceiver<UiAction>,
    phase: u8,
    at: Instant,
    started: Instant,
    root: Option<HWND>,
    result: Arc<Mutex<Option<serde_json::Value>>>,
    first_foreground: bool,
    second_foreground: bool,
}
impl Probe {
    fn publish(&self, ctx: &egui::Context, state: AppSnapshot) {
        self.updates.send(UiUpdate::Snapshot(state)).unwrap();
        ctx.request_repaint();
    }
    fn next(&mut self) {
        self.phase += 1;
        self.at = Instant::now();
    }
    fn finish(&self, ctx: &egui::Context, value: serde_json::Value) {
        *self.result.lock().unwrap() = Some(value);
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
    }
    fn check(&mut self, ctx: &egui::Context) -> Result<bool, String> {
        if self.started.elapsed() > Duration::from_secs(12) {
            return Err("Synthetic incoming popup timed out".into());
        }
        let root = self.root.ok_or("No own root")?;
        if !own(root) {
            return Err("Root ownership changed".into());
        }
        let windows = popups()?;
        if windows.len() > 1 {
            return Err("More than one owned incoming popup".into());
        }
        let elapsed = self.at.elapsed();
        match self.phase {
            0 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                self.next();
            }
            1 if elapsed >= Duration::from_millis(350) => {
                if !unsafe { IsIconic(root).as_bool() } {
                    return Err("Own root did not minimize".into());
                }
                self.publish(ctx, state(1, true, true));
                self.next();
            }
            2 if elapsed >= Duration::from_millis(400) => {
                if !windows.is_empty() {
                    return Err("DND created a popup".into());
                }
                self.publish(ctx, state(2, false, true));
                self.next();
            }
            3 if elapsed >= Duration::from_millis(400) => {
                let popup = *windows
                    .first()
                    .ok_or("Incoming event failed to open a native popup with root minimized")?;
                if !unsafe {
                    IsIconic(root).as_bool()
                        && IsWindowVisible(popup).as_bool()
                        && GetWindowLongPtrW(popup, GWL_EXSTYLE) & WS_EX_TOPMOST.0 as isize != 0
                } {
                    return Err("Popup not visible/topmost or root was restored".into());
                }
                self.first_foreground = unsafe { GetForegroundWindow() == popup };
                unsafe {
                    PostMessageW(Some(popup), WM_CLOSE, WPARAM(0), LPARAM(0))
                        .map_err(|e| e.to_string())?;
                }
                self.next();
            }
            4 if elapsed >= Duration::from_millis(450) => {
                if !windows.is_empty() {
                    return Err("Closing X did not suppress this ringing instance".into());
                }
                self.publish(ctx, state(3, false, true));
                self.next();
            }
            5 if elapsed >= Duration::from_millis(400) => {
                let popup = *windows
                    .first()
                    .ok_or("New ringing instance did not rearm")?;
                if !unsafe { IsIconic(root).as_bool() && IsWindowVisible(popup).as_bool() } {
                    return Err("Rearmed popup restored root or was hidden".into());
                }
                self.second_foreground = unsafe { GetForegroundWindow() == popup };
                self.publish(ctx, state(3, false, false));
                self.next();
            }
            6 if elapsed >= Duration::from_millis(450) => {
                if !windows.is_empty() {
                    return Err("Cancelled incoming event left its popup open".into());
                }
                let mut kinds = Vec::new();
                while let Ok(action) = self.commands.try_recv() {
                    kinds.push(action.kind);
                }
                if kinds
                    .iter()
                    .any(|kind| matches!(kind.as_str(), "join-call" | "reject-call"))
                {
                    return Err("X/cancellation dispatched a network call action".into());
                }
                self.finish(ctx,serde_json::json!({"ok":true,"syntheticOnly":true,"dndNoPopup":true,"rootRemainedMinimized":true,"visibleTopmost":true,"closeWasLocalIgnore":true,"sameChannelNewInstanceRearmed":true,"cancelledCallClosedPopup":true,"callActionsEmitted":0,"firstForeground":self.first_foreground,"secondForeground":self.second_foreground,"capturedPixels":false,"note":"Foreground keyboard activation is subject to Windows policy; topmost visibility and local dismissal are checked."}));
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }
}
impl eframe::App for Probe {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0., 0., 0., 0.]
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.root.is_none() {
            if let Ok(handle) = frame.window_handle() {
                if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                    self.root = Some(HWND(handle.hwnd.get() as _));
                }
            }
        }
        eframe::App::update(&mut self.ui, ctx, frame);
        match self.check(ctx) {
            Ok(true) => return,
            Err(error) => {
                self.finish(ctx,serde_json::json!({"ok":false,"syntheticOnly":true,"error":error,"capturedPixels":false}));
                return;
            }
            _ => {}
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("evidence/incoming-popup-native"));
    std::fs::create_dir_all(&output)?;
    let data = output.join("synthetic-data");
    std::fs::create_dir_all(&data)?;
    storage::save_json(
        &data.join("ui-settings.json"),
        &serde_json::json!({"WindowsNotifications":false,"Opacity":1.,"Blur":0.,"Zoom":1.}),
    )?;
    let result = Arc::new(Mutex::new(None));
    let delivered = result.clone();
    let title = format!("NotDiscord synthetic incoming {}", uuid::Uuid::new_v4());
    eframe::run_native(
        &title,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(&title)
                .with_inner_size([700., 500.])
                .with_active(false),
            ..Default::default()
        },
        Box::new(move |cc| {
            let (tx, commands) = tokio::sync::mpsc::unbounded_channel();
            let (updates, rx) = mpsc::channel();
            let mut ui = NotDiscordUi::new(cc, data, tx, rx);
            ui.enable_synthetic_incoming_popup();
            Ok(Box::new(Probe {
                ui,
                updates,
                commands,
                phase: 0,
                at: Instant::now(),
                started: Instant::now(),
                root: None,
                result: delivered,
                first_foreground: false,
                second_foreground: false,
            }))
        }),
    )?;
    let value = result.lock().unwrap().take().unwrap_or_else(
        || serde_json::json!({"ok":false,"error":"Synthetic probe closed before completion"}),
    );
    storage::save_json(&output.join("result.json"), &value)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    if value["ok"] != true {
        std::process::exit(1);
    }
    Ok(())
}
