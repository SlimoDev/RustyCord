//! Follow decisions use actual message rows, never a pixel estimate: a tall
//! image and a one-line message both count as one message above the latest.
use super::*;

#[derive(Clone)]
struct Reading {
    last_visible: String,
    rendered_tail: String,
}

fn key(state: &AppSnapshot) -> egui::Id {
    egui::Id::new(("chat-reading-window", &state.selected_channel_id,
        state.notification_generation, state.current_user.as_ref().map(|user| user.id.as_str())))
}

pub(super) fn remember(ctx: &egui::Context, state: &AppSnapshot,
    rows: &[(usize, f32, f32)], offset: f32, height: f32) {
    let key = key(state);
    let end = offset + height;
    let reading = rows.iter().rev()
        .find(|(_, top, bottom)| *top < end && *bottom > offset)
        .and_then(|(index, _, _)| state.messages.get(*index))
        .zip(state.messages.last())
        .map(|(visible, tail)| Reading {last_visible: visible.id.clone(), rendered_tail: tail.id.clone()});
    ctx.data_mut(|data| match reading {
        Some(reading) => data.insert_temp(key, reading),
        None => {data.remove::<Reading>(key);},
    });
}

pub(super) fn within_last_messages(ctx: &egui::Context, state: &AppSnapshot, count: usize) -> bool {
    let key = key(state);
    let Some(reading) = ctx.data(|data| data.get_temp::<Reading>(key)) else {return false;};
    if !state.messages.last().is_some_and(|tail| tail.id == reading.rendered_tail) {return false;}
    state.messages.iter().rposition(|message| message.id == reading.last_visible)
        .is_some_and(|index| state.messages.len().saturating_sub(index + 1) <= count)
}
