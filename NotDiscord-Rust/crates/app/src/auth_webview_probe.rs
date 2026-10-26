//! Explicitly invoked native compatibility probe; never built into NotDiscord.
//! A fresh, owned profile loads only about:blank inside the real login Job.
//! External navigation/resource requests are rejected, SSO/extensions are off,
//! and Chromium networking is sent to a held local socket with no upstream.
//! These guards are not an OS network sandbox: run with network disconnected
//! when validating absence of WebView runtime background traffic as well.
use super::*;
use std::{cell::Cell, net::TcpListener, rc::Rc, sync::mpsc, time::Instant};
use webview2_com::{
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, Microsoft::Web::WebView2::Win32::*,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, WebResourceRequestedEventHandler, take_pwstr,
};
use windows::{
    Win32::{
        Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, IStream},
            LibraryLoader::GetModuleHandleW,
        },
        UI::WindowsAndMessaging::*,
    },
    core::{BOOL, Interface, PCWSTR, PWSTR, w},
};

const HELPER: &str = "authentication::lifecycle::webview_probe::native_webview2_probe_child";
const PREFIX: &str = "notdiscord-auth-webview-";
const ROOT_ENV: &str = "NOTDISCORD_SYNTHETIC_WEBVIEW_ROOT";
const ID_ENV: &str = "NOTDISCORD_SYNTHETIC_WEBVIEW_ID";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("{PREFIX}{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
fn owned_root(path: &Path) -> bool {
    let Ok(root) = fs::canonicalize(path) else {
        return false;
    };
    let Ok(temporary) = fs::canonicalize(std::env::temp_dir()) else {
        return false;
    };
    root.parent() == Some(temporary.as_path())
        && root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.strip_prefix(PREFIX)
                    .is_some_and(super::super::valid_job_id)
            })
        && plain_metadata(&root).is_some_and(|metadata| metadata.is_dir())
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !owned_root(&self.0) {
            return;
        }
        // Retain a failed cleanup's ownership record rather than conceal it.
        let Ok(report) = recover_inactive(&self.0) else {
            return;
        };
        if report.active != 0 || report.deferred != 0 {
            return;
        }
        if fs::read_dir(self.0.join("login-jobs")).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().extension().is_some_and(|e| e == "json"))
        }) {
            return;
        }
        if safe_profile_tree(&self.0, 0, &mut 20_000) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

/// No-op in the ordinary suite. Only a specifically spawned test helper joins
/// the Job; the parent test harness can never assign itself by calling this.
#[test]
fn native_webview2_probe_child() {
    let (Some(root), Ok(id)) = (std::env::var_os(ROOT_ENV), std::env::var(ID_ENV)) else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(owned_root(&root) && super::super::valid_job_id(&id));
    let result = run_child(&root, &id);
    if let Err(error) = &result {
        let _ = fs::write(root.join("probe-error.txt"), error.as_bytes());
    }
    result.unwrap();
}

#[tokio::test]
#[ignore = "Requires installed native WebView2; fresh about:blank only. Disconnect network to also isolate runtime background services."]
async fn native_webview2_job_compatibility() {
    run_case(false).await.unwrap();
    run_case(true).await.unwrap();
}

