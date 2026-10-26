//! Group DM removal is a distinct owner action, never server moderation.
use super::*;

fn can_remove(state: &AppSnapshot, target: &str) -> bool {
    let Some(me) = state.current_user.as_ref() else { return false; };
    state.signed_in && !state.demo && state.view == "chat"
        && state.network_safety == notdiscord_core::safety::SafetyState::Allowed
        && state.selected_guild_id.is_empty() && state.selected_channel_kind == "group"
        && notdiscord_core::parsing::valid_id(&state.selected_channel_id)
        && notdiscord_core::parsing::valid_id(&me.id)
        && notdiscord_core::parsing::valid_id(target)
        && state.group_owner_id == me.id && target != me.id
        && state.group_members.iter().any(|member| member.user.id == me.id)
        && state.group_members.iter().any(|member| member.user.id == target)
}

pub(super) fn action_error(action: &UiAction, state: &AppSnapshot) -> Option<String> {
    if action.kind != "remove-group-member" { return None; }
    if action.channel_id != state.selected_channel_id
        || action.extra.parse::<u64>().ok() != Some(state.member_context_version)
        || !can_remove(state, &action.id) {
        return Some("El grupo, su líder o sus miembros cambiaron. Vuelve a abrir la acción.".into());
    }
    if state.group_member_removal_pending_ids.contains(&action.id) {
        return Some("Espera a que se confirme la eliminación pendiente.".into());
    }
    None
}

impl NotDiscordUi {
    pub(super) fn draw_group_member_removal(&mut self, ui: &mut egui::Ui, person: &Person, state: &AppSnapshot) {
        if !can_remove(state, &person.id) { return; }
        ui.separator();
        let pending = state.group_member_removal_pending_ids.contains(&person.id);
        if ui.add_enabled(!pending, egui::Button::new(
            RichText::new("Eliminar del grupo").color(Color32::from_rgb(242, 63, 67)),
        )).clicked() {
            self.confirm(
                "Eliminar del grupo",
                &format!("¿Eliminar a {} de «{}»?", person.name, state.channel_name),
                UiAction { kind: "remove-group-member".into(), id: person.id.clone(),
                    channel_id: state.selected_channel_id.clone(), extra: state.member_context_version.to_string(),
                    ..Default::default() },
            );
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests;
