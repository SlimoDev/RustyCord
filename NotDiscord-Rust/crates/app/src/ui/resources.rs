use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions, Vec2};
use image::AnimationDecoder;
#[path = "avatar_cache.rs"]
mod avatar_cache;
#[path = "image_spool.rs"]
mod image_spool;
#[path = "resources/custom_animation.rs"]
mod custom_animation;
use avatar_cache::AvatarCache;
use image_spool::{DecodeGate, DecodePermit, EncodedSpool};
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
const BUDGET: usize = 8 * 1024 * 1024;
const MAX_TEXTURES: usize = 256;
const AVATAR_SIDE: u32 = 384;
const MAX_AVATAR_USERS: usize = 512;
const MAX_ENCODED: u64 = super::media_limit::MAX_BYTES;
#[cfg(test)]
#[path = "resources/p2_offline_soak.rs"]
mod p2_offline_soak;
#[cfg(test)]
#[path = "resources/p2_emoji_cold.rs"]
mod p2_emoji_cold;
type Pixels = (u32, u32, Vec<u8>);
type PictureResult = Result<Pixels, String>;
type AnimationFrames = Vec<(u32, u32, Vec<u8>, Duration)>;
#[derive(Clone)]
enum PictureSource {
    Remote(String),
    Local(PathBuf),
}
struct PictureRequest {
    generation: u64,
    key: String,
    source: PictureSource,
    side: u32,
}
pub struct PastedAttachment {
    pub channel: String,
    pub account: String,
    result: Result<Option<PathBuf>, String>,
}
impl PastedAttachment {
    pub fn take_result(&mut self) -> Result<Option<PathBuf>, String> {
        std::mem::replace(&mut self.result, Ok(None))
    }
}
impl Drop for PastedAttachment {
    fn drop(&mut self) {
        if let Ok(Some(path)) = &self.result {
            let _ = std::fs::remove_file(path);
        }
    }
}
pub type EmojiRequest = (u64, (String, u32));
pub type EmojiResult = (u64, (String, u32), Option<(u32, u32, Vec<u8>)>);
pub fn emoji_worker(
    ctx: egui::Context,
    suspended: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
) -> (mpsc::SyncSender<EmojiRequest>, mpsc::Receiver<EmojiResult>) {
    let (request, receive) = mpsc::sync_channel::<EmojiRequest>(64);
    let (done, ready) = mpsc::sync_channel(2);
    std::thread::spawn(move || {
        // Construct on this worker, not on the UI thread. Factories and the
        // current surface initialize only after its first valid request.
        let mut renderer = crate::platform::EmojiRenderer::new();
        while let Ok((generation, key)) = receive.recv() {
            if suspended.load(Ordering::Acquire) || generation != epoch.load(Ordering::Acquire) { continue; }
            let result = renderer.render(&key.0, key.1);
            if suspended.load(Ordering::Acquire) || generation != epoch.load(Ordering::Acquire) { continue; }
            if done.send((generation, key, result)).is_err() { break; }
            ctx.request_repaint();
        }
    });
    (request, ready)
}
struct EmojiSectionPaint {
    painter: egui::Painter,
    shape: egui::layers::ShapeIdx,
    rect: egui::Rect,
    url: String,
}
#[derive(Default)]
struct EmojiSection {
    paints: Vec<EmojiSectionPaint>,
    external_hover: bool,
}
pub struct Pictures {
    download_limit: Arc<AtomicU64>,
    visual_suspended: Arc<AtomicBool>,
    suspended_animation: Option<String>,
    custom_animations: custom_animation::Cache,
    message_animation: Option<bool>,
    emoji_sections: Vec<EmojiSection>,
    textures: HashMap<String, (TextureHandle, usize, u64)>,
    pending: HashSet<String>,
    failed: HashMap<String, (Instant, String)>,
    request: mpsc::SyncSender<PictureRequest>,
    ready: mpsc::Receiver<(u64, String, PictureResult, DecodePermit)>,
    local_sources: HashMap<String, PathBuf>,
    paste_request: mpsc::SyncSender<(String, String, PathBuf)>,
    paste_ready: mpsc::Receiver<PastedAttachment>,
    image_generation: Arc<AtomicU64>,
    clock: u64,
    used: usize,
    pub animation: Option<Animation>,
    animation_request: mpsc::SyncSender<(u64, String)>,
    animation_ready: mpsc::Receiver<(u64, String, Result<AnimationFrames, String>)>,
    animation_pending: Option<(u64, String)>,
    animation_failed: Option<(String, String)>,
    animation_generation: Arc<AtomicU64>,
    animation_seen: bool,
    large: Option<(String, TextureHandle)>,
    avatar_sources: HashMap<String, (String, u64)>,
    avatar_cache: Arc<Mutex<Option<Arc<AvatarCache>>>>,
    #[cfg(test)]
    decode_gate: Arc<DecodeGate>,
}
fn avatar_source(person: &notdiscord_core::models::Person) -> String {
    let Some(raw) = person.avatar_url.as_deref().filter(|url| allowed(url)) else {
        return String::new();
    };
    let Ok(mut url) = url::Url::parse(raw) else {
        return String::new();
    };
    // Canonical identity is independent of requested display resolution.
    if matches!(
        url.host_str(),
        Some("cdn.discordapp.com" | "media.discordapp.net")
    ) && (url.path().contains("/avatars/") || url.path().starts_with("/embed/avatars/"))
    {
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(key, _)| key != "size")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        url.set_query(None);
        url.query_pairs_mut()
            .extend_pairs(pairs)
            .append_pair("size", "256");
    }
    url.into()
}
fn avatar_uv(source: Vec2) -> egui::Rect {
    let longest = source.x.max(source.y).max(1.);
    let extent = egui::vec2(
        (source.y / longest).max(0.0001),
        (source.x / longest).max(0.0001),
    );
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), extent)
}
fn avatar_color(id: &str) -> egui::Color32 {
    let hash = id.bytes().fold(2166136261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16777619)
    });
    let colors = [
        (88, 101, 242),
        (62, 95, 138),
        (115, 67, 150),
        (35, 109, 94),
        (148, 70, 93),
        (137, 88, 38),
    ];
    let (r, g, b) = colors[hash as usize % colors.len()];
    egui::Color32::from_rgb(r, g, b)
}
pub struct Animation {
    pub url: String,
    pub frames: Vec<(TextureHandle, Duration)>,
    pub index: usize,
    pub next: Instant,
}
impl Drop for Pictures {
    fn drop(&mut self) {
        self.stop();
    }
}
fn allowed(value: &str) -> bool {
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return false;
    }
    let Ok(u) = url::Url::parse(value) else {
        return false;
    };
    if u.scheme() != "https" || u.port().is_some() || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    u.host_str().is_some_and(|host| {
        notdiscord_core::parsing::is_media_host(host)
            || matches!(host, "cdn.discordapp.com" | "media.discordapp.net")
            || notdiscord_core::parsing::discord_external_proxy(&u)
            || host == "i.scdn.co" && u.path().starts_with("/image/")
    })
}
pub fn is_image_url(value: &str) -> bool {
    if notdiscord_core::parsing::gif_page(value) {
        return true;
    }
    if !allowed(value) {
        return false;
    }
    url::Url::parse(value).is_ok_and(|u| {
        let path = u.path().to_ascii_lowercase();
        [
            ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tif", ".tiff", ".ico", ".avif",
        ]
        .iter()
        .any(|extension| path.ends_with(extension))
    })
}
static SOURCES: OnceLock<Mutex<HashMap<String, (String, Instant)>>> = OnceLock::new();
static HTTP: OnceLock<Option<reqwest::blocking::Client>> = OnceLock::new();
static SAVING: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());
struct SaveSlot;
impl Drop for SaveSlot {
    fn drop(&mut self) {
        let mut active = SAVING.0.lock().unwrap_or_else(|e| e.into_inner());
        *active = active.saturating_sub(1);
        SAVING.1.notify_one();
    }
}
pub fn save_original(source: &str, destination: &Path, limit: u64) -> Result<(), String> {
    let limit = limit.clamp(1, MAX_ENCODED);
    #[cfg(feature = "offline-probes")]
    if probe_forbids_network() {
        return Err("Red desactivada de forma irreversible para la prueba offline.".into());
    }
    let mut active = SAVING
        .0
        .lock()
        .map_err(|_| "No se pudo iniciar la descarga.")?;
    while *active >= 2 {
        active = SAVING
            .1
            .wait(active)
            .map_err(|_| "No se pudo iniciar la descarga.")?;
    }
    *active += 1;
    drop(active);
    let _slot = SaveSlot;
    let source = resolved_url(source).ok_or("Origen multimedia no disponible o no permitido.")?;
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| "No se pudo iniciar la descarga.")?;
    let mut response = client
        .get(source)
        .send()
        .map_err(|_| "No se pudo descargar el original.")?;
    let kind = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if response.status() != reqwest::StatusCode::OK
        || !kind.starts_with("image/")
        || kind.contains("svg")
    {
        return Err("El servidor no devolvió una imagen o un GIF compatible.".into());
    }
    if response.content_length().is_some_and(|n| n > limit) {
        return Err(super::media_limit::error(limit));
    }
    let name = destination
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let temporary =
        destination.with_file_name(format!("{name}.{}.download", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<(), String> {
        let mut output = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| "No se pudo crear el archivo de destino.")?;
        let mut buffer = [0u8; 32768];
        let mut total = 0u64;
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|_| "La descarga se interrumpió.")?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > limit {
                return Err(super::media_limit::error(limit));
            }
            output
                .write_all(&buffer[..count])
                .map_err(|_| "No se pudo guardar la imagen.")?;
        }
        output
            .flush()
            .map_err(|_| "No se pudo guardar la imagen.")?;
        drop(output);
        std::fs::rename(&temporary, destination)
            .map_err(|_| "No se pudo guardar el archivo de destino.")?;
        Ok(())
    })();
    let _ = std::fs::remove_file(temporary);
    result
}
#[cfg(feature = "offline-probes")]
static PROBE_NO_NETWORK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "offline-probes")]
static PROBE_NETWORK_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "offline-probes")]
pub(super) fn forbid_network_for_probe() {
    // One-way for the lifetime of this diagnostic process, before UI workers.
    PROBE_NO_NETWORK.store(true, Ordering::Release);
}
#[cfg(feature = "offline-probes")]
pub(super) fn probe_network_attempts() -> u64 { PROBE_NETWORK_ATTEMPTS.load(Ordering::Acquire) }
#[cfg(feature = "offline-probes")]
fn probe_forbids_network() -> bool {
    if PROBE_NO_NETWORK.load(Ordering::Acquire) {
        PROBE_NETWORK_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
        true
    } else { false }
}
fn client() -> Option<&'static reqwest::blocking::Client> {
    #[cfg(feature = "offline-probes")]
    if probe_forbids_network() { return None; }
    HTTP.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .ok()
    })
    .as_ref()
}
pub fn remember_source(page: &str, media: &str) -> bool {
    if !notdiscord_core::parsing::gif_page(page) || !allowed(media) {
        return false;
    }
    let mut known = SOURCES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if known.len() >= 128 && !known.contains_key(page) {
        if let Some(old) = known.keys().next().cloned() {
            known.remove(&old);
        }
    }
    let changed = known
        .get(page)
        .is_none_or(|(previous, _)| previous != media);
    known.insert(page.into(), (media.into(), Instant::now()));
    changed
}
pub fn resolved_url(value: &str) -> Option<String> {
    if allowed(value) {
        return Some(value.into());
    }
    if !notdiscord_core::parsing::gif_page(value) {
        return None;
    }
    if let Some((url, _)) = SOURCES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .get(value)
        .filter(|(_, at)| at.elapsed() < Duration::from_secs(15 * 60))
        .cloned()
    {
        return Some(url);
    }
    let c = client()?;
    let response = c.get(value).send().ok()?.error_for_status().ok()?;
    if response.content_length().is_some_and(|n| n > 512 * 1024) {
        return None;
    }
    let mut bytes = Vec::new();
    response.take(512 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 512 * 1024 {
        return None;
    }
    let html = String::from_utf8_lossy(&bytes);
    let result = media_from_html(&html)?;
    remember_source(value, &result);
    Some(result)
}
fn media_from_html(html: &str) -> Option<String> {
    let patterns = [
        r#"(?i)"contentUrl"\s*:\s*"([^"\r\n]+)""#,
        r#"(?i)<meta[^>]+property=["']og:image["'][^>]+content=["']([^"']+)["']"#,
        r#"(?i)<meta[^>]+content=["']([^"']+)["'][^>]+property=["']og:image["']"#,
    ];
    let mut candidates = Vec::new();
    for pattern in patterns {
        let re = regex::Regex::new(pattern).ok()?;
        for capture in re.captures_iter(&html) {
            let raw = capture
                .get(1)?
                .as_str()
                .replace("&amp;", "&")
                .replace("\\/", "/")
                .replace("\\u002F", "/");
            let raw = raw.replace("\\u002f", "/").replace("\\u0026", "&");
            if allowed(&raw)
                && url::Url::parse(&raw).is_ok_and(|url| {
                    ![".mp4", ".webm", ".html"]
                        .iter()
                        .any(|suffix| url.path().to_ascii_lowercase().ends_with(suffix))
                })
            {
                candidates.push(raw);
            }
        }
    }
    candidates.sort_by_key(|url| !notdiscord_core::parsing::direct_gif(url));
    candidates.into_iter().next()
}
#[cfg(test)]
fn response_bytes(
    response: impl Read,
    advertised: Option<u64>,
    limit: u64,
) -> Result<Vec<u8>, String> {
    response_bytes_cancellable(response, advertised, limit, || false)
}
fn response_bytes_cancellable(
    mut response: impl Read,
    advertised: Option<u64>,
    limit: u64,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, String> {
    if advertised.is_some_and(|n| n > limit) {
        return Err(super::media_limit::error(limit));
    }
    let mut out = Vec::new();
    let mut buffer = [0u8; 32 * 1024];
    loop {
        if cancelled() { return Err("Carga de imagen cancelada.".into()); }
        let remaining = (limit + 1).saturating_sub(out.len() as u64).min(buffer.len() as u64) as usize;
        let count = response.read(&mut buffer[..remaining])
            .map_err(|_| "La descarga de la imagen se interrumpió.".to_string())?;
        if count == 0 { break; }
        out.extend_from_slice(&buffer[..count]);
        if out.len() as u64 > limit { return Err(super::media_limit::error(limit)); }
    }
    Ok(out)
}
fn http_image_error(status: reqwest::StatusCode) -> Option<String> {
    if status.is_success() {
        return None;
    }
    Some(match status.as_u16() {
        401 | 403 => "HTTP 401/403: acceso denegado o enlace caducado. Vuelve a cargar el chat para obtener un enlace actualizado.",
        404 | 410 => "HTTP 404/410: el servidor no entregó la imagen; puede ser un enlace caducado o un recurso no disponible. Vuelve a cargar el chat antes de darlo por eliminado.",
        429 => "El servidor limitó las descargas. Inténtalo de nuevo en unos segundos.",
        _ => "No se pudo descargar la imagen del servidor.",
    }.into())
}
fn download_bytes_cancellable(url: &str, limit: u64, cancelled: impl Fn() -> bool) -> Result<Vec<u8>, String> {
    if cancelled() { return Err("Carga de imagen cancelada.".into()); }
    let url = resolved_url(url).ok_or(if notdiscord_core::parsing::gif_page(url) {
        "El proveedor no entregó el GIF. Vuelve a cargar el chat para actualizar su enlace multimedia, o abre el original."
    } else {"Origen de imagen no permitido o no disponible."})?;
    let c = client().ok_or("No se pudo iniciar la descarga de la imagen.")?;
    let response = c.get(&url).send().map_err(|e| {
        if e.is_timeout() {
            "La descarga de la imagen tardó demasiado. Puedes reintentar.".to_string()
        } else {
            "No se pudo conectar con el servidor de la imagen.".to_string()
        }
    })?;
    if let Some(error) = http_image_error(response.status()) {
        return Err(error);
    }
    let length = response.content_length();
    response_bytes_cancellable(response, length, limit, cancelled)
}
/// Clipboard receives the original pixels; display thumbnails use a separate, smaller decode.
pub fn decode_original(value: &str, limit: u64) -> PictureResult {
    let (spool, _) = download_spool(value, limit.clamp(1, MAX_ENCODED), || false)?;
    decode_file(spool.path(), 8192)
}
static DECODING: Mutex<()> = Mutex::new(());
#[cfg(test)]
fn decode_encoded(data: &[u8], max_side: u32) -> PictureResult {
    if data.is_empty() || data.len() as u64 > MAX_ENCODED || !(1..=8192).contains(&max_side) {
        return Err("Imagen vacía o demasiado grande.".into());
    }
    // This lock only bounds simultaneous decoding; it protects no mutable
    // decoder state. A rejected image must not disable all later images.
    let _decoding = DECODING.lock().unwrap_or_else(|error| error.into_inner());
    // Ask WIC first. An image crate header check used to reject Windows-supported
    // formats before WIC had a chance to decode them (for example BMP/TIFF).
    if let Some(pixels) = crate::platform::decode_system_image(data, max_side) {
        return Ok(pixels);
    }
    let reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|_| "El archivo no contiene una imagen compatible.")?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| "El archivo no contiene una imagen compatible.")?;
    if !crate::platform::valid_image_dimensions(width, height) {
        return Err("La imagen supera 8192 píxeles por lado o 24 millones de píxeles.".into());
    }
    // WebP is not installed as a WIC codec on every Windows version.
    if image::guess_format(data).ok() != Some(image::ImageFormat::WebP) {
        return Err("Windows no pudo decodificar esta imagen.".into());
    }
    let im = image::load_from_memory(data)
        .map_err(|_| "La imagen está dañada.")?
        .thumbnail(max_side, max_side)
        .into_rgba8();
    Ok((im.width(), im.height(), im.into_raw()))
}
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs())
        .unwrap_or(0)
}
fn download_spool(
    url: &str,
    limit: u64,
    cancelled: impl Fn() -> bool,
) -> Result<(EncodedSpool, reqwest::header::HeaderMap), String> {
    let url = resolved_url(url).ok_or("Origen de imagen no permitido o no disponible.")?;
    let c = client().ok_or("No se pudo iniciar la descarga de la imagen.")?;
    let response = c.get(&url).send().map_err(|e| {
        if e.is_timeout() {
            "La descarga de la imagen tardó demasiado. Puedes reintentar.".to_owned()
        } else {
            "No se pudo conectar con el servidor de la imagen.".to_owned()
        }
    })?;
    if let Some(error) = http_image_error(response.status()) {
        return Err(error);
    }
    let length = response.content_length();
    let headers = response.headers().clone();
    let spool = EncodedSpool::copy_cancellable(
        response,
        length,
        limit,
        &std::env::temp_dir(),
        cancelled,
    )?;
    Ok((spool, headers))
}
fn decode_file(path: &Path, side: u32) -> PictureResult {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    if !(1..=8192).contains(&side) {
        return Err("Imagen vacía o demasiado grande.".into());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(0x00200000)
        .open(path)
        .map_err(|_| "No se pudo leer el adjunto preparado.")?;
    let m = file
        .metadata()
        .map_err(|_| "No se pudo leer el adjunto preparado.")?;
    if !m.is_file() || m.file_attributes() & 0x400 != 0 || m.len() == 0 || m.len() > MAX_ENCODED {
        return Err("Imagen vacía o demasiado grande.".into());
    }
    let _decoding = DECODING.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(pixels) = crate::platform::decode_system_image_file(path, side) {
        return Ok(pixels);
    }
    let reader = image::ImageReader::new(std::io::BufReader::new(
        file.try_clone()
            .map_err(|_| "No se pudo leer el adjunto preparado.")?,
    ))
    .with_guessed_format()
    .map_err(|_| "El archivo no contiene una imagen compatible.")?;
    if reader.format() != Some(image::ImageFormat::WebP) {
        return Err("Windows no pudo decodificar esta imagen.".into());
    }
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| "La imagen está dañada.")?;
    if !crate::platform::valid_image_dimensions(width, height) {
        return Err("La imagen supera 8192 píxeles por lado o 24 millones de píxeles.".into());
    }
    let reader = image::ImageReader::open(path)
        .map_err(|_| "No se pudo leer el adjunto preparado.")?
        .with_guessed_format()
        .map_err(|_| "La imagen está dañada.")?;
    let image = reader
        .decode()
        .map_err(|_| "La imagen está dañada.")?
        .thumbnail(side, side)
        .into_rgba8();
    Ok((image.width(), image.height(), image.into_raw()))
}
enum PreparedImage {
    Local(PathBuf),
    Cached(avatar_cache::CachedAvatar, String),
    Download(EncodedSpool, String, reqwest::header::HeaderMap),
}
fn prepare_source(
    source: &PictureSource,
    cache: Option<&AvatarCache>,
    limit: u64,
    cancelled: impl Fn() -> bool,
) -> Result<PreparedImage, String> {
    match source {
        PictureSource::Local(path) => Ok(PreparedImage::Local(path.clone())),
        PictureSource::Remote(url) => {
            if let Some(cached) = cache.and_then(|cache| cache.lookup(url, unix_now())) {
                return Ok(PreparedImage::Cached(cached, url.clone()));
            }
            let (spool, headers) = download_spool(url, limit, cancelled)?;
            Ok(PreparedImage::Download(spool, url.clone(), headers))
        }
    }
}
fn decode_prepared(
    prepared: PreparedImage,
    side: u32,
    cache: Option<&AvatarCache>,
) -> PictureResult {
    match prepared {
        PreparedImage::Local(path) => decode_file(&path, side),
        PreparedImage::Cached(cached, url) => {
            let result = decode_file(cached.path(), side);
            drop(cached);
            if result.is_err()
                && let Some(cache) = cache
            {
                cache.invalidate(&url, unix_now());
            }
            result
        }
        PreparedImage::Download(spool, url, headers) => {
            let result = decode_file(spool.path(), side);
            if result.is_ok()
                && let Some(cache) = cache
            {
                let _ = cache.store(&url, &headers, spool.path(), unix_now());
            }
            result
        }
    }
}
fn avatar_download_url(url: &str, width: u32) -> String {
    let Ok(mut value) = url::Url::parse(url) else {
        return url.into();
    };
    if !matches!(
        value.host_str(),
        Some("cdn.discordapp.com" | "media.discordapp.net")
    ) || !(value.path().contains("/avatars/") || value.path().starts_with("/embed/avatars/"))
    {
        return url.into();
    }
    // CDN sizes are powers of two. A large viewer must not silently request
    // the same 512px file used by an ordinary thumbnail.
    let side = width.clamp(16,4096).next_power_of_two().min(4096);
    let pairs: Vec<_> = value
        .query_pairs()
        .filter(|(key, _)| key != "size")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    value.set_query(None);
    value
        .query_pairs_mut()
        .extend_pairs(pairs)
        .append_pair("size", &side.to_string());
    value.into()
}
fn avatar_decode_side(physical:u32,full:bool)->u32 {
    // Ordinary rows need at most half the old 256px avatar source. Match the
    // actual physical display size, including DPI, instead of halving a small
    // already rendered face and making it unnecessarily blurry.
    if full {512} else {physical.clamp(32,128).next_power_of_two().min(128)}
}
fn avatar_texture_key(source:&str,side:u32,full:bool)->String{
    format!("{source}\tavatar-{}-{side}",if full{"profile"}else{"compact"})
}
fn side_bucket(width: u32) -> u32 {
    match width {
        0..=64 => 64,
        65..=128 => 128,
        129..=384 => 384,
        385..=768 => 768,
        _ => 1600,
    }
}
fn image_key(url: &str, width: u32) -> String {
    format!("{url}\t{}", side_bucket(width))
}
impl Pictures {
    pub(super) fn set_download_limit(&mut self, limit: u64) {
        let limit = limit.clamp(1, MAX_ENCODED);
        if self.download_limit.swap(limit, Ordering::AcqRel) == limit { return; }
        // Discard completions admitted under the previous setting. Retrying
        // the visible sources must not wait for a cached size-limit failure.
        self.image_generation.fetch_add(1, Ordering::AcqRel);
        self.pending.clear();
        self.failed.clear();
        self.stop();
    }
    #[cfg(any(test, feature = "offline-probes"))]
    pub(super) fn seed_sized_fixture(&mut self, ctx: &egui::Context, url: &str, side: u32) -> egui::TextureId {
        let pixels = ColorImage::new([2, 2], vec![egui::Color32::from_rgb(30, 180, 70); 4]);
        let texture = ctx.load_texture("synthetic-custom-emoji", pixels, TextureOptions::NEAREST);
        let id = texture.id();
        if let Some((_, bytes, _)) = self.textures.insert(image_key(url, side), (texture, 16, self.clock)) {
            self.used = self.used.saturating_sub(bytes);
        }
        self.used += 16;
        id
    }
    #[cfg(any(test, feature = "offline-probes"))]
    pub(super) fn pending_fixture_requests(&self) -> usize { self.pending.len() }
    #[cfg(any(test, feature = "offline-probes"))]
    pub(super) fn seed_emoji_animation_fixture(&mut self, ctx: &egui::Context, url: &str, count: usize) -> Vec<egui::TextureId> {
        self.custom_animations.fixture(ctx, url, count)
    }
    #[cfg(any(test, feature = "offline-probes"))]
    pub(super) fn seed_emoji_animation_sized_fixture(&mut self, ctx: &egui::Context, url: &str, count: usize, side: usize) -> Vec<egui::TextureId> {
        self.custom_animations.fixture_sized(ctx, url, count, side)
    }
    #[cfg(any(test, feature = "offline-probes"))]
    pub(super) fn custom_animation_metrics(&self) -> serde_json::Value { self.custom_animations.metrics() }
    #[cfg(any(test, debug_assertions, feature = "offline-probes"))]
    pub(super) fn seed_fixture(&mut self, ctx: &egui::Context, url: &str) {
        let pixels = ColorImage::from_rgba_unmultiplied(
            [2, 2],
            &[
                240, 30, 40, 255, 30, 220, 80, 255, 30, 70, 240, 255, 230, 200, 30, 255,
            ],
        );
        let texture = ctx.load_texture("synthetic-viewer-pixels", pixels, TextureOptions::NEAREST);
        let key = image_key(url, 1600);
        if let Some((_, bytes, _)) = self.textures.insert(key, (texture, 16, self.clock)) {
            self.used = self.used.saturating_sub(bytes);
        }
        self.used += 16;
    }
    #[cfg(any(debug_assertions, feature = "offline-probes"))]
    pub(super) fn seed_fixture_file(&mut self, url: &str, path: PathBuf) {
        let key = image_key(url, 1600);
        self.textures
            .remove(&key)
            .map(|(_, bytes, _)| self.used = self.used.saturating_sub(bytes));
        self.failed.remove(&key);
        let request = PictureRequest {
            generation: self.image_generation.load(Ordering::Acquire),
            key: key.clone(),
            source: PictureSource::Local(path),
            side: 1600,
        };
        if self.request.try_send(request).is_ok() {
            self.pending.insert(key);
        }
    }
    pub fn new(ctx: egui::Context) -> Self {
        let download_limit = Arc::new(AtomicU64::new(super::media_limit::bytes(super::media_limit::DEFAULT_MIB)));
        let (request, rx) = mpsc::sync_channel::<PictureRequest>(64);
        let (done, ready) = mpsc::sync_channel(1);
        let image_generation = Arc::new(AtomicU64::new(0));
        let admission = Arc::new(DecodeGate::default());
        let avatar_cache = Arc::new(Mutex::new(None::<Arc<AvatarCache>>));
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..3 {
            let rx = rx.clone();
            let done = done.clone();
            let ctx = ctx.clone();
            let epoch = image_generation.clone();
            let admission = admission.clone();
            let avatar_cache = avatar_cache.clone();
            let download_limit = download_limit.clone();
            std::thread::spawn(move || {
                loop {
                    let Ok(request) = rx.lock().unwrap().recv() else {
                        break;
                    };
                    if epoch.load(Ordering::Acquire) != request.generation {
                        continue;
                    }
                    let cache = avatar_cache
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone();
                    // Downloads remain concurrent, but wait on disk instead of
                    // each retaining an encoded body plus a finished bitmap.
                    let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        prepare_source(&request.source, cache.as_deref(), download_limit.load(Ordering::Acquire), || {
                            epoch.load(Ordering::Acquire) != request.generation
                        })
                    }))
                    .unwrap_or_else(|_| Err("No se pudo preparar esta imagen.".into()));
                    let Some(permit) =
                        admission.acquire(|| epoch.load(Ordering::Acquire) != request.generation)
                    else {
                        continue;
                    };
                    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||prepared.and_then(|prepared| decode_prepared(prepared, request.side, cache.as_deref()))))
                        .unwrap_or_else(|_|Err("No se pudo decodificar esta imagen. La aplicación puede seguir usándose.".into()));
                    if epoch.load(Ordering::Acquire) != request.generation {
                        continue;
                    }
                    if done
                        .send((request.generation, request.key, out, permit))
                        .is_err()
                    {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        let (animation_request, rx) = mpsc::sync_channel::<(u64, String)>(1);
        let (done, animation_ready) = mpsc::sync_channel(1);
        let wake = ctx.clone();
        let animation_generation = Arc::new(AtomicU64::new(0));
        let epoch = animation_generation.clone();
        let animation_limit = download_limit.clone();
        std::thread::spawn(move || {
            while let Ok((generation, url)) = rx.recv() {
                if epoch.load(Ordering::Acquire) != generation {
                    continue;
                }
                let frames = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| download_bytes_cancellable(&url, animation_limit.load(Ordering::Acquire), || epoch.load(Ordering::Acquire) != generation)
                    .and_then(|bytes| {
                        decode_animation_checked(&bytes, || {
                            epoch.load(Ordering::Acquire) != generation
                        })
                    }))).unwrap_or_else(|_|Err("No se pudo decodificar este GIF. Puedes reintentar o abrir el original.".into()));
                if epoch.load(Ordering::Acquire) != generation { continue; }
                if done.send((generation, url, frames)).is_err() {
                    break;
                }
                wake.request_repaint();
            }
        });
        let (paste_request, paste_receive) = mpsc::sync_channel::<(String, String, PathBuf)>(2);
        let (paste_done, paste_ready) = mpsc::sync_channel(2);
        let wake = ctx.clone();
        std::thread::spawn(move || {
            while let Ok((channel, account, root)) = paste_receive.recv() {
                let result = crate::platform::paste_image_to_png(&root);
                if paste_done
                    .send(PastedAttachment {
                        channel,
                        account,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
                wake.request_repaint();
            }
        });
        Self {
            download_limit,
            visual_suspended: Arc::new(AtomicBool::new(false)),
            suspended_animation: None,
            custom_animations: custom_animation::Cache::default(),
            message_animation: None,
            emoji_sections: Vec::new(),
            textures: HashMap::new(),
            pending: HashSet::new(),
            failed: HashMap::new(),
            request,
            ready,
            local_sources: HashMap::new(),
            paste_request,
            paste_ready,
            image_generation,
            clock: 0,
            used: 0,
            animation: None,
            animation_request,
            animation_ready,
            animation_pending: None,
            animation_failed: None,
            animation_generation,
            animation_seen: false,
            large: None,
            avatar_sources: HashMap::new(),
            avatar_cache,
            #[cfg(test)]
            decode_gate: admission,
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        if self.visuals_suspended() {
            while self.ready.try_recv().is_ok() {}
            while self.animation_ready.try_recv().is_ok() {}
            self.custom_animations.finish_frame(true);
            return;
        }
        self.custom_animations.poll(ctx);
        while let Ok((generation, url, result, _permit)) = self.ready.try_recv() {
            if generation != self.image_generation.load(Ordering::Acquire) {
                continue;
            }
            self.pending.remove(&url);
            if let Ok((w, h, pixels)) = result {
                let weight = pixels.len();
                if weight > BUDGET / 2 {
                    let tex = ctx.load_texture(
                        &url,
                        ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels),
                        TextureOptions::LINEAR,
                    );
                    self.large = Some((url, tex));
                    continue;
                }
                while (self.used + weight > BUDGET || self.textures.len() >= MAX_TEXTURES)
                    && !self.textures.is_empty()
                {
                    let key = self
                        .textures
                        .iter()
                        .min_by_key(|(_, v)| v.2)
                        .map(|(k, _)| k.clone())
                        .unwrap();
                    let old = self.textures.remove(&key).unwrap();
                    self.used -= old.1;
                }
                if weight <= BUDGET {
                    let tex = ctx.load_texture(
                        &url,
                        ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels),
                        TextureOptions::LINEAR,
                    );
                    self.clock += 1;
                    self.textures.insert(url, (tex, weight, self.clock));
                    self.used += weight;
                }
            } else if let Err(error) = result {
                if self.failed.len() >= 256 {
                    if let Some(old) = self
                        .failed
                        .iter()
                        .min_by_key(|(_, (at, _))| *at)
                        .map(|(key, _)| key.clone())
                    {
                        self.failed.remove(&old);
                    }
                }
                self.failed.insert(url, (Instant::now(), error));
            }
        }
        while let Ok((generation, url, decoded)) = self.animation_ready.try_recv() {
            if self.animation_pending.as_ref() != Some(&(generation, url.clone())) {
                continue;
            }
            self.animation_pending = None;
            let decoded = match decoded {
                Ok(frames) => frames,
                Err(error) => {
                    self.animation_failed = Some((url, error));
                    continue;
                }
            };
            let frames = decoded
                .into_iter()
                .enumerate()
                .map(|(index, (w, h, pixels, delay))| {
                    let texture = ctx.load_texture(
                        format!("gif:{url}:{index}"),
                        ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels),
                        TextureOptions::LINEAR,
                    );
                    (texture, delay)
                })
                .collect::<Vec<_>>();
            if !frames.is_empty() {
                let next = Instant::now() + frames[0].1;
                self.animation = Some(Animation {
                    url,
                    frames,
                    index: 0,
                    next,
                });
            }
        }
    }
    pub fn texture(&mut self, url: &str) -> Option<TextureHandle> {
        self.texture_sized(url, 384)
    }
    fn remember_avatar(&mut self, person: &notdiscord_core::models::Person) -> String {
        let source = avatar_source(person);
        let owner = if person.id.is_empty() {
            format!("unnamed:{}", person.name)
        } else {
            person.id.clone()
        };
        self.clock += 1;
        let previous = self
            .avatar_sources
            .insert(owner, (source.clone(), self.clock));
        if let Some((old, _)) = previous.filter(|(old, _)| *old != source) {
            if !old.is_empty() && !self.avatar_sources.values().any(|(known, _)| *known == old) {
                let prefix=format!("{old}\t");
                let keys=self.textures.keys().chain(self.failed.keys()).filter(|key|key.starts_with(&prefix)).cloned().collect::<HashSet<_>>();
                for key in keys {
                    if let Some((_, weight, _)) = self.textures.remove(&key) {
                        self.used = self.used.saturating_sub(weight);
                    }
                    self.failed.remove(&key);
                }
            }
        }
        while self.avatar_sources.len() > MAX_AVATAR_USERS {
            let Some(oldest) = self
                .avatar_sources
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            self.avatar_sources.remove(&oldest);
        }
        source
    }
    /// A decorative avatar: the owning row decides whether clicks select/open it.
    pub fn paint_avatar(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        person: &notdiscord_core::models::Person,
    ) {
        self.paint_avatar_quality(ui,rect,person,false);
    }
    fn paint_avatar_quality(&mut self,ui:&egui::Ui,rect:egui::Rect,person:&notdiscord_core::models::Person,full:bool){
        if !ui.is_rect_visible(rect) {
            return;
        }
        super::autoscroll::exclude(ui,rect);
        let canonical = ui.ctx().data_mut(|data| {
            data.get_temp_mut_or_default::<HashMap<String, Option<String>>>(egui::Id::new(
                "canonical-avatar-urls",
            ))
            .get(&person.id)
            .cloned()
        });
        let current = canonical.map(|avatar_url| {
            let mut current = person.clone();
            current.avatar_url = avatar_url;
            current
        });
        let person = current.as_ref().unwrap_or(person);
        let source = self.remember_avatar(person);
        let side = rect.width().min(rect.height()).max(1.);
        let rect = egui::Rect::from_center_size(rect.center(), Vec2::splat(side));
        let pixels = (side * ui.ctx().pixels_per_point())
            .ceil()
            .clamp(16., AVATAR_SIDE as f32) as u32;
        let requested=avatar_decode_side(pixels,full);
        let texture=if allowed(&source){self.texture_source(avatar_texture_key(&source,requested,full),PictureSource::Remote(avatar_download_url(&source,requested)),requested)}else{None};
        // Reduced lists deliberately do not reuse a full profile bitmap: that
        // would retain the larger decode for every ordinary row after a visit.
        if let Some(texture) = texture {
            egui::Image::new(&texture)
                .uv(avatar_uv(texture.size_vec2()))
                .corner_radius(side * 0.5)
                .paint_at(ui, rect);
        } else {
            use unicode_segmentation::UnicodeSegmentation;
            ui.painter()
                .circle_filled(rect.center(), side * 0.5, avatar_color(&person.id));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                person
                    .name
                    .graphemes(true)
                    .find(|g| !g.trim().is_empty())
                    .unwrap_or("?")
                    .to_uppercase(),
                egui::FontId::proportional((side * 0.45).max(11.)),
                egui::Color32::WHITE,
            );
        }
    }
    pub fn avatar(
        &mut self,
        ui: &mut egui::Ui,
        person: &notdiscord_core::models::Person,
        size: f32,
    ) -> egui::Response {
        let size = if size.is_finite() {
            size.clamp(16., 256.)
        } else {
            32.
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
        self.paint_avatar(ui, rect, person);
        response
    }
    pub fn profile_avatar(&mut self,ui:&mut egui::Ui,person:&notdiscord_core::models::Person,size:f32)->egui::Response{
        let size=if size.is_finite(){size.clamp(16.,256.)}else{88.};
        let(rect,response)=ui.allocate_exact_size(Vec2::splat(size),egui::Sense::click());
        self.paint_avatar_quality(ui,rect,person,true);response
    }
    pub fn avatar_viewer_url(&mut self,ctx:&egui::Context,person:&notdiscord_core::models::Person)->Option<String>{
        let canonical=ctx.data_mut(|data|data.get_temp_mut_or_default::<HashMap<String,Option<String>>>(egui::Id::new("canonical-avatar-urls")).get(&person.id).cloned());
        let mut current=person.clone();if let Some(url)=canonical {current.avatar_url=url;}
        let source=self.remember_avatar(&current);allowed(&source).then(||avatar_download_url(&source,2048))
    }
    pub fn texture_sized(&mut self, url: &str, width: u32) -> Option<TextureHandle> {
        let source = if allowed(url) || notdiscord_core::parsing::gif_page(url) {
            PictureSource::Remote(avatar_download_url(url, width))
        } else {
            return None;
        };
        self.texture_source(image_key(url, width), source, side_bucket(width))
    }
    /// Only a path explicitly supplied by the local file picker/paste flow may use this entry point.
    pub fn texture_path(&mut self, path: &Path, width: u32) -> Option<TextureHandle> {
        let source_id = format!("local:{}", path.to_string_lossy());
        if self.local_sources.len() >= 128 && !self.local_sources.contains_key(&source_id) {
            if let Some(old) = self.local_sources.keys().next().cloned() {
                self.local_sources.remove(&old);
            }
        }
        self.local_sources
            .insert(source_id.clone(), path.to_owned());
        self.texture_source(
            image_key(&source_id, width),
            PictureSource::Local(path.to_owned()),
            side_bucket(width),
        )
    }
    fn texture_source(
        &mut self,
        key: String,
        source: PictureSource,
        side: u32,
    ) -> Option<TextureHandle> {
        self.clock += 1;
        if let Some((key_large, texture)) = &self.large {
            if *key_large == key {
                return Some(texture.clone());
            }
        }
        if let Some((t, _, at)) = self.textures.get_mut(&key) {
            *at = self.clock;
            return Some(t.clone());
        }
        // Failures remain explicit until Retry; silently re-fetching an expired URL forever is misleading.
        if !self.failed.contains_key(&key)
            && !self.pending.contains(&key)
            && self.pending.len() < 64
            && !self.visuals_suspended()
        {
            let request = PictureRequest {
                generation: self.image_generation.load(Ordering::Acquire),
                key: key.clone(),
                source,
                side,
            };
            if self.request.try_send(request).is_ok() {
                self.pending.insert(key);
            }
        }
        None
    }
    pub fn image_error(&self, url: &str, width: u32) -> Option<&str> {
        if !allowed(url) && !notdiscord_core::parsing::gif_page(url) {
            return Some("Origen de imagen no permitido o no disponible.");
        }
        self.failed
            .get(&image_key(url, width))
            .map(|(_, message)| message.as_str())
    }
    pub fn path_error(&self, path: &Path, width: u32) -> Option<&str> {
        self.failed
            .get(&image_key(
                &format!("local:{}", path.to_string_lossy()),
                width,
            ))
            .map(|(_, message)| message.as_str())
    }
    pub fn retry(&mut self, url: &str, width: u32) {
        self.failed.remove(&image_key(url, width));
    }
    pub fn retry_path(&mut self, path: &Path, width: u32) {
        self.failed.remove(&image_key(&format!("local:{}", path.to_string_lossy()), width));
    }
    pub fn paste_bitmap(&self, channel: &str, account: &str, root: PathBuf) -> Result<(), String> {
        self.paste_request
            .try_send((channel.into(), account.into(), root))
            .map_err(|_| {
                "Ya se están preparando imágenes del portapapeles. Espera un momento.".into()
            })
    }
    pub fn take_pasted(&self) -> Vec<PastedAttachment> {
        self.paste_ready.try_iter().collect()
    }
    pub fn image(
        &mut self,
        ui: &mut egui::Ui,
        url: &str,
        size: Vec2,
        round: bool,
    ) -> egui::Response {
        let response=self.image_inner(ui,url,size,round);
        super::autoscroll::exclude(ui,response.rect);
        response
    }
    fn image_inner(
        &mut self, ui:&mut egui::Ui, url:&str, size:Vec2, round:bool,
    )->egui::Response {
        if !ui.is_rect_visible(egui::Rect::from_min_size(ui.cursor().min, size)) {
            return ui.allocate_exact_size(size, egui::Sense::click()).1;
        }
        if let Some(t) =
            self.texture_sized(url, (size.x * ui.ctx().pixels_per_point()).ceil() as u32)
        {
            ui.add(
                egui::Image::new(&t)
                    .fit_to_exact_size(size)
                    .corner_radius(if round { size.x / 2. } else { 5. })
                    .sense(egui::Sense::click()),
            )
        } else {
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
            ui.painter().rect_filled(
                rect,
                if round { size.x / 2. } else { 5. },
                ui.visuals().faint_bg_color,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "◌",
                egui::FontId::proportional(size.y * 0.45),
                ui.visuals().weak_text_color(),
            );
            response
        }
    }
    pub fn stop(&mut self) {
        self.image_generation.fetch_add(1, Ordering::AcqRel);
        self.pending.clear();
        self.animation_generation.fetch_add(1, Ordering::AcqRel);
        self.animation = None;
        self.animation_pending = None;
        self.animation_failed = None;
        self.suspended_animation = None;
        self.large = None;
    }
    pub(super) fn visual_suspension(&self) -> Arc<AtomicBool> { self.visual_suspended.clone() }
    pub(super) fn visuals_suspended(&self) -> bool { self.visual_suspended.load(Ordering::Acquire) }
    pub(super) fn set_visual_suspended(&mut self, suspended: bool) {
        if self.visual_suspended.swap(suspended, Ordering::AcqRel) == suspended { return; }
        if suspended {
            let resume = self.animation.as_ref().map(|animation| animation.url.clone())
                .or_else(|| self.animation_pending.as_ref().map(|(_, url)| url.clone()))
                .or_else(|| self.suspended_animation.clone());
            self.stop();
            self.suspended_animation = resume;
            self.custom_animations.clear();
        }
    }
    pub(super) fn release_viewer(&mut self, url: &str, local: Option<&Path>) {
        let source = local.map(|path| format!("local:{}", path.to_string_lossy()))
            .unwrap_or_else(|| url.to_owned());
        let key = image_key(&source, 1600);
        if let Some((_, bytes, _)) = self.textures.remove(&key) {
            self.used = self.used.saturating_sub(bytes);
        }
        self.failed.remove(&key);
        self.local_sources.remove(&source);
    }
    /// Session replacement releases retained capacities and private image
    /// textures. Ordinary navigation still uses stop() to preserve its LRU.
    pub fn clear_account(&mut self) {
        self.stop();
        self.custom_animations.clear();
        self.textures = HashMap::new();
        self.pending = HashSet::new();
        self.failed = HashMap::new();
        self.local_sources = HashMap::new();
        self.avatar_sources = HashMap::new();
        self.used = 0;
        *self.avatar_cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
        while self.ready.try_recv().is_ok() {}
        while self.animation_ready.try_recv().is_ok() {}
    }
    pub fn configure_avatar_cache(&mut self, base: &Path, account: &str) {
        let cache = AvatarCache::open(base, account).ok().map(Arc::new);
        *self.avatar_cache.lock().unwrap_or_else(|e| e.into_inner()) = cache;
    }
    pub fn play(&mut self, _ctx: &egui::Context, url: String) {
        self.stop();
        if self.visuals_suspended() { self.suspended_animation = Some(url); return; }
        let generation = self.animation_generation.load(Ordering::Acquire);
        if self
            .animation_request
            .try_send((generation, url.clone()))
            .is_ok()
        {
            self.animation_pending = Some((generation, url));
        } else {
            self.animation_failed = Some((
                url,
                "Otra animación se está cancelando. Vuelve a pulsar Reproducir.".into(),
            ));
        }
    }
    pub fn animation_loading(&self, url: &str) -> bool {
        self.animation_pending
            .as_ref()
            .is_some_and(|(_, pending)| pending == url)
    }
    pub fn animation_error(&self, url: &str) -> Option<&str> {
        self.animation_failed
            .as_ref()
            .filter(|(failed, _)| failed == url)
            .map(|(_, error)| error.as_str())
    }
    pub fn animated(&mut self, ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
        if self.visuals_suspended() { return None; }
        // Only resume a user-selected GIF when its surface is visible again.
        // A removed message or closed viewer must not restart a hidden decode.
        if self.suspended_animation.as_deref() == Some(url) {
            let generation = self.animation_generation.load(Ordering::Acquire);
            match self.animation_request.try_send((generation, url.to_owned())) {
                Ok(()) => {
                    self.suspended_animation = None;
                    self.animation_pending = Some((generation, url.to_owned()));
                }
                Err(mpsc::TrySendError::Full(_)) => ctx.request_repaint_after(Duration::from_millis(100)),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.suspended_animation = None;
                    self.animation_failed = Some((url.to_owned(), "No se pudo reanudar el GIF.".into()));
                }
            }
        }
        let a = self.animation.as_mut()?;
        if a.url != url {
            return None;
        }
        self.animation_seen = true;
        if Instant::now() >= a.next {
            a.index = (a.index + 1) % a.frames.len();
            a.next = Instant::now() + a.frames[a.index].1;
        }
        ctx.request_repaint_after(a.next.saturating_duration_since(Instant::now()));
        Some(a.frames[a.index].0.clone())
    }
    pub fn animated_emoji(&mut self, ctx: &egui::Context, url: &str) -> Option<TextureHandle> {
        if self.visuals_suspended() { return None; }
        self.custom_animations.texture(ctx, url)
    }
    pub fn custom_emoji_playing(&self) -> bool {
        self.message_animation.unwrap_or(false)
    }
    pub(super) fn has_emoji_section(&self) -> bool { !self.emoji_sections.is_empty() }
    pub(super) fn has_emoji_scope(&self) -> bool { self.has_emoji_section() || self.message_animation.is_some() }
    pub(super) fn begin_emoji_section(&mut self) { self.emoji_sections.push(EmojiSection::default()); }
    pub(super) fn include_emoji_section_hover(&mut self, hovered_visible: bool) {
        if let Some(section)=self.emoji_sections.last_mut() {section.external_hover |= hovered_visible;}
    }
    pub(super) fn stage_emoji(&mut self, painter: egui::Painter, shape: egui::layers::ShapeIdx, rect: egui::Rect, url: String) {
        if let Some(section)=self.emoji_sections.last_mut() { section.paints.push(EmojiSectionPaint {painter,shape,rect,url}); }
    }
    pub(super) fn end_emoji_section(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        let Some(section)=self.emoji_sections.pop() else {return};
        if let Some(parent)=self.emoji_sections.last_mut() { parent.external_hover |= section.external_hover; parent.paints.extend(section.paints); return; }
        let hovered=ui.is_enabled() && !ui.ctx().input(|input|input.viewport().minimized.unwrap_or(false))
            && (section.external_hover || ui.rect_contains_pointer(rect.intersect(ui.clip_rect())));
        if !hovered {return;}
        for painted in section.paints {
            if !painted.rect.intersects(painted.painter.clip_rect()) {continue;}
            if let Some(texture)=self.custom_animations.texture(ui.ctx(),&painted.url) {
                painted.painter.set(painted.shape,egui::Shape::image(texture.id(),painted.rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE));
            }
        }
    }
    pub fn begin_message(&mut self, hovered_visible: bool) {
        self.message_animation = Some(hovered_visible);
    }
    pub fn end_message(&mut self) {
        self.message_animation = None;
    }
    pub fn begin_frame(&mut self) {
        self.animation_seen = false;
        self.message_animation = None;
        self.emoji_sections.clear();
        self.custom_animations.begin_frame();
    }
    pub fn finish_frame(&mut self, minimized: bool) {
        self.set_visual_suspended(minimized);
        self.custom_animations.finish_frame(minimized);
        if !minimized && self.animation.is_some() && !self.animation_seen {
            self.stop();
        }
    }
}
#[cfg(test)]
fn decode_animation_bytes(bytes: &[u8], cancelled: impl Fn() -> bool) -> AnimationFrames {
    decode_animation_checked(bytes, cancelled).unwrap_or_default()
}
fn decode_animation_checked(
    bytes: &[u8],
    cancelled: impl Fn() -> bool,
) -> Result<AnimationFrames, String> {
    let Ok(mut scan) = gif::DecodeOptions::new().read_info(Cursor::new(bytes)) else {
        return Err("Decodificación GIF: cabecera no válida o archivo incompatible.".into());
    };
    let (w, h) = (scan.width() as u32, scan.height() as u32);
    if w == 0 || h == 0 || w > 1024 || h > 1024 {
        return Err("Presupuesto local GIF: máximo 1024 píxeles por lado.".into());
    }
    let mut count = 0;
    loop {
        if cancelled() {
            return Err("Carga del GIF cancelada.".into());
        }
        match scan.next_frame_info() {
            Ok(Some(frame)) => {
                if frame.width == 0
                    || frame.height == 0
                    || frame.width > 1024
                    || frame.height > 1024
                    || frame.left as u32 >= w
                    || frame.top as u32 >= h
                {
                    return Err("Decodificación GIF: dimensiones de fotograma no válidas.".into());
                }
                count += 1;
                if count > 400 || w as u64 * h as u64 * count as u64 > 64_000_000 {
                    return Err("Presupuesto local GIF: máximo 400 fotogramas y 64 millones de píxeles procesados.".into());
                }
            }
            Ok(None) => break,
            Err(_) => return Err("Decodificación GIF: datos de fotograma dañados.".into()),
        }
    }
    if count == 0 {
        return Err("Decodificación GIF: no contiene fotogramas.".into());
    }
    let memory_scale = (BUDGET as f64 / (4. * w as f64 * h as f64 * count as f64)).sqrt();
    let scale = memory_scale
        .min(1.)
        .min(360. / w as f64)
        .min(220. / h as f64);
    let target_w = ((w as f64 * scale) as u32).max(1);
    let target_h = ((h as f64 * scale) as u32).max(1);
    if target_w as usize * target_h as usize * 4 * count > BUDGET {
        return Err("Presupuesto local GIF: máximo 8 MiB de fotogramas reducidos.".into());
    }
    let Ok(dec) = image::codecs::gif::GifDecoder::new(Cursor::new(bytes)) else {
        return Err("Decodificación GIF: no se pudo iniciar el decodificador.".into());
    };
    let mut frames = Vec::with_capacity(count);
    for frame in dec.into_frames() {
        if cancelled() || frames.len() >= count {
            return Err("Carga del GIF cancelada o fotogramas inesperados.".into());
        }
        let Ok(f) = frame else { return Err("Decodificación GIF: datos de imagen dañados.".into()) };
        let delay = f.delay().numer_denom_ms();
        let delay = Duration::from_millis((delay.0 / delay.1.max(1)).clamp(20, 10_000) as u64);
        let image = f.into_buffer();
        if image.width() != w || image.height() != h {
            return Err("Decodificación GIF: dimensiones inconsistentes.".into());
        }
        let mut thumbnail = vec![0; target_w as usize * target_h as usize * 4];
        for y in 0..target_h {
            for x in 0..target_w {
                let source = (((y * h / target_h) * w + x * w / target_w) * 4) as usize;
                let destination = ((y * target_w + x) * 4) as usize;
                thumbnail[destination..destination + 4]
                    .copy_from_slice(&image.as_raw()[source..source + 4]);
            }
        }
        frames.push((target_w, target_h, thumbnail, delay));
    }
    if frames.len() != count {
        Err("Decodificación GIF: secuencia incompleta.".into())
    } else {
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_viewer_releases_full_texture_and_local_source_but_keeps_chat_thumbnail() {
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        let url = "offline-viewer-fixture";
        pictures.seed_sized_fixture(&ctx, url, 384);
        pictures.seed_sized_fixture(&ctx, url, 1600);
        assert_eq!(pictures.used, 32);
        pictures.stop();
        pictures.release_viewer(url, None);
        assert_eq!(pictures.used, 16);
        assert!(pictures.textures.contains_key(&image_key(url, 384)));
        assert!(!pictures.textures.contains_key(&image_key(url, 1600)));
        let path = PathBuf::from("synthetic-unsent-image.png");
        let source = format!("local:{}", path.to_string_lossy());
        pictures.seed_sized_fixture(&ctx, &source, 1600);
        pictures.local_sources.insert(source.clone(), path.clone());
        pictures.release_viewer("", Some(&path));
        assert_eq!(pictures.used, 16);
        assert!(!pictures.local_sources.contains_key(&source));
    }
    #[test]
    fn gif_errors_distinguish_decoding_from_local_budget_and_proxy_origins() {
        assert!(decode_animation_checked(b"not a GIF",||false).unwrap_err().starts_with("Decodificación"));
        let mut bytes=Vec::new();
        {
            let mut encoder=gif::Encoder::new(&mut bytes,2048,1,&[0,0,0,255,255,255]).unwrap();
            let frame=gif::Frame { width:2048,height:1,buffer:std::borrow::Cow::Owned(vec![0;2048]),..Default::default() };
            encoder.write_frame(&frame).unwrap();
        }
        assert!(decode_animation_checked(&bytes,||false).unwrap_err().starts_with("Presupuesto local"));
        assert!(allowed("https://images-ext-2.discordapp.net/external/synthetic/https/media.tenor.com/example.gif"));
        assert!(!allowed("https://images-ext-2.discordapp.net.evil.invalid/external/example.gif"));
        assert!(!allowed("https://:secret@cdn.discordapp.com/example.gif"));
        let forbidden=http_image_error(reqwest::StatusCode::FORBIDDEN).unwrap();
        let missing=http_image_error(reqwest::StatusCode::NOT_FOUND).unwrap();
        assert!(forbidden.contains("401/403") && missing.contains("404/410"));
        assert!(missing.contains("caducado") && !missing.contains("ya no está disponible"));
    }
    #[test]
    fn gif_resolution_prefers_animation_and_uses_metadata_without_loading_the_page() {
        let html = r#"<meta property="og:image" content="https://static.klipy.com/thumb.png"><script>{"contentUrl":"https:\/\/static.klipy.com\/movie.mp4"}</script><script>{"contentUrl":"https:\/\/static.klipy.com\/movie.gif"}</script>"#;
        assert_eq!(
            media_from_html(html).as_deref(),
            Some("https://static.klipy.com/movie.gif")
        );
        let page = format!("https://klipy.com/gifs/fixture-{}", uuid::Uuid::new_v4());
        assert!(remember_source(
            &page,
            "https://static.klipy.com/fixture.gif"
        ));
        assert!(!remember_source(
            &page,
            "https://static.klipy.com/fixture.gif"
        ));
        assert_eq!(
            resolved_url(&page).as_deref(),
            Some("https://static.klipy.com/fixture.gif")
        );
    }
    #[test]
    fn animation_errors_are_visible_and_stale_completions_do_not_restore_them() {
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        let (send, receive) = mpsc::channel();
        pictures.animation_ready = receive;
        pictures.animation_pending = Some((0, "fixture".into()));
        send.send((0, "fixture".into(), Err("Fixture GIF failure".into())))
            .unwrap();
        pictures.poll(&ctx);
        assert_eq!(
            pictures.animation_error("fixture"),
            Some("Fixture GIF failure")
        );
        assert!(!pictures.animation_loading("fixture"));
        pictures.stop();
        send.send((0, "fixture".into(), Err("Stale failure".into())))
            .unwrap();
        pictures.poll(&ctx);
        assert!(pictures.animation_error("fixture").is_none());
    }
    #[test]
    fn avatar_sources_share_size_variants_and_keep_signatures() {
        let person = |size| notdiscord_core::models::Person {
            id: "10".into(),
            name: "Fixture".into(),
            avatar_url: Some(format!(
                "https://cdn.discordapp.com/avatars/10/hash.png?size={size}&ex=signature"
            )),
            ..Default::default()
        };
        assert_eq!(avatar_source(&person(32)), avatar_source(&person(256)));
        assert!(avatar_source(&person(32)).contains("ex=signature"));
        assert!(avatar_source(&person(32)).contains("size=256"));
        assert_eq!(
            avatar_uv(egui::vec2(200., 100.)).size(),
            egui::vec2(0.5, 1.)
        );
        assert_eq!(
            avatar_uv(egui::vec2(100., 200.)).size(),
            egui::vec2(1., 0.5)
        );
        let url = "https://cdn.discordapp.com/avatars/10/hash.png?ex=ff&is=10&hm=keep&size=256";
        for (physical, expected) in [(40, "64"), (80, "128"), (384, "512")] {
            let request = url::Url::parse(&avatar_download_url(url, physical)).unwrap();
            let pairs: HashMap<_, _> = request.query_pairs().collect();
            assert_eq!(pairs["size"], expected);
            assert_eq!(pairs["ex"], "ff");
            assert_eq!(pairs["is"], "10");
            assert_eq!(pairs["hm"], "keep");
        }
    }
    #[test]
    fn compact_avatars_follow_physical_dpi_without_sharing_profile_or_viewer_decodes() {
        for (physical,expected) in [(24,32),(40,64),(80,128),(160,128),(384,128)] {
            assert_eq!(avatar_decode_side(physical,false),expected);
        }
        assert_eq!(avatar_decode_side(88,true),512);
        let source="https://cdn.discordapp.com/avatars/10/hash.png?size=256&hm=keep";
        assert_ne!(avatar_texture_key(source,128,false),avatar_texture_key(source,512,true));
        let compact=avatar_download_url(source,128);let profile=avatar_download_url(source,512);let viewer=avatar_download_url(source,2048);
        assert!(compact.contains("size=128"));assert!(profile.contains("size=512"));assert!(viewer.contains("size=2048"));
        for url in [compact,profile,viewer] {assert!(url.contains("hm=keep"));}
        // Dimension arithmetic only: halving each axis leaves one quarter of
        // RGBA pixels; this is not a measurement of process memory.
        assert_eq!((128*128*4)*4,256*256*4);
    }
    #[test]
    fn account_clear_releases_image_caches_and_drops_old_epoch_results() {
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        let (send, receive) = mpsc::channel();
        pictures.ready = receive;
        let result = |generation| {
            (
                generation,
                "fixture".into(),
                Ok((1, 1, vec![1, 2, 3, 255])),
                Arc::new(DecodeGate::default()).acquire(|| false).unwrap(),
            )
        };
        send.send(result(0)).unwrap();
        pictures.poll(&ctx);
        pictures
            .failed
            .insert("old".into(), (Instant::now(), "failure".into()));
        pictures
            .avatar_sources
            .insert("42".into(), ("old-url".into(), 0));
        pictures
            .local_sources
            .insert("old-local".into(), PathBuf::from("old"));
        pictures.pending.insert("old".into());
        assert!(pictures.used > 0);
        pictures.clear_account();
        assert_eq!(pictures.used, 0);
        assert_eq!(pictures.textures.capacity(), 0);
        assert_eq!(pictures.failed.capacity(), 0);
        assert_eq!(pictures.avatar_sources.capacity(), 0);
        assert_eq!(pictures.local_sources.capacity(), 0);
        assert_eq!(pictures.pending.capacity(), 0);
        send.send(result(0)).unwrap();
        pictures.poll(&ctx);
        assert!(pictures.textures.is_empty());
        send.send(result(pictures.image_generation.load(Ordering::Acquire)))
            .unwrap();
        pictures.poll(&ctx);
        assert_eq!(pictures.textures.len(), 1);
    }
    #[test]
    fn changed_avatar_releases_unshared_image_and_keeps_shared_users() {
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        let mut first = notdiscord_core::models::Person {
            id: "10".into(),
            name: "Fixture".into(),
            avatar_url: Some("https://cdn.discordapp.com/avatars/10/old.png".into()),
            ..Default::default()
        };
        let old = pictures.remember_avatar(&first);
        let key = image_key(&old, AVATAR_SIDE);
        let texture = ctx.load_texture(
            "fixture-avatar",
            ColorImage::from_rgba_unmultiplied([1, 1], &[20, 40, 60, 255]),
            TextureOptions::LINEAR,
        );
        pictures.textures.insert(key.clone(), (texture, 4, 0));
        pictures.used = 4;
        let mut second = first.clone();
        second.id = "11".into();
        pictures.remember_avatar(&second);
        first.avatar_url = Some("https://cdn.discordapp.com/avatars/10/new.png".into());
        pictures.remember_avatar(&first);
        assert!(pictures.textures.contains_key(&key));
        second.avatar_url = None;
        pictures.remember_avatar(&second);
        assert!(!pictures.textures.contains_key(&key));
        assert_eq!(pictures.used, 0);
        assert!(pictures.pending.is_empty());
    }
    #[test]
    fn avatar_fallback_is_immediate_without_download_and_owner_cache_is_bounded() {
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        let mut person = notdiscord_core::models::Person {
            id: "10".into(),
            name: "Fixture".into(),
            ..Default::default()
        };
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = pictures.avatar(ui, &person, 36.);
                assert_eq!(response.rect.size(), Vec2::splat(36.));
            });
        });
        assert!(pictures.pending.is_empty() && pictures.textures.is_empty());
        person.avatar_url = Some("https://cdn.discordapp.com/avatars/10/stale.png".into());
        ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<HashMap<String, Option<String>>>(egui::Id::new(
                "canonical-avatar-urls",
            ))
            .insert("10".into(), None);
        });
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                pictures.avatar(ui, &person, 36.);
            });
        });
        assert!(
            pictures.pending.is_empty(),
            "A stale message author cannot re-download a removed canonical avatar"
        );
        for id in 0..700 {
            pictures.remember_avatar(&notdiscord_core::models::Person {
                id: id.to_string(),
                ..Default::default()
            });
        }
        assert_eq!(pictures.avatar_sources.len(), MAX_AVATAR_USERS);
    }
    #[test]
    fn shared_image_cache_limits_count_as_well_as_bytes() {
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        let (send, receive) = mpsc::channel();
        pictures.ready = receive;
        for index in 0..300 {
            let key = format!("fixture-{index}");
            send.send((
                0,
                key,
                Ok((1, 1, vec![10, 20, 30, 255])),
                Arc::new(DecodeGate::default()).acquire(|| false).unwrap(),
            ))
            .unwrap();
        }
        pictures.poll(&ctx);
        assert_eq!(pictures.textures.len(), MAX_TEXTURES);
        assert_eq!(pictures.used, MAX_TEXTURES * 4);
        assert!(pictures.used <= BUDGET);
    }
    use std::borrow::Cow;
    fn gif_frames(count: usize, delay: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut bytes, 2, 1, &[255, 0, 0, 0, 0, 255]).unwrap();
            encoder.set_repeat(gif::Repeat::Infinite).unwrap();
            for index in 0..count {
                let mut f = gif::Frame::default();
                f.width = 2;
                f.height = 1;
                f.delay = delay;
                f.buffer = Cow::Owned(vec![(index % 2) as u8; 2]);
                encoder.write_frame(&f).unwrap();
            }
        }
        bytes
    }
    #[test]
    fn keeps_all_four_hundred_frames_and_long_delays() {
        let decoded = decode_animation_bytes(&gif_frames(400, 500), || false);
        assert_eq!(decoded.len(), 400);
        assert_eq!(decoded[0].3, Duration::from_secs(5));
        assert_ne!(decoded[0].2, decoded[1].2);
        assert!(decoded.iter().map(|f| f.2.len()).sum::<usize>() <= 8 * 1024 * 1024);
    }
    #[test]
    fn invalid_over_limit_animation_returns_no_partial_playback() {
        assert!(decode_animation_bytes(&gif_frames(401, 10), || false).is_empty());
        let decoded = decode_animation_bytes(&gif_frames(2, 2000), || false);
        assert_eq!(decoded[0].3, Duration::from_secs(10));
    }
    #[test]
    fn honours_background_disposal() {
        let mut bytes = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut bytes, 2, 1, &[255, 0, 0, 0, 0, 255]).unwrap();
            let mut first = gif::Frame::default();
            first.width = 2;
            first.height = 1;
            first.delay = 10;
            first.dispose = gif::DisposalMethod::Background;
            first.buffer = Cow::Borrowed(&[0, 0]);
            encoder.write_frame(&first).unwrap();
            let mut next = gif::Frame::default();
            next.left = 1;
            next.width = 1;
            next.height = 1;
            next.delay = 10;
            next.buffer = Cow::Borrowed(&[1]);
            encoder.write_frame(&next).unwrap();
        }
        let decoded = decode_animation_bytes(&bytes, || false);
        assert_eq!(decoded.len(), 2);
        assert_eq!(&decoded[1].2[..4], &[0, 0, 0, 0]);
        assert_eq!(&decoded[1].2[4..], &[0, 0, 255, 255]);
    }
    #[test]
    fn cancelled_generation_does_not_publish_partial_animation() {
        assert!(decode_animation_bytes(&gif_frames(20, 10), || true).is_empty());
    }
    #[test]
    fn disallows_untrusted_or_control_character_media() {
        assert!(!allowed("https://example.com/image.png"));
        assert!(!allowed("http://cdn.discordapp.com/a.png"));
        assert!(!allowed("https://cdn.discordapp.com/a\n.png"));
        assert!(!allowed("https://user@cdn.discordapp.com/a.png"));
        assert!(allowed("https://static2.klipy.com/abc.gif"));
    }
    fn png_fixture(width: u32, height: u32) -> Vec<u8> {
        let source = image::RgbaImage::from_pixel(width, height, image::Rgba([25, 50, 75, 128]));
        let mut bytes = Cursor::new(Vec::new());
        source
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[test]
    fn original_pixels_are_not_silently_reduced_to_viewer_thumbnail() {
        let bytes = png_fixture(2000, 10);
        let original = decode_encoded(&bytes, 8192).expect("original WIC pixels");
        let thumbnail = decode_encoded(&bytes, 384).expect("thumbnail WIC pixels");
        assert_eq!((original.0, original.1), (2000, 10));
        assert_eq!((thumbnail.0, thumbnail.1), (384, 1));
        assert_eq!(original.2[3], 128);
    }
    #[test]
    fn raised_limit_reaches_the_streamed_file_decoder_without_the_old_twenty_five_mib_cutoff() {
        let directory=std::env::temp_dir().join(format!("notdiscord-configured-image-{}",uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&directory).unwrap();
        let png=png_fixture(2,1);
        let length=26*1024*1024;
        // A tiny valid PNG with trailing padding exercises encoded file size
        // separately from pixel dimensions, without a large pixel fixture.
        let source=Cursor::new(&png).chain(std::io::repeat(0).take(length-png.len() as u64));
        let spool=EncodedSpool::copy_from(source,Some(length),50*1024*1024,&directory).unwrap();
        let pixels=decode_file(spool.path(),128).expect("The configured encoded limit must reach WIC too");
        assert_eq!((pixels.0,pixels.1),(2,1));
        drop(spool);
        assert!(EncodedSpool::copy_from(Cursor::new([]),Some(length),25*1024*1024,&directory).is_err());
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn accepts_images_over_old_five_mib_limit_but_bounds_chunked_downloads() {
        let data = vec![0u8; 5 * 1024 * 1024 + 1];
        assert_eq!(
            response_bytes(Cursor::new(&data), None, MAX_ENCODED)
                .unwrap()
                .len(),
            data.len()
        );
        assert!(response_bytes(Cursor::new([1u8; 33]), None, 32).is_err());
        assert!(response_bytes(Cursor::new([]), Some(33), 32).is_err());
    }
    #[test]
    fn configured_download_limit_updates_workers_and_discards_old_failures_and_completions() {
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        let url = "https://cdn.discordapp.com/attachments/1/2/synthetic.png";
        pictures.seed_sized_fixture(&ctx,url,128);
        let old = pictures.image_generation.load(Ordering::Acquire);
        pictures.pending.insert("old-request".into());
        pictures.failed.insert("old-limit".into(),(Instant::now(),super::super::media_limit::error(25*1024*1024)));
        pictures.animation_pending = Some((0,"old-gif".into()));
        pictures.set_download_limit(50*1024*1024);
        assert_eq!(pictures.download_limit.load(Ordering::Acquire),50*1024*1024);
        assert!(pictures.image_generation.load(Ordering::Acquire)>old);
        assert!(pictures.pending.is_empty() && pictures.failed.is_empty());
        assert!(pictures.animation_pending.is_none());
        assert!(pictures.textures.contains_key(&image_key(url,128)));
        let (send,receive)=mpsc::sync_channel(2);
        pictures.ready=receive;
        let gate=Arc::new(DecodeGate::default());
        send.send((old,"stale".into(),Err("old error".into()),gate.acquire(||false).unwrap())).unwrap();
        pictures.poll(&ctx);
        assert!(!pictures.failed.contains_key("stale"));
        let current=pictures.image_generation.load(Ordering::Acquire);
        pictures.set_download_limit(50*1024*1024);
        assert_eq!(pictures.image_generation.load(Ordering::Acquire),current,"Unchanged limits must not cancel requests every frame");
    }
    #[test]
    fn download_limit_uses_the_selected_boundary_and_error_text_for_known_and_chunked_bodies() {
        for limit in [8,32,1024] {
            assert_eq!(response_bytes(std::io::repeat(7).take(limit),Some(limit),limit).unwrap().len() as u64,limit);
            assert_eq!(response_bytes(std::io::repeat(7).take(limit+1),None,limit).unwrap_err(),super::super::media_limit::error(limit));
            assert_eq!(response_bytes(Cursor::new([]),Some(limit+1),limit).unwrap_err(),super::super::media_limit::error(limit));
        }
    }
    #[test]
    fn static_signed_cdn_links_are_previews_and_expired_links_are_errors() {
        assert!(is_image_url(
            "https://cdn.discordapp.com/attachments/1/2/image.PNG?ex=123&is=456&hm=789"
        ));
        assert!(!is_image_url("https://example.com/image.png"));
        assert!(!is_image_url("https://cdn.discordapp.com/movie.mp4"));
        assert!(!is_image_url("https://cdn.discordapp.com/image.svg"));
        assert!(
            http_image_error(reqwest::StatusCode::FORBIDDEN)
                .unwrap()
                .contains("caducado")
        );
        assert!(
            http_image_error(reqwest::StatusCode::NOT_FOUND)
                .unwrap()
                .contains("HTTP 404/410")
        );
        assert!(http_image_error(reqwest::StatusCode::OK).is_none());
    }
    #[test]
    fn explicit_local_attachment_uses_worker_and_decoding_queue_survives_navigation() {
        let root =
            std::env::temp_dir().join(format!("notdiscord-pictures-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("fixture.png");
        std::fs::write(&path, png_fixture(80, 40)).unwrap();
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        assert!(
            pictures
                .texture_sized(&path.to_string_lossy(), 128)
                .is_none(),
            "remote API must reject arbitrary paths"
        );
        assert!(pictures.texture_path(&path, 128).is_none());
        pictures.stop();
        assert!(pictures.texture_path(&path, 128).is_none());
        let deadline = Instant::now() + Duration::from_secs(3);
        let texture = loop {
            pictures.poll(&ctx);
            if let Some(t) = pictures.texture_path(&path, 128) {
                break t;
            }
            assert!(
                Instant::now() < deadline,
                "worker should finish a local fixture after navigation"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(texture.size(), [80, 40]);
        let bad = root.join("bad.png");
        std::fs::write(&bad, b"not a PNG").unwrap();
        pictures.texture_path(&bad, 128);
        while pictures.path_error(&bad, 128).is_none() {
            pictures.poll(&ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(bad).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn actual_workers_keep_one_bitmap_with_a_slow_consumer_and_cancel_waiters_on_account_clear() {
        let root = std::env::temp_dir().join(format!(
            "notdiscord-image-admission-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let image = png_fixture(1600, 1200);
        let paths: Vec<_> = (0..3)
            .map(|index| {
                let path = root.join(format!("{index}.png"));
                std::fs::write(&path, &image).unwrap();
                path
            })
            .collect();
        drop(image);
        let ctx = egui::Context::default();
        let mut pictures = Pictures::new(ctx.clone());
        for path in &paths {
            assert!(pictures.texture_path(path, 1600).is_none());
        }
        let first = pictures.ready.recv_timeout(Duration::from_secs(3)).unwrap();
        let (_, _, pixels) = first.2.as_ref().unwrap();
        assert_eq!(pixels.len(), 1600 * 1200 * 4);
        let deadline = Instant::now() + Duration::from_secs(3);
        while pictures.decode_gate.waiting() < 2 {
            assert!(
                Instant::now() < deadline,
                "All remaining producers must wait before decoding"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            pictures.ready.try_recv().is_err(),
            "Blocked senders cannot hide extra decoded bitmaps"
        );
        pictures.clear_account();
        drop(first);
        assert!(pictures.texture_path(&paths[0], 128).is_none());
        let texture = loop {
            pictures.poll(&ctx);
            if let Some(texture) = pictures.texture_path(&paths[0], 128) {
                break texture;
            }
            assert!(
                Instant::now() < deadline,
                "A new epoch must not deadlock behind old results"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(texture.size(), [128, 96]);
        assert!(pictures.large.is_none(), "No old large bitmap may reappear");
        drop(pictures);
        for path in paths {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn discarded_async_paste_cleans_only_its_owned_temporary() {
        let root = std::env::temp_dir().join(format!(
            "notdiscord-paste-transfer-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).unwrap();
        let dropped = root.join("discarded.png");
        std::fs::write(&dropped, b"fixture").unwrap();
        drop(PastedAttachment {
            channel: "200".into(),
            account: "100".into(),
            result: Ok(Some(dropped.clone())),
        });
        assert!(!dropped.exists());
        let retained = root.join("transferred.png");
        std::fs::write(&retained, b"fixture").unwrap();
        let mut item = PastedAttachment {
            channel: "200".into(),
            account: "100".into(),
            result: Ok(Some(retained.clone())),
        };
        assert_eq!(item.take_result().unwrap(), Some(retained.clone()));
        drop(item);
        assert!(retained.exists());
        std::fs::remove_file(retained).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
