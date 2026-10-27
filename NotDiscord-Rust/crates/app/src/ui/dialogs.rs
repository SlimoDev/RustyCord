use super::*;
use eframe::egui::{self, Vec2};
use sha2::{Digest, Sha256};

#[derive(Clone, Default)]
struct SearchKeyboard {
    scope: Option<egui::Id>,
    selected: Option<usize>,
}
pub(super) fn suspend_search_keyboard(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<SearchKeyboard>(egui::Id::new("search-keyboard"));
        data.remove::<bool>(egui::Id::new("search-modal-was-open"));
        data.remove::<u64>(egui::Id::new("search-header-input-frame"));
    });
}

fn search_editing_input(input: &egui::InputState) -> bool {
    chat::composer_printable_input(input)
        || input.events.iter().any(|event| matches!(event,
            egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)
                | egui::Event::Ime(egui::ImeEvent::Enabled | egui::ImeEvent::Preedit(_) | egui::ImeEvent::Commit(_))))
        || [egui::Key::Backspace,egui::Key::Delete,egui::Key::Home,egui::Key::End,egui::Key::ArrowLeft,egui::Key::ArrowRight]
            .iter().any(|key|input.key_pressed(*key))
        || input.modifiers.command && [egui::Key::A,egui::Key::C,egui::Key::X,egui::Key::V,egui::Key::Z,egui::Key::Y]
            .iter().any(|key|input.key_pressed(*key))
}
fn consume_search_key(input:&mut egui::InputState,key:egui::Key)->bool {
    let mut consumed=false;
    input.events.retain(|event| {
        let matches=matches!(event,egui::Event::Key {key:pressed,pressed:true,modifiers,..}
            if *pressed==key && *modifiers==egui::Modifiers::NONE);
        consumed|=matches;
        !matches
    });
    consumed
}

fn centered_rect(area: egui::Rect, desired: Vec2) -> egui::Rect {
    let available = area.shrink(12.);
    egui::Rect::from_center_size(
        available.center(),
        desired.min(available.size().max(Vec2::splat(1.))),
    )
}

/// Reserve the action footer before laying out unbounded text/lists. Both
/// children receive their own clips, so growing content cannot cover actions.
fn dialog_content_and_footer(ui: &mut egui::Ui, footer_height: f32) -> (egui::Ui, egui::Ui) {
    let mut rect = ui.available_rect_before_wrap();
    if !rect.height().is_finite() {
        rect.max.y = rect.min.y + (ui.ctx().content_rect().height() - 100.).max(1.);
    }
    // Resize/window padding can make the requested layout rectangle taller
    // than its actual painter clip. Anchor actions to drawable content rather
    // than to those clipped-away pixels, including after zoom/resize changes.
    rect = rect.intersect(ui.clip_rect());
    let footer_height = footer_height.min(rect.height()).max(1.);
    let gap = ui.spacing().item_spacing.y.min((rect.height() - footer_height).max(0.));
    let body = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), (rect.bottom() - footer_height - gap).max(rect.top())));
    let footer = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - footer_height), rect.max);
    ui.allocate_rect(rect, egui::Sense::hover());
    let child = |ui: &mut egui::Ui, id, rect| {
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        child
    };
    (child(ui, "dialog-content", body), child(ui, "dialog-footer", footer))
}

fn dialog_action_height(ui: &egui::Ui) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
    (text_height + ui.spacing().button_padding.y * 2.)
        .max(ui.spacing().interact_size.y).max(28.)
}

fn dialog_action_row<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    // `horizontal` reserves only interact_size.y before positioning its child.
    // In a bottom-up footer, larger button text would then grow below the clip.
    // Reserve the actual single-line button height before anchoring the row.
    let size = egui::vec2(ui.available_width(), dialog_action_height(ui));
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), add_contents)
}

#[derive(Clone, Default)]
struct EditMentionDocument {
    message: String,
    channel: String,
    owner: String,
    generation: u64,
    draft: Draft,
    pending: Option<EditSubmission>,
    // TextEdit's undo stack stores text only. Keep a small, local metadata
    // companion so undo/redo restores IDs without inferring IDs from names.
    versions: Vec<([u8; 32], Vec<Mention>)>,
}
#[derive(Clone)]
struct EditSubmission {
    id: String,
    revision: u64,
}
impl EditMentionDocument {
    fn remember(&mut self, fingerprint: [u8; 32], mentions: Vec<Mention>) {
        if self.versions.last().is_some_and(|(previous, _)| *previous == fingerprint) { return; }
        self.versions.push((fingerprint, mentions));
        // Keep the before/after pair even for a large permitted source. Only
        // digests and mention metadata are retained here; TextEdit owns undo's
        // text. The editor reserves this same metadata allowance in its budget.
        while self.versions.len() > 8
            || self.versions.len() > 2 && self.versions_bytes() > self.history_reserve() {
            self.versions.remove(0);
        }
    }
}
fn edit_document_id() -> egui::Id { egui::Id::new("edit-mention-document") }
fn edit_text_id() -> egui::Id { egui::Id::new("edit-mention-text") }
pub(super) fn edit_document_bytes(ctx: &egui::Context) -> usize {
    ctx.data_mut(|data| {
        let Some(document) = data.remove_temp::<EditMentionDocument>(edit_document_id()) else { return 0; };
        let bytes = super::text_budget::weight(&document.draft).saturating_add(document.versions_bytes());
        data.insert_temp(edit_document_id(), document);
        bytes
    })
}
impl EditMentionDocument {
    fn versions_bytes(&self) -> usize {
        self.versions.iter().map(|(_, mentions)| 32 + mentions.iter().map(|mention|
            mention.id.len() + mention.label.len() + std::mem::size_of_val(mention)).sum::<usize>()).sum()
    }
    fn history_reserve(&self) -> usize {
        let metadata = |mentions: &[Mention]| mentions.iter().map(|mention|
            mention.id.len() + mention.label.len() + std::mem::size_of_val(mention)).sum::<usize>();
        self.versions.iter().map(|(_,mentions)|metadata(mentions))
            .chain(std::iter::once(metadata(&self.draft.mentions))).max().unwrap_or(0)
            .saturating_mul(2).saturating_add(64).max(128 * 1024)
    }
}
fn edit_fingerprint(text: &str) -> [u8; 32] { Sha256::digest(text.as_bytes()).into() }

