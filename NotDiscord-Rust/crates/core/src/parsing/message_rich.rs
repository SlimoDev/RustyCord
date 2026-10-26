//! Bounded presentation of embeds and message components. Interactive IDs,
//! interaction tokens and arbitrary JSON trees are deliberately not retained.
use super::*;

const EMBED_TEXT: usize = 6000;
const COMPONENT_TEXT: usize = 8000;
const LINK_BYTES: usize = 8192;

struct Budget { text: usize, links: usize }
impl Budget {
    fn new(text: usize) -> Self { Self { text, links: LINK_BYTES } }
    fn text(&mut self, value: &Value, key: &str, limit: usize) -> String {
        self.take(value.get(key).and_then(Value::as_str).unwrap_or(""), limit)
    }
    fn take(&mut self, value: &str, limit: usize) -> String {
        let text: String = value.chars().take(limit.min(self.text)).collect();
        self.text = self.text.saturating_sub(text.chars().count());
        text
    }
    fn link(&mut self, value: &Value, key: &str) -> String {
        let Some(raw) = value.get(key).and_then(Value::as_str) else { return String::new(); };
        if raw.len() > 2048 || raw.len() > self.links || raw.chars().any(char::is_control) { return String::new(); }
        if !url::Url::parse(raw).is_ok_and(|url| matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some() && url.username().is_empty() && url.password().is_none()) { return String::new(); }
        self.links -= raw.len(); raw.to_owned()
    }
}

pub fn rich_embeds(value: &Value) -> Vec<ChatEmbed> {
    let mut budget = Budget::new(EMBED_TEXT);
    let mut urls = HashSet::new();
    let mut out = Vec::new();
    for value in items(value).iter().take(10) {
        let raw_url = value.get("url").and_then(Value::as_str).unwrap_or("");
        // Discord deduplicates embed URLs. No URL means an independent card.
        if !raw_url.is_empty() && raw_url.len() <= 2048 && !urls.insert(raw_url) { continue; }
        let title = budget.text(value, "title", 256);
        let description = budget.text(value, "description", 4096);
        let author = budget.text(&value["author"], "name", 256);
        let provider = budget.text(&value["provider"], "name", 256);
        let footer = budget.text(&value["footer"], "text", 2048);
        let fields: Vec<_> = items(&value["fields"]).iter().take(25).map(|field| ChatEmbedField {
            name: budget.text(field, "name", 256), value: budget.text(field, "value", 1024), inline: b(field, "inline"),
        }).filter(|field| !field.name.is_empty() || !field.value.is_empty()).collect();
        // Image/GIF-only embeds stay on the existing deduplicated media path.
        if title.is_empty() && description.is_empty() && author.is_empty() && provider.is_empty() && footer.is_empty() && fields.is_empty() { continue; }
        out.push(ChatEmbed { title, description, author, provider, footer, fields,
            url: budget.link(value, "url"), author_url: budget.link(&value["author"], "url"),
            color: i(value, "color").clamp(0, 0xff_ffff) });
    }
    out
}

