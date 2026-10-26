use crate::{
    api::ServiceError,
    models::{ChatMessage, NavItem, Person},
};
use chrono::{Datelike, NaiveDate};
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

const KEYS: &str = "from|has|in|before|after|during|mentions|pinned|author_type|author-type|author";
static FILTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?i)(?:^|\s)(?P<key>{KEYS}):\s*(?P<value>"[^"]*"|[^\s]+)"#
    ))
    .unwrap()
});
static MISSING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)(?:^|\s)({KEYS}):\s*$")).unwrap());
static ALIAS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)(^|\s):(?P<key>{KEYS})(:|\s|$)")).unwrap());
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

#[derive(Clone, Debug, Default)]
pub struct SearchContext {
    pub current_user_id: String,
    pub people: Vec<Person>,
    pub channels: Vec<NavItem>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedSearch {
    pub content: String,
    pub filters: Vec<(String, String)>,
    pub author_type: String,
}
impl ParsedSearch {
    pub fn query_string(&self) -> String {
        let mut pairs = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in &self.filters {
            pairs.append_pair(k, v);
        }
        if !self.content.is_empty() {
            pairs.append_pair("content", &self.content);
        }
        pairs.finish()
    }
    pub fn matches(&self, message: &ChatMessage) -> bool {
        match self.author_type.as_str() {
            "user" => !message.author.is_bot && message.webhook_id.is_empty(),
            "bot" => message.author.is_bot && message.webhook_id.is_empty(),
            "webhook" => !message.webhook_id.is_empty(),
            _ => true,
        }
    }
}
pub fn normalize_aliases(input: &str) -> String {
    ALIAS
        .replace_all(input, |c: &regex::Captures<'_>| {
            format!(
                "{}{}:",
                c.get(1).map(|v| v.as_str()).unwrap_or(""),
                &c["key"]
            )
        })
        .into_owned()
}
fn err(message: impl Into<String>) -> ServiceError {
    ServiceError::new(message)
}
pub fn parse(input: &str, context: &SearchContext) -> Result<ParsedSearch, ServiceError> {
    if input.trim().is_empty() || input.chars().count() > 2000 {
        return Err(err("Escribe una búsqueda de entre 1 y 2000 caracteres."));
    }
    let input = normalize_aliases(input);
    if MISSING.is_match(&input) {
        return Err(err(
            "Hay un filtro sin valor. Ejemplo: from:usuario has:image before:2026-10-01.",
        ));
    }
    let mut parsed = ParsedSearch::default();
    let mut unique = HashMap::new();
    let mut cursor = 0;
    let mut text = String::new();
    for captures in FILTER.captures_iter(&input) {
        let whole = captures.get(0).unwrap();
        text.push_str(&input[cursor..whole.start()]);
        text.push(' ');
        cursor = whole.end();
        let key = captures["key"].to_ascii_lowercase();
        let raw = &captures["value"];
        let value = if raw.starts_with('"') && raw.ends_with('"') && raw.len() > 1 {
            &raw[1..raw.len() - 1]
        } else {
            raw
        };
        if value.is_empty() || value.contains('"') {
            return Err(err(
                "El filtro contiene comillas incompletas o un valor vacío.",
            ));
        }
        let (api_key, api_value) = match key.as_str() {
            "from" | "mentions" => (
                if key == "from" {
                    "author_id"
                } else {
                    "mentions"
                },
                resolve(
                    value,
                    context
                        .people
                        .iter()
                        .take(5000)
                        .flat_map(|p| [(&p.id, &p.name), (&p.id, &p.username)]),
                    &context.current_user_id,
                    true,
                )?,
            ),
            "in" => (
                "channel_id",
                resolve(
                    value,
                    context.channels.iter().take(2000).map(|c| (&c.id, &c.name)),
                    "",
                    false,
                )?,
            ),
            "has" => {
                let mut v = value.to_ascii_lowercase();
                if v == "forward" {
                    v = "snapshot".into()
                }
                if !matches!(
                    v.as_str(),
                    "link"
                        | "embed"
                        | "file"
                        | "image"
                        | "video"
                        | "sound"
                        | "sticker"
                        | "poll"
                        | "snapshot"
                ) {
                    return Err(err(
                        "has: admite link, embed, file, image, video, sound, sticker, poll o forward.",
                    ));
                }
                ("has", v)
            }
            "author" | "author_type" | "author-type" => {
                let v = value.to_ascii_lowercase();
                if !matches!(v.as_str(), "user" | "bot" | "webhook") {
                    return Err(err("author: admite user, bot o webhook."));
                }
                if !parsed.author_type.is_empty() && parsed.author_type != v {
                    return Err(err("Hay filtros incompatibles de tipo de autor."));
                }
                parsed.author_type = v;
                continue;
            }
            "during" => {
                let (from, until) = during_dates(value)?;
                add_filter(
                    &mut parsed,
                    &mut unique,
                    "min_id",
                    date_snowflake(from)?,
                    &key,
                )?;
                add_filter(
                    &mut parsed,
                    &mut unique,
                    "max_id",
                    date_snowflake(until)?,
                    &key,
                )?;
                continue;
            }
            "pinned" => {
                let v = value.to_ascii_lowercase();
                if !matches!(v.as_str(), "true" | "false") {
                    return Err(err("Usa pinned:true o pinned:false."));
                }
                ("pinned", v)
            }
            _ => {
                let date =
                    parse_date(value).ok_or_else(|| err("Las fechas deben usar AAAA-MM-DD."))?;
                (
                    if key == "before" { "max_id" } else { "min_id" },
                    date_snowflake(date)?,
                )
            }
        };
        add_filter(&mut parsed, &mut unique, api_key, api_value, &key)?;
    }
    text.push_str(&input[cursor..]);
    if let (Some(min), Some(max)) = (unique.get("min_id"), unique.get("max_id")) {
        if min.parse::<u64>().unwrap_or(0) >= max.parse::<u64>().unwrap_or(0) {
            return Err(err("after: debe ser anterior a before:."));
        }
    }
    parsed.content = WHITESPACE.replace_all(&text, " ").trim().to_owned();
    Ok(parsed)
}
fn add_filter(
    parsed: &mut ParsedSearch,
    unique: &mut HashMap<String, String>,
    key: &str,
    value: String,
    name: &str,
) -> Result<(), ServiceError> {
    if matches!(key, "pinned" | "max_id" | "min_id") {
        if unique.get(key).is_some_and(|old| old != &value) {
            return Err(err(format!("Hay filtros incompatibles para {name}.")));
        }
        unique.insert(key.into(), value.clone());
    }
    let pair = (key.to_owned(), value);
    if !parsed.filters.contains(&pair) {
        parsed.filters.push(pair)
    }
    if parsed.filters.len() > 20 {
        return Err(err("Usa como máximo 20 filtros por búsqueda."));
    }
    Ok(())
}
fn parse_date(value: &str) -> Option<NaiveDate> {
    if value.len() != 10
        || value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
    {
        return None;
    }
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}
fn during_dates(value: &str) -> Result<(NaiveDate, NaiveDate), ServiceError> {
    let result = if value.len() == 10 {
        parse_date(value).and_then(|from| from.succ_opt().map(|until| (from, until)))
    } else if value.len() == 7 {
        parse_date(&format!("{value}-01")).and_then(|from| {
            from.checked_add_months(chrono::Months::new(1))
                .map(|until| (from, until))
        })
    } else if value.len() == 4 {
        parse_date(&format!("{value}-01-01")).and_then(|from| {
            NaiveDate::from_ymd_opt(from.year() + 1, 1, 1).map(|until| (from, until))
        })
    } else {
        None
    };
    result.ok_or_else(|| err("during: usa AAAA-MM-DD, AAAA-MM o AAAA."))
}
pub fn date_snowflake(date: NaiveDate) -> Result<String, ServiceError> {
    let millis = date
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis()
        - 1_420_070_400_000;
    if millis < 0 || millis > i64::MAX >> 22 {
        return Err(err(
            "La fecha está fuera del intervalo de búsqueda de Discord.",
        ));
    }
    Ok((millis << 22).to_string())
}
fn resolve<'a>(
    value: &str,
    known: impl Iterator<Item = (&'a String, &'a String)>,
    own: &str,
    person: bool,
) -> Result<String, ServiceError> {
    let mut value = value.trim();
    if person && value.eq_ignore_ascii_case("me") {
        value = own
    }
    if value.starts_with("<@") || value.starts_with("<#") {
        value = value.trim_matches(['<', '>', '@', '#', '!'])
    } else {
        value = value.trim_start_matches(if person { '@' } else { '#' })
    }
    if !value.is_empty() && value.len() <= 20 && value.bytes().all(|v| v.is_ascii_digit()) {
        return Ok(value.to_owned());
    }
    let ids: HashSet<_> = known
        .filter(|(_, name)| name.to_lowercase() == value.to_lowercase())
        .map(|(id, _)| id)
        .collect();
    if ids.len() != 1 {
        return Err(err(if ids.is_empty() {
            format!("No conozco «{value}». Usa su ID o un nombre visible en esta sesión.")
        } else {
            format!("Hay varios «{value}». Usa su ID para distinguirlos.")
        }));
    }
    Ok((*ids.into_iter().next().unwrap()).clone())
}