fn remap_cursor(old: &str, new: &str, index: usize) -> usize {
    let a: Vec<_> = old.chars().collect();
    let b: Vec<_> = new.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
    if index <= prefix { index }
    else if index >= a.len() - suffix { (index as isize + b.len() as isize - a.len() as isize).max(0) as usize }
    else { b.len() - suffix }
}
/// Shared window shell. The app's scroll adapter owns the content scrollbar,
/// so dialog bodies support the same middle-button interaction as other lists.
/// Fixed-footer layouts opt out with vscroll(false) and own their body scroll.
pub(super) struct DialogWindow<'a> {
    window: egui::Window<'a>,
    scroll: bool,
    scroll_id: egui::Id,
}
impl<'a> DialogWindow<'a> {
    pub(super) fn open(mut self, open: &'a mut bool) -> Self { self.window = self.window.open(open); self }
    pub(super) fn vscroll(mut self, enabled: bool) -> Self { self.scroll = enabled; self }
    pub(super) fn auto_sized(mut self) -> Self { self.window = self.window.auto_sized(); self }
    pub(super) fn min_width(mut self, width: f32) -> Self { self.window = self.window.min_width(width); self }
    pub(super) fn max_width(mut self, width: f32) -> Self { self.window = self.window.max_width(width); self }
    pub(super) fn order(mut self, order: egui::Order) -> Self { self.window = self.window.order(order); self }
    pub(super) fn resizable(mut self, enabled: bool) -> Self { self.window = self.window.resizable(enabled); self }
    pub(super) fn collapsible(mut self, enabled: bool) -> Self { self.window = self.window.collapsible(enabled); self }
    pub(super) fn show<R>(self, ctx: &egui::Context, contents: impl FnOnce(&mut egui::Ui) -> R)
        -> Option<egui::InnerResponse<Option<R>>> {
        self.window.vscroll(false).show(ctx, |ui| {
            if self.scroll {
                let height = ui.available_height().max(1.);
                theme::vertical_scroll(ui).id_salt(self.scroll_id).max_height(height)
                    .min_scrolled_height(0.).auto_shrink([false, true]).show(ui, contents).inner
            } else { contents(ui) }
        })
    }
}

pub(super) fn centered_dialog<'a>(
    ctx: &egui::Context,
    title: impl Into<egui::WidgetText>,
    id: impl std::hash::Hash,
    size: Vec2,
) -> DialogWindow<'a> {
    let rect = centered_rect(ctx.content_rect(), size);
    let inner = (rect.size() - egui::vec2(16., 40.)).max(Vec2::splat(1.));
    let mut frame = egui::Frame::window(&ctx.style());
    // The shared content scroll area already owns its right-hand gutter.
    // Avoid another right padding outside that scrollbar.
    frame.inner_margin.right = 0;
    let id = egui::Id::new(id);
    let window = egui::Window::new(title)
        .order(egui::Order::Foreground)
        .frame(frame)
        .id(id)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .constrain_to(ctx.content_rect().shrink(8.))
        .default_size(inner)
        .max_size(inner)
        .min_size(inner.min(egui::vec2(280., 140.)))
        .collapsible(false)
        .vscroll(false);
    DialogWindow { window, scroll: true, scroll_id: id.with("dialog-content-scroll") }
}
fn search_popup_rect(screen: egui::Rect, search: egui::Rect) -> egui::Rect {
    let screen = screen.shrink(8.);
    let width = 410f32.min(screen.width()).max(1.);
    let below = (screen.bottom() - search.bottom() - 5.).max(0.);
    let above = (search.top() - screen.top() - 5.).max(0.);
    let use_below = below >= 210. || below >= above;
    let height = 430f32.min(if use_below { below } else { above }).max(1.);
    let x = (search.right() - width).clamp(screen.left(), screen.right() - width);
    let y = if use_below {
        search.bottom() + 5.
    } else {
        search.top() - 5. - height
    };
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_footer_uses_actual_clip_even_when_requested_space_extends_past_it() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700., 500.))),
            ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let clip = egui::Rect::from_min_max(ui.clip_rect().min, ui.clip_rect().max - egui::vec2(0., 32.));
                ui.set_clip_rect(clip);
                ui.label("Fixed heading");
                let (body, mut footer) = dialog_content_and_footer(ui, 64.);
                assert!(body.max_rect().bottom() <= footer.max_rect().top());
                assert!(clip.contains_rect(footer.max_rect()));
                footer.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let response = ui.button("Continuar");
                    assert!(clip.contains_rect(response.rect), "The full action target must remain drawable and clickable");
                });
            });
        });
    }
    #[test]
    fn fixed_footer_action_row_reserves_large_button_text_before_bottom_up_layout() {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let _ = ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700., 500.))),
            ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(30.));
                ui.spacing_mut().interact_size.y = 18.;
                let clip = egui::Rect::from_min_max(ui.clip_rect().min, ui.clip_rect().max - egui::vec2(0., 32.));
                ui.set_clip_rect(clip);
                ui.label("Fixed heading");
                let footer_height = dialog_action_height(ui) + 24.;
                let (body, mut footer) = dialog_content_and_footer(ui, footer_height);
                footer.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    dialog_action_row(ui, |ui| {
                        for label in ["Continuar", "Cancelar"] {
                            let response = ui.button(label);
                            assert!(response.rect.height() > ui.spacing().interact_size.y);
                            assert!(clip.contains_rect(response.rect), "The full enlarged action target must fit: {:?} in {clip:?}", response.rect);
                            assert!(response.rect.top() >= body.clip_rect().bottom());
                            actions.push(response.rect);
                        }
                    });
                });
            });
        });
        assert_eq!(actions.len(), 2);
    }
    fn isolated_dialog_frame(app: &mut NotDiscordUi, state: &AppSnapshot, physical: Vec2, events: Vec<egui::Event>) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, physical / ctx.zoom_factor())),
            events, ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |_| {});
            app.draw_dialog(ctx, state);
        })
    }
    fn visible_text_rect(output: &egui::FullOutput, label: &str, screen: egui::Rect) -> egui::Rect {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                assert!(shape.clip_rect.expand(0.5).contains_rect(rect), "{label} clipped: {rect:?} by {:?}", shape.clip_rect);
                assert!(screen.expand(0.5).contains_rect(rect), "{label} outside {screen:?}: {rect:?}");
                Some(rect)
            }
            _ => None,
        }).unwrap_or_else(|| panic!("Missing {label}"))
    }
    #[test]
    fn edit_dialog_displays_names_and_submits_original_ids_after_text_insertion() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let mut state = AppSnapshot { demo: true, selected_channel_id: "20".into(), search_people: vec![Person { id: "42".into(), name: "Visible name".into(), ..Default::default() }], ..Default::default() };
        state.messages = vec![ChatMessage { id: "100".into(), channel_id: "20".into(), content: "Hello <@42> <@43>".into(), ..Default::default() }].into();
        app.prompt_for("Editar mensaje", "Contenido del mensaje", "edit", "100", "Hello <@42> <@43>", "20", true);
        for _ in 0..4 { isolated_dialog_frame(&mut app, &state, egui::vec2(1000., 700.), vec![]); }
        assert_eq!(app.prompt, "Hello @Visible name @Usuario");
        state.search_people.push(Person { id: "43".into(), name: "Later resolution".into(), ..Default::default() });
        isolated_dialog_frame(&mut app, &state, egui::vec2(1000., 700.), vec![]);
        assert_eq!(app.prompt, "Hello @Visible name @Later resolution");
        ctx.memory_mut(|memory| memory.request_focus(edit_text_id()));
        let mut editor = egui::TextEdit::load_state(&ctx, edit_text_id()).unwrap();
        editor.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
        editor.store(&ctx, edit_text_id());
        isolated_dialog_frame(&mut app, &state, egui::vec2(1000., 700.), vec![egui::Event::Text("Prefix ".into())]);
        assert!(app.prompt.starts_with("Prefix Hello"));
        let output = isolated_dialog_frame(&mut app, &state, egui::vec2(1000., 700.), vec![]);
        let button = text_center(&output, "Continuar");
        for pressed in [true, false] { isolated_dialog_frame(&mut app, &state, egui::vec2(1000., 700.), click_at(button, pressed)); }
        let request = commands.try_recv().unwrap();
        assert_eq!((request.kind.as_str(), request.id.as_str(), request.channel_id.as_str()), ("edit", "100", "20"));
        assert_eq!(request.text, "Prefix Hello <@42> <@43>");
        assert!(matches!(app.dialog, Some(Dialog::Prompt { .. })));
        let document = ctx.data(|data| data.get_temp::<EditMentionDocument>(edit_document_id())).unwrap();
        let extra: Value = serde_json::from_str(&request.extra).unwrap();
        assert_eq!(document.pending.as_ref().unwrap().id, s(&extra, "editSubmissionId"));
        assert_eq!(document.draft.text, "Prefix Hello @Visible name @Later resolution");
    }
    #[test]
    fn long_edit_and_forward_lists_keep_actions_visible_after_resize_and_zoom() {
        let (mut app, _, _) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let message = ChatMessage { id: "100".into(), channel_id: "20".into(), ..Default::default() };
        let state = AppSnapshot { demo: true, selected_channel_id: "20".into(), direct_messages: (0..100).map(|i| NavItem {
            id: (1000 + i).to_string(), name: format!("Destination {i}"), kind: "dm".into(), ..Default::default()
        }).collect(), ..Default::default() };
        for (size, zoom) in [(egui::vec2(1100., 800.), 1.), (egui::vec2(740., 520.), 1.5), (egui::vec2(620., 440.), 1.), (egui::vec2(1100., 800.), 1.25)] {
            ctx.set_zoom_factor(zoom);
            app.prompt_for("Editar mensaje", "Contenido del mensaje", "edit", "100", &"Long line with content\n".repeat(100), "20", true);
            for _ in 0..5 { isolated_dialog_frame(&mut app, &state, size, vec![]); }
            let output = isolated_dialog_frame(&mut app, &state, size, vec![]);
            let footer = visible_text_rect(&output, "Continuar", ctx.content_rect());
            visible_text_rect(&output, "Cancelar", ctx.content_rect());
            visible_text_rect(&output, "Editar mensaje", ctx.content_rect());
            let content = output.shapes.iter().find(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.starts_with("Long line with content\n"))).unwrap();
            assert!(content.clip_rect.bottom() <= footer.top(), "Text editor must stay above action footer");
            app.dialog = Some(Dialog::Forward(message.clone()));
            app.forward_targets = state.direct_messages.clone();
            for _ in 0..5 { isolated_dialog_frame(&mut app, &state, size, vec![]); }
            let output = isolated_dialog_frame(&mut app, &state, size, vec![]);
            visible_text_rect(&output, "Reenviar a los destinos seleccionados", ctx.content_rect());
            visible_text_rect(&output, "Cerrar", ctx.content_rect());
            visible_text_rect(&output, "Reenviar mensaje", ctx.content_rect());
            assert!(ctx.data(|data| data.get_temp::<EditMentionDocument>(edit_document_id())).is_none(), "Closing edit releases its document");
            app.dialog = None;
            isolated_dialog_frame(&mut app, &state, size, vec![]);
        }
    }
    fn modal_frame(app: &mut NotDiscordUi, state: &AppSnapshot, events: Vec<egui::Event>) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 900.))),
            events, ..Default::default()
        }, |ctx| {
            app.draw_modal_barrier(ctx);
            app.draw_shell(ctx, state);
            app.draw_dialog(ctx, state);
        })
    }
    fn text_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => Some(text.pos + text.galley.size() / 2.),
            _ => None,
        }).unwrap_or_else(|| panic!("Missing {label}"))
    }
    fn click_at(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![egui::Event::PointerMoved(position), egui::Event::PointerButton {
            pos: position, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
        }]
    }
    #[test]
    fn modal_blocks_the_real_shell_but_its_confirmation_remains_operable() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        let state = AppSnapshot::welcome();
        for _ in 0..3 { modal_frame(&mut app, &state, vec![]); }
        let background = text_center(&modal_frame(&mut app, &state, vec![]), "Iniciar sesión en Discord");
        app.dialog = Some(Dialog::Confirm {
            title: "Synthetic modal".into(), description: "Only a local UI test".into(),
            action: UiAction::new("synthetic-confirm"),
        });
        for _ in 0..3 { modal_frame(&mut app, &state, vec![]); }
        for pressed in [true, false] { modal_frame(&mut app, &state, click_at(background, pressed)); }
        while let Ok(action) = commands.try_recv() { assert_ne!(action.kind, "login"); }
        let confirm = text_center(&modal_frame(&mut app, &state, vec![]), "Confirmar");
        for pressed in [true, false] { modal_frame(&mut app, &state, click_at(confirm, pressed)); }
        assert_eq!(commands.try_recv().unwrap().kind, "synthetic-confirm");
        assert!(app.dialog.is_none());
        for _ in 0..3 { modal_frame(&mut app, &state, vec![]); }
        for pressed in [true, false] { modal_frame(&mut app, &state, click_at(background, pressed)); }
        assert_eq!(commands.try_recv().unwrap().kind, "login", "Closing the modal restores the shell");
    }
    #[test]
    fn integrated_viewer_ownership_blocks_parent_shell_without_closing_it() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        let state = AppSnapshot::welcome();
        for _ in 0..3 { modal_frame(&mut app, &state, vec![]); }
        let background = text_center(&modal_frame(&mut app, &state, vec![]), "Iniciar sesión en Discord");
        app.viewer = Some(Viewer { url: String::new(), local_path: None, zoom: 1., pan: Vec2::ZERO });
        for _ in 0..3 { modal_frame(&mut app, &state, vec![]); }
        for pressed in [true, false] { modal_frame(&mut app, &state, click_at(background, pressed)); }
        while let Ok(action) = commands.try_recv() { assert_ne!(action.kind, "login"); }
        assert!(app.viewer.is_some(), "Background clicks must not close the integrated viewer");
    }
    #[test]
    fn search_filter_modal_keeps_its_own_text_input_and_can_submit() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        let state = AppSnapshot { demo: true, signed_in: true, view: "chat".into(), selected_channel_id: "synthetic-channel".into(), ..Default::default() };
        app.search_open = true;
        for _ in 0..3 { modal_frame(&mut app, &state, vec![]); }
        modal_frame(&mut app, &state, vec![egui::Event::Text("from:synthetic".into())]);
        assert_eq!(app.search, "from:synthetic");
        modal_frame(&mut app, &state, vec![egui::Event::Key {
            key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE,
        }]);
        let actions: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok()).collect();
        let searches: Vec<_> = actions.iter().filter(|action| action.kind == "search").collect();
        assert_eq!(searches.len(), 1);
        assert_eq!(searches[0].text, "from:synthetic");
        assert!(!app.search_open);
    }
    #[test]
    fn unicode_catalog_is_loaded_only_for_its_open_picker_and_released_after_closing() {
        let (mut app, _, _) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let state = AppSnapshot::welcome();
        let frame = |app: &mut NotDiscordUi| {
            let _ = ctx.run(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100., 800.))),
                ..Default::default()
            }, |ctx| app.draw_picker(ctx, &state));
        };
        assert_eq!(app.emojis.capacity(), 0);
        app.open_picker(false, &state);
        frame(&mut app);
        assert!(app.emojis.len() > 1000);
        assert!(app.emojis.iter().any(|row| row.0 == "😀" && row.2.contains("sonriendo")));
        let original = app.emojis.as_ptr();
        frame(&mut app);
        assert_eq!(app.emojis.as_ptr(), original, "Repainting reuses the same catalogue");
        app.picker = None;
        frame(&mut app);
        assert_eq!(app.emojis.capacity(), 0);
        app.open_picker(true, &state);
        frame(&mut app);
        assert_eq!(app.emojis.capacity(), 0, "GIF selection needs no Unicode catalogue");
    }
    #[test]
    fn custom_emoji_picker_keeps_its_source_resolution_and_selection_click() {
        let (mut app,_,_)=super::super::tests::harness();
        let ctx=app.ctx.clone();
        app.prefs.allow_gif_playback=false;
        let state=AppSnapshot {demo:true,selected_channel_id:"20".into(),guilds:vec![NavItem {id:"50".into(),name:"Offline guild".into(),..Default::default()}],..Default::default()};
        app.picker=Some(Picker {guild:"50".into(),items:vec![serde_json::json!({"Id":"401","Name":"Synthetic","Animated":true})],..Default::default()});
        let poster=app.pictures.seed_sized_fixture(&ctx,"https://cdn.discordapp.com/emojis/401.webp?size=48",64);
        let frame=|app:&mut NotDiscordUi,events|ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1100.,800.))),events,..Default::default()},|ctx|app.draw_picker(ctx,&state));
        for _ in 0..3 {frame(&mut app,vec![]);}
        let output=frame(&mut app,vec![]);
        let position=output.shapes.iter().find_map(|shape|match &shape.shape {egui::Shape::Mesh(mesh) if mesh.texture_id==poster=>Some(mesh.calc_bounds().center()),_=>None}).unwrap();
        for pressed in [true,false] {frame(&mut app,click_at(position,pressed));}
        assert!(app.picker.is_some());
        assert_eq!(app.drafts["20"].text,"<a:Synthetic:401>");
        assert_eq!(app.pictures.pending_fixture_requests(),0);
        assert_eq!(app.pictures.custom_animation_metrics()["worker"],false);
    }
    #[test]
    fn invite_dialog_explicit_click_delegates_once_with_the_invite_id() {
        let ctx = egui::Context::default();
        let (sender, mut commands) = tokio::sync::mpsc::unbounded_channel();
        let (_, updates) = std::sync::mpsc::channel();
        let mut app = NotDiscordUi::from_context(
            &ctx,
            std::env::temp_dir().join(format!("notdiscord-invite-ui-{}", uuid::Uuid::new_v4())),
            sender,
            updates,
        );
        let state = AppSnapshot { demo: true, ..Default::default() };
        app.dialog = Some(Dialog::Invite("Abc_12-X".into()));
        let frame = |app: &mut NotDiscordUi, events| ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100., 700.))),
            events,
            ..Default::default()
        }, |ctx| {
            egui::CentralPanel::default().show(ctx, |_| {});
            app.draw_dialog(ctx, &state);
        });
        for _ in 0..3 { frame(&mut app, vec![]); }
        assert!(commands.try_recv().is_err(), "Rendering an invite must not delegate it");
        let output = frame(&mut app, vec![]);
        let target = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Continuar en Discord" => Some(text.pos + text.galley.size() / 2.),
            _ => None,
        }).expect("Explicit external action is visible");
        for pressed in [true, false] {
            frame(&mut app, vec![egui::Event::PointerMoved(target), egui::Event::PointerButton {
                pos: target, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
            }]);
        }
        let action = commands.try_recv().unwrap();
        assert_eq!(action.kind, "invite-accept");
        assert_eq!(action.id, "Abc_12-X");
        assert!(action.text.is_empty());
        assert!(app.dialog.is_none());
        frame(&mut app, vec![]);
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn forward_dialog_selects_destination_keeps_pending_and_does_not_repeat_partial_sends(){
        let ctx=egui::Context::default();let (sender,mut commands)=tokio::sync::mpsc::unbounded_channel();let (_,updates)=std::sync::mpsc::channel();
        let mut app=NotDiscordUi::from_context(&ctx,std::env::temp_dir().join(format!("notdiscord-forward-ui-{}",uuid::Uuid::new_v4())),sender,updates);
        let message=ChatMessage{id:"100".into(),channel_id:"20".into(),content:"Fixture source".into(),..Default::default()};
        let state=AppSnapshot{signed_in:true,view:"chat".into(),selected_channel_id:"20".into(),direct_messages:vec![NavItem{id:"30".into(),name:"Fixture destination".into(),kind:"dm".into(),..Default::default()}],..Default::default()};
        app.state=state.clone();app.dialog=Some(Dialog::Forward(message.clone()));app.forward_targets=state.direct_messages.clone();app.forward_result=serde_json::json!({"channel_id":"20","message_id":"100"});
        let frame=|app:&mut NotDiscordUi,events|ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(900.,700.))),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|_|{});app.draw_dialog(ctx,&state);});
        let text=|output:&egui::FullOutput,needle:&str|output.shapes.iter().find_map(|shape|match &shape.shape{egui::Shape::Text(text) if text.galley.job.text==needle=>Some(text.pos+text.galley.size()/2.),_=>None}).unwrap_or_else(||panic!("Missing {needle}"));
        let pointer=|position,pressed|vec![egui::Event::PointerMoved(position),egui::Event::PointerButton{pos:position,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}];
        for _ in 0..3{frame(&mut app,vec![]);}let output=frame(&mut app,vec![]);let target=text(&output,"Fixture destination");
        for pressed in [true,false]{frame(&mut app,pointer(target,pressed));}assert_eq!(app.forward_selected,["30"]);
        let output=frame(&mut app,vec![]);let send=text(&output,"Reenviar a los destinos seleccionados");for pressed in [true,false]{frame(&mut app,pointer(send,pressed));}
        let action=commands.try_recv().unwrap();assert_eq!((action.kind.as_str(),action.id.as_str(),action.channel_id.as_str()),("forward-many","100","20"));assert_eq!(action.text,r#"["30"]"#);assert!(app.forward_pending);assert!(matches!(app.dialog,Some(Dialog::Forward(_))));
        for pressed in [true,false]{frame(&mut app,pointer(send,pressed));}assert!(commands.try_recv().is_err());
        app.data_update("forward-result",serde_json::json!({"channel_id":"20","message_id":"100","sent":1,"total":2,"partial":true,"error":"Fixture second destination rejected"}));assert!(!app.forward_pending);
        let output=frame(&mut app,vec![]);text(&output,"Fixture second destination rejected");
        for pressed in [true,false]{frame(&mut app,pointer(send,pressed));}assert!(commands.try_recv().is_err());
    }
    #[test]
    fn dialogs_follow_app_area_origin_and_resize_without_screen_coordinates() {
        let area = egui::Rect::from_min_size(egui::pos2(200., 100.), egui::vec2(1000., 650.));
        let desired = egui::vec2(520., 570.);
        let rect = centered_rect(area, desired);
        assert_eq!(rect.center(), area.center());
        assert!(area.contains_rect(rect));
        let shift = egui::vec2(-370., 140.);
        assert_eq!(
            centered_rect(area.translate(shift), desired),
            rect.translate(shift)
        );
        let smaller = egui::Rect::from_min_size(area.min, egui::vec2(420., 300.));
        assert!(smaller.contains_rect(centered_rect(smaller, desired)));
    }
    #[test]
    fn integrated_viewer_resizes_inside_root_and_preserves_chat_position_and_draft() {
        let (mut app, _, _) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        app.drafts.insert("20".into(), Draft { text: "Unsent fixture".into(), ..Default::default() });
        app.positions.insert("20".into(), ChatPosition { offset: 137., anchor: "30".into(), anchor_top: -12., at_bottom: false, distance_to_bottom: None });
        app.pictures.seed_fixture(&ctx, "offline-viewer-fixture");
        app.viewer = Some(Viewer { url: "offline-viewer-fixture".into(), local_path: None, zoom: 1., pan: Vec2::ZERO });
        for size in [egui::vec2(1100., 800.), egui::vec2(650., 430.), egui::vec2(1000., 700.)] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for _ in 0..4 {
                let output = ctx.run(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ctx| app.draw_viewer(ctx));
                assert!(output.viewport_output.keys().all(|id| *id == egui::ViewportId::ROOT));
            }
            let rect = ctx.memory(|memory| memory.area_rect(egui::Id::new("media-viewer"))).unwrap();
            assert!(screen.expand(1.).contains_rect(rect), "{rect:?} exceeds {screen:?}");
            assert!((rect.center() - screen.center()).length() < 2.);
        }
        let _ = ctx.run(egui::RawInput { events: vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }], ..Default::default() }, |ctx| app.draw_viewer(ctx));
        assert!(app.viewer.is_none());
        assert_eq!(app.drafts["20"].text, "Unsent fixture");
        assert_eq!(app.positions["20"].offset, 137.);
        assert_eq!(app.positions["20"].anchor, "30");
        assert!(!app.positions["20"].at_bottom);
    }
    #[test]
    fn viewer_reopens_without_reading_stale_child_input_and_closes_on_escape() {
        let (mut app, _, _) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        let child = egui::ViewportId::from_hash_of("notdiscord-image-viewer");
        // A closed child's retained internal input need not describe a current
        // backend viewport. Opening must use current root input, not unwrap it.
        ctx.input_mut_for(child, |input| {
            input.raw.viewport_id = child;
            input.raw.viewports.clear();
        });
        for _ in 0..3 {
            app.viewer = Some(Viewer { url: "offline-invalid-url".into(), local_path: None, zoom: 1., pan: Vec2::ZERO });
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900., 700.))),
                ..Default::default()
            };
            input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().outer_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::ZERO));
            let _ = ctx.run(input.clone(), |ctx| app.draw_viewer(ctx));
            assert!(app.viewer.is_some());
            input.events.push(egui::Event::Key { key:egui::Key::Escape,physical_key:None,pressed:true,repeat:false,modifiers:egui::Modifiers::NONE });
            let _ = ctx.run(input, |ctx| app.draw_viewer(ctx));
            assert!(app.viewer.is_none());
            let mut release = egui::RawInput::default();
            release.events.push(egui::Event::Key { key:egui::Key::Escape,physical_key:None,pressed:false,repeat:false,modifiers:egui::Modifiers::NONE });
            let _ = ctx.run(release, |ctx| app.draw_viewer(ctx));
            drop(ctx.tex_manager().write().take_delta());
        }
    }

    #[test]
    fn integrated_viewer_closes_with_its_visible_cross() {
        let (mut app, _, _) = super::super::tests::harness();
        let ctx = app.ctx.clone();
        app.pictures.seed_fixture(&ctx, "offline-viewer-fixture");
        app.viewer = Some(Viewer { url: "offline-viewer-fixture".into(), local_path: None, zoom: 1., pan: Vec2::ZERO });
        let frame = |app: &mut NotDiscordUi, events| ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100., 800.))),
            events, ..Default::default()
        }, |ctx| app.draw_viewer(ctx));
        for _ in 0..3 { frame(&mut app, vec![]); }
        let output = frame(&mut app, vec![]);
        let rect = ctx.memory(|memory| memory.area_rect(egui::Id::new("media-viewer"))).unwrap();
        let cross = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::LineSegment { points, .. } if points[0].x > rect.right() - 40.
                && points[0].y < rect.top() + 45. && points[1].x > points[0].x
                && points[1].y > points[0].y => Some(points[0].lerp(points[1], 0.5)),
            _ => None,
        }).expect("Visible cross in the viewer title bar");
        for pressed in [true, false] { frame(&mut app, vec![egui::Event::PointerMoved(cross),
            egui::Event::PointerButton { pos: cross, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]); }
        assert!(app.viewer.is_none());
    }

    #[test]
    fn popup_keeps_first_row_and_footer_inside_small_and_normal_screens() {
        for size in [
            egui::vec2(1360., 860.),
            egui::vec2(1000., 650.),
            egui::vec2(420., 300.),
        ] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for y in [60., size.y - 50.] {
                let search =
                    egui::Rect::from_min_size(egui::pos2(size.x - 340., y), egui::vec2(320., 32.));
                let popup = search_popup_rect(screen, search);
                assert!(screen.contains_rect(popup));
                assert!(popup.width() <= 410.);
                assert!(popup.height() >= 150.);
                // Heading/padding, one full 56px row and footer fit without
                // requiring the list to begin at a negative scroll position.
                assert!(popup.height() - 76. >= 56.);
                assert!(popup.bottom() <= search.top() - 4. || popup.top() >= search.bottom() + 4.);
            }
        }
    }

    #[test]
    fn viewer_fit_and_one_hundred_percent_are_distinct_and_bounded() {
        let natural = egui::vec2(1600., 1200.);
        let canvas = egui::vec2(800., 600.);
        let fit = viewer_fit(natural, canvas);
        assert!(fit < 0.5);
        assert!((natural * fit).x < canvas.x && (natural * fit).y < canvas.y);
        let mut viewer = Viewer {
            url: String::new(),
            local_path: None,
            zoom: 1.,
            pan: Vec2::ZERO,
        };
        zoom_viewer(&mut viewer, natural, canvas, 1. / fit, Vec2::ZERO);
        assert!((fit * viewer.zoom - 1.).abs() < 0.0001);
        zoom_viewer(&mut viewer, natural, canvas, 100000., Vec2::ZERO);
        assert!((fit * viewer.zoom - 8.).abs() < 0.0001);
    }

    #[test]
    fn viewer_zoom_preserves_pointer_anchor_and_pan_never_exposes_empty_edges() {
        let natural = egui::vec2(1600., 1200.);
        let canvas = egui::vec2(800., 600.);
        let mut viewer = Viewer {
            url: String::new(),
            local_path: None,
            zoom: 2.,
            pan: egui::vec2(30., 20.),
        };
        let anchor = egui::vec2(50., 30.);
        let point_before = (anchor - viewer.pan) / viewer.zoom;
        zoom_viewer(&mut viewer, natural, canvas, 3., anchor);
        let point_after = (anchor - viewer.pan) / viewer.zoom;
        assert!((point_before - point_after).length() < 0.001);
        viewer.pan = Vec2::splat(10000.);
        clamp_viewer_pan(&mut viewer, natural, canvas);
        let overflow = (natural * viewer_fit(natural, canvas) * viewer.zoom - canvas) * 0.5;
        assert!(viewer.pan.x <= overflow.x && viewer.pan.y <= overflow.y);
        viewer.zoom = 1.;
        clamp_viewer_pan(&mut viewer, natural, canvas);
        assert_eq!(viewer.pan, Vec2::ZERO);
    }

    #[test]
    fn original_file_name_keeps_type_without_url_query_or_path_characters() {
        assert_eq!(
            media_file_name(
                "https://cdn.discordapp.com/attachments/1/2/photo.jpg?ex=123",
                "imagen"
            ),
            "photo.jpg"
        );
        assert_eq!(
            media_file_name("https://tenor.com/view/example-gif-123", "imagen"),
            "animacion.gif"
        );
        assert_eq!(
            media_file_name("https://cdn.discordapp.com/a", "bad/name:*"),
            "badname.png"
        );
    }

    #[test]
    fn feedback_slot_is_shared_with_workers_without_clipboard_or_disk_operations() {
        let ctx = egui::Context::default();
        let worker = set_media_feedback(&ctx, "fixture", "Copiando", true, false);
        assert!(begin_media_operation(&ctx, "other", "Guardando").is_none());
        worker.lock().unwrap().text = "Copiada".into();
        assert_eq!(media_feedback(&ctx).lock().unwrap().text, "Copiada");
        worker.lock().unwrap().busy = false;
        assert!(begin_media_operation(&ctx, "other", "Guardando").is_some());
        set_media_feedback(&ctx, "other", "Guardando", true, false);
        assert_eq!(worker.lock().unwrap().url, "other");
    }
}

