//! Account-scoped cache of pure, source-preserving Markdown analysis. It owns
//! no theme, fonts, widths, resolved people or media handles.
use eframe::egui;
use notdiscord_core::markdown::{self, BlockKind, Document};
use std::{collections::hash_map::DefaultHasher, hash::{Hash, Hasher}, ops::Range, sync::{Arc, Mutex}};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 256 * 1024;

struct Entry {
    hash: u64,
    source: String,
    masks: Vec<Range<usize>>,
    document: Arc<Document>,
    bytes: usize,
    used: u64,
}
struct Cache { entries: Vec<Entry>, bytes: usize, clock: u64 }
impl Default for Cache {
    fn default() -> Self {
        Self { entries: Vec::with_capacity(MAX_ENTRIES), bytes: MAX_ENTRIES * std::mem::size_of::<Entry>(), clock: 0 }
    }
}
type Slot = Arc<Mutex<Cache>>;
fn id() -> egui::Id { egui::Id::new("account-markdown-analysis-cache") }

pub(super) fn clear(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<Slot>(id()));
}

pub(super) fn parse(source: &str, masks: &[Range<usize>]) -> Document {
    if masks.is_empty() { return markdown::parse(source); }
    // Equal byte lengths preserve every original UTF-8 range. Display names
    // are metadata atoms, even when their spelling contains Markdown syntax.
    let mut recognition = source.to_owned();
    for range in masks.iter().rev() {
        if recognition.get(range.clone()).is_some() {
            recognition.replace_range(range.clone(), &"x".repeat(range.len()));
        }
    }
    markdown::parse(&recognition)
}

pub(super) fn document(ctx: &egui::Context, source: &str, masks: &[Range<usize>]) -> Arc<Document> {
    if source.len() > MAX_SOURCE_BYTES { return Arc::new(parse(source, masks)); }
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    masks.hash(&mut hasher);
    let hash = hasher.finish();
    let slot = ctx.data_mut(|data| data.get_temp_mut_or_default::<Slot>(id()).clone());
    {
        let mut cache = slot.lock().unwrap_or_else(|error| error.into_inner());
        cache.clock = cache.clock.wrapping_add(1);
        let used = cache.clock;
        if let Some(entry) = cache.entries.iter_mut().find(|entry|
            entry.hash == hash && entry.source == source && entry.masks == masks) {
            entry.used = used;
            return entry.document.clone();
        }
    }
    // Parsing runs outside both egui's data lock and the cache lock.
    let document = Arc::new(parse(source, masks));
    let bytes = source.len().saturating_add(masks.len() * std::mem::size_of::<Range<usize>>())
        .saturating_add(std::mem::size_of::<Entry>() + std::mem::size_of::<Document>())
        .saturating_add(document.blocks.capacity() * std::mem::size_of::<markdown::Block>())
        .saturating_add(document.blocks.iter().map(|block| {
            block.spans.capacity() * std::mem::size_of::<markdown::Span>()
                + match &block.kind { BlockKind::CodeBlock { language: Some(language) } => language.capacity(), _ => 0 }
        }).sum::<usize>());
    if bytes.saturating_add(MAX_ENTRIES * std::mem::size_of::<Entry>()) > MAX_BYTES { return document; }
    let mut cache = slot.lock().unwrap_or_else(|error| error.into_inner());
    while cache.entries.len() >= MAX_ENTRIES || cache.bytes.saturating_add(bytes) > MAX_BYTES {
        let Some(index) = cache.entries.iter().enumerate().min_by_key(|(_, entry)| entry.used).map(|(index, _)| index) else { break; };
        let entry = cache.entries.swap_remove(index);
        cache.bytes = cache.bytes.saturating_sub(entry.bytes);
    }
    cache.clock = cache.clock.wrapping_add(1);
    let used = cache.clock;
    cache.entries.push(Entry { hash, source: source.into(), masks: masks.to_vec(), document: document.clone(), bytes, used });
    cache.bytes = cache.bytes.saturating_add(bytes);
    document
}
