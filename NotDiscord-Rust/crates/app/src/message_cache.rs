use notdiscord_core::models::{ChatMessage, MessageWindow, MessageWindowMode, Person};
use std::{collections::HashMap, sync::Mutex};

#[derive(Default)]
struct Window {
    messages: Vec<ChatMessage>,
    bytes: usize,
    touch: u64,
    cursor: MessageWindow,
}
#[derive(Default)]
struct CacheState {
    channels: HashMap<String, Window>,
    clock: u64,
    revision: u64,
    active: String,
}
/// Global baseline limits: 3 channels, 200 messages, 4 MiB estimated UTF-16
/// text/metadata. This estimate must not be presented as process memory.
#[derive(Default)]
pub struct MessageCache {
    state: Mutex<CacheState>,
}
impl MessageCache {
    /// Keep the visible interval stable even when background channels receive
    /// enough delivery to rotate the small LRU.
    pub fn set_active(&mut self, channel: &str) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).active = channel.into();
    }
    /// Read one message without duplicating the entire channel window. A point
    /// lookup keeps the same LRU semantics as reading that window.
    pub fn find(&self, channel: &str, id: &str) -> Option<ChatMessage> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.clock = state.clock.wrapping_add(1);
        let touch = state.clock;
        state.channels.get_mut(channel).and_then(|window| {
            window.touch = touch;
            window
                .messages
                .iter()
                .find(|message| message.id == id)
                .cloned()
        })
    }
    pub fn get(&self, id: &str) -> Vec<ChatMessage> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.clock = state.clock.wrapping_add(1);
        let touch = state.clock;
        if let Some(window) = state.channels.get_mut(id) {
            window.touch = touch;
            window.messages.clone()
        } else {
            Vec::new()
        }
    }
    pub fn window(&self, channel: &str) -> MessageWindow {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).channels.get(channel)
            .map(|window| window.cursor.clone())
            .unwrap_or_else(|| MessageWindow { channel_id: channel.into(), ..Default::default() })
    }
    pub fn replace(&mut self, id: &str, messages: Vec<ChatMessage>) {
        self.replace_window(id, messages, MessageWindowMode::Latest, false);
    }
    /// Normalize/sort once per REST page, then enforce the global budget once.
    pub fn replace_window(&mut self, id: &str, messages: Vec<ChatMessage>, mode: MessageWindowMode, has_newer: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        replace_window(&mut state, id, messages, mode, has_newer);
    }
    /// Freeze the live frontier before requesting an older page. New delivery
    /// can then signal newer rows without changing this request's cursor.
    pub fn begin_history(&mut self, channel: &str) {
        if let Some(window) = self.state.lock().unwrap_or_else(|e| e.into_inner()).channels.get_mut(channel) {
            window.cursor.mode = MessageWindowMode::History;
        }
    }
    pub fn prepend(&mut self, channel: &str, mut page: Vec<ChatMessage>) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (has_newer, contiguous) = state.channels.get(channel)
            .map(|window| (window.cursor.has_newer, window.cursor.contiguous)).unwrap_or((true, true));
        if let Some(window) = state.channels.remove(channel) {
            // Existing gateway updates win over overlapping REST payloads.
            page.extend(window.messages);
        }
        replace_window(&mut state, channel, page, MessageWindowMode::History, has_newer);
        if let Some(window) = state.channels.get_mut(channel) { window.cursor.contiguous &= contiguous; }
    }
    pub fn upsert(&mut self, msg: ChatMessage) {
        cache_upsert(
            &mut self.state.lock().unwrap_or_else(|e| e.into_inner()),
            msg,
        )
    }
    /// Mutate only the matching row. No full-window clone/reinsert is needed
    /// for edits, pins, deletes, or reaction counters.
    pub fn update(&mut self, channel: &str, id: &str, update: impl FnOnce(&mut ChatMessage)) -> Option<ChatMessage> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let window = state.channels.get_mut(channel)?;
        let index = window.messages.iter().position(|message| message.id == id)?;
        let before = estimate(&window.messages[index]);
        update(&mut window.messages[index]);
        let after = estimate(&window.messages[index]);
        window.bytes = window.bytes.saturating_sub(before).saturating_add(after);
        let result = window.messages[index].clone();
        if after > 4 * 1024 * 1024 {
            window.messages.remove(index);
            window.bytes = window.bytes.saturating_sub(after);
            window.cursor.contiguous = false;
        }
        changed(&mut state, channel);
        enforce_limits(&mut state, channel);
        Some(result)
    }
    pub fn mark_reply_deleted(&mut self, channel: &str, id: &str) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(window) = state.channels.get_mut(channel) else { return; };
        let mut updated = false;
        for reply in &mut window.messages {
            if reply.reply_to.as_deref() == Some(id)
                && reply.reply_preview.as_ref().is_none_or(|preview| preview.channel_id == channel) {
                let before = estimate(reply);
                reply.reply_preview = Some(notdiscord_core::models::MessageReplyPreview {
                    message_id: id.into(), channel_id: channel.into(), deleted: true, ..Default::default()
                });
                window.bytes = window.bytes.saturating_sub(before).saturating_add(estimate(reply));
                updated = true;
            }
        }
        if updated { changed(&mut state, channel); enforce_limits(&mut state, channel); }
    }
    pub fn remove(&mut self, channel: &str, id: &str) {
        cache_remove(
            &mut self.state.lock().unwrap_or_else(|e| e.into_inner()),
            channel,
            id,
        )
    }
    pub fn clear(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.channels.clear();
        state.active.clear();
    }
    pub fn count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .channels
            .values()
            .map(|w| w.messages.len())
            .sum()
    }
    pub fn channel_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .channels
            .len()
    }
    pub fn estimated_text_bytes(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .channels
            .values()
            .map(|w| w.bytes)
            .sum()
    }
}
fn cache_remove(state: &mut CacheState, channel: &str, id: &str) {
    if let Some(w) = state.channels.get_mut(channel) {
        if let Some(index) = w.messages.iter().position(|m| m.id == id) {
            w.bytes = w.bytes.saturating_sub(estimate(&w.messages.remove(index)));
            w.cursor.contiguous = false;
        }
        if w.messages.is_empty() {
            state.channels.remove(channel);
        }
    }
    changed(state, channel);
}
fn newer_id(a: &str, b: &str) -> bool { a.len().cmp(&b.len()).then(a.cmp(b)).is_gt() }
fn message_order(a: &ChatMessage, b: &ChatMessage) -> std::cmp::Ordering {
    a.timestamp.cmp(&b.timestamp).then(a.id.len().cmp(&b.id.len())).then(a.id.cmp(&b.id))
}
fn changed(state: &mut CacheState, channel: &str) {
    state.revision = state.revision.wrapping_add(1).max(1);
    state.clock = state.clock.wrapping_add(1);
    if let Some(window) = state.channels.get_mut(channel) {
        window.touch = state.clock;
        window.cursor.channel_id = channel.into();
        window.cursor.oldest_id = window.messages.first().map(|message| message.id.clone()).unwrap_or_default();
        window.cursor.newest_id = window.messages.last().map(|message| message.id.clone()).unwrap_or_default();
        window.cursor.revision = state.revision;
    }
}
fn replace_window(state: &mut CacheState, channel: &str, messages: Vec<ChatMessage>, mode: MessageWindowMode, has_newer: bool) {
    let mut unique = HashMap::with_capacity(messages.len());
    let mut contiguous = true;
    for message in messages.into_iter().filter(|message| message.channel_id == channel) {
        if estimate(&message) <= 4 * 1024 * 1024 { unique.insert(message.id.clone(), message); }
        else { contiguous = false; }
    }
    let mut messages = unique.into_values().collect::<Vec<_>>();
    messages.sort_by(message_order);
    let bytes = messages.iter().map(estimate).sum();
    state.channels.insert(channel.into(), Window { messages, bytes, touch: 0,
        cursor: MessageWindow { mode, has_newer, contiguous, ..Default::default() } });
    changed(state, channel);
    enforce_limits(state, channel);
}
fn cache_upsert(state: &mut CacheState, message: ChatMessage) {
    let cost = estimate(&message);
    let channel = message.channel_id.clone();
    if cost > 4 * 1024 * 1024 {
        cache_remove(state, &channel, &message.id);
        return;
    }
    let w = state.channels.entry(channel.clone()).or_default();
    if let Some(index) = w.messages.iter().position(|m| m.id == message.id) {
        w.bytes = w.bytes.saturating_sub(estimate(&w.messages.remove(index)))
    } else if !w.messages.is_empty() && (w.cursor.mode != MessageWindowMode::Latest
        || !w.cursor.contiguous || !newer_id(&message.id, &w.cursor.newest_id)) {
        // Never join a historical interval to an unrelated live/reaction row.
        if newer_id(&message.id, &w.cursor.newest_id) { w.cursor.has_newer = true; }
        return;
    }
    let index = w
        .messages
        .binary_search_by(|m| message_order(m, &message))
        .unwrap_or_else(|i| i);
    w.messages.insert(index, message);
    w.bytes = w.bytes.saturating_add(cost);
    changed(state, &channel);
    enforce_limits(state, &channel);
}
fn enforce_limits(state: &mut CacheState, channel: &str) {
    let protected = if state.channels.contains_key(&state.active) { state.active.clone() } else { channel.into() };
    while state.channels.len() > 3 {
        if let Some(old) = state
            .channels
            .iter()
            .filter(|(id, _)| **id != protected)
            .min_by_key(|(_, w)| w.touch)
            .map(|(k, _)| k.clone())
        {
            state.channels.remove(&old);
        }
    }
    while state
        .channels
        .values()
        .map(|w| w.messages.len())
        .sum::<usize>()
        > 200
        || state.channels.values().map(|w| w.bytes).sum::<usize>() > 4 * 1024 * 1024
    {
        let Some(old) = state
            .channels
            .iter()
            .filter(|(id, _)| state.channels.len() == 1 || **id != protected)
            .min_by_key(|(_, w)| w.touch)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        if state.channels.len() > 1 && old != protected {
            state.channels.remove(&old);
            continue;
        }
        let w = state.channels.get_mut(&old).unwrap();
        if !w.messages.is_empty() {
            let index = if w.cursor.mode == MessageWindowMode::Latest { 0 } else {
                w.cursor.has_newer = true;
                w.messages.len() - 1
            };
            w.bytes = w.bytes.saturating_sub(estimate(&w.messages.remove(index)));
        }
        if w.messages.is_empty() {
            state.channels.remove(&old);
        } else {
            changed(state, &old);
        }
    }
}
fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}
fn person_chars(p: &Person) -> usize {
    utf16_len(&p.id)
        + utf16_len(&p.name)
        + utf16_len(&p.username)
        + utf16_len(&p.display_name)
        + p.avatar_url.as_deref().map_or(0, utf16_len)
}
fn estimate(m: &ChatMessage) -> usize {
    let mut chars = utf16_len(&m.id)
        + utf16_len(&m.channel_id)
        + utf16_len(&m.author.id)
        + utf16_len(&m.author.name)
        + m.author.avatar_url.as_deref().map_or(0, utf16_len)
        + utf16_len(&m.content)
        + m.previous_contents.iter().map(|text| utf16_len(text)).sum::<usize>()
        + m.reply_to.as_deref().map_or(0, utf16_len);
    for a in &m.attachments {
        chars += utf16_len(&a.id)
            + utf16_len(&a.name)
            + utf16_len(&a.url)
            + a.content_type.as_deref().map_or(0, utf16_len)
    }
    for sticker in &m.stickers {
        chars += utf16_len(&sticker.id) + utf16_len(&sticker.name);
    }
    for embed in &m.embeds {
        chars += [&embed.title, &embed.description, &embed.url, &embed.author, &embed.author_url, &embed.provider, &embed.footer]
            .into_iter().map(|text| utf16_len(text)).sum::<usize>();
        chars += embed.fields.iter().map(|field| utf16_len(&field.name) + utf16_len(&field.value)).sum::<usize>();
    }
    chars += m.components.iter().map(|component| utf16_len(&component.text) + utf16_len(&component.url)).sum::<usize>();
    chars += m.mentions.iter().map(person_chars).sum::<usize>();
    if let Some(r) = &m.reply_preview {
        chars += utf16_len(&r.message_id)
            + utf16_len(&r.channel_id)
            + utf16_len(&r.content)
            + r.author.as_ref().map_or(0, person_chars)
            + r.mentions.iter().map(person_chars).sum::<usize>()
    }
    chars.saturating_mul(2)
        + 304
        + m.attachments.len() * 96
        + m.mentions.len() * 96
        + m.previous_contents.len() * 24
        + m.embeds.len() * std::mem::size_of::<notdiscord_core::models::ChatEmbed>()
        + m.embeds.iter().map(|embed| embed.fields.len() * std::mem::size_of::<notdiscord_core::models::ChatEmbedField>()).sum::<usize>()
        + m.components.len() * std::mem::size_of::<notdiscord_core::models::ChatComponent>()
        + m.reply_preview
            .as_ref()
            .map_or(0, |r| 128 + r.mentions.len() * 96)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rich_bot_text_and_component_vectors_share_the_existing_cache_budget() {
        let mut cache = MessageCache::default();
        let mut value = message("1", "2");
        cache.upsert(value.clone());
        let before = cache.estimated_text_bytes();
        value.embeds = vec![notdiscord_core::models::ChatEmbed { title: "A bot response".into(),
            description: "Known field contents".repeat(100), ..Default::default() }];
        value.components = vec![notdiscord_core::models::ChatComponent { kind: 10,
            text: "Displayed component".repeat(100), ..Default::default() }];
        cache.upsert(value.clone());
        assert!(cache.estimated_text_bytes() > before + 6000);
        value.components[0].text = "x".repeat(3 * 1024 * 1024);
        cache.upsert(value);
        assert!(cache.get("2").is_empty());
        assert!(cache.estimated_text_bytes() <= 4 * 1024 * 1024);
    }
    #[test]
    fn observed_versions_count_toward_the_existing_global_memory_budget() {
        let mut cache = MessageCache::default();
        let mut value = message("1", "2");
        cache.upsert(value.clone());
        let initial = cache.estimated_text_bytes();
        value.previous_contents = vec!["Observed 😀".repeat(100); 8];
        cache.upsert(value.clone());
        assert!(cache.estimated_text_bytes() > initial + 10000);
        value.previous_contents = vec!["x".repeat(3 * 1024 * 1024)];
        cache.upsert(value);
        assert!(cache.get("2").is_empty());
    }
    fn message(id: &str, channel: &str) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            channel_id: channel.into(),
            content: "Synthetic test text".into(),
            ..ChatMessage::default()
        }
    }
    #[test]
    fn bounds_are_global() {
        let mut cache = MessageCache::default();
        for channel in ["1", "2", "3"] {
            cache.replace(
                channel,
                (0..100).map(|n| message(&n.to_string(), channel)).collect(),
            )
        }
        assert_eq!(cache.count(), 200);
        assert_eq!(cache.channel_count(), 2);
        assert!(cache.get("1").is_empty());
        assert!(cache.estimated_text_bytes() <= 4 * 1024 * 1024);
    }
    #[test]
    fn read_touches_lru_and_oversized_not_truncated() {
        let mut c = MessageCache::default();
        for n in ["1", "2", "3"] {
            c.upsert(message("1", n))
        }
        c.get("1");
        c.upsert(message("1", "4"));
        assert!(c.get("2").is_empty());
        let mut m = message("1", "1");
        m.content = "x".repeat(3 * 1024 * 1024);
        c.upsert(m);
        assert!(c.get("1").is_empty());
    }
    #[test]
    fn metadata_and_unicode_cost() {
        let mut c = MessageCache::default();
        let mut m = message("1", "2");
        m.content = "😀".into();
        c.upsert(m.clone());
        let before = c.estimated_text_bytes();
        m.mentions = vec![Person {
            id: "3".into(),
            name: "Mention".into(),
            ..Person::default()
        }];
        c.upsert(m);
        assert!(c.estimated_text_bytes() > before);
    }
    #[test]
    fn point_lookup_keeps_lru_and_returns_an_independent_matching_message() {
        let mut cache = MessageCache::default();
        for channel in ["1", "2", "3"] {
            cache.upsert(message("10", channel));
        }
        let mut found = cache.find("1", "10").unwrap();
        found.content = "Locally edited snapshot".into();
        assert_eq!(
            cache.find("1", "10").unwrap().content,
            "Synthetic test text"
        );
        cache.upsert(message("10", "4"));
        assert!(cache.find("2", "10").is_none());
        assert!(cache.find("1", "10").is_some());
        assert!(cache.find("1", "missing").is_none());
        assert_eq!(cache.count(), 3);
    }
}
