//! A short-lived native WebView2 sign-in process. It reads only requests made by
//! its own fresh login profile. No injected scripts, existing browser profiles,
//! credential scans, console handoffs or plaintext credential files are used.
use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, WebResourceRequestedEventHandler, take_pwstr,
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
            LibraryLoader::GetModuleHandleW,
        },
        UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, PWSTR, w},
};
use zeroize::{Zeroize, Zeroizing};

#[path = "auth_lifecycle.rs"]
mod lifecycle;

/// An owned secret that cannot enter generic JSON or ordinary debug output.
/// The controller validates its identity through the P0 gate before saving it.
pub struct Credential(Zeroizing<String>);
impl Credential {
    pub(crate) fn from_secret(secret: Zeroizing<String>) -> Result<Self, String> {
        if secret.len() < 20 || secret.len() > 4096 || secret.contains(['\r', '\n']) {
            return Err("La sesión recibida no es válida.".into());
        }
        Ok(Self(secret))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    pub fn into_secret(self) -> Zeroizing<String> {
        self.0
    }
    fn from_handoff(encrypted: &[u8]) -> Result<Self, String> {
        Self::from_secret(crate::storage::open_session_secret(encrypted)?)
    }
}
/// Takes only the COM-owned Authorization value returned by GetHeader. The
/// native UTF-16 allocation and our UTF-8 allocation both have explicit cleanup.
unsafe fn take_authorization(source: PWSTR) -> Zeroizing<String> {
    if source.is_null() {return Zeroizing::new(String::new());}
    struct NativeSecret {pointer:PWSTR,len:usize}
    impl Drop for NativeSecret {
        fn drop(&mut self) {
            unsafe {
                std::slice::from_raw_parts_mut(self.pointer.0,self.len).zeroize();
                CoTaskMemFree(Some(self.pointer.0.cast()));
            }
        }
    }
    // WebView2 owns a NUL-terminated CoTaskMem allocation until we free it,
    // exactly as for take_pwstr; this variant also wipes its original storage.
    let native=NativeSecret {pointer:source,len:unsafe {source.as_wide().len()}};
    let mut text=Zeroizing::new(String::with_capacity(native.len.min(4096)));
    if native.len>4096 {return text;}
    for character in char::decode_utf16(unsafe {std::slice::from_raw_parts(native.pointer.0,native.len)}.iter().copied()) {
        match character {
            Ok(character)=>text.push(character),
            Err(_)=>{text.zeroize();return text;}
        }
    }
    text
}
impl std::fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Credential([REDACTED])")
    }
}

thread_local! {static CONTROLLER:RefCell<Option<ICoreWebView2Controller>>=const{RefCell::new(None)};}
fn valid_job_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn captured_origin(uri: &str) -> bool {
    url::Url::parse(uri).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("discord.com")
            && u.port_or_known_default() == Some(443)
            && u.username().is_empty()
            && u.password().is_none()
            && u.path().starts_with("/api/")
    })
}
fn allowed_navigation(uri: &str) -> bool {
    uri == "about:blank"
        || url::Url::parse(uri).is_ok_and(|u| {
            u.scheme() == "https"
                && u.host_str()
                    .is_some_and(|h| h == "discord.com" || h.ends_with(".discord.com"))
                && u.username().is_empty()
                && u.password().is_none()
        })
}

