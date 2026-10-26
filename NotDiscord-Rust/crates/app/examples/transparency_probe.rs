//! Synthetic native renderer probe. No account, Gateway, app registration,
//! clipboard, full-desktop capture, or access to another app's pixels.
//! Reads the composited screen ONLY inside its own client rectangle, wholly
//! covered by its own topmost root and opaque checkerboard. PID, bounds and Z
//! coverage are checked immediately before/after copying. No full-screen bitmap,
//! monitor enumeration, focus request, or capture of another app is permitted.
use eframe::egui::{self, Color32};
use notdiscord::{storage, ui};
use notdiscord_core::models::AppSnapshot;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CAPTUREBLT, ClientToScreen,
            CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC,
            DeleteObject, EndPaint, FillRect, GetDC, HGDIOBJ, PAINTSTRUCT, ReleaseDC, SRCCOPY,
            SelectObject, UpdateWindow,
        },
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentProcessId},
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowW, GW_HWNDNEXT,
            GetClientRect, GetTopWindow, GetWindow, GetWindowRect, GetWindowThreadProcessId,
            HWND_TOPMOST, IsIconic, IsWindowVisible, RegisterClassW, SWP_NOACTIVATE, SWP_NOMOVE,
            SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos, UnregisterClassW, WM_ERASEBKGND, WM_PAINT,
            WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
        },
    },
    core::{PCWSTR, w},
};