pub(super) fn viewer_fit(natural: Vec2, canvas: Vec2) -> f32 {
    ((canvas.x - 20.).max(1.) / natural.x.max(1.))
        .min((canvas.y - 20.).max(1.) / natural.y.max(1.))
        .min(1.)
        .max(0.0001)
}
pub(super) fn clamp_viewer_pan(viewer: &mut Viewer, natural: Vec2, canvas: Vec2) {
    let overflow =
        ((natural * viewer_fit(natural, canvas) * viewer.zoom - canvas) * 0.5).max(Vec2::ZERO);
    viewer.pan.x = viewer.pan.x.clamp(-overflow.x, overflow.x);
    viewer.pan.y = viewer.pan.y.clamp(-overflow.y, overflow.y);
}
pub(super) fn zoom_viewer(viewer: &mut Viewer, natural: Vec2, canvas: Vec2, requested: f32, anchor: Vec2) {
    let fit = viewer_fit(natural, canvas);
    let previous = viewer.zoom.max(0.0001);
    viewer.zoom = (requested * fit).clamp(fit.min(0.05), 8.) / fit;
    viewer.pan = anchor + (viewer.pan - anchor) * (viewer.zoom / previous);
    clamp_viewer_pan(viewer, natural, canvas);
}
#[derive(Clone, Default)]
struct MediaFeedback {
    url: String,
    text: String,
    busy: bool,
    error: bool,
}
type MediaFeedbackSlot = std::sync::Arc<std::sync::Mutex<MediaFeedback>>;
fn media_feedback(ctx: &egui::Context) -> MediaFeedbackSlot {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<MediaFeedbackSlot>(egui::Id::new("media-operation-feedback"))
            .clone()
    })
}
fn set_media_feedback(
    ctx: &egui::Context,
    url: &str,
    text: &str,
    busy: bool,
    error: bool,
) -> MediaFeedbackSlot {
    let slot = media_feedback(ctx);
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = MediaFeedback {
        url: url.into(),
        text: text.into(),
        busy,
        error,
    };
    ctx.request_repaint();
    slot
}
fn begin_media_operation(ctx: &egui::Context, url: &str, text: &str) -> Option<MediaFeedbackSlot> {
    let slot = media_feedback(ctx);
    {
        let mut feedback = slot.lock().unwrap_or_else(|e| e.into_inner());
        if feedback.busy {
            return None;
        }
        *feedback = MediaFeedback {
            url: url.into(),
            text: text.into(),
            busy: true,
            error: false,
        };
    }
    ctx.request_repaint();
    Some(slot)
}
fn media_file_name(url: &str, suggested: &str) -> String {
    if notdiscord_core::parsing::gif_page(url) {
        return "animacion.gif".into();
    }
    let candidate = if !suggested.is_empty() && suggested != "imagen" {
        suggested.to_string()
    } else {
        url::Url::parse(url)
            .ok()
            .and_then(|u| {
                u.path_segments()
                    .and_then(|mut p| p.next_back())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| "imagen.png".into())
    };
    let candidate: String = candidate
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c))
        .take(140)
        .collect();
    if candidate.is_empty() {
        "imagen.png".into()
    } else if std::path::Path::new(&candidate).extension().is_none() {
        format!("{candidate}.png")
    } else {
        candidate
    }
}

impl NotDiscordUi {
    pub(super) fn clear_edit_prompt(&mut self, ctx: &egui::Context) {
        if matches!(&self.dialog, Some(Dialog::Prompt { action, .. }) if action.kind == "edit") {
            self.dialog = None;
            self.prompt.clear();
            self.dialog_error.clear();
        }
        ctx.data_mut(|data| {
            data.remove::<EditMentionDocument>(edit_document_id());
            data.remove::<egui::text_edit::TextEditState>(edit_text_id());
        });
    }

    pub(super) fn edit_result(&mut self, result: &Value) {
        let Some(error) = result["error"].as_str() else { return; };
        let Some(mut document) = self.ctx.data(|data| data.get_temp::<EditMentionDocument>(edit_document_id())) else { return; };
        let Some(pending) = &document.pending else { return; };
        // A generic error or a late reply to a closed/replaced editor must not
        // discard newer work or open another account's message.
        if !self.state.signed_in
            || self.state.current_user.as_ref().map(|user| user.id.as_str()) != Some(document.owner.as_str())
            || self.state.notification_generation != document.generation
            || s(result, "owner") != document.owner
            || result["generation"].as_u64() != Some(document.generation)
            || s(result, "submissionId") != pending.id
            || result["revision"].as_u64() != Some(pending.revision)
            || s(result, "channelId") != document.channel
            || s(result, "messageId") != document.message
            || !matches!(&self.dialog, Some(Dialog::Prompt { action, .. })
                if action.kind == "edit" && action.channel_id == document.channel && action.id == document.message)
        { return; }
        if error.is_empty() && document.draft.revision==pending.revision {
            let ctx = self.ctx.clone();
            self.clear_edit_prompt(&ctx);
        } else {
            document.pending = None;
            self.dialog_error = if error.is_empty() {"La revisión enviada se guardó. Hay cambios posteriores que todavía no se han enviado.".into()}else{error.into()};
            self.ctx.data_mut(|data| data.insert_temp(edit_document_id(), document));
        }
    }

