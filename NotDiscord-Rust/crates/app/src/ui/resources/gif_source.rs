//! Resolves provider pages without confusing their poster with their animation.
//! The shared HTTP client never contains a Discord token or browser cookies.
use super::*;
use notdiscord_core::parsing::{animation_rendition, provider_animation_source};

const PAGE_BYTES: u64 = 2 * 1024 * 1024;
#[derive(Default, Clone)]
struct Sources {
    poster: Option<String>,
    animation: Option<String>,
    video_only: bool,
}
#[derive(Default)]
struct Entry {
    sources: Sources,
    checked: Option<Instant>,
    error: Option<String>,
    pending: bool,
}
#[derive(Default)]
struct Cache {
    entries: Mutex<HashMap<String, Entry>>,
    changed: Condvar,
}
static CACHE: OnceLock<Cache> = OnceLock::new();
fn cache() -> &'static Cache {
    CACHE.get_or_init(Cache::default)
}
#[derive(Clone, Copy)]
pub(super) enum Want {
    Poster,
    Animation,
}
impl Sources {
    fn choose(&self, want: Want) -> Option<String> {
        match want {
            Want::Animation => self.animation.clone(),
            Want::Poster => self.poster.as_ref().or(self.animation.as_ref()).cloned(),
        }
    }
}
pub fn remember_source(page: &str, media: &str) -> bool {
    if !notdiscord_core::parsing::gif_page(page) || !allowed(media) || !animation_rendition(media) {
        return false;
    }
    let mut entries = cache()
        .entries
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if entries.len() >= 128 && !entries.contains_key(page) {
        if let Some(old) = entries
            .iter()
            .filter(|(_, entry)| !entry.pending)
            .min_by_key(|(_, entry)| entry.checked)
            .map(|(key, _)| key.clone())
        {
            entries.remove(&old);
        } else {
            return false;
        }
    }
    let entry = entries.entry(page.into()).or_default();
    let changed = entry.sources.animation.as_deref() != Some(media);
    entry.sources.animation = Some(media.into());
    entry.checked = Some(Instant::now());
    entry.error = None;
    cache().changed.notify_all();
    changed
}
/// Used by explicit retry, without deleting a usable poster or a known rendition.
pub(super) fn retry(page: &str) {
    let mut entries = cache()
        .entries
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = entries.get_mut(page) {
        if entry.error.take().is_some() {
            entry.checked = None;
        }
    }
}
pub(super) fn invalidate(page: &str, source: &str) {
    let mut entries = cache()
        .entries
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(entry) = entries.get_mut(page) {
        if entry.sources.animation.as_deref() == Some(source) {
            entry.sources.animation = None;
        }
        if entry.sources.poster.as_deref() == Some(source) {
            entry.sources.poster = None;
        }
        entry.checked = None;
        entry.error = None;
    }
}
pub fn resolved_url(value: &str) -> Option<String> {
    if allowed(value) {
        Some(value.into())
    } else {
        resolve(value, Want::Animation, || false).ok()
    }
}
pub(super) fn resolve(
    value: &str,
    want: Want,
    cancelled: impl Fn() -> bool,
) -> Result<String, String> {
    if allowed(value) {
        return if matches!(want, Want::Poster) || animation_rendition(value) {
            Ok(value.into())
        } else {
            Err("El enlace no contiene una animación GIF o WebP compatible; MP4/WebM no se envían al decodificador de GIF.".into())
        };
    }
    if !notdiscord_core::parsing::gif_page(value) {
        return Err("Origen multimedia no permitido o no disponible.".into());
    }
    let cache = cache();
    loop {
        if cancelled() {
            return Err("Carga de animación cancelada.".into());
        }
        let mut entries = cache
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if entries.len() >= 128 && !entries.contains_key(value) {
            if let Some(old) = entries
                .iter()
                .filter(|(_, entry)| !entry.pending)
                .min_by_key(|(_, entry)| entry.checked)
                .map(|(key, _)| key.clone())
            {
                entries.remove(&old);
            } else {
                return Err("Hay demasiadas páginas GIF pendientes; vuelve a intentarlo cuando termine una carga.".into());
            }
        }
        let entry = entries.entry(value.into()).or_default();
        if entry
            .checked
            .is_some_and(|at| at.elapsed() < Duration::from_secs(15 * 60))
        {
            if let Some(source) = entry.sources.choose(want) {
                return Ok(source);
            }
            if entry
                .checked
                .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
            {
                if let Some(error) = &entry.error {
                    return Err(error.clone());
                }
            }
        }
        if entry.pending {
            drop(
                cache
                    .changed
                    .wait_timeout(entries, Duration::from_millis(100)),
            );
            continue;
        }
        entry.pending = true;
        drop(entries);
        break;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        read_page(value, &cancelled)
    }))
    .unwrap_or_else(|_| {
        Err("No se pudo leer el origen de la animación. Puedes reintentar.".into())
    });
    let mut entries = cache
        .entries
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let entry = entries.entry(value.into()).or_default();
    entry.pending = false;
    if cancelled() {
        entry.checked = None;
        entry.error = None;
        cache.changed.notify_all();
        return Err("Carga de animación cancelada.".into());
    }
    entry.checked = Some(Instant::now());
    match result {
        Ok(sources) => {
            if entry.sources.animation.is_none() {
                entry.sources.animation = sources.animation;
            }
            if entry.sources.poster.is_none() {
                entry.sources.poster = sources.poster;
            }
            entry.sources.video_only = sources.video_only;
            entry.error=entry.sources.animation.is_none().then(||if entry.sources.video_only{
                "El proveedor sólo publicó una variante MP4/WebM; no entregó una animación GIF/WebP compatible. La vista previa sigue disponible. Puedes reintentar o abrir el original.".into()
            }else{"La página no publicó un enlace de animación GIF/WebP. Se conserva su vista previa; puedes reintentar o abrir el original.".into()});
        }
        Err(error) => entry.error = Some(error),
    }
    let output = entry.sources.choose(want).ok_or_else(|| {
        entry
            .error
            .clone()
            .unwrap_or_else(|| "El proveedor no entregó una imagen compatible.".into())
    });
    // Keep the existing bounded session cache; concurrent waiters only refer to
    // entries that are pending and are never evicted while resolving.
    if entries.len() > 128 {
        if let Some(old) = entries
            .iter()
            .filter(|(key, entry)| key.as_str() != value && !entry.pending)
            .min_by_key(|(_, entry)| entry.checked)
            .map(|(key, _)| key.clone())
        {
            entries.remove(&old);
        }
    }
    cache.changed.notify_all();
    output
}
/// Redirects are followed explicitly so each target is revalidated before any
/// request. No arbitrary host, HTTP downgrade, credentials or unbounded loop.
pub(super) fn get(
    value: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<reqwest::blocking::Response, String> {
    let client = client().ok_or("No se pudo preparar la conexión multimedia.")?;
    let mut current = value.to_owned();
    for _ in 0..=4 {
        if cancelled() {
            return Err("Carga multimedia cancelada.".into());
        }
        if !allowed(&current) && !notdiscord_core::parsing::gif_page(&current) {
            return Err("El proveedor redirigió a un origen no permitido.".into());
        }
        let response = client.get(&current).send().map_err(|error| {
            if error.is_timeout() {
                "La consulta multimedia tardó demasiado. Puedes reintentar.".to_owned()
            } else {
                "No se pudo conectar con el proveedor multimedia.".into()
            }
        })?;
        if response.status().is_redirection() {
            let next = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("El proveedor devolvió una redirección sin destino.")?;
            current = url::Url::parse(&current)
                .ok()
                .and_then(|base| base.join(next).ok())
                .map(String::from)
                .ok_or("El proveedor devolvió una redirección no válida.")?;
            continue;
        }
        if !response.status().is_success() {
            return Err(match response.status().as_u16(){
                401|403=>format!("El proveedor devolvió HTTP {} y no permite resolver o descargar la animación. Puedes reintentar o abrir el original; la vista previa no demuestra que la animación esté disponible.",response.status().as_u16()),
                404|410=>format!("El proveedor devolvió HTTP {}: el enlace multimedia puede haber caducado. Puedes reintentar o abrir el original.",response.status().as_u16()),
                429=>"El proveedor limitó las consultas multimedia (HTTP 429). Espera antes de reintentar.".into(),
                _=>format!("El proveedor multimedia devolvió HTTP {}. Puedes reintentar.",response.status().as_u16()),
            });
        }
        return Ok(response);
    }
    Err("El proveedor superó el límite de redirecciones multimedia.".into())
}
fn read_page(page: &str, cancelled: &impl Fn() -> bool) -> Result<Sources, String> {
    let response = get(page, cancelled)?;
    if allowed(response.url().as_str()) && animation_rendition(response.url().as_str()) {
        return Ok(Sources {
            animation: Some(response.url().to_string()),
            ..Default::default()
        });
    }
    let length = response.content_length();
    let bytes = response_bytes_cancellable(response, length, PAGE_BYTES, cancelled)?;
    if cancelled() {
        return Err("Carga de animación cancelada.".into());
    }
    Ok(from_html(&String::from_utf8_lossy(&bytes), page))
}
fn unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("\\/", "/")
        .replace("\\u002F", "/")
        .replace("\\u002f", "/")
        .replace("\\u0026", "&")
}
fn from_html(html: &str, page: &str) -> Sources {
    let mut sources = Sources::default();
    // Parse meta attributes independent of attribute order. An og:image is only
    // a poster; KLIPY documents preview GIFs as single-frame images.
    let meta = regex::Regex::new(r"(?is)<meta\b[^>]{0,8192}>").unwrap();
    let attr =
        regex::Regex::new(r#"(?is)\b(property|name|content)\s*=\s*["']([^"']*)["']"#).unwrap();
    for tag in meta.find_iter(html).take(64) {
        let mut key = String::new();
        let mut content = String::new();
        for field in attr.captures_iter(tag.as_str()) {
            if field[1].eq_ignore_ascii_case("content") {
                content = unescape(&field[2]);
            } else {
                key = field[2].to_ascii_lowercase();
            }
        }
        if !allowed(&content) {
            continue;
        }
        match key.as_str() {
            "og:image" | "og:image:url" | "twitter:image" => {
                if sources.poster.is_none() {
                    sources.poster = Some(content);
                }
            }
            "og:video" | "og:video:url" | "og:video:secure_url" | "twitter:player:stream" => {
                if animation_rendition(&content) {
                    sources.animation.get_or_insert(content);
                } else {
                    sources.video_only = true;
                }
            }
            _ => {}
        }
    }
    let scripts = regex::Regex::new(r"(?is)<script\b([^>]{0,8192})>(.*?)</script\s*>").unwrap();
    for script in scripts.captures_iter(html).take(128) {
        let attributes = script[1].to_ascii_lowercase();
        let text = script[2].trim();
        if text.len() > PAGE_BYTES as usize {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            let mut budget = 4096;
            find_page_media(
                &value,
                page,
                attributes.contains("ld+json") || page.is_empty(),
                0,
                &mut budget,
                &mut sources,
            );
        }
    }
    sources
}
fn find_page_media(
    value: &serde_json::Value,
    page: &str,
    main_entity: bool,
    depth: u8,
    budget: &mut usize,
    sources: &mut Sources,
) {
    if depth > 16 || *budget == 0 {
        return;
    }
    *budget -= 1;
    match value {
        serde_json::Value::Object(object) => {
            let slug = url::Url::parse(page)
                .ok()
                .and_then(|url| {
                    url.path_segments()
                        .and_then(|mut parts| parts.next_back())
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            let matches = object
                .get("slug")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == slug)
                || ["itemurl", "url", "@id"]
                    .iter()
                    .filter_map(|key| object.get(*key).and_then(|v| v.as_str()))
                    .any(|url| notdiscord_core::parsing::same_media_resource(url, page));
            let content = object.get("contentUrl").and_then(|v| v.as_str());
            if matches || main_entity && content.is_some() {
                if let Some(source) = provider_animation_source(value) {
                    sources.animation.get_or_insert(source);
                }
                if content.is_some_and(|url| allowed(url) && !animation_rendition(url)) {
                    sources.video_only = true;
                }
            }
            for (key, child) in object {
                let lower = key.to_ascii_lowercase();
                if lower.contains("preview")
                    || lower.contains("thumbnail")
                    || lower.ends_with("_still")
                {
                    continue;
                }
                if matches!(
                    key.as_str(),
                    "related"
                        | "recommendations"
                        | "related_items"
                        | "suggestions"
                        | "itemListElement"
                ) {
                    continue;
                }
                find_page_media(
                    child,
                    page,
                    main_entity || matches,
                    depth + 1,
                    budget,
                    sources,
                );
            }
        }
        serde_json::Value::Array(values) => {
            for child in values.iter().take(128) {
                find_page_media(child, page, main_entity, depth + 1, budget, sources);
            }
        }
        _ => {}
    }
}
#[cfg(test)]
pub(super) fn media_from_html(html: &str) -> Option<String> {
    from_html(html, "").animation
}