/// Starts the same Rust executable in its isolated login mode. Identity is
/// validated exactly once by the controller through its P0-protected API before
/// it can persist a session. No credential travels through JSON completions.
pub async fn interactive(data: &Path) -> Result<Credential, String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let data = data.to_path_buf();
    let recovery_data = data.clone();
    let recovery = tokio::task::spawn_blocking(move || lifecycle::recover_inactive(&recovery_data))
        .await
        .map_err(|_| "No se pudo revisar la limpieza del inicio de sesión.")??;
    if recovery.deferred != 0 {
        return Err("Quedan recursos propios de inicio de sesión que Windows no pudo retirar. Cierra sus ventanas pendientes y vuelve a intentarlo.".into());
    }
    let mut resources = lifecycle::Resources::prepare(&data, &id)?;
    let executable = std::env::current_exe().map_err(|_| "No se pudo localizar NotDiscord.")?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--login")
        .arg("--data-dir")
        .arg(&data)
        .arg("--login-handoff")
        .arg(&id)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // This suppresses a console, not the explicitly requested login window.
    command.creation_flags(0x08000000);
    let child = command
        .spawn()
        .map_err(|_| "No se pudo abrir la ventana de inicio de sesión.")?;
    resources.attach(child);
    let result =
        tokio::time::timeout(Duration::from_secs(15 * 60), resources.child_mut().wait()).await;
    let successful = match result {
        Ok(Ok(status)) => status.success(),
        _ => false,
    };
    let encrypted = successful.then(|| resources.read_handoff()).flatten();
    // Once scheduled, cleanup owns the child and the resources independently of
    // this future. Cancelling while awaiting finish cannot cancel that cleanup.
    resources.finish().await?;
    if !successful {
        return Err("No se pudo completar el inicio de sesión.".into());
    }
    let Some(encrypted) = encrypted else {
        return Err("Inicio de sesión cancelado.".into());
    };
    Credential::from_handoff(&encrypted)
}
pub fn login_process(data: &Path) {
    let args = std::env::args().collect::<Vec<_>>();
    let id = args
        .iter()
        .position(|a| a == "--login-handoff")
        .and_then(|i| args.get(i + 1));
    let Some(id) = id.filter(|v| valid_job_id(v)) else {
        return;
    };
    let Some(resources) = lifecycle::ChildLease::acquire(data, id) else {
        return;
    };
    if resources.join_process().is_err() {
        return;
    }
    let profile = resources.profile();
    if fs::create_dir(&profile).is_err() {
        return;
    }
    let result = login_window(&profile, resources.handoff());
    if result.is_err() {
        unsafe {
            let _ = MessageBoxW(
                None,
                w!(
                    "No se pudo abrir el inicio de sesión. Comprueba WebView2 y la conexión a Internet."
                ),
                w!("NotDiscord · Inicio de sesión"),
                MB_OK | MB_ICONWARNING,
            );
        }
    }
}
fn login_window(profile: &Path, handoff: PathBuf) -> Result<(), String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|_| "No se pudo iniciar WebView2.")?;
    }
    let result = run_login_window(profile, handoff);
    CONTROLLER.with(|c| {
        if let Some(controller) = c.borrow_mut().take() {
            unsafe {
                let _ = controller.Close();
            }
        }
    });
    unsafe { CoUninitialize() };
    result
}
fn run_login_window(profile: &Path, handoff: PathBuf) -> Result<(), String> {
    let hwnd = unsafe {
        let instance = HINSTANCE(
            GetModuleHandleW(None)
                .map_err(|_| "No se pudo localizar el módulo de NotDiscord.")?
                .0,
        );
        let class = WNDCLASSW {
            hInstance: instance,
            lpfnWndProc: Some(window_proc),
            lpszClassName: w!("NotDiscordRustLogin"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            Default::default(),
            w!("NotDiscordRustLogin"),
            w!("NotDiscord · Inicio de sesión de Discord"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            900,
            760,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|_| "No se pudo crear la ventana de inicio de sesión.")?
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    let profile: Vec<u16> = profile
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                PCWSTR(profile.as_ptr()),
                None::<&ICoreWebView2EnvironmentOptions>,
                &handler,
            )
            .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error, environment| {
            error?;
            let _ = tx.send(environment.ok_or_else(|| windows::core::Error::from(E_POINTER)));
            Ok(())
        }),
    )
    .map_err(|_| "No se pudo preparar el navegador de inicio de sesión.")?;
    let environment = rx
        .recv()
        .map_err(|_| "Inicio de sesión cancelado.")?
        .map_err(|_| "No se pudo crear WebView2.")?;
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            environment
                .CreateCoreWebView2Controller(hwnd, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error, controller| {
            error?;
            let _ = tx.send(controller.ok_or_else(|| windows::core::Error::from(E_POINTER)));
            Ok(())
        }),
    )
    .map_err(|_| "No se pudo preparar la ventana de inicio de sesión.")?;
    let controller = rx
        .recv()
        .map_err(|_| "Inicio de sesión cancelado.")?
        .map_err(|_| "No se pudo crear la ventana WebView2.")?;
    let web = unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).map_err(|_| "No se pudo medir la ventana.")?;
        controller
            .SetBounds(rect)
            .map_err(|_| "No se pudo preparar la ventana.")?;
        controller
            .SetIsVisible(true)
            .map_err(|_| "No se pudo mostrar el navegador.")?;
        controller
            .CoreWebView2()
            .map_err(|_| "No se pudo iniciar el navegador.")?
    };
    CONTROLLER.with(|c| *c.borrow_mut() = Some(controller.clone()));
    let finished = Rc::new(Cell::new(false));
    let capture_finished = finished.clone();
    let mut request_token = 0;
    let mut navigation_token = 0;
    let mut new_window_token = 0;
    unsafe {
        let settings = web
            .Settings()
            .map_err(|_| "No se pudo configurar el navegador.")?;
        settings
            .SetAreDevToolsEnabled(false)
            .map_err(|_| "No se pudo configurar el navegador.")?;
        settings
            .SetIsStatusBarEnabled(false)
            .map_err(|_| "No se pudo configurar el navegador.")?;
        web.AddWebResourceRequestedFilter(
            w!("https://discord.com/api/*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )
        .map_err(|_| "No se pudo preparar el acceso autenticado.")?;
        web.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                if capture_finished.get() {
                    return Ok(());
                }
                let Some(args) = args else { return Ok(()) };
                let request = args.Request()?;
                let mut uri = PWSTR::null();
                request.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                if !captured_origin(&uri) {
                    return Ok(());
                }
                let headers = request.Headers()?;
                let mut header = PWSTR::null();
                if headers.GetHeader(w!("Authorization"), &mut header).is_err() {
                    return Ok(());
                }
                let token = take_authorization(header);
                if token.len() < 20 || token.len() > 4096 || token.contains(['\r', '\n']) {
                    return Ok(());
                }
                if let Ok(encrypted) = crate::storage::protect(token.as_bytes(), false, true).map(Zeroizing::new) {
                    if crate::storage::atomic_write(&handoff, &encrypted).is_ok() {
                        capture_finished.set(true);
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
                Ok(())
            })),
            &mut request_token,
        )
        .map_err(|_| "No se pudo observar el inicio de sesión propio.")?;
        web.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    if !allowed_navigation(&take_pwstr(uri)) {
                        args.SetCancel(true)?
                    }
                }
                Ok(())
            })),
            &mut navigation_token,
        )
        .map_err(|_| "No se pudo restringir el navegador de inicio de sesión.")?;
        web.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?
                }
                Ok(())
            })),
            &mut new_window_token,
        )
        .map_err(|_| "No se pudo restringir las ventanas del navegador.")?;
        web.Navigate(w!("https://discord.com/login"))
            .map_err(|_| "No se pudo abrir Discord.")?;
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result <= 0 {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        finished.set(true);
        let _ = web.remove_WebResourceRequested(request_token);
        let _ = web.remove_NavigationStarting(navigation_token);
        let _ = web.remove_NewWindowRequested(new_window_token);
        let _ = controller.Close();
    }
    Ok(())
}
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE => {
            CONTROLLER.with(|c| {
                if let Some(controller) = c.borrow().as_ref() {
                    unsafe {
                        let mut rect = RECT::default();
                        if GetClientRect(hwnd, &mut rect).is_ok() {
                            let _ = controller.SetBounds(rect);
                        }
                    }
                }
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_is_owned_validated_and_debug_output_is_redacted() {
        let secret = "synthetic-secret-never-a-real-token";
        let credential = Credential::from_secret(Zeroizing::new(secret.into())).unwrap();
        assert_eq!(credential.as_str(), secret);
        assert!(!format!("{credential:?}").contains(secret));
        let owned = credential.into_secret();
        assert_eq!(owned.as_str(), secret);
        for invalid in [
            "short",
            "synthetic-secret-with-newline\nvalue",
            "synthetic-secret-with-carriage\rvalue",
        ] {
            assert!(Credential::from_secret(Zeroizing::new(invalid.into())).is_err());
        }
    }
    #[test]
    fn synthetic_dpapi_handoff_returns_only_a_typed_secret_and_rejects_corruption() {
        let value = "synthetic-dpapi-handoff-never-a-real-token";
        let encrypted = crate::storage::protect(value.as_bytes(), false, true).unwrap();
        assert_eq!(
            Credential::from_handoff(&encrypted).unwrap().as_str(),
            value
        );
        assert!(Credential::from_handoff(b"corrupt synthetic handoff").is_err());
        let wrong_namespace = crate::storage::protect(value.as_bytes(), true, true).unwrap();
        assert!(Credential::from_handoff(&wrong_namespace).is_err());
    }
    #[test]
    fn only_own_discord_api_requests() {
        assert!(captured_origin("https://discord.com/api/v9/users/@me"));
        for uri in [
            "http://discord.com/api/v9/users/@me",
            "https://discord.com.evil.com/api/x",
            "https://user@discord.com/api/x",
            "https://discord.com:445/api/x",
            "https://discord.com/login",
        ] {
            assert!(!captured_origin(uri))
        }
    }
    #[test]
    fn navigation_and_handoff_are_scoped() {
        assert!(allowed_navigation("https://discord.com/login"));
        assert!(!allowed_navigation("https://discord.com.evil.com"));
        assert!(valid_job_id("0123456789abcdef0123456789abcdef"));
        assert!(!valid_job_id("../../another-profile"));
        assert!(!valid_job_id(""))
    }
}
