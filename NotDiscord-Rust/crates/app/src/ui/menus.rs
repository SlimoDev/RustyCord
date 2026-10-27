//! Stateless presentation helpers. Drawing never performs an action on behalf
//! of a caller; it only returns the response to the caller's fixed target.
use eframe::egui::{self, RichText};
use notdiscord_core::models::{AppSnapshot,NavItem,Person};

#[derive(Default)]
pub(super) struct Sections { populated: bool }
impl Sections {
    pub(super) fn new() -> Self { Self::default() }
    /// Call only for a group with at least one visible item. Conditional groups
    /// consequently have no leading, trailing or doubled separator.
    pub(super) fn section(&mut self, ui: &mut egui::Ui, present: bool) -> bool {
        if present {
            if self.populated { ui.separator(); }
            self.populated = true;
        }
        present
    }
}
pub(super) fn action(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(label))
}
pub(super) fn destructive(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).color(egui::Color32::from_rgb(242,63,67))))
}
pub(super) fn disabled(ui:&mut egui::Ui,label:&str,reason:&str)->egui::Response {
    action(ui,label,false).on_disabled_hover_text(reason)
}
pub(super) fn toggle(ui:&mut egui::Ui,value:&mut bool,label:&str)->egui::Response {ui.checkbox(value,label)}
pub(super) fn submenu(ui:&mut egui::Ui,label:&str,contents:impl FnOnce(&mut egui::Ui))->egui::InnerResponse<Option<()>> {
    ui.menu_button(label,contents)
}
#[derive(Clone)]
pub(super) enum Target {Navigation(NavItem),Person(Person)}
#[derive(Clone)]
struct Selection {popup:egui::Id,scope:egui::Id,target:Target}
pub(super) fn bind(response:&egui::Response,target:Target,state:&AppSnapshot)->Target {
    let ctx=&response.ctx;
    let key=egui::Id::new("context-menu-fixed-target");
    let popup=egui::Popup::default_response_id(response);
    let scope=egui::Id::new((state.notification_generation,state.current_user.as_ref().map(|user|user.id.as_str())));
    if response.enabled() && response.secondary_clicked() {
        requested(ctx,popup);
        ctx.data_mut(|data|data.insert_temp(key,Selection {popup,scope,target:target.clone()}));
        target
    } else {
        ctx.data(|data|data.get_temp::<Selection>(key))
            .filter(|selected|selected.popup==popup && selected.scope==scope && egui::Popup::is_id_open(ctx,popup))
            .map_or(target,|selected|selected.target)
    }
}

#[derive(Clone)]
struct Owner { popup: egui::Id, openings: u64 }
pub(super) fn clear(ctx:&egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<Owner>(egui::Id::new("context-menu-owner"));
        data.remove::<Selection>(egui::Id::new("context-menu-fixed-target"));
    });
    egui::Popup::close_all(ctx);
}
/// One retained owner, independent of how many different targets are clicked.
/// Resolve all Context metadata outside the data transaction (non-reentrant).
pub(super) fn requested(ctx: &egui::Context, popup: egui::Id) {
    let key=egui::Id::new("context-menu-owner");
    let previous=ctx.data(|data|data.get_temp::<Owner>(key));
    if let Some(previous)=previous.as_ref().filter(|old|old.popup!=popup) {
        egui::Popup::close_id(ctx,previous.popup);
    }
    let openings=previous.map_or(1,|old|old.openings.saturating_add(1));
    ctx.data_mut(|data|data.insert_temp(key,Owner {popup,openings}));
    notdiscord_core::diagnostics::record(notdiscord_core::diagnostics::Component::Menus,
        notdiscord_core::diagnostics::Code::MenuOpened,notdiscord_core::diagnostics::Phase::Present,0);
}