fn media_link(value: &Value, message: &Value, budget: &mut Budget) -> String {
    let raw = value.get("url").and_then(Value::as_str).unwrap_or("");
    if let Some(name) = raw.strip_prefix("attachment://") {
        if let Some(attachment) = items(&message["attachments"]).iter().take(50)
            .find(|attachment| attachment.get("filename").and_then(Value::as_str) == Some(name)) {
            return budget.link(attachment, "url");
        }
    }
    if value.get("proxy_url").and_then(Value::as_str).is_some_and(trusted_media) {
        return budget.link(value, "proxy_url");
    }
    budget.link(value, "url")
}
fn walk(value: &Value, message: &Value, depth: u8, from_embed: bool,
    seen: &mut usize, budget: &mut Budget, out: &mut Vec<ChatComponent>) {
    if depth > 4 || *seen >= 40 || out.len() >= 40 || !value.is_object() { return; }
    *seen += 1;
    let kind = i(value, "type");
    let mut component = ChatComponent { kind, depth, from_embed, spoiler: b(value, "spoiler"), color: i(value, "accent_color").clamp(0, 0xff_ffff), ..Default::default() };
    match kind {
        1 | 9 | 17 | 18 => {
            component.text = budget.text(value, "label", 256);
            if component.text.is_empty() { component.text = budget.text(value, "description", 1024); }
        }
        10 => component.text = budget.text(value, "content", 4000),
        2 => {
            component.text = budget.text(value, "label", 80);
            if component.text.is_empty() { component.text = budget.text(&value["emoji"], "name", 80); }
            // Link buttons open a browser only after a user click. Custom-ID
            // buttons remain visibly disabled and have no executable ID here.
            if i(value, "style") == 5 { component.url = budget.link(value, "url"); }
        }
        3 | 5..=8 | 19 | 21..=23 => {
            component.text = budget.text(value, "placeholder", 150);
            for option in items(&value["options"]).iter().take(25) {
                let label = budget.text(option, "label", 100);
                if !label.is_empty() { if !component.text.is_empty() { component.text.push('\n'); } component.text.push_str(&label); }
            }
        }
        11 | 13 => {
            let media = &value[if kind == 13 { "file" } else { "media" }];
            component.url = media_link(media, message, budget);
            component.text = budget.text(value, "description", 1024);
        }
        12 => {
            for item in items(&value["items"]).iter().take(10) {
                if out.len() >= 40 { break; }
                out.push(ChatComponent { kind: 12, url: media_link(&item["media"], message, budget),
                    text: budget.text(item, "description", 1024), depth, from_embed, spoiler: b(item, "spoiler"), ..Default::default() });
            }
        }
        14 => {}
        _ => component.text = budget.text(value, "content", 1000),
    }
    if kind != 12 { out.push(component); }
    for child in items(&value["components"]).iter().take(40) {
        walk(child, message, depth + 1, from_embed, seen, budget, out);
    }
    if let Some(accessory) = value.get("accessory") { walk(accessory, message, depth + 1, from_embed, seen, budget, out); }
    if let Some(child) = value.get("component") { walk(child, message, depth + 1, from_embed, seen, budget, out); }
}
pub fn message_components(value: &Value, message: &Value) -> Vec<ChatComponent> {
    let mut budget = Budget::new(COMPONENT_TEXT);
    let mut out = Vec::new();
    let mut seen = 0;
    for component in items(value).iter().take(40) { walk(component, message, 0, false, &mut seen, &mut budget, &mut out); }
    for embed in items(&message["embeds"]).iter().take(10) {
        for component in items(&embed["components"]).iter().take(40) { walk(component, message, 0, true, &mut seen, &mut budget, &mut out); }
    }
    bound_components(out)
}
pub(super) fn bound_components(mut components: Vec<ChatComponent>) -> Vec<ChatComponent> {
    components.truncate(40);
    let mut budget = Budget::new(COMPONENT_TEXT);
    for component in &mut components {
        component.text = budget.take(&component.text, 4000);
        if component.url.len() > budget.links { component.url.clear(); }
        budget.links = budget.links.saturating_sub(component.url.len());
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn bot_embed_only_message_retains_text_and_updates_without_losing_partial_fields() {
        let message = super::super::message(&json!({"id":"300","channel_id":"200","author":{"id":"100","username":"Dyno","bot":true},
            "content":"","embeds":[{"title":"Server information","description":"Welcome <@42>","color":5793266,
                "fields":[{"name":"Rules","value":"Be kind","inline":true}],"footer":{"text":"Automated notice"}}]}));
        assert!(message.author.is_bot); assert!(message.content.is_empty());
        assert_eq!(message.embeds[0].fields[0].value, "Be kind");
        let partial = update_message(&message, &json!({"id":"300","pinned":true}));
        assert_eq!(partial.embeds, message.embeds);
        let cleared = update_message(&partial, &json!({"id":"300","embeds":[]}));
        assert!(cleared.embeds.is_empty());
    }
    #[test]
    fn embed_and_component_budgets_reject_unsafe_links_and_deep_or_oversized_trees() {
        let huge = "😀".repeat(10000);
        let embeds = rich_embeds(&json!([{"title":huge,"description":huge,"url":"javascript:alert(1)","fields":[{"name":huge,"value":huge}]}]));
        let characters: usize = embeds.iter().map(|embed| embed.title.chars().count() + embed.description.chars().count()
            + embed.fields.iter().map(|field| field.name.chars().count()+field.value.chars().count()).sum::<usize>()).sum();
        assert!(characters <= 6000); assert!(embeds[0].url.is_empty());
        let mut tree = json!({"type":10,"content":"TOO DEEP"});
        for _ in 0..20 { tree = json!({"type":17,"components":[tree]}); }
        let parsed = message_components(&json!([tree]), &json!({}));
        assert!(parsed.len() <= 5); assert!(!parsed.iter().any(|component| component.text.contains("TOO DEEP")));
        let input = json!((0..100).map(|_| json!({"type":10,"content":huge})).collect::<Vec<_>>());
        let parsed = message_components(&input, &json!({}));
        assert!(parsed.len() <= 40); assert!(parsed.iter().map(|component| component.text.chars().count()).sum::<usize>() <= 8000);
    }
    #[test]
    fn components_v2_preserve_text_and_safe_link_but_never_interaction_ids() {
        let raw = json!({"id":"300","channel_id":"200","flags":32768,"author":{"id":"100","username":"App","bot":true},
            "components":[{"type":17,"components":[{"type":10,"content":"A real component message"},
                {"type":1,"components":[{"type":2,"label":"Approve","style":1,"custom_id":"private-interaction-id"},
                    {"type":2,"label":"Documentation","style":5,"url":"https://example.com/docs"}]}]}]});
        let message = super::super::message(&raw);
        assert!(message.components_v2);
        assert!(message.components.iter().any(|component| component.text == "A real component message"));
        assert!(message.components.iter().any(|component| component.text == "Documentation" && component.url == "https://example.com/docs"));
        assert!(!serde_json::to_string(&message.components).unwrap().contains("private-interaction-id"));
        let partial = update_message(&message, &json!({"id":"300","pinned":true}));
        assert_eq!(partial.components, message.components);
        assert!(update_message(&message, &json!({"id":"300","components":[]})).components.is_empty());
    }
}
