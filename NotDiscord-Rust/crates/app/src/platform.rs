use std::{
    borrow::Cow,
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
};
use windows::{
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::*,
    Win32::{
        Graphics::{Direct2D::Common::*, Direct2D::*, DirectWrite::*, Dxgi::Common::*, Imaging::*},
        System::Com::*,
    },
    core::{HSTRING, Interface, w},
};
mod notification_activation;
pub(crate) use notification_activation::{notification_open_action, remember_root_window, restore_notification_window};

pub fn open_link(url: &str) -> Result<(), String> {
    let u = url::Url::parse(url).map_err(|_| "Enlace no válido.")?;
    if !matches!(u.scheme(), "https" | "http") {
        return Err("Protocolo no permitido.".into());
    }
    webbrowser::open(url)
        .map(|_| ())
        .map_err(|_| "No se pudo abrir el enlace.".into())
}

/// Normalize an invite before delegating its review to Discord's website.
/// This never forwards credentials, query parameters, or arbitrary URL paths.
pub fn official_invite_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_whitespace) {
        return Err("Usa un código o un enlace HTTPS de invitación de Discord válido.".into());
    }
    let code = if value.contains("://") {
        let authority = value
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
            .unwrap_or("");
        let url = url::Url::parse(value).map_err(|_| "Invitación no válida.")?;
        if url.scheme() != "https"
            || !matches!(
                url.host_str(),
                Some("discord.gg" | "discord.com" | "www.discord.com")
            )
            || url.port_or_known_default() != Some(443)
            || authority.contains('@')
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Usa un enlace HTTPS de invitación de Discord sin parámetros.".into());
        }
        let parts = url.path().split('/').collect::<Vec<_>>();
        match (url.host_str(), parts.as_slice()) {
            (Some("discord.gg"), ["", code] | ["", code, ""])
            | (
                Some("discord.com" | "www.discord.com"),
                ["", "invite", code] | ["", "invite", code, ""],
            ) => code.to_string(),
            _ => return Err("El enlace no corresponde a una invitación de Discord.".into()),
        }
    } else {
        value.to_owned()
    };
    if code.is_empty()
        || code.len() > 100
        || !code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err("El código de invitación no es válido.".into());
    }
    Ok(format!("https://discord.gg/{code}"))
}

#[cfg(test)]
mod invite_tests {
    use super::official_invite_url;

    #[test]
    fn official_invites_are_canonical_without_transferring_parameters() {
        for value in [
            "Abc_12-X",
            " https://discord.gg/Abc_12-X/ ",
            "https://discord.com/invite/Abc_12-X",
            "https://www.discord.com/invite/Abc_12-X/",
            "https://discord.gg:443/Abc_12-X",
        ] {
            assert_eq!(
                official_invite_url(value).unwrap(),
                "https://discord.gg/Abc_12-X"
            );
        }
    }

    #[test]
    fn official_invites_reject_foreign_origins_credentials_and_ambiguous_paths() {
        for value in [
            "",
            "https://evil.test/abc",
            "http://discord.gg/abc",
            "https://discord.gg.evil.test/abc",
            "https://discord.gg:8443/abc",
            "https://user:secret@discord.gg/abc",
            "https://@discord.gg/abc",
            "https://discord.gg/abc?token=secret",
            "https://discord.gg/abc#fragment",
            "https://discord.com/channels/abc",
            "https://discord.com/abc",
            "https://discord.gg/abc/extra",
            "https://discord.gg/abc//",
            "https://discord.gg/a%62c",
            "https://discord.gg/",
            "a/b",
            "ab cd",
            "https://discord.\tgg/abc",
            "file:///abc",
            "javascript:abc",
        ] {
            assert!(official_invite_url(value).is_err(), "accepted {value:?}");
        }
        assert!(official_invite_url(&"a".repeat(101)).is_err());
    }
}
static CLIPBOARD_IO: Mutex<()> = Mutex::new(());
pub fn valid_image_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= 8192
        && height <= 8192
        && width as u64 * height as u64 <= 24_000_000
}
pub fn copy_image(width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    if !valid_image_dimensions(width, height) || rgba.len() != width as usize * height as usize * 4
    {
        return Err("Imagen no válida.".into());
    }
    let _operation = CLIPBOARD_IO
        .lock()
        .map_err(|_| "Portapapeles no disponible.")?;
    arboard::Clipboard::new()
        .map_err(|_| "Portapapeles no disponible.")?
        .set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: Cow::Borrowed(rgba),
        })
        .map_err(|_| "No se pudo copiar la imagen.".into())
}