const TILE: i32 = 32;
#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmFlush() -> i32;
}
fn checker(x: i32, y: i32) -> u8 {
    if ((x / TILE) + (y / TILE)) % 2 == 0 {
        224
    } else {
        32
    }
}
unsafe extern "system" fn underlay_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if message == WM_PAINT {
        unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut bounds = RECT::default();
            if GetClientRect(hwnd, &mut bounds).is_ok() {
                for y in (0..bounds.bottom).step_by(TILE as usize) {
                    for x in (0..bounds.right).step_by(TILE as usize) {
                        let value = checker(x, y) as u32;
                        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(
                            value | (value << 8) | (value << 16),
                        ));
                        let rect = RECT {
                            left: x,
                            top: y,
                            right: (x + TILE).min(bounds.right),
                            bottom: (y + TILE).min(bounds.bottom),
                        };
                        FillRect(dc, &rect, brush);
                        let _ = DeleteObject(HGDIOBJ(brush.0));
                    }
                }
            }
            let _ = EndPaint(hwnd, &paint);
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, message, w, l) }
}
struct Underlay {
    hwnd: HWND,
    class: Vec<u16>,
    instance: HINSTANCE,
}
impl Underlay {
    fn new(root: HWND, bounds: RECT) -> Result<Self, String> {
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).map_err(|e| e.to_string())?.0);
            let class: Vec<u16> = format!("NotDiscordSyntheticBackdrop-{}", uuid::Uuid::new_v4())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let definition = WNDCLASSW {
                lpfnWndProc: Some(underlay_proc),
                hInstance: instance,
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            if RegisterClassW(&definition) == 0 {
                return Err("Cannot register private checkerboard class".into());
            }
            let hwnd = match CreateWindowExW(
                WS_EX_TOOLWINDOW,
                PCWSTR(class.as_ptr()),
                w!("NotDiscord synthetic checkerboard"),
                WS_POPUP,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                None,
                None,
                Some(instance),
                None,
            ) {
                Ok(hwnd) => hwnd,
                Err(error) => {
                    let _ = UnregisterClassW(PCWSTR(class.as_ptr()), Some(instance));
                    return Err(error.to_string());
                }
            };
            let underlay = Self {
                hwnd,
                class,
                instance,
            };
            // Insert directly behind our topmost root. No other app is moved.
            SetWindowPos(
                root,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
            .map_err(|e| e.to_string())?;
            SetWindowPos(
                hwnd,
                Some(root),
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
            .map_err(|e| e.to_string())?;
            let _ = UpdateWindow(hwnd);
            Ok(underlay)
        }
    }
}
impl Drop for Underlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(PCWSTR(self.class.as_ptr()), Some(self.instance));
        }
    }
}
fn own(hwnd: HWND) -> bool {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}
fn client_bounds(hwnd: HWND) -> Result<RECT, String> {
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
        let mut origin = POINT { x: 0, y: 0 };
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return Err("Cannot locate own client area".into());
        }
        Ok(RECT {
            left: origin.x,
            top: origin.y,
            right: origin.x + rect.right,
            bottom: origin.y + rect.bottom,
        })
    }
}
fn intersects(a: RECT, b: RECT) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}
fn verify_capture(root: HWND, underlay: &Underlay, bounds: RECT) -> Result<(), String> {
    unsafe {
        if !own(root)
            || !own(underlay.hwnd)
            || !IsWindowVisible(root).as_bool()
            || !IsWindowVisible(underlay.hwnd).as_bool()
            || IsIconic(root).as_bool()
            || IsIconic(underlay.hwnd).as_bool()
        {
            return Err(
                "Own probe windows are unavailable or hidden; no pixels were captured".into(),
            );
        }
        let current = client_bounds(root)?;
        if current != bounds {
            return Err("Own probe window moved; capture aborted".into());
        }
        let mut behind = RECT::default();
        GetWindowRect(underlay.hwnd, &mut behind).map_err(|e| e.to_string())?;
        if behind != bounds {
            return Err("Synthetic checkerboard does not exactly cover the capture region".into());
        }
        let mut window = GetTopWindow(None).map_err(|e| e.to_string())?;
        let mut found_root = false;
        for _ in 0..4096 {
            if window == root {
                found_root = true;
                break;
            }
            if IsWindowVisible(window).as_bool() && !IsIconic(window).as_bool() {
                let mut other = RECT::default();
                if GetWindowRect(window, &mut other).is_ok() && intersects(other, bounds) {
                    return Err("A different visible window overlaps the private probe region; capture aborted".into());
                }
            }
            match GetWindow(window, GW_HWNDNEXT) {
                Ok(next) => window = next,
                Err(_) => break,
            }
        }
        if !found_root {
            return Err("Cannot establish the private probe window's Z order".into());
        }
        let next = GetWindow(root, GW_HWNDNEXT).map_err(|e| e.to_string())?;
        if next != underlay.hwnd {
            return Err(
                "Synthetic checkerboard is not immediately behind the probe; capture aborted"
                    .into(),
            );
        }
        Ok(())
    }
}
fn capture_covered_client_region(
    root: HWND,
    underlay: &Underlay,
) -> Result<image::RgbaImage, String> {
    let bounds = client_bounds(root)?;
    let (width, height) = (
        (bounds.right - bounds.left) as u32,
        (bounds.bottom - bounds.top) as u32,
    );
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > 8_000_000
    {
        return Err("Unexpected private capture dimensions".into());
    }
    if unsafe { DwmFlush() } < 0 {
        return Err("Cannot synchronize the compositor for the private probe region".into());
    }
    verify_capture(root, underlay, bounds)?;
    unsafe {
        // DWM composition is absent from GetDC(root). The screen source is
        // restricted to the checked client rectangle, with our opaque underlay
        // covering every pixel exposed by the transparent root. Never allocate
        // or copy the desktop/monitor dimensions or sample outside that region.
        let source = GetDC(None);
        if source.is_invalid() {
            return Err("Covered-region screen DC is unavailable".into());
        }
        let memory = CreateCompatibleDC(Some(source));
        if memory.is_invalid() {
            ReleaseDC(None, source);
            return Err("Own capture buffer is unavailable".into());
        }
        let mut bits = std::ptr::null_mut();
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let bitmap = match CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        {
            Ok(bitmap) => bitmap,
            Err(error) => {
                let _ = DeleteDC(memory);
                ReleaseDC(None, source);
                return Err(error.to_string());
            }
        };
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let copied = verify_capture(root, underlay, bounds).and_then(|_| {
            BitBlt(
                memory,
                0,
                0,
                width as i32,
                height as i32,
                Some(source),
                bounds.left,
                bounds.top,
                SRCCOPY | CAPTUREBLT,
            )
            .map_err(|e| e.to_string())
        });
        let verified = verify_capture(root, underlay, bounds);
        let pixels = if copied.is_ok() && verified.is_ok() && !bits.is_null() {
            let mut bytes =
                std::slice::from_raw_parts(bits.cast::<u8>(), width as usize * height as usize * 4)
                    .to_vec();
            for pixel in bytes.chunks_exact_mut(4) {
                pixel.swap(0, 2);
                pixel[3] = 255;
            }
            Some(bytes)
        } else {
            None
        };
        SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, source);
        copied?;
        verified?;
        image::RgbaImage::from_raw(width, height, pixels.ok_or("No private capture pixels")?)
            .ok_or("Invalid private capture dimensions".into())
    }
}

