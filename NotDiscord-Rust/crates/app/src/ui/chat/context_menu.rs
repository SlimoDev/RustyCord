//! A message owns one popup. Its children contribute a target instead of
//! opening a competing egui popup, while retaining their original responses.
use super::*;

#[derive(Clone)]
pub(super) enum Target {
    Link(String),
    Media { url: String, poster: String, name: String },
    Person(Person),
}

#[derive(Clone)]
struct Selection {
    base: egui::Id,
    scope: egui::Id,
    frame: u64,
    target: Option<Target>,
}

#[derive(Clone, Copy)]
struct RowIdentity { base: egui::Id, scope: egui::Id }
#[derive(Clone, Copy)]
struct PopupState { id: egui::Id, frame: u64 }

fn active_key() -> egui::Id { egui::Id::new("message-context-active-row") }
fn selection_key() -> egui::Id { egui::Id::new("message-context-selection") }
fn scope_for(channel: &str, message: &str, view: &str, state: &AppSnapshot) -> egui::Id {
    egui::Id::new(("message-context", channel, message, view,
        state.notification_generation, state.current_user.as_ref().map(|person| person.id.as_str())))
}
fn scope_id(message: &ChatMessage, state: &AppSnapshot) -> egui::Id {
    scope_for(&message.channel_id, &message.id, &state.view, state)
}
pub(super) fn is_open(response: &egui::Response) -> bool {
    let frame = response.ctx.cumulative_frame_nr();
    let key = response.id.with("message-context-popup");
    let state = response.ctx.data(|data| data.get_temp::<PopupState>(key)).filter(|state| state.frame == frame);
    state.is_some_and(|state| egui::Popup::is_id_open(&response.ctx, state.id))
}

pub(super) struct Scope {
    ctx: egui::Context,
    previous: Option<RowIdentity>,
}
impl Scope {
    pub(super) fn begin(ui: &egui::Ui, message: &ChatMessage, state: &AppSnapshot) -> Self {
        let ctx = ui.ctx();
        let base = scope_id(message, state);
        let scope = if state.view == "history" { base.with(ui.id()) } else { base };
        let previous = ctx.data_mut(|data| {
            let previous = data.get_temp::<RowIdentity>(active_key());
            data.insert_temp(active_key(), RowIdentity { base, scope });
            previous
        });
        Self { ctx: ctx.clone(), previous }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        self.ctx.data_mut(|data| match self.previous {
            Some(previous) => data.insert_temp(active_key(), previous),
            None => { data.remove::<RowIdentity>(active_key()); },
        });
    }
}

/// Return true inside a message, including frames with no click: the caller
/// must then leave popup ownership to the row. Allocate target data only when
/// an actual contextual click selected it.
pub(super) fn capture(ui: &egui::Ui, response: &egui::Response, target: impl FnOnce() -> Target) -> bool {
    let Some(identity) = ui.ctx().data(|data| data.get_temp::<RowIdentity>(active_key())) else { return false; };
    if response.enabled() && response.secondary_clicked() && !interaction::cancelling(ui.ctx()) {
        let frame = ui.ctx().cumulative_frame_nr();
        let target = target();
        ui.ctx().data_mut(|data| data.insert_temp(selection_key(), std::sync::Arc::new(Selection {
            base: identity.base, scope: identity.scope, frame, target: Some(target) })));
    }
    true
}

pub(super) fn clear(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<RowIdentity>(active_key());
        data.remove::<std::sync::Arc<Selection>>(selection_key());
    });
}

