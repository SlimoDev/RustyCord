use base64::Engine;
use notdiscord_core::parsing::{i, items, s};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[path="pickers/favorites_wire.rs"]
pub mod favorites_wire;

pub fn safe_link(value: &str) -> bool {
    if value.len() > 4096 {
        return false;
    }
    let Ok(u) = url::Url::parse(value) else {
        return false;
    };
    if u.scheme() != "https"
        || u.port_or_known_default() != Some(443)
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return false;
    }
    let h = u.host_str().unwrap_or("");
    matches!(
        h,
        "tenor.com"
            | "www.tenor.com"
            | "giphy.com"
            | "www.giphy.com"
            | "klipy.com"
            | "www.klipy.com"
            | "cdn.discordapp.com"
            | "media.discordapp.net"
    ) || media_host(h)
}
pub fn media_host(h: &str) -> bool {
    matches!(
        h,
        "static.klipy.com"
            | "static1.klipy.com"
            | "static2.klipy.com"
            | "media.tenor.co"
            | "c.tenor.com"
    ) || regex::Regex::new(r"^(media\d{0,2}\.tenor\.com|(?:media\d{0,2}|i)\.giphy\.com)$")
        .unwrap()
        .is_match(h)
}
pub fn is_page(value: &str) -> bool {
    safe_link(value) && notdiscord_core::parsing::gif_page(value)
}
pub fn normalized(value: &str) -> String {
    let value = value.trim();
    let link = if value.starts_with("//") {
        format!("https:{value}")
    } else {
        value.into()
    };
    if safe_link(&link) {
        link
    } else {
        String::new()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct GifItem {
    pub id: String,
    pub url: String,
    pub thumbnail_url: String,
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub animation_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct GifCategory {
    pub name: String,
    pub thumbnail_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct GifPage {
    pub items: Vec<GifItem>,
    pub categories: Vec<GifCategory>,
    pub has_more: bool,
    pub note: String,
}
/// Correlation crosses the UI/backend boundary intact. It is never a cache key
/// containing an account; caches are discarded when the session is replaced.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRequest {
    pub owner: String,
    pub generation: u64,
    pub opening: String,
    pub request: String,
    pub mode: String,
    pub query: String,
    pub limit: usize,
}
impl GifItem {
    pub fn descriptor(&self) -> notdiscord_core::parsing::MediaDescriptor {
        notdiscord_core::parsing::media_descriptor(&notdiscord_core::models::ChatAttachment {
            url:self.url.clone(),preview_url:(!self.thumbnail_url.is_empty()).then(||self.thumbnail_url.clone()),
            animation_url:(!self.animation_url.is_empty()).then(||self.animation_url.clone()),
            width:self.width,height:self.height,animated:true,..Default::default()
        })
    }
}
pub fn endpoint(mode: &str, text: &str, limit: usize) -> Result<String, String> {
    let limit = if limit <= 20 {
        20
    } else if limit <= 50 {
        50
    } else {
        100
    };
    let q = format!("media_format=gif&locale=es-ES&limit={limit}");
    Ok(match mode {
        "categories" => format!("gifs/trending?{q}"),
        "trending" => format!("gifs/trending-gifs?{q}"),
        "search" if !text.trim().is_empty() && text.chars().count() <= 150 => format!(
            "gifs/search?q={}&{q}",
            url::form_urlencoded::byte_serialize(text.trim().as_bytes()).collect::<String>()
        ),
        "favorites" => "users/@me/settings-proto/2".into(),
        _ => return Err("Consulta de GIF no válida.".into()),
    })
}
fn preview(value: &str) -> String {
    if !safe_link(value) || is_page(value) {
        return String::new();
    }
    let u = url::Url::parse(value).unwrap();
    let path = u.path().to_ascii_lowercase();
    if path.ends_with(".mp4") || path.ends_with(".webm") {
        String::new()
    } else {
        value.into()
    }
}
fn text(value: &Value, key: &str) -> String {
    notdiscord_core::parsing::limited(&s(value, key), 4096)
}
pub fn parse(value: &Value, mode: &str, limit: usize) -> Result<GifPage, String> {
    if mode == "favorites" {
        let encoded=value.get("settings").and_then(Value::as_str).ok_or("Discord no entregó el estado de guardados; la lista confirmada se conserva")?;
        return Ok(GifPage{items:favorites(encoded)?,note:"Incluye tus guardados de Discord. Las estrellas nuevas se guardan en NotDiscord para esta cuenta.".into(),..Default::default()});
    }
    let categories = items(&value["categories"])
        .iter()
        .take(50)
        .filter_map(|c| {
            let name = text(c, "name");
            (!name.is_empty() && name.encode_utf16().count() <= 150).then(|| GifCategory {
                name,
                thumbnail_url: preview(&normalized(&text(c, "src"))),
            })
        })
        .collect();
    let values = if value.is_array() {
        value
    } else {
        &value["gifs"]
    };
    let list = items(values)
        .iter()
        .take(limit.clamp(1, 96))
        .filter_map(|v| {
            let link = normalized(&text(v, "url"));
            let animation = notdiscord_core::parsing::provider_animation_source(v).unwrap_or_default();
            let src = ["gif_src", "gifSrc", "src"]
                .iter()
                .map(|key| normalized(&text(v, key)))
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            let link = if link.is_empty() { src.clone() } else { link };
            if link.is_empty() {
                return None;
            }
            let mut candidate = normalized(&text(v, "preview"));
            if preview(&candidate).is_empty() {
                candidate = src.clone()
            }
            let mut thumbnail = preview(&candidate);
            if thumbnail.is_empty() && is_page(&link) {
                thumbnail = link.clone()
            }
            let title = text(v, "title");
            Some(GifItem {
                id: text(v, "id"),
                url: link,
                title: if title.is_empty() {
                    "GIF".into()
                } else {
                    title
                },
                thumbnail_url: thumbnail,
                width: i(v, "width").clamp(0, 8192),
                height: i(v, "height").clamp(0, 8192),
                animation_url: animation,
            })
        })
        .collect::<Vec<_>>();
    Ok(GifPage {
        has_more: list.len() >= limit && limit < 96,
        items: list,
        categories,
        note: String::new(),
    })
}
pub fn favorites(encoded: &str) -> Result<Vec<GifItem>, String> {
    if encoded.len() > 1_400_000 {
        return Err("Guardados demasiado grandes.".into());
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Guardados no válidos.")?;
    if data.len() > 1024 * 1024 {
        return Err("Guardados demasiado grandes.".into());
    }
    let mut found = Vec::new();
    for root in fields(&data)? {
        if root.n != 2 || root.wire != 2 {
            continue;
        }
        for entry in fields(root.data)? {
            if entry.n != 1 || entry.wire != 2 || found.len() >= 400 {
                continue;
            }
            let mut link = String::new();
            let mut src = String::new();
            let (mut width, mut height, mut order) = (0, 0, 0);
            for map in fields(entry.data)? {
                if map.n == 1 && map.wire == 2 {
                    link = String::from_utf8_lossy(map.data).into_owned();
                }
                if map.n == 2 && map.wire == 2 {
                    for v in fields(map.data)? {
                        match (v.n, v.wire) {
                            (2, 2) => src = String::from_utf8_lossy(v.data).into_owned(),
                            (3, 0) => width = v.scalar.min(8192) as i32,
                            (4, 0) => height = v.scalar.min(8192) as i32,
                            (5, 0) => order = v.scalar,
                            _ => (),
                        }
                    }
                }
            }
            let mut link = normalized(&link);
            let src = normalized(&src);
            if link.is_empty() {
                link = src.clone()
            }
            if !link.is_empty() {
                let mut thumbnail = preview(&src);
                if thumbnail.is_empty() && is_page(&link) {
                    thumbnail = link.clone()
                }
                found.push((
                    order,
                    GifItem {
                        id: link.clone(),
                        url: link,
                        thumbnail_url: thumbnail,
                        title: "Guardado en Discord".into(),
                        width,
                        height,
                        animation_url: if notdiscord_core::parsing::direct_animation(&src) {
                            src
                        } else {
                            String::new()
                        },
                    },
                ));
            }
        }
    }
    found.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    let mut seen = std::collections::HashSet::new();
    Ok(found
        .into_iter()
        .map(|(_, v)| v)
        .filter(|v| seen.insert(v.url.clone()))
        .collect())
}
struct Field<'a> {
    n: u64,
    wire: u8,
    data: &'a [u8],
    scalar: u64,
}
fn varint(data: &[u8], c: &mut usize) -> Result<u64, String> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let b = *data.get(*c).ok_or("Guardados truncados.")?;
        *c += 1;
        if shift == 63 && b > 1 {
            return Err("Varint no válido.".into());
        }
        value |= ((b & 127) as u64) << shift;
        if b & 128 == 0 {
            return Ok(value);
        }
    }
    Err("Varint no válido.".into())
}
fn fields(data: &[u8]) -> Result<Vec<Field<'_>>, String> {
    let mut c = 0;
    let mut result = Vec::new();
    while c < data.len() {
        if result.len() >= 6000 {
            return Err("Demasiados campos.".into());
        }
        let tag = varint(data, &mut c)?;
        let n = tag >> 3;
        let wire = (tag & 7) as u8;
        if n == 0 {
            return Err("Campo no válido.".into());
        }
        if wire == 0 {
            let scalar = varint(data, &mut c)?;
            result.push(Field {
                n,
                wire,
                data: &[],
                scalar,
            });
            continue;
        }
        let size = match wire {
            1 => 8,
            5 => 4,
            2 => varint(data, &mut c)? as usize,
            _ => return Err("Wire no válido.".into()),
        };
        let end = c
            .checked_add(size)
            .filter(|n| *n <= data.len())
            .ok_or("Guardados truncados.")?;
        result.push(Field {
            n,
            wire,
            data: &data[c..end],
            scalar: 0,
        });
        c = end;
    }
    Ok(result)
}