struct Probe {
    ui: ui::NotDiscordUi,
    output: PathBuf,
    started: Instant,
    frames: u32,
    root: Option<HWND>,
    underlay: Option<Underlay>,
    texture: egui::TextureHandle,
    result: Arc<Mutex<Option<serde_json::Value>>>,
    requested: bool,
    scale: f32,
    window_title: Vec<u16>,
}
impl Probe {
    fn complete(&mut self, ctx: &egui::Context, result: Result<serde_json::Value, String>) {
        *self.result.lock().unwrap() = Some(match result {
            Ok(value) => value,
            Err(error) => {
                serde_json::json!({"ok":false,"error":error,"source":"covered-own-client-screen-region"})
            }
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    fn inspect(&self, image: &egui::ColorImage) -> Result<serde_json::Value, String> {
        let underlay = self
            .underlay
            .as_ref()
            .ok_or("Own synthetic checkerboard is unavailable")?;
        let root = self.root.ok_or("Own root HWND is unavailable")?;
        let raw: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_srgba_unmultiplied())
            .collect();
        image::save_buffer(
            self.output.join("framebuffer-rgba.png"),
            &raw,
            image.width() as u32,
            image.height() as u32,
            image::ColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
        let mut composed = image::RgbaImage::new(image.width() as u32, image.height() as u32);
        for y in 0..image.height() {
            for x in 0..image.width() {
                let pixel = image[(x, y)];
                let background = checker(x as i32, y as i32) as f32;
                let rest = 1. - pixel.a() as f32 / 255.;
                composed.put_pixel(
                    x as u32,
                    y as u32,
                    image::Rgba([
                        (pixel.r() as f32 + background * rest)
                            .round()
                            .clamp(0., 255.) as u8,
                        (pixel.g() as f32 + background * rest)
                            .round()
                            .clamp(0., 255.) as u8,
                        (pixel.b() as f32 + background * rest)
                            .round()
                            .clamp(0., 255.) as u8,
                        255,
                    ]),
                );
            }
        }
        composed
            .save(self.output.join("synthetic-composite.png"))
            .map_err(|e| e.to_string())?;
        // This fixture has no messages or media downloads. Sample the empty
        // central-panel floor, below the welcome text and above calibration.
        let scale = self.scale;
        let mut samples = Vec::new();
        for x in [520., 560., 600., 640., 680., 720.] {
            let x = (x * scale) as u32;
            let y = (400. * scale) as u32;
            if x >= image.width() as u32 || y >= image.height() as u32 {
                continue;
            }
            let alpha = image[(x as usize, y as usize)].a();
            if !(125..=131).contains(&alpha) {
                return Err(format!(
                    "Expected one 50% base surface at synthetic sample; got alpha {alpha}"
                ));
            }
            samples.push((x, y));
        }
        if samples.len() != 6 {
            return Err("Synthetic sample region did not fit".into());
        }
        let point = |x: f32, y: f32| ((x * scale) as usize, (y * scale) as usize);
        let media = point(512., 532.);
        let line = point(570., 532.);
        if media.0 >= image.width()
            || media.1 >= image.height()
            || line.0 >= image.width()
            || line.1 >= image.height()
        {
            return Err("Opaque calibration did not fit the current client region".into());
        }
        if image[media].a() != 255 || image[line].a() != 255 {
            return Err("A media or line calibration was made translucent".into());
        }
        let mut opaque_text = false;
        for y in (510. * scale) as usize..((550. * scale) as usize).min(image.height()) {
            for x in (590. * scale) as usize..((760. * scale) as usize).min(image.width()) {
                if image[(x, y)].a() == 255 {
                    opaque_text = true;
                }
            }
        }
        if !opaque_text {
            return Err("No opaque text interior survived the opacity setting".into());
        }
        let capture = match capture_covered_client_region(root, underlay) {
            Ok(capture) => capture,
            Err(error) => {
                return Ok(
                    serde_json::json!({"ok":false,"framebufferAlpha50":true,"opaqueMedia":true,"opaqueLine":true,"opaqueText":true,"compositorVerified":false,"captureError":error,"source":"covered-own-client-screen-region"}),
                );
            }
        };
        if capture.dimensions() != (image.width() as u32, image.height() as u32) {
            return Err("DPI/capture dimensions differ; no compositor comparison made".into());
        }
        capture
            .save(self.output.join("covered-client-region.png"))
            .map_err(|e| e.to_string())?;
        let mut maximum_error = 0u8;
        for (x, y) in samples {
            for component in 0..3 {
                maximum_error = maximum_error
                    .max(capture[(x, y)][component].abs_diff(composed[(x, y)][component]));
            }
        }
        let compositor = maximum_error <= 12;
        Ok(
            serde_json::json!({"ok":compositor,"framebufferAlpha50":true,"opaqueMedia":true,"opaqueLine":true,"opaqueText":true,"compositorVerified":compositor,"maximumSampleError":maximum_error,"width":image.width(),"height":image.height(),"coverageVerifiedBeforeAndAfter":true,"source":"covered-own-client-screen-region","limit":if compositor{""}else{"The tightly covered client region differed from the expected synthetic DWM composition; no wider capture was attempted."}}),
        )
    }
}
impl eframe::App for Probe {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0., 0., 0., 0.]
    }
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.scale = ctx.pixels_per_point();
        if self.started.elapsed() > Duration::from_secs(12) {
            self.complete(ctx, Err("Synthetic renderer probe timed out".into()));
            return;
        }
        if self.root.is_none() {
            // Winit can request the initial frame before its HWND becomes
            // active. Locate only our fresh UUID caption and validate its PID;
            // never infer ownership from whichever application has focus.
            let root = unsafe { FindWindowW(None, PCWSTR(self.window_title.as_ptr())) }.ok();
            let Some(root) = root.filter(|root| own(*root)) else {
                ctx.request_repaint_after(Duration::from_millis(40));
                return;
            };
            self.root = Some(root);
            match client_bounds(root).and_then(|bounds| Underlay::new(root, bounds)) {
                Ok(window) => self.underlay = Some(window),
                Err(error) => {
                    self.complete(ctx, Err(error));
                    return;
                }
            }
        }
        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        eframe::App::update(&mut self.ui, ctx, frame);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("synthetic-calibration"),
        ));
        let rect = egui::Rect::from_min_size(egui::pos2(480., 500.), egui::vec2(64., 64.));
        painter.image(
            self.texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
        painter.line_segment(
            [egui::pos2(570., 500.), egui::pos2(570., 564.)],
            egui::Stroke::new(3_f32, Color32::WHITE),
        );
        painter.text(
            egui::pos2(590., 510.),
            egui::Align2::LEFT_TOP,
            "Tinta opaca",
            egui::FontId::proportional(22.),
            Color32::WHITE,
        );
        if let Some(image) = screenshot {
            let result = self.inspect(&image);
            self.complete(ctx, result);
            return;
        }
        self.frames += 1;
        if self.frames >= 12 && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint_after(Duration::from_millis(40));
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("evidence").join(format!("transparency-{}", uuid::Uuid::new_v4()))
        });
    std::fs::create_dir_all(&output)?;
    let data = output.join("synthetic-data");
    std::fs::create_dir_all(&data)?;
    storage::save_json(
        &data.join("ui-settings.json"),
        &serde_json::json!({"Opacity":0.5,"Zoom":1.0,"WindowsNotifications":false}),
    )?;
    let result = Arc::new(Mutex::new(None));
    let delivered = result.clone();
    let app_output = output.clone();
    let window_title = format!(
        "NotDiscord synthetic transparency probe {}",
        uuid::Uuid::new_v4()
    );
    let title_wide: Vec<u16> = window_title.encode_utf16().chain(Some(0)).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&window_title)
            .with_inner_size([1060., 700.])
            .with_position([120., 120.])
            .with_resizable(false)
            .with_decorations(false)
            .with_active(false)
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        &window_title,
        options,
        Box::new(move |cc| {
            let (command, _commands) = tokio::sync::mpsc::unbounded_channel();
            let (updates_tx, updates) = std::sync::mpsc::channel();
            updates_tx
                .send(ui::UiUpdate::Snapshot(AppSnapshot::welcome()))
                .unwrap();
            let ui = ui::NotDiscordUi::new(cc, data, command, updates);
            let texture = cc.egui_ctx.load_texture(
                "synthetic-opaque-media",
                egui::ColorImage::new([2, 2], vec![Color32::from_rgb(230, 40, 80); 4]),
                egui::TextureOptions::NEAREST,
            );
            Ok(Box::new(Probe {
                ui,
                output: app_output,
                started: Instant::now(),
                frames: 0,
                root: None,
                underlay: None,
                texture,
                result: delivered,
                requested: false,
                scale: 1.,
                window_title: title_wide,
            }))
        }),
    )?;
    let result = result.lock().unwrap().take().unwrap_or_else(
        || serde_json::json!({"ok":false,"error":"Probe closed before collecting its own pixels"}),
    );
    storage::save_json(&output.join("result.json"), &result)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    if result["ok"] != true {
        std::process::exit(1);
    }
    Ok(())
}
