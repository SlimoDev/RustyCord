//! Only the existing application's root HWND may be restored by a toast click.
use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(crate) fn notification_open_action(
    channel: &str,
    account: &str,
    generation: u64,
) -> Option<notdiscord_core::models::UiAction> {
    if !notdiscord_core::parsing::valid_id(channel) || !notdiscord_core::parsing::valid_id(account)
    {
        return None;
    }
    let mut action = notdiscord_core::models::UiAction::id("notification-open", channel);
    action.extra = serde_json::json!({"accountId":account,"generation":generation}).to_string();
    Some(action)
}

fn root_window_id() -> egui::Id {
    egui::Id::new("notification-root-window")
}
pub(crate) fn remember_root_window(ctx: &egui::Context, frame: &eframe::Frame) {
    if let Ok(handle) = frame.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            ctx.data_mut(|data| data.insert_temp(root_window_id(), handle.hwnd.get()));
        }
    }
}
pub(crate) fn restore_notification_window(ctx: &egui::Context) {
    // Queue the same state for winit so its cached viewport state follows Win32.
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
    if let Some(handle) = ctx.data(|data| data.get_temp::<isize>(root_window_id())) {
        let _ = focus_owned_root(handle);
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}
fn focus_owned_root(handle: isize) -> bool {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetWindowThreadProcessId, IsIconic, SW_MAXIMIZE, SW_RESTORE,
            SetForegroundWindow, ShowWindow,
        },
    };
    let hwnd = HWND(handle as _);
    if handle == 0 {
        return false;
    }
    unsafe {
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut owner));
        if owner != std::process::id() || GetAncestor(hwnd, GA_ROOT) != hwnd {
            return false;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        // Windows may refuse foreground transfer. Do not bypass its rules by
        // attaching input threads, stealing focus repeatedly, or making us topmost.
        SetForegroundWindow(hwnd).as_bool()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headless_activation_orders_restore_maximize_and_focus_without_a_native_window() {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            restore_notification_window(ctx);
        });
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(matches!(commands[0], egui::ViewportCommand::Visible(true)));
        assert!(matches!(
            commands[1],
            egui::ViewportCommand::Minimized(false)
        ));
        assert!(matches!(
            commands[2],
            egui::ViewportCommand::Maximized(true)
        ));
        assert!(matches!(commands[3], egui::ViewportCommand::Focus));
        assert!(!focus_owned_root(0));
    }
}