    pub(super) fn background_blocked(&self) -> bool {
        self.dialog.is_some() || self.picker.is_some() || self.viewer.is_some() || self.search_open
            || crate::diagnostics::is_open(&self.ctx)
            || self.ctx.data(|data|data.get_temp::<u64>(egui::Id::new("overlay-dismissal-block")))
                == Some(self.ctx.cumulative_frame_nr())
    }
    pub(super) fn draw_modal_barrier(&self, ctx: &egui::Context) {
        if !self.background_blocked() { return; }
        let id = egui::Id::new("notdiscord-modal-background");
        // The backdrop may be raised when clicked. Its order must remain below
        // foreground modal windows even after an outside click; otherwise it
        // covers the dialog and intercepts its confirmation/input controls.
        let layer = egui::LayerId::new(egui::Order::Middle, id);
        let rect = ctx.content_rect();
        ctx.memory_mut(|memory| memory.set_modal_layer(layer));
        egui::Area::new(id)
            .order(egui::Order::Middle)
            .fixed_pos(rect.min)
            .movable(false)
            .interactable(true)
            .sense(egui::Sense::click_and_drag())
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());
                ui.allocate_rect(rect, egui::Sense::click_and_drag());
                ui.painter().rect_filled(rect, 0., egui::Color32::from_black_alpha(60));
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    pub(super) fn open_media(&mut self) {
        self.dialog = Some(Dialog::Media);
        self.simple("media-settings");

    }
    fn draw_edit_prompt(&mut self, ctx: &egui::Context, state: &AppSnapshot, title: &str, description: &str, action: &UiAction, open: &mut bool) -> bool {
        let owner = state.current_user.as_ref().map_or("", |user| user.id.as_str());
        let explicit = state.messages.iter().chain(state.pinned_messages.iter())
            .find(|message| message.id == action.id && message.channel_id == action.channel_id)
            .map(|message| message.mentions.as_slice()).unwrap_or(&[]);
        let mut document = ctx.data_mut(|data| data.remove_temp::<EditMentionDocument>(edit_document_id()))
            .filter(|document| document.message == action.id && document.channel == action.channel_id
                && document.owner == owner && document.generation == state.notification_generation)
            .unwrap_or_else(|| EditMentionDocument {
                message: action.id.clone(), channel: action.channel_id.clone(),
                owner: owner.into(), generation: state.notification_generation,
                draft: mentions::from_source(&self.prompt, explicit, state),
                pending: None,
                versions: Vec::new(),
            });
        let old = document.draft.text.clone();
        if mentions::refresh_names(&mut document.draft, explicit, state) {
            if let Some(mut editor) = egui::TextEdit::load_state(ctx, edit_text_id()) {
                if let Some(mut range) = editor.cursor.char_range() {
                    range.primary.index = remap_cursor(&old, &document.draft.text, range.primary.index);
                    range.secondary.index = remap_cursor(&old, &document.draft.text, range.secondary.index);
                    editor.cursor.set_char_range(Some(range));
                    editor.store(ctx, edit_text_id());
                }
            }
        }
        let mut cancel = false;
        let tail_budget=self.draft_input_limit(&document.draft,None,document.history_reserve()).saturating_sub(document.draft.text.len());
        let enter_tail=document.pending.is_none().then(||super::input_ownership::take_edit_submit(ctx,edit_text_id(),tail_budget)).flatten();
        let enter=enter_tail.is_some();
        let tail_bytes=enter_tail.as_ref().map_or(0,String::len);
        centered_dialog(ctx, title, "edit-message-dialog", egui::vec2(680., 480.))
            .vscroll(false).open(open).show(ctx, |ui| {
                ui.label(description);
                let row = dialog_action_height(ui);
                let note = mentions::broadcast_note(&document.draft.text, state);
                let footer_height = row + ui.spacing().item_spacing.y + if note.is_some() { 58. } else { 22. };
                let (mut body, mut footer) = dialog_content_and_footer(ui, footer_height);
                let size = self.prefs.chat_size;
                let ink = self.theme.ink;
                let allow_broadcast = mentions::everyone_allowed(state);
                let spans = document.draft.mentions.clone();
                let byte_limit = self.draft_input_limit(&document.draft, None,
                    document.history_reserve()).saturating_sub(tail_bytes).max(document.draft.text.len());
                let backup = super::text_budget::EditorBackup::new(ctx, edit_text_id(), document.draft.text.len(), byte_limit);
                let mut outcome = None;
                let undo_or_redo = ctx.input(|input| input.events.iter().any(|event| matches!(event,
                    egui::Event::Key { key: egui::Key::Z | egui::Key::Y, pressed: true, modifiers, .. }
                    if (modifiers.command || modifiers.ctrl) && !modifiers.alt)));
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, width: f32| {
                    let job = mentions::editor_job_cached(ui.ctx(), text.as_str(), &spans, size, ink, allow_broadcast, width);
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                let height = body.available_height().max(1.);
                theme::vertical_scroll(&mut body).id_salt("edit-message-scroll")
                    .max_height(height).min_scrolled_height(0.).auto_shrink([false, false])
                    .show(&mut body, |ui| {
                        ui.add_enabled_ui(document.pending.is_none(), |ui| {
                            let mut buffer = super::text_budget::Buffer::new(&mut document.draft.text, byte_limit);
                            ui.add(bordered(egui::TextEdit::multiline(&mut buffer)
                                .id(edit_text_id()).font(egui::FontId::proportional(size))
                                .desired_width(ui.available_width()).desired_rows(8).layouter(&mut layouter)).focus_id(edit_text_id()));
                            outcome = Some(buffer.finish());
                        });
                    });
                let outcome = outcome.unwrap_or(super::text_budget::Outcome { before: None, rejected: false });
                if outcome.rejected {
                    backup.restore(ctx, edit_text_id());
                    self.dialog_error = super::text_budget::ERROR.into();
                }
                if let Some(before) = outcome.before.filter(|before| before != &document.draft.text) {
                    let fingerprint = edit_fingerprint(&document.draft.text);
                    let restored = undo_or_redo.then(|| document.versions.iter().rev()
                        .find(|(previous, _)| *previous == fingerprint).map(|(_, spans)| spans.clone())).flatten();
                    if let Some(spans) = restored { document.draft.mentions = spans; }
                    else { mentions::adjust(&before, &document.draft.text, &mut document.draft.mentions); }
                    document.remember(edit_fingerprint(&before), spans);
                    document.remember(fingerprint, document.draft.mentions.clone());
                    document.draft.revision = document.draft.revision.saturating_add(1);
                }
                // Fixed actions are laid out from the bottom, ahead of any
                // explanatory text. Long content/status never moves them.
                footer.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let content = chat::submission_content(&document.draft).and_then(|content| {
                        state.upload_limit.validate_content(&content)?;
                        Ok(content)
                    });
                    let pending = document.pending.is_some();
                    dialog_action_row(ui, |ui| {
                        if (ui.add_enabled(content.is_ok() && !pending, egui::Button::new(if pending { "Guardando…" } else { "Continuar" })).clicked() || enter)
                            && content.is_ok() && !pending && !outcome.rejected {
                            let mut request = action.clone();
                            request.text = mentions::outgoing(&document.draft);
                            let submission = uuid::Uuid::new_v4().simple().to_string();
                            request.extra = serde_json::json!({"editSubmissionId":submission,
                                "editAccountId":document.owner,"editGeneration":document.generation,
                                "editRevision":document.draft.revision}).to_string();
                            if self.command.send(request).is_ok() {
                                document.pending = Some(EditSubmission { id: submission, revision: document.draft.revision });
                                self.dialog_error.clear();
                            }
                            else { self.dialog_error = "El servicio de la aplicación se cerró.".into(); }
                        }
                        if ui.button(if pending { "Cerrar" } else { "Cancelar" })
                            .on_hover_text(if pending { "Cierra el editor; la operación ya enviada puede completarse y no se cancela." } else { "Descartar los cambios del editor." })
                            .clicked() { cancel = true; }
                    });
                    if let Some(note) = note { ui.add(egui::Label::new(RichText::new(note).size(12.).color(self.theme.muted)).truncate()).on_hover_text(note); }
                    if !self.dialog_error.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&self.dialog_error).color(Color32::from_rgb(242, 63, 67))).truncate()).on_hover_text(&self.dialog_error);
                    }
                    else if let Err(error) = &content {
                        ui.add(egui::Label::new(RichText::new(error).color(Color32::from_rgb(242, 63, 67))).truncate()).on_hover_text(error);
                    }
                    else if pending { ui.small("Esperando confirmación; el texto se conserva hasta recibir el resultado."); }
                    else { ui.small(format!("{} / {}", content.as_ref().map_or(0,|text| text.encode_utf16().count()), state.upload_limit.message_char_limit())); }
                });
            });
        if let Some(tail)=enter_tail.filter(|tail|!tail.is_empty()) {
            let before=document.draft.text.clone();
            let old_mentions=document.draft.mentions.clone();
            let mut editor=egui::TextEdit::load_state(ctx,edit_text_id()).unwrap_or_default();
            let cursor=editor.cursor.char_range().map_or(document.draft.text.chars().count(),|range|range.primary.index);
            chat::restore_post_send_text(&mut document.draft,&tail,cursor);
            document.remember(edit_fingerprint(&before),old_mentions);
            document.remember(edit_fingerprint(&document.draft.text),document.draft.mentions.clone());
            editor.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor+tail.chars().count()))));
            editor.store(ctx,edit_text_id());
        }
        self.prompt = document.draft.text.clone();
        if cancel { *open = false; }
        if *open { ctx.data_mut(|data| data.insert_temp(edit_document_id(), document)); }
        else {
            self.prompt = String::new();
            ctx.data_mut(|data| data.remove::<egui::text_edit::TextEditState>(edit_text_id()));
        }
        false
    }
    pub(super) fn draw_dialog(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        self.draw_search_popup(ctx, state);
        if !matches!(&self.dialog, Some(Dialog::Prompt { action, .. }) if action.kind == "edit") {
            ctx.data_mut(|data| {
                data.remove::<EditMentionDocument>(edit_document_id());
                data.remove::<egui::text_edit::TextEditState>(edit_text_id());
            });
        }
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        let mut open = true;
        match dialog {
            Dialog::GuildInvite(guild) => {
                let mut done = false;
                centered_dialog(
                    ctx,
                    "Invitar a personas al servidor",
                    "guild-invite-dialog",
                    egui::vec2(470., 500.),
                )
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Elige un canal para crear una invitación válida durante 24 horas.");
                    let channels = field(&self.dialog_data, "channels")
                        .and_then(|v| serde_json::from_value::<Vec<NavItem>>(v.clone()).ok())
                        .unwrap_or_default();
                    theme::vertical_scroll(ui)
                        .max_height(350.)
                        .show(ui, |ui| {
                            for channel in channels {
                                ui.selectable_value(
                                    &mut self.prompt,
                                    channel.id,
                                    format!("# {}", channel.name),
                                );
                            }
                        });
                    if ui
                        .add_enabled(
                            !self.prompt.is_empty(),
                            egui::Button::new("Crear invitación al canal seleccionado"),
                        )
                        .clicked()
                    {
                        self.send("create-guild-invite", self.prompt.clone(), guild, "", "");
                        done = true;
                    }
                });
                if done {
                    self.dialog = None;
                }
            }
            Dialog::Members => {
                let shown = centered_dialog(
                    ctx,
                    "Miembros del canal",
                    "channel-members-dialog",
                    egui::vec2(360., 560.),
                )
                .open(&mut open)
                .show(ctx, |ui| self.draw_members(ui, state));
                if open && shown.is_some() {
                    member_demand::drawn(ctx, true);
                }
            }
            Dialog::DmProfile => {
                centered_dialog(ctx, "Perfil", "dm-profile-dialog", egui::vec2(370., 600.))
                    .open(&mut open)
                    .show(ctx, |ui| self.draw_dm_sidebar(ui, state));
            }
            Dialog::Settings => self.draw_appearance_settings(ctx, state, &mut open),
            Dialog::Sounds => self.draw_sound_settings(ctx, &mut open),
            Dialog::Sources => {open=self.draw_source_selector(ctx,state);}
            Dialog::Cameras | Dialog::CamerasPreview => {open=self.draw_camera_selector(ctx,state,matches!(dialog,Dialog::CamerasPreview));}
            Dialog::Media => self.draw_media_settings(ctx, state),
            Dialog::Prompt {
                title,
                description,
                action,
                multiline,
            } => {
                let mut submit = false;
                let mut next_dialog = None;
                if action.kind == "edit" {
                    submit = self.draw_edit_prompt(ctx, state, &title, &description, &action, &mut open);
                } else {
                centered_dialog(
                    ctx,
                    title,
                    "prompt-dialog",
                    egui::vec2(
                        470.,
                        if action.kind == "group-add" {
                            500.
                        } else if multiline {
                            360.
                        } else {
                            200.
                        },
                    ),
                )
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(description);
                    if action.kind == "group-add" {
                        theme::vertical_scroll(ui)
                            .max_height(350.)
                            .show(ui, |ui| {
                                for person in &state.group_recipient_candidates {
                                    if self.person_selection_row(ui, person).clicked() {
                                        self.send(
                                            "group-add",
                                            &person.id,
                                            "",
                                            "",
                                            &state.selected_channel_id,
                                        );
                                        submit = true;
                                    }
                                }
                            });
                    } else {
                        let field_id=egui::Id::new(("prompt-primary",&action.kind,&action.id,&action.channel_id));
                        let enter=!multiline && super::input_ownership::take_submit(ctx,field_id);
                        if multiline {
                            ui.add_sized(
                                [ui.available_width(), 140.],
                                bordered(egui::TextEdit::multiline(&mut self.prompt).id(field_id)
                                    .char_limit(moderation::prompt_limit(&action).unwrap_or(usize::MAX))).focus_id(field_id),
                            );
                        } else {
                            ui.add(bordered(egui::TextEdit::singleline(&mut self.prompt).id(field_id)
                                .char_limit(moderation::prompt_limit(&action).unwrap_or(usize::MAX))).focus_id(field_id));
                        }
                        let policy_error = moderation::action_error(&action, state);
                        let field_error = moderation::prompt_error(&action,&self.prompt);
                        if let Some(limit) = moderation::prompt_limit(&action) {ui.small(format!("{} / {limit}",self.prompt.chars().count()));}
                        if let Some(error) = &policy_error {ui.add(egui::Label::new(error).wrap());}
                        if let Some(error) = &field_error {ui.add(egui::Label::new(error).wrap());}
                        if action.kind=="member-moderation" && !self.dialog_error.is_empty() {ui.colored_label(self.theme.accent,&self.dialog_error);}
                        if (ui.add_enabled(policy_error.is_none()&&field_error.is_none(),egui::Button::new("Continuar")).clicked() || enter)
                            && policy_error.is_none() && field_error.is_none() {
                            if action.kind == "member-moderation" {
                                match self.member_moderation_submission(&action,state) {
                                    Ok((action,confirmation)) => {
                                        if confirmation.is_none() {let _=self.command.send(action);}
                                        next_dialog=confirmation;submit=true;
                                    }
                                    Err(error)=>self.dialog_error=error,
                                }
                            } else {
                                let mut action = action.clone();action.text = self.prompt.clone();
                                let _ = self.command.send(action);submit = true;
                            }
                        }
                    }
                });
                }
                if submit {
                    self.dialog = next_dialog;
                }
            }
            Dialog::Confirm {
                title,
                description,
                action,
            } => {
                let mut done = false;
                centered_dialog(ctx, title, "confirmation-dialog", egui::vec2(470., 220.))
                    .open(&mut open)
                    .collapsible(false)
                    .show(ctx, |ui| {
                        ui.label(description);
                        let policy_error=moderation::action_error(&action,state)
                            .or_else(||group_members::action_error(&action,state));
                        if let Some(error)=&policy_error {ui.add(egui::Label::new(error).wrap());}
                        ui.add_space(15.);
                        ui.horizontal(|ui| {
                            if ui.button("Cancelar").clicked() {
                                done = true;
                            }
                            if ui
                                .add_enabled(policy_error.is_none(),egui::Button::new(RichText::new("Confirmar").color(
                                    if matches!(action.kind.as_str(),"delete"|"remove-group-member") || action.kind=="member-moderation" && matches!(action.extra.as_str(),"kick"|"ban") {
                                        Color32::from_rgb(242, 63, 67)
                                    } else {
                                        self.theme.ink
                                    },
                                )))
                                .clicked()
                            {
                                let _ = self.command.send(action);
                                done = true;
                            }
                        });
                    });
                if done {
                    self.dialog = None;
                }
            }
            Dialog::Forward(message) => {
                let mut done = false;
                let result=self.forward_result.clone();
                let finished=result["sent"].as_u64().is_some_and(|sent|sent>0)||result["partial"]==true;
                let locked=self.forward_pending||finished;
                centered_dialog(
                    ctx,
                    "Reenviar mensaje",
                    "forward-dialog",
                    egui::vec2(520., 570.),
                )
                .vscroll(false)
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Elige hasta diez destinos y confirma el reenvío.");
                    let row = dialog_action_height(ui);
                    let send_height = ui.fonts_mut(|fonts| fonts.layout(
                        "Reenviar a los destinos seleccionados".into(),
                        egui::TextStyle::Button.resolve(ui.style()), self.theme.ink,
                        (ui.available_width() - ui.spacing().button_padding.x * 2.).max(1.),
                    )).size().y + ui.spacing().button_padding.y * 2.;
                    let footer_height = send_height + row + 26. + ui.spacing().item_spacing.y * 2.;
                    let (mut content, mut footer) = dialog_content_and_footer(ui, footer_height);
                    let height = content.available_height().max(1.);
                    theme::vertical_scroll(&mut content).id_salt("forward-content")
                        .max_height(height).min_scrolled_height(0.).auto_shrink([false, false])
                        .show(&mut content, |ui| {
                    ui.add_enabled_ui(!locked,|ui|{
                    let old = self.forward_scope.clone();
                    egui::ComboBox::from_label("Destino")
                        .selected_text(if self.forward_scope.is_empty() {
                            "Conversaciones"
                        } else {
                            state
                                .guilds
                                .iter()
                                .find(|g| g.id == self.forward_scope)
                                .map(|g| g.name.as_str())
                                .unwrap_or("Servidor")
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.forward_scope,
                                String::new(),
                                "Conversaciones",
                            );
                            for guild in &state.guilds {
                                ui.selectable_value(
                                    &mut self.forward_scope,
                                    guild.id.clone(),
                                    &guild.name,
                                );
                            }
                        });
                    if self.forward_scope != old {
                        if self.forward_scope.is_empty() {
                            self.forward_targets = state.direct_messages.clone();
                        } else {
                            self.forward_targets.clear();
                            self.send("forward-channels", self.forward_scope.clone(), "", "", "");
                        }
                    }
                    ui.add(bordered(egui::TextEdit::singleline(&mut self.forward_query).hint_text("Buscar destino")));
                    let targets = std::mem::take(&mut self.forward_targets);
                    let forward_query = self.forward_query.to_lowercase();
                            for item in targets
                                .iter()
                                .filter(|n| n.name.to_lowercase().contains(&forward_query))
                            {
                                if matches!(item.kind.as_str(), "category" | "section") {
                                    continue;
                                }
                                let allowed = item
                                    .access
                                    .as_ref()
                                    .map(|a| a.can_view && a.can_send)
                                    .unwrap_or(true);
                                let mut selected = self.forward_selected.contains(&item.id);
                                ui.horizontal(|ui| {
                                    self.nav_avatar(ui, item, 32.);
                                    if ui
                                        .add_enabled(
                                            allowed
                                                && (selected || self.forward_selected.len() < 10),
                                            egui::Checkbox::new(&mut selected, &item.name),
                                        )
                                        .changed()
                                    {
                                        if selected {
                                            self.forward_selected.push(item.id.clone());
                                        } else {
                                            self.forward_selected.retain(|x| x != &item.id);
                                        }
                                    }
                                    if !allowed {
                                        ui.small("Sin permiso de envío");
                                    }
                                });
                            }
                    self.forward_targets = targets;
                    });
                    if self.forward_pending {ui.horizontal(|ui|{ui.spinner();ui.label("Reenviando mensaje…");});}
                    else if result["sent"].as_u64().is_some() {
                        let sent=result["sent"].as_u64().unwrap_or(0);let total=result["total"].as_u64().unwrap_or(self.forward_selected.len() as u64);
                        if result["partial"]==true {ui.label(format!("Se reenviaron {sent} de {total} destinos. Los envíos confirmados no se repetirán."));}
                        else if sent>0 {ui.colored_label(self.theme.accent,format!("Mensaje reenviado a {sent} destino(s)."));}
                    }
                    if !self.forward_pending && !s(&result,"error").is_empty(){ui.add(egui::Label::new(s(&result,"error")).wrap());}
                    });
                    footer.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if ui.button("Cerrar").clicked(){done=true;}
                    if ui
                        .add_enabled_ui(!locked && !self.forward_selected.is_empty() && message.channel_id==state.selected_channel_id,
                            |ui| ui.add_sized([ui.available_width(), send_height], egui::Button::new("Reenviar a los destinos seleccionados")))
                        .inner
                        .clicked()
                    {
                        let action=UiAction {
                                kind: "forward-many".into(),
                                id: message.id.clone(),
                                text: serde_json::to_string(&self.forward_selected)
                                    .unwrap_or_default(),
                                channel_id: message.channel_id.clone(),
                                ..Default::default()
                            };
                        self.forward_pending=true;
                        self.forward_result=serde_json::json!({"channel_id":message.channel_id,"message_id":message.id});
                        if self.command.send(action).is_err(){self.forward_pending=false;self.forward_result=serde_json::json!({"channel_id":message.channel_id,"message_id":message.id,"sent":0,"total":self.forward_selected.len(),"partial":false,"error":"El servicio de la aplicación se cerró."});}
                    }
                    ui.label(format!("{} / 10 destinos", self.forward_selected.len()));
                    });
                });
                if done{self.dialog=None;}
            }
            Dialog::Invite(code) => {
                let mut accepted = false;
                centered_dialog(
                    ctx,
                    "Revisar invitación en Discord",
                    "accept-invite-dialog",
                    egui::vec2(440., 250.),
                )
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    let guild = field(&self.dialog_data, "guild").unwrap_or(&self.dialog_data);
                    let name = s(guild, "name");
                    ui.heading(if name.is_empty() { "Invitación de servidor" } else { name });
                    ui.label("Discord abrirá esta invitación en tu navegador. Allí podrás revisarla y decidir si quieres unirte.");
                    if ui.button("Continuar en Discord").clicked() {
                        self.send("invite-accept", &code, "", "", "");
                        accepted = true;
                    }
                });
                if accepted {
                    self.dialog = None;
                }
            }
        }
        if !open {
            if let Some(backup) = self.settings_backup.take() {
                self.prefs = backup;
                self.theme = Theme::from_preferences(&self.prefs);
                self.theme.apply(ctx, &self.prefs);
            }
            self.dialog = None;
        }
    }
    fn refresh_search_suggestions(&mut self, state: &AppSnapshot) -> bool {
        use notdiscord_core::search::{self, SearchContext};
        let signature = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.search,
            self.search_cursor,
            state.selected_channel_id,
            state.presence_version,
            state.search_people.len(),
            state.channels.len(),
            state.notification_generation,
            state.current_user.as_ref().map_or("",|person|person.id.as_str())
        );
        let reset_scroll = signature != self.search_signature;
        if reset_scroll {
            let context = SearchContext {
                current_user_id: state
                    .current_user
                    .as_ref()
                    .map(|p| p.id.clone())
                    .unwrap_or_default(),
                people: state.search_people.iter().take(2000).cloned().collect(),
                channels: state
                    .channels
                    .iter()
                    .filter(|c| c.kind == "text" && c.access.as_ref().is_some_and(|a| a.can_view))
                    .take(2000)
                    .cloned()
                    .collect(),
            };
            self.search_suggestions = search::suggest(
                &self.search,
                search::char_to_byte(&self.search, self.search_cursor),
                &context,
                Some(chrono::Local::now().date_naive()),
            );
            self.search_signature = signature;
        }
        reset_scroll
    }
    fn draw_search_popup(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        if self.dialog.is_some() || self.picker.is_some() || self.viewer.is_some() || crate::diagnostics::is_open(ctx) {
            self.search_open = false;
        }
        let open_id = egui::Id::new("search-modal-was-open");
        let keyboard_id = egui::Id::new("search-keyboard");
        if !self.search_open {
            ctx.data_mut(|data| {data.remove::<bool>(open_id);data.remove::<SearchKeyboard>(keyboard_id);});
            return;
        }
        let newly_opened = ctx.data_mut(|data| {
            let previous = data.get_temp::<bool>(open_id).unwrap_or(false);
            data.insert_temp(open_id, true);
            !previous
        });
        use notdiscord_core::search;
        let scope=egui::Id::new((&state.selected_channel_id,state.notification_generation,
            state.current_user.as_ref().map(|person|person.id.as_str())));
        let mut keyboard=ctx.data(|data|data.get_temp::<SearchKeyboard>(keyboard_id)).unwrap_or_default();
        if newly_opened || keyboard.scope!=Some(scope) {
            keyboard=SearchKeyboard {scope:Some(scope),..Default::default()};
        }
        let ime_active=chat::composer_ime_active(ctx);
        let input_id=egui::Id::new("search-filter-input");
        let defer_header=newly_opened && ctx.data(|data|data.get_temp::<u64>(egui::Id::new("search-header-input-frame")))
            ==Some(ctx.cumulative_frame_nr());
        if newly_opened {
            let range=egui::TextEdit::load_state(ctx,egui::Id::new("message-search"))
                .and_then(|state|state.cursor.char_range())
                .unwrap_or_else(||egui::text::CCursorRange::one(egui::text::CCursor::new(self.search_cursor)));
            let mut edit=egui::TextEdit::load_state(ctx,input_id).unwrap_or_default();
            edit.cursor.set_char_range(Some(range));edit.store(ctx,input_id);
        }
        let keyboard_available=ctx.input(|input|input.focused && !input.pointer.any_down())
            && !egui::Popup::is_any_open(ctx);
        let focus=ctx.memory(|memory|memory.focused());
        if !defer_header && keyboard_available
            && (newly_opened || focus.is_none_or(|focus|focus==input_id) && ctx.input(search_editing_input)) {
            // Focus before TextEdit consumes this frame's input. The header
            // handoff is deferred when it already processed these same events.
            ctx.memory_mut(|memory|memory.request_focus(input_id));
        }
        let initial_focus=ctx.data(|data|data.get_temp::<(u64,Option<egui::Id>)>(egui::Id::new("composer-frame-focus")))
            .filter(|(frame,_)|*frame==ctx.cumulative_frame_nr()).and_then(|(_,focus)|focus);
        let editor_owned=ctx.memory(|memory|memory.has_focus(input_id)) || initial_focus==Some(input_id);
        let navigation=keyboard_available && editor_owned && !defer_header && !ime_active;
        let (down,up,tab,enter)=if navigation {ctx.input_mut(|input|(
            consume_search_key(input,egui::Key::ArrowDown),
            consume_search_key(input,egui::Key::ArrowUp),
            keyboard.selected.is_some() && consume_search_key(input,egui::Key::Tab),
            consume_search_key(input,egui::Key::Enter),
        ))}else{(false,false,false,false)};
        if down||up||tab||enter {
            // Focus traversal is computed from raw events at begin_pass.
            // Consuming Tab alone cannot undo it; keep this editor as owner
            // before it handles any text from the same batch.
            ctx.memory_mut(|memory|{memory.move_focus(egui::FocusDirection::None);memory.request_focus(input_id);});
        }
        let popup_rect = search_popup_rect(ctx.content_rect(), self.search_rect);
        let mut chosen = None;
        let popup = egui::Window::new("Filtros de búsqueda")
            .order(egui::Order::Foreground)
            .id(egui::Id::new("search-completion"))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .fixed_pos(popup_rect.min)
            .fixed_size(popup_rect.size())
            .constrain_to(ctx.content_rect().shrink(4.))
            .show(ctx, |ui| {
                let input = ui.add_sized([ui.available_width(), 32.], bordered(
                    egui::TextEdit::singleline(&mut self.search)
                        .id(input_id)
                        .hint_text("Escribe o combina filtros…"),
                ).focus_id(input_id));
                if defer_header { input.request_focus(); }
                self.search_focused = input.has_focus();
                if let Some(cursor) = egui::TextEdit::load_state(ctx, input.id)
                    .and_then(|state| state.cursor.char_range()) {
                    self.search_cursor = cursor.primary.index;
                }
                let reset_scroll=self.refresh_search_suggestions(state);
                if reset_scroll {keyboard.selected=None;}
                let suggestions=self.search_suggestions.clone();
                let basic=search::at(&self.search,search::char_to_byte(&self.search,self.search_cursor)).is_none();
                let more_id=ui.make_persistent_id("search-more-filters");
                let expanded=egui::collapsing_header::CollapsingState::load_with_default_open(ctx,more_id,false).is_open();
                let count=suggestions.len().min(if basic && !expanded {4}else{100});
                keyboard.selected=keyboard.selected.filter(|index|*index<count);
                if count>0 {
                    if down {keyboard.selected=Some(keyboard.selected.map_or(0,|index|(index+1).min(count-1)));}
                    if up {keyboard.selected=Some(keyboard.selected.map_or(count-1,|index|index.saturating_sub(1)));}
                }
                if (tab || enter) && let Some(suggestion)=keyboard.selected.and_then(|index|suggestions.get(index)) {
                    chosen=Some((suggestion.key.clone(),suggestion.value.clone()));
                }
                let submit=enter && chosen.is_none();
                ui.separator();
                ui.label(RichText::new("Filtros").strong().color(self.theme.muted));
                let footer_height=dialog_action_height(ui)+ui.spacing().item_spacing.y*3.+6.;
                let mut scroll = theme::vertical_scroll(ui)
                    .id_salt("search-filter-suggestions")
                    .max_height((ui.available_height() - footer_height).max(1.))
                    .auto_shrink([false, false]);
                if reset_scroll {
                    scroll = scroll.vertical_scroll_offset(0.);
                }
                scroll.show(ui, |ui| {
                    for (index, suggestion) in
                        suggestions
                            .iter()
                            .enumerate()
                            .take(if basic { 4 } else { 100 })
                    {
                        if index>0 {ui.separator();}
                        let response = self.search_suggestion_row(ui, suggestion, state, keyboard.selected==Some(index));
                        if (down||up) && keyboard.selected==Some(index) {response.scroll_to_me(Some(egui::Align::Center));}
                        if response.hovered() && ui.input(|input|input.pointer.delta()!=egui::Vec2::ZERO) {keyboard.selected=Some(index);}
                        if response.clicked() {
                            chosen = Some((suggestion.key.clone(), suggestion.value.clone()));
                        }
                    }
                    if basic {
                        ui.separator();
                        egui::CollapsingHeader::new(
                            "Más filtros · fechas, fijados y tipo de autor",
                        )
                        .id_salt("search-more-filters")
                        .show(ui, |ui| {
                            for (index,suggestion) in suggestions.iter().enumerate().skip(4).take(96) {
                                if index>4 {ui.separator();}
                                let response=self.search_suggestion_row(ui, suggestion, state, keyboard.selected==Some(index));
                                if (down||up) && keyboard.selected==Some(index) {response.scroll_to_me(Some(egui::Align::Center));}
                                if response.hovered() && ui.input(|input|input.pointer.delta()!=egui::Vec2::ZERO) {keyboard.selected=Some(index);}
                                if response.clicked() {
                                    chosen =
                                        Some((suggestion.key.clone(), suggestion.value.clone()));
                                }
                            }
                            ui.separator();
                            for (value, label) in [
                                ("user", "Personas"),
                                ("bot", "Bots"),
                                ("webhook", "Webhooks"),
                            ] {
                                if ui.button(format!("Tipo de autor: {label}")).clicked() {
                                    chosen = Some(("author".into(), value.into()));
                                }
                            }
                        });
                    }
                    if suggestions.is_empty() {
                        ui.label("Sin coincidencias. Escribe el nombre, ID o valor del filtro.");
                    }
                });
                ui.separator();
                if ui.button("Buscar con estos filtros").clicked() || submit {
                    self.send(
                        "search",
                        "",
                        self.search.clone(),
                        "",
                        &state.selected_channel_id,
                    );
                    self.search_open = false;
                }
            });
        if let Some((key, value)) = chosen {
            let (text, caret) = search::complete(
                &self.search,
                search::char_to_byte(&self.search, self.search_cursor),
                &key,
                &value,
            );
            self.search = text;
            self.search_cursor = search::byte_to_char(&self.search, caret);
            let id = egui::Id::new("search-filter-input");
            let mut edit = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
            edit.cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(self.search_cursor),
                )));
            edit.store(ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
            self.search_signature.clear();
            keyboard.selected=None;
        }
        if ctx.input(|i| !ime_active && i.key_pressed(egui::Key::Escape) || !i.focused) {
            self.search_open = false;
        }
        if let Some(popup) = popup {
            if ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| {
                        !popup.response.rect.contains(p) && !self.search_rect.contains(p)
                    })
            }) {
                self.search_open = false;
            }
        }
        ctx.data_mut(|data| {
            if self.search_open {data.insert_temp(keyboard_id,keyboard);}
            else {data.remove::<bool>(open_id);data.remove::<SearchKeyboard>(keyboard_id);}
        });
    }
    fn search_suggestion_row(
        &mut self,
        ui: &mut egui::Ui,
        suggestion: &notdiscord_core::search::SearchSuggestion,
        state: &AppSnapshot,
        selected: bool,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(1.), 56.),
            egui::Sense::click(),
        );
        let visuals = ui.style().interact(&response);
        ui.painter().rect_filled(rect, 5., if selected {self.theme.selected}else{visuals.weak_bg_fill});
        let person = if matches!(suggestion.key.as_str(), "from" | "mentions") {
            if suggestion.value == "me" {
                state.current_user.as_ref()
            } else {
                state
                    .search_people
                    .iter()
                    .find(|p| p.id == suggestion.value)
            }
        } else {
            None
        };
        let title = person
            .filter(|p| !p.name.is_empty())
            .map(|p| {
                if suggestion.value == "me" {
                    format!("Yo · {}", p.name)
                } else {
                    p.name.clone()
                }
            })
            .unwrap_or_else(|| suggestion.title.clone());
        let description = person
            .filter(|p| !p.username.is_empty())
            .map(|p| {
                if suggestion.value == "me" {
                    format!("@{} · Tu cuenta", p.username)
                } else {
                    format!("@{}", p.username)
                }
            })
            .unwrap_or_else(|| suggestion.description.clone());
        let mut left = rect.left() + 10.;
        if matches!(suggestion.key.as_str(), "from" | "mentions") && !suggestion.value.is_empty() {
            let avatar = egui::Rect::from_min_size(
                egui::pos2(left, rect.center().y - 18.),
                Vec2::splat(36.),
            );
            let placeholder = Person {
                id: suggestion.value.clone(),
                name: suggestion.title.clone(),
                ..Default::default()
            };
            self.pictures
                .paint_avatar(ui, avatar, person.unwrap_or(&placeholder));
            left = avatar.right() + 10.;
        }
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(left, rect.top()),
            rect.right_bottom() - egui::vec2(8., 0.),
        );
        let painter = ui
            .painter()
            .with_clip_rect(text_rect.intersect(ui.clip_rect()));
        painter.text(
            egui::pos2(left, rect.top() + 10.),
            egui::Align2::LEFT_TOP,
            &title,
            egui::FontId::proportional(15.),
            self.theme.ink,
        );
        painter.text(
            egui::pos2(left, rect.top() + 32.),
            egui::Align2::LEFT_TOP,
            &description,
            egui::FontId::proportional(12.),
            self.theme.muted,
        );
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("{title}\n{description}"))
    }
    fn person_selection_row(&mut self, ui: &mut egui::Ui, person: &Person) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().max(1.), 56.),
            egui::Sense::click(),
        );
        ui.painter()
            .rect_filled(rect, 5., ui.style().interact(&response).weak_bg_fill);
        let avatar = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 26., rect.center().y),
            Vec2::splat(36.),
        );
        self.pictures.paint_avatar(ui, avatar, person);
        let left = avatar.right() + 10.;
        let painter = ui.painter().with_clip_rect(
            egui::Rect::from_min_max(
                egui::pos2(left, rect.top()),
                rect.right_bottom() - egui::vec2(8., 0.),
            )
            .intersect(ui.clip_rect()),
        );
        painter.text(
            egui::pos2(left, rect.top() + 10.),
            egui::Align2::LEFT_TOP,
            &person.name,
            egui::FontId::proportional(15.),
            self.theme.ink,
        );
        let username = if person.username.is_empty() {
            "Perfil de usuario".into()
        } else {
            format!("@{}", person.username)
        };
        painter.text(
            egui::pos2(left, rect.top() + 32.),
            egui::Align2::LEFT_TOP,
            &username,
            egui::FontId::proportional(12.),
            self.theme.muted,
        );
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("{}\n{username}", person.name))
    }
    fn picker_scope(&self, state: &AppSnapshot) -> Value {
        serde_json::json!({"owner":state.current_user.as_ref().map_or("",|person|person.id.as_str()),"generation":state.notification_generation})
    }
    fn history_command(&mut self,kind:&str,mut extra:Value,state:&AppSnapshot){
        extra["owner"]=self.picker_scope(state)["owner"].clone();
        extra["generation"]=state.notification_generation.into();
        self.send(kind,"","",extra.to_string(),"");
    }
    pub(super) fn receive_picker_history(&mut self,data:&Value){
        if s(data,"owner")!=self.picker_account||data["generation"].as_u64()!=Some(self.state.notification_generation){return;}
        if let Ok(history)=serde_json::from_value::<PickerHistory>(data["history"].clone()){
            self.picker_history=history;
            if let Some(picker)=&mut self.picker{picker.items_dirty=true;picker.history_revision=data["revision"].as_u64().unwrap_or(0);}
        }
    }
    pub(super) fn receive_gif_catalog(&mut self,data:&Value){
        let Ok(stamp)=serde_json::from_value::<crate::pickers::CatalogRequest>(data["stamp"].clone())else{return};
        if stamp.owner!=self.picker_account||stamp.generation!=self.state.notification_generation{return;}
        let Some(picker)=&mut self.picker else{return};
        if !picker.gif||picker.open_id!=stamp.opening||picker.request_id!=stamp.request
            ||picker.category!=stamp.mode||picker.limit as usize!=stamp.limit{return;}
        picker.pending=false;
        if picker.query!=stamp.query{return;}
        let error=s(data,"error");
        if !error.is_empty(){picker.note=error.into();return;}
        let Ok(page)=serde_json::from_value::<crate::pickers::GifPage>(data["page"].clone())else{picker.note="El catálogo no tiene un formato válido. Reintenta.".into();return};
        for item in &page.items{resources::remember_source(&item.url,&item.animation_url);}
        picker.remote_items=page.items.into_iter().filter_map(|item|serde_json::to_value(item).ok()).collect();
        picker.categories=page.categories.into_iter().filter_map(|item|serde_json::to_value(item).ok()).collect();
        picker.note=page.note;picker.has_more=page.has_more;picker.items_revision=picker.items_revision.wrapping_add(1);picker.items_dirty=true;
    }
    pub(super) fn receive_gif_favorites(&mut self,data:&Value){
        if s(data,"owner")!=self.picker_account||data["generation"].as_u64()!=Some(self.state.notification_generation){return;}
        if let Ok(page)=serde_json::from_value::<crate::pickers::GifPage>(data["page"].clone()){
            self.remote_gif_favorites=page.items.into_iter().take(400).filter_map(|item|serde_json::to_value(item).ok()).collect();
            if let Some(picker)=&mut self.picker{picker.items_dirty=true;}
        }
    }
    pub(super) fn receive_gif_favorite_result(&mut self,data:&Value){
        if s(data,"owner")!=self.picker_account||data["generation"].as_u64()!=Some(self.state.notification_generation){return;}
        let id=egui::Id::new(("gif-removal",&self.picker_account,self.state.notification_generation));
        let matches=self.ctx.data(|memory|memory.get_temp::<(String,String)>(id)).is_some_and(|(url,operation)|url==s(data,"url")&&operation==s(data,"operation"));
        if !matches{return;}
        self.ctx.data_mut(|memory|memory.remove::<(String,String)>(id));
        if !s(data,"error").is_empty(){self.error=s(data,"error").into();}
    }
    fn request_gif_catalog(&mut self,picker:&mut Picker,state:&AppSnapshot){
        picker.request_id=uuid::Uuid::new_v4().to_string();picker.pending=true;
        let stamp=crate::pickers::CatalogRequest{
            owner:state.current_user.as_ref().map_or("",|person|person.id.as_str()).into(),generation:state.notification_generation,
            opening:picker.open_id.clone(),request:picker.request_id.clone(),mode:picker.category.clone(),query:picker.query.clone(),limit:picker.limit as usize,
        };
        self.send("gif-catalog","",stamp.query.clone(),serde_json::to_string(&stamp).unwrap_or_default(),&state.selected_channel_id);
    }
    fn rebuild_gif_items(&self,picker:&mut Picker){
        if !picker.items_dirty{return;}
        let mut values=match picker.category.as_str(){
            "recent"=>self.picker_history.gif_recent.iter().filter_map(|value|serde_json::to_value(value).ok()).collect(),
            "favorites"=>self.remote_gif_favorites.clone(),
            _=>picker.remote_items.clone(),
        };
        picker.remote_favorites=self.remote_gif_favorites.iter().map(|value|s(value,"Url").to_owned()).collect();
        if picker.category=="favorites"{
            for value in &self.picker_history.gif_favorites{
                if self.picker_history.gif_remote_removals.iter().any(|entry|entry.confirmed&&value.descriptor().matches_url(&entry.url)){continue;}
                if !values.iter().any(|old|gif_value_descriptor(old).same_medium(&value.descriptor())){values.push(serde_json::to_value(value).unwrap_or_default());}
            }
        }
        let mut seen=std::collections::HashSet::new();
        values.retain(|value|seen.insert(gif_value_descriptor(value).logical_id));
        for item in &mut values{
            let descriptor=gif_value_descriptor(item);
            let remote=self.remote_gif_favorites.iter().find(|value|gif_value_descriptor(value).same_medium(&descriptor)).map(|value|s(value,"Url")).unwrap_or("");
            let local=self.picker_history.gif_favorites.iter().any(|value|value.descriptor().same_medium(&descriptor));
            item["NotDiscordSaved"]=Value::Bool(local||!remote.is_empty());
            item["NotDiscordRemoteUrl"]=remote.into();
            if let Some(animation)=descriptor.animation_url{resources::remember_source(&gif_picker_value(item).value,&animation);}
        }
        picker.items=values;picker.items_dirty=false;
    }
    pub(super) fn open_picker(&mut self,gif:bool,state:&AppSnapshot){
        if let Some(importer)=&mut self.txt_import{importer.cancel();}
        let mut picker=Picker{gif,category:if gif{"trending".into()}else{"all".into()},limit:20,open_id:uuid::Uuid::new_v4().to_string(),items_dirty:true,..Default::default()};
        self.history_command("picker-history",serde_json::json!({}),state);
        if gif{
            // Read remote stars independently; its response never replaces the
            // active mode's catalog. Shared backend cache/coalescing bounds it.
            let mut favorites=Picker{gif:true,category:"favorites".into(),limit:96,open_id:uuid::Uuid::new_v4().to_string(),..Default::default()};
            self.request_gif_catalog(&mut favorites,state);
            self.request_gif_catalog(&mut picker,state);
        }
        self.picker=Some(picker);
    }
    fn pick(
        &mut self,
        value: PickerValue,
        gif: bool,
        state: &AppSnapshot,
        react: Option<(String, String)>,
    ) {
        if let Some((channel, message)) = react {
            self.send("react-toggle", message, &value.value, "", &channel);
        } else {
            let editor_id = egui::Id::new(("composer", &state.selected_channel_id));
            let selection = egui::TextEdit::load_state(&self.ctx, editor_id)
                .and_then(|editor| editor.cursor.char_range())
                .map(|range| (range.primary.index.min(range.secondary.index),
                    range.primary.index.max(range.secondary.index)));
            let fallback = self.composer_cursor;
            if self.draft(&state.selected_channel_id).is_none() { return; }
            let mut draft = self.drafts.get(&state.selected_channel_id).cloned().unwrap_or_default();
            let previous_draft = draft.clone();
            let mut chars: Vec<_> = draft.text.chars().collect();
            let (start, end) = selection.unwrap_or((fallback, fallback));
            let start = start.min(chars.len());
            let end = end.min(chars.len()).max(start);
            let mut insertion = String::new();
            if gif && start > 0 && !chars[start - 1].is_whitespace() { insertion.push(' '); }
            insertion.push_str(&value.value);
            if gif && end < chars.len() && !chars[end].is_whitespace() { insertion.push(' '); }
            let cursor = start + insertion.chars().count();
            chars.splice(start..end, insertion.chars());
            let previous = std::mem::replace(&mut draft.text, chars.into_iter().collect());
            super::mentions::adjust(&previous, &draft.text, &mut draft.mentions);
            draft.revision += 1;
            if !self.accepts_draft_edit(&previous_draft,&draft,Some(&state.selected_channel_id),0) {
                self.error = super::text_budget::ERROR.into();
                return;
            }
            self.drafts.insert(state.selected_channel_id.clone(),draft);
            self.composer_cursor = cursor;
            let mut editor = egui::TextEdit::load_state(&self.ctx, editor_id).unwrap_or_default();
            editor.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(cursor))));
            editor.store(&self.ctx, editor_id);
        }
        self.history_command("picker-recent",serde_json::json!({"value":value,"gif":gif}),state);
    }
    pub(super) fn draw_picker(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        let owner = state.current_user.as_ref().map_or("",|person|person.id.as_str());
        let picker_visible = self.picker.as_ref().is_some_and(|picker|picker.gif);
        if let Some(result) = self.txt_import.as_mut().and_then(|importer|
            importer.poll(owner,state.notification_generation,picker_visible)) {
            match result {
                Ok(values) => {
                    self.history_command("picker-import",serde_json::json!({"values":values}),state);
                }
                Err(error) => self.error = error,
            }
        }
        let Some(mut picker) = self.picker.take() else {
            if let Some(importer) = &mut self.txt_import { importer.cancel(); }
            self.emojis = Vec::new();
            return;
        };
        let mut open = !ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let mut choice = None;
        let mut request = false;
        egui::Window::new(if picker.gif { "GIF" } else { "Emojis" })
            .id(egui::Id::new("content-picker"))
            .order(egui::Order::Foreground)
            .open(&mut open)
            .default_size([if picker.gif { 620. } else { 610. }, 580.])
            .min_size([540., 420.])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.heading(if picker.gif {
                    "Busca un GIF"
                } else {
                    "Elige un emoji"
                });
                ui.horizontal(|ui| {
                    let response = ui.add(bordered(egui::TextEdit::singleline(&mut picker.query)));
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        picker.category = "search".into();
                        request = true;
                    }
                    if ui.button("Buscar").clicked() {
                        picker.category = "search".into();
                        request = true;
                    }
                });
                if picker.gif {
                    ui.horizontal_wrapped(|ui| {
                        for (key, name) in [
                            ("trending", "Populares"),
                            ("categories", "Categorías"),
                            ("recent", "Recientes"),
                            ("favorites", "Guardados"),
                        ] {
                            if ui.selectable_label(picker.category == key, name).clicked() {
                                picker.category = key.into();
                                picker.limit = 20;
                                picker.items.clear();picker.remote_items.clear();picker.categories.clear();picker.items_dirty=true;
                                request = true;
                            }
                        }
                        let importing = self.txt_import.as_ref().is_some_and(super::txt_import::Importer::pending);
                        if ui.add_enabled(!importing,egui::Button::new("Importar enlaces"))
                            .on_hover_text("Archivo TXT UTF-8 de hasta 256 KiB. Se lee en segundo plano; un error conserva los favoritos.").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Lista de enlaces", &["txt"])
                                .pick_file()
                            {
                                if self.txt_import.is_none() {
                                    match super::txt_import::Importer::new(ctx.clone()) {
                                        Ok(importer) => self.txt_import = Some(importer),
                                        Err(error) => self.error = error,
                                    }
                                }
                                if let Some(importer) = &mut self.txt_import {
                                    if let Err(error) = importer.start(path,owner,state.notification_generation) { self.error = error; }
                                }
                            }
                        }
                        if importing {
                            ui.spinner(); ui.label("Importando enlaces…");
                            if ui.button("Cancelar importación").clicked()
                                && let Some(importer) = &mut self.txt_import { importer.cancel(); }
                        }
                    });
                    self.rebuild_gif_items(&mut picker);
                    if picker.pending {
                        ui.spinner();
                    }
                    let rows = picker.items.len().div_ceil(3);
                    theme::vertical_scroll(ui)
                        .id_salt("gif-items")
                        .max_height(400.)
                        .show_rows(ui, 130., rows, |ui, range| {
                            for row in range {
                                ui.horizontal(|ui| {
                                    for item in picker.items.iter().skip(row * 3).take(3) {
                                        let url = if s(item, "Url").is_empty() {
                                            s(item, "Value")
                                        } else {
                                            s(item, "Url")
                                        };
                                        let thumb = if s(item, "ThumbnailUrl").is_empty() {
                                            if s(item, "ImageUrl").is_empty() {
                                                url
                                            } else {
                                                s(item, "ImageUrl")
                                            }
                                        } else {
                                            s(item, "ThumbnailUrl")
                                        };
                                        let name = if s(item, "Title").is_empty() {
                                            s(item, "Name")
                                        } else {
                                            s(item, "Title")
                                        };
                                        ui.vertical(|ui| {
                                            if self
                                                .pictures
                                                .image(ui, thumb, egui::vec2(170., 95.), false)
                                                .on_hover_text(name)
                                                .clicked()
                                            {
                                                choice = Some(gif_picker_value(item));
                                            }
                                            let remote_url=(!s(item,"NotDiscordRemoteUrl").is_empty()).then(||s(item,"NotDiscordRemoteUrl").to_owned());
                                            let favorite=field(item,"NotDiscordSaved").and_then(Value::as_bool).unwrap_or(false);
                                            let pending_id=egui::Id::new(("gif-removal",&self.picker_account,state.notification_generation));
                                            let removing=self.ctx.data(|memory|memory.get_temp::<(String,String)>(pending_id).is_some());
                                            if ui.add_enabled(!removing,egui::Button::new(if favorite{"★ Guardado"}else{"☆ Guardar"}).small())
                                                .on_hover_text(if remote_url.is_some(){"Quitar de Discord y del guardado local"}else{"Guardar o quitar para esta cuenta de NotDiscord"}).clicked(){
                                                if let Some(remote_url)=remote_url{
                                                    let operation=uuid::Uuid::new_v4().to_string();
                                                    self.ctx.data_mut(|memory|memory.insert_temp(pending_id,(remote_url.clone(),operation.clone())));
                                                    let mut scope=self.picker_scope(state);scope["operation"]=operation.into();let extra=scope.to_string();
                                                    self.send("gif-favorite-remove",remote_url,"",extra,"");
                                                }else{
                                                    let value=gif_picker_value(item);
                                                    self.history_command("picker-favorite",serde_json::json!({"value":value,"saved":!favorite}),state);
                                                }
                                            }
                                        });
                                    }
                                });
                            }
                        });
                    for category in &picker.categories {
                        let name = s(category, "Name");
                        if ui.button(name).clicked() {
                            picker.query = name.into();
                            picker.category = "search".into();
                            request = true;
                        }
                    }
                    if !matches!(picker.category.as_str(),"recent"|"favorites") && picker.has_more && !picker.pending && picker.limit < 96 && ui.button("Cargar más").clicked() {
                        picker.limit = if picker.limit < 50 { 50 } else { 96 };
                        request = true;
                    }
                    ui.label(
                        RichText::new(&picker.note)
                            .size(12.)
                            .color(self.theme.muted),
                    );
                } else {
                    if !self.picker_history.emoji_recent.is_empty() {
                        ui.label(
                            RichText::new("USADOS RECIENTEMENTE")
                                .size(12.)
                                .color(self.theme.muted),
                        );
                        let recent = self.picker_history.emoji_recent.clone();
                        self.emoji_section(ui,"recent-emoji-section",|this,ui|ui.horizontal_wrapped(|ui| {
                            for value in recent {
                                let response = if value.image_url.is_empty() {
                                    this.emoji_button(ui, &value.value, 26.)
                                } else if let Some(emoji)=custom_text::emoji_token(&value.value,state) {
                                    this.emoji_at_resolution(ui,&emoji,30.,48).interact(egui::Sense::click())
                                } else {
                                    this.pictures.image(
                                        ui,
                                        &value.image_url,
                                        Vec2::splat(30.),
                                        false,
                                    )
                                };
                                if response.on_hover_text(&value.name).clicked() {
                                    choice = Some(value);
                                }
                            }
                        }));
                        ui.separator();
                    }
                    let previous = picker.guild.clone();
                    egui::ComboBox::from_label("Emojis")
                        .selected_text(if picker.guild.is_empty() {
                            "Windows · Unicode"
                        } else {
                            state
                                .guilds
                                .iter()
                                .find(|g| g.id == picker.guild)
                                .map(|g| g.name.as_str())
                                .unwrap_or("Servidor")
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut picker.guild,
                                String::new(),
                                "Windows · Unicode",
                            );
                            for guild in &state.guilds {
                                ui.selectable_value(
                                    &mut picker.guild,
                                    guild.id.clone(),
                                    &guild.name,
                                );
                            }
                        });
                    if previous != picker.guild && !picker.guild.is_empty() {
                        picker.items.clear();
                        self.send("guild-emojis", &picker.guild, "", "", "");
                        picker.pending = true;
                    }
                    if picker.guild.is_empty() {
                        if self.emojis.is_empty() {
                            self.emojis = load_emoji_catalog();
                        }
                        // Keep the catalogue outside self while drawing buttons;
                        // filtering can borrow rows instead of cloning every label.
                        let emojis = std::mem::take(&mut self.emojis);
                        let mut categories = Vec::new();
                        for (_, category, _) in &emojis {
                            if !categories.contains(category) { categories.push(*category); }
                        }
                        categories.sort_unstable();
                        egui::ComboBox::from_label("Categoría")
                            .selected_text(&picker.category)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut picker.category, "all".into(), "Todos");
                                for category in categories {
                                    ui.selectable_value(
                                        &mut picker.category,
                                        category.into(),
                                        category,
                                    );
                                }
                            });
                        let query = picker.query.to_lowercase();
                        let show_all = query.is_empty() && matches!(picker.category.as_str(), "all" | "search");
                        let filtered = (!show_all).then(|| emojis
                            .iter()
                            .filter(|e| {
                                (picker.category == "all"
                                    || picker.category == "search"
                                    || e.1 == picker.category)
                                    && (query.is_empty()
                                        || e.0.contains(&query)
                                        || e.2.to_lowercase().contains(&query))
                            })
                            .collect::<Vec<_>>());
                        let count = filtered.as_ref().map_or(emojis.len(), Vec::len);
                        theme::vertical_scroll(ui)
                            .id_salt("unicode-items")
                            .max_height(360.)
                            .show_rows(ui, 39., count.div_ceil(11), |ui, range| {
                                for row in range {
                                    ui.horizontal(|ui| {
                                        for index in row * 11..((row + 1) * 11).min(count) {
                                            let e = filtered.as_ref().map_or(&emojis[index], |items| items[index]);
                                            let response = self.emoji_button(ui, &e.0, 26.);
                                            if response.on_hover_ui(|ui| {
                                                for part in e.2.split('\t') { ui.label(part); }
                                            }).clicked() {
                                                choice = Some(PickerValue {
                                                    value: e.0.into(),
                                                    name: short(&e.2.replace('\t', " · "), 150),
                                                    image_url: String::new(),
                                                    ..Default::default()
                                                });
                                            }
                                        }
                                    });
                                }
                            });
                        self.emojis = emojis;
                    } else {
                        if picker.pending {
                            ui.spinner();
                        }
                        let query = picker.query.to_lowercase();
                        self.emoji_section(ui,("catalog-emoji-section",&picker.guild),|this,ui|theme::vertical_scroll(ui)
                            .max_height(360.)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    for item in picker
                                        .items
                                        .iter()
                                        .filter(|e| s(e, "Name").to_lowercase().contains(&query))
                                    {
                                        let id = s(item, "Id");
                                        let name = s(item, "Name");
                                        let animated = field(item, "Animated")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false);
                                        let url = format!(
                                            "https://cdn.discordapp.com/emojis/{}.webp?size=48",
                                            id
                                        );
                                        if this.emoji_at_resolution(ui,&StatusEmoji {id:id.into(),name:name.into(),animated},34.,48)
                                            .interact(egui::Sense::click())
                                            .on_hover_text(name)
                                            .clicked()
                                        {
                                            choice = Some(PickerValue {
                                                value: format!(
                                                    "<{}:{}:{}>",
                                                    if animated { "a" } else { "" },
                                                    name,
                                                    id
                                                ),
                                                name: name.into(),
                                                image_url: url,
                                                ..Default::default()
                                            });
                                        }
                                    }
                                });
                            }));
                    }
                }
            });
        if request && picker.gif {
            picker.remote_items.clear();picker.categories.clear();picker.items_dirty=true;picker.has_more=false;
            if picker.category=="recent"{
                picker.pending=false;picker.request_id.clear();
                let mut cancel=self.picker_scope(state);cancel["opening"]=picker.open_id.clone().into();
                self.send("gif-catalog-cancel","","",cancel.to_string(),"");
            }else{self.request_gif_catalog(&mut picker,state);}
        }
        if let Some(choice) = choice {
            let closes = picker.gif || picker.react.is_some();
            self.pick(choice, picker.gif, state, picker.react.clone());
            if closes { open = false; }
        }
        if open {
            self.picker = Some(picker);
        } else {
            let mut cancel=self.picker_scope(state);cancel["opening"]=picker.open_id.clone().into();
            self.send("gif-catalog-cancel","","",cancel.to_string(),"");
            self.emojis=Vec::new();
            if let Some(importer)=&mut self.txt_import{importer.cancel();}
        }
    }
    pub(super) fn draw_viewer(&mut self, ctx: &egui::Context) {
        let Some(mut viewer) = self.viewer.take() else { return; };
        let mut open = !ctx.input(|input| input.key_pressed(egui::Key::Escape));
        if open {
            centered_dialog(ctx, "Imagen / GIF", "media-viewer", egui::vec2(940., 730.))
                .vscroll(false)
                .open(&mut open)
                .resizable(true)
                .show(ctx, |ui| self.viewer_content(ui, &mut viewer));
        }
        if ctx.data_mut(|data| data.remove_temp::<bool>(egui::Id::new("close-viewer-for-image-settings"))).unwrap_or(false) {
            open = false;
        }
        if open {
            self.viewer = Some(viewer);
        } else {
            self.pictures.stop();
            self.pictures.release_viewer(&viewer.url, viewer.local_path.as_deref());
            if let Some(path) = viewer.local_path { self.cleanup_unreferenced_temporary(&[path]); }
        }
    }
    fn viewer_content(&mut self, ui: &mut egui::Ui, viewer: &mut Viewer) {
        let ctx = ui.ctx().clone();
        let texture = if let Some(path) = &viewer.local_path {
            self.pictures.texture_path(path, 1600)
        } else {
            self.pictures.animated(&ctx, &viewer.url)
                .or_else(|| self.pictures.texture_sized(&viewer.url, 1600))
        };
        let failed = if let Some(path) = &viewer.local_path {
            self.pictures.path_error(path, 1600)
        } else {
            self.pictures.image_error(&viewer.url, 1600)
        }.map(str::to_owned);
        let feedback = media_feedback(&ctx)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let busy = feedback.url == viewer.url && feedback.busy;
        let mut zoom_factor = 1.;
        let mut fit = false;
        let mut original = false;
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(texture.is_some(), |ui| {
                if ui.button("−").on_hover_text("Alejar").clicked() {
                    zoom_factor = 1. / 1.2;
                }
                if ui.button("+").on_hover_text("Acercar").clicked() {
                    zoom_factor = 1.2;
                }
                if ui.button("Ajustar").clicked() {
                    fit = true;
                }
                if ui.button("100 %").clicked() {
                    original = true;
                }
            });
            if let Some(path) = &viewer.local_path {
                let name = if self.owned_temporary.contains(path) { "Imagen pegada.png".into() }
                    else { path.file_name().unwrap_or_default().to_string_lossy() };
                ui.label(format!("{name} · adjunto sin enviar"));
            } else {
            if ui
                .add_enabled(!busy, egui::Button::new("Guardar original…"))
                .clicked()
            {
                self.save_media(&viewer.url, "imagen");
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Copiar imagen"))
                .clicked()
            {
                self.copy_image(&viewer.url);
            }
            if ui.button("Copiar enlace").clicked() {
                ctx.copy_text(viewer.url.clone());
                if !busy {
                    set_media_feedback(&ctx, &viewer.url, "Enlace copiado", false, false);
                }
            }
            if ui.button("Abrir original").clicked() {
                if crate::platform::open_link(&viewer.url).is_err() {
                    set_media_feedback(
                        &ctx,
                        &viewer.url,
                        "No se pudo abrir el original.",
                        false,
                        true,
                    );
                }
            }
            if notdiscord_core::parsing::direct_gif(&viewer.url)
                || notdiscord_core::parsing::gif_page(&viewer.url)
            {
                let playing = self
                    .pictures
                    .animation
                    .as_ref()
                    .is_some_and(|a| a.url == viewer.url);
                if ui
                    .add_enabled(
                        self.prefs.allow_gif_playback && !self.pictures.animation_loading(&viewer.url),
                        egui::Button::new(if playing {
                            "Ⅱ Pausar GIF"
                        } else if self.pictures.animation_loading(&viewer.url) {
                            "Cargando GIF…"
                        } else {
                            "▶ Reproducir GIF"
                        }),
                    )
                    .clicked()
                {
                    if playing {
                        self.pictures.stop();
                    } else {
                        self.pictures.play(&ctx, viewer.url.clone());
                    }
                }
            }
            }
        });
        if let Some(error)=viewer.local_path.is_none().then(|| self.pictures.animation_error(&viewer.url)).flatten().map(str::to_owned) {
            if self.media_error_link(ui, &error) {
                ctx.data_mut(|data| data.insert_temp(egui::Id::new("close-viewer-for-image-settings"), true));
            }
        }
        if viewer.local_path.is_none() && feedback.url == viewer.url && !feedback.text.is_empty() {
            ui.horizontal(|ui| {
                if feedback.busy {
                    ui.spinner();
                }
                if super::media_limit::is_limit_error(&feedback.text) {
                    if self.media_error_link(ui, &feedback.text) {
                        ctx.data_mut(|data| data.insert_temp(egui::Id::new("close-viewer-for-image-settings"), true));
                    }
                } else { ui.colored_label(
                    if feedback.error {
                        ui.visuals().error_fg_color
                    } else {
                        self.theme.muted
                    },
                    &feedback.text,
                ); }
            });
        }
        if let Some(error) = failed.as_ref().filter(|_| texture.is_none()) {
            ui.horizontal_wrapped(|ui| {
                if self.media_error_link(ui, error) {
                    ctx.data_mut(|data| data.insert_temp(egui::Id::new("close-viewer-for-image-settings"), true));
                }
                if ui.button("Reintentar").clicked() {
                    if let Some(path) = &viewer.local_path { self.pictures.retry_path(path, 1600); }
                    else { self.pictures.retry(&viewer.url, 1600); }
                }
            });
        }
        ui.separator();
        let extent = ui.available_size().max(Vec2::splat(1.));
        let (canvas, response) = ui.allocate_exact_size(extent, egui::Sense::click_and_drag());
        let painter = ui
            .painter()
            .with_clip_rect(canvas.intersect(ui.clip_rect()));
        painter.rect_filled(canvas, 4., self.theme.rail);
        if let Some(texture) = texture {
            let natural = texture.size_vec2();
            if fit || response.double_clicked() {
                viewer.zoom = 1.;
                viewer.pan = Vec2::ZERO;
            } else if original {
                let requested = 1. / viewer_fit(natural, canvas.size());
                zoom_viewer(viewer, natural, canvas.size(), requested, Vec2::ZERO);
            } else if zoom_factor != 1. {
                let requested = viewer.zoom * zoom_factor;
                zoom_viewer(viewer, natural, canvas.size(), requested, Vec2::ZERO);
            }
            if response.hovered() {
                let scroll = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll != 0. {
                    let anchor = ctx
                        .input(|i| i.pointer.hover_pos())
                        .unwrap_or(canvas.center())
                        - canvas.center();
                    let requested = viewer.zoom * (scroll * 0.002).exp();
                    zoom_viewer(viewer, natural, canvas.size(), requested, anchor);
                }
            }
            if response.dragged() {
                viewer.pan += ctx.input(|i| i.pointer.delta());
            }
            clamp_viewer_pan(viewer, natural, canvas.size());
            let scale = viewer_fit(natural, canvas.size()) * viewer.zoom;
            let rect = egui::Rect::from_center_size(canvas.center() + viewer.pan, natural * scale);
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
            let caption = format!(
                "{:.0} % · arrastra para desplazar · rueda para ampliar",
                scale * 100.
            );
            painter.text(
                canvas.left_bottom() + egui::vec2(10., -10.),
                egui::Align2::LEFT_BOTTOM,
                caption,
                egui::FontId::proportional(12.),
                self.theme.muted,
            );
            response.clone().on_hover_cursor(if response.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        } else {
            painter.text(
                canvas.center(),
                egui::Align2::CENTER_CENTER,
                if failed.is_some() {
                    "Imagen no disponible"
                } else {
                    "Cargando imagen…"
                },
                egui::FontId::proportional(16.),
                self.theme.muted,
            );
            if failed.is_none() {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }
    pub(super) fn copy_image(&mut self, url: &str) {
        let limit = self.prefs.image_download_limit_bytes();
        let url = url.to_string();
        let ctx = self.ctx.clone();
        let command = self.command.clone();
        let Some(slot) = begin_media_operation(&ctx, &url, "Copiando imagen…") else {
            return;
        };
        std::thread::spawn(move || {
            let result = resources::decode_original(&url, limit)
                .and_then(|(w, h, p)| crate::platform::copy_image(w, h, &p));
            if let Err(error) = &result {
                let _ = command.send(UiAction {
                    kind: "local-error".into(),
                    text: error.clone(),
                    ..Default::default()
                });
            }
            let mut feedback = slot.lock().unwrap_or_else(|e| e.into_inner());
            if feedback.url == url {
                feedback.busy = false;
                feedback.error = result.is_err();
                feedback.text = result
                    .err()
                    .unwrap_or_else(|| "Imagen copiada al portapapeles".into());
            }
            drop(feedback);
            ctx.request_repaint();
        });
    }
    pub(super) fn save_media(&mut self, url: &str, name: &str) {
        let limit = self.prefs.image_download_limit_bytes();
        let name = media_file_name(url, name);
        let url = url.to_string();
        let ctx = self.ctx.clone();
        let command = self.command.clone();
        let Some(slot) = begin_media_operation(&ctx, &url, "Elige dónde guardar el original…")
        else {
            return;
        };
        std::thread::spawn(move || {
            let result = rfd::FileDialog::new()
                .set_title("Guardar imagen / GIF")
                .set_file_name(name)
                .add_filter(
                    "Imágenes y GIF",
                    &["png", "jpg", "jpeg", "gif", "webp", "bmp"],
                )
                .save_file()
                .map(|path| resources::save_original(&url, &path, limit));
            if let Some(Err(error)) = &result {
                let _ = command.send(UiAction {
                    kind: "local-error".into(),
                    text: error.clone(),
                    ..Default::default()
                });
            }
            let mut feedback = slot.lock().unwrap_or_else(|e| e.into_inner());
            if feedback.url == url {
                feedback.busy = false;
                feedback.error = result.as_ref().is_some_and(Result::is_err);
                feedback.text = match result {
                    Some(Ok(())) => "Original guardado".into(),
                    Some(Err(error)) => error,
                    None => "Guardado cancelado".into(),
                };
            }
            drop(feedback);
            ctx.request_repaint();
        });
    }
}

fn gif_picker_value(value:&Value)->PickerValue{
    let url=if s(value,"Url").is_empty(){s(value,"Value")}else{s(value,"Url")};
    let thumbnail=if s(value,"ThumbnailUrl").is_empty(){s(value,"ImageUrl")}else{s(value,"ThumbnailUrl")};
    let title=if s(value,"Title").is_empty(){s(value,"Name")}else{s(value,"Title")};
    PickerValue{value:url.into(),name:short(title,150),image_url:thumbnail.into(),animation_url:s(value,"AnimationUrl").into(),
        width:field(value,"Width").and_then(Value::as_i64).unwrap_or(0).clamp(0,8192)as i32,
        height:field(value,"Height").and_then(Value::as_i64).unwrap_or(0).clamp(0,8192)as i32}
}
fn gif_value_descriptor(value:&Value)->notdiscord_core::parsing::MediaDescriptor{gif_picker_value(value).descriptor()}
