use crate::models::*;
use serde_json::Value;
use std::collections::HashSet;
#[path = "parsing/message_rich.rs"]
mod message_rich;
pub use message_rich::{message_components, rich_embeds};
#[path = "parsing/gif_media.rs"]
mod gif_media;
pub use gif_media::{animation_rendition, direct_animation, provider_animation_source};

pub fn s(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}
pub fn i(value: &Value, key: &str) -> i32 {
    value
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or(0)
}
pub fn b(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}
pub fn items(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub fn limited(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
pub fn timestamp(value: &str) -> Option<Timestamp> {
    chrono::DateTime::parse_from_rfc3339(value).ok()
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 24 && id.bytes().all(|v| v.is_ascii_digit())
}
pub fn account_created_at(id: &str) -> Option<Timestamp> {
    let n = id.parse::<u64>().ok()?;
    chrono::DateTime::from_timestamp_millis(i64::try_from(n >> 22).ok()? + 1_420_070_400_000)
        .map(|v| v.fixed_offset())
}

pub fn person(value: &Value) -> Person {
    let id = s(value, "id");
    let avatar = s(value, "avatar");
    let username = s(value, "username");
    let display_name = s(value, "global_name");
    let name = if !display_name.is_empty() {
        display_name.clone()
    } else if !username.is_empty() {
        username.clone()
    } else {
        "Usuario".into()
    };
    let avatar_url = if !avatar.is_empty() {
        Some(format!(
            "https://cdn.discordapp.com/avatars/{id}/{avatar}.png?size=64"
        ))
    } else {
        id.parse::<u64>().ok().map(|n| {
            let d = s(value, "discriminator").parse::<u64>().unwrap_or(0);
            let index = if d > 0 { d % 5 } else { (n >> 22) % 6 };
            format!("https://cdn.discordapp.com/embed/avatars/{index}.png")
        })
    };
    Person {
        id,
        name,
        avatar_url,
        username,
        display_name,
        is_bot: b(value, "bot"),
    }
}
pub fn guild_icon(value: &Value) -> Option<String> {
    let hash = s(value, "icon");
    (!hash.is_empty()).then(|| {
        format!(
            "https://cdn.discordapp.com/icons/{}/{}.png?size=64",
            s(value, "id"),
            hash
        )
    })
}
pub fn guild(value: &Value) -> NavItem {
    let bits = crate::access::read_bits(value, "permissions");
    let admin = b(value, "owner") || bits & 8 != 0;
    NavItem {
        id: s(value, "id"),
        name: s(value, "name"),
        kind: "guild".into(),
        icon_url: guild_icon(value),
        can_invite: admin || bits & 1 != 0,
        can_manage: admin || bits & 32 != 0,
        ..NavItem::default()
    }
}
pub fn channel(value: &Value) -> NavItem {
    let kind = i(value, "type");
    let mut name = s(value, "name");
    let mut icon = None;
    let mut recipient_id = String::new();
    let people: Vec<_> = items(&value["recipients"])
        .iter()
        .take(100)
        .map(person)
        .collect();
    if name.is_empty() {
        name = people
            .iter()
            .take(6)
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
    let avatars = people
        .iter()
        .filter_map(|p| p.avatar_url.clone())
        .take(4)
        .collect();
    if kind == 1 {
        if let Some(p) = people.first() {
            recipient_id = p.id.clone();
            icon = p.avatar_url.clone()
        }
    }
    if kind == 3 && !s(value, "icon").is_empty() {
        icon = Some(format!(
            "https://cdn.discordapp.com/channel-icons/{}/{}.png?size=64",
            s(value, "id"),
            s(value, "icon")
        ))
    }
    NavItem {
        id: s(value, "id"),
        name: if name.is_empty() {
            "Conversación".into()
        } else {
            name
        },
        kind: match kind {
            1 => "dm",
            3 => "group",
            2 => "voice",
            13 => "stage",
            4 => "category",
            15 => "forum",
            10 | 11 | 12 => "thread",
            _ => "text",
        }
        .into(),
        parent_id: s(value, "parent_id"),
        icon_url: icon,
        last_message_id: s(value, "last_message_id"),
        message_preview: value
            .get("last_message")
            .filter(|message| message.is_object())
            .map(|value| message_preview(&message(value))),
        recipient_id,
        recipient_avatars: avatars,
        ..NavItem::default()
    }
}
/// Stable sidebar order: uncategorized text/voice, then categories and their
/// descendants. Positions belong to siblings, not to one global channel list.
pub fn ordered_guild_channels<'a>(values: impl IntoIterator<Item = &'a Value>) -> Vec<&'a Value> {
    let mut values: Vec<_> = values.into_iter().filter(|value| valid_id(&s(value, "id"))).take(2000).collect();
    values.sort_by(|a, b| {
        let group = |value: &Value| match i(value, "type") { 4 => 2, 2 | 13 => 1, _ => 0 };
        let aid = s(a, "id"); let bid = s(b, "id");
        (group(a), i(a, "position"), aid.len(), &aid).cmp(&(group(b), i(b, "position"), bid.len(), &bid))
    });
    let ids: std::collections::HashSet<_> = values.iter().map(|value| s(value, "id")).collect();
    let mut children = std::collections::HashMap::<String, Vec<&Value>>::new();
    for value in &values { children.entry(s(value, "parent_id")).or_default().push(*value); }
    fn append<'a>(value: &'a Value, children: &std::collections::HashMap<String, Vec<&'a Value>>,
        seen: &mut std::collections::HashSet<String>, result: &mut Vec<&'a Value>, depth: usize) {
        let id = s(value, "id");
        if !seen.insert(id.clone()) { return; }
        result.push(value);
        if depth < 16 {
            if let Some(items) = children.get(&id) { for child in items { append(child, children, seen, result, depth + 1); } }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::with_capacity(values.len());
    for value in &values {
        if !ids.contains(&s(value, "parent_id")) { append(value, &children, &mut seen, &mut result, 0); }
    }
    // Preserve every accessible item even if malformed parent cycles are sent.
    for value in values { append(value, &children, &mut seen, &mut result, 0); }
    result
}
pub fn relationship(value: &Value) -> Relationship {
    let kind = i(value, "type");
    Relationship {
        user: person(&value["user"]),
        kind,
        since: if kind == 1 {
            timestamp(&s(value, "since"))
        } else {
            None
        },
    }
}
pub fn emoji(value: &Value) -> StatusEmoji {
    StatusEmoji {
        name: limited(&s(value, "name"), 100),
        id: s(value, "id"),
        animated: b(value, "animated"),
    }
}
pub fn activity(value: &Value) -> PresenceActivity {
    let application_id = s(value, "application_id");
    let large = s(&value["assets"], "large_image");
    let large_image_url = if let Some(rest) = large.strip_prefix("spotify:") {
        format!("https://i.scdn.co/image/{rest}")
    } else if !application_id.is_empty()
        && !large.is_empty()
        && large.bytes().all(|v| v.is_ascii_digit())
    {
        format!("https://cdn.discordapp.com/app-assets/{application_id}/{large}.png?size=128")
    } else {
        String::new()
    };
    let time = |key: &str| {
        value["timestamps"][key]
            .as_i64()
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|v| v.fixed_offset())
    };
    PresenceActivity {
        kind: i(value, "type"),
        name: limited(&s(value, "name"), 160),
        details: limited(&s(value, "details"), 300),
        state: limited(&s(value, "state"), 300),
        emoji: value.get("emoji").filter(|v| v.is_object()).map(emoji),
        application_id,
        large_image_url,
        started_at: time("start"),
        ends_at: time("end"),
    }
}
pub fn presence(value: &Value) -> UserPresence {
    let status = s(value, "status");
    let status = if matches!(
        status.as_str(),
        "online" | "idle" | "dnd" | "offline" | "invisible"
    ) {
        status
    } else {
        String::new()
    };
    let activities: Vec<_> = items(&value["activities"])
        .iter()
        .filter(|v| v.is_object())
        .take(8)
        .map(activity)
        .collect();
    let custom = activities.iter().find(|a| a.kind == 4);
    UserPresence {
        status,
        custom_status: custom.map(|a| a.state.clone()).unwrap_or_default(),
        custom_status_emoji: custom.and_then(|a| a.emoji.clone()),
        activities: activities.into_iter().filter(|a| a.kind != 4).collect(),
    }
}
pub fn presence_summary(activities: &[PresenceActivity]) -> String {
    let Some(a) = activities.first() else {
        return String::new();
    };
    limited(
        &match a.kind {
            0 => format!("Jugando a {}", playing_activity_label(a)),
            1 => format!("Transmitiendo {}", a.name),
            2 => format!(
                "Escuchando {}",
                if a.details.is_empty() {
                    &a.name
                } else {
                    &a.details
                }
            ),
            3 => format!("Viendo {}", a.name),
            5 => format!("Compitiendo en {}", a.name),
            _ => a.name.clone(),
        },
        220,
    )
}
pub fn compact_activity(activities: &[PresenceActivity]) -> String {
    let Some(a) = activities.first() else {
        return String::new();
    };
    // Layout decides the visible width; tooltips retain the complete title.
    match a.kind {
        2 => {
            if a.details.is_empty() {
                a.name.clone()
            } else {
                a.details.clone()
            }
        }
        0 => playing_activity_label(a),
        _ => a.name.clone(),
    }
}
fn playing_activity_label(activity: &PresenceActivity) -> String {
    if activity.details.trim().is_empty() || activity.details.eq_ignore_ascii_case(&activity.name) {
        activity.name.clone()
    } else {
        limited(&format!("{} · {}", activity.name, activity.details), 300)
    }
}
pub fn profile_details(
    value: &Value,
    fallback: &ProfileDetails,
    guilds: &[NavItem],
    people: &[Person],
) -> ProfileDetails {
    let mut p = fallback.clone();
    if value.get("user").is_some_and(Value::is_object) {
        let user=&value["user"];
        let id=s(user,"id");
        // A partial profile must not erase the identity already observed in
        // the conversation, nor merge another user's response into it.
        if !p.user.id.is_empty() && !id.is_empty() && id!=p.user.id {return p;}
        let resolved=person(user);
        if p.user.id.is_empty() {p.user.id=id;}
        if user.get("username").is_some_and(Value::is_string) && !resolved.username.is_empty() {p.user.username=resolved.username;}
        if user.get("global_name").is_some() {p.user.display_name=resolved.display_name;}
        if user.get("avatar").is_some() {p.user.avatar_url=resolved.avatar_url;}
        if let Some(bot)=user.get("bot").and_then(Value::as_bool) {p.user.is_bot=bot;}
        if !p.user.display_name.is_empty() {p.user.name=p.user.display_name.clone();}
        else if !p.user.username.is_empty() {p.user.name=p.user.username.clone();}
        else if p.user.name.is_empty() {p.user.name="Usuario".into();}
    }
    p.created_at = account_created_at(&p.user.id);
    if let Some(bio)=value["user_profile"].get("bio").or_else(||value["user"].get("bio"))
        .or_else(||p.user.is_bot.then(||value["application"].get("description")).flatten()) {
        p.bio=limited(bio.as_str().unwrap_or(""),4000);
    }
    if value.get("mutual_friends").is_some() {
    p.mutual_friends_available = value["mutual_friends"].is_array();
    p.mutual_friends = items(&value["mutual_friends"])
        .iter()
        .take(1000)
        .filter_map(|v| {
            if v.is_object() {
                Some(person(v.get("user").unwrap_or(v)))
            } else {
                v.as_str()
                    .and_then(|id| people.iter().find(|p| p.id == id).cloned())
            }
        })
        .filter(|p| !p.id.is_empty())
        .collect();
    }
    if value.get("mutual_guilds").is_some() {
    p.mutual_guilds_available = value["mutual_guilds"].is_array();
    p.mutual_guilds = items(&value["mutual_guilds"])
        .iter()
        .take(500)
        .filter_map(|v| {
            let id = if let Some(id) = v.as_str() {
                id.to_owned()
            } else {
                s(v, "id")
            };
            if id.is_empty() {
                return None;
            }
            let known = guilds.iter().find(|g| g.id == id);
            let icon = guild_icon(v).or_else(|| known.and_then(|g| g.icon_url.clone()));
            let name = s(v, "name");
            Some(NavItem {
                id,
                name: if name.is_empty() {
                    known
                        .map(|g| g.name.clone())
                        .unwrap_or_else(|| "Servidor".into())
                } else {
                    name
                },
                kind: "guild".into(),
                icon_url: icon,
                ..NavItem::default()
            })
        })
        .collect();
    }
    p
}

pub fn safe_media_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| {
        value.len() <= 4096
            && u.scheme() == "https"
            && u.port_or_known_default() == Some(443)
            && u.username().is_empty()
            && u.password().is_none()
    })
}
pub fn is_media_host(host: &str) -> bool {
    if matches!(
        host,
        "static.klipy.com"
            | "static1.klipy.com"
            | "static2.klipy.com"
            | "media.tenor.co"
            | "c.tenor.com"
            | "i.giphy.com"
    ) {
        return true;
    }
    [".tenor.com", ".giphy.com"].iter().any(|suffix| {
        host.strip_suffix(suffix).is_some_and(|prefix| {
            prefix
                .strip_prefix("media")
                .is_some_and(|n| n.len() <= 2 && n.bytes().all(|c| c.is_ascii_digit()))
        })
    })
}
pub fn gif_page(value: &str) -> bool {
    if !safe_media_url(value) {
        return false;
    }
    let u = url::Url::parse(value).unwrap();
    matches!(u.host_str(), Some("tenor.com" | "www.tenor.com")) && u.path().starts_with("/view/")
        || matches!(
            u.host_str(),
            Some("giphy.com" | "www.giphy.com" | "klipy.com" | "www.klipy.com")
        ) && u.path().starts_with("/gifs/")
}
pub fn direct_gif(value: &str) -> bool {
    if !safe_media_url(value) {
        return false;
    }
    let u = url::Url::parse(value).unwrap();
    let Some(host) = u.host_str() else {
        return false;
    };
    (is_media_host(host) || matches!(host, "cdn.discordapp.com" | "media.discordapp.net") || discord_external_proxy(&u))
        && u.path().to_ascii_lowercase().ends_with(".gif")
        && !u
            .query_pairs()
            .any(|(k, v)| k.eq_ignore_ascii_case("format") && !v.eq_ignore_ascii_case("gif"))
}
pub fn trusted_media(value: &str) -> bool {
    safe_media_url(value)
        && url::Url::parse(value).is_ok_and(|u| {
            u.host_str().is_some_and(|h| {
                is_media_host(h) || matches!(h, "cdn.discordapp.com" | "media.discordapp.net") || discord_external_proxy(&u)
            })
        })
}
pub fn discord_external_proxy(url: &url::Url) -> bool {
    matches!(url.host_str(), Some("images-ext-1.discordapp.net" | "images-ext-2.discordapp.net"))
        && url.path().starts_with("/external/")
}
/// Only Discord attachment signatures are omitted. Transformation/crop queries,
/// names, attachment IDs and all third-party queries remain part of identity.
pub fn media_identity(value: &str) -> Option<String> {
    if !trusted_media(value) && !gif_page(value) { return None; }
    let mut url = url::Url::parse(value).ok()?;
    if discord_external_proxy(&url) {
        if let Some((_, source)) = url.path().split_once("/https/") {
            let mut original = url::Url::parse(&format!("https://{source}")).ok()?;
            original.set_query(url.query());
            if trusted_media(original.as_str()) && !discord_external_proxy(&original) { url = original; }
        }
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    if matches!(url.host_str(), Some("cdn.discordapp.com" | "media.discordapp.net"))
        && segments.len() == 4 && segments[0] == "attachments" && valid_id(segments[1])
        && valid_id(segments[2]) && !segments[3].is_empty() {
        let mut query = url.query_pairs().filter(|(key, _)| !["ex", "is", "hm"].iter().any(|auth| key.eq_ignore_ascii_case(auth)))
            .map(|(key, value)| (key.into_owned(), value.into_owned())).collect::<Vec<_>>();
        query.sort();
        url.set_host(Some("cdn.discordapp.com")).ok()?;
        url.set_query(None);
        if !query.is_empty() { url.query_pairs_mut().extend_pairs(query); }
    }
    url.set_fragment(None);
    Some(url.into())
}
pub fn same_media_resource(left: &str, right: &str) -> bool {
    !left.is_empty() && (left == right || media_identity(left).zip(media_identity(right)).is_some_and(|(a, b)| a == b))
}
/// One logical medium and its metadata-proven download aliases. A public page,
/// poster and animation rendition are different URLs, not three UI cards.
/// Provider query strings are retained; only media_identity's known Discord
/// attachment identity can differ from a signed download URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaDescriptor {
    pub logical_id: String,
    pub page_url: Option<String>,
    pub poster_url: Option<String>,
    pub animation_url: Option<String>,
    pub width: i32,
    pub height: i32,
    pub aliases: Vec<String>,
}
impl MediaDescriptor {
    pub fn matches_url(&self, candidate: &str) -> bool {
        self.aliases.iter().any(|alias| same_media_resource(alias, candidate))
    }
    pub fn same_medium(&self, other: &Self) -> bool {
        if let (Some(left),Some(right))=(&self.page_url,&other.page_url){
            return same_media_resource(left,right)||self.animation_url.as_deref().zip(other.animation_url.as_deref()).is_some_and(|(left,right)|same_media_resource(left,right));
        }
        other.aliases.iter().any(|alias| self.matches_url(alias))
    }
}
pub fn media_descriptor(file: &ChatAttachment) -> MediaDescriptor {
    let mut aliases = Vec::with_capacity(3);
    for value in [&file.url].into_iter().chain(file.preview_url.as_ref()).chain(file.animation_url.as_ref()) {
        if !value.is_empty() && !aliases.iter().any(|known: &String| same_media_resource(known, value)) {
            aliases.push(value.clone());
        }
    }
    let page_url = gif_page(&file.url).then(|| file.url.clone());
    let identity = page_url.as_deref().unwrap_or(&file.url);
    MediaDescriptor {
        logical_id: media_identity(identity).unwrap_or_else(|| identity.into()),
        page_url,
        poster_url: file.preview_url.clone(),
        animation_url: file.animation_url.clone(),
        width: file.width,
        height: file.height,
        aliases,
    }
}
fn signed_media_expiry(value: &str) -> Option<u64> {
    let url = url::Url::parse(value).ok()?;
    if !matches!(url.host_str(), Some("cdn.discordapp.com" | "media.discordapp.net")) || !url.path().starts_with("/attachments/") { return None; }
    url.query_pairs().find(|(key, _)| key == "ex").and_then(|(_, value)| u64::from_str_radix(&value, 16).ok())
}
pub fn prefer_media_url<'a>(current: &'a str, candidate: &'a str) -> &'a str {
    if !same_media_resource(current, candidate) { return current; }
    let now = chrono::Utc::now().timestamp().max(0) as u64;
    let score = |url| { let expiry = signed_media_expiry(url); (expiry.is_none_or(|until| until > now), expiry.unwrap_or(0)) };
    if score(candidate) > score(current) { candidate } else { current }
}
pub fn deduplicate_media(files: Vec<ChatAttachment>) -> Vec<ChatAttachment> {
    let mut result: Vec<ChatAttachment> = Vec::new();
    for file in files {
        let descriptor = media_descriptor(&file);
        let same = result.iter_mut().find(|old| media_descriptor(old).same_medium(&descriptor));
        if let Some(old) = same {
            old.url = prefer_media_url(&old.url, &file.url).to_owned();
            if let Some(preview) = &file.preview_url {
                old.preview_url = Some(old.preview_url.as_ref().map(|current| prefer_media_url(current, preview)).unwrap_or(preview).to_owned());
            }
            if let Some(animation)=&file.animation_url{
                old.animation_url=Some(old.animation_url.as_ref().map(|current|prefer_media_url(current,animation)).unwrap_or(animation).to_owned());
            }
            if let Some(preview) = &old.preview_url {
                if direct_gif(&old.url) && direct_gif(preview) { old.url = prefer_media_url(&old.url, preview).to_owned(); }
            }
            old.animated |= file.animated;
            if old.width <= 0 { old.width = file.width; }
            if old.height <= 0 { old.height = file.height; }
        } else { result.push(file); }
    }
    result
}
pub fn attachments(value: &Value) -> Vec<ChatAttachment> {
    items(value)
        .iter()
        .take(20)
        .map(|v| {
            let original = s(v, "url");
            let proxy = s(v, "proxy_url");
            let animated = direct_gif(&original) || direct_gif(&proxy);
            let mut chosen = if animated {
                if direct_gif(&original) {
                    original
                } else {
                    proxy.clone()
                }
            } else if !proxy.is_empty() {
                proxy.clone()
            } else {
                original
            };
            if animated && direct_gif(&proxy) { chosen = prefer_media_url(&chosen, &proxy).to_owned(); }
            let ct = s(v, "content_type");
            let ct = if animated {
                "image/gif".into()
            } else if ct.eq_ignore_ascii_case("image/gif") {
                "image/unknown".into()
            } else {
                ct
            };
            ChatAttachment {
                id: s(v, "id"),
                name: s(v, "filename"),
                url: chosen.clone(),
                content_type: Some(ct),
                width: i(v, "width"),
                height: i(v, "height"),
                size: v["size"].as_i64().unwrap_or(0),
                animated,
                preview_url: (!proxy.is_empty() && proxy != chosen).then_some(proxy),
                animation_url: animated.then_some(chosen),
            }
        })
        .collect()
}
pub fn embeds(value: &Value) -> Vec<ChatAttachment> {
    let mut out = Vec::new();
    for v in items(value).iter().take(10) {
        let image = &v["image"];
        let thumb = &v["thumbnail"];
        let page = s(v, "url");
        let gif_context = gif_page(&page) || s(v,"type")=="gifv";
        let mut preview = s(thumb, "proxy_url");
        if preview.is_empty() {
            preview = s(thumb, "url")
        }
        let direct = [
            provider_animation_source(v).unwrap_or_default(),
            s(image, "url"),
            s(image, "proxy_url"),
            s(&v["video"], "url"),
            s(&v["video"], "proxy_url"),
        ]
        .into_iter()
        .filter(|v| direct_gif(v) || gif_context && animation_rendition(v)).reduce(|current, candidate| prefer_media_url(&current, &candidate).to_owned());
        if gif_page(&page) || direct.is_some() {
            let is_page = gif_page(&page);
            let poster = if trusted_media(&preview){preview}else{direct.clone().unwrap_or_default()};
            let animation_url=direct.clone();
            out.push(ChatAttachment {
                id: format!("embed-{}", out.len()),
                name: "GIF".into(),
                url: if is_page { page } else { direct.unwrap() },
                content_type: Some("image/gif".into()),
                width: i(if is_page { thumb } else { image }, "width"),
                height: i(if is_page { thumb } else { image }, "height"),
                animated: true,
                preview_url: trusted_media(&poster).then_some(poster),
                animation_url,
                ..ChatAttachment::default()
            });
            continue;
        }
        for source in [image, thumb] {
            let mut url = s(source, "proxy_url");
            if !trusted_media(&url) {
                url = s(source, "url")
            }
            if !trusted_media(&url) || out.iter().any(|a| a.url == url) {
                continue;
            }
            out.push(ChatAttachment {
                id: format!("embed-{}", out.len()),
                name: "Imagen".into(),
                url,
                content_type: Some("image/png".into()),
                width: i(source, "width"),
                height: i(source, "height"),
                ..ChatAttachment::default()
            })
        }
    }
    deduplicate_media(out)
}
pub fn message_mentions(value: &Value) -> Vec<Person> {
    let mut ids = HashSet::new();
    items(&value["mentions"])
        .iter()
        .take(100)
        .filter_map(|v| {
            let mut p = person(v);
            if !valid_id(&p.id) || !ids.insert(p.id.clone()) {
                return None;
            }
            let nick = s(&v["member"], "nick");
            if !nick.is_empty() {
                p.name = nick
            }
            p.name = limited(&p.name, 128);
            p.username = limited(&p.username, 128);
            p.display_name = limited(&p.display_name, 128);
            Some(p)
        })
        .collect()
}
pub fn reply_content(content: &str, files: bool, embeds: bool) -> String {
    if !content.is_empty() {
        limited(content, 500)
    } else if files {
        "Archivo adjunto".into()
    } else if embeds {
        "Contenido incrustado".into()
    } else {
        "Mensaje sin texto".into()
    }
}
/// Sticker items are present on messages; legacy full sticker objects use the
/// same identity/name. Keep only real, bounded metadata, never a fixed shortcode.
pub fn message_stickers(value: &Value) -> Vec<ChatSticker> {
    let mut result: Vec<ChatSticker> = Vec::new();
    for item in items(&value["sticker_items"])
        .iter()
        .chain(items(&value["stickers"]).iter())
        .take(20)
    {
        let id = s(item, "id");
        if !valid_id(&id) {
            continue;
        }
        let name = s(item, "name")
            .chars()
            .filter(|ch| !ch.is_control())
            .take(128)
            .collect::<String>();
        if let Some(existing) = result.iter_mut().find(|sticker| sticker.id == id) {
            if existing.name.is_empty() {
                existing.name = name;
            }
            if existing.format_type == 0 {
                existing.format_type = i(item, "format_type");
            }
        } else if result.len() < 10 {
            result.push(ChatSticker {
                id,
                name,
                format_type: i(item, "format_type"),
            });
        }
    }
    result
}
/// Summary for conversation, notification and reply surfaces. Chat continues
/// to render the original content and the actual forwarding author separately.
pub fn message_preview(message: &ChatMessage) -> String {
    if message.forwarded {
        "Reenvió un mensaje".into()
    } else if !message.content.trim().is_empty() {
        limited(&message.content, 500)
    } else if !message.stickers.is_empty() {
        message
            .stickers
            .iter()
            .map(ChatSticker::code)
            .collect::<Vec<_>>()
            .join(" ")
    } else if let Some(text) = message.embeds.iter().flat_map(|embed|
        [&embed.title, &embed.description].into_iter().chain(embed.fields.iter().map(|field| &field.value)))
        .chain(message.components.iter().filter(|component| component.kind == 10).map(|component| &component.text))
        .find(|text| !text.trim().is_empty()) {
        limited(text, 500)
    } else {
        reply_content("", !message.attachments.is_empty(), false)
    }
}
fn value_preview(value: &Value) -> String {
    if i(&value["message_reference"], "type") == 1 {
        return "Reenvió un mensaje".into();
    }
    if value.get("content").and_then(Value::as_str).is_none_or(|text| text.trim().is_empty()) {
        let embeds = rich_embeds(&value["embeds"]);
        let components = message_components(&value["components"], value);
        if let Some(text) = embeds.iter().flat_map(|embed| [&embed.title, &embed.description])
            .chain(components.iter().filter(|component| component.kind == 10).map(|component| &component.text))
            .find(|text| !text.trim().is_empty()) { return limited(text, 500); }
    }
    let stickers = message_stickers(value);
    if s(value, "content").trim().is_empty() && !stickers.is_empty() {
        stickers
            .iter()
            .map(ChatSticker::code)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        reply_content(
            &s(value, "content"),
            !items(&value["attachments"]).is_empty(),
            !items(&value["embeds"]).is_empty(),
        )
    }
}
pub fn reply_preview(
    value: &Value,
    fallback_id: &str,
    fallback_channel: &str,
) -> Option<MessageReplyPreview> {
    let reference = value.get("message_reference");
    if reference.is_some_and(|r| !r.is_object() || i(r, "type") == 1) {
        return None;
    }
    let id = reference
        .map(|r| s(r, "message_id"))
        .unwrap_or_else(|| fallback_id.into());
    if !valid_id(&id) {
        return None;
    }
    let channel = s(value, "channel_id");
    let channel = if channel.is_empty() {
        fallback_channel.into()
    } else {
        channel
    };
    let ref_channel = reference.map(|r| s(r, "channel_id")).unwrap_or_default();
    if !ref_channel.is_empty() && ref_channel != channel {
        return Some(MessageReplyPreview {
            message_id: id,
            channel_id: ref_channel,
            unavailable: true,
            ..MessageReplyPreview::default()
        });
    }
    let Some(source) = value.get("referenced_message") else {
        return Some(MessageReplyPreview {
            message_id: id,
            channel_id: channel,
            unavailable: true,
            ..MessageReplyPreview::default()
        });
    };
    if source.is_null() {
        return Some(MessageReplyPreview {
            message_id: id,
            channel_id: channel,
            deleted: true,
            ..MessageReplyPreview::default()
        });
    }
    let source_channel = s(source, "channel_id");
    if !source.is_object()
        || s(source, "id") != id
        || !source_channel.is_empty() && source_channel != channel
    {
        return Some(MessageReplyPreview {
            message_id: id,
            channel_id: channel,
            unavailable: true,
            ..MessageReplyPreview::default()
        });
    }
    Some(MessageReplyPreview {
        message_id: id,
        channel_id: channel,
        author: source.get("author").filter(|v| v.is_object()).map(person),
        content: value_preview(source),
        mentions: message_mentions(source),
        mention_games: crate::game_mentions::parse(&source["mention_games"]),
        ..MessageReplyPreview::default()
    })
}
pub fn reactions(value: &Value) -> Vec<MessageReaction> {
    items(value)
        .iter()
        .take(100)
        .filter(|v| v.get("emoji").is_some_and(Value::is_object))
        .filter_map(|v| {
            let mut e = emoji(&v["emoji"]);
            if !valid_id(&e.id) {
                e.id.clear()
            }
            if !e.id.is_empty() && e.name.is_empty() {
                e.name = "emoji".into()
            }
            let count = i(v, "count").clamp(0, 1_000_000);
            (count > 0 && !e.name.is_empty()).then(|| MessageReaction {
                emoji: e,
                count,
                me: b(v, "me") || b(v, "me_burst"),
                normal_me: Some(b(v, "me")),
                burst_me: b(v, "me_burst"),
            })
        })
        .collect()
}
fn call_metadata(mut m: ChatMessage, call: &Value) -> ChatMessage {
    // Only explicit remote outcomes can distinguish a rejected call from one
    // the recipient simply did not answer. Participants alone cannot do that.
    if let Some(outcome) = call.get("status").and_then(Value::as_str).filter(|value| {
        matches!(
            *value,
            "declined" | "rejected" | "missed" | "answered" | "ended"
        )
    }) {
        m.call_outcome = outcome.into();
    }
    if call.get("ended_timestamp").is_some() {
        m.call_ended_at = timestamp(&s(call, "ended_timestamp"))
    }
    if call.get("participants").is_some() {
        m.call_participants_known = true;
        m.call_participant_ids = items(&call["participants"])
            .iter()
            .filter_map(Value::as_str)
            .take(100)
            .map(String::from)
            .collect()
    }
    let started = m.call_started_at.unwrap_or(m.timestamp);
    m.call_started_at = Some(started);
    m.message_type = 3;
    m.content = if m.call_ended_at.is_some() {
        if m.call_participant_ids.is_empty() {
            "Llamada finalizada sin participantes"
        } else {
            "Llamada finalizada"
        }
    } else {
        "Llamada iniciada"
    }
    .into();
    if let Some(end) = m.call_ended_at {
        let elapsed = end - started;
        if elapsed.num_seconds() >= 0 {
            let secs = elapsed.num_seconds();
            m.content.push_str(&if secs >= 3600 {
                format!(" · {}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
            } else {
                format!(" · {}:{:02}", secs / 60, secs % 60)
            })
        }
    }
    m
}
pub fn message(value: &Value) -> ChatMessage {
    let forwarded = i(&value["message_reference"], "type") == 1;
    let display = if forwarded {
        items(&value["message_snapshots"])
            .first()
            .and_then(|v| v.get("message"))
            .filter(|v| v.is_object())
            .unwrap_or(value)
    } else {
        value
    };
    let mut files = attachments(&display["attachments"]);
    files.extend(embeds(&display["embeds"]));
    files = deduplicate_media(files);
    files.truncate(50);
    let reference = s(&value["message_reference"], "message_id");
    let mut m = ChatMessage {
        id: s(value, "id"),
        channel_id: s(value, "channel_id"),
        author: person(&value["author"]),
        content: s(display, "content"),
        timestamp: timestamp(&s(value, "timestamp")).unwrap_or_default(),
        attachments: files,
        embeds: rich_embeds(&display["embeds"]),
        components: message_components(&display["components"], display),
        components_v2: display.get("flags").and_then(Value::as_u64).is_some_and(|flags| flags & (1 << 15) != 0),
        stickers: message_stickers(display),
        edited: !s(value, "edited_timestamp").is_empty(),
        mentions_everyone: b(display, "mention_everyone"),
        reply_to: (!forwarded && valid_id(&reference)).then_some(reference),
        message_type: i(value, "type"),
        pinned: b(value, "pinned"),
        forwarded,
        webhook_id: s(value, "webhook_id"),
        mentions: message_mentions(display),
        mention_games: crate::game_mentions::parse(&display["mention_games"]),
        reply_preview: reply_preview(value, "", ""),
        reactions: reactions(&value["reactions"]),
        ..ChatMessage::default()
    };
    if m.message_type == 3 {
        return call_metadata(m, &value["call"]);
    }
    if m.content.is_empty() {
        m.content = match m.message_type {
            1 => "Añadió participantes al grupo",
            2 => "Salió un participante del grupo",
            4 => "Cambió el nombre del grupo",
            5 => "Cambió la imagen del grupo",
            6 => "Fijó un mensaje",
            7 => "Se unió al servidor",
            _ => "",
        }
        .into()
    }
    m
}
pub fn update_message(previous: &ChatMessage, value: &Value) -> ChatMessage {
    let mut m = previous.clone();
    if let Some(everyone) = value.get("mention_everyone").and_then(Value::as_bool) {
        m.mentions_everyone = everyone;
    }
    let forwarded = value
        .get("message_reference")
        .map(|reference| i(reference, "type") == 1)
        .unwrap_or(previous.forwarded);
    let snapshot = forwarded
        .then(|| {
            items(&value["message_snapshots"])
                .first()
                .and_then(|snapshot| snapshot.get("message"))
                .filter(|message| message.is_object())
        })
        .flatten();
    let display = snapshot.unwrap_or(value);
    if let Some(content) = display
        .get("content")
        .and_then(Value::as_str)
        .filter(|_| !forwarded || snapshot.is_some())
    {
        if m.message_type != 3 && content != previous.content {
            if m.previous_contents.len() >= 8 {
                m.previous_contents.remove(0);
            }
            // Discord contents are bounded. Bound malformed fixtures too.
            m.previous_contents.push(previous.content.chars().take(4000).collect());
        }
        m.content = content.into();
        m.edited |= m.message_type != 3 && content != previous.content
    }
    m.edited |= !s(value, "edited_timestamp").is_empty();
    let update_files = !forwarded || snapshot.is_some();
    let mut files = if update_files && display.get("attachments").is_some() {
        attachments(&display["attachments"])
    } else {
        previous
            .attachments
            .iter()
            .filter(|a| !a.id.starts_with("embed-"))
            .cloned()
            .collect()
    };
    files.extend(if update_files && display.get("embeds").is_some() {
        embeds(&display["embeds"])
    } else {
        previous
            .attachments
            .iter()
            .filter(|a| a.id.starts_with("embed-"))
            .cloned()
            .collect()
    });
    files = deduplicate_media(files);
    files.truncate(50);
    m.attachments = files;
    if update_files && display.get("embeds").is_some() {
        m.embeds = rich_embeds(&display["embeds"]);
    }
    if update_files && (display.get("components").is_some() || display.get("embeds").is_some()) {
        let mut components: Vec<_> = previous.components.iter().filter(|component|
            !component.from_embed && display.get("components").is_none()).cloned().collect();
        components.extend(message_components(&display["components"], display));
        components.extend(previous.components.iter().filter(|component|
            component.from_embed && display.get("embeds").is_none()).cloned());
        m.components = message_rich::bound_components(components);
    }
    if update_files && display.get("flags").is_some() {
        m.components_v2 = display.get("flags").and_then(Value::as_u64).is_some_and(|flags| flags & (1 << 15) != 0);
    }
    if update_files && (display.get("sticker_items").is_some() || display.get("stickers").is_some())
    {
        m.stickers = message_stickers(display);
    }
    if let Some(p) = value.get("pinned").and_then(Value::as_bool) {
        m.pinned = p
    }
    if value.get("reactions").is_some() {
        m.reactions = reactions(&value["reactions"])
    }
    if update_files && display.get("mentions").is_some() {
        m.mentions = message_mentions(display)
    }
    if update_files && display.get("mention_games").is_some() {
        m.mention_games=crate::game_mentions::parse(&display["mention_games"]);
    }
    if let Some(reference) = value.get("message_reference") {
        if !reference.is_object() || i(reference, "type") == 1 {
            m.reply_to = None;
            m.reply_preview = None;
            m.forwarded = reference.is_object() && i(reference, "type") == 1
        } else {
            let id = s(reference, "message_id");
            m.forwarded = false;
            if valid_id(&id) {
                m.reply_to = Some(id)
            } else {
                m.reply_to = None;
                m.reply_preview = None
            }
        }
    }
    if value.get("message_reference").is_some() || value.get("referenced_message").is_some() {
        if let Some(id) = m.reply_to.as_ref() {
            let preview = reply_preview(value, id, &m.channel_id);
            if value.get("referenced_message").is_some()
                || previous.reply_to != m.reply_to
                || previous.reply_preview.is_none()
            {
                m.reply_preview = preview
            }
        }
    }
    if value.get("call").is_some() {
        m = call_metadata(m, &value["call"])
    }
    m
}
pub fn context_messages(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    if !messages.iter().any(|message| {
        message.reply_to.is_some()
            && !message.reply_preview.as_ref().is_some_and(|preview| {
                preview.deleted || preview.channel_id != message.channel_id
            })
    }) {
        return messages;
    }
    let lookup: std::collections::HashMap<_, _> =
        messages.iter().enumerate().map(|(index, message)| (message.id.clone(), index)).collect();
    for index in 0..messages.len() {
        let m = &messages[index];
        if m.reply_preview
            .as_ref()
            .is_some_and(|r| r.deleted || r.channel_id != m.channel_id)
        {
            continue;
        }
        if let Some(original) = m.reply_to.as_ref().and_then(|id| lookup.get(id)).map(|index| &messages[*index]) {
            let preview = MessageReplyPreview {
                message_id: original.id.clone(),
                channel_id: original.channel_id.clone(),
                author: Some(original.author.clone()),
                content: message_preview(original),
                deleted: original.deleted,
                mentions: original.mentions.clone(),
                mention_games: original.mention_games.clone(),
                ..MessageReplyPreview::default()
            };
            messages[index].reply_preview = Some(preview);
        }
    }
    messages
}
pub fn order_dms(items: &mut [NavItem]) {
    items.sort_by(|a, b| {
        b.has_active_call
            .cmp(&a.has_active_call)
            .then(b.incoming_call_ringing.cmp(&a.incoming_call_ringing))
            .then(b.is_pinned.cmp(&a.is_pinned))
            .then(
                b.last_message_id
                    .parse::<u64>()
                    .unwrap_or(0)
                    .cmp(&a.last_message_id.parse().unwrap_or(0)),
            )
            .then(
                b.id.parse::<u64>()
                    .unwrap_or(0)
                    .cmp(&a.id.parse().unwrap_or(0)),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn playing_rich_presence_keeps_the_game_detail_in_members_and_compact_labels() {
        let rpc=activity(&json!({"type":0,"name":"RPCS3","application_id":"424004941485572097",
            "details":"Synthetic PS3 game","state":"Playing","timestamps":{"start":1700000000000i64}}));
        assert_eq!(compact_activity(&[rpc.clone()]),"RPCS3 · Synthetic PS3 game");
        assert_eq!(presence_summary(&[rpc.clone()]),"Jugando a RPCS3 · Synthetic PS3 game");
        assert_eq!(rpc.started_at.unwrap().timestamp(),1700000000);
        let duplicate=PresenceActivity {name:"RPCS3".into(),details:"rpcs3".into(),..Default::default()};
        assert_eq!(compact_activity(&[duplicate]),"RPCS3");
    }
    #[test]
    fn sidebar_order_groups_siblings_and_preserves_orphans_cycles_and_all_channel_types() {
        let values = vec![json!({"id":"40","type":4,"position":1}),
            json!({"id":"50","type":4,"position":0}),json!({"id":"41","type":2,"parent_id":"40","position":0}),
            json!({"id":"42","type":0,"parent_id":"40","position":1}),json!({"id":"51","type":15,"parent_id":"50","position":0}),
            json!({"id":"52","type":11,"parent_id":"51","position":0}),json!({"id":"30","type":2,"position":0}),
            json!({"id":"20","type":0,"position":0}),json!({"id":"21","type":5,"position":0}),
            json!({"id":"60","type":13,"parent_id":"50","position":1}),json!({"id":"70","type":16,"parent_id":"999","position":2}),
            json!({"id":"80","type":4,"parent_id":"81"}),json!({"id":"81","type":4,"parent_id":"80"})];
        let ordered = ordered_guild_channels(&values).iter().map(|value| s(value,"id")).collect::<Vec<_>>();
        assert_eq!(ordered, ["20","21","70","30","50","51","52","60","40","42","41","80","81"]);
        let mut reverse = values.clone(); reverse.reverse();
        assert_eq!(ordered_guild_channels(&reverse).iter().map(|value| s(value,"id")).collect::<Vec<_>>(), ordered);
        assert_eq!(ordered.len(), values.len());
    }
    #[test]
    fn signed_attachment_aliases_share_identity_but_distinct_files_and_crops_do_not() {
        let old="https://media.discordapp.net/attachments/10/20/laundry.gif?ex=1&is=2&hm=old";
        let fresh="https://cdn.discordapp.com/attachments/10/20/laundry.gif?hm=new&ex=ffffffff&is=3";
        assert!(same_media_resource(old,fresh));
        assert_eq!(prefer_media_url(old,fresh),fresh);
        for other in ["https://cdn.discordapp.com/attachments/10/21/laundry.gif?ex=ffffffff",
            "https://cdn.discordapp.com/attachments/10/20/another.gif?ex=ffffffff",
            "https://cdn.discordapp.com/attachments/10/20/laundry.gif?ex=ffffffff&crop=0,0,50,50",
            "https://cdn.discordapp.com/attachments/10/20/laundry.gif?ex=ffffffff&format=webp"] {
            assert!(!same_media_resource(old,other),"{other}");
        }
        assert!(!same_media_resource("",""));
    }
    #[test]
    fn message_attachment_and_embed_use_one_fresh_gif_variant() {
        let old="https://cdn.discordapp.com/attachments/10/20/laundry.gif?ex=1&is=2&hm=old";
        let fresh="https://media.discordapp.net/attachments/10/20/laundry.gif?ex=ffffffff&is=3&hm=new";
        let parsed=message(&json!({"id":"30","channel_id":"10","content":old,
            "attachments":[{"id":"20","filename":"laundry.gif","url":old,"proxy_url":fresh,"width":64,"height":48}],
            "embeds":[{"url":old,"image":{"url":old,"proxy_url":fresh,"width":64,"height":48},"thumbnail":{"url":fresh}}]}));
        assert_eq!(parsed.attachments.len(),1);
        assert_eq!(parsed.attachments[0].id,"20");
        assert_eq!(parsed.attachments[0].url,fresh);
        assert!(parsed.attachments[0].animated);
    }
    #[test]
    fn discord_external_proxy_validation_keeps_host_and_path_boundaries() {
        let proxy="https://images-ext-1.discordapp.net/external/synthetic/https/cdn.discordapp.com/attachments/10/20/laundry.gif?ex=ffffffff";
        assert!(trusted_media(proxy) && direct_gif(proxy));
        assert!(same_media_resource(proxy,"https://cdn.discordapp.com/attachments/10/20/laundry.gif?ex=1"));
        for blocked in ["https://images-ext-1.discordapp.net.evil.example/external/synthetic/a.gif",
            "https://images-ext-1.discordapp.net/other/a.gif",
            "https://user:password@images-ext-1.discordapp.net/external/synthetic/a.gif"] {
            assert!(!trusted_media(blocked));
        }
    }
    #[test]
    fn edits_retain_only_observed_bounded_versions_and_metadata_updates_do_not_invent_them() {
        let initial = message(&serde_json::json!({"id":"1","channel_id":"2","content":"original","edited_timestamp":"2026-10-22T01:00:00Z"}));
        assert!(initial.edited && initial.previous_contents.is_empty());
        let unchanged = update_message(&initial, &serde_json::json!({"pinned":true}));
        assert!(unchanged.previous_contents.is_empty());
        let mut changed = update_message(&unchanged, &serde_json::json!({"content":"new"}));
        assert_eq!(changed.previous_contents, vec!["original"]);
        for index in 0..20 { changed = update_message(&changed, &serde_json::json!({"content":format!("revision {index}")})); }
        assert_eq!(changed.previous_contents.len(), 8);
        assert_eq!(changed.previous_contents[0], "revision 11");
        assert_eq!(changed.previous_contents[7], "revision 18");
        assert_eq!(changed.content, "revision 19");
    }
    #[test]
    fn everyone_metadata_is_preserved_until_an_explicit_update() {
        let original = message(&serde_json::json!({"id":"1","content":"@everyone","mention_everyone":true}));
        assert!(original.mentions_everyone);
        assert!(update_message(&original, &serde_json::json!({"pinned":true})).mentions_everyone);
        assert!(!update_message(&original, &serde_json::json!({"mention_everyone":false})).mentions_everyone);
    }
    #[test]
    fn contextual_replies_keep_edits_chains_deleted_and_external_previews_with_synthetic_ids() {
        let original = ChatMessage { id:"source:one".into(), channel_id:"preview".into(), content:"Edited source 日本語🙂".into(), author:Person{id:"person:one".into(),name:"Author".into(),..Default::default()}, edited:true,..Default::default() };
        let reply = ChatMessage { id:"reply:two".into(), channel_id:"preview".into(), content:"Reply body".into(), reply_to:Some(original.id.clone()),..Default::default() };
        let chain = ChatMessage { id:"chain:three".into(), channel_id:"preview".into(), reply_to:Some(reply.id.clone()),..Default::default() };
        let deleted = ChatMessage { id:"deleted:four".into(), channel_id:"preview".into(), reply_to:Some(original.id.clone()), reply_preview:Some(MessageReplyPreview{message_id:original.id.clone(),channel_id:"preview".into(),deleted:true,..Default::default()}),..Default::default() };
        let external = ChatMessage { id:"external:five".into(), channel_id:"preview".into(), reply_to:Some(original.id.clone()), reply_preview:Some(MessageReplyPreview{message_id:original.id.clone(),channel_id:"other-preview".into(),unavailable:true,..Default::default()}),..Default::default() };
        let messages = context_messages(vec![original,reply,chain,deleted,external]);
        assert_eq!(messages[1].reply_preview.as_ref().unwrap().content,"Edited source 日本語🙂");
        assert_eq!(messages[1].reply_preview.as_ref().unwrap().author.as_ref().unwrap().id,"person:one");
        assert_eq!(messages[2].reply_preview.as_ref().unwrap().content,"Reply body");
        assert!(messages[3].reply_preview.as_ref().unwrap().deleted);
        assert_eq!(messages[4].reply_preview.as_ref().unwrap().channel_id,"other-preview");
        assert!(messages[4].reply_preview.as_ref().unwrap().unavailable);
        assert!(messages[0].edited);
    }
    use serde_json::json;
    #[test]
    fn real_sticker_names_survive_parsing_partial_updates_and_reply_previews() {
        let first = message(
            &json!({"id":"100","channel_id":"20","sticker_items":[{"id":"301","name":"wave","format_type":3},{"id":"302","name":"Saludo feliz","format_type":1}]}),
        );
        assert_eq!(first.content, "");
        assert_eq!(message_preview(&first), ":wave: :Saludo feliz:");
        assert_eq!(first.stickers[0].format_type, 3);
        let unchanged = update_message(&first, &json!({"pinned":true}));
        assert_eq!(unchanged.stickers, first.stickers);
        let updated = update_message(
            &unchanged,
            &json!({"sticker_items":[{"id":"303","name":"hug","format_type":4}]}),
        );
        assert_eq!(message_preview(&updated), ":hug:");
        assert!(update_message(&updated, &json!({"sticker_items":[]}))
            .stickers
            .is_empty());
        let reply=reply_preview(&json!({"channel_id":"20","message_reference":{"message_id":"100"},"referenced_message":{"id":"100","sticker_items":[{"id":"301","name":"wave"}]}}),"","").unwrap();
        assert_eq!(reply.content, ":wave:");
        let cached = context_messages(vec![
            first,
            message(
                &json!({"id":"101","channel_id":"20","message_reference":{"message_id":"100"}}),
            ),
        ]);
        assert_eq!(
            cached[1].reply_preview.as_ref().unwrap().content,
            ":wave: :Saludo feliz:"
        );
    }
    #[test]
    fn missing_sticker_name_uses_actual_id_and_legacy_items_merge_without_duplicates() {
        let parsed = message(
            &json!({"stickers":[{"id":"301","name":"Actual legacy name"},{"id":"302"}],"sticker_items":[{"id":"301","name":"","format_type":3},{"id":"invalid","name":"Not real"}]}),
        );
        assert_eq!(parsed.stickers.len(), 2);
        assert_eq!(
            message_preview(&parsed),
            ":Actual legacy name: :sticker_302:"
        );
        assert!(
            serde_json::from_value::<ChatMessage>(json!({"Stickers":null}))
                .unwrap()
                .stickers
                .is_empty()
        );
    }
    #[test]
    fn forwarded_preview_is_exact_and_chat_preserves_original_content_and_forwarder() {
        let forward = json!({"id":"100","channel_id":"20","author":{"id":"2","username":"Forwarder"},"message_reference":{"type":1,"message_id":"90"},"message_snapshots":[{"message":{"content":"Original accessible text","author":{"id":"3","username":"Original"},"sticker_items":[{"id":"301","name":"wave"}]}}]});
        let parsed = message(&forward);
        assert_eq!(parsed.author.id, "2");
        assert_eq!(parsed.content, "Original accessible text");
        assert_eq!(parsed.stickers[0].code(), ":wave:");
        assert_eq!(message_preview(&parsed), "Reenvió un mensaje");
        let partial = update_message(&parsed, &json!({"content":"","pinned":true}));
        assert_eq!(partial.content, parsed.content);
        assert_eq!(partial.stickers, parsed.stickers);
        let reply=reply_preview(&json!({"channel_id":"20","message_reference":{"message_id":"100"},"referenced_message":forward}),"","").unwrap();
        assert_eq!(reply.author.as_ref().unwrap().id, "2");
        assert_eq!(reply.content, "Reenvió un mensaje");
        let conversation = channel(&json!({"id":"20","type":1,"last_message":forward}));
        assert_eq!(
            conversation.message_preview.as_deref(),
            Some("Reenvió un mensaje")
        );
    }
    #[test]
    fn gif_embed_retains_direct_animation_over_static_thumbnail_without_losing_page_url() {
        let page = "https://tenor.com/view/synthetic-123";
        let animation = "https://media.tenor.com/synthetic/animation.gif";
        let parsed = embeds(
            &json!([{"url":page,"image":{"url":animation},"thumbnail":{"url":"https://media.tenor.com/synthetic/poster.png","width":320,"height":180}}]),
        );
        assert_eq!(parsed[0].url, page);
        assert_eq!(parsed[0].preview_url.as_deref(), Some("https://media.tenor.com/synthetic/poster.png"));
        assert_eq!(parsed[0].animation_url.as_deref(), Some(animation));
        assert!(parsed[0].animated);
    }
    #[test]
    fn default_avatar_and_display() {
        let p = person(&json!({"id":"428314160380641304","username":"u","global_name":"Nombre"}));
        assert_eq!(p.name, "Nombre");
        assert!(p.avatar_url.unwrap().contains("embed/avatars/"))
    }
    #[test]
    fn group_icons() {
        let c = channel(&json!({"id":"1","type":3,"icon":"a"}));
        assert_eq!(
            c.icon_url.as_deref(),
            Some("https://cdn.discordapp.com/channel-icons/1/a.png?size=64")
        )
    }
    #[test]
    fn forward_uses_snapshot() {
        let m = message(
            &json!({"id":"1","channel_id":"2","author":{"id":"3"},"content":"","message_reference":{"type":1,"message_id":"8"},"message_snapshots":[{"message":{"content":"original"}}]}),
        );
        assert!(m.forwarded);
        assert_eq!(m.content, "original");
        assert!(m.reply_to.is_none())
    }
    #[test]
    fn reply_deleted_and_cross_channel() {
        let p=reply_preview(&json!({"channel_id":"1","message_reference":{"message_id":"4"},"referenced_message":null}),"","").unwrap();
        assert!(p.deleted);
        let p = reply_preview(
            &json!({"channel_id":"1","message_reference":{"message_id":"4","channel_id":"2"}}),
            "",
            "",
        )
        .unwrap();
        assert!(p.unavailable);
        assert_eq!(p.channel_id, "2")
    }
    #[test]
    fn partial_update_preserves_body() {
        let m = message(
            &json!({"id":"1","channel_id":"2","content":"old","message_reference":{"message_id":"3"},"referenced_message":{"id":"3","content":"preview"}}),
        );
        let u = update_message(
            &m,
            &json!({"message_reference":{"message_id":"3"},"pinned":true}),
        );
        assert_eq!(u.content, "old");
        assert_eq!(u.reply_preview.unwrap().content, "preview");
        assert!(u.pinned)
    }
    #[test]
    fn gif_format_validation() {
        assert!(direct_gif("https://media.tenor.com/a.gif"));
        assert!(!direct_gif("https://media.tenor.com/a.gif?format=webp"));
        assert!(!direct_gif("https://example.com/a.gif"));
        assert!(gif_page("https://tenor.com/view/123"));
        assert!(!safe_media_url("https://x@y.com/a"))
    }
    #[test]
    fn profile_unavailable_not_empty_success() {
        let f = ProfileDetails::default();
        let p = profile_details(&json!({"user":{"id":"1"}}), &f, &[], &[]);
        assert!(!p.mutual_friends_available && !p.mutual_guilds_available)
    }
    #[test]
    fn partial_bot_profiles_keep_observed_identity_and_explicit_fields_update_only_their_user() {
        let fallback=ProfileDetails {user:Person {id:"200".into(),name:"Bot visible".into(),username:"bot_user".into(),display_name:"Bot visible".into(),is_bot:true,avatar_url:Some("https://cdn.discordapp.com/avatars/200/known.png".into())},
            bio:"Known bio".into(),mutual_guilds_available:true,mutual_guilds:vec![NavItem{id:"500".into(),name:"Known guild".into(),..Default::default()}],..Default::default()};
        let partial=profile_details(&json!({"user":{"id":"200"}}),&fallback,&[],&[]);
        assert_eq!(partial.user,fallback.user);assert_eq!(partial.bio,"Known bio");assert_eq!(partial.mutual_guilds,fallback.mutual_guilds);
        let described=profile_details(&json!({"user":{"id":"200"},"application":{"description":"Description supplied with this profile"}}),&fallback,&[],&[]);
        assert_eq!(described.bio,"Description supplied with this profile");
        let changed=profile_details(&json!({"user":{"id":"200","global_name":null,"username":"renamed","bot":true,"avatar":null},"user_profile":{"bio":""},"mutual_guilds":null}),&fallback,&[],&[]);
        assert_eq!(changed.user.name,"renamed");assert!(changed.user.is_bot);assert_ne!(changed.user.avatar_url,fallback.user.avatar_url);assert!(changed.bio.is_empty());assert!(!changed.mutual_guilds_available&&changed.mutual_guilds.is_empty());
        let unrelated=profile_details(&json!({"user":{"id":"999","username":"Wrong user"},"user_profile":{"bio":"Other bio"}}),&fallback,&[],&[]);
        assert_eq!(unrelated,fallback);
    }
    #[test]
    fn custom_presence_separated() {
        let p = presence(
            &json!({"status":"online","activities":[{"type":4,"state":"hola","emoji":{"name":"😀"}},{"type":2,"details":"song","name":"Spotify"}]}),
        );
        assert_eq!(p.custom_status, "hola");
        assert_eq!(p.activities.len(), 1)
    }
    #[test]
    fn call_duration() {
        let m = message(
            &json!({"type":3,"timestamp":"2026-10-20T10:00:00Z","call":{"participants":["1"],"ended_timestamp":"2026-10-20T11:02:03Z"}}),
        );
        assert!(m.content.ends_with("1:02:03"))
    }
    #[test]
    fn own_burst_flags_preserved() {
        let r = reactions(&json!([{"emoji":{"name":"😀"},"count":2,"me":false,"me_burst":true}]));
        assert!(r[0].me);
        assert_eq!(r[0].normal_me, Some(false));
        assert!(r[0].burst_me)
    }
}