async fn run_case(cancel: bool) -> Result<(), String> {
    let fixture = Fixture::new();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut resources = Resources::prepare(&fixture.0, &id)?;
    let paths = resources.job.as_ref().unwrap().paths.clone();
    let ready = fixture.0.join("probe-ready.txt");
    let operation: Result<(), String> = async {
        let child =
            tokio::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                .args(["--exact", HELPER, "--nocapture"])
                .env(ROOT_ENV, &fixture.0)
                .env(ID_ENV, &id)
                // Runtime environment overrides must point at the same fresh
                // profile, never at an inherited user's browser profile.
                .env("WEBVIEW2_USER_DATA_FOLDER", paths.profile())
                .env_remove("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
                .env_remove("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER")
                .env_remove("WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER")
                .env_remove("WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER")
                .kill_on_drop(true)
                .creation_flags(0x08000000)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
        resources.attach(child);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while !ready.exists() {
            if let Some(status) = resources
                .child_mut()
                .try_wait()
                .map_err(|e| e.to_string())?
            {
                let detail = crate::storage::read_bounded(&fixture.0.join("probe-error.txt"), 1024)
                    .map(|value| String::from_utf8_lossy(&value).into_owned())
                    .unwrap_or_default();
                return Err(format!("Private WebView probe exited {status}: {detail}"));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(
                    "Private WebView probe did not complete about:blank within 30 seconds".into(),
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let proof =
            crate::storage::read_bounded(&ready, 128).ok_or("Missing bounded probe proof")?;
        let proof = std::str::from_utf8(&proof).map_err(|_| "Invalid probe proof")?;
        let (source, process) = proof.split_once('\n').ok_or("Incomplete probe proof")?;
        if source != "about:blank" {
            return Err("Unexpected probe navigation".into());
        }
        let browser_pid: u32 = process.parse().map_err(|_| "Invalid probe process")?;
        let tree = resources
            .job
            .as_ref()
            .unwrap()
            .process_tree
            .as_ref()
            .unwrap();
        if tree.active_for_test() < 2 || !tree.contains_process_for_test(browser_pid)? {
            return Err("WebView browser did not inherit the owned process Job".into());
        }
        if !cancel {
            fs::write(fixture.0.join("probe-close.txt"), b"close").map_err(|e| e.to_string())?;
            let status = tokio::time::timeout(Duration::from_secs(8), resources.child_mut().wait())
                .await
                .map_err(|_| "WebView did not close within eight seconds")?
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err("WebView graceful close failed".into());
            }
        }
        Ok(())
    }
    .await;
    let cleanup = if cancel {
        let state = resources.state.clone();
        let failure = resources.failure.clone();
        drop(resources);
        tokio::time::timeout(Duration::from_secs(8), async {
            while state.load(Ordering::Acquire) == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| "WebView cancellation cleanup timed out")?;
        if state.load(Ordering::Acquire) != 1 {
            Err(failure
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| CLEANUP_FAILED.into()))
        } else {
            Ok(())
        }
    } else {
        resources.finish().await
    };
    operation?;
    cleanup?;
    if paths.profile().exists() || paths.record().exists() || paths.handoff().exists() {
        return Err("Private WebView resources remained after cleanup".into());
    }
    Ok(())
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}
struct ProbeWindow(HWND);
impl Drop for ProbeWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
struct ProbeController(ICoreWebView2Controller);
impl Drop for ProbeController {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Close();
        }
    }
}

fn run_child(root: &Path, id: &str) -> Result<(), String> {
    let lease = ChildLease::acquire(root, id).ok_or("Missing owned probe lease")?;
    lease.join_process()?; // Must precede every possible WebView process spawn.
    fs::create_dir(lease.profile()).map_err(|e| e.to_string())?;
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?
    };
    let _apartment = Apartment;
    let window = unsafe {
        let instance = HINSTANCE(GetModuleHandleW(None).map_err(|e| e.to_string())?.0);
        let class = WNDCLASSW {
            hInstance: instance,
            lpfnWndProc: Some(probe_window_proc),
            lpszClassName: w!("NotDiscordPrivateOfflineWebViewProbe"),
            ..Default::default()
        };
        RegisterClassW(&class);
        ProbeWindow(
            CreateWindowExW(
                Default::default(),
                class.lpszClassName,
                w!("Private offline about:blank probe"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                240,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|e| e.to_string())?,
        )
    };
    // Holding the bound socket prevents port reuse; nothing accepts requests or
    // forwards bytes. Only loopback can be contacted by Chromium's proxy path.
    let local_proxy =
        TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    let options = CoreWebView2EnvironmentOptions::default();
    unsafe {
        options.set_allow_single_sign_on_using_os_primary_account(false);
        options.set_are_browser_extensions_enabled(false);
        options.set_exclusive_user_data_folder_access(true);
        options.set_is_custom_crash_reporting_enabled(true);
        options.set_additional_browser_arguments(format!(
            "--proxy-server=http://{} --proxy-bypass-list=<-loopback> --host-resolver-rules=\"MAP * ~NOTFOUND\" --disable-quic --disable-background-networking --disable-component-update --disable-sync --no-first-run",
            local_proxy.local_addr().map_err(|e| e.to_string())?
        ));
    }
    let options: ICoreWebView2EnvironmentOptions = options.into();
    let profile: Vec<u16> = lease
        .profile()
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
                &options,
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
    .map_err(|e| format!("Create private WebView environment: {e}"))?;
    let environment = rx
        .recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    // Verify the runtime did not apply an inherited profile-directory override.
    let environment7: ICoreWebView2Environment7 = environment.cast().map_err(|e| e.to_string())?;
    let mut actual_profile = PWSTR::null();
    unsafe {
        environment7
            .UserDataFolder(&mut actual_profile)
            .map_err(|e| e.to_string())?
    };
    let actual_profile = PathBuf::from(take_pwstr(actual_profile));
    if fs::canonicalize(actual_profile).map_err(|e| e.to_string())?
        != fs::canonicalize(lease.profile()).map_err(|e| e.to_string())?
    {
        return Err("Runtime selected a profile outside this fresh fixture".into());
    }
    let hwnd = window.0;
    let environment_for_controller = environment.clone();
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            environment_for_controller
                .CreateCoreWebView2Controller(hwnd, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |error, controller| {
            error?;
            let _ = tx.send(controller.ok_or_else(|| windows::core::Error::from(E_POINTER)));
            Ok(())
        }),
    )
    .map_err(|e| format!("Create private WebView controller: {e}"))?;
    let controller = ProbeController(
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?,
    );
    unsafe {
        controller
            .0
            .SetIsVisible(false)
            .map_err(|e| e.to_string())?
    };
    let web = unsafe { controller.0.CoreWebView2().map_err(|e| e.to_string())? };
    let completed = Rc::new(Cell::new(false));
    let succeeded = Rc::new(Cell::new(false));
    let mut navigation_token = 0;
    let mut new_window_token = 0;
    let mut resource_token = 0;
    let mut completion_token = 0;
    unsafe {
        web.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut uri = PWSTR::null();
                args.Uri(&mut uri)?;
                args.SetCancel(take_pwstr(uri) != "about:blank")?;
                Ok(())
            })),
            &mut navigation_token,
        )
        .map_err(|e| e.to_string())?;
        web.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?
                }
                Ok(())
            })),
            &mut new_window_token,
        )
        .map_err(|e| e.to_string())?;
        web.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
            .map_err(|e| e.to_string())?;
        let blocked_response = environment
            .CreateWebResourceResponse(
                None::<&IStream>,
                403,
                w!("Blocked by private offline fixture"),
                w!("Content-Length: 0\r\n"),
            )
            .map_err(|e| e.to_string())?;
        web.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetResponse(&blocked_response)?
                }
                Ok(())
            })),
            &mut resource_token,
        )
        .map_err(|e| e.to_string())?;
        let done = completed.clone();
        let success = succeeded.clone();
        web.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                let mut successful = BOOL::default();
                if let Some(args) = args {
                    args.IsSuccess(&mut successful)?
                }
                success.set(successful.as_bool());
                done.set(true);
                Ok(())
            })),
            &mut completion_token,
        )
        .map_err(|e| e.to_string())?;
        web.Navigate(w!("about:blank")).map_err(|e| e.to_string())?;
    }
    pump_until(Duration::from_secs(10), || completed.get())?;
    if !succeeded.get() {
        return Err("Private about:blank navigation failed".into());
    }
    let mut source = PWSTR::null();
    let mut browser_pid = 0;
    unsafe {
        web.Source(&mut source).map_err(|e| e.to_string())?;
        web.BrowserProcessId(&mut browser_pid)
            .map_err(|e| e.to_string())?;
    }
    if take_pwstr(source) != "about:blank" || browser_pid == 0 {
        return Err("Unexpected private WebView source or process".into());
    }
    crate::storage::atomic_write(
        &root.join("probe-ready.txt"),
        format!("about:blank\n{browser_pid}").as_bytes(),
    )?;
    pump_until(Duration::from_secs(40), || {
        root.join("probe-close.txt").exists()
    })?;
    unsafe {
        let _ = web.remove_NavigationCompleted(completion_token);
        let _ = web.remove_WebResourceRequested(resource_token);
        let _ = web.remove_NewWindowRequested(new_window_token);
        let _ = web.remove_NavigationStarting(navigation_token);
    }
    Ok(())
}

fn pump_until(timeout: Duration, done: impl Fn() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while !done() {
        unsafe {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == WM_QUIT {
                    return Err("Private probe received WM_QUIT".into());
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if Instant::now() >= deadline {
            return Err("Private WebView message-pump deadline expired".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
unsafe extern "system" fn probe_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}
