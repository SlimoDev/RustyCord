//! Per-user app identity and Start-menu shortcut, implemented without .NET.
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::{
    core::{Interface, GUID, PCWSTR, PWSTR},
    Win32::{
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
            StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0},
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        },
        System::Variant::VT_LPWSTR,
        UI::Shell::{
            IShellLinkW, PropertiesSystem::IPropertyStore, SetCurrentProcessExplicitAppUserModelID,
            ShellLink,
        },
    },
};
const APP_ID: &str = crate::brand::APP_ID;
const MAX_TOAST_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_CACHED_IMAGE_BYTES: u64 = 64 * 1024;
const TOAST_IMAGE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
static IMAGE_IO: Mutex<()> = Mutex::new(());
static IMAGE_HTTP: OnceLock<Option<reqwest::blocking::Client>> = OnceLock::new();

/// Unpackaged Win32 toasts cannot load HTTPS image sources. Resolve only Discord
/// origin images into a small local PNG, without credentials or redirects.
pub fn toast_image_url(value: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str() != Some("cdn.discordapp.com")
        || !["/avatars/", "/channel-icons/", "/icons/", "/embed/avatars/"]
            .iter()
            .any(|prefix| url.path().starts_with(prefix))
    {
        return None;
    }
    url.set_fragment(None);
    let mut pairs = url
        .query_pairs()
        .filter(|(key, _)| key != "size")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    pairs.push(("size".into(), "128".into()));
    url.query_pairs_mut().clear().extend_pairs(pairs);
    Some(url)
}
fn cached_image(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| {
        metadata.is_file()
            && metadata.len() > 0
            && metadata.len() <= MAX_CACHED_IMAGE_BYTES
            && metadata
                .modified()
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age < TOAST_IMAGE_TTL)
    })
}
fn write_toast_png(root: &Path, name: &str, rgba: image::RgbaImage) -> Option<PathBuf> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .ok()?;
    if bytes.get_ref().len() > MAX_CACHED_IMAGE_BYTES as usize {
        return None;
    }
    let _guard = IMAGE_IO.lock().ok()?;
    std::fs::create_dir_all(root).ok()?;
    let path = root.join(name);
    crate::storage::atomic_write(&path, bytes.get_ref()).ok()?;
    let mut cached = std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let entry_path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let owned = name == "fallback.png"
                || name.strip_suffix(".png").is_some_and(|stem| {
                    stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit())
                });
            let metadata = entry.metadata().ok()?;
            (owned && metadata.is_file())
                .then(|| (entry_path, metadata.modified().ok(), metadata.len()))
        })
        .collect::<Vec<_>>();
    cached.sort_by_key(|(_, modified, _)| *modified);
    let mut count = cached.len();
    let mut total = cached.iter().map(|(_, _, len)| *len).sum::<u64>();
    for (old, modified, len) in cached {
        if old == path || old.file_name().is_some_and(|name| name == "fallback.png") {
            continue;
        }
        if count <= 64
            && total <= 4 * 1024 * 1024
            && modified
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|age| age < TOAST_IMAGE_TTL)
        {
            continue;
        }
        if std::fs::remove_file(old).is_ok() {
            count = count.saturating_sub(1);
            total = total.saturating_sub(len);
        }
    }
    Some(path)
}
pub fn fallback_toast_image(root: &Path) -> Option<PathBuf> {
    let path = root.join("fallback.png");
    if cached_image(&path) {
        return Some(path);
    }
    let icon = image::load_from_memory(include_bytes!("../../../assets/discord.ico"))
        .ok()?
        .thumbnail(96, 96)
        .to_rgba8();
    write_toast_png(root, "fallback.png", icon)
}
pub fn prepare_toast_image(root: &Path, source: Option<&str>) -> Option<PathBuf> {
    let image = (|| {
        let url = toast_image_url(source?)?;
        let name = format!("{:x}.png", Sha256::digest(url.as_str().as_bytes()));
        let path = root.join(&name);
        if cached_image(&path) {
            return Some(path);
        }
        let client = IMAGE_HTTP
            .get_or_init(|| {
                reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(2))
                    .timeout(Duration::from_secs(4))
                    .build()
                    .ok()
            })
            .as_ref()?;
        let response = client.get(url).send().ok()?.error_for_status().ok()?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_TOAST_IMAGE_BYTES as u64)
        {
            return None;
        }
        let mut bytes = Vec::new();
        response
            .take(MAX_TOAST_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > MAX_TOAST_IMAGE_BYTES {
            return None;
        }
        let (width, height, rgba) = crate::platform::decode_system_image(&bytes, 96)?;
        write_toast_png(
            root,
            &name,
            image::RgbaImage::from_raw(width, height, rgba)?,
        )
    })();
    image.or_else(|| fallback_toast_image(root))
}
pub fn toast_image_uri(path: &str) -> Option<String> {
    let path = Path::new(path);
    let disk = matches!(path.components().next(),Some(std::path::Component::Prefix(prefix)) if matches!(prefix.kind(),std::path::Prefix::Disk(_)|std::path::Prefix::VerbatimDisk(_)));
    if !disk
        || !path.is_absolute()
        || !path.is_file()
        || path.extension().is_none_or(|extension| extension != "png")
        || !path.parent().is_some_and(|parent| {
            parent
                .file_name()
                .is_some_and(|name| name == "notification-images")
        })
    {
        return None;
    }
    let name = path.file_stem()?.to_str()?;
    if name != "fallback"
        && !(name.len() == 64 && name.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return None;
    }
    url::Url::from_file_path(path)
        .ok()
        .map(|url| url.to_string())
}
#[cfg(test)]
mod image_tests {
    use super::*;
    #[test]
    fn toast_images_allow_only_discord_origin_paths_without_credentials_or_redirects() {
        for source in [
            "https://cdn.discordapp.com/avatars/2/a.png?size=4096",
            "https://cdn.discordapp.com/channel-icons/20/a.png",
            "https://cdn.discordapp.com/icons/10/a.png",
            "https://cdn.discordapp.com/embed/avatars/1.png",
        ] {
            let url = toast_image_url(source).unwrap();
            assert!(url
                .query_pairs()
                .any(|(key, value)| key == "size" && value == "128"));
        }
        for source in [
            "http://cdn.discordapp.com/avatars/2/a.png",
            "https://cdn.discordapp.com.evil.test/avatars/2/a.png",
            "https://user@cdn.discordapp.com/avatars/2/a.png",
            "https://cdn.discordapp.com:444/avatars/2/a.png",
            "https://cdn.discordapp.com/attachments/1/file.png",
            "file:///C:/photo.png",
        ] {
            assert!(toast_image_url(source).is_none(), "{source}");
        }
    }
    #[test]
    fn fallback_is_a_bounded_local_png_and_external_paths_cannot_enter_toast_xml() {
        let root = std::env::temp_dir().join(format!(
            "notdiscord-notification-test-{}",
            uuid::Uuid::new_v4()
        ));
        let cache = root.join("notification-images");
        let path = prepare_toast_image(&cache, None).unwrap();
        let (width, height) = image::image_dimensions(&path).unwrap();
        assert!(width <= 96 && height <= 96);
        assert!(std::fs::metadata(&path).unwrap().len() < MAX_CACHED_IMAGE_BYTES);
        let uri = toast_image_uri(path.to_str().unwrap()).unwrap();
        assert!(uri.starts_with("file:///"));
        assert!(toast_image_uri("https://cdn.discordapp.com/avatars/2/a.png").is_none());
        let external = root.join("external.png");
        std::fs::copy(&path, &external).unwrap();
        assert!(toast_image_uri(external.to_str().unwrap()).is_none());
        assert_eq!(prepare_toast_image(&cache, None).unwrap(), path);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn toast_cache_prunes_only_owned_files_and_preserves_latest_image() {
        let root = std::env::temp_dir()
            .join(format!(
                "notdiscord-notification-cache-{}",
                uuid::Uuid::new_v4()
            ))
            .join("notification-images");
        std::fs::create_dir_all(&root).unwrap();
        let unrelated = root.join("user-image.png");
        std::fs::write(&unrelated, b"user data").unwrap();
        for index in 0..70 {
            std::fs::write(root.join(format!("{index:064x}.png")), b"old fixture").unwrap();
        }
        let current = write_toast_png(
            &root,
            &format!("{:064x}.png", 999),
            image::RgbaImage::from_pixel(4, 4, image::Rgba([120, 60, 180, 255])),
        )
        .unwrap();
        let owned = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().len() == 68)
            .count();
        assert!(owned <= 64);
        assert!(current.is_file());
        assert!(unrelated.is_file());
        std::fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
}
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
pub fn set_app_id() {
    let id = wide(std::ffi::OsStr::new(APP_ID));
    let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr())) };
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegCreateKeyExW(
        key: *mut std::ffi::c_void,
        subkey: *const u16,
        reserved: u32,
        class: *mut u16,
        options: u32,
        access: u32,
        security: *const std::ffi::c_void,
        result: *mut *mut std::ffi::c_void,
        disposition: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        key: *mut std::ffi::c_void,
        name: *const u16,
        reserved: u32,
        kind: u32,
        data: *const u8,
        bytes: u32,
    ) -> i32;
    fn RegCloseKey(key: *mut std::ffi::c_void) -> i32;
}
pub fn register(exe: &Path, runtime: &Path, data: &Path) -> Result<(), String> {
    let programs = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .ok_or("Carpeta de programas no disponible.")?
        .join("Microsoft/Windows/Start Menu/Programs");
    std::fs::create_dir_all(&programs)
        .map_err(|_| "No se pudo preparar la identidad de avisos.")?;
    let icon = runtime.join("discord.ico");
    let icon = if icon.is_file() {
        icon
    } else {
        exe.to_path_buf()
    };
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = (|| -> windows::core::Result<()> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            let executable = wide(exe.as_os_str());
            let working = wide(runtime.as_os_str());
            let icon = wide(icon.as_os_str());
            let args = format!(
                "--runtime-root \"{}\" --data-dir \"{}\"",
                runtime.display(),
                data.display()
            );
            let args = wide(std::ffi::OsStr::new(&args));
            link.SetPath(PCWSTR(executable.as_ptr()))?;
            link.SetWorkingDirectory(PCWSTR(working.as_ptr()))?;
            link.SetArguments(PCWSTR(args.as_ptr()))?;
            link.SetIconLocation(PCWSTR(icon.as_ptr()), 0)?;
            let properties: IPropertyStore = link.cast()?;
            let mut id = wide(std::ffi::OsStr::new(APP_ID));
            let key = PROPERTYKEY {
                fmtid: GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
                pid: 5,
            };
            // SetValue copies this borrowed string. Do not PropVariantClear memory owned by Vec.
            // PROPVARIANT itself implements Drop/PropVariantClear. This value
            // borrows Vec storage, so suppress its outer destructor as well.
            let value = std::mem::ManuallyDrop::new(PROPVARIANT {
                Anonymous: PROPVARIANT_0 {
                    Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                        vt: VT_LPWSTR,
                        wReserved1: 0,
                        wReserved2: 0,
                        wReserved3: 0,
                        Anonymous: PROPVARIANT_0_0_0 {
                            pwszVal: PWSTR(id.as_mut_ptr()),
                        },
                    }),
                },
            });
            properties.SetValue(&key, &*value)?;
            properties.Commit()?;
            let persist: IPersistFile = link.cast()?;
            let destination = wide(programs.join(crate::brand::START_MENU_SHORTCUT).as_os_str());
            persist.Save(PCWSTR(destination.as_ptr()), true)?;
            Ok(())
        })();
        if initialized {
            CoUninitialize();
        }
        result.map_err(|_| "Windows no pudo registrar el acceso para avisos.")?;
        let subkey = wide(std::ffi::OsStr::new(&format!(
            "Software\\Classes\\AppUserModelId\\{APP_ID}"
        )));
        let mut key = std::ptr::null_mut();
        if RegCreateKeyExW(
            0x80000001u32 as i32 as isize as *mut std::ffi::c_void,
            subkey.as_ptr(),
            0,
            std::ptr::null_mut(),
            0,
            2,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != 0
        {
            return Err("Windows no pudo registrar la identidad de avisos.".into());
        }
        let mut success = true;
        for (name, value) in [
            ("DisplayName", crate::brand::NAME.into()),
            ("IconUri", icon.to_string_lossy().into_owned()),
            ("NotDiscordExecutable", exe.to_string_lossy().into_owned()),
        ] {
            let name = wide(std::ffi::OsStr::new(name));
            let value = wide(std::ffi::OsStr::new(&value));
            success &= RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                1,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            ) == 0;
        }
        RegCloseKey(key);
        if !success {
            return Err("Windows no pudo guardar la identidad de avisos.".into());
        }
    }
    Ok(())
}
