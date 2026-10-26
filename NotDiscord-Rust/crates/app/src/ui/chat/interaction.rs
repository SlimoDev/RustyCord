//! Shared pointer ownership for every message view. Geometry alone cannot
//! distinguish text from an overlapping button, popup or foreground toolbar.
use super::*;

#[derive(Clone, Default)]
struct Gesture {
    scope: Option<egui::Id>,
    frame: u64,
    cancelling_middle: bool,
}
pub(super) fn suspend(ctx: &egui::Context) {
    ctx.data_mut(|data|data.remove::<Gesture>(egui::Id::new("history-gesture-owner")));
}

pub(super) fn begin(ctx: &egui::Context, scope: egui::Id, state: &AppSnapshot) {
    let identity = egui::Id::new((scope, state.notification_generation,
        state.current_user.as_ref().map(|person| person.id.as_str())));
    let frame = ctx.cumulative_frame_nr();
    let (pressed, down, released, focused, escape, wheel) = ctx.input(|input| (
        input.pointer.primary_pressed() || input.pointer.secondary_pressed(),
        input.pointer.primary_down() || input.pointer.secondary_down(),
        input.pointer.primary_released() || input.pointer.secondary_released(),
        input.focused, input.key_pressed(egui::Key::Escape),
        input.raw_scroll_delta != egui::Vec2::ZERO));
    ctx.data_mut(|data| {
        let key = egui::Id::new("history-gesture-owner");
        let mut gesture = data.get_temp::<Gesture>(key).unwrap_or_default();
        if gesture.scope != Some(identity) || frame > gesture.frame.saturating_add(1) {
            gesture = Gesture { scope: Some(identity), frame, ..Default::default() };
            data.remove::<MessageActionRegion>(egui::Id::new("message-actions-region"));
            data.remove::<HistorySelection>(egui::Id::new("history-selection"));
            data.remove::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"));
        }
        if !down && !released || !focused { gesture.cancelling_middle = false; }
        if let Some(mut middle) = data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll")) {
            if middle.origin.is_some() && (pressed || escape || wheel || !focused) {
                // A click dismissing middle-button scrolling belongs to that
                // gesture; its release must not activate a message underneath.
                gesture.cancelling_middle |= pressed && focused;
                middle.origin = None;
                data.insert_temp(egui::Id::new("history-middle-scroll"), middle);
            }
        }
        gesture.frame = frame;
        data.insert_temp(key, gesture);
    });
}

pub(super) fn cancelling(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<Gesture>(egui::Id::new("history-gesture-owner")))
        .is_some_and(|gesture| gesture.cancelling_middle)
}

pub(super) fn selecting(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<HistorySelection>(egui::Id::new("history-selection")))
        .is_some_and(|selection| selection.dragging)
}

pub(super) fn controls_allowed(ctx: &egui::Context) -> bool {
    !cancelling(ctx) && !selecting(ctx)
}

pub(super) fn over_panel(ctx: &egui::Context, pointer: egui::Pos2) -> bool {
    reaction_panel_rect(ctx).is_some_and(|rect| rect.contains(pointer))
}

pub(super) fn owns_surface(ctx: &egui::Context, pointer: egui::Pos2, layer: egui::LayerId) -> bool {
    !over_panel(ctx, pointer) && ctx.layer_id_at(pointer) == Some(layer)
}

pub(super) fn row_hover(ctx: &egui::Context, row: &egui::Response, key: egui::Id) -> bool {
    let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else { return false; };
    !over_panel(ctx, pointer) && matches!(ctx.layer_id_at(pointer), Some(layer)
        if layer == row.layer_id || layer == egui::LayerId::new(egui::Order::Foreground, key))
}

pub(super) fn shortcut_consumed(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<HistoryFrame>(egui::Id::new("history-frame")).shortcut_consumed = true;
    });
}