/// Caret and token ranges are UTF-8 byte offsets. Use `char_to_byte` for egui cursors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchToken {
    pub key: String,
    pub partial: String,
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSuggestion {
    pub key: String,
    pub value: String,
    pub title: String,
    pub description: String,
}
pub fn char_to_byte(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map(|(p, _)| p)
        .unwrap_or(text.len())
}
pub fn byte_to_char(text: &str, index: usize) -> usize {
    text[..safe_boundary(text, index)].chars().count()
}
fn safe_boundary(text: &str, index: usize) -> usize {
    let mut n = index.min(text.len());
    while !text.is_char_boundary(n) {
        n -= 1
    }
    n
}
pub fn at(input: &str, caret: usize) -> Option<SearchToken> {
    if input.chars().count() > 2000 {
        return None;
    }
    let caret = safe_boundary(input, caret);
    let before = &input[..caret];
    let mut starts = vec![0];
    starts.extend(
        before
            .char_indices()
            .filter(|(_, c)| c.is_whitespace())
            .map(|(i, c)| i + c.len_utf8()),
    );
    for start in starts.into_iter().rev() {
        let candidate = &before[start..];
        let normalized = normalize_aliases(candidate);
        let Some((key, partial)) = normalized.split_once(':') else {
            continue;
        };
        let key = key.to_ascii_lowercase();
        if !is_key(&key) {
            continue;
        }
        let partial = partial.trim_start();
        if !partial.starts_with('"') && partial.chars().any(char::is_whitespace) {
            continue;
        }
        let mut end = caret;
        let quoted = partial.starts_with('"') && (!partial.ends_with('"') || partial == "\"");
        for (pos, ch) in input[caret..].char_indices() {
            if !quoted && ch.is_whitespace() {
                break;
            }
            end = caret + pos + ch.len_utf8();
            if quoted && ch == '"' {
                break;
            }
        }
        return Some(SearchToken {
            key,
            partial: partial.trim_matches('"').into(),
            start,
            end,
        });
    }
    None
}
fn is_key(key: &str) -> bool {
    matches!(
        key,
        "from"
            | "has"
            | "mentions"
            | "in"
            | "before"
            | "after"
            | "during"
            | "pinned"
            | "author"
            | "author_type"
            | "author-type"
    )
}
fn suggestion(
    key: &str,
    value: impl Into<String>,
    title: impl Into<String>,
    description: impl Into<String>,
) -> SearchSuggestion {
    SearchSuggestion {
        key: key.into(),
        value: value.into(),
        title: title.into(),
        description: description.into(),
    }
}
pub fn suggest(
    input: &str,
    caret: usize,
    context: &SearchContext,
    today: Option<NaiveDate>,
) -> Vec<SearchSuggestion> {
    let Some(token) = at(input, caret) else {
        return [
            ("from", "De una persona", "from: usuario"),
            (
                "has",
                "Contiene un tipo de contenido",
                "has: enlace, imagen, archivo…",
            ),
            ("mentions", "Menciona a una persona", "mentions: usuario"),
            ("in", "En un canal", "in: canal"),
            ("before", "Antes de una fecha", "before: AAAA-MM-DD"),
            ("after", "Después de una fecha", "after: AAAA-MM-DD"),
            (
                "during",
                "Durante un día, mes o año",
                "during: AAAA-MM-DD · AAAA-MM · AAAA",
            ),
            ("pinned", "Mensajes fijados", "pinned: true o false"),
        ]
        .into_iter()
        .map(|(k, t, d)| suggestion(k, "", t, d))
        .collect();
    };
    let query = token.partial.to_lowercase();
    let matches = |value: &str| value.to_lowercase().contains(&query);
    let key = token.key.as_str();
    let mut values = Vec::new();
    match key {
        "from" | "mentions" => {
            if matches(&format!("me yo {}", context.current_user_id)) {
                values.push(suggestion(key, "me", "Yo", "Tu cuenta"))
            }
            let mut ids = HashSet::new();
            for p in context.people.iter().take(2000) {
                if !p.id.is_empty()
                    && ids.insert(&p.id)
                    && matches(&format!("{} {} {}", p.name, p.username, p.id))
                {
                    values.push(suggestion(
                        key,
                        &p.id,
                        &p.name,
                        if p.username.is_empty() {
                            "Persona conocida en esta sesión".into()
                        } else {
                            format!("@{}", p.username)
                        },
                    ))
                }
            }
        }
        "has" => {
            for (v, t) in [
                ("link", "Enlace"),
                ("image", "Imagen"),
                ("video", "Vídeo"),
                ("file", "Archivo"),
                ("embed", "Contenido incrustado"),
                ("sound", "Audio"),
                ("poll", "Encuesta"),
                ("forward", "Reenvío"),
                ("sticker", "Sticker"),
            ] {
                if matches(&format!("{v} {t}")) {
                    values.push(suggestion(key, v, t, format!("has:{v}")))
                }
            }
        }
        "in" => {
            let mut ids = HashSet::new();
            for c in context.channels.iter().take(2000) {
                if matches!(c.kind.as_str(), "text" | "thread")
                    && ids.insert(&c.id)
                    && matches(&format!("{} {}", c.name, c.id))
                {
                    values.push(suggestion(
                        key,
                        &c.id,
                        format!("# {}", c.name),
                        "Sólo este canal",
                    ))
                }
            }
        }
        "pinned" => {
            for (v, t) in [("true", "Sí, fijados"), ("false", "Sin fijar")] {
                if matches(&format!("{v} {t}")) {
                    values.push(suggestion(key, v, t, format!("pinned:{v}")))
                }
            }
        }
        "author" | "author_type" | "author-type" => {
            for (v, t) in [
                ("user", "Personas"),
                ("bot", "Bots"),
                ("webhook", "Webhooks"),
            ] {
                if matches(&format!("{v} {t}")) {
                    values.push(suggestion(key, v, t, "Filtro de resultados cargados"))
                }
            }
        }
        _ => {
            let date = today.unwrap_or_else(|| chrono::Local::now().date_naive());
            let day = date.format("%Y-%m-%d").to_string();
            values.push(suggestion(key, &day, "Hoy", &day));
            if key == "during" {
                values.push(suggestion(
                    key,
                    date.format("%Y-%m").to_string(),
                    "Este mes",
                    "AAAA-MM",
                ));
                values.push(suggestion(key, date.year().to_string(), "Este año", "AAAA"))
            } else if let Some(yesterday) = date.pred_opt() {
                let day = yesterday.format("%Y-%m-%d").to_string();
                values.push(suggestion(key, &day, "Ayer", &day))
            }
            values.retain(|v| matches(&format!("{} {}", v.value, v.title)))
        }
    }
    values.truncate(12);
    values
}
pub fn complete(input: &str, caret: usize, key: &str, value: &str) -> (String, usize) {
    let caret = safe_boundary(input, caret);
    if input.chars().count() > 2000 || !is_key(key) {
        return (input.into(), caret);
    }
    let token = at(input, caret).filter(|t| t.key == key);
    let start = token.as_ref().map(|t| t.start).unwrap_or(caret);
    let end = token.as_ref().map(|t| t.end).unwrap_or(caret);
    let before = &input[..start];
    let after = &input[end..];
    let encoded = if value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('"', ""))
    } else {
        value.into()
    };
    let mut insertion = format!(
        "{}{key}:{encoded}",
        if before.chars().last().is_some_and(|c| !c.is_whitespace()) {
            " "
        } else {
            ""
        }
    );
    if !value.is_empty() && !after.chars().next().is_some_and(char::is_whitespace) {
        insertion.push(' ')
    }
    let result = format!("{before}{insertion}{after}");
    if result.chars().count() > 2000 {
        (input.into(), caret)
    } else {
        (result, before.len() + insertion.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> SearchContext {
        SearchContext {
            current_user_id: "1".into(),
            people: vec![Person {
                id: "2".into(),
                name: "Bright".into(),
                username: "user".into(),
                ..Person::default()
            }],
            channels: vec![NavItem {
                id: "3".into(),
                name: "general".into(),
                kind: "text".into(),
                ..NavItem::default()
            }],
        }
    }
    #[test]
    fn combined_filters() {
        let p = parse(
            "from:Bright has:image in:general hello mentions:me pinned:true",
            &context(),
        )
        .unwrap();
        assert_eq!(p.content, "hello");
        assert!(p.filters.contains(&("author_id".into(), "2".into())));
        assert_eq!(p.filters.len(), 5)
    }
    #[test]
    fn alias_and_quotes() {
        assert_eq!(normalize_aliases(":from me has:image"), "from:me has:image");
        let p = parse(":from:Bright has:forward", &context()).unwrap();
        assert!(p.filters.contains(&("has".into(), "snapshot".into())))
    }
    #[test]
    fn invalid_dates_and_ranges() {
        for q in [
            "before:2014-01-01",
            "during:2026-13",
            "after:2026-10-20 before:2026-01-01",
            "has:unknown",
            "from:",
        ] {
            assert!(parse(q, &context()).is_err(), "{q}")
        }
    }
    #[test]
    fn during_year_month_day() {
        for q in ["during:2026", "during:2026-10", "during:2026-10-20"] {
            let p = parse(q, &context()).unwrap();
            assert_eq!(p.filters.len(), 2)
        }
    }
    #[test]
    fn names_ambiguous_rejected() {
        let mut c = context();
        c.people.push(Person {
            id: "9".into(),
            name: "Bright".into(),
            ..Person::default()
        });
        assert!(parse("from:Bright", &c).is_err())
    }
    #[test]
    fn unicode_caret_completion() {
        let text = "😀 from:Br";
        let choices = suggest(text, text.len(), &context(), None);
        assert_eq!(choices[0].value, "2");
        let (result, caret) = complete(text, text.len(), "from", "2");
        assert_eq!(result, "😀 from:2 ");
        assert_eq!(caret, result.len());
        assert_eq!(char_to_byte("😀abc", 1), 4);
        assert_eq!(byte_to_char("😀abc", 4), 1)
    }
    #[test]
    fn type_filter_stays_local() {
        let p = parse("author:bot hi", &context()).unwrap();
        assert_eq!(p.author_type, "bot");
        assert!(!p.query_string().contains("author"));
        assert!(!p.matches(&ChatMessage::default()))
    }
    #[test]
    fn selection_replaces_token_only() {
        let s = "from:Br has:image";
        let (result, _) = complete(s, 7, "from", "2");
        assert_eq!(result, "from:2 has:image")
    }
}
