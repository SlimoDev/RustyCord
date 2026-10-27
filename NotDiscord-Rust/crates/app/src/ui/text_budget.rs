//! Local editing/storage budgets. These are deliberately separate from the
//! account's 2k/4k Discord send limit and never truncate an accepted edit.
use super::{Draft, NotDiscordUi};
use eframe::egui::{self, text_edit::TextEditState};
use std::ops::Range;
type TextEditUndoer = egui::util::undoer::Undoer<(egui::text::CCursorRange,String)>;

pub(super) const PER_DRAFT_BYTES: usize = 256 * 1024;
pub(super) const GLOBAL_BYTES: usize = 8 * 1024 * 1024;
pub(super) const ERROR: &str = "La edición supera el presupuesto local de borradores (256 KiB por texto y 8 MiB compartidos). El texto anterior se conserva; reduce el pegado o libera un borrador pendiente.";

pub(super) fn weight(draft: &Draft) -> usize {
    draft.text.len().saturating_add(draft.mentions.iter().map(|mention|
        mention.id.len() + mention.label.len() + std::mem::size_of_val(mention)).sum::<usize>())
        .saturating_add(draft.paths.iter().map(|path| path.as_os_str().len()).sum::<usize>())
        .saturating_add(draft.reply.as_ref().map_or(0, |reply| reply.message_id.len() + reply.channel_id.len() + reply.content.len()
            + reply.mentions.iter().chain(reply.author.iter()).map(|person|
                person.id.len() + person.name.len() + person.username.len() + person.display_name.len()
                    + person.avatar_url.as_ref().map_or(0,String::len)).sum::<usize>()))
}

impl NotDiscordUi {
    pub(super) fn draft_input_limit(&self, current: &Draft, exclude_channel: Option<&str>, extra: usize) -> usize {
        let occupied = self.drafts.iter().filter(|(channel,_)| Some(channel.as_str()) != exclude_channel)
            .map(|(_,draft)| weight(draft)).chain(self.sent_drafts.values().map(weight))
            .fold(extra.saturating_add(super::dialogs::edit_document_bytes(&self.ctx)), usize::saturating_add)
            .saturating_add(weight(current).saturating_sub(current.text.len()));
        // Existing content is grandfathered so a tighter remaining aggregate
        // budget cannot truncate it. Growth is rejected until space is freed.
        PER_DRAFT_BYTES.min(GLOBAL_BYTES.saturating_sub(occupied)).max(current.text.len())
    }
    pub(super) fn accepts_draft_edit(&self, before: &Draft, after: &Draft, exclude_channel: Option<&str>, extra: usize) -> bool {
        let limit = self.draft_input_limit(before,exclude_channel,extra)
            .saturating_add(weight(before).saturating_sub(before.text.len()));
        after.text.len() <= PER_DRAFT_BYTES.max(before.text.len()) && weight(after) <= limit
    }
}

pub(super) struct Outcome { pub before: Option<String>, pub rejected: bool }
pub(super) struct Buffer<'a> {
    text: &'a mut String,
    limit: usize,
    before: Option<String>,
    rejected: bool,
}
impl<'a> Buffer<'a> {
    pub(super) fn new(text: &'a mut String, limit: usize) -> Self {
        Self { text, limit, before: None, rejected: false }
    }
    fn remember(&mut self) { if self.before.is_none() { self.before = Some(self.text.clone()); } }
    pub(super) fn finish(mut self) -> Outcome {
        if self.rejected && let Some(before) = self.before.take() { *self.text = before; }
        Outcome { before: self.before, rejected: self.rejected }
    }
}
impl egui::TextBuffer for Buffer<'_> {
    fn is_mutable(&self) -> bool { true }
    fn as_str(&self) -> &str { self.text }
    fn insert_text(&mut self, value: &str, char_index: usize) -> usize {
        if self.rejected { return 0; }
        if self.text.len().saturating_add(value.len()) > self.limit { self.rejected = true; return 0; }
        if !value.is_empty() { self.remember(); }
        <String as egui::TextBuffer>::insert_text(self.text, value, char_index)
    }
    fn delete_char_range(&mut self, range: Range<usize>) {
        if self.rejected || range.start == range.end { return; }
        self.remember();
        <String as egui::TextBuffer>::delete_char_range(self.text, range);
    }
    fn replace_with(&mut self, value: &str) {
        if self.rejected { return; }
        if value.len() > self.limit { self.rejected = true; return; }
        if self.text.as_str() != value { self.remember(); value.clone_into(self.text); }
    }
    fn type_id(&self) -> std::any::TypeId { std::any::TypeId::of::<Buffer<'static>>() }
}

/// Snapshot the undo stack only when an input batch can cross the byte budget
/// (or restores an older undo state). Idle and ordinary small edits do not copy
/// either the source or undo history merely to detect a text change.
pub(super) struct EditorBackup { state: Option<TextEditState>, undo: Option<TextEditUndoer> }
impl EditorBackup {
    pub(super) fn new(ctx: &egui::Context, id: egui::Id, bytes: usize, limit: usize) -> Self {
        let risk = ctx.input(|input| {
            let mut added = 0_usize;
            let mut undo = false;
            for event in &input.events {
                match event {
                    egui::Event::Text(value) | egui::Event::Paste(value)
                    | egui::Event::Ime(egui::ImeEvent::Preedit(value) | egui::ImeEvent::Commit(value)) => added = added.saturating_add(value.len()),
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        if matches!(key, egui::Key::Enter | egui::Key::Tab) { added = added.saturating_add(4); }
                        undo |= (modifiers.command || modifiers.ctrl) && matches!(key, egui::Key::Z | egui::Key::Y);
                    }
                    _ => {}
                }
            }
            undo || bytes.saturating_add(added) > limit
        });
        let state = egui::TextEdit::load_state(ctx,id);
        let undo = risk.then(|| state.as_ref().map(TextEditState::undoer)).flatten();
        Self { state, undo }
    }
    pub(super) fn restore(self, ctx: &egui::Context, id: egui::Id) {
        // The widget may have consumed an IME Commit/Disabled while rejecting
        // its text. Keep that post-event lifecycle instead of resurrecting the
        // previous composition flag; only cursor/undo belong to the rollback.
        let mut state = egui::TextEdit::load_state(ctx,id).unwrap_or_default();
        let existed = self.state.is_some();
        state.cursor = self.state.map_or_else(Default::default,|previous|previous.cursor);
        if let Some(undo) = self.undo { state.set_undoer(undo); }
        else if !existed { state.clear_undoer(); }
        state.store(ctx,id);
    }
}