/// Preserve Discord's emoji token and metadata. An unavailable emoji is an
/// actionable send error, never an invented CDN link with a second preview.
pub fn prepare_outgoing(text: &str, allow: impl Fn(&str) -> bool) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut start, mut cursor) = (0, 0);
    while cursor < bytes.len() {
        if bytes[cursor] != b'`' || escaped(bytes, cursor) {
            cursor += 1;
            continue;
        }
        out.push_str(&transform_plain(&text[start..cursor], &allow)?);
        let mut ticks = 1;
        while cursor + ticks < bytes.len() && bytes[cursor + ticks] == b'`' {
            ticks += 1
        }
        let mut close = cursor + ticks;
        while close < bytes.len() {
            if bytes[close] != b'`' || escaped(bytes, close) {
                close += 1;
                continue;
            }
            let mut end_ticks = 1;
            while close + end_ticks < bytes.len() && bytes[close + end_ticks] == b'`' {
                end_ticks += 1
            }
            if end_ticks == ticks {
                close += ticks;
                break;
            }
            close += end_ticks
        }
        out.push_str(&text[cursor..close]);
        cursor = close;
        start = close;
    }
    if start < text.len() {
        out.push_str(&transform_plain(&text[start..], &allow)?)
    }
    Ok(out)
}
fn escaped(bytes: &[u8], index: usize) -> bool {
    bytes[..index]
        .iter()
        .rev()
        .take_while(|b| **b == b'\\')
        .count()
        % 2
        == 1
}
fn transform_plain(text: &str, allow: &impl Fn(&str) -> bool) -> Result<String, String> {
    use std::sync::LazyLock;
    static TOKEN: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"https?://[^\s<>`"\x00-\x1F]+|<(?P<animated>a?):(?P<name>[^:\s<>]{1,64}):(?P<id>[0-9]{1,20})>"#).unwrap()
    });
    let mut unavailable = None;
    let output = TOKEN
        .replace_all(text, |c: &regex::Captures| {
            let m = c.get(0).unwrap();
            if m.as_str().starts_with("http") {
                clean_url(m.as_str())
            } else if text[..m.start()].ends_with('\\') || allow(&c["id"]) {
                m.as_str().into()
            } else {
                unavailable.get_or_insert_with(|| c["name"].to_owned());
                m.as_str().into()
            }
        })
        .into_owned();
    if let Some(name) = unavailable {
        Err(format!("El emoji :{name}: no está disponible con tus permisos o suscripción, o faltan datos recientes de su servidor. Abre su selector o servidor para actualizarlo. El borrador se conserva."))
    } else { Ok(output) }
}
fn clean_url(token: &str) -> String {
    let mut end = token.len();
    while end > 0 {
        let c = token[..end].chars().last().unwrap();
        let open = match c {
            ')' => '(',
            ']' => '[',
            '}' => '{',
            _ => '\0',
        };
        if ".,;".contains(c)
            || (open != '\0'
                && token[..end].matches(c).count() > token[..end].matches(open).count())
        {
            end -= c.len_utf8()
        } else {
            break;
        }
    }
    let value = &token[..end];
    let Some(q) = value.find('?') else {
        return token.into();
    };
    let hash = value.find('#').unwrap_or(value.len());
    if hash < q {
        return token.into();
    }
    let parts = value[q + 1..hash].split('&').collect::<Vec<_>>();
    let tracking = [
        "fbclid", "gclid", "dclid", "msclkid", "yclid", "mc_cid", "mc_eid", "igshid", "_hsenc",
        "_hsmi", "mkt_tok", "vero_id",
    ];
    let key = |p: &str| {
        url::form_urlencoded::parse(p.as_bytes())
            .next()
            .map(|(k, _)| k.to_lowercase())
            .unwrap_or_default()
    };
    if parts.iter().any(|p| {
        matches!(
            key(p).as_str(),
            "signature" | "sig" | "x-amz-signature" | "x-goog-signature"
        )
    }) {
        return token.into();
    }
    let kept = parts
        .iter()
        .filter(|p| {
            let k = key(p);
            !k.starts_with("utm_") && !tracking.contains(&k.as_str())
        })
        .copied()
        .collect::<Vec<_>>();
    format!(
        "{}{}{}{}",
        &value[..q],
        if kept.is_empty() {
            String::new()
        } else {
            format!("?{}", kept.join("&"))
        },
        &value[hash..],
        &token[end..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracking_and_signatures() {
        assert_eq!(
            prepare_outgoing("https://a.com/p?utm_source=x&ok=1.", |_| false).unwrap(),
            "https://a.com/p?ok=1."
        );
        assert_eq!(
            prepare_outgoing("https://a.com/p?sig=x&utm_source=y", |_| false).unwrap(),
            "https://a.com/p?sig=x&utm_source=y"
        );
    }
    #[test]
    fn emoji_tokens_keep_metadata_and_unavailable_tokens_do_not_become_urls() {
        assert_eq!(prepare_outgoing("<:a:123>", |_| true).unwrap(), "<:a:123>");
        assert!(prepare_outgoing("<a:a:123>", |_| false).is_err());
    }
    #[test]
    fn host_validation() {
        assert!(!safe_link("https://cdn.discordapp.com.evil.test/a"));
        assert!(safe_link("https://media1.tenor.com/a.gif"));
        assert!(!safe_link("http://media1.tenor.com/a.gif"));
    }
    #[test]
    fn code_and_incomplete_code_are_preserved() {
        for source in [
            "`<:a:123> https://a.com/?utm_x=y`",
            "```rs\n<:a:123>\n```",
            "`unfinished <:a:123>",
        ] {
            assert_eq!(prepare_outgoing(source, |_| false).unwrap(), source);
        }
        assert!(prepare_outgoing("😀 `<:a:123>` <:b:456>", |_| false).is_err());
        assert_eq!(prepare_outgoing(r"\<:a:123>", |_| false).unwrap(), r"\<:a:123>");
    }
    #[test]
    fn invalid_preview_does_not_become_animation() {
        let data = serde_json::json!([{"url":"https://tenor.com/view/a","gif_src":"https://media.tenor.com/a.mp4","preview":"https://media.tenor.com/a.webm"}]);
        let page = parse(&data, "trending", 20).unwrap();
        assert_eq!(page.items[0].thumbnail_url, "https://tenor.com/view/a");
        assert!(page.items[0].animation_url.is_empty());
    }
    #[test]
    fn accepted_remote_limits_and_display_cap() {
        assert!(endpoint("trending", "", 21).unwrap().ends_with("limit=50"));
        assert!(endpoint("trending", "", 96).unwrap().ends_with("limit=100"));
        let values = (0..100)
            .map(|n| serde_json::json!({"url":format!("https://tenor.com/view/{n}")}))
            .collect::<Vec<_>>();
        let page = parse(&serde_json::json!(values), "trending", 96).unwrap();
        assert_eq!(page.items.len(), 96);
        assert!(!page.has_more);
    }
    fn field(number: u8, data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![number << 3 | 2];
        let mut n = data.len() as u64;
        while n >= 128 {
            bytes.push((n as u8 & 127) | 128);
            n >>= 7
        }
        bytes.push(n as u8);
        bytes.extend_from_slice(data);
        bytes
    }
    #[test]
    fn legacy_favorite_proto_fields_and_order() {
        fn entry(id: &str, order: u8) -> Vec<u8> {
            let link = format!("https://tenor.com/view/{id}");
            let src = format!("https://media.tenor.com/{id}.gif");
            let mut metadata = field(2, src.as_bytes());
            metadata.extend_from_slice(&[0x18, 42, 0x20, 43, 0x28, order]);
            let mut map = field(1, link.as_bytes());
            map.extend(field(2, &metadata));
            field(1, &map)
        }
        let mut entries = entry("old", 1);
        entries.extend(entry("new", 9));
        let encoded = base64::engine::general_purpose::STANDARD.encode(field(2, &entries));
        let gifs = favorites(&encoded).unwrap();
        assert_eq!(gifs.len(), 2);
        assert!(gifs[0].url.ends_with("new"));
        assert_eq!(gifs[0].width, 42);
        assert!(gifs[0].animation_url.ends_with("new.gif"));
    }
    #[test]
    fn truncated_favorite_proto_fails_closed() {
        for bytes in [vec![0], vec![0x12, 80, 1], vec![0xff; 20]] {
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            assert!(favorites(&encoded).is_err())
        }
    }
}