impl NotDiscordUi {
    pub(in crate::ui) fn forget_deleted_message_context(&mut self, channel: &str, message: &str) {
        let scopes = ["chat", "pins", "search", "history"].map(|view| scope_for(channel, message, view, &self.state));
        let selected = self.ctx.data(|data| data.get_temp::<std::sync::Arc<Selection>>(selection_key()))
            .filter(|selection| scopes.contains(&selection.base));
        if let Some(selected) = selected {
            egui::Popup::close_id(&self.ctx, selected.scope.with("popup"));
            self.ctx.data_mut(|data| data.remove::<std::sync::Arc<Selection>>(selection_key()));
        }
        for scope in scopes { egui::Popup::close_id(&self.ctx, scope.with("popup")); }
    }
    pub(super) fn message_context_menu(&mut self, ui: &mut egui::Ui, response: &egui::Response,
        message: &ChatMessage, state: &AppSnapshot, copy_text: Option<&str>) {
        let Some(identity) = ui.ctx().data(|data| data.get_temp::<RowIdentity>(active_key())) else { return; };
        let scope = identity.scope;
        let frame = ui.ctx().cumulative_frame_nr();
        let popup_id = scope.with("popup");
        let popup_key = response.id.with("message-context-popup");
        ui.ctx().data_mut(|data| data.insert_temp(popup_key, PopupState { id: popup_id, frame }));
        let selection = ui.ctx().data(|data| data.get_temp::<std::sync::Arc<Selection>>(selection_key()))
            .filter(|selection| selection.scope == scope);
        let child_clicked = selection.as_ref().is_some_and(|selection| selection.frame == frame);
        let text_clicked = ui.ctx().data(|data| data.get_temp::<MessageTextGesture>(egui::Id::new("message-text-shortcuts")))
            .is_some_and(|gesture| gesture.secondary_clicked);
        let pointer_click = ui.ctx().input(|input|
            input.pointer.button_clicked(egui::PointerButton::Secondary).then(|| input.pointer.interact_pos()).flatten());
        // A child can consume the Response click (links, reactions, buttons).
        // Geometry is a fallback only on this row's actual, unobscured layer.
        let row_clicked = pointer_click.is_some_and(|pointer|
            response.rect.intersect(ui.clip_rect()).contains(pointer)
                && interaction::owns_surface(ui.ctx(), pointer, response.layer_id));
        let requested = ui.is_enabled() && !interaction::cancelling(ui.ctx())
            && (child_clicked || text_clicked || row_clicked || response.secondary_clicked());
        let target = if requested && !child_clicked {
            ui.ctx().data_mut(|data| data.insert_temp(selection_key(), std::sync::Arc::new(Selection { base: identity.base, scope, frame, target: None })));
            None
        } else { selection.and_then(|selection| selection.target.clone()) };
        // An asynchronously deleted message may still have an open popup from
        // its live media. Never reintroduce downloads or retry into a tombstone.
        let target = target.filter(|target| !message.deleted || !matches!(target, Target::Media { .. }));
        let command = if requested { Some(egui::SetOpenCommand::Bool(true)) }
            else if response.clicked() { Some(egui::SetOpenCommand::Bool(false)) } else { None };
        egui::Popup::context_menu(response).id(popup_id).open_memory(command).show(|ui| {
            match &target {
                Some(Target::Link(url)) => self.message_link_menu(ui, url),
                Some(Target::Media { url, poster, name }) => self.media_preview_menu(ui, url, poster, name),
                Some(Target::Person(person)) => self.people_menu(ui, person, state),
                None => {},
            }
            if target.is_some() { ui.separator(); }
            self.message_menu_with_text(ui, message, state, copy_text.unwrap_or(&message.content));
        });
    }

    fn message_link_menu(&mut self, ui: &mut egui::Ui, url: &str) {
        let supported = url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some() && url.username().is_empty() && url.password().is_none());
        if ui.add_enabled(supported, egui::Button::new("Abrir enlace")).clicked() {
            self.open_message_link(ui.ctx(), url);
            ui.close();
        }
        if ui.button("Copiar enlace").clicked() {
            ui.ctx().copy_text(url.to_owned());
            ui.close();
        }
    }

    pub(super) fn media_preview_menu(&mut self, ui: &mut egui::Ui, url: &str, poster: &str, name: &str) {
        if ui.button("Reintentar vista previa").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.pictures.retry(poster, 384);
            self.pictures.retry(url, 384);
            ui.close();
        }
        self.media_menu(ui, url, name);
    }
}