pub fn foreground_is_current_process() -> bool {
    unsafe {
        let hwnd = windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        let mut pid = 0;
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == std::process::id()
    }
}
/// Called only while the composer owns focus. egui-winit consumes Ctrl+V before
/// producing Key events, and produces no Paste event at all for bitmap-only data.
pub fn paste_key_state(ctrl: bool, shift: bool) -> (bool, bool) {
    unsafe {
        let get = windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        let control = ctrl || get(0x11) & i16::MIN != 0;
        let shift = shift || get(0x10) & i16::MIN != 0;
        // Drain the transition bits while focused even without the modifier.
        // Otherwise an earlier ordinary V press could masquerade as Ctrl+V
        // when Control is pressed on a later frame.
        let v_state = get(0x56);
        let insert_state = get(0x2d);
        let v = if control { v_state } else { 0 };
        let insert = if shift { insert_state } else { 0 };
        ((v | insert) & i16::MIN != 0, (v | insert) & 1 != 0)
    }
}
pub fn clipboard_has_image() -> bool {
    use windows::Win32::System::DataExchange::*;
    unsafe {
        let png = RegisterClipboardFormatW(w!("PNG"));
        [png, 2, 8, 17]
            .into_iter()
            .any(|format| format != 0 && IsClipboardFormatAvailable(format).is_ok())
    }
}
struct ClipboardOpen;
impl Drop for ClipboardOpen {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::DataExchange::CloseClipboard();
        }
    }
}
struct Apartment(bool);
impl Apartment {
    fn new() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() })
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
fn bitmap_pixels(
    handle: windows::Win32::Graphics::Gdi::HBITMAP,
) -> Result<(u32, u32, Vec<u8>), String> {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let mut bitmap = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(handle.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut bitmap as *mut BITMAP).cast()),
        ) == 0
            || bitmap.bmWidth <= 0
            || bitmap.bmHeight <= 0
            || !valid_image_dimensions(bitmap.bmWidth as u32, bitmap.bmHeight as u32)
        {
            return Err(
                "La imagen del portapapeles supera 8192 píxeles por lado o 24 millones de píxeles."
                    .into(),
            );
        }
        let _apartment = Apartment::new();
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| "No se pudo preparar la imagen del portapapeles.")?;
        let image = factory
            .CreateBitmapFromHBITMAP(handle, HPALETTE::default(), WICBitmapIgnoreAlpha)
            .map_err(|_| "No se pudo convertir el bitmap del portapapeles.")?;
        let source: IWICBitmapSource = image
            .cast()
            .map_err(|_| "No se pudo convertir el bitmap del portapapeles.")?;
        wic_pixels(&factory, &source, 8192)
            .map_err(|_| "No se pudo convertir el bitmap del portapapeles.".into())
    }
}
fn clipboard_pixels() -> Result<Option<(u32, u32, Vec<u8>)>, String> {
    use windows::Win32::{
        Foundation::HGLOBAL,
        Graphics::Gdi::*,
        System::{DataExchange::*, Memory::*},
    };
    let _operation = CLIPBOARD_IO
        .lock()
        .map_err(|_| "Portapapeles no disponible.")?;
    if !clipboard_has_image() {
        return Ok(None);
    }
    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(None).is_ok() } {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if !opened {
        return Err("El portapapeles está ocupado. Vuelve a pegar la imagen.".into());
    }
    let _clipboard = ClipboardOpen;
    unsafe {
        let png = RegisterClipboardFormatW(w!("PNG"));
        if png != 0 && IsClipboardFormatAvailable(png).is_ok() {
            let handle = GetClipboardData(png)
                .map_err(|_| "No se pudo leer la imagen PNG del portapapeles.")?;
            let global = HGLOBAL(handle.0);
            let length = GlobalSize(global);
            if length == 0 || length > 25 * 1024 * 1024 {
                return Err("La imagen PNG supera el límite de 25 MiB.".into());
            }
            let memory = GlobalLock(global);
            if memory.is_null() {
                return Err("No se pudo leer la imagen PNG del portapapeles.".into());
            }
            let data = std::slice::from_raw_parts(memory as *const u8, length).to_vec();
            let _ = GlobalUnlock(global);
            return decode_system_image(&data, 8192).map(Some).ok_or_else(|| {
                "La imagen PNG del portapapeles es inválida o demasiado grande.".into()
            });
        }
        // Windows synthesizes CF_BITMAP from CF_DIB/CF_DIBV5. This also supports
        // screenshot providers that arboard::get_image cannot read (DIB-only).
        let handle =
            GetClipboardData(2).map_err(|_| "No se pudo leer el bitmap del portapapeles.")?;
        bitmap_pixels(HBITMAP(handle.0)).map(Some)
    }
}
pub fn paste_image_to_png(root: &Path) -> Result<Option<PathBuf>, String> {
    let Some((width, height, rgba)) = clipboard_pixels()? else {
        return Ok(None);
    };
    save_pasted_rgba(root, width, height, &rgba).map(Some)
}
fn save_pasted_rgba(root: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<PathBuf, String> {
    use std::io::Write;
    if !valid_image_dimensions(width, height) || rgba.len() != width as usize * height as usize * 4
    {
        return Err("Imagen no válida.".into());
    }
    std::fs::create_dir_all(root).map_err(|_| "No se pudo preparar el adjunto local.")?;
    let path = root.join(format!("{}.png", uuid::Uuid::new_v4()));
    let result = (|| {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "No se pudo crear el adjunto local.")?;
        let mut output = std::io::BufWriter::new(file);
        image::write_buffer_with_format(
            &mut output,
            rgba,
            width,
            height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|_| "No se pudo preparar el PNG del portapapeles.")?;
        output
            .flush()
            .map_err(|_| "No se pudo guardar la imagen pegada.")?;
        let size = output
            .get_ref()
            .metadata()
            .map_err(|_| "No se pudo comprobar la imagen pegada.")?
            .len();
        if size > 25 * 1024 * 1024 {
            return Err("La imagen PNG supera el límite de 25 MiB.");
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = std::fs::remove_file(&path);
        return Err(error.into());
    }
    Ok(path)
}

/// Worker-local native resources. Rc's marker deliberately prevents Send/Sync:
/// the single-threaded D2D factory and COM apartment never cross threads.
pub(crate) struct EmojiRenderer {
    surface: Option<EmojiSurface>,
    factories: Option<EmojiFactories>,
    com_ready: bool,
    uninitialize: bool,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
    #[cfg(test)]
    fail_next_draw: bool,
}
struct EmojiFactories {
    wic: IWICImagingFactory,
    d2d: ID2D1Factory,
    write: IDWriteFactory,
}
struct EmojiSurface {
    // Release target-owned objects before their backing bitmap.
    brush: ID2D1SolidColorBrush,
    format: IDWriteTextFormat,
    target: ID2D1RenderTarget,
    bitmap: IWICBitmap,
    size: u32,
}
impl EmojiRenderer {
    pub(crate) fn new() -> Self {
        Self {
            surface: None, factories: None, com_ready: false, uninitialize: false,
            _thread: std::marker::PhantomData,
            #[cfg(test)]
            fail_next_draw: false,
        }
    }
    fn ensure_factories(&mut self) -> windows::core::Result<()> {
        unsafe {
            if !self.com_ready {
                let result = CoInitializeEx(None, COINIT_MULTITHREADED);
                if result.is_ok() {
                    // S_OK and S_FALSE both add a matching COM reference.
                    self.uninitialize = true;
                } else if result != windows::Win32::Foundation::RPC_E_CHANGED_MODE {
                    result.ok()?;
                }
                // A caller already in STA keeps ownership of that apartment;
                // retain the compatibility of the stateless wrapper there.
                self.com_ready = true;
            }
            if self.factories.is_none() {
                self.factories = Some(EmojiFactories {
                    wic: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?,
                    d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                    write: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
                });
            }
        }
        Ok(())
    }
    pub(crate) fn render(&mut self, text: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
        if !(16..=192).contains(&size) || text.is_empty() || text.encode_utf16().count() > 64 {
            return None;
        }
        // Input validation precedes COM and all native/heap allocations.
        let mut utf16 = [0u16; 64];
        let mut length = 0;
        for code in text.encode_utf16() { utf16[length] = code; length += 1; }
        let result = (|| -> windows::core::Result<Vec<u8>> {
            self.ensure_factories()?;
            if self.surface.as_ref().is_none_or(|surface| surface.size != size) {
                // Drop first: never retain multiple sizes or an unbounded pool.
                self.surface = None;
                self.surface = Some(EmojiSurface::new(self.factories.as_ref().unwrap(), size)?);
            }
            #[cfg(test)]
            if self.fail_next_draw {
                self.fail_next_draw = false;
                return Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL));
            }
            self.surface.as_ref().unwrap().draw(&utf16[..length])
        })();
        match result {
            Ok(pixels) => Some((size, size, pixels)),
            Err(_) => {
                // EndDraw/CopyPixels failure may leave the render target unusable.
                // Retry a later request using a new bounded surface.
                self.surface = None;
                None
            }
        }
    }
}
impl Drop for EmojiRenderer {
    fn drop(&mut self) {
        // COM interfaces must be released on their owning thread while the
        // apartment remains initialized, including after partial initialization.
        self.surface = None;
        self.factories = None;
        if self.uninitialize { unsafe { CoUninitialize(); } }
    }
}
impl EmojiSurface {
    fn new(factories: &EmojiFactories, size: u32) -> windows::core::Result<Self> {
        unsafe {
            let bitmap = factories.wic.CreateBitmap(
                size,
                size,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapCacheOnLoad,
            )?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = factories.d2d.CreateWicBitmapRenderTarget(&bitmap, &props)?;
            let format = factories.write.CreateTextFormat(
                w!("Segoe UI Emoji"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size as f32 * 0.78,
                w!("en-us"),
            )?;
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let brush = target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
                None,
            )?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(Self { brush, format, target, bitmap, size })
        }
    }
    fn draw(&self, text: &[u16]) -> windows::core::Result<Vec<u8>> {
        unsafe {
            self.target.BeginDraw();
            self.target.Clear(Some(&D2D1_COLOR_F::default()));
            self.target.DrawText(
                text,
                &self.format,
                &D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: self.size as f32,
                    bottom: self.size as f32,
                },
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            self.target.EndDraw(None, None)?;
            let mut bgra = vec![0u8; (self.size * self.size * 4) as usize];
            self.bitmap.CopyPixels(std::ptr::null(), self.size * 4, &mut bgra)?;
            for p in bgra.chunks_exact_mut(4) {
                p.swap(0, 2);
                let a = p[3] as u32;
                if a > 0 {
                    for c in &mut p[..3] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8
                    }
                }
            }
            Ok(bgra)
        }
    }
}
/// Stateless compatibility wrapper; long-lived workers use EmojiRenderer.
pub fn render_emoji(text: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    EmojiRenderer::new().render(text, size)
}
const NOTIFICATION_APP_ID: &str = "com.notdiscord.Terminal.Native";
struct TrackedToast {
    notification: ToastNotification,
    channel: String,
    message_id: String,
}
pub struct Notifications {
    toasts: VecDeque<TrackedToast>,
}
impl Notifications {
    pub fn new() -> Self {
        Self {
            toasts: VecDeque::new(),
        }
    }
    pub fn show(
        &mut self,
        title: &str,
        body: &str,
        channel: &str,
        command: tokio::sync::mpsc::UnboundedSender<notdiscord_core::models::UiAction>,
    ) -> Result<(), String> {
        self.show_with_avatar(title, body, channel, None, command)
    }
    pub fn show_with_avatar(
        &mut self,
        title: &str,
        body: &str,
        channel: &str,
        avatar: Option<&str>,
        command: tokio::sync::mpsc::UnboundedSender<notdiscord_core::models::UiAction>,
    ) -> Result<(), String> {
        self.show_message(title, body, channel, "", avatar, command)
    }
    pub fn show_message(
        &mut self,
        title: &str,
        body: &str,
        channel: &str,
        message_id: &str,
        avatar: Option<&str>,
        command: tokio::sync::mpsc::UnboundedSender<notdiscord_core::models::UiAction>,
    ) -> Result<(), String> {
        // HTTPS images are unsupported for this unpackaged Win32 application.
        // The controller prepares a bounded local PNG before the new entry point.
        let _ = avatar;
        self.show_prepared_message(title, body, channel, message_id, None, command)
    }
    pub fn show_prepared_message(
        &mut self,
        title: &str,
        body: &str,
        channel: &str,
        message_id: &str,
        image_path: Option<&str>,
        command: tokio::sync::mpsc::UnboundedSender<notdiscord_core::models::UiAction>,
    ) -> Result<(), String> {
        self.show_prepared_message_for_session(title,body,channel,message_id,image_path,"",0,command)
    }
    pub fn show_prepared_message_for_session(
        &mut self,
        title: &str,
        body: &str,
        channel: &str,
        message_id: &str,
        image_path: Option<&str>,
        account: &str,
        generation: u64,
        command: tokio::sync::mpsc::UnboundedSender<notdiscord_core::models::UiAction>,
    ) -> Result<(), String> {
        if !notdiscord_core::parsing::valid_id(channel)
            || (!message_id.is_empty() && !notdiscord_core::parsing::valid_id(message_id))
        {
            return Err("Destino de la notificación no válido.".into());
        }
        let xml = XmlDocument::new().map_err(|_| "Notificaciones no disponibles.")?;
        let avatar = image_path
            .and_then(crate::notification_identity::toast_image_uri)
            .map(|v| {
                format!(
                    "<image placement=\"appLogoOverride\" hint-crop=\"circle\" src=\"{}\"/>",
                    escape(&v)
                )
            })
            .unwrap_or_default();
        let launch = if message_id.is_empty() {
            String::new()
        } else {
            format!(" launch=\"ndmsg:{channel}:{message_id}\"")
        };
        let text = format!(
            "<toast{launch}><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text>{avatar}</binding></visual><audio silent=\"true\"/></toast>",
            escape(title),
            escape(&body.chars().take(160).collect::<String>())
        );
        xml.LoadXml(&HSTRING::from(text))
            .map_err(|_| "No se pudo preparar el aviso.")?;
        let toast =
            ToastNotification::CreateToastNotification(&xml).map_err(|_| "Aviso no disponible.")?;
        if !message_id.is_empty() {
            toast
                .SetTag(&HSTRING::from(notification_key(message_id)))
                .and_then(|_| toast.SetGroup(&HSTRING::from(notification_key(channel))))
                .map_err(|_| "No se pudo identificar el aviso.")?;
        }
        // Legacy callers without an authenticated origin may display an aviso,
        // but cannot create a navigation command with the next account's identity.
        let activation = notification_open_action(channel,account,generation);
        toast
            .Activated(&windows::Foundation::TypedEventHandler::new(move |_, _| {
                if let Some(action)=&activation { let _ = command.send(action.clone()); }
                Ok(())
            }))
            .map_err(|_| "Aviso no disponible.")?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(NOTIFICATION_APP_ID))
            .and_then(|n| n.Show(&toast))
            .map_err(|_| "Windows no pudo mostrar el aviso.")?;
        self.toasts.push_back(TrackedToast {
            notification: toast,
            channel: channel.into(),
            message_id: message_id.into(),
        });
        while self.toasts.len() > 16 {
            self.toasts.pop_front();
        }
        Ok(())
    }
    /// Retire only confirmed messages in this channel, including our prior-run history.
    /// Calls and messages newer than the read watermark are deliberately retained.
    pub fn clear_read(&mut self, channel: &str, through_id: &str) -> Result<(), String> {
        if !notdiscord_core::parsing::valid_id(channel)
            || !notdiscord_core::parsing::valid_id(through_id)
        {
            return Err("Marca de lectura no válida.".into());
        }
        let app = HSTRING::from(NOTIFICATION_APP_ID);
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app)
            .map_err(|_| "Notificaciones no disponibles.")?;
        let history = ToastNotificationManager::History()
            .map_err(|_| "Historial de notificaciones no disponible.")?;
        let mut failed = false;
        self.toasts.retain(|tracked| {
            if notification_is_read(&tracked.channel, &tracked.message_id, channel, through_id) {
                failed |= notifier.Hide(&tracked.notification).is_err();
                failed |= history
                    .RemoveGroupedTagWithId(
                        &HSTRING::from(notification_key(&tracked.message_id)),
                        &HSTRING::from(notification_key(channel)),
                        &app,
                    )
                    .is_err();
                false
            } else {
                true
            }
        });
        if let Ok(items) = history.GetHistoryWithId(&app) {
            let count = items.Size().unwrap_or(0).min(512);
            for index in 0..count {
                let Ok(toast) = items.GetAt(index) else {
                    continue;
                };
                let launch = toast
                    .Content()
                    .and_then(|xml| xml.DocumentElement())
                    .and_then(|root| root.GetAttribute(&HSTRING::from("launch")));
                let Ok(launch) = launch else { continue };
                let launch = launch.to_string();
                let Some((owner_channel, message)) = notification_target(&launch) else {
                    continue;
                };
                if !notification_is_read(owner_channel, message, channel, through_id) {
                    continue;
                }
                let tag = HSTRING::from(notification_key(message));
                let group = HSTRING::from(notification_key(owner_channel));
                // Require our complete ownership marker, never infer ownership from body text.
                if toast.Tag().is_ok_and(|value| value == tag)
                    && toast.Group().is_ok_and(|value| value == group)
                {
                    failed |= history.RemoveGroupedTagWithId(&tag, &group, &app).is_err();
                }
            }
        } else {
            failed = true;
        }
        if failed {
            Err("Windows no pudo retirar algún aviso leído.".into())
        } else {
            Ok(())
        }
    }
}
// WinRT accepts identifiers of at most 16 characters on all supported Windows versions.
fn notification_key(id: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(id.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn notification_target(launch: &str) -> Option<(&str, &str)> {
    let (channel, message) = launch.strip_prefix("ndmsg:")?.split_once(':')?;
    (notdiscord_core::parsing::valid_id(channel) && notdiscord_core::parsing::valid_id(message))
        .then_some((channel, message))
}
fn notification_is_read(owner_channel: &str, message: &str, channel: &str, through: &str) -> bool {
    owner_channel == channel
        && notdiscord_core::parsing::valid_id(message)
        && (message.len() < through.len() || message.len() == through.len() && message <= through)
}
fn escape(text: &str) -> String {
    text.chars()
        .filter(|c| {
            matches!(*c, '\t' | '\n' | '\r') || *c >= ' ' && *c != '\u{fffe}' && *c != '\u{ffff}'
        })
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
pub fn key_down(key: i32) -> bool {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(key) & i16::MIN != 0 }
}

/// WIC's decoding/scaling pipeline avoids retaining the full source pixel buffer.
/// Encoded-byte, dimension and concurrency limits are checked by the caller too.
unsafe fn wic_pixels(
    factory: &IWICImagingFactory,
    source: &IWICBitmapSource,
    max_side: u32,
) -> windows::core::Result<(u32, u32, Vec<u8>)> {
    unsafe {
        let (mut width, mut height) = (0, 0);
        source.GetSize(&mut width, &mut height)?;
        if !valid_image_dimensions(width, height) {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        let scale = (max_side as f64 / width.max(height) as f64).min(1.0);
        let target_width = (width as f64 * scale).max(1.0) as u32;
        let target_height = (height as f64 * scale).max(1.0) as u32;
        let scaled: IWICBitmapSource = if (target_width, target_height) != (width, height) {
            let scaler = factory.CreateBitmapScaler()?;
            scaler.Initialize(
                source,
                target_width,
                target_height,
                WICBitmapInterpolationModeFant,
            )?;
            scaler.cast()?
        } else {
            source.clone()
        };
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &scaled,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let mut rgba = vec![0u8; target_width as usize * target_height as usize * 4];
        converter.CopyPixels(std::ptr::null(), target_width * 4, &mut rgba)?;
        Ok((target_width, target_height, rgba))
    }
}
pub fn decode_system_image(bytes: &[u8], max_side: u32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        if bytes.is_empty() || bytes.len() > 256 * 1024 * 1024 || !(1..=8192).contains(&max_side) {
            return None;
        }
        let _apartment = Apartment::new();
        (|| -> windows::core::Result<_> {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = windows::Win32::UI::Shell::SHCreateMemStream(Some(bytes))
                .ok_or_else(windows::core::Error::from_win32)?;
            let decoder = factory.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )?;
            let source: IWICBitmapSource = decoder.GetFrame(0)?.cast()?;
            wic_pixels(&factory, &source, max_side)
        })()
        .ok()
    }
}
/// Filename decoding avoids retaining a second encoded memory stream. The
/// retained read handle rejects replacement for the whole synchronous decode.
pub fn decode_system_image_file(path: &Path, max_side: u32) -> Option<(u32, u32, Vec<u8>)> {
    use std::os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    };
    if !(1..=8192).contains(&max_side) {
        return None;
    }
    let path = std::fs::canonicalize(path).ok()?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x00200000)
        .open(&path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file()
        || metadata.file_attributes() & 0x400 != 0
        || metadata.len() == 0
        || metadata.len() > 256 * 1024 * 1024
    {
        return None;
    }
    let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
    if name.contains(&0) {
        return None;
    }
    name.push(0);
    unsafe {
        let _apartment = Apartment::new();
        (|| -> windows::core::Result<_> {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let decoder = factory.CreateDecoderFromFilename(
                windows::core::PCWSTR(name.as_ptr()),
                None,
                windows::Win32::Foundation::GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )?;
            let source: IWICBitmapSource = decoder.GetFrame(0)?.cast()?;
            wic_pixels(&factory, &source, max_side)
        })()
        .ok()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emoji_limits() {
        assert!(render_emoji("", 32).is_none());
        assert!(render_emoji("a", 1).is_none());
    }
    #[test]
    fn xml_escaped() {
        assert_eq!(escape("<a&b>"), "&lt;a&amp;b&gt;");
        assert_eq!(escape("\0\u{0008}hello\n\t\u{ffff}&"), "hello\n\t&amp;");
    }
    #[test]
    fn message_toast_ownership_and_partial_read_watermark_are_precise() {
        assert_eq!(notification_key("1234567890123456789").len(), 16);
        assert_eq!(notification_key("20"), notification_key("20"));
        assert_ne!(notification_key("20"), notification_key("21"));
        assert_eq!(notification_target("ndmsg:20:101"), Some(("20", "101")));
        assert!(notification_target("ndmsg:20:101:extra").is_none());
        assert!(notification_target("another:20:101").is_none());
        assert!(notification_target("ndmsg:20:").is_none());
        assert!(notification_is_read("20", "99", "20", "100"));
        assert!(notification_is_read("20", "100", "20", "100"));
        assert!(!notification_is_read("20", "101", "20", "100"));
        assert!(!notification_is_read("21", "99", "20", "100"));
        assert!(!notification_is_read("20", "", "20", "100"));
    }
}

#[cfg(test)]
mod native_image_tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn wic_thumbnail_preserves_ratio_and_alpha() {
        let source = image::RgbaImage::from_pixel(200, 100, image::Rgba([20, 40, 60, 128]));
        let mut png = Cursor::new(Vec::new());
        source.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let (width, height, pixels) = decode_system_image(png.get_ref(), 64).expect("Windows WIC");
        assert_eq!((width, height), (64, 32));
        // WIC scales premultiplied pixels; unpremultiplication may round by one.
        for (actual, expected) in pixels[..3].iter().zip([20_u8, 40, 60]) {
            assert!(actual.abs_diff(expected) <= 1);
        }
        assert_eq!(pixels[3], 128);
    }

    #[test]
    fn directwrite_draws_the_system_color_emoji() {
        let (_, _, pixels) = render_emoji("😀", 48).expect("Windows DirectWrite");
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 128 && p[0] != p[2]));
    }
    #[test]
    fn emoji_session_rejects_invalid_input_before_initializing_native_resources() {
        let mut renderer = EmojiRenderer::new();
        let overlong = "😀".repeat(33);
        for (text, size) in [("",32), ("😀",15), ("😀",193), (overlong.as_str(),32)] {
            assert!(renderer.render(text, size).is_none());
            assert!(!renderer.com_ready && !renderer.uninitialize);
            assert!(renderer.factories.is_none() && renderer.surface.is_none());
        }
    }
    #[test]
    fn emoji_session_reuses_cleared_surfaces_and_matches_stateless_rgba_at_all_sizes() {
        let mut renderer = EmojiRenderer::new();
        let mut factory = None;
        for size in [16,32,64,192] {
            let first = renderer.render("😀", size).expect("Windows DirectWrite session");
            assert_eq!((first.0,first.1,first.2.len()), (size,size,(size*size*4) as usize));
            assert!(first.2.chunks_exact(4).any(|pixel| pixel[3] == 0), "transparent background survives");
            assert!(first.2.chunks_exact(4).any(|pixel| pixel[3] > 128 && pixel[0] != pixel[2]), "native color survives");
            assert_eq!(first, render_emoji("😀", size).unwrap());
            let current_factory = renderer.factories.as_ref().unwrap().write.as_raw() as usize;
            if let Some(previous) = factory { assert_eq!(previous,current_factory); }
            factory = Some(current_factory);
            let target = renderer.surface.as_ref().unwrap().target.as_raw() as usize;
            let different = renderer.render("🦊", size).unwrap();
            assert_eq!(target, renderer.surface.as_ref().unwrap().target.as_raw() as usize);
            assert_ne!(first.2, different.2);
            assert_eq!(different, render_emoji("🦊", size).unwrap());
            assert_eq!(first, renderer.render("😀", size).unwrap(), "reusing must clear the previous glyph");
            assert!(renderer.render("invalid", 193).is_none());
            assert_eq!(target, renderer.surface.as_ref().unwrap().target.as_raw() as usize);
        }
    }
    #[test]
    fn emoji_draw_failure_returns_none_and_discards_only_the_surface_before_recovery() {
        let mut renderer = EmojiRenderer::new();
        let first = renderer.render("😀", 32).expect("Windows DirectWrite session");
        let factory = renderer.factories.as_ref().unwrap().write.as_raw() as usize;
        renderer.fail_next_draw = true;
        assert!(renderer.render("🦊", 32).is_none());
        assert!(renderer.surface.is_none());
        assert_eq!(factory, renderer.factories.as_ref().unwrap().write.as_raw() as usize);
        assert_eq!(first, renderer.render("😀", 32).unwrap());
    }
    #[test]
    fn wic_decodes_bmp_without_requiring_image_crate_bmp_feature() {
        let mut bitmap = vec![0u8; 62];
        bitmap[..2].copy_from_slice(b"BM");
        bitmap[2..6].copy_from_slice(&62u32.to_le_bytes());
        bitmap[10..14].copy_from_slice(&54u32.to_le_bytes());
        bitmap[14..18].copy_from_slice(&40u32.to_le_bytes());
        bitmap[18..22].copy_from_slice(&2u32.to_le_bytes());
        bitmap[22..26].copy_from_slice(&1u32.to_le_bytes());
        bitmap[26..28].copy_from_slice(&1u16.to_le_bytes());
        bitmap[28..30].copy_from_slice(&24u16.to_le_bytes());
        bitmap[54..60].copy_from_slice(&[30, 20, 10, 60, 50, 40]);
        let (width, height, pixels) =
            decode_system_image(&bitmap, 8192).expect("Windows BMP codec");
        assert_eq!((width, height), (2, 1));
        assert_eq!(pixels, [10, 20, 30, 255, 40, 50, 60, 255]);
    }
    #[test]
    fn pasted_png_writer_preserves_pixels_without_touching_clipboard() {
        let root =
            std::env::temp_dir().join(format!("notdiscord-paste-fixture-{}", uuid::Uuid::new_v4()));
        let pixels = [20, 40, 60, 128, 80, 100, 120, 255];
        let path = save_pasted_rgba(&root, 2, 1, &pixels).unwrap();
        let data = std::fs::read(&path).unwrap();
        let (w, h, rgba) = decode_system_image(&data, 8192).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, pixels);
        assert!(save_pasted_rgba(&root, 9000, 1, &[]).is_err());
        assert!(save_pasted_rgba(&root, 2, 1, &[0]).is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn original_and_clipboard_image_dimensions_are_bounded_before_pixel_allocation() {
        assert!(valid_image_dimensions(8192, 1));
        assert!(valid_image_dimensions(6000, 4000));
        assert!(!valid_image_dimensions(6001, 4000));
        assert!(!valid_image_dimensions(8193, 1));
        assert!(!valid_image_dimensions(0, 1));
    }
    #[test]
    fn png_decode_preserves_original_pixels_and_alpha_without_clipboard() {
        let expected = vec![
            10, 20, 30, 128, 40, 50, 60, 255, 70, 80, 90, 64, 100, 110, 120, 255,
        ];
        let source = image::RgbaImage::from_raw(2, 2, expected.clone()).unwrap();
        let mut png = Cursor::new(Vec::new());
        source.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let (width, height, pixels) =
            decode_system_image(png.get_ref(), 8192).expect("Windows WIC PNG");
        assert_eq!((width, height), (2, 2));
        assert_eq!(pixels, expected);
    }
    #[test]
    fn synthetic_dib_bitmap_uses_production_conversion_without_clipboard() {
        use windows::Win32::Graphics::Gdi::*;
        struct OwnedBitmap(HBITMAP);
        impl Drop for OwnedBitmap {
            fn drop(&mut self) {
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(self.0.0));
                }
            }
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 3,
                biHeight: -2,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 24,
                ..Default::default()
            },
            ..Default::default()
        };
        // Own pixels in a DIB section. No DC, screen capture, clipboard, window
        // station switch, or window is involved in constructing this handle.
        let expected = [
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140, 150,
            255, 160, 170, 180, 255,
        ];
        let bgra: Vec<u8> = expected
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0], 0])
            .collect();
        let mut memory = std::ptr::null_mut();
        let bitmap = OwnedBitmap(
            unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut memory, None, 0) }
                .expect("Synthetic DIB section"),
        );
        assert!(!memory.is_null());
        unsafe {
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), memory.cast(), bgra.len());
        }
        let (width, height, pixels) =
            bitmap_pixels(bitmap.0).expect("Production HBITMAP converter");
        assert_eq!((width, height), (3, 2));
        assert_eq!(pixels, expected);
        assert!(bitmap_pixels(HBITMAP::default()).is_err());
    }
}
