//! System-composited backdrop in a private sibling HWND behind the WGL root.
//! Production never reads back desktop pixels or blurs foreground content.
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::OnceLock;
use windows::{
    Foundation::{IPropertyValue, PropertyValue},
    Graphics::Effects::{
        IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl,
    },
    System::{DispatcherQueue, DispatcherQueueController},
    UI::Composition::{
        CompositionEffectBrush, CompositionEffectFactory, CompositionEffectSourceParameter,
        Compositor, Desktop::DesktopWindowTarget, SpriteVisual,
    },
    Win32::{
        Foundation::{
            E_BOUNDS, E_INVALIDARG, E_POINTER, ERROR_CLASS_ALREADY_EXISTS, FreeLibrary,
            GetLastError, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
        },
        Graphics::{
            Direct2D::CLSID_D2D1GaussianBlur,
            Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute},
            Gdi::ClientToScreen,
        },
        System::{
            LibraryLoader::{
                GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
            },
            WinRT::{
                Composition::ICompositorDesktopInterop,
                DQTAT_COM_STA, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
                Graphics::Direct2D::{
                    GRAPHICS_EFFECT_PROPERTY_MAPPING, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT,
                    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
                },
            },
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, IsIconic,
            IsWindowVisible, RegisterClassW, SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos,
            ShowWindow, WM_ERASEBKGND, WM_NCDESTROY, WM_NCHITTEST, WM_SHOWWINDOW, WM_SIZE,
            WM_WINDOWPOSCHANGED, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
            WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
        },
    },
    core::{BOOL, GUID, HRESULT, HSTRING, Interface, PCSTR, PCWSTR, Result, implement, w},
};

// Direct2D effect metadata accepted by Windows Composition. This uses the
// system Gaussian effect directly, without a Win2D runtime or desktop readback.
#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct GaussianBlur {
    source: IGraphicsEffectSource,
}
#[allow(non_snake_case)]
impl IGraphicsEffectSource_Impl for GaussianBlur_Impl {}
#[allow(non_snake_case)]
impl IGraphicsEffect_Impl for GaussianBlur_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(HSTRING::from("Blur"))
    }
    fn SetName(&self, name: &HSTRING) -> Result<()> {
        if name == "Blur" {
            Ok(())
        } else {
            Err(E_INVALIDARG.into())
        }
    }
}
#[allow(non_snake_case)]
impl IGraphicsEffectD2D1Interop_Impl for GaussianBlur_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }
    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        if name.is_null() || index.is_null() || mapping.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let value = unsafe { name.to_string()? };
        let property = match value.as_str() {
            "StandardDeviation" => 0,
            "Optimization" => 1,
            "BorderMode" => 2,
            _ => return Err(E_INVALIDARG.into()),
        };
        unsafe {
            index.write(property);
            mapping.write(GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT);
        }
        Ok(())
    }
    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }
    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingle(0.)?.cast(),
            1 => PropertyValue::CreateUInt32(1)?.cast(), // balanced optimization
            2 => PropertyValue::CreateUInt32(1)?.cast(), // hard border
            _ => Err(E_BOUNDS.into()),
        }
    }
    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(E_BOUNDS.into())
        }
    }
    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
}

type Subclass = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT;
type CreateQueue =
    unsafe extern "system" fn(DispatcherQueueOptions, *mut *mut core::ffi::c_void) -> HRESULT;
