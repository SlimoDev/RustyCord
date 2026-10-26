//! GIF providers return both still previews and several independent rendition
//! URLs. A preview/MP4 URL must never be substituted for the GIF/WebP rendition.
use super::*;

pub fn direct_animation(value: &str) -> bool {
    direct_gif(value)
        || animation_rendition(value)
            && url::Url::parse(value).is_ok_and(|url| {
                url.query_pairs().any(|(key, value)| {
                    key.eq_ignore_ascii_case("animated") && value.eq_ignore_ascii_case("true")
                })
            })
}
/// A decodable rendition URL in an explicit animation metadata context. Bare
/// WebP links outside that context remain ordinary images.
pub fn animation_rendition(value: &str) -> bool {
    if direct_gif(value) {
        return true;
    }
    if !trusted_media(value) {
        return false;
    }
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    if url.query_pairs().any(|(key, value)| {
        key.eq_ignore_ascii_case("animated") && value.eq_ignore_ascii_case("false")
    }) {
        return false;
    }
    let format = url
        .query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case("format"))
        .map(|(_, value)| value.into_owned());
    let webp = format
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("webp"))
        || format.is_none() && url.path().to_ascii_lowercase().ends_with(".webp");
    webp && !value.chars().any(char::is_control)
}
fn animation_url(value: &Value) -> Option<String> {
    let raw = value
        .as_str()
        .or_else(|| value.get("url").and_then(Value::as_str))?;
    let raw = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_owned()
    }
    .replace("\\/", "/");
    animation_rendition(&raw).then_some(raw)
}
/// Reads only explicitly named animation fields. `preview`, `gifpreview`,
/// `*_still` and video renditions are deliberately not fallback animations.
pub fn provider_animation_source(value: &Value) -> Option<String> {
    for field in ["gif_src", "gifSrc"] {
        if let Some(source) = animation_url(&value[field]) {
            return Some(source);
        }
    }
    let formats = &value["media_formats"];
    for field in [
        "gif",
        "mediumgif",
        "tinygif",
        "nanogif",
        "gif_transparent",
        "tinygif_transparent",
        "nanogif_transparent",
        "webp",
        "webp_transparent",
        "tinywebp_transparent",
        "nanowebp_transparent",
    ] {
        if let Some(source) = animation_url(&formats[field]) {
            return Some(source);
        }
    }
    for size in ["hd", "md", "sm", "xs"] {
        for format in ["gif", "webp"] {
            if let Some(source) = animation_url(&value["file"][size][format]) {
                return Some(source);
            }
        }
    }
    for field in ["contentUrl", "src"] {
        if let Some(source) = animation_url(&value[field]) {
            return Some(source);
        }
    }
    None
}