type SetSubclass = unsafe extern "system" fn(HWND, Option<Subclass>, usize, usize) -> BOOL;
type RemoveSubclass = unsafe extern "system" fn(HWND, Option<Subclass>, usize) -> BOOL;
type DefaultSubclass = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
struct NativeApis {
    // Retain one process-lifetime reference after the first positive blur level.
    // A hook/COM object must never call code from an unloaded library. Nothing
    // loads these DLLs when blur stays at zero (normal PE imports were removed).
    _libraries: [usize; 2],
    create_queue: CreateQueue,
    set_subclass: SetSubclass,
    remove_subclass: RemoveSubclass,
    default_subclass: DefaultSubclass,
}
static NATIVE_APIS: OnceLock<std::result::Result<NativeApis, HRESULT>> = OnceLock::new();
fn native_apis() -> Result<&'static NativeApis> {
    NATIVE_APIS
        .get_or_init(|| unsafe { NativeApis::load().map_err(|error| error.code()) })
        .as_ref()
        .map_err(|code| windows::core::Error::from_hresult(*code))
}
impl NativeApis {
    unsafe fn load() -> Result<Self> {
        unsafe {
            let messaging =
                LoadLibraryExW(w!("CoreMessaging.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)?;
            let controls =
                match LoadLibraryExW(w!("comctl32.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) {
                    Ok(library) => library,
                    Err(error) => {
                        let _ = FreeLibrary(messaging);
                        return Err(error);
                    }
                };
            let loaded = (|| -> Result<Self> {
                let address = |library: HMODULE, name: &'static [u8]| {
                    GetProcAddress(library, PCSTR(name.as_ptr()))
                        .ok_or_else(windows::core::Error::from_win32)
                };
                Ok(Self {
                    _libraries: [messaging.0 as usize, controls.0 as usize],
                    create_queue: std::mem::transmute::<
                        unsafe extern "system" fn() -> isize,
                        CreateQueue,
                    >(address(
                        messaging,
                        b"CreateDispatcherQueueController\0",
                    )?),
                    set_subclass: std::mem::transmute::<
                        unsafe extern "system" fn() -> isize,
                        SetSubclass,
                    >(address(controls, b"SetWindowSubclass\0")?),
                    remove_subclass: std::mem::transmute::<
                        unsafe extern "system" fn() -> isize,
                        RemoveSubclass,
                    >(address(
                        controls,
                        b"RemoveWindowSubclass\0",
                    )?),
                    default_subclass: std::mem::transmute::<
                        unsafe extern "system" fn() -> isize,
                        DefaultSubclass,
                    >(address(
                        controls,
                        b"DefSubclassProc\0",
                    )?),
                })
            })();
            if loaded.is_err() {
                let _ = FreeLibrary(controls);
                let _ = FreeLibrary(messaging);
            }
            loaded
        }
    }
    unsafe fn queue(&self) -> Result<DispatcherQueueController> {
        unsafe {
            let mut raw = core::ptr::null_mut();
            (self.create_queue)(
                DispatcherQueueOptions {
                    dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                    threadType: DQTYPE_THREAD_CURRENT,
                    apartmentType: DQTAT_COM_STA,
                },
                &mut raw,
            )
            .ok()?;
            if raw.is_null() {
                return Err(E_POINTER.into());
            }
            Ok(DispatcherQueueController::from_raw(raw))
        }
    }
}
const SUBCLASS_ID: usize = 0x4e44424c;
/// Blend the system host backdrop with the untouched desktop as well as varying
/// our additional Gaussian radius. HostBackdropBrush may already be softened by
/// Windows; varying only the radius made that base blur appear at full strength
/// as soon as the slider crossed zero. The transparent sibling exposes the
/// original desktop below it, so its opacity provides a continuous 0..1 mix.
fn blur_parameters(level: u8) -> (f32, f32) {
    let fraction = f32::from(level.min(100)) / 100.;
    (fraction, 30. * fraction * fraction)
}
struct LayerHost {
    root: HWND,
    layer: HWND,
    apis: &'static NativeApis,
}
impl LayerHost {
    fn follow(&self) -> Result<()> {
        unsafe {
            if !IsWindowVisible(self.root).as_bool() || IsIconic(self.root).as_bool() {
                let _ = ShowWindow(self.layer, SW_HIDE);
                return Ok(());
            }
            let mut rect = RECT::default();
            GetClientRect(self.root, &mut rect)?;
            let mut origin = POINT { x: 0, y: 0 };
            if !ClientToScreen(self.root, &mut origin).as_bool() {
                return Err(windows::core::Error::from_win32());
            }
            SetWindowPos(
                self.layer,
                Some(self.root),
                origin.x,
                origin.y,
                rect.right,
                rect.bottom,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
    }
}
unsafe extern "system" fn root_hook(
    hwnd: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    unsafe {
        if data != 0 {
            let host = &*(data as *const LayerHost);
            if message == WM_NCDESTROY {
                let _ = ShowWindow(host.layer, SW_HIDE);
                let _ = (host.apis.remove_subclass)(hwnd, Some(root_hook), id);
            } else if matches!(message, WM_WINDOWPOSCHANGED | WM_SHOWWINDOW | WM_SIZE) {
                let _ = host.follow();
            }
        }
        if let Some(Ok(apis)) = NATIVE_APIS.get() {
            (apis.default_subclass)(hwnd, message, w, l)
        } else {
            DefWindowProcW(hwnd, message, w, l)
        }
    }
}
unsafe extern "system" fn layer_proc(hwnd: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match message {
        WM_NCHITTEST => LRESULT(-1),
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, message, w, l) },
    }
}
#[derive(Default)]
pub struct Backdrop {
    active: Option<NativeBackdrop>,
    attempted: Option<(isize, u8)>,
    pub error: Option<String>,
}
struct NativeBackdrop {
    target: DesktopWindowTarget,
    visual: SpriteVisual,
    compositor: Compositor,
    effect: CompositionEffectBrush,
    factory: CompositionEffectFactory,
    _queue: Option<DispatcherQueueController>,
    host: Box<LayerHost>,
    size: (i32, i32),
}
impl Backdrop {
    pub fn update(&mut self, frame: &eframe::Frame, amount: f32, opacity: f32, visible: bool) {
        let Ok(handle) = frame.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        self.set(
            handle.hwnd.get(),
            if visible && opacity < 1. { amount } else { 0. },
        );
    }
    pub fn set(&mut self, hwnd: isize, amount: f32) {
        let level = if amount.is_finite() {
            amount.clamp(0., 100.).round() as u8
        } else {
            0
        };
        if level == 0 {
            self.active = None;
            self.attempted = None;
            self.error = None;
            return;
        }
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.host.root.0 as isize != hwnd)
        {
            self.active = None;
        }
        if self.attempted == Some((hwnd, level)) {
            if let Some(active) = &mut self.active {
                let _ = active.resize();
            }
            return;
        }
        let result = (|| -> Result<()> {
            if self.active.is_none() {
                self.active = Some(NativeBackdrop::new(HWND(hwnd as _))?);
            }
            let active = self.active.as_mut().unwrap();
            active.resize()?;
            let (opacity, sigma) = blur_parameters(level);
            active.visual.SetOpacity(opacity)?;
            active
                .effect
                .Properties()?
                .InsertScalar(&HSTRING::from("Blur.StandardDeviation"), sigma)
        })();
        self.attempted = Some((hwnd, level));
        self.error=result.err().map(|_|"El desenfoque nativo no está disponible en este sistema o está desactivado por Windows.".into());
        if self.error.is_some() {
            self.active = None;
        }
    }
    pub fn active(&self) -> bool {
        self.active.is_some()
    }
    pub fn native_window(&self) -> Option<isize> {
        self.active
            .as_ref()
            .map(|active| active.host.layer.0 as isize)
    }
}
impl NativeBackdrop {
    fn new(root: HWND) -> Result<Self> {
        unsafe {
            let apis = native_apis()?;
            let queue = if DispatcherQueue::GetForCurrentThread().is_ok() {
                None
            } else {
                Some(apis.queue()?)
            };
            let instance = HINSTANCE(GetModuleHandleW(None)?.0);
            let class = WNDCLASSW {
                lpfnWndProc: Some(layer_proc),
                hInstance: instance,
                lpszClassName: w!("NotDiscordBackdropLayer"),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(windows::core::Error::from_win32());
            }
            let layer = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOREDIRECTIONBITMAP,
                w!("NotDiscordBackdropLayer"),
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance),
                None,
            )?;
            let host = Box::new(LayerHost { root, layer, apis });
            let build = (|| -> Result<Self> {
                let compositor = Compositor::new()?;
                let interop: ICompositorDesktopInterop = compositor.cast()?;
                let target = interop.CreateDesktopWindowTarget(layer, false)?;
                let enabled = BOOL(1);
                DwmSetWindowAttribute(
                    layer,
                    DWMWA_USE_HOSTBACKDROPBRUSH,
                    &enabled as *const _ as _,
                    std::mem::size_of::<BOOL>() as u32,
                )?;
                let visual = compositor.CreateSpriteVisual()?;
                let source = CompositionEffectSourceParameter::Create(&HSTRING::from("Backdrop"))?;
                let definition: IGraphicsEffect = GaussianBlur {
                    source: source.cast()?,
                }
                .into();
                let properties: windows_collections::IIterable<HSTRING> =
                    vec![HSTRING::from("Blur.StandardDeviation")].into();
                let factory =
                    compositor.CreateEffectFactoryWithProperties(&definition, &properties)?;
                let effect = factory.CreateBrush()?;
                effect.SetSourceParameter(
                    &HSTRING::from("Backdrop"),
                    &compositor.CreateHostBackdropBrush()?,
                )?;
                visual.SetBrush(&effect)?;
                target.SetRoot(&visual)?;
                Ok(Self {
                    target,
                    visual,
                    compositor,
                    effect,
                    factory,
                    _queue: queue,
                    host,
                    size: (0, 0),
                })
            })();
            let mut result = match build {
                Ok(result) => result,
                Err(error) => {
                    let _ = DestroyWindow(layer);
                    return Err(error);
                }
            };
            if !(apis.set_subclass)(
                root,
                Some(root_hook),
                SUBCLASS_ID,
                (&*result.host as *const LayerHost) as usize,
            )
            .as_bool()
            {
                return Err(windows::core::Error::from_win32());
            }
            result.resize()?;
            Ok(result)
        }
    }
    fn resize(&mut self) -> Result<()> {
        self.host.follow()?;
        let mut rect = RECT::default();
        unsafe {
            GetClientRect(self.host.root, &mut rect)?;
        }
        let size = (rect.right.max(0), rect.bottom.max(0));
        if size != self.size {
            self.visual.SetSize(windows_numerics::Vector2 {
                X: size.0 as f32,
                Y: size.1 as f32,
            })?;
            self.size = size;
        }
        Ok(())
    }
}
impl Drop for NativeBackdrop {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.host.apis.remove_subclass)(self.host.root, Some(root_hook), SUBCLASS_ID);
            let _ = ShowWindow(self.host.layer, SW_HIDE);
            let _ = self
                .target
                .SetRoot(None::<&windows::UI::Composition::Visual>);
            let _ = self.target.Close();
            let _ = self.visual.Close();
            let _ = self.effect.Close();
            let _ = self.factory.Close();
            let _ = self.compositor.Close();
            let _ = DestroyWindow(self.host.layer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_blends_continuously_instead_of_enabling_a_full_opacity_host_brush() {
        assert_eq!(blur_parameters(0), (0., 0.));
        assert_eq!(blur_parameters(100), (1., 30.));
        assert_eq!(blur_parameters(255), (1., 30.));
        let mut previous = (0., 0.);
        for level in 1..=100 {
            let current = blur_parameters(level);
            assert!(current.0 > previous.0 && current.1 > previous.1);
            assert!(current.0 - previous.0 < 0.011);
            previous = current;
        }
        assert!(blur_parameters(1).0 < 0.02);
        assert!(blur_parameters(1).1 < 0.01);
    }
}
