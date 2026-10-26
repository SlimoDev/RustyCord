use super::*;
use super::mentions::completion;
use eframe::egui::{self, Align, Vec2};
#[path = "chat/rich.rs"]
mod rich;
#[path = "chat/reaction_tooltip.rs"]
mod reaction_tooltip;
#[path = "chat/interaction.rs"]
mod interaction;
#[path = "chat/viewport.rs"]
mod viewport;
#[path = "chat/text_cache.rs"]
mod text_cache;
pub(super) use reaction_tooltip::{clear_reaction_users, reaction_panel_open, reaction_panel_rect, reaction_users_result};
pub(super) fn clear_presentation(ctx:&egui::Context) {
    text_cache::clear(ctx);
    clear_geometry(ctx);
}
pub(super) fn clear_geometry(ctx:&egui::Context) {
    ctx.data_mut(|data|data.remove::<viewport::Rows>(egui::Id::new("message-viewport-rows")));
}

pub(super) fn suspend_interactions(ctx: &egui::Context) {
    interaction::suspend(ctx);
    clear_reaction_users(ctx);
    ctx.data_mut(|data| {
        data.remove::<HistorySelection>(egui::Id::new("history-selection"));
        data.remove::<HistoryFrame>(egui::Id::new("history-frame"));
        data.remove::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"));
        data.remove::<MessageActionRegion>(egui::Id::new("message-actions-region"));
        data.remove::<(egui::Id,bool)>(egui::Id::new("history-pointer-gesture"));
        data.remove::<bool>(egui::Id::new("composer-ime-active"));
    });
    ctx.plugin::<egui::text_selection::LabelSelectionState>().lock().clear_selection();
}

impl NotDiscordUi {
    pub(super) fn prepare_latest_scroll(&mut self) {
        let channel = self.state.selected_channel_id.clone();
        self.prepare_latest_scroll_for(&channel);
    }
    pub(super) fn prepare_latest_scroll_for(&mut self, channel: &str) {
        self.restore_anchor = None;
        self.focus_id.clear();
        self.focus_until = None;
        self.focus_scroll_pending = false;
        self.positions.remove(channel);
        self.pending_scroll = None;
        self.scroll_bottom = true;
        self.ctx.data_mut(|data| {
            data.remove::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"));
            data.insert_temp(
                egui::Id::new("force-latest-scroll"),
                channel.to_owned(),
            )
        });
        self.ctx.request_repaint();
    }
    fn near_recent_history(&self, ctx: &egui::Context, state: &AppSnapshot) -> bool {
        let scope=egui::Id::new(("message-scroll", &state.selected_channel_id, "chat"));
        let manual_motion=ctx.data(|data|
            data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"))
                .is_some_and(|middle|middle.scope==Some(scope)&&middle.origin.is_some())
            || data.get_temp::<HistorySelection>(egui::Id::new("history-selection"))
                .is_some_and(|selection|selection.scope==Some(scope)&&selection.dragging));
        state.view == "chat" && !self.focus_scroll_pending && self.focus_id.is_empty()
            && !manual_motion
            && latest_window(ctx, state)
            && self.positions.get(&state.selected_channel_id).map_or(self.scroll_bottom, |position|
                position.at_bottom || position.distance_to_bottom.is_some_and(|distance| distance <= 180.))
    }
    pub(super) fn follows_nearby_new_messages(&self, ctx: &egui::Context, next: &AppSnapshot) -> bool {
        if next.view != "chat" || self.state.view != "chat" || !next.signed_in
            || next.selected_channel_id != self.state.selected_channel_id
            || next.selected_channel_id != self.last_channel
            || next.notification_generation != self.state.notification_generation
            || next.current_user.as_ref().map(|user| &user.id) != self.state.current_user.as_ref().map(|user| &user.id)
            || next.focus_message_version != self.focus_version || self.background_blocked()
            || super::voice::call_chat_hidden(ctx,&self.state)
            || super::voice::call_chat_hidden(ctx,next)
            || ctx.input(|input| input.pointer.any_down() || input.raw_scroll_delta != egui::Vec2::ZERO
                || input.smooth_scroll_delta != egui::Vec2::ZERO)
            || !self.near_recent_history(ctx, &self.state) {
            return false;
        }
        let (Some(previous), Some(current)) = (self.state.messages.last(), next.messages.last()) else { return false; };
        previous.id != current.id && (
            current.timestamp > previous.timestamp
                || notdiscord_core::parsing::valid_id(&previous.id) && notdiscord_core::parsing::valid_id(&current.id)
                    && (current.id.len() > previous.id.len() || current.id.len() == previous.id.len() && current.id > previous.id)
        )
    }
    pub(super) fn draw_chat(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        let chat = state.view == "chat";
        let history_pending = history_page_pending(ui.ctx(), state);
        reaction_tooltip::begin_frame(ui.ctx(), state);
        if matches!(state.view.as_str(), "pins" | "search" | "history") {
            self.draw_chat_return_header(ui, if state.view == "pins" { "pins" } else { "return-chat" });
        }
        let latest_window = latest_window(ui.ctx(), state);
        let at_latest = latest_window && self.positions.get(&state.selected_channel_id)
            .map_or(self.scroll_bottom, |position| position.at_bottom);
        let composer_height = if chat {
            composer_height(
                ui,
                self.drafts.get(&state.selected_channel_id),
                self.prefs.chat_size,
            )
        } else {
            0.
        };
        let incoming = self.draw_incoming_call_in_chat(ui, state);
        let hidden_voice_chat=chat && super::voice::call_chat_hidden(ui.ctx(),state);
        if !incoming && super::voice::call_panel_visible(state) {
            self.draw_call_in_height(
                ui,
                state,
                if hidden_voice_chat {(ui.available_height()-8.).max(80.)} else {(ui.available_height() - composer_height - 100.)
                    .min(ui.available_height() * 0.52)
                    .max(80.)},
            );
        }
        if hidden_voice_chat || chat && super::voice::call_chat_hidden(ui.ctx(),state) {
            // Hiding the surface retires only its transient input state. The
            // retained channel position, anchor and draft belong to the user.
            suspend_interactions(ui.ctx());
            return;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.;
            ui.spacing_mut().button_padding = egui::vec2(7., 3.);
            if chat {
                if ui
                    .add_enabled(
                        !state.busy && !history_pending && state.has_older_messages
                            && !state.messages.is_empty() && self.positions.get(&state.selected_channel_id)
                                .is_some_and(|position| position.offset <= 1.)
                            && self.pending_scroll.is_none_or(|offset| offset <= 1.)
                            && self.restore_anchor.is_none() && !self.focus_scroll_pending,
                        egui::Button::new(RichText::new("↑ Mensajes anteriores").size(12.))
                            .min_size(egui::vec2(0., 22.)),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    let request = HistoryPageRequest::new(state);
                    if self.command.send(UiAction { kind: "older".into(), id: request.id.clone(),
                        channel_id: request.channel.clone(), ..Default::default() }).is_ok() {
                        ui.ctx().data_mut(|data| data.insert_temp(history_page_request_key(), request));
                    } else { self.error = "El servicio de la aplicación se cerró.".into(); }
                }
                if ui
                    .add_enabled(
                        !state.busy && !at_latest,
                        egui::Button::new(RichText::new("Volver al inicio ↓").size(12.))
                            .min_size(egui::vec2(0., 22.)),
                    )
                    .on_hover_text("Ir a los mensajes más recientes")
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    if latest_window {
                        // The newest page is already retained: moving within
                        // it needs no request and must also leave middle mode.
                        self.prepare_latest_scroll_for(&state.selected_channel_id);
                    } else {
                        // Keep the current anchor until the newest page arrives.
                        self.simple("latest-messages");
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let menu = ui.menu_button(RichText::new("···").size(12.), |ui| {
                    if ui
                        .add_enabled(
                            state.signed_in && !state.selected_channel_id.is_empty(),
                            egui::Button::new("Ver registro de esta sesión (RAM)"),
                        )
                        .on_hover_text("Ediciones y borrados observados en este canal; no se guardan en disco.")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.send("view", "history", "", "", &state.selected_channel_id);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !state.selected_guild_id.is_empty(),
                            egui::Button::new("Crear invitación al canal (24 h)"),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.confirm(
                        "Crear invitación",
                        "¿Crear una invitación a este canal válida durante 24 horas y copiarla?",
                        UiAction::new("create-invite"),
                    );
                        ui.close();
                    }
                });
                menu.response.on_hover_cursor(egui::CursorIcon::PointingHand);
                if !state.new_message_notice.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&state.new_message_notice)
                                .size(13.)
                                .color(self.theme.accent),
                        )
                        .truncate(),
                    );
                }
            });
        });
        let list = if state.view == "pins" {
            &state.pinned_messages
        } else {
            &state.messages
        };
        if state.view == "pins" {
            ui.heading("Mensajes fijados");
        } else if state.view == "history" {
            ui.label(RichText::new("Ediciones y borrados de esta sesión, sólo en RAM. Se eliminan al cerrar la aplicación o la sesión.")
                .small().color(self.theme.muted));
        }
        if list.is_empty() {
            ui.label(if state.busy {
                "Cargando mensajes…"
            } else if state.view == "pins" {
                "No hay mensajes fijados."
            } else if state.view == "history" {
                "No se han observado ediciones ni borrados en este canal durante esta sesión."
            } else {
                "No hay mensajes para mostrar."
            });
        }
        let scroll_salt = egui::Id::new(("message-scroll", &state.selected_channel_id, &state.view));
        interaction::begin(ui.ctx(), scroll_salt, state);
        let cancel_history_gesture = interaction::cancelling(ui.ctx());
        let manual_history = !self.background_blocked() && ui.is_enabled()
            && history_manual_input(ui.ctx(), scroll_salt, ui.layer_id());
        if manual_history {
            // A deliberate gesture wins over snapshot/resize restoration and
            // message focus. There is only one owner of the next offset.
            self.restore_anchor = None;
            self.focus_scroll_pending = false;
            self.scroll_bottom = false;
        }
        let layout_key = egui::Id::new(("chat-layout", &state.selected_channel_id, &state.view));
        let layout = (
            ui.available_width(),
            self.prefs.chat_size,
            self.prefs.compact,
            ui.ctx().pixels_per_point().to_bits(),
        );
        let previous_layout = ui
            .ctx()
            .data(|data| data.get_temp::<(f32, f32, bool, u32)>(layout_key));
        if chat
            && !manual_history
            && previous_layout.is_some_and(|old| old != layout)
            && !self.scroll_bottom
            && self.restore_anchor.is_none()
            && !self.focus_scroll_pending
        {
            if let Some(position) = self
                .positions
                .get(&state.selected_channel_id)
                .filter(|p| !p.anchor.is_empty())
            {
                self.restore_anchor = Some((position.anchor.clone(), position.anchor_top));
            }
        }
        ui.ctx()
            .data_mut(|data| data.insert_temp(layout_key, layout));
        let previous_offset = self.positions.get(&state.selected_channel_id).map(|position| position.offset);
        let pending_matches_position = self.pending_scroll.is_none_or(|pending|
            previous_offset.is_some_and(|previous| (pending - previous).abs() < 0.5));
        let restore_request = self.restore_anchor.clone().or_else(|| {
            // Keep the visible row stable when an image finishes loading or
            // history is prepended, including during a held gesture. Manual
            // input below receives only the geometry delta, not an old offset.
            (chat && !self.scroll_bottom && pending_matches_position
                && !self.focus_scroll_pending).then(|| self.positions.get(&state.selected_channel_id)
                .filter(|position| !position.anchor.is_empty())
                .map(|position| (position.anchor.clone(), position.anchor_top))).flatten()
        });
        let mut restore_offset = None;
        let spacing = ui.spacing().item_spacing.y;
        let typing_height = if state.typing_text.is_empty() {
            0.
        } else {
            ui.fonts_mut(|fonts| fonts.row_height(&egui::FontId::proportional(12.))) + spacing
        };
        let tail_height = if chat {
            composer_height + spacing + typing_height + CHAT_COMPOSER_GAP
        } else {
            0.
        };
        let force_latest = chat
            && ui.ctx().data_mut(|data| {
                let key = egui::Id::new("force-latest-scroll");
                if data.get_temp::<String>(key).as_deref()
                    == Some(state.selected_channel_id.as_str())
                {
                    data.remove::<String>(key);
                    true
                } else {
                    false
                }
            });
        if force_latest {
            // Reset the stored sticky state, not the content offset to an enormous
            // value: egui lays out content before clamping offsets, so f32::MAX
            // loses row geometry. A fresh state sticks to the measured real end.
            egui::scroll_area::State::default().store(ui.ctx(), ui.make_persistent_id(scroll_salt));
        }
        let requested_offset = self.pending_scroll.take();
        let rows_key = egui::Id::new("message-viewport-rows");
        let mut rows = ui.ctx().data_mut(|data|data.remove_temp::<viewport::Rows>(rows_key)).unwrap_or_default();
        if chat { rows.sync(scroll_salt, layout, &state.messages); }
        let complete_layout = !chat || force_latest || viewport::complete_layout(ui.ctx(),scroll_salt);
        // The initial pass positions the newest rows using finite geometry.
        // It must not start downloads for the provisional, older viewport.
        self.pictures.set_request_admission(!force_latest);
        self.pictures.set_media_owner("chat",100);
        let mut area = theme::vertical_scroll(ui)
            .id_salt(scroll_salt)
            .auto_shrink([false, false])
            .min_scrolled_height(0.)
            .max_height((ui.available_height() - tail_height).max(0.))
            // One handler below owns wheel input, including selection drags
            // and the foreground message toolbar. Nested scroll areas can
            // still consume it first; scrollbar dragging remains native.
            .scroll_source(egui::scroll_area::ScrollSource { drag: false, scroll_bar: true, mouse_wheel: false })
            .animated(false)
            .stick_to_bottom(self.scroll_bottom && chat && requested_offset.is_none());
        if let Some(offset) = requested_offset {
            area = area.vertical_scroll_offset(offset);
        }
        let mut first_visible = String::new();
        let mut anchor_top = 0.;
        let mut rendered_offset = 0.;
        let row_heights = &mut self.row_heights;
        let old_heights = std::mem::take(row_heights);
        let mut measured = HashMap::new();
        let out = area.show_viewport(ui, |ui, viewport| {
            if cancel_history_gesture { ui.disable(); }
            rendered_offset = viewport.min.y;
            history_selection_begin(ui.ctx(), scroll_salt);
            ui.set_width(ui.available_width());
            let content_top = ui.cursor().top();
            for (index, message) in list.iter().enumerate() {
                let previous = index.checked_sub(1).and_then(|i| list.get(i));
                let date = previous
                    .is_none_or(|p| local_date(&p.timestamp) != local_date(&message.timestamp));
                let continuation = chat && previous.is_some_and(|p| can_group(p, message));
                let content_y = ui.cursor().top() - content_top;
                let estimated = if chat { rows.height(index,message,continuation,date) } else { 0. };
                let margin = viewport.height().max(200.) * 0.75;
                let wanted = viewport.min.y-margin .. viewport.max.y+margin;
                let forced = self.focus_scroll_pending && message.id == self.focus_id
                    || restore_request.as_ref().is_some_and(|(id,_)|id==&message.id);
                if !complete_layout && !forced
                    && (content_y+estimated < wanted.start || content_y > wanted.end) {
                    if let Some((id, top)) = &restore_request {
                        if message.id == *id { restore_offset = Some((content_y-*top).max(0.)); }
                    }
                    if first_visible.is_empty() && content_y+estimated > viewport.min.y {
                        first_visible=message.id.clone();anchor_top=content_y-viewport.min.y;
                    }
                    ui.add_space(estimated);
                    continue;
                }
                {
                    let begin = ui.cursor().top();
                    let content_y = begin - content_top;
                    if let Some((id, top)) = &restore_request {
                        if message.id == *id {
                            restore_offset = Some((content_y - *top).max(0.));
                        }
                    }
                    if date {
                        ui.add_space(12.);
                        let date = spanish_date(local_date(&message.timestamp), true);
                        let galley = ui.painter().layout_no_wrap(
                            date,
                            egui::FontId::proportional(13.),
                            self.theme.muted,
                        );
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), galley.size().y.max(18.)),
                            egui::Sense::hover(),
                        );
                        let center = rect.center();
                        let half = galley.size().x / 2. + 9.;
                        let stroke = egui::Stroke::new(1.0_f32, self.theme.line);
                        if center.x - half > rect.left() {
                            ui.painter().line_segment(
                                [
                                    egui::pos2(rect.left(), center.y),
                                    egui::pos2(center.x - half, center.y),
                                ],
                                stroke,
                            );
                            ui.painter().line_segment(
                                [
                                    egui::pos2(center.x + half, center.y),
                                    egui::pos2(rect.right(), center.y),
                                ],
                                stroke,
                            );
                        }
                        ui.painter()
                            .galley(center - galley.size() / 2., galley, self.theme.muted);
                        ui.add_space(7.);
                    }
                    let response = ui.scope_builder(egui::UiBuilder::new().id(("history-row",scroll_salt,&message.id)),
                        |ui| self.draw_message(ui, message, state, continuation)).inner;
                    let height = (ui.cursor().top() - begin).max(1.);
                    if first_visible.is_empty() && content_y + height > viewport.min.y {
                        first_visible = message.id.clone();
                        anchor_top = content_y - viewport.min.y;
                    }
                    if message.id == self.focus_id && self.focus_until.is_some() {
                        if self.focus_scroll_pending {
                            response.scroll_to_me(Some(Align::Center));
                            self.focus_scroll_pending = false;
                        }
                    }
                    measured.insert(message.id.clone(), height);
                    if chat { rows.measure(index,height); }
                }
            }
            if chat {
                self.draw_pending_messages(ui, state);
            }
            // Real content padding keeps the last row clear of the editor while
            // keeping stick-to-bottom and the measured scrollbar end consistent.
            ui.add_space(CHAT_COMPOSER_GAP);
        });
        self.pictures.set_request_admission(true);
        self.pictures.set_media_owner("ui",0);
        ui.ctx().data_mut(|data|data.insert_temp(rows_key,rows));
        let selecting = history_selection_finish(ui.ctx(), out.inner_rect, scroll_salt,
            !ui.is_enabled() || self.background_blocked());
        let advance_motion = ui.ctx().current_pass_index() == 0;
        let wheel_delta = if advance_motion && ui.is_enabled() && !self.background_blocked()
            && history_wheel_target(ui.ctx(), out.inner_rect, ui.layer_id()) {
            // Consume only leftovers from nested content. Owning this here
            // also keeps wheel input available while selecting text or
            // hovering a foreground message action.
            let vertical_only = ui.style().always_scroll_the_only_direction;
            ui.ctx().input(|input| input.smooth_scroll_delta.y + if vertical_only { input.smooth_scroll_delta.x } else { 0. })
        } else { 0. };
        if wheel_delta != 0. {
            let end = (out.content_size.y - out.inner_rect.height()).max(0.);
            let next = (out.state.offset.y - wheel_delta).clamp(0., end);
            if (next - out.state.offset.y).abs() > 0.01 {
                let vertical_only = ui.style().always_scroll_the_only_direction;
                ui.ctx().input_mut(|input| {
                    input.smooth_scroll_delta.y = 0.;
                    if vertical_only { input.smooth_scroll_delta.x = 0.; }
                });
                self.pending_scroll = Some(next);
                self.scroll_bottom = false;
                ui.ctx().request_repaint();
            }
        }
        let selection_velocity = ui.ctx().input(|input| {
            input.pointer.hover_pos().map_or(0., |pointer| {
                selection_scroll_velocity(out.inner_rect, pointer, selecting)
            })
        });
        let middle_velocity = history_middle_scroll(ui, out.inner_rect, scroll_salt,
            self.background_blocked() || egui::Popup::is_any_open(ui.ctx()) || reaction_panel_open(ui.ctx()));
        let velocity = if !advance_motion || wheel_delta != 0. { 0. } else if selecting { selection_velocity } else { middle_velocity };
        if velocity != 0. {
            let dt = ui.ctx().input(|input| input.stable_dt).clamp(0., 0.05);
            let end = (out.content_size.y - out.inner_rect.height()).max(0.);
            let next = (out.state.offset.y + velocity * dt).clamp(0., end);
            if (next - out.state.offset.y).abs() > 0.01 {
                self.pending_scroll = Some(next);
                self.scroll_bottom = false;
                ui.ctx().request_repaint();
            }
        }
        if force_latest {
            // Compute the real finite end after laying out the replacement page.
            // Resetting sticky state alone does not move an already unstuck
            // ScrollArea reliably. Discard this layout pass before painting it.
            let end = (out.content_size.y - out.inner_rect.height()).max(0.);
            if (rendered_offset - end).abs() > 0.5 {
                self.pending_scroll = Some(end);
                ui.ctx()
                    .request_discard("position the latest messages after their measured layout");
            }
        }
        self.row_heights = old_heights;
        self.row_heights.extend(measured);
        let manual_scroll = manual_history || wheel_delta != 0. || velocity != 0.;
        if chat && restore_request.is_some() {
            if let Some(offset) = restore_offset {
                let geometry_changed = previous_offset.is_some_and(|previous| (offset - previous).abs() > 0.5);
                let offset = if manual_scroll {
                    self.pending_scroll.unwrap_or(out.state.offset.y)
                        + previous_offset.map_or(0., |previous| offset - previous)
                } else { offset };
                let offset = offset.clamp(0., (out.content_size.y - out.inner_rect.height()).max(0.));
                if (self.pending_scroll.unwrap_or(out.state.offset.y) - offset).abs() > 0.5 {
                    self.pending_scroll = Some(offset);
                    if geometry_changed {
                        // An image or prepended page changed content above the
                        // reader. Lay out once more before showing that jump;
                        // time-based motion advances only on the first pass.
                        ui.ctx().request_discard("preserve the history anchor after content changes height");
                    } else { ui.ctx().request_repaint(); }
                }
                self.restore_anchor = None;
            } else if !state.busy && !list.is_empty() {
                self.restore_anchor = None;
                // A retired anchor is not a request to jump to the newest
                // message. Keep the finite, clamped position instead.
            }
        }
        if self.row_heights.len() > 400 {
            let valid: std::collections::HashSet<_> = list.iter().map(|m| m.id.as_str()).collect();
            self.row_heights.retain(|k, _| valid.contains(k.as_str()));
        }
        if chat && !state.selected_channel_id.is_empty() {
            if self.positions.len() >= 1024
                && !self.positions.contains_key(&state.selected_channel_id)
            {
                if let Some(k) = self.positions.keys().next().cloned() {
                    self.positions.remove(&k);
                }
            }
            let effective_offset = self.pending_scroll.unwrap_or(out.state.offset.y);
            let at_bottom = effective_offset + out.inner_rect.height() >= out.content_size.y - 8.;
            self.scroll_bottom = at_bottom && !manual_scroll;
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("chat-visibility"),
                    (
                        state.selected_channel_id.clone(),
                        list.last().map(|m| m.id.clone()).unwrap_or_default(),
                        at_bottom,
                    ),
                )
            });
            self.positions.insert(
                state.selected_channel_id.clone(),
                ChatPosition {
                    offset: effective_offset,
                    at_bottom,
                    distance_to_bottom: Some((out.content_size.y - out.inner_rect.height() - effective_offset).max(0.)),
                    anchor: first_visible,
                    // Rows were laid out before ScrollArea applies thumb,
                    // sticky or scroll-to changes. Substituting its final
                    // offset here makes the next anchor restore undo input.
                    anchor_top: anchor_top + rendered_offset - effective_offset,
                },
            );
            let _ = anchor_top;
        }
        if chat {
            ui.add_space(CHAT_COMPOSER_GAP);
            self.draw_composer(ui, state);
        }
    }
    /// A view's back navigation lives outside its scrolled content, including
    /// while a call panel occupies part of the available height.
    pub(super) fn draw_chat_return_header(&mut self, ui: &mut egui::Ui, action: &str) {
        ui.horizontal(|ui| {
            if ui.button("← Volver al chat").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { self.simple(action); }
        });
    }
    fn draw_message(
        &mut self,
        ui: &mut egui::Ui,
        m: &ChatMessage,
        state: &AppSnapshot,
        continuation: bool,
    ) -> egui::Response {
        self.emoji_section(ui, ("chat-message-emojis", &m.channel_id, &m.id), |this, ui| {
            this.draw_message_contents(ui, m, state, continuation)
        }).inner
    }
    fn draw_message_contents(
        &mut self,
        ui: &mut egui::Ui,
        m: &ChatMessage,
        state: &AppSnapshot,
        continuation: bool,
    ) -> egui::Response {
        if is_system_record(m.message_type) && !m.deleted {
            return self.draw_call_record(ui, m, state);
        }
        let selected = m.id == self.focus_id;
        let personal_reply = text_cache::personal_reply(ui.ctx(),m, state)
            || super::mentions::highlights_current_user(m, state);
        let shortcuts_allowed = ui.is_enabled() && !ui.ctx().wants_keyboard_input()
            && !egui::Popup::is_any_open(ui.ctx()) && !self.background_blocked();
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("message-text-shortcuts"), MessageTextGesture::default()));
        let mut continuation_time = None;
        let action_key = egui::Id::new(("message-actions", &m.channel_id, &m.id));
        // Register the row beneath its children. Adding Response::interact after
        // rendering the frame put the row above image/reply/link hit targets.
        let response = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .id_salt(("message-row", &m.id))
                    .sense(egui::Sense::click()),
                |ui| {
                    ui.set_max_width(ui.available_width());
                    egui::Frame::new()
                        .fill(if selected {
                            self.theme.selected
                        } else if personal_reply {
                            personal_reply_fill()
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(4)
                        .inner_margin(egui::Margin {
                            left: 10,
                            right: 10,
                            top: if self.prefs.compact {
                                3
                            } else if continuation {
                                1
                            } else {
                                13
                            },
                            bottom: if continuation { 1 } else { 5 },
                        })
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                let spacing = ui.spacing().item_spacing.x;
                                ui.spacing_mut().item_spacing.x = 0.;
                                if continuation {
                                    continuation_time = Some(
                                        ui.allocate_exact_size(
                                            egui::vec2(56., self.prefs.chat_size * 1.45),
                                            egui::Sense::hover(),
                                        )
                                        .0,
                                    );
                                } else {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(56., 40.),
                                        egui::Layout::top_down(Align::Min),
                                        |ui| {
                                            ui.set_min_size(egui::vec2(56., 40.));
                                            let image = self.avatar(ui, &m.author, "", 40.)
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if image.clicked() {
                                                self.send(
                                                    "profile",
                                                    &m.author.id,
                                                    "",
                                                    "",
                                                    &m.channel_id,
                                                );
                                            }
                                            image.context_menu(|ui| {
                                                self.people_menu(ui, &m.author, state)
                                            });
                                        },
                                    );
                                }
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.x = spacing;
                                    ui.spacing_mut().item_spacing.y = 1.;
                                    ui.set_width((ui.available_width() - 4.).max(1.));
                                    ui.set_max_width(ui.available_width());
                                    if !continuation {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add(
                                                    egui::Label::new(
                                                        RichText::new(message_display_name(m, state))
                                                            .size(16.)
                                                            .strong(),
                                                    )
                                                    .truncate()
                                                    .sense(egui::Sense::click()),
                                                )
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                self.send(
                                                    "profile",
                                                    &m.author.id,
                                                    "",
                                                    "",
                                                    &m.channel_id,
                                                );
                                            }
                                            ui.label(
                                                RichText::new(local_time(&m.timestamp))
                                                    .size(12.)
                                                    .color(self.theme.muted),
                                            );
                                            if m.author.is_bot || !m.webhook_id.is_empty() {
                                                ui.label(RichText::new(if m.webhook_id.is_empty() { "BOT" } else { "WEBHOOK" })
                                                    .size(10.).strong().color(self.theme.ink).background_color(self.theme.selected));
                                            }
                                            if m.edited {
                                                ui.small("(editado)");
                                            }
                                        });
                                    }
                                    if m.forwarded {
                                        ui.label(
                                            RichText::new("↪ Reenviado")
                                                .size(12.)
                                                .color(self.theme.muted),
                                        );
                                    }
                                    if let Some(reply) = &m.reply_preview {
                                        let author = reply
                                            .author
                                            .as_ref()
                                            .map(|p| reply_display_name(reply, p, state))
                                            .unwrap_or("Mensaje");
                                        let content = if reply.deleted {
                                            "Mensaje eliminado"
                                        } else if reply.unavailable {
                                            "Mensaje no disponible"
                                        } else {
                                            &reply.content
                                        };
                                        let content =
                                            readable_mentions(content, &reply.mentions, state);
                                        let r = reply_preview(
                                            ui,
                                            &format!("↳ {}: {}", author, short(&content, 110)),
                                            self.prefs.chat_size,
                                            self.theme.muted,
                                            self.theme.selected,
                                            !reply.deleted && !reply.unavailable,
                                        );
                                        if r.clicked() {
                                            self.jump_message(&reply.channel_id, &reply.message_id);
                                        }
                                    }
                                    for previous in &m.previous_contents {
                                        selectable_message_label(ui, RichText::new(readable_mentions(previous, &m.mentions, state))
                                            .size(self.prefs.chat_size).italics().color(self.theme.muted));
                                    }
                                    if m.edited && m.previous_contents.is_empty() && state.view != "history" {
                                        ui.label(RichText::new("Versión anterior no disponible en esta sesión")
                                            .size(11.).italics().color(self.theme.muted));
                                    }
                                    if m.deleted {
                                        ui.label(
                                            RichText::new("Mensaje eliminado")
                                                .italics()
                                                .color(Color32::from_rgb(242, 63, 67)),
                                        );
                                        selectable_message_label(ui, RichText::new(readable_mentions(&m.content, &m.mentions, state))
                                            .size(self.prefs.chat_size).color(Color32::from_rgb(242, 63, 67)));
                                        self.draw_deleted_message_rich(ui, m, state);
                                        for attachment in &m.attachments {
                                            ui.label(RichText::new(format!("Adjunto conocido: {}", attachment.name))
                                                .size(self.prefs.chat_size).color(Color32::from_rgb(242, 63, 67)));
                                        }
                                    } else {
                                        self.message_text(ui, &m.content, m, state);
                                        self.draw_message_rich(ui, m, state);
                                        for sticker in &m.stickers {
                                            self.message_text(ui, &sticker.code(), m, state);
                                        }
                                    }
                                    for a in m.attachments.iter().filter(|attachment| !m.deleted && !m.components_v2
                                        && !m.components.iter().any(|component| matches!(component.kind, 11..=13)
                                            && !component.url.is_empty() && notdiscord_core::parsing::same_media_resource(&component.url, &attachment.url))) {
                                        let image = a
                                            .content_type
                                            .as_deref()
                                            .is_some_and(|t| t.starts_with("image/"))
                                            || a.width > 0 && a.height > 0;
                                        if image {
                                            self.media_card(
                                                ui,
                                                &a.url,
                                                a.preview_url.as_deref().unwrap_or(&a.url),
                                                a.animated || a.url.contains(".gif"),
                                                &a.name,
                                                Some((
                                                    a.width.max(0) as u32,
                                                    a.height.max(0) as u32,
                                                )),
                                            );
                                        } else {
                                            ui.horizontal(|ui| {
                                                if ui.link(format!("📎 {}", a.name)).clicked() {
                                                    let _ = crate::platform::open_link(&a.url);
                                                }
                                                ui.small(format!(
                                                    "{:0.1} MiB",
                                                    a.size as f64 / 1048576.
                                                ));
                                            });
                                        }
                                    }
                                    if !m.reactions.is_empty() {
                                        ui.horizontal_wrapped(|ui| {
                                            for r in &m.reactions {
                                                let response = egui::Frame::new()
                                                    .fill(if r.me {
                                                        self.theme.selected
                                                    } else {
                                                        self.theme.surface
                                                    })
                                                    .corner_radius(5)
                                                    .inner_margin(egui::Margin::symmetric(6, 3))
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            self.emoji(ui, &r.emoji, 18.);
                                                            ui.label(r.count.to_string());
                                                        });
                                                    })
                                                    .response
                                                    .interact(egui::Sense::click())
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                                if response.clicked() {
                                                    self.send(
                                                        "react-toggle",
                                                        &m.id,
                                                        r.emoji.reaction_key(),
                                                        "",
                                                        &m.channel_id,
                                                    );
                                                }
                                                self.reaction_tooltip(ui, &response, m, r, state);
                                            }
                                        });
                                    }
                                });
                            });
                        });
                },
            )
            .response;
        if personal_reply {
            ui.painter().with_clip_rect(ui.clip_rect()).vline(
                response.rect.left() + 1.5,
                response.rect.y_range(),
                egui::Stroke::new(3_f32, personal_reply_accent()),
            );
        }
        history_register_row(ui.ctx(), response.id);
        let text_shortcuts = ui.ctx().data(|data| data.get_temp::<MessageTextGesture>(egui::Id::new("message-text-shortcuts"))).unwrap_or_default();
        let mut context_menu = egui::Popup::context_menu(&response);
        if text_shortcuts.secondary_clicked {
            // Selectable text owns its hit target. Route its context click to
            // the row without stealing clicks from links, images or mentions.
            context_menu = context_menu.open_memory(Some(egui::SetOpenCommand::Bool(true)));
        }
        context_menu.show(|ui| self.message_menu(ui, m, state));
        let own = state.current_user.as_ref().is_some_and(|person| person.id == m.author.id);
        if shortcuts_allowed && own && !m.deleted && state.view == "chat" && state.signed_in
            && !state.busy && state.channel_access.can_send {
            let delete_click = text_shortcuts.delete_clicked || response.clicked() && ui.ctx().input(|i| i.key_down(egui::Key::Delete));
            if delete_click {
                interaction::shortcut_consumed(ui.ctx());
                self.confirm_message_delete(m);
            } else if !m.forwarded && (text_shortcuts.double_clicked || response.double_clicked()) {
                interaction::shortcut_consumed(ui.ctx());
                self.prompt_for("Editar mensaje", "Contenido del mensaje", "edit", &m.id, &m.content, &m.channel_id, true);
            }
        }
        if !own && !m.deleted && state.view == "chat" && state.signed_in
            && state.current_user.is_some() && m.channel_id == state.selected_channel_id
            && !state.busy && state.channel_access.can_send && ui.is_enabled()
            && !self.background_blocked() && !egui::Popup::is_any_open(ui.ctx())
            && (text_shortcuts.double_clicked || response.double_clicked()) {
            interaction::shortcut_consumed(ui.ctx());
            self.reply_to(m);
            ui.memory_mut(|memory| memory.request_focus(egui::Id::new(("composer", &m.channel_id))));
        }
        let previous_actions = ui.ctx().data(|data| data.get_temp::<MessageActionRegion>(egui::Id::new("message-actions-region")))
            .filter(|cached| cached.id == action_key)
            .map(|cached| cached.rect.translate(response.rect.min - cached.row_origin));
        let row_region = previous_actions.map_or(response.rect, |actions| response.rect.union(actions));
        let row_hovered = ui
            .ctx()
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| row_region.intersect(ui.clip_rect()).contains(pos));
        let owned_hover = interaction::row_hover(ui.ctx(), &response, action_key);
        self.pictures.include_emoji_section_hover(row_hovered && owned_hover);
        if row_hovered && owned_hover {
            if let Some(rect) = continuation_time {
                ui.painter().text(
                    egui::pos2(rect.right() - 9., rect.top() + 2.),
                    egui::Align2::RIGHT_TOP,
                    local_time(&m.timestamp),
                    egui::FontId::proportional(11.),
                    self.theme.muted,
                );
            }
        }
        let popup_id = egui::Id::new(("message-overflow", &m.channel_id, &m.id));
        let menu_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
        if row_hovered && owned_hover && interaction::controls_allowed(ui.ctx()) && ui.is_enabled() && !response.context_menu_opened()
            && matches!(state.view.as_str(), "pins" | "search") && state.signed_in {
            self.reference_message_actions(ui, m, state, action_key, response.rect, previous_actions);
        }
        if (row_hovered && owned_hover || menu_open) && interaction::controls_allowed(ui.ctx()) && ui.is_enabled()
            && !response.context_menu_opened() && state.view == "chat" && state.signed_in
        {
            let own = state
                .current_user
                .as_ref()
                .is_some_and(|u| u.id == m.author.id);
            let count = 4
                + usize::from(state.can_pin_messages)
                + if own {
                    if m.forwarded { 1 } else { 2 }
                } else {
                    0
                };
            let width = previous_actions.map(|rect| rect.width()).unwrap_or(count as f32 * 38. + 16.);
            let pos = egui::pos2(
                (response.rect.right() - width).max(response.rect.left()),
                response.rect.top(),
            );
            let mut more_rect = egui::Rect::NOTHING;
            let actions = egui::Area::new(action_key)
                .fixed_pos(pos)
                .constrain_to(ui.clip_rect())
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.;
                    egui::Frame::new()
                        .fill(self.theme.panel)
                        .corner_radius(5)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let writable = !state.busy && !m.deleted;
                                if ui
                                    .add_enabled(
                                        writable && state.channel_access.can_send,
                                        egui::Button::new("↩").small(),
                                    )
                                    .on_hover_text("Responder")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.reply_to(m);
                                }
                                if ui
                                    .add_enabled(
                                        writable
                                            && state.channel_access.can_view
                                            && !m.forwarded
                                            && matches!(m.message_type, 0 | 19),
                                        egui::Button::new("↪").small(),
                                    )
                                    .on_hover_text("Reenviar")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.start_forward(m, state);
                                }
                                if ui
                                    .add_enabled(
                                        writable && state.channel_access.can_react,
                                        egui::Button::new("☺").small(),
                                    )
                                    .on_hover_text("Añadir reacción")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    self.open_picker(false, state);
                                    if let Some(picker) = &mut self.picker {
                                        picker.react = Some((m.channel_id.clone(), m.id.clone()));
                                    }
                                }
                                if state.can_pin_messages
                                    && pin_action_button(ui, writable, self.theme.ink, m.pinned)
                                        .on_hover_text(if m.pinned {
                                            "Desfijar mensaje"
                                        } else {
                                            "Fijar mensaje"
                                        })
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                {
                                    self.send(
                                        if m.pinned {
                                            "unpin-message"
                                        } else {
                                            "pin-message"
                                        },
                                        &m.id,
                                        "",
                                        "",
                                        &m.channel_id,
                                    );
                                }
                                if own
                                    && !m.forwarded
                                    && ui
                                        .add_enabled(
                                            writable && state.channel_access.can_send,
                                            egui::Button::new("✎").small(),
                                        )
                                        .on_hover_text("Editar")
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                {
                                    self.prompt_for(
                                        "Editar mensaje",
                                        "Contenido del mensaje",
                                        "edit",
                                        &m.id,
                                        &m.content,
                                        &m.channel_id,
                                        true,
                                    );
                                }
                                if own
                                    && ui
                                        .add_enabled(
                                            writable,
                                            egui::Button::new(
                                                RichText::new("×")
                                                    .color(Color32::from_rgb(242, 63, 67)),
                                            )
                                            .small(),
                                        )
                                        .on_hover_text("Eliminar mensaje")
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .clicked()
                                {
                                    self.confirm_message_delete(m);
                                }
                                let more = ui.small_button("⋯").on_hover_cursor(egui::CursorIcon::PointingHand);
                                more_rect = more.rect;
                                egui::Popup::menu(&more)
                                    .id(popup_id)
                                    .show(|ui| self.message_menu(ui, m, state));
                            });
                        });
                });
            ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("message-actions-region"),
                MessageActionRegion { id: action_key, rect: actions.response.rect, row_origin: response.rect.min, more: more_rect }));
        }
        response
    }
    fn draw_call_record(
        &mut self,
        ui: &mut egui::Ui,
        m: &ChatMessage,
        state: &AppSnapshot,
    ) -> egui::Response {
        let me = state
            .current_user
            .as_ref()
            .map(|p| p.id.as_str())
            .unwrap_or("");
        let pinned = m.message_type == 6;
        let (description, detail, negative) = if pinned {
            (
                "fijó un mensaje en este canal.".into(),
                "Mensaje fijado".into(),
                false,
            )
        } else if m.message_type == 3 {
            call_record_description(m, me)
        } else {
            (
                system_record_description(m),
                "Actividad del canal".into(),
                false,
            )
        };
        let color = if m.message_type != 3 {
            self.theme.muted
        } else if negative {
            Color32::from_rgb(242, 63, 67)
        } else {
            presence_color("online")
        };
        let response = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .id_salt(("call-record", &m.id))
                    .sense(egui::Sense::click()),
                |ui| {
                    egui::Frame::new()
                        .fill(if m.id == self.focus_id {
                            self.theme.selected
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(4)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.;
                                let (icon, _) = ui.allocate_exact_size(
                                    egui::vec2(56., 26.),
                                    egui::Sense::hover(),
                                );
                                let rect = egui::Rect::from_min_size(
                                    icon.min + egui::vec2(8., 1.),
                                    egui::vec2(24., 24.),
                                );
                                if pinned {
                                    super::icons::paint_pin(ui.painter(), rect, color);
                                } else if m.message_type == 3 {
                                    super::icons::paint_call(ui.painter(), rect, color);
                                } else {
                                    paint_member_event(
                                        ui.painter(),
                                        rect,
                                        color,
                                        m.message_type == 2,
                                    );
                                }
                                ui.vertical(|ui| {
                                    ui.set_max_width(ui.available_width());
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.;
                                        if ui
                                            .add(
                                                egui::Label::new(
                                                    RichText::new(message_display_name(m, state))
                                                        .size(self.prefs.chat_size)
                                                        .strong(),
                                                )
                                                .sense(egui::Sense::click()),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.send(
                                                "profile",
                                                &m.author.id,
                                                "",
                                                "",
                                                &m.channel_id,
                                            );
                                        }
                                        if pinned {
                                            selectable_message_label(ui,
                                                RichText::new("fijó")
                                                    .size(self.prefs.chat_size)
                                                    .color(self.theme.muted),
                                            );
                                            let target = m.reply_to.as_deref().filter(|id| {
                                                notdiscord_core::parsing::valid_id(id)
                                            });
                                            let available = target.is_some()
                                                && m.reply_preview
                                                    .as_ref()
                                                    .is_none_or(|preview| !preview.deleted);
                                            let link = ui.add(
                                                egui::Label::new(
                                                    RichText::new("un mensaje")
                                                        .size(self.prefs.chat_size)
                                                        .color(if available {
                                                            self.theme.accent
                                                        } else {
                                                            self.theme.muted
                                                        }),
                                                )
                                                .sense(if available {
                                                    egui::Sense::click()
                                                } else {
                                                    egui::Sense::hover()
                                                }),
                                            );
                                            if available
                                                && link
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .on_hover_text("Ir al mensaje original")
                                                    .clicked()
                                            {
                                                self.jump_message(&m.channel_id, target.unwrap());
                                            }
                                            selectable_message_label(ui,
                                                RichText::new("en este canal.")
                                                    .size(self.prefs.chat_size)
                                                    .color(self.theme.muted),
                                            );
                                        } else {
                                            selectable_message_label(ui,
                                                RichText::new(&description)
                                                    .size(self.prefs.chat_size)
                                                    .color(if negative {
                                                        color
                                                    } else {
                                                        self.theme.muted
                                                    }),
                                            );
                                        }
                                        ui.label(
                                            RichText::new(local_time(&m.timestamp))
                                                .size(12.)
                                                .color(self.theme.muted),
                                        );
                                    });
                                });
                            });
                        });
                },
            )
            .response
            .on_hover_text(detail);
        history_register_row(ui.ctx(), response.id);
        response.context_menu(|ui| {
            if ui.button("Copiar texto").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                ui.ctx()
                    .copy_text(format!("{} {}", message_display_name(m, state), description));
                ui.close();
            }
            if ui.button("Copiar enlace al mensaje").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                ui.ctx().copy_text(format!(
                    "https://discord.com/channels/{}/{}/{}",
                    if state.selected_guild_id.is_empty() {
                        "@me"
                    } else {
                        &state.selected_guild_id
                    },
                    m.channel_id,
                    m.id
                ));
                ui.close();
            }
        });
        response
    }
    fn message_text(
        &mut self,
        ui: &mut egui::Ui,
        text: &str,
        m: &ChatMessage,
        state: &AppSnapshot,
    ) {
        let mut media: Vec<&str> = Vec::new();
        let parsed = text_cache::parsed(ui.ctx(),text);
        for parsed_line in &parsed.lines {
            let line = &parsed.source[parsed_line.range.clone()];
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 2.;
                ui.spacing_mut().item_spacing.y = 0.;
                ui.set_min_height(self.prefs.chat_size * 1.45);
                let mut end = 0;
                for capture in &parsed_line.tokens {
                    let whole = capture.get(0,line).unwrap();
                    let named = capture
                        .get(5,line)
                        .and_then(|name| text_cache::named(ui.ctx(),name.as_str(), m, state));
                    if capture.get(5,line).is_some() && named.is_none() {
                        // An unknown name stays in the same plain text run.
                        continue;
                    }
                    if whole.start() > end {
                        self.message_plain(
                            ui,
                            &line[end..whole.start()],
                            self.prefs.chat_size,
                            self.theme.ink,
                        );
                    }
                    if capture.get(6,line).is_some() {
                        if m.mentions_everyone && !m.forwarded {
                            mention_label(ui, whole.as_str().trim_start_matches('@'), self.prefs.chat_size);
                        } else {
                            self.message_plain(ui, whole.as_str(), self.prefs.chat_size, self.theme.ink);
                        }
                    } else if let Some(id) = capture.get(1,line) {
                        let name = super::mentions::name(id.as_str(), &m.mentions, state);
                        if mention_label(ui, name, self.prefs.chat_size).clicked() {
                            self.send("profile", id.as_str(), "", "", &m.channel_id);
                        }
                    } else if let Some(id) = capture.get(4,line) {
                        let emoji = StatusEmoji {
                            id: id.as_str().into(),
                            name: capture.get(3,line).unwrap().as_str().into(),
                            animated: capture.get(2,line).is_some_and(|a| a.as_str() == "a"),
                        };
                        self.message_emoji(ui, &emoji, self.prefs.chat_size * 1.4, &format!(":{}:",emoji.name));
                    } else if let Some(emoji) = named {
                        self.message_emoji(ui, &emoji, self.prefs.chat_size * 1.4, &format!(":{}:",emoji.name));
                    } else {
                        let url = whole.as_str();
                        let gif_preview = (notdiscord_core::parsing::direct_gif(url)
                            || notdiscord_core::parsing::gif_page(url))
                            && super::mentions::plain_text_at(text, parsed_line.range.start + whole.start());
                        // GIF messages use their media card as the visible
                        // target. Keep the original content for editing/copy
                        // and the card's existing Copy link context action.
                        if !gif_preview && history_link_activated(&selectable_link_label(ui,
                            RichText::new(url).size(self.prefs.chat_size)
                                .line_height(Some(self.prefs.chat_size * 1.45)).underline()
                                .color(Color32::from_rgb(119, 153, 255)))) {
                            self.open_message_link(ui.ctx(), url);
                        }
                        if resources::is_image_url(url)
                            && !m.components.iter().any(|component| matches!(component.kind, 11..=13)
                                && notdiscord_core::parsing::same_media_resource(&component.url, url))
                            && !m
                                .attachments
                                .iter()
                                .any(|a| notdiscord_core::parsing::same_media_resource(&a.url, url)
                                    || a.preview_url.as_deref().is_some_and(|preview| notdiscord_core::parsing::same_media_resource(preview, url)))
                        {
                            if let Some(index) = media.iter().position(|known| notdiscord_core::parsing::same_media_resource(known, url)) {
                                media[index] = notdiscord_core::parsing::prefer_media_url(media[index], url);
                            } else { media.push(url); }
                        }
                    }
                    end = whole.end();
                }
                if end < line.len() {
                    self.message_plain(ui, &line[end..], self.prefs.chat_size, self.theme.ink);
                }
            });
        }
        for url in media {
            let gif = notdiscord_core::parsing::direct_gif(url)
                || notdiscord_core::parsing::gif_page(url);
            self.media_card(ui, url, url, gif, if gif { "GIF" } else { "Imagen" }, None);
        }
    }
    fn message_plain(&mut self, ui: &mut egui::Ui, text: &str, size: f32, color: Color32) {
        use unicode_segmentation::UnicodeSegmentation;
        let mut start = 0;
        let mut count = 0;
        for (index, element) in text.grapheme_indices(true) {
            if count >= 64
                || !element
                    .chars()
                    .any(|ch| ch == '\u{fe0f}' || (0x1f000..=0x1faff).contains(&(ch as u32)))
            {
                continue;
            }
            if start < index {
                selectable_message_label(ui,
                    RichText::new(&text[start..index])
                        .size(size)
                        .line_height(Some(size * 1.45))
                        .color(color),
                );
            }
            self.message_emoji(
                ui,
                &StatusEmoji {
                    name: element.into(),
                    ..Default::default()
                },
                size * 1.3,
                element,
            );
            count += 1;
            start = index + element.len();
        }
        if start < text.len() {
            selectable_message_label(ui,
                RichText::new(&text[start..])
                    .size(size)
                    .line_height(Some(size * 1.45))
                    .color(color),
            );
        }
    }
    fn message_emoji(&mut self,ui:&mut egui::Ui,emoji:&StatusEmoji,size:f32,copy_text:&str) {
        // Keep the bitmap renderer and register a logical selection atom. Its
        // blank galley never adds the emoji to the egui fallback font atlas.
        let shape=ui.painter().add(egui::Shape::Noop);
        let image=self.emoji(ui,emoji,size);
        let active=ui.ctx().data_mut(|data|data.get_temp_mut_or_default::<HistoryFrame>(egui::Id::new("history-frame")).active);
        if !active {return;}
        let response=ui.interact(image.rect,image.id.with("history-emoji"),egui::Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Text);
        let galley=ui.painter().layout_no_wrap(" ".into(),egui::FontId::proportional(size),self.theme.ink);
        let label=HistoryLabel {id:response.id,layer:response.layer_id,position:response.rect.min,
            galley,clip:ui.clip_rect(),shape,fallback:self.theme.ink,double_clicked:response.double_clicked(),
            triple_clicked:response.triple_clicked(),link:false,
            atom:Some((response.rect,copy_text.to_owned())),
            owns_primary:response.enabled()&&(response.is_pointer_button_down_on()
                ||response.drag_started_by(egui::PointerButton::Primary)||response.clicked_by(egui::PointerButton::Primary))};
        ui.ctx().data_mut(|data|data.get_temp_mut_or_default::<HistoryFrame>(egui::Id::new("history-frame")).labels.push(label));
        record_message_text_gesture(ui.ctx(),&response);
    }
    fn message_menu(&mut self, ui: &mut egui::Ui, m: &ChatMessage, state: &AppSnapshot) {
        let writable = state.view == "chat" && state.signed_in && !state.busy && !m.deleted;
        if ui
            .add_enabled(
                writable && state.channel_access.can_send,
                egui::Button::new("Responder"),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.reply_to(m);
            ui.close();
        }
        if ui
            .add_enabled(
                writable && !m.forwarded && matches!(m.message_type, 0 | 19),
                egui::Button::new("Reenviar mensaje"),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.start_forward(m, state);
            ui.close();
        }
        if ui
            .add_enabled(
                writable && state.channel_access.can_react,
                egui::Button::new("Añadir reacción"),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.open_picker(false, state);
            if let Some(p) = &mut self.picker {
                p.react = Some((m.channel_id.clone(), m.id.clone()));
            }
            ui.close();
        }
        if state.can_pin_messages {
            if ui
                .add_enabled(!m.deleted && state.signed_in && !state.busy, egui::Button::new(if m.pinned {
                    "Desfijar mensaje"
                } else {
                    "Fijar mensaje"
                }))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.send(
                    if m.pinned {
                        "unpin-message"
                    } else {
                        "pin-message"
                    },
                    &m.id,
                    "",
                    "",
                    &m.channel_id,
                );
                ui.close();
            }
        }
        if state
            .current_user
            .as_ref()
            .is_some_and(|u| u.id == m.author.id)
        {
            if !m.forwarded
                && ui
                    .add_enabled(writable && state.channel_access.can_send, egui::Button::new("Editar mensaje"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
            {
                self.prompt_for(
                    "Editar mensaje",
                    "Contenido del mensaje",
                    "edit",
                    &m.id,
                    &m.content,
                    &m.channel_id,
                    true,
                );
                ui.close();
            }
            if ui
                .add_enabled(
                    writable && state.channel_access.can_send,
                    egui::Button::new(
                        RichText::new("Eliminar mensaje").color(Color32::from_rgb(242, 63, 67)),
                    ),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.confirm_message_delete(m);
                ui.close();
            }
        }
        if ui.button("Copiar texto").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            ui.ctx().copy_text(m.content.clone());
            ui.close();
        }
        if ui.button("Copiar enlace del mensaje").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            ui.ctx().copy_text(format!(
                "https://discord.com/channels/{}/{}/{}",
                if state.selected_guild_id.is_empty() {
                    "@me"
                } else {
                    &state.selected_guild_id
                },
                m.channel_id,
                m.id
            ));
            ui.close();
        }
        if ui.button("Copiar ID").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            ui.ctx().copy_text(m.id.clone());
            ui.close();
        }
    }
    fn confirm_message_delete(&mut self, message: &ChatMessage) {
        self.confirm("Eliminar mensaje", "¿Eliminar este mensaje? Esta acción no se puede deshacer.", UiAction {
            kind: "delete".into(), id: message.id.clone(), channel_id: message.channel_id.clone(), ..Default::default()
        });
    }
    fn reply_to(&mut self, m: &ChatMessage) {
        let Some(d) = self.draft(&m.channel_id) else {
            return;
        };
        d.reply = Some(MessageReplyPreview {
            message_id: m.id.clone(),
            channel_id: m.channel_id.clone(),
            author: Some(m.author.clone()),
            content: notdiscord_core::parsing::message_preview(m),
            mentions: m.mentions.clone(),
            ..Default::default()
        });
        d.revision += 1;
    }
    fn jump_message(&mut self, channel: &str, id: &str) {
        self.send("jump-message", id, "", "", channel);
        self.focus_id = id.into();
        self.focus_scroll_pending = true;
        self.focus_until = Some(Instant::now() + Duration::from_secs(2));
        self.scroll_bottom = false;
    }
    fn reference_message_actions(&mut self, ui: &mut egui::Ui, message: &ChatMessage,
        state: &AppSnapshot, key: egui::Id, row: egui::Rect, previous: Option<egui::Rect>) {
        let can_unpin = state.view == "pins" && state.can_pin_messages
            && message.channel_id == state.selected_channel_id;
        let width = previous.map_or(if can_unpin { 218. } else { 180. }, |rect| rect.width());
        let actions = egui::Area::new(key)
            .fixed_pos(egui::pos2((row.right() - width).max(row.left()), row.top()))
            .constrain_to(ui.clip_rect()).order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing.x = 4.;
                egui::Frame::new().fill(self.theme.panel).corner_radius(5).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!state.busy && !message.id.is_empty() && !message.channel_id.is_empty(),
                            egui::Button::new("Ir al mensaje original").small())
                            .on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.jump_message(&message.channel_id, &message.id);
                        }
                        if can_unpin && pin_action_button(ui, !state.busy && !message.deleted,
                            self.theme.ink, true).on_hover_text("Desfijar mensaje")
                            .on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.send("unpin-message", &message.id, "", "", &message.channel_id);
                        }
                    });
                });
            }).response;
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("message-actions-region"),
            MessageActionRegion { id: key, rect: actions.rect, row_origin: row.min, more: egui::Rect::NOTHING }));
    }
    fn open_message_link(&mut self, ctx: &egui::Context, url: &str) {
        // Synthetic interaction tests observe OpenUrl without launching the
        // user's browser. The production click keeps the native opener.
        #[cfg(test)]
        ctx.open_url(egui::OpenUrl::new_tab(url));
        #[cfg(not(test))]
        {
            let _ = ctx;
            if let Err(error) = crate::platform::open_link(url) { self.error = error; }
        }
    }
    fn start_forward(&mut self, m: &ChatMessage, state: &AppSnapshot) {
        if self.forward_pending {
            self.error = "Espera a que termine el reenvío en curso.".into();
            return;
        }
        if m.deleted || m.forwarded || !matches!(m.message_type, 0 | 19) {
            self.error = "Sólo se pueden reenviar mensajes originales disponibles.".into();
            return;
        }
        self.forward_scope.clear();
        self.forward_targets = state.direct_messages.clone();
        self.forward_selected.clear();
        self.forward_query.clear();
        self.forward_result = serde_json::json!({"channel_id":m.channel_id,"message_id":m.id});
        self.dialog = Some(Dialog::Forward(m.clone()));
    }
    fn media_card(
        &mut self,
        ui: &mut egui::Ui,
        url: &str,
        poster: &str,
        gif: bool,
        name: &str,
        dimensions: Option<(u32, u32)>,
    ) {
        if notdiscord_core::parsing::gif_page(url) && notdiscord_core::parsing::direct_gif(poster) {
            if resources::remember_source(url, poster) {
                self.pictures.retry(url, 384);
                self.pictures.retry(url, 1600);
            }
        }
        let max = egui::vec2(ui.available_width().min(360.), 220.);
        let size = media_slot_size(max, dimensions);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        if ui.is_rect_visible(rect) {
            let texture = self
                .pictures
                .animated(ui.ctx(), url)
                .or_else(|| self.pictures.texture(poster))
                .or_else(|| (poster != url && self.pictures.image_error(poster, 384).is_some())
                    .then(|| self.pictures.texture(url)).flatten());
            if let Some(texture) = texture {
                let image_size = texture.size_vec2();
                let scale = (rect.width() / image_size.x)
                    .min(rect.height() / image_size.y)
                    .min(1.);
                let image_rect = egui::Rect::from_center_size(rect.center(), image_size * scale);
                ui.painter().image(
                    texture.id(),
                    image_rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    Color32::WHITE,
                );
            } else {
                ui.painter().rect_filled(rect, 5., self.theme.surface);
                let label = if let Some(error) = self.pictures.image_error(url, 384).or_else(|| self.pictures.image_error(poster, 384)) {
                    format!(
                        "Imagen no disponible\n{error}\nClic para abrir el visor · Botón derecho para reintentar"
                    )
                } else {
                    "Cargando vista previa…".into()
                };
                let galley = ui.painter().layout(
                    label,
                    egui::FontId::proportional(12.),
                    self.theme.muted,
                    (rect.width() - 16.).max(1.),
                );
                ui.painter().with_clip_rect(rect.shrink(6.)).galley(
                    rect.center() - galley.size() / 2.,
                    galley,
                    self.theme.muted,
                );
            }
        }
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            self.viewer = Some(Viewer {
                url: url.into(),
                local_path: None,
                zoom: 1.,
                pan: Vec2::ZERO,
            });
        }
        response.context_menu(|ui| {
            if ui.button("Reintentar vista previa").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.pictures.retry(poster, 384);
                self.pictures.retry(url, 384);
                ui.close();
            }
            self.media_menu(ui, url, name);
        });
        let preview_limit = self.pictures.image_error(url, 384)
            .or_else(|| self.pictures.image_error(poster, 384))
            .filter(|error| super::media_limit::is_limit_error(error))
            .map(str::to_owned);
        if let Some(error) = preview_limit {
            self.media_error_link(ui, &error);
        }
        if gif {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.prefs.allow_gif_playback && !self.pictures.animation_loading(url),
                        egui::Button::new(
                            if self
                                .pictures
                                .animation
                                .as_ref()
                                .is_some_and(|a| a.url == url)
                            {
                                "Ⅱ Detener"
                            } else if self.pictures.animation_loading(url) {
                                "Cargando GIF…"
                            } else {
                                "▶ GIF"
                            },
                        ),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    if self
                        .pictures
                        .animation
                        .as_ref()
                        .is_some_and(|a| a.url == url)
                    {
                        self.pictures.stop();
                    } else {
                        self.pictures.play(ui.ctx(), url.into());
                    }
                }
                if ui.small_button("Ampliar").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    self.viewer = Some(Viewer {
                        url: url.into(),
                        local_path: None,
                        zoom: 1.,
                        pan: Vec2::ZERO,
                    });
                }
            });
            if let Some(error) = self.pictures.animation_error(url).map(str::to_owned) {
                self.media_error_link(ui, &error);
            }
        }
    }
    pub(super) fn media_menu(&mut self, ui: &mut egui::Ui, url: &str, name: &str) {
        if ui.button("Abrir imagen").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.viewer = Some(Viewer {
                url: url.into(),
                local_path: None,
                zoom: 1.,
                pan: Vec2::ZERO,
            });
            ui.close();
        }
        if ui.button("Copiar imagen").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.copy_image(url);
            ui.close();
        }
        if ui.button("Copiar enlace").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            ui.ctx().copy_text(url.into());
            ui.close();
        }
        if ui.button("Guardar original").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.save_media(url, name);
            ui.close();
        }
    }
    pub(super) fn poll_clipboard_attachments(&mut self) {
        for mut pasted in self.pictures.take_pasted() {
            let path = match pasted.take_result() {
                Ok(Some(path)) => path,
                Ok(None) => continue,
                Err(error) => {
                    self.error = error;
                    continue;
                }
            };
            self.owned_temporary.insert(path.clone());
            let account = self
                .state
                .current_user
                .as_ref()
                .map(|p| p.id.as_str())
                .unwrap_or("");
            if !self.state.signed_in
                || account != pasted.account
                || (!self.drafts.contains_key(&pasted.channel) && self.drafts.len() >= 1024)
            {
                self.cleanup_temporary(&[path]);
                continue;
            }
            let mut draft = self.drafts.remove(&pasted.channel).unwrap_or_default();
            add_attachment(&mut draft, path.clone(), &mut self.error);
            if !draft.paths.contains(&path) {
                self.cleanup_temporary(&[path]);
            }
            if !self.draft_order.contains(&pasted.channel) {
                self.draft_order.push_back(pasted.channel.clone());
            }
            self.drafts.insert(pasted.channel.clone(), draft);
        }
    }
    fn draw_composer(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        self.emoji_section(ui, ("chat-composer-emojis", &state.selected_channel_id), |this, ui| {
            this.draw_composer_contents(ui, state)
        });
    }
    fn draw_composer_contents(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        prepare_composer_input(ui.ctx());
        let channel = &state.selected_channel_id;
        let mut draft = self.drafts.remove(channel).unwrap_or_default();
        let room = self.drafts.len() < 1024;
        let allowed = ui.is_enabled()
            && state.signed_in
            && !state.busy
            && state.channel_access.can_send
            && room;
        if !room {
            self.error = "Hay 1024 borradores abiertos. Borra uno antes de preparar otro.".into();
        }
        let editor_id = egui::Id::new(("composer", channel));
        let initial_focus = ui.ctx().data(|data| data.get_temp::<(u64, Option<egui::Id>)>(
            egui::Id::new("composer-frame-focus"))).and_then(|(_, focus)| focus);
        let ime_active = composer_ime_active(ui.ctx());
        let contextual_enter = allowed && state.view == "chat" && !channel.is_empty()
            && !ime_active && self.submissions.len() < 64
            && (!draft.text.trim().is_empty() || !draft.paths.is_empty())
            && (self.mention_closed || completion::query(&draft.text, self.composer_cursor).is_none());
        if allowed && !self.background_blocked() && !egui::Popup::is_any_open(ui.ctx())
            && !ui.ctx().wants_keyboard_input()
            && initial_focus.is_none_or(|focus| focus == editor_id)
            && ui.ctx().input(|input| composer_printable_input(input)
                || contextual_enter && composer_send_enter(input) && !input.pointer.any_down()) {
            // Request before constructing TextEdit, so it receives this pass's
            // untouched Text/Enter event. Enter then uses the ordinary editor
            // submission path, including mentions, attachments and late acks.
            ui.memory_mut(|memory| memory.request_focus(editor_id));
        }
        // egui clears focused_widget on Escape during begin_pass, before this
        // UI runs. Recover only this editor's previous ownership to let Escape
        // close its visible completion instead of leaving suggestions behind.
        let escape_pressed=ui.ctx().input(|input|input.key_pressed(egui::Key::Escape));
        let editor_owned_input = ui.memory(|memory| memory.has_focus(editor_id)
            || escape_pressed && initial_focus.is_none() && memory.had_focus_last_frame(editor_id));
        let completion_keyboard = allowed && editor_owned_input && !ime_active
            && ui.ctx().input(|input| input.focused)
            && !self.background_blocked() && !egui::Popup::is_any_open(ui.ctx());
        let previous_mention = completion::query(&draft.text, self.composer_cursor);
        sync_completion_query(ui.ctx(), editor_id, &previous_mention,
            &mut self.mention_index, &mut self.mention_closed);
        let previous_choices = previous_mention.as_ref().filter(|_| !self.mention_closed)
            .map(|(_, query)| completion::cached_choices(ui.ctx(),query, state, 8)).unwrap_or_default();
        // TextEdit otherwise gives Tab to focus traversal before the popup can
        // handle it. Reserve completion keys before constructing the editor;
        // clear egui's already queued traversal as well as consuming the event.
        let mut completion_accept = false;
        if allowed && !ime_active && state.view=="chat" && !channel.is_empty()
            && initial_focus.is_none_or(|focus|focus==editor_id)
            && !self.background_blocked() && !egui::Popup::is_any_open(ui.ctx())
            && previous_choices.is_empty()
            && ui.ctx().input(|input|input.focused&&!input.pointer.any_down()
                && input.key_pressed(egui::Key::Tab)&&input.modifiers==egui::Modifiers::NONE) {
            let selection=egui::TextEdit::load_state(ui.ctx(),editor_id).and_then(|editor|editor.cursor.char_range())
                .map(|range|(range.primary.index.min(range.secondary.index),range.primary.index.max(range.secondary.index)))
                .unwrap_or((self.composer_cursor,self.composer_cursor));
            let incoming=ui.ctx().input(|input|completion::query_before_tab(&draft.text,selection,&input.events));
            if incoming.as_ref().is_some_and(|(_,query)|!completion::cached_choices(ui.ctx(),query,state,8).is_empty()) {
                // Tab can already have focused a header/button earlier in this
                // pass. The captured start-of-pass owner decides whether this
                // text belongs to chat; never steal another editor's input.
                ui.ctx().input_mut(|input|{input.consume_key(egui::Modifiers::NONE,egui::Key::Tab);});
                completion_accept=true;
                ui.memory_mut(|memory|{memory.move_focus(egui::FocusDirection::None);memory.request_focus(editor_id);});
            }
        }
        if completion_keyboard && !previous_choices.is_empty() {
            let consumed = ui.ctx().input_mut(|input| {
                let mut consumed = false;
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    self.mention_index = (self.mention_index + 1) % previous_choices.len();
                    consumed = true;
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    self.mention_index = (self.mention_index + previous_choices.len() - 1) % previous_choices.len();
                    consumed = true;
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                    || consume_composer_send_enter(input) {
                    completion_accept = true;
                    consumed = true;
                }
                if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    self.mention_closed = true;
                    consumed = true;
                }
                consumed
            });
            if consumed {
                ui.memory_mut(|memory| {
                    memory.move_focus(egui::FocusDirection::None);
                    memory.request_focus(editor_id);
                });
            }
        }
        let capture_ready = allowed && ui.memory(|memory| memory.has_focus(editor_id))
            && !self.background_blocked() && !egui::Popup::is_any_open(ui.ctx())
            && !ime_active
            && self.submissions.len() < 64
            && (!draft.text.trim().is_empty() || !draft.paths.is_empty())
            && (self.mention_closed || completion::query(&draft.text, self.composer_cursor).is_none());
        let after_enter = ui.ctx().input_mut(|input| take_post_send_text(input, capture_ready));
        let focused = ui.is_enabled()
            && ui.memory(|m| m.has_focus(editor_id))
            && ui.ctx().input(|i| i.focused)
            && crate::platform::foreground_is_current_process();
        let previous = ui.ctx().data(|d| {
            d.get_temp::<(bool, bool)>(egui::Id::new(("composer-paste", channel)))
                .unwrap_or_default()
        });
        let (down, pressed) = if focused {
            ui.ctx().input(|i| {
                crate::platform::paste_key_state(
                    i.modifiers.ctrl || i.modifiers.command,
                    i.modifiers.shift,
                )
            })
        } else {
            (false, false)
        };
        let paste_event = ui.ctx().input(|i| {
            i.events
                .iter()
                .any(|event| matches!(event, egui::Event::Paste(_)))
        });
        let mut paste = allowed
            && state.channel_access.can_attach
            && paste_gesture(focused, previous.0, down, pressed, paste_event, previous.1)
            && crate::platform::clipboard_has_image();
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::new(("composer-paste", channel)), (focused, down))
        });
        if paste {
            ui.ctx().input_mut(|i| {
                i.events
                    .retain(|event| !matches!(event, egui::Event::Paste(_)))
            });
        }
        let mut send = false;
        let mut discard = false;
        let mut reply_jump = None;
        let editor_height = composer_text_height(ui, &draft.text, self.prefs.chat_size);
        egui::Frame::new()
            .fill(self.theme.surface)
            .corner_radius(8)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                if let Some(reply) = draft.reply.clone() {
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 26.).max(1.);
                        let response = ui
                            .allocate_ui_with_layout(
                                egui::vec2(width, reply_preview_height(ui, self.prefs.chat_size)),
                                egui::Layout::left_to_right(Align::Center),
                                |ui| {
                                    reply_preview(
                                        ui,
                                        &format!(
                                            "↳ {}: {}",
                                            reply
                                                .author
                                                .as_ref()
                                                .map(|p| reply_display_name(&reply, p, state))
                                                .unwrap_or("Mensaje"),
                                            short(&readable_mentions(&reply.content, &reply.mentions, state), 90)
                                        ),
                                        self.prefs.chat_size,
                                        self.theme.muted,
                                        self.theme.selected,
                                        !reply.deleted && !reply.unavailable,
                                    )
                                },
                            )
                            .inner;
                        if response.clicked() {
                            reply_jump = Some((reply.channel_id.clone(), reply.message_id.clone()));
                        }
                        if ui
                            .small_button(
                                RichText::new("×")
                                    .size(reply_preview_font_size(self.prefs.chat_size)),
                            )
                            .on_hover_text("Cancelar respuesta")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            draft.reply = None;
                            draft.revision += 1;
                        }
                    });
                }
                let mut remove = None;
                if !draft.paths.is_empty() {
                    theme::horizontal_scroll(ui)
                        .id_salt(("prepared-attachments", channel))
                        .max_height(prepared_attachments_height(ui,draft.paths.len()))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, path) in draft.paths.iter().enumerate() {
                                    egui::Frame::new()
                                        .fill(self.theme.panel)
                                        .corner_radius(5)
                                        .stroke(egui::Stroke::new(1.0_f32,self.theme.line))
                                        .inner_margin(egui::Margin::same(8))
                                        .show(ui, |ui| {
                                            // The surrounding row is horizontal; every card
                                            // explicitly stacks its own contents vertically.
                                            ui.vertical(|ui| {
                                                ui.set_width(PREPARED_CARD_WIDTH);
                                                let previewable = previewable_path(path);
                                                let (slot,response)=ui.allocate_exact_size(egui::vec2(PREPARED_CARD_WIDTH,PREPARED_PREVIEW_HEIGHT),if previewable { egui::Sense::click() } else { egui::Sense::hover() });
                                                ui.painter().rect_filled(slot,4,self.theme.base);
                                                let side=(PREPARED_CARD_WIDTH*ui.ctx().pixels_per_point()).ceil() as u32;
                                                if previewable {
                                                    if let Some(texture)=self.pictures.texture_path(path,side) {
                                                        let dimensions=texture.size();
                                                        let size=media_slot_size(slot.size(),Some((dimensions[0] as u32,dimensions[1] as u32)));
                                                        let rect=egui::Rect::from_center_size(slot.center(),size);
                                                        egui::Image::new(&texture).paint_at(ui,rect);
                                                    } else {
                                                        let text=self.pictures.path_error(path,side).unwrap_or("Preparando vista previa…");
                                                        ui.scope_builder(egui::UiBuilder::new().max_rect(slot).layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)),|ui| {
                                                            ui.add(egui::Label::new(RichText::new(text).size(12.)).wrap());
                                                        });
                                                    }
                                                } else {
                                                    ui.painter().text(slot.center(),egui::Align2::CENTER_CENTER,"Archivo adjunto",egui::FontId::proportional(12.),self.theme.muted);
                                                }
                                                if previewable && response.on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .on_hover_text("Abrir vista previa del adjunto sin enviarlo").clicked() {
                                                    self.viewer = Some(Viewer { url: String::new(), local_path: Some(path.clone()), zoom: 1., pan: Vec2::ZERO });
                                                }
                                                let name=if self.owned_temporary.contains(path) {"Imagen pegada.png".into()}else{path.file_name().unwrap_or_default().to_string_lossy()};
                                                ui.add(egui::Label::new(RichText::new(name).size(13.)).truncate());
                                                ui.add(egui::Label::new(RichText::new(prepared_file_size(ui.ctx(),path)).size(12.).color(self.theme.muted)).truncate());
                                                ui.scope(|ui|{
                                                    ui.spacing_mut().button_padding.y=3.;
                                                    if ui.add_sized([PREPARED_CARD_WIDTH,prepared_remove_height(ui)],egui::Button::new(RichText::new("Quitar").size(12.))).on_hover_text("Quitar adjunto").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {remove=Some(index);}
                                                });
                                            });
                                        });
                                }
                            });
                        });
                    ui.add_space(3.);
                }
                if let Some(index) = remove {
                    let removed = draft.paths.remove(index);
                    self.cleanup_unreferenced_temporary(&[removed]);
                    draft.revision += 1;
                }
                let emoji_tokens = composer_custom_preview_tokens(&draft.text);
                if !emoji_tokens.is_empty() {
                    let side = composer_emoji_preview_side(self.prefs.chat_size);
                    let height = composer_emoji_preview_height(ui, self.prefs.chat_size);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
                    let mut preview_ui = ui.new_child(egui::UiBuilder::new()
                        .id_salt(("composer-emoji-preview", channel)).max_rect(rect)
                        .layout(egui::Layout::top_down(Align::Min)));
                    preview_ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                    theme::horizontal_scroll(&mut preview_ui).id_salt("draft-custom-emojis")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .min_scrolled_height(side).max_height(height).auto_shrink([false, false])
                        .show(&mut preview_ui, |ui| {
                            ui.horizontal(|ui| {
                                for token in emoji_tokens {
                                    let (slot, response) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
                                    let mut emoji_ui = ui.new_child(egui::UiBuilder::new().max_rect(slot)
                                        .layout(egui::Layout::top_down(Align::Min)));
                                    emoji_ui.set_clip_rect(slot.intersect(ui.clip_rect()));
                                    emoji_ui.style_mut().interaction.selectable_labels = false;
                                    self.rich_custom_text(&mut emoji_ui, token, state, side / 1.4);
                                    response.on_hover_text(token);
                                }
                            });
                        });
                }
                let old = draft.text.clone();
                let response = ui
                    .horizontal_top(|ui| {
                        let gaps = ui.spacing().item_spacing.x * 4.;
                        let width = (ui.available_width() - 28. - 36. - 28. - 32. - gaps).max(30.);
                        if ui
                            .add_enabled(
                                allowed && state.channel_access.can_attach,
                                egui::Button::new("+").min_size(egui::vec2(28., 30.)),
                            )
                            .on_hover_text("Adjuntar archivos")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            if let Some(paths) = rfd::FileDialog::new().pick_files() {
                                for path in paths {
                                    add_attachment(&mut draft, path, &mut self.error);
                                }
                            }
                        }
                        let response = theme::vertical_scroll(ui)
                            .id_salt(("composer-text-scroll", channel))
                            .max_width(width)
                            .max_height(editor_height)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                let spans = draft.mentions.clone();
                                let size = self.prefs.chat_size;
                                let ink = self.theme.ink;
                                let broadcast = super::mentions::everyone_allowed(state);
                                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                                    ui.fonts_mut(|fonts| fonts.layout_job(super::mentions::editor_job(text.as_str(), &spans, size, ink, broadcast, wrap_width)))
                                };
                                ui.add_enabled(
                                    allowed,
                                    egui::TextEdit::multiline(&mut draft.text)
                                        .id(editor_id)
                                        .font(egui::FontId::proportional(self.prefs.chat_size))
                                        .frame(false)
                                        .margin(egui::Margin::symmetric(4, 5))
                                        .return_key(egui::KeyboardShortcut::new(
                                            egui::Modifiers::SHIFT,
                                            egui::Key::Enter,
                                        ))
                                        .desired_width(width)
                                        .desired_rows(1)
                                        .layouter(&mut layouter)
                                        .hint_text(RichText::new(if allowed {
                                            "Escribe un mensaje…"
                                        } else {
                                            &state.channel_access.reason
                                        }).color(self.theme.muted)),
                                )
                            })
                            .inner;
                        if ui
                            .add_enabled(
                                allowed,
                                egui::Button::new("GIF").min_size(egui::vec2(36., 30.)),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.open_picker(true, state);
                        }
                        if ui
                            .add_enabled(
                                allowed,
                                egui::Button::new("☺").min_size(egui::vec2(28., 30.)),
                            )
                            .on_hover_text("Emojis")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            self.open_picker(false, state);
                        }
                        if ui
                            .add_enabled(
                                allowed
                                    && (!draft.text.trim().is_empty() || !draft.paths.is_empty()),
                                egui::Button::new("➤").min_size(egui::vec2(32., 30.)),
                            )
                            .on_hover_text("Enviar mensaje")
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked()
                        {
                            send = true;
                        }
                        response
                    })
                    .inner;
                response.context_menu(|ui| {
                    if ui
                        .add_enabled(
                            allowed && state.channel_access.can_attach,
                            egui::Button::new("Pegar imagen"),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        paste = true;
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !draft.text.is_empty()
                                || !draft.paths.is_empty()
                                || draft.reply.is_some(),
                            egui::Button::new("Borrar borrador"),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        discard = true;
                        ui.close();
                    }
                });
                if let Some(note) = super::mentions::broadcast_note(&draft.text, state) {
                    response.clone().on_hover_text(note);
                }
                let output =
                    egui::TextEdit::load_state(ui.ctx(), egui::Id::new(("composer", channel)));
                if let Some(range) = output.and_then(|s| s.cursor.char_range()) {
                    self.composer_cursor = range.primary.index.min(draft.text.chars().count());
                }
                if response.changed() {
                    adjust_mentions(&old, &draft.text, &mut draft.mentions);
                    draft.revision += 1;
                    self.mention_closed = false;
                    if self.prefs.send_typing
                        && !draft.text.trim().is_empty()
                        && self.last_typing.elapsed() > Duration::from_secs(8)
                    {
                        self.send("typing", "", "", "", channel);
                        self.last_typing = Instant::now();
                    }
                }
                let mention = completion::query(&draft.text, self.composer_cursor);
                if allowed && mention.as_ref().is_some_and(|(_,query)|query.starts_with(':'))
                    && !state.selected_guild_id.is_empty()
                    && !state.emojis.iter().any(|emoji|emoji.guild_id==state.selected_guild_id) {
                    let guild=&state.selected_guild_id;
                    let owner=format!("{}:{guild}",state.current_user.as_ref().map(|person|person.id.as_str()).unwrap_or(""));
                    let request=ui.ctx().data_mut(|data| {
                        let key=egui::Id::new("composer-emoji-catalog-request");
                        if data.get_temp::<(String,Instant)>(key).is_some_and(|(previous,at)|previous==owner
                            && (!response.changed()||at.elapsed()<Duration::from_secs(60))) {false}
                        else {data.insert_temp(key,(owner,Instant::now()));true}
                    });
                    if request {self.send("guild-emojis",guild,"","",channel);}
                }
                sync_completion_query(ui.ctx(), editor_id, &mention,
                    &mut self.mention_index, &mut self.mention_closed);
                let choices: Vec<_> = mention
                    .as_ref()
                    .filter(|_| !self.mention_closed)
                    .map(|(_, query)| completion::cached_choices(ui.ctx(),query, state, 8))
                    .unwrap_or_default();
                self.mention_index = self.mention_index.min(choices.len().saturating_sub(1));
                let mut choose = completion_accept.then_some(self.mention_index).filter(|_| !choices.is_empty());
                // Text and Tab may arrive in the same input batch, so the
                // token can appear only after TextEdit has processed its text.
                let batched_completion_tab = completion_keyboard && !choices.is_empty()
                    && ui.ctx().input(|input| input.key_pressed(egui::Key::Tab)
                        && input.modifiers == egui::Modifiers::NONE);
                if allowed && (response.has_focus() || batched_completion_tab) && !ime_active
                    && ui.ctx().input(|input| input.focused)
                    && !self.background_blocked() && !egui::Popup::is_any_open(ui.ctx()) {
                    ui.ctx().input_mut(|input| {
                        if !choices.is_empty() {
                            if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                                self.mention_index = (self.mention_index + 1) % choices.len();
                            }
                            if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                                self.mention_index =
                                    (self.mention_index + choices.len() - 1) % choices.len();
                            }
                            if input.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                                || consume_composer_send_enter(input)
                            {
                                choose = Some(self.mention_index.min(choices.len() - 1));
                            }
                            if input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                                self.mention_closed = true;
                            }
                        } else if consume_composer_send_enter(input) {
                            send = true;
                        }
                    });
                }
                if !choices.is_empty() && !self.mention_closed {
                    if let Some(index)=self.draw_completions(ui.ctx(),response.rect,&choices,self.mention_index) {choose=Some(index);}
                }
                if let Some(index) = choose {
                    if let Some((start, _)) = mention {
                        let Some(cursor)=completion::insert(&mut draft,start,self.composer_cursor,&choices[index]) else {return;};
                        self.composer_cursor = cursor;
                        let editor_id = egui::Id::new(("composer", channel));
                        if let Some(mut editor) = egui::TextEdit::load_state(ui.ctx(), editor_id) {
                            editor
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(
                                    egui::text::CCursor::new(self.composer_cursor),
                                )));
                            editor.store(ui.ctx(), editor_id);
                        }
                        ui.memory_mut(|memory| {
                            memory.move_focus(egui::FocusDirection::None);
                            memory.request_focus(editor_id);
                        });
                        self.mention_closed = true;
                        self.mention_index = 0;
                    }
                }
            });
        if discard {
            self.cleanup_unreferenced_temporary(&draft.paths);
            draft = Draft::default();
        }
        if let Some((channel, id)) = reply_jump {
            self.jump_message(&channel, &id);
        }
        for dropped in ui.ctx().input(|i| {
            if allowed && state.channel_access.can_attach {
                i.raw.dropped_files.clone()
            } else {
                Vec::new()
            }
        }) {
            if let Some(path) = dropped.path {
                add_attachment(&mut draft, path, &mut self.error);
            }
        }
        if paste {
            let account = state
                .current_user
                .as_ref()
                .map(|p| p.id.as_str())
                .unwrap_or("");
            if let Err(error) = self.pictures.paste_bitmap(
                channel,
                account,
                self.data_dir.join("draft-attachments"),
            ) {
                self.error = error;
            }
        }
        if send && allowed && (!draft.text.trim().is_empty() || !draft.paths.is_empty()) {
            let text = outgoing_mentions(&draft);
            if text.encode_utf16().count() > 4000 {
                self.error="El mensaje con sus menciones supera el límite de 4000 caracteres. El borrador se conserva.".into();
                restore_post_send_text(&mut draft, &after_enter, self.composer_cursor);
                self.drafts.insert(channel.clone(), draft);
                return;
            }
            if self.submissions.len() >= 64 {
                self.error = "Hay 64 envíos pendientes. Reintenta o descarta uno; el borrador actual se conserva.".into();
            } else {
                let submission = uuid::Uuid::new_v4().simple().to_string();
                let mut captured = std::mem::take(&mut draft);
                captured.captured_at = Some(Instant::now());
                self.submitted_paths.insert(submission.clone(), captured.paths.clone());
                self.submissions.insert(submission.clone(), (channel.clone(), captured.revision));
                self.sent_drafts.insert(submission.clone(), captured);
                self.send_captured_draft(&submission, channel);
                if self.near_recent_history(ui.ctx(), state) {
                    self.prepare_latest_scroll_for(channel);
                }
                draft.text = after_enter.clone();
                if !draft.text.is_empty() { draft.revision = 1; }
                self.composer_cursor = draft.text.chars().count();
                if let Some(mut editor) = egui::TextEdit::load_state(ui.ctx(), editor_id) {
                    editor.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(self.composer_cursor))));
                    editor.store(ui.ctx(), editor_id);
                }
            }
        } else if !after_enter.is_empty() {
            // Completion, a canceled send or a rejected gesture retains the
            // post-Enter text at its logical cursor instead of dropping it.
            restore_post_send_text(&mut draft, &after_enter, self.composer_cursor);
        }
        if !draft.text.is_empty() || draft.reply.is_some() || !draft.paths.is_empty() {
            if !self.draft_order.contains(channel) {
                self.draft_order.push_back(channel.clone());
            }
            self.drafts.insert(channel.clone(), draft);
        } else {
            self.draft_order.retain(|c| c != channel);
        }
        if !state.typing_text.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(&state.typing_text)
                        .size(12.)
                        .color(self.theme.muted),
                )
                .truncate(),
            );
        }
    }
    fn send_captured_draft(&mut self, submission: &str, channel: &str) {
        let Some(draft) = self.sent_drafts.get(submission) else { return; };
        let text = outgoing_mentions(draft);
        let extra = serde_json::json!({"replyId":draft.reply.as_ref().map(|r|r.message_id.as_str()).unwrap_or(""),
            "paths":draft.paths.iter().map(|p|p.to_string_lossy()).collect::<Vec<_>>(),
            "submissionId":submission,"revision":draft.revision}).to_string();
        self.failed_submissions.remove(submission);
        if self.command.send(UiAction { kind: "send-prepared".into(), text, extra, channel_id: channel.into(), ..Default::default() }).is_err() {
            self.failed_submissions.insert(submission.to_owned());
            self.error = "El servicio de envío se cerró; el mensaje pendiente se conserva para recuperarlo.".into();
        }
    }
    fn draw_pending_messages(&mut self, ui: &mut egui::Ui, state: &AppSnapshot) {
        let channel = &state.selected_channel_id;
        let mut pending = self.submissions.iter().filter(|(_, (owner, _))| owner == channel)
            .filter_map(|(id, _)| self.sent_drafts.get(id).map(|draft| (id.clone(), draft.clone())))
            .collect::<Vec<_>>();
        pending.sort_by(|a, b| a.1.captured_at.cmp(&b.1.captured_at).then(a.0.cmp(&b.0)));
        for (id, draft) in pending {
            let failed = self.failed_submissions.contains(&id);
            egui::Frame::new().fill(self.theme.panel).corner_radius(4).inner_margin(6.).show(ui, |ui| {
                ui.label(RichText::new(if failed { "Envío sin confirmar" } else { "Enviando…" }).size(12.).color(self.theme.muted));
                if !draft.text.is_empty() { ui.label(RichText::new(short(&draft.text, 300)).size(self.prefs.chat_size).color(self.theme.muted)); }
                if !draft.paths.is_empty() { ui.small(format!("{} archivo(s) adjunto(s)", draft.paths.len())); }
                if failed {
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(state.channel_access.can_send && !state.busy, egui::Button::new("Reintentar")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            // Same submission -> same server nonce, including after ambiguous failure.
                            self.send_captured_draft(&id, channel);
                        }
                        let empty = self.drafts.get(channel).is_none_or(|next| next.text.is_empty() && next.paths.is_empty() && next.reply.is_none());
                        if ui.add_enabled(empty, egui::Button::new("Recuperar borrador"))
                            .on_hover_text("Disponible cuando el siguiente borrador esté vacío; nunca lo sobrescribe.")
                            .on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.drafts.insert(channel.clone(), draft.clone());
                            self.sent_drafts.remove(&id); self.submissions.remove(&id);
                            self.submitted_paths.remove(&id); self.failed_submissions.remove(&id);
                        }
                        if ui.button("Copiar texto").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { ui.ctx().copy_text(draft.text.clone()); }
                        if ui.button("Descartar pendiente").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            self.sent_drafts.remove(&id); self.submissions.remove(&id);
                            self.submitted_paths.remove(&id); self.failed_submissions.remove(&id);
                            self.cleanup_unreferenced_temporary(&draft.paths);
                        }
                    });
                }
            });
        }
    }
}
const CHAT_COMPOSER_GAP: f32 = 10.;

fn history_wheel_target(ctx: &egui::Context, viewport: egui::Rect, layer: egui::LayerId) -> bool {
    let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else { return false; };
    if !viewport.contains(pointer) || interaction::over_panel(ctx, pointer) { return false; }
    let top = ctx.layer_id_at(pointer);
    if top == Some(layer) { return true; }
    ctx.data(|data| data.get_temp::<MessageActionRegion>(egui::Id::new("message-actions-region")))
        .is_some_and(|actions| actions.rect.intersect(viewport).contains(pointer)
            && top == Some(egui::LayerId::new(egui::Order::Foreground, actions.id)))
}

fn history_manual_input(ctx: &egui::Context, scope: egui::Id, layer: egui::LayerId) -> bool {
    let selection = ctx.data(|data| data.get_temp::<HistorySelection>(egui::Id::new("history-selection")))
        .is_some_and(|selection| selection.scope == Some(scope) && selection.dragging);
    let middle = ctx.data(|data| data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll")))
        .is_some_and(|middle| middle.scope == Some(scope) && middle.origin.is_some());
    let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport")));
    let (pressed, down, focused, pointer) = ctx.input(|input|
        (input.pointer.primary_pressed(), input.pointer.primary_down(), input.focused, input.pointer.interact_pos()));
    let owns_press = pointer.is_some_and(|pointer| interaction::owns_surface(ctx, pointer, layer));
    let drag = ctx.data_mut(|data| {
        let key = egui::Id::new("history-pointer-gesture");
        let mut active = data.get_temp::<(egui::Id, bool)>(key).unwrap_or((scope, false));
        if active.0 != scope || !down || !focused { active = (scope, false); }
        if pressed && focused && owns_press {
            active.1 = viewport.zip(pointer).is_some_and(|(viewport, pointer)| viewport.expand(20.).contains(pointer));
        }
        // A scrollbar retains its drag when the pointer leaves its track.
        // Do not give anchor restoration ownership until the button is released.
        data.insert_temp(key, active);
        active.1
    });
    let gesture = viewport.is_some_and(|viewport| {
        let wheel = history_wheel_target(ctx, viewport, layer)
            && ctx.input(|input| input.smooth_scroll_delta != egui::Vec2::ZERO || input.raw_scroll_delta != egui::Vec2::ZERO);
        wheel
    });
    selection || middle || drag || gesture
}

fn take_post_send_text(input: &mut egui::InputState, capture_ready: bool) -> String {
    if !capture_ready { return String::new(); }
    let Some(enter) = input.events.iter().position(|event| matches!(event,
        egui::Event::Key { key: egui::Key::Enter, pressed: true, repeat: false, modifiers, .. }
        if *modifiers == egui::Modifiers::NONE)) else { return String::new(); };
    let tail = &input.events[enter + 1..];
    // Keep complex IME/key/pointer gestures on TextEdit's existing path. The
    // normal rapid-typing case is text/paste immediately after Enter in one pass.
    if !tail.iter().all(|event| matches!(event, egui::Event::Text(_) | egui::Event::Paste(_)
        | egui::Event::Key { key: egui::Key::Enter, pressed: false, .. })) {
        return String::new();
    }
    let mut text = String::new();
    for event in tail {
        if let egui::Event::Text(value) | egui::Event::Paste(value) = event { text.push_str(value); }
    }
    if !text.is_empty() {
        let mut index = 0;
        input.events.retain(|event| {
            let keep = index <= enter || !matches!(event, egui::Event::Text(_) | egui::Event::Paste(_));
            index += 1;
            keep
        });
    }
    text
}

fn restore_post_send_text(draft: &mut Draft, text: &str, cursor: usize) {
    if text.is_empty() { return; }
    let old = draft.text.clone();
    let mut chars = draft.text.chars().collect::<Vec<_>>();
    let at = cursor.min(chars.len());
    chars.splice(at..at, text.chars());
    draft.text = chars.into_iter().collect();
    adjust_mentions(&old, &draft.text, &mut draft.mentions);
    draft.revision += 1;
}

pub(super) fn composer_printable_input(input: &egui::InputState) -> bool {
    let alt_gr = cfg!(target_os="windows") && input.modifiers.ctrl && input.modifiers.alt && !input.modifiers.mac_cmd;
    input.focused && (alt_gr || !input.modifiers.ctrl && !input.modifiers.command && !input.modifiers.alt)
        && input.events.iter().any(|event| matches!(event, egui::Event::Text(text) if text.chars().any(|ch| !ch.is_control())))
}

pub(super) fn prepare_composer_input(ctx: &egui::Context) {
    let frame = ctx.cumulative_frame_nr();
    let key = egui::Id::new("composer-frame-focus");
    if ctx.data(|data| data.get_temp::<(u64, Option<egui::Id>)>(key))
        .is_some_and(|(stored, _)| stored == frame) { return; }
    // Capture before any widget can release focus while handling Enter. That
    // Enter belongs to its original control even if focus is now empty.
    let focus = ctx.memory(|memory| memory.focused());
    ctx.data_mut(|data| data.insert_temp(key, (frame, focus)));
}

fn composer_send_enter(input: &egui::InputState) -> bool {
    input.focused && input.modifiers == egui::Modifiers::NONE
        && input.events.iter().any(|event| matches!(event,
            egui::Event::Key { key: egui::Key::Enter, pressed: true, repeat: false, modifiers, .. }
            if *modifiers == egui::Modifiers::NONE))
}

fn consume_composer_send_enter(input: &mut egui::InputState) -> bool {
    if !composer_send_enter(input) { return false; }
    input.events.retain(|event| !matches!(event,
        egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. }
        if *modifiers == egui::Modifiers::NONE));
    true
}

pub(super) fn composer_ime_active(ctx: &egui::Context) -> bool {
    let key = egui::Id::new("composer-ime-active");
    let mut active = ctx.data(|data| data.get_temp::<bool>(key)).unwrap_or(false);
    let mut blocked = active;
    ctx.input(|input| {
        if !input.focused { active = false; }
        // Raw events remain visible after another TextEdit has consumed its
        // IME events. A commit/disable frame still owns Enter for that pass.
        for event in &input.raw.events {
            if let egui::Event::Ime(event) = event {
                blocked = true;
                active = matches!(event, egui::ImeEvent::Enabled | egui::ImeEvent::Preedit(_));
            }
        }
    });
    ctx.data_mut(|data| data.insert_temp(key, active));
    blocked
}

fn selection_scroll_velocity(viewport: egui::Rect, pointer: egui::Pos2, selecting: bool) -> f32 {
    if !selecting || pointer.x < viewport.left() || pointer.x > viewport.right() { return 0.; }
    let edge = 48_f32.min(viewport.height() / 3.).max(1.);
    let top = ((viewport.top() + edge - pointer.y) / edge).clamp(0., 1.);
    let bottom = ((pointer.y - (viewport.bottom() - edge)) / edge).clamp(0., 1.);
    (bottom * bottom - top * top) * 650.
}

#[derive(Clone)]
struct MessageActionRegion { id: egui::Id, rect: egui::Rect, row_origin: egui::Pos2, more: egui::Rect }

#[derive(Clone)]
struct HistoryPageRequest { id: String, channel: String, owner: String, generation: u64 }
impl HistoryPageRequest {
    fn new(state: &AppSnapshot) -> Self {
        Self { id: uuid::Uuid::new_v4().to_string(), channel: state.selected_channel_id.clone(),
            owner: state.current_user.as_ref().map_or(String::new(), |person| person.id.clone()),
            generation: state.notification_generation }
    }
    fn matches(&self, state: &AppSnapshot) -> bool {
        state.signed_in && state.view == "chat" && self.channel == state.selected_channel_id
            && self.owner == state.current_user.as_ref().map_or("", |person| person.id.as_str())
            && self.generation == state.notification_generation
    }
}
fn history_page_request_key() -> egui::Id { egui::Id::new("history-page-request") }
pub(super) fn clear_history_page_request(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<HistoryPageRequest>(history_page_request_key()));
}
fn history_page_pending(ctx: &egui::Context, state: &AppSnapshot) -> bool {
    let request = ctx.data(|data| data.get_temp::<HistoryPageRequest>(history_page_request_key()));
    if request.as_ref().is_some_and(|request| request.matches(state)) { true }
    else { clear_history_page_request(ctx); false }
}
pub(super) fn finish_history_page_request(ctx: &egui::Context, value: &Value) {
    let matches = ctx.data(|data| data.get_temp::<HistoryPageRequest>(history_page_request_key()))
        .is_some_and(|request| value["requestId"].as_str() == Some(request.id.as_str())
            && value["channelId"].as_str() == Some(request.channel.as_str()));
    if matches { clear_history_page_request(ctx); ctx.request_repaint(); }
}

#[derive(Clone, Copy, PartialEq)]
struct HistoryCursor { id: egui::Id, character: usize }
#[derive(Clone, Default)]
struct HistorySelection {
    scope: Option<egui::Id>,
    anchor: Option<HistoryCursor>,
    focus: Option<HistoryCursor>,
    dragging: bool,
    drag_origin: Option<egui::Pos2>,
    edges_armed: bool,
    last_wheel: Option<f64>,
}
#[derive(Clone)]
struct HistoryLabel {
    id: egui::Id,
    layer: egui::LayerId,
    position: egui::Pos2,
    galley: std::sync::Arc<egui::Galley>,
    clip: egui::Rect,
    shape: egui::layers::ShapeIdx,
    fallback: Color32,
    double_clicked: bool,
    triple_clicked: bool,
    link: bool,
    owns_primary: bool,
    atom: Option<(egui::Rect, String)>,
}
#[derive(Clone, Default)]
struct MessageTextGesture { double_clicked: bool, delete_clicked: bool, secondary_clicked: bool }
#[derive(Clone, Default)]
struct HistoryFrame {
    active: bool,
    labels: Vec<HistoryLabel>,
    rows: Vec<egui::Id>,
    shortcut_consumed: bool,
}
fn history_selection_begin(ctx: &egui::Context, scope: egui::Id) {
    ctx.data_mut(|data| {
        let selection = data.get_temp_mut_or_default::<HistorySelection>(egui::Id::new("history-selection"));
        if selection.scope != Some(scope) { *selection = HistorySelection { scope: Some(scope), ..Default::default() }; }
        data.insert_temp(egui::Id::new("history-frame"), HistoryFrame { active: true, ..Default::default() });
    });
}
fn history_register_row(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|data| {
        let frame = data.get_temp_mut_or_default::<HistoryFrame>(egui::Id::new("history-frame"));
        if frame.active { frame.rows.push(id); }
    });
}
fn history_selection_finish(ctx: &egui::Context, viewport: egui::Rect, scope: egui::Id, input_blocked: bool) -> bool {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("history-viewport"), viewport));
    let frame = ctx.data_mut(|data| data.remove_temp::<HistoryFrame>(egui::Id::new("history-frame"))).unwrap_or_default();
    let mut selection = ctx.data(|data| data.get_temp::<HistorySelection>(egui::Id::new("history-selection"))).unwrap_or_default();
    let (pointer, pressed, down, released, escape, copying, focused) = ctx.input(|input| (
        input.pointer.interact_pos(), input.pointer.primary_pressed(), input.pointer.primary_down(),
        input.pointer.primary_released(), input.key_pressed(egui::Key::Escape),
        input.events.iter().any(|event| matches!(event, egui::Event::Copy | egui::Event::Cut)), input.focused));
    let (now, wheel) = ctx.input(|input| (input.time, input.events.iter().any(|event| matches!(event, egui::Event::MouseWheel { .. }))));
    if wheel { selection.last_wheel = Some(now); }
    if selection.scope != Some(scope) || escape {
        selection = HistorySelection { scope: Some(scope), ..Default::default() };
    }
    let blocked = input_blocked || !focused || interaction::cancelling(ctx) || frame.shortcut_consumed;
    if blocked { selection.dragging = false; selection.edges_armed = false; }
    if frame.shortcut_consumed { selection.anchor = None; selection.focus = None; }
    let label_rect = |label: &HistoryLabel| label.atom.as_ref().map_or_else(
        ||egui::Rect::from_min_size(label.position, label.galley.size()),|(rect,_)|*rect);
    let cursor_at = |label: &HistoryLabel, pointer: egui::Pos2| HistoryCursor {
        id: label.id, character: label.atom.as_ref().map_or_else(
            ||label.galley.cursor_from_pos(pointer - label.position).index,
            |(rect,_)|usize::from(pointer.x>=rect.center().x)),
    };
    if pressed && !blocked {
        let target = pointer.and_then(|pointer| frame.labels.iter().find(|label|
            label.owns_primary && label_rect(label).intersect(viewport).contains(pointer)
                && interaction::owns_surface(ctx, pointer, label.layer)).map(|label| (label, pointer)));
        if let Some((label, pointer)) = target {
            let cursor = cursor_at(label, pointer);
            if let Some(focused) = ctx.memory(|memory| memory.focused()) {
                ctx.memory_mut(|memory| memory.surrender_focus(focused));
            }
            if !ctx.input(|input| input.modifiers.shift) || selection.anchor.is_none() { selection.anchor = Some(cursor); }
            selection.focus = Some(cursor); selection.dragging = true;
            selection.drag_origin = Some(pointer); selection.edges_armed = false;
            ctx.plugin::<egui::text_selection::LabelSelectionState>().lock().clear_selection();
        } else if pointer.is_some_and(|pointer| frame.labels.first().is_some_and(|label|
            interaction::owns_surface(ctx, pointer, label.layer))) {
            // Foreground menus and panels keep their own selection/focus.
            selection.anchor = None; selection.focus = None; selection.dragging = false;
        }
    }
    if selection.dragging && (down || released) {
        if let Some(pointer) = pointer {
            if selection.drag_origin.is_some_and(|origin| (pointer.y - origin.y).abs() > 8.) { selection.edges_armed = true; }
            // Only a visible label can become the moving endpoint. In contrast
            // to clipping an offscreen galley, this never creates an inverted
            // rectangle which selects every retained message in one pass.
            let clamped = viewport.clamp(pointer);
            if let Some(label) = frame.labels.iter().filter(|label| label_rect(label).intersects(viewport))
                .min_by(|a, b| label_rect(a).distance_sq_to_pos(clamped).total_cmp(&label_rect(b).distance_sq_to_pos(clamped))) {
                selection.focus = Some(cursor_at(label, clamped));
            }
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
    }
    if released && !blocked {
        if let Some((label, pointer)) = pointer.and_then(|pointer| frame.labels.iter().find(|label|
            (label.double_clicked || label.triple_clicked) && label_rect(label).intersect(viewport).contains(pointer)).map(|label| (label, pointer))) {
            let cursor = cursor_at(label, pointer);
            let chars: Vec<char> = label.galley.job.text.chars().collect();
            let mut start = cursor.character.min(chars.len());
            let mut end = start;
            if label.triple_clicked { start = 0; end = chars.len(); }
            else {
                let word = |character| egui::text_selection::text_cursor_state::is_word_char(character);
                if start > 0 && (start == chars.len() || word(chars[start - 1])) { start -= 1; }
                let kind = chars.get(start).copied().map(word);
                while start > 0 && chars.get(start - 1).copied().map(word) == kind { start -= 1; }
                while end < chars.len() && chars.get(end).copied().map(word) == kind { end += 1; }
            }
            selection.anchor = Some(HistoryCursor { id: label.id, character: start });
            selection.focus = Some(HistoryCursor { id: label.id, character: end });
        }
    }
    if released || !down { selection.dragging = false; }
    if !blocked && !ctx.wants_keyboard_input() && !egui::Popup::is_any_open(ctx) && !reaction_panel_open(ctx) {
        if let Some(focus) = selection.focus {
            if let Some(label) = frame.labels.iter().find(|label| label.id == focus.id) {
                let anchor = selection.anchor.filter(|anchor| anchor.id == focus.id).unwrap_or(focus);
                let mut cursor_range = egui::text::CCursorRange::two(egui::text::CCursor::new(anchor.character), egui::text::CCursor::new(focus.character));
                // `two` sorts its arguments; keep which cursor is moving.
                cursor_range.primary = egui::text::CCursor::new(focus.character);
                cursor_range.secondary = egui::text::CCursor::new(anchor.character);
                let events = ctx.input(|input| input.events.clone());
                for event in &events {
                    if cursor_range.on_event(ctx.os(), event, &label.galley, label.id) {
                        selection.focus = Some(HistoryCursor { id: label.id, character: cursor_range.primary.index });
                        let keep_anchor = matches!(event, egui::Event::Key { modifiers, .. } if modifiers.shift);
                        if !keep_anchor || selection.anchor.is_none() {
                            selection.anchor = Some(HistoryCursor { id: label.id, character: cursor_range.secondary.index });
                        }
                    }
                }
            }
        }
    }
    let endpoint = |cursor: HistoryCursor| frame.labels.iter().position(|label| label.id == cursor.id).map(|index| (index, cursor.character));
    let range = selection.anchor.and_then(endpoint).zip(selection.focus.and_then(endpoint));
    if selection.anchor.is_some() && range.is_none() { selection.anchor = None; selection.focus = None; selection.dragging = false; }
    let mut copied = String::new();
    let mut previous_rect: Option<egui::Rect> = None;
    if let Some((a, b)) = range {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        for (index, label) in frame.labels.iter().enumerate().take(end.0 + 1).skip(start.0) {
            let text = &label.galley.job.text;
            let from = if index == start.0 { start.1 } else { 0 };
            let to = if index == end.0 { end.1 } else { text.chars().count() };
            if copying && from < to {
                let part: String = label.atom.as_ref().map_or_else(
                    ||text.chars().skip(from).take(to - from).collect(),|(_,text)|text.clone());
                if previous_rect.is_some_and(|previous| previous.bottom() <= label.position.y) { copied.push('\n'); }
                copied.push_str(&part);
                previous_rect = Some(label_rect(label));
            }
            if label_rect(label).intersects(viewport) && from < to {
                if let Some((rect,_))=&label.atom {
                    let fill=ctx.style().visuals.selection.bg_fill;
                    ctx.graphics_mut(|layers| {
                        if let Some(list)=layers.get_mut(label.layer) {
                            list.set(label.shape,label.clip,egui::Shape::rect_filled(*rect,2.,fill));
                        }
                    });
                    continue;
                }
                let mut galley = label.galley.clone();
                egui::text_selection::visuals::paint_text_selection(&mut galley, &ctx.style().visuals,
                    &egui::text::CCursorRange::two(egui::text::CCursor::new(from), egui::text::CCursor::new(to)), None);
                ctx.graphics_mut(|layers| {
                    if let Some(list) = layers.get_mut(label.layer) {
                        list.set(label.shape, label.clip, egui::Shape::galley(label.position, galley, label.fallback));
                    }
                });
            }
        }
    }
    if copying && !blocked && !ctx.wants_keyboard_input() && !egui::Popup::is_any_open(ctx)
        && !reaction_panel_open(ctx) && !copied.is_empty() { ctx.copy_text(copied); }
    // Starting a horizontal selection in the first/last visible line is not
    // an instruction to scroll. A vertical drag arms edges; explicit wheel
    // input takes priority while its smooth scroll settles.
    let dragging = selection.dragging && selection.edges_armed
        && selection.last_wheel.is_none_or(|last| now - last >= 0.30);
    ctx.data_mut(|data| {
        // Keep only two stable endpoints between frames, never the layout
        // galleys, textures or a full copy of the message page.
        data.insert_temp(egui::Id::new("history-selection"), selection);
        data.insert_temp(egui::Id::new("history-middle-allowed"), frame.rows.into_iter().chain(frame.labels.iter().filter(|label| !label.link).map(|label| label.id)).collect::<Vec<_>>());
    });
    dragging
}

#[derive(Clone, Default)]
struct HistoryMiddleScroll { scope: Option<egui::Id>, origin: Option<egui::Pos2>, last_frame: u64 }
fn middle_scroll_velocity(origin: egui::Pos2, pointer: egui::Pos2) -> f32 {
    let distance = pointer.y - origin.y;
    distance.signum() * ((distance.abs() - 12.).max(0.) / 130.).powi(2).min(1.) * 1100.
}
fn history_middle_scroll(ui: &mut egui::Ui, viewport: egui::Rect, scope: egui::Id, blocked: bool) -> f32 {
    let ctx = ui.ctx();
    let current_frame = ctx.cumulative_frame_nr();
    let mut state = ctx.data(|data| data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"))).unwrap_or_default();
    let (pointer, middle, cancel) = ctx.input(|input| (input.pointer.interact_pos(), input.pointer.button_clicked(egui::PointerButton::Middle),
        input.key_pressed(egui::Key::Escape) || input.pointer.primary_pressed() || input.pointer.secondary_pressed()
            || input.raw_scroll_delta != egui::Vec2::ZERO || !input.focused));
    if blocked || state.scope != Some(scope) || cancel || current_frame > state.last_frame.saturating_add(1) {
        state = HistoryMiddleScroll { scope: Some(scope), origin: None, last_frame: current_frame };
    }
    state.last_frame = current_frame;
    if middle && state.origin.is_some() { state.origin = None; }
    else if middle && !blocked && !cancel && !interaction::cancelling(ctx)
        && !interaction::selecting(ctx) && pointer.is_some_and(|pointer|
            viewport.contains(pointer) && interaction::owns_surface(ctx, pointer, ui.layer_id())) {
        let clicked = ctx.interaction_snapshot(|snapshot| snapshot.clicked);
        let allowed = ctx.data(|data| data.get_temp::<Vec<egui::Id>>(egui::Id::new("history-middle-allowed"))).unwrap_or_default();
        // Links, avatars, media, buttons and sliders own their input. Only
        // blank history or its selectable plain text starts this mode.
        if clicked.is_none_or(|id| allowed.contains(&id)) { state.origin = pointer; }
    }
    let velocity = state.origin.zip(pointer).map_or(0., |(origin, pointer)| {
        let painter = ui.painter().with_clip_rect(viewport);
        painter.circle_stroke(origin, 9., egui::Stroke::new(1_f32, ui.visuals().text_color()));
        painter.line_segment([origin - egui::vec2(0., 5.), origin + egui::vec2(0., 5.)], egui::Stroke::new(1_f32, ui.visuals().text_color()));
        ctx.set_cursor_icon(egui::CursorIcon::AllScroll);
        middle_scroll_velocity(origin, pointer)
    });
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("history-middle-scroll"), state));
    velocity
}

fn selectable_message_label(ui: &mut egui::Ui, text: RichText) -> egui::Response {
    // Label::ui skips the selection plugin when clipped, which makes egui forget
    // an endpoint as soon as a wheel/auto-scroll takes it outside the viewport.
    // Register retained endpoints but keep the original painter clip unchanged.
    let (position, galley, response) = egui::Label::new(text).selectable(true).layout_in_ui(ui);
    if !register_history_label(ui, &response, position, galley.clone()) {
        egui::text_selection::LabelSelectionState::label_text_selection(ui, &response, position, galley,
            ui.visuals().text_color(), egui::Stroke::NONE);
    }
    record_message_text_gesture(ui.ctx(),&response);
    response
}
fn record_message_text_gesture(ctx:&egui::Context,response:&egui::Response) {
    let delete_down = ctx.input(|input| input.key_down(egui::Key::Delete));
    let double_clicked = response.double_clicked();
    let delete_clicked = response.clicked() && delete_down;
    let secondary_clicked = response.secondary_clicked();
    ctx.data_mut(|data| {
        let key = egui::Id::new("message-text-shortcuts");
        let mut actions = data.get_temp::<MessageTextGesture>(key).unwrap_or_default();
        actions.double_clicked |= double_clicked;
        actions.delete_clicked |= delete_clicked;
        actions.secondary_clicked |= secondary_clicked;
        data.insert_temp(key, actions);
    });
}
/// Shared mention labels participate in history selection without routing their
/// click/double-click through the message edit shortcuts.
pub(super) fn register_history_label(ui: &egui::Ui, response: &egui::Response, position: egui::Pos2, galley: std::sync::Arc<egui::Galley>) -> bool {
    let active = ui.ctx().data_mut(|data| data.get_temp_mut_or_default::<HistoryFrame>(egui::Id::new("history-frame")).active);
    if active {
        let shape = ui.painter().add(egui::Shape::galley(position, galley.clone(), ui.visuals().text_color()));
        let label = HistoryLabel { id: response.id, layer: response.layer_id, position, galley, clip: ui.clip_rect(), shape, fallback: ui.visuals().text_color(), double_clicked: response.double_clicked(), triple_clicked: response.triple_clicked(), link: false,
            atom: None, owns_primary: response.enabled() && (response.is_pointer_button_down_on()
                || response.drag_started_by(egui::PointerButton::Primary) || response.clicked_by(egui::PointerButton::Primary)) };
        ui.ctx().data_mut(|data| data.get_temp_mut_or_default::<HistoryFrame>(egui::Id::new("history-frame")).labels.push(label));
    }
    active
}
fn selectable_link_label(ui: &mut egui::Ui, text: RichText) -> egui::Response {
    // A link is still text: register the same galley in the history selection
    // while reserving the hand cursor and click action for an actual click.
    let (position, galley, response) = egui::Label::new(text).wrap().selectable(true)
        .sense(egui::Sense::click_and_drag()).layout_in_ui(ui);
    if register_history_label(ui, &response, position, galley.clone()) {
        ui.ctx().data_mut(|data| {
            if let Some(label) = data.get_temp_mut_or_default::<HistoryFrame>(egui::Id::new("history-frame"))
                .labels.last_mut() { label.link = true; }
        });
    } else {
        egui::text_selection::LabelSelectionState::label_text_selection(ui, &response, position, galley,
            ui.visuals().text_color(), egui::Stroke::NONE);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn history_link_activated(response: &egui::Response) -> bool {
    response.clicked() && !interaction::cancelling(&response.ctx) && !response.ctx.data(|data|
        data.get_temp::<HistorySelection>(egui::Id::new("history-selection")))
        .is_some_and(|selection| selection.dragging && selection.anchor != selection.focus)
}
fn is_system_record(kind: i32) -> bool {
    // Replies and application command messages remain ordinary messages.
    // Public Message Types; 19/20/21/23 carry user/interaction content.
    // https://docs.discord.com/developers/resources/message#message-types
    matches!(kind, 1..=12 | 14..=18 | 22 | 24..=29 | 31 | 32 | 36..=39 | 44 | 46)
}
fn message_display_name<'a>(message: &'a ChatMessage, state: &'a AppSnapshot) -> &'a str {
    if !state.selected_guild_id.is_empty() && message.webhook_id.is_empty() && !message.forwarded {
        state.guild_display_name(&message.author)
    } else { &message.author.name }
}
fn reply_display_name<'a>(reply: &MessageReplyPreview, person: &'a Person, state: &'a AppSnapshot) -> &'a str {
    if let Some(original)=state.messages.iter().find(|message|message.id==reply.message_id&&message.channel_id==reply.channel_id){
        if !original.webhook_id.is_empty()||original.forwarded{return &person.name;}
    }
    if state.selected_guild_id.is_empty(){&person.name}else{state.guild_display_name(person)}
}
fn system_record_description(message: &ChatMessage) -> String {
    match message.message_type {
        1 => {
            let names = message
                .mentions
                .iter()
                .take(20)
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>();
            if names.is_empty() {
                "añadió participantes al grupo.".into()
            } else {
                format!("añadió a {} al grupo.", names.join(", "))
            }
        }
        2 => "salió del grupo.".into(),
        4 if !message.content.is_empty() && message.content != "Cambió el nombre del grupo" => {
            format!(
                "cambió el nombre del grupo a «{}».",
                short(&message.content, 160)
            )
        }
        4 => "cambió el nombre del grupo.".into(),
        5 => "cambió la imagen del grupo.".into(),
        7 => "se unió al servidor.".into(),
        8 => "mejoró el servidor.".into(),
        9..=11 => format!(
            "mejoró el servidor y alcanzó el nivel {}.",
            message.message_type - 8
        ),
        12 => "añadió actualizaciones a este canal.".into(),
        14 => "— el servidor dejó de estar disponible en Descubrimiento.".into(),
        15 => "— el servidor vuelve a estar disponible en Descubrimiento.".into(),
        16 => "— aviso sobre la disponibilidad del servidor en Descubrimiento.".into(),
        17 => "— aviso final sobre la disponibilidad del servidor en Descubrimiento.".into(),
        18 if !message.content.is_empty() => {
            format!("creó el hilo «{}».", short(&message.content, 160))
        }
        18 => "creó un hilo.".into(),
        22 => "— recordatorio de invitaciones del servidor.".into(),
        24 => "— se registró una acción de moderación automática.".into(),
        25 => "— se registró una suscripción a un rol.".into(),
        26 => "— aviso de una función Premium.".into(),
        27 => "— comenzó una sesión de escenario.".into(),
        28 => "— terminó una sesión de escenario.".into(),
        29 => "— cambió un participante con voz en el escenario.".into(),
        31 => "— cambió el tema del escenario.".into(),
        32 => "— se registró una suscripción Premium de aplicación.".into(),
        36 => "— se activó el modo de alerta de incidentes.".into(),
        37 => "— se desactivó el modo de alerta de incidentes.".into(),
        38 => "— se notificó una posible incursión en el servidor.".into(),
        39 => "— se notificó una falsa alarma de incidente.".into(),
        44 => "— se recibió una notificación de compra.".into(),
        46 => "— se publicó el resultado de una encuesta.".into(),
        _ => short(&message.content, 300),
    }
}
fn paint_member_event(painter: &egui::Painter, rect: egui::Rect, color: Color32, leaving: bool) {
    let point = |x: f32, y: f32| rect.min + egui::vec2(x, y) * (rect.width() / 24.);
    let stroke = egui::Stroke::new(1.7_f32, color);
    painter.circle_stroke(point(8., 7.), rect.width() * 0.12, stroke);
    painter.line_segment([point(3., 19.), point(3., 16.)], stroke);
    painter.line_segment([point(3., 16.), point(13., 16.)], stroke);
    painter.line_segment([point(13., 16.), point(13., 19.)], stroke);
    let (start, end) = if leaving { (15., 23.) } else { (23., 15.) };
    let side = if leaving { -3. } else { 3. };
    painter.line_segment([point(start, 10.), point(end, 10.)], stroke);
    painter.line_segment([point(end + side, 7.), point(end, 10.)], stroke);
    painter.line_segment([point(end + side, 13.), point(end, 10.)], stroke);
}
fn message_rich_pattern() -> &'static regex::Regex {
    static RICH: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"https?://[^\s<>]+|<@!?([0-9]+)>|<(a?):([^:\s<>]+):([0-9]+)>|:([A-Za-z0-9_]{2,64}):|(@(?:everyone|here))\b",
        )
        .unwrap()
    });
    &RICH
}
fn readable_mentions(text: &str, explicit: &[Person], state: &AppSnapshot) -> String {
    super::mentions::readable(text, explicit, state)
}
fn mention_label(ui: &mut egui::Ui, name: &str, size: f32) -> egui::Response {
    super::mentions::label(ui, name, size)
}
fn known_named_emoji(
    name: &str,
    message: &ChatMessage,
    state: &AppSnapshot,
) -> Option<StatusEmoji> {
    // An explicit Discord token in this message is stronger evidence than a
    // name-only alias. Never invent IDs, make HTTP requests, or pick arbitrarily
    // between two different emojis sharing a name.
    let mut explicit: Option<StatusEmoji> = None;
    for capture in message_rich_pattern().captures_iter(&message.content) {
        if capture
            .get(4)
            .is_none_or(|value| !notdiscord_core::parsing::valid_id(value.as_str()))
            || capture.get(3).is_none_or(|value| value.as_str() != name)
        {
            continue;
        }
        let emoji = StatusEmoji {
            id: capture[4].into(),
            name: name.into(),
            animated: capture.get(2).is_some_and(|value| value.as_str() == "a"),
        };
        if explicit
            .as_ref()
            .is_some_and(|previous| previous.id != emoji.id)
        {
            return None;
        }
        explicit = Some(emoji);
    }
    if explicit.is_some() {
        return explicit;
    }
    let mut known: Option<&CustomEmoji> = None;
    for emoji in state.emojis.iter().filter(|emoji| {
        emoji.name == name
            && notdiscord_core::parsing::valid_id(&emoji.id)
            && (state.selected_guild_id.is_empty() || emoji.guild_id == state.selected_guild_id)
    }) {
        if known.is_some_and(|previous| previous.id != emoji.id) {
            return None;
        }
        if notdiscord_core::parsing::valid_id(&emoji.id) {
            known = Some(emoji);
        }
    }
    known.map(|emoji| StatusEmoji {
        id: emoji.id.clone(),
        name: emoji.name.clone(),
        animated: emoji.animated,
    })
}
fn can_group(previous: &ChatMessage, message: &ChatMessage) -> bool {
    previous.author.id == message.author.id
        && previous.channel_id == message.channel_id
        && local_date(&previous.timestamp) == local_date(&message.timestamp)
        && message.timestamp >= previous.timestamp
        && (message.timestamp - previous.timestamp) < chrono::Duration::minutes(5)
        && message.reply_to.is_none()
        && !message.edited
        && message.previous_contents.is_empty()
        && !message.deleted
        && !previous.deleted
        && message.message_type == 0
        && previous.message_type == 0
}
fn pin_action_button(
    ui: &mut egui::Ui,
    enabled: bool,
    color: Color32,
    pinned: bool,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::new("").small().min_size(egui::vec2(23., 24.)),
    );
    super::icons::paint_pin(
        ui.painter(),
        egui::Rect::from_center_size(response.rect.center(), egui::vec2(19., 19.)),
        if enabled {
            color
        } else {
            color.gamma_multiply(0.5)
        },
    );
    if pinned {
        ui.painter().line_segment(
            [
                response.rect.left_bottom() + egui::vec2(5., -5.),
                response.rect.right_top() + egui::vec2(-5., 5.),
            ],
            egui::Stroke::new(2.0_f32, Color32::from_rgb(242, 63, 67)),
        );
    }
    response
}
fn call_record_description(message: &ChatMessage, me: &str) -> (String, String, bool) {
    let outgoing = !me.is_empty() && message.author.id == me;
    let direction = if outgoing {
        "Llamada saliente"
    } else {
        "Llamada entrante"
    };
    let rejected = matches!(message.call_outcome.as_str(), "declined" | "rejected");
    let missed = message.call_outcome == "missed"
        || (!outgoing
            && !me.is_empty()
            && message.call_ended_at.is_some()
            && (message.call_participants_known || !message.call_participant_ids.is_empty())
            && !message.call_participant_ids.iter().any(|id| id == me));
    if rejected {
        return (
            "· Llamada rechazada".into(),
            format!("{direction} · Rechazada"),
            true,
        );
    }
    if missed {
        return (
            "· Llamada perdida".into(),
            format!("{direction} · Sin respuesta"),
            true,
        );
    }
    let Some(end) = message.call_ended_at else {
        return (
            "inició una llamada.".into(),
            format!("{direction} · En curso"),
            false,
        );
    };
    let seconds = end
        .signed_duration_since(message.call_started_at.unwrap_or(message.timestamp))
        .num_seconds();
    if seconds < 0 {
        return (
            "· Llamada finalizada".into(),
            format!("{direction} · Duración no disponible"),
            false,
        );
    }
    let duration = if seconds < 60 {
        "unos segundos".into()
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        format!(
            "{minutes} {}",
            if minutes == 1 { "minuto" } else { "minutos" }
        )
    } else {
        format!("{} h {} min", seconds / 3600, seconds / 60 % 60)
    };
    (
        format!("inició una llamada que duró {duration}."),
        format!(
            "{direction} · Finalizada · {}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        ),
        false,
    )
}
fn reply_preview(
    ui: &mut egui::Ui,
    text: &str,
    chat_size: f32,
    foreground: Color32,
    hover: Color32,
    available: bool,
) -> egui::Response {
    let padding = egui::vec2(4., 3.) * (chat_size / 17.);
    let mut layout = egui::text::LayoutJob::simple(
        text.into(),
        egui::FontId::proportional(reply_preview_font_size(chat_size)),
        foreground,
        (ui.available_width() - padding.x * 2.).max(1.),
    );
    layout.wrap.max_rows = 1;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(layout));
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(
            ui.available_width().max(1.),
            galley.size().y + padding.y * 2.,
        ),
        if available {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    if available && response.hovered() {
        ui.painter().rect_filled(rect, 4., hover);
    }
    ui.painter().galley(rect.min + padding, galley, foreground);
    if available {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Ir al mensaje original")
    } else {
        response.on_hover_text("El mensaje original no está disponible")
    }
}
fn reply_preview_font_size(chat_size: f32) -> f32 {
    (chat_size * (13. / 17.)).max(11.)
}
fn reply_preview_height(ui: &egui::Ui, chat_size: f32) -> f32 {
    ui.fonts_mut(|fonts| {
        fonts.row_height(&egui::FontId::proportional(reply_preview_font_size(
            chat_size,
        )))
    }) + (6. * chat_size / 17.)
}
fn media_slot_size(max: egui::Vec2, dimensions: Option<(u32, u32)>) -> egui::Vec2 {
    let source = dimensions
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map(|(w, h)| egui::vec2(w as f32, h as f32))
        .unwrap_or(egui::vec2(320., 180.));
    let factor = (max.x.max(1.) / source.x)
        .min(max.y.max(1.) / source.y)
        .min(1.);
    source * factor
}
const PREPARED_CARD_WIDTH: f32 = 160.;
const PREPARED_PREVIEW_HEIGHT: f32 = 80.;
fn prepared_remove_height(ui: &egui::Ui) -> f32 {
    ui.fonts_mut(|fonts| fonts.row_height(&egui::FontId::proportional(12.)))
        .ceil()
        + 6.
}
fn prepared_attachments_height(ui: &egui::Ui, count: usize) -> f32 {
    let text = ui.fonts_mut(|fonts| {
        fonts.row_height(&egui::FontId::proportional(13.)).ceil()
            + fonts.row_height(&egui::FontId::proportional(12.)).ceil()
    });
    let gap = ui.spacing().item_spacing;
    let cards =
        count as f32 * (PREPARED_CARD_WIDTH + 18.) + (count.saturating_sub(1)) as f32 * gap.x;
    let scrollbar = if cards > ui.available_width() - 16. {
        ui.spacing().scroll.bar_width
            + ui.spacing().scroll.bar_inner_margin
            + ui.spacing().scroll.bar_outer_margin
    } else {
        0.
    };
    PREPARED_PREVIEW_HEIGHT + text + prepared_remove_height(ui) + 18. + 3. * gap.y + scrollbar
}
fn prepared_file_size(ctx: &egui::Context, path: &std::path::Path) -> String {
    let bytes = ctx.data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<HashMap<PathBuf, Option<u64>>>(egui::Id::new(
            "prepared-file-sizes",
        ));
        if !cache.contains_key(path) {
            if cache.len() >= 128 {
                cache.clear();
            }
            cache.insert(
                path.to_path_buf(),
                std::fs::metadata(path)
                    .ok()
                    .filter(|metadata| metadata.is_file())
                    .map(|metadata| metadata.len()),
            );
        }
        cache.get(path).copied().flatten()
    });
    match bytes {
        Some(bytes) if bytes >= 1024 * 1024 => format!("{:.1} MiB", bytes as f64 / (1024. * 1024.)),
        Some(bytes) => format!("{:.0} KiB", bytes as f64 / 1024.),
        None => "Tamaño no disponible".into(),
    }
}
fn composer_text_height(ui: &egui::Ui, text: &str, font_size: f32) -> f32 {
    let width = (ui.available_width() - 156. - ui.spacing().item_spacing.x * 4.).max(30.);
    ui.fonts_mut(|fonts| {
        fonts.layout(
            text.into(),
            egui::FontId::proportional(font_size),
            Color32::WHITE,
            width,
        )
    })
    .size()
    .y
    .clamp(font_size + 2., 130.)
        + 10.
}
fn composer_height(ui: &egui::Ui, draft: Option<&Draft>, font_size: f32) -> f32 {
    composer_text_height(ui, draft.map(|d| d.text.as_str()).unwrap_or(""), font_size)
        .max(composer_control_height(ui))
        + 16.
        + if draft.is_some_and(|d| d.reply.is_some()) {
            reply_preview_height(ui, font_size).max(22. * font_size / 17.)
                + ui.spacing().item_spacing.y
        } else {
            0.
        }
        + if draft.is_some_and(|d| !d.paths.is_empty()) {
            prepared_attachments_height(ui, draft.map_or(0, |draft| draft.paths.len()))
                + 3.
                + ui.spacing().item_spacing.y
        } else {
            0.
        }
        + if draft.is_some_and(|draft| !composer_custom_preview_tokens(&draft.text).is_empty()) {
            composer_emoji_preview_height(ui, font_size) + ui.spacing().item_spacing.y
        } else { 0. }
}
fn composer_custom_preview_tokens(text: &str) -> Vec<&str> {
    message_rich_pattern().captures_iter(text)
        .filter(|capture| capture.get(4).is_some() || capture.get(5).is_some())
        .filter_map(|capture| capture.get(0).map(|whole| whole.as_str()))
        .take(8).collect()
}
fn composer_emoji_preview_side(font_size: f32) -> f32 { (font_size * 1.4).clamp(20., 28.) }
fn composer_emoji_preview_height(ui: &egui::Ui, font_size: f32) -> f32 {
    let scroll = ui.spacing().scroll;
    composer_emoji_preview_side(font_size) + scroll.bar_width + scroll.bar_inner_margin + scroll.bar_outer_margin
}
fn composer_control_height(ui: &egui::Ui) -> f32 {
    ui.fonts_mut(|fonts| fonts.row_height(&egui::TextStyle::Button.resolve(ui.style())))
        .max(ui.spacing().interact_size.y - ui.spacing().button_padding.y * 2.)
        + ui.spacing().button_padding.y * 2.
}
fn paste_gesture(
    focused: bool,
    previously_focused: bool,
    down: bool,
    pressed_since_poll: bool,
    paste_event: bool,
    previously_down: bool,
) -> bool {
    focused
        && (paste_event
            || down && !previously_down
            || pressed_since_poll && previously_focused && !previously_down)
}
fn previewable_path(path: &std::path::Path) -> bool {
    path.extension().and_then(|v| v.to_str()).is_some_and(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "avif"
        )
    })
}
fn add_attachment(d: &mut Draft, path: PathBuf, error: &mut String) {
    if d.paths.contains(&path) {
        return;
    }
    let Ok(meta) = std::fs::metadata(&path) else {
        *error = "No se pudo leer el archivo.".into();
        return;
    };
    let total: u64 = d
        .paths
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    if !meta.is_file()
        || d.paths.len() >= 10
        || meta.len() > 25 * 1024 * 1024
        || total + meta.len() > 100 * 1024 * 1024
    {
        *error = "Máximo diez adjuntos, 25 MiB por archivo y 100 MiB por mensaje.".into();
        return;
    }
    d.paths.push(path);
    d.revision += 1;
}
fn mention_token(text: &str, cursor: usize) -> Option<(usize, String)> {
    super::mentions::query(text, cursor)
}
fn sync_completion_query(ctx: &egui::Context, editor: egui::Id, query: &Option<(usize, String)>,
    index: &mut usize, closed: &mut bool) {
    let key = egui::Id::new("active-mention-completion");
    let current = (editor, query.clone());
    let previous = ctx.data(|data| data.get_temp::<(egui::Id, Option<(usize, String)>)>(key));
    if previous.as_ref() != Some(&current) {
        *index = 0;
        *closed = false;
    }
    ctx.data_mut(|data| data.insert_temp(key, current));
}
fn insert_mention(draft: &mut Draft, start: usize, end: usize, person: &Person) {
    super::mentions::insert(draft, start, end, person)
}
fn adjust_mentions(old: &str, new: &str, spans: &mut Vec<Mention>) {
    super::mentions::adjust(old, new, spans)
}
fn latest_window(ctx: &egui::Context, state: &AppSnapshot) -> bool {
    if !state.message_window.channel_id.is_empty() && state.message_window.channel_id == state.selected_channel_id {
        return state.message_window.mode == MessageWindowMode::Latest
            && !state.message_window.has_newer && state.message_window.contiguous;
    }
    let key = egui::Id::new("chat-latest-known");
    let channel = &state.selected_channel_id;
    let last_visible = state
        .messages
        .last()
        .map(|message| message.id.as_str())
        .unwrap_or("");
    let from_navigation = state
        .direct_messages
        .iter()
        .chain(&state.channels)
        .find(|item| item.id == *channel)
        .map(|item| item.last_message_id.as_str())
        .unwrap_or("");
    ctx.data_mut(|data| {
        let known = data.get_temp_mut_or_default::<HashMap<String, String>>(key);
        for candidate in [last_visible, from_navigation] {
            if notdiscord_core::parsing::valid_id(candidate)
                && known.get(channel).is_none_or(|previous| {
                    candidate.len() > previous.len()
                        || candidate.len() == previous.len() && candidate > previous.as_str()
                })
            {
                if known.len() >= 1024 && !known.contains_key(channel) {
                    if let Some(evict) = known.keys().next().cloned() {
                        known.remove(&evict);
                    }
                }
                known.insert(channel.clone(), candidate.into());
            }
        }
        known.get(channel).is_none_or(|latest| {
            last_visible.len() > latest.len()
                || last_visible.len() == latest.len() && last_visible >= latest.as_str()
        })
    })
}
fn outgoing_mentions(draft: &Draft) -> String {
    super::mentions::outgoing(draft)
}

fn personal_reply_accent() -> Color32 { Color32::from_rgb(250, 166, 26) }
fn personal_reply_fill() -> Color32 { Color32::from_rgba_unmultiplied(250, 166, 26, 25) }

/// A displayed name or a mention in the reply body is not evidence that this
/// user received a reply. Resolve the referenced message's actual author.
#[cfg(test)]
fn replies_to_current_user(message: &ChatMessage, state: &AppSnapshot) -> bool {
    let Some(current) = state.current_user.as_ref().filter(|person| !person.id.is_empty()) else { return false; };
    let Some(reference) = message.reply_to.as_deref().filter(|id| {
        notdiscord_core::parsing::valid_id(id) && *id != message.id
    }) else { return false; };
    if message.forwarded || message.deleted || message.channel_id.is_empty() { return false; }
    let original = state.messages.iter().find(|original| {
        original.id == reference && original.channel_id == message.channel_id && !original.author.id.is_empty()
    });
    if let Some(original) = original { return original.author.id == current.id; }
    message.reply_preview.as_ref().is_some_and(|preview| {
        preview.message_id == reference && preview.channel_id == message.channel_id
            && !preview.unavailable
            && preview.author.as_ref().is_some_and(|author| author.id == current.id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("chat/interaction_tests.rs");
    #[test]
    fn signed_gif_link_and_embedded_alias_paint_one_card_with_manual_controls() {
        let (mut app,ctx,_)=ui_harness();let (_,state)=message_fixture();
        let old="https://cdn.discordapp.com/attachments/10/20/laundry.gif?ex=1&hm=old";
        let fresh="https://media.discordapp.net/attachments/10/20/laundry.gif?ex=ffffffff&hm=new";
        let mut message=notdiscord_core::parsing::message(&serde_json::json!({"id":"300","channel_id":"200","content":old,
            "author":{"id":"100","username":"Synthetic"},
            "attachments":[{"id":"20","filename":"laundry.gif","url":old,"proxy_url":fresh,"width":64,"height":48}],
            "embeds":[{"url":old,"image":{"url":fresh,"width":64,"height":48}}]}));
        message.author.avatar_url=None;
        let texture=app.pictures.seed_sized_fixture(&ctx,fresh,384);
        let output=message_frame(&mut app,&ctx,&message,&state,vec![]);
        assert_eq!(output.shapes.iter().filter(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.texture_id==texture)).count(),1);
        assert_eq!(output.shapes.iter().filter(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="▶ GIF")).count(),1);
        painted_text(&output,"Ampliar");
        assert_eq!(app.pictures.pending_fixture_requests(),0,"all requested media must already be synthetic cached pixels");
    }
    #[test]
    fn blocked_poster_can_fall_back_to_the_known_media_without_automatic_retry_loops() {
        let (mut app,ctx,_)=ui_harness();
        let actual="https://media.discordapp.net/attachments/10/20/laundry.gif?ex=ffffffff";
        let blocked="https://untrusted.invalid/poster.gif";
        let texture=app.pictures.seed_sized_fixture(&ctx,actual,384);
        for _ in 0..3 {
            let output=ctx.run(egui::RawInput::default(),|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.media_card(ui,actual,blocked,true,"GIF",Some((64,48))));});
            assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.texture_id==texture)));
            assert_eq!(app.pictures.pending_fixture_requests(),0);
        }
    }
    #[test]
    fn enter_followed_by_printable_text_in_the_same_frame_starts_the_next_draft() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, state) = message_fixture();
        app.prefs.send_typing = false;
        chat_frame(&mut app, &ctx, &state, vec![]);
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::Text("First".into())]);
        chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Enter,true), egui::Event::Text("Next".into())]);
        let action = commands.try_recv().unwrap();
        assert_eq!(action.text,"First");
        assert_eq!(app.drafts["200"].text,"Next");
        let extra:Value=serde_json::from_str(&action.extra).unwrap();
        app.data_update("send-ack",serde_json::json!({"submissionId":extra["submissionId"]}));
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Enter,false),egui::Event::Text("!".into())]);
        assert_eq!(app.drafts["200"].text,"Next!");
    }
    #[test]
    fn pending_messages_use_capture_order_and_deleted_rows_do_not_offer_pin_mutations() {
        let (mut app,ctx,mut commands)=ui_harness();
        let (mut message,mut state)=message_fixture();
        let first=Instant::now();
        for (id,text,time) in [("z-last-id","Captured first",first),("a-first-id","Captured second",first+Duration::from_millis(1))] {
            app.submissions.insert(id.into(),("200".into(),0));
            app.sent_drafts.insert(id.into(),Draft{text:text.into(),captured_at:Some(time),..Default::default()});
        }
        let output=ctx.run(egui::RawInput::default(),|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.draw_pending_messages(ui,&state));});
        assert!(painted_text(&output,"Captured first").top()<painted_text(&output,"Captured second").top());
        message.deleted=true;state.can_pin_messages=true;
        let menu=ctx.run(egui::RawInput::default(),|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.message_menu(ui,&message,&state));});
        let pin=painted_text(&menu,"Fijar mensaje").center();
        for pressed in [true,false] {
            let _=ctx.run(egui::RawInput{events:pointer(pin,pressed,egui::PointerButton::Primary),..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|app.message_menu(ui,&message,&state));});
        }
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn composer_custom_preview_is_bounded_and_has_its_final_height_on_the_first_frame() {
        for width in [260., 700.] {
            let (mut app, ctx, _) = ui_harness();
            let (_, state) = message_fixture();
            app.prefs.send_typing = false;
            let token = "<:Synthetic:400>";
            let text = [token; 12].join(" ");
            assert_eq!(composer_custom_preview_tokens(&text).len(), 8);
            let url = StatusEmoji { id: "400".into(), name: "Synthetic".into(), ..Default::default() }.image_url().unwrap();
            let texture = app.pictures.seed_sized_fixture(&ctx, &url, 64);
            app.drafts.insert("200".into(), Draft { text: text.clone(), ..Default::default() });
            let mut measured = Vec::new();
            for _ in 0..2 {
                let output = ctx.run(egui::RawInput { screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(width,500.))), ..Default::default() }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let expected = composer_height(ui, app.drafts.get("200"), app.prefs.chat_size);
                        let top = ui.cursor().top();
                        app.draw_composer(ui, &state);
                        measured.push((ui.cursor().top() - top, expected));
                    });
                });
                assert!(output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) => mesh.texture_id == texture,
                    egui::Shape::Rect(rect) => rect.fill_texture_id() == texture,
                    _ => false,
                }), "preview must draw the injected local pixels on its first pass");
                assert!(!ctx.memory(|memory| memory.has_focus(egui::Id::new(("composer", "200")))));
                assert_eq!(app.drafts["200"].text, text);
            }
            assert!((measured[0].0-measured[1].0).abs()<1., "{measured:?}");
            for (actual, expected) in measured { assert!((actual-expected).abs()<12., "width={width}, actual={actual} expected={expected}"); }
        }
    }
    fn key_event(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE }
    }
    #[test]
    fn emoji_tab_uses_real_editor_input_for_unicode_and_custom_without_sending() {
        for custom in [false,true] {
            let (mut app,ctx,mut commands)=ui_harness();let (_,mut state)=message_fixture();app.prefs.send_typing=false;
            if custom {state.selected_guild_id="50".into();state.emojis=vec![CustomEmoji{id:"401".into(),name:"JazzCat".into(),
                guild_id:"50".into(),available:true,usable:true,..Default::default()}].into();}
            seed_completion_emojis(&mut app,&ctx,&state);
            chat_frame(&mut app,&ctx,&state,vec![]);
            let query=if custom {":jazz"} else {":joy"};
            chat_frame(&mut app,&ctx,&state,vec![egui::Event::Text(query.into())]);
            let expected=if custom {"<:JazzCat:401> "}else{"😂 "};
            chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Tab,true)]);
            assert_eq!(app.drafts["200"].text,expected);assert_eq!(app.composer_cursor,expected.chars().count());
            assert!(ctx.memory(|memory|memory.has_focus(egui::Id::new(("composer","200")))));
            assert!(commands.try_recv().is_err());
            assert_eq!(app.emojis.capacity(),0,"completion must not retain the full picker catalogue");
            chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Tab,false),egui::Event::Text("next".into())]);
            assert_eq!(app.drafts["200"].text,format!("{expected}next"));
        }
    }
    #[test]
    fn emoji_tab_in_the_middle_preserves_suffix_and_places_the_real_textedit_cursor() {
        let (mut app,ctx,mut commands)=ui_harness();let (_,state)=message_fixture();app.prefs.send_typing=false;
        app.drafts.insert("200".into(),Draft{text:"before :joy after".into(),..Default::default()});
        chat_frame(&mut app,&ctx,&state,vec![]);
        let id=egui::Id::new(("composer","200"));
        let mut editor=egui::TextEdit::load_state(&ctx,id).unwrap();
        editor.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(11))));editor.store(&ctx,id);
        ctx.memory_mut(|memory|memory.request_focus(id));app.composer_cursor=11;
        chat_frame(&mut app,&ctx,&state,vec![]);
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Tab,true)]);
        assert_eq!(app.drafts["200"].text,"before 😂  after");assert_eq!(app.composer_cursor,"before 😂 ".chars().count());
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Tab,false),egui::Event::Text("NEXT".into())]);
        assert_eq!(app.drafts["200"].text,"before 😂 NEXT after");assert!(commands.try_recv().is_err());
    }
    #[test]
    fn emoji_arrows_escape_mouse_and_batched_tab_keep_the_composer_draft() {
        let (mut app,ctx,mut commands)=ui_harness();let (_,mut state)=message_fixture();app.prefs.send_typing=false;
        state.selected_guild_id="50".into();state.emojis=[("401","JazzOne"),("402","JazzTwo")].into_iter().map(|(id,name)|CustomEmoji {
            id:id.into(),name:name.into(),guild_id:"50".into(),available:true,usable:true,..Default::default()
        }).collect();
        seed_completion_emojis(&mut app,&ctx,&state);
        chat_frame(&mut app,&ctx,&state,vec![]);
        chat_frame(&mut app,&ctx,&state,vec![egui::Event::Text(":jazz".into()),key_event(egui::Key::Tab,true)]);
        assert!(commands.try_recv().is_err(),"A brand-new query and Tab in the same batch must never send");
        assert_eq!(app.drafts.get("200").expect("Batched text must reach TextEdit before Tab can move focus").text,"<:JazzOne:401> ");
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Tab,false),egui::Event::Text(":jazz".into())]);
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::ArrowDown,true)]);
        assert_eq!(app.mention_index,1);
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::ArrowDown,false),key_event(egui::Key::Tab,true)]);
        assert_eq!(app.drafts["200"].text,"<:JazzOne:401> <:JazzTwo:402> ");
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Tab,false),egui::Event::Text(":jazz".into())]);
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Escape,true)]);
        assert!(app.mention_closed);assert!(app.drafts["200"].text.ends_with(":jazz"));
        // Changing the query reopens the same shared presenter; click its image
        // side, rather than its text, to verify that the whole row is a target.
        chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Escape,false),egui::Event::Text("T".into())]);
        for _ in 0..2 {chat_frame(&mut app,&ctx,&state,vec![]);}
        let row=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(("composer-completion-row",0)))).unwrap();
        for pressed in [true,false] {chat_frame(&mut app,&ctx,&state,pointer(row.left_center()+egui::vec2(10.,0.),pressed,egui::PointerButton::Primary));}
        assert!(app.drafts["200"].text.ends_with("<:JazzTwo:402> "));
        assert!(commands.try_recv().is_err());
    }
    fn seed_completion_emojis(app:&mut NotDiscordUi,ctx:&egui::Context,state:&AppSnapshot) {
        app.prefs.allow_gif_playback=false;
        for emoji in &state.emojis {
            let url=StatusEmoji{id:emoji.id.clone(),name:emoji.name.clone(),animated:false}.image_url().unwrap();
            for side in [28,(composer_emoji_preview_side(app.prefs.chat_size)*ctx.pixels_per_point()).ceil() as u32] {
                app.pictures.seed_sized_fixture(ctx,&url,side);
            }
        }
    }
    #[test]
    fn everyone_and_self_completion_keep_real_wire_mentions_and_permission_boundary() {
        for broadcast in [false,true] {
            let (mut app,ctx,mut commands)=ui_harness();let (_,mut state)=message_fixture();app.prefs.send_typing=false;
            state.current_user=Some(Person{id:"100".into(),name:"Self".into(),username:"self".into(),..Default::default()});
            state.selected_guild_id="50".into();state.channel_access.can_mention_everyone=true;
            chat_frame(&mut app,&ctx,&state,vec![]);
            chat_frame(&mut app,&ctx,&state,vec![egui::Event::Text(if broadcast {"@ev"}else{"@Sel"}.into())]);
            chat_frame(&mut app,&ctx,&state,vec![key_event(egui::Key::Tab,true)]);
            assert_eq!(outgoing_mentions(&app.drafts["200"]),if broadcast {"@everyone "}else{"<@100> "});
            assert!(commands.try_recv().is_err());
            if broadcast {state.channel_access.can_mention_everyone=false;assert!(super::super::mentions::choices("ev",&state,8).is_empty());}
        }
    }
    #[test]
    fn real_mentions_highlight_the_whole_message_for_self_and_others_but_never_copied_code_or_forwarded_text() {
        let (mut app,ctx,mut commands)=ui_harness();let (_,mut state)=message_fixture();
        state.current_user=Some(Person{id:"100".into(),name:"Self".into(),..Default::default()});
        for author in ["100","101"] {
            for (content,metadata,broadcast,expected) in [("<@100>",true,false,true),("<@!100>",true,false,true),
                ("<@100>",false,false,false),("`<@100>`",true,false,false),("\\<@100>",true,false,false),
                ("@everyone",false,true,true),("@everyone",false,false,false),("`@everyone`",false,true,false),
                ("\\@everyone",false,true,false),("name@everyone_else",false,true,false)] {
                let message=notdiscord_core::parsing::message(&serde_json::json!({"id":"300","channel_id":"200","author":{"id":author,"username":"Sender"},
                    "content":content,"mentions":if metadata {serde_json::json!([{"id":"100","username":"Self"}])}else{serde_json::json!([])},"mention_everyone":broadcast}));
                let frame=message_frame(&mut app,&ctx,&message,&state,vec![]);
                let highlighted=frame.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::LineSegment{stroke,..} if stroke.color==personal_reply_accent()&&stroke.width==3.));
                assert_eq!(highlighted,expected,"author={author}, content={content}");
                if content=="@everyone" {
                    let inline=frame.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="@everyone"&&text.galley.job.sections.iter().any(|section|section.format.background==super::super::mentions::format(app.prefs.chat_size).background)));
                    assert_eq!(inline,broadcast,"inline and whole-message styles use actual mention metadata");
                }
                let mut forwarded=message;forwarded.forwarded=true;
                assert!(!super::super::mentions::highlights_current_user(&forwarded,&state));
            }
        }
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn mention_tab_completes_the_first_result_without_leaving_the_editor_or_sending() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, mut state) = message_fixture();
        app.prefs.send_typing = false;
        state.search_people = vec![
            Person { id: "101".into(), name: "Alice".into(), username: "alice".into(), ..Default::default() },
            Person { id: "102".into(), name: "Amy".into(), username: "amy".into(), ..Default::default() },
        ];
        chat_frame(&mut app, &ctx, &state, vec![]);
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::Text("@".into())]);
        assert_eq!(app.mention_index, 0);
        chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Tab, true)]);
        assert_eq!(app.drafts["200"].text, "@Alice ");
        assert_eq!(outgoing_mentions(&app.drafts["200"]), "<@101> ");
        assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(("composer", "200")))));
        assert!(commands.try_recv().is_err());
        chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Tab, false), egui::Event::Text("hello".into())]);
        assert_eq!(app.drafts["200"].text, "@Alice hello");
    }
    #[test]
    fn mention_tab_respects_arrow_selection_and_new_query_starts_at_the_first_result() {
        for change_query in [false, true] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (_, mut state) = message_fixture();
            app.prefs.send_typing = false;
            state.search_people = vec![
                Person { id: "101".into(), name: "Alice".into(), username: "alice".into(), ..Default::default() },
                Person { id: "102".into(), name: "Amy".into(), username: "amy".into(), ..Default::default() },
            ];
            chat_frame(&mut app, &ctx, &state, vec![]);
            chat_frame(&mut app, &ctx, &state, vec![egui::Event::Text("@".into())]);
            chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::ArrowDown, true)]);
            assert_eq!(app.mention_index, 1);
            if change_query {
                chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::ArrowDown, false), egui::Event::Text("A".into())]);
                assert_eq!(app.mention_index, 0);
            }
            chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Tab, true)]);
            let expected = if change_query { "@Alice " } else { "@Amy " };
            assert_eq!(app.drafts["200"].text, expected);
            assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(("composer", "200")))));
            assert!(commands.try_recv().is_err());
        }
    }
    #[test]
    fn mention_query_text_and_tab_in_one_batch_completes_the_updated_query() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, mut state) = message_fixture();
        app.prefs.send_typing = false;
        state.search_people = vec![
            Person { id: "101".into(), name: "Alice".into(), username: "alice".into(), ..Default::default() },
            Person { id: "102".into(), name: "Bob".into(), username: "bob".into(), ..Default::default() },
        ];
        chat_frame(&mut app, &ctx, &state, vec![]);
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::Text("@".into())]);
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::Text("B".into()), key_event(egui::Key::Tab, true)]);
        assert_eq!(app.drafts["200"].text, "@Bob ");
        assert_eq!(outgoing_mentions(&app.drafts["200"]), "<@102> ");
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn printable_autofocus_detaches_send_and_late_ack_never_clears_the_next_draft() {
        for guild in [false, true] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (_, mut state) = message_fixture();
            if guild { state.selected_guild_id = "10".into(); }
            app.prefs.send_typing = false;
            chat_frame(&mut app, &ctx, &state, vec![]);
            chat_frame(&mut app, &ctx, &state, vec![egui::Event::Text("Primer mensaje".into())]);
            assert_eq!(app.drafts["200"].text, "Primer mensaje");
            chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Enter, true)]);
            let action = commands.try_recv().unwrap();
            assert_eq!((action.kind.as_str(), action.text.as_str()), ("send-prepared", "Primer mensaje"));
            let extra: Value = serde_json::from_str(&action.extra).unwrap();
            let id = extra["submissionId"].as_str().unwrap();
            assert!(!app.drafts.contains_key("200"));
            assert_eq!(app.sent_drafts[id].text, "Primer mensaje");
            chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Enter, false), egui::Event::Text("Siguiente".into())]);
            // A new draft can have the same revision as the captured one.
            app.drafts.get_mut("200").unwrap().revision = extra["revision"].as_u64().unwrap();
            app.data_update("send-ack", serde_json::json!({"submissionId":id,"channelId":"200","revision":extra["revision"]}));
            assert_eq!(app.drafts["200"].text, "Siguiente");
            assert!(app.sent_drafts.is_empty() && app.submissions.is_empty());
        }
    }
    #[test]
    fn failed_detached_send_retries_the_same_nonce_without_mutating_current_work() {
        let (mut app, _, mut commands) = ui_harness();
        app.submissions.insert("capture".into(), ("200".into(), 7));
        app.sent_drafts.insert("capture".into(), Draft { text: "captured".into(), revision: 7, ..Default::default() });
        app.drafts.insert("200".into(), Draft { text: "new work".into(), ..Default::default() });
        app.data_update("send-error", serde_json::json!({"submissionId":"capture","channelId":"200","revision":7}));
        assert!(app.failed_submissions.contains("capture"));
        app.send_captured_draft("capture", "200");
        let action = commands.try_recv().unwrap();
        let extra: Value = serde_json::from_str(&action.extra).unwrap();
        assert_eq!(extra["submissionId"], "capture");
        assert_eq!(action.text, "captured");
        assert_eq!(app.drafts["200"].text, "new work");
        assert!(!app.failed_submissions.contains("capture"));
    }
    #[test]
    fn autofocus_respects_other_inputs_modals_permissions_and_shortcuts() {
        for blocked in ["input", "modal", "permission", "ctrl"] {
            let (mut app, ctx, _) = ui_harness();
            let (_, mut state) = message_fixture();
            app.prefs.send_typing = false;
            if blocked == "modal" { app.dialog = Some(Dialog::Settings); }
            if blocked == "permission" { state.channel_access.can_send = false; }
            let other = egui::Id::new("another-input");
            if blocked == "input" { ctx.memory_mut(|memory| memory.request_focus(other)); }
            let mut other_text = String::new();
            let _ = ctx.run(egui::RawInput {
                events: vec![egui::Event::Text("x".into())],
                modifiers: if blocked == "ctrl" { egui::Modifiers::CTRL } else { egui::Modifiers::NONE },
                ..Default::default()
            }, |ctx| { egui::CentralPanel::default().show(ctx, |ui| {
                if blocked == "input" { ui.add(egui::TextEdit::singleline(&mut other_text).id(other)); }
                app.draw_composer(ui, &state);
            }); });
            assert!(app.drafts.get("200").is_none_or(|draft| draft.text.is_empty()), "{blocked}");
            if blocked == "input" { assert_eq!(other_text, "x"); }
        }
    }
    #[test]
    fn contextual_enter_sends_the_existing_draft_from_history_and_keeps_one_submission() {
        for select_history in [false, true] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (message, mut state) = message_fixture();
            state.messages = vec![message].into();
            app.prefs.send_typing = false;
            app.drafts.insert("200".into(), Draft { text: "Prepared from history".into(), ..Default::default() });
            let initial = chat_frame(&mut app, &ctx, &state, vec![]);
            if select_history {
                let text = painted_text(&initial, "Fixture body");
                let start = text.left_center() + egui::vec2(2., 0.);
                let end = text.right_center() - egui::vec2(2., 0.);
                chat_frame(&mut app, &ctx, &state, pointer(start, true, egui::PointerButton::Primary));
                chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(end)]);
                chat_frame(&mut app, &ctx, &state, pointer(end, false, egui::PointerButton::Primary));
                assert!(!copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy])).is_empty());
            } else {
                let blank = egui::pos2(450., 250.);
                for pressed in [true, false] {
                    chat_frame(&mut app, &ctx, &state, pointer(blank, pressed, egui::PointerButton::Primary));
                }
            }
            assert!(!ctx.wants_keyboard_input(), "history must own no editor or button focus");
            chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Enter, true)]);
            let sent = commands.try_recv().unwrap();
            assert_eq!((sent.kind.as_str(), sent.channel_id.as_str(), sent.text.as_str()),
                ("send-prepared", "200", "Prepared from history"));
            let extra: Value = serde_json::from_str(&sent.extra).unwrap();
            let id = extra["submissionId"].as_str().unwrap();
            assert_eq!(app.sent_drafts[id].text, "Prepared from history");
            assert!(!app.drafts.contains_key("200"));
            assert_eq!(app.submissions.len(), 1);
            assert!(ctx.memory(|memory| memory.has_focus(egui::Id::new(("composer", "200")))));
            app.drafts.insert("200".into(), Draft { text: "Next draft".into(), ..Default::default() });
            let mut repeat = key_event(egui::Key::Enter, true);
            if let egui::Event::Key { repeat, .. } = &mut repeat { *repeat = true; }
            chat_frame(&mut app, &ctx, &state, vec![repeat]);
            chat_frame(&mut app, &ctx, &state, vec![]);
            assert!(commands.try_recv().is_err(), "holding Enter must not send the next draft");
            assert_eq!(app.submissions.len(), 1);
            assert_eq!(app.drafts["200"].text, "Next draft");
        }
    }
    #[test]
    fn contextual_enter_uses_the_same_mentions_attachment_reply_and_rapid_typing_capture() {
        for attachment_only in [false, true] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (message, state) = message_fixture();
            app.prefs.send_typing = false;
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/UNICODE-LICENSE.txt");
            let mut draft = Draft { paths: vec![path.clone()], reply: Some(MessageReplyPreview {
                message_id: "250".into(), channel_id: "200".into(), content: "Local original".into(),
                ..Default::default()
            }), ..Default::default() };
            if !attachment_only { insert_mention(&mut draft, 0, 0, &message.author); }
            let expected = outgoing_mentions(&draft);
            app.drafts.insert("200".into(), draft);
            chat_frame(&mut app, &ctx, &state, vec![]);
            assert!(!ctx.wants_keyboard_input());
            chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Enter, true), egui::Event::Text("Next".into())]);
            let sent = commands.try_recv().unwrap();
            let extra: Value = serde_json::from_str(&sent.extra).unwrap();
            assert_eq!(sent.kind, "send-prepared");
            assert_eq!(sent.text, expected);
            assert_eq!(extra["replyId"], "250");
            assert_eq!(extra["paths"], serde_json::json!([path.to_string_lossy()]));
            let id = extra["submissionId"].as_str().unwrap();
            assert_eq!(app.sent_drafts[id].paths, vec![path]);
            assert_eq!(app.drafts["200"].text, "Next");
            assert!(app.drafts["200"].paths.is_empty() && app.drafts["200"].reply.is_none());
            assert!(commands.try_recv().is_err());
        }
    }
    #[test]
    fn contextual_enter_respects_blocked_contexts_modifiers_consumption_and_pending_limits() {
        for blocked in ["modal", "prompt", "search", "viewer", "popup", "permission", "busy", "signed-out",
            "empty", "pending-limit", "other-view", "completion", "shift", "ctrl", "alt", "repeat", "unfocused", "consumed"] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (_, mut state) = message_fixture();
            app.prefs.send_typing = false;
            let text = match blocked { "empty" => "  ", "completion" => "@Fi", _ => "Keep draft" };
            app.drafts.insert("200".into(), Draft { text: text.into(), ..Default::default() });
            app.composer_cursor = text.chars().count();
            match blocked {
                "modal" => app.dialog = Some(Dialog::Settings),
                "prompt" => app.prompt_for("Edit", "Other input", "edit", "300", "Keep prompt", "200", true),
                "search" => app.search_open = true,
                "viewer" => app.viewer = Some(Viewer { url: String::new(), local_path: None, zoom: 1., pan: egui::Vec2::ZERO }),
                "permission" => state.channel_access.can_send = false,
                "busy" => state.busy = true,
                "signed-out" => state.signed_in = false,
                "other-view" => state.view = "search".into(),
                "pending-limit" => for index in 0..64 { app.submissions.insert(index.to_string(), ("200".into(), 0)); },
                _ => {}
            }
            if blocked == "repeat" {
                // egui derives repeat from keys_down, ignoring a synthetic
                // repeat flag on the first press. Establish a real held key.
                let _ = ctx.run(egui::RawInput { events: vec![key_event(egui::Key::Enter, true)], ..Default::default() }, |ctx| {
                    prepare_composer_input(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| { ui.disable(); app.draw_composer(ui, &state); });
                });
            }
            let modifiers = match blocked { "shift" => egui::Modifiers::SHIFT, "ctrl" => egui::Modifiers::CTRL,
                "alt" => egui::Modifiers::ALT, _ => egui::Modifiers::NONE };
            let enter = egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true,
                repeat: blocked == "repeat", modifiers };
            let _ = ctx.run(egui::RawInput { events: vec![enter], modifiers, focused: blocked != "unfocused", ..Default::default() }, |ctx| {
                prepare_composer_input(ctx);
                if blocked == "popup" { egui::Popup::open_id(ctx, egui::Id::new("other-popup")); }
                if blocked == "consumed" { assert!(ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))); }
                egui::CentralPanel::default().show(ctx, |ui| app.draw_composer(ui, &state));
            });
            assert_eq!(app.drafts["200"].text, text, "{blocked}");
            assert!(commands.try_recv().is_err(), "{blocked}");
            assert!(!ctx.memory(|memory| memory.has_focus(egui::Id::new(("composer", "200")))), "{blocked}");
            if blocked == "prompt" { assert_eq!(app.prompt, "Keep prompt"); }
        }
    }
    #[test]
    fn confirming_another_editor_or_focused_control_never_sends_the_chat_draft() {
        for control in ["singleline", "multiline", "button"] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (_, state) = message_fixture();
            app.prefs.send_typing = false;
            app.drafts.insert("200".into(), Draft { text: "Keep chat draft".into(), ..Default::default() });
            let mut other_text = String::from("Other input");
            let mut clicked = false;
            let mut after_control = None;
            let mut other_id = None;
            for warmup in [true, false] {
                let events = if warmup { vec![] } else if control == "singleline" {
                    vec![key_event(egui::Key::Enter, true), egui::Event::Text("Stay out of chat".into())]
                } else { vec![key_event(egui::Key::Enter, true)] };
                let _ = ctx.run(egui::RawInput { events,
                    ..Default::default() }, |ctx| {
                    prepare_composer_input(ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let response = match control {
                            "singleline" => ui.add(egui::TextEdit::singleline(&mut other_text).id(egui::Id::new("other-editor"))),
                            "multiline" => ui.add(egui::TextEdit::multiline(&mut other_text).id(egui::Id::new("other-editor"))),
                            _ => ui.button("Other control")
                        };
                        other_id = Some(response.id);
                        clicked |= response.clicked();
                        if warmup { response.request_focus(); }
                        after_control = ui.memory(|memory| memory.focused());
                        app.draw_composer(ui, &state);
                    });
                });
                if warmup { assert!(ctx.memory(|memory| memory.has_focus(other_id.unwrap()))); }
            }
            if control == "singleline" { assert!(after_control.is_none(), "singleline Enter must actually release focus in this regression"); }
            if control == "multiline" { assert!(other_text.ends_with('\n')); }
            if control == "button" { assert!(clicked, "Enter must activate its focused control"); }
            assert_eq!(app.drafts["200"].text, "Keep chat draft");
            assert!(commands.try_recv().is_err(), "{control}");
        }
    }
    #[test]
    fn ime_owns_enter_across_composition_frames_and_shift_enter_keeps_its_newline() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, state) = message_fixture();
        app.prefs.send_typing = false;
        app.drafts.insert("200".into(), Draft { text: "Keep draft".into(), ..Default::default() });
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::Ime(egui::ImeEvent::Enabled), egui::Event::Ime(egui::ImeEvent::Preedit("候補".into()))]);
        for events in [vec![key_event(egui::Key::Enter, true)],
            vec![key_event(egui::Key::Enter, false), egui::Event::Ime(egui::ImeEvent::Commit("候補".into())), key_event(egui::Key::Enter, true)],
            vec![key_event(egui::Key::Enter, false), egui::Event::Ime(egui::ImeEvent::Disabled), key_event(egui::Key::Enter, true)]] {
            chat_frame(&mut app, &ctx, &state, events);
            assert_eq!(app.drafts["200"].text, "Keep draft");
            assert!(commands.try_recv().is_err());
            assert!(!ctx.wants_keyboard_input());
        }
        chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Enter, false), key_event(egui::Key::Enter, true)]);
        assert_eq!(commands.try_recv().unwrap().text, "Keep draft");
        app.drafts.insert("200".into(), Draft { text: "Line".into(), ..Default::default() });
        let _ = ctx.run(egui::RawInput { events: vec![key_event(egui::Key::Enter, false), egui::Event::Key {
            key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::SHIFT }],
            modifiers: egui::Modifiers::SHIFT, ..Default::default() }, |ctx| {
                prepare_composer_input(ctx);
                egui::CentralPanel::default().show(ctx, |ui| app.draw_composer(ui, &state));
            });
        assert!(app.drafts["200"].text.contains('\n'));
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn message_overflow_survives_pointer_exit_and_closes_with_escape() {
        let (mut app, ctx, _) = ui_harness();
        let (message, state) = message_fixture();
        ctx.style_mut(|style| style.animation_time = 0.);
        let first = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let body = painted_text(&first, "Fixture body").center();
        let mut hovered = message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(body)]);
        for _ in 0..2 { hovered = message_frame(&mut app, &ctx, &message, &state, vec![]); }
        let more = painted_text(&hovered, "⋯").center();
        for pressed in [true, false] { message_frame(&mut app, &ctx, &message, &state, pointer(more, pressed, egui::PointerButton::Primary)); }
        let mut opened = message_frame(&mut app, &ctx, &message, &state, vec![]);
        for _ in 0..2 { opened = message_frame(&mut app, &ctx, &message, &state, vec![]); }
        let copy = painted_text(&opened, "Copiar texto").center();
        let inside = message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(copy)]);
        painted_text(&inside, "Copiar texto");
        assert!(egui::Popup::is_id_open(&ctx, egui::Id::new(("message-overflow", "200", "300"))));
        message_frame(&mut app, &ctx, &message, &state, vec![key_event(egui::Key::Escape, true)]);
        assert!(!egui::Popup::is_id_open(&ctx, egui::Id::new(("message-overflow", "200", "300"))));
    }
    #[test]
    fn overflow_button_entire_width_and_popup_options_remain_interactive() {
        for scale in [1., 1.5] {
            let (mut app, ctx, _) = ui_harness();
            ctx.set_zoom_factor(scale);
            ctx.style_mut(|style| style.animation_time = 0.);
            let (message, mut state) = message_fixture();
            state.current_user = Some(message.author.clone()); state.can_pin_messages = true;
            let first = message_frame(&mut app, &ctx, &message, &state, vec![]);
            let body = painted_text(&first, "Fixture body").center();
            message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(body)]);
            for _ in 0..3 { message_frame(&mut app, &ctx, &message, &state, vec![]); }
            let region = ctx.data(|data| data.get_temp::<MessageActionRegion>(egui::Id::new("message-actions-region"))).unwrap();
            let right_edge = egui::pos2(region.more.right() - 0.2, region.more.center().y);
            for fraction in [0.1, 0.5, 0.9, 0.99] {
                let point = egui::pos2(egui::lerp(region.more.x_range(), fraction), region.more.center().y);
                let frame = message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(point)]);
                painted_text(&frame, "⋯");
            }
            for pressed in [true, false] { message_frame(&mut app, &ctx, &message, &state, pointer(right_edge, pressed, egui::PointerButton::Primary)); }
            let mut popup = message_frame(&mut app, &ctx, &message, &state, vec![]);
            for _ in 0..3 { popup = message_frame(&mut app, &ctx, &message, &state, vec![]); }
            let copy = painted_text(&popup, "Copiar texto").center();
            message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(copy)]);
            message_frame(&mut app, &ctx, &message, &state, pointer(copy, true, egui::PointerButton::Primary));
            let copied = message_frame(&mut app, &ctx, &message, &state, pointer(copy, false, egui::PointerButton::Primary));
            assert!(copied.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "Fixture body")));
            assert!(!egui::Popup::is_id_open(&ctx, egui::Id::new(("message-overflow", "200", "300"))));
        }
    }
    fn copied_history(output: &egui::FullOutput) -> String {
        output.platform_output.commands.iter().filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()), _ => None,
        }).last().unwrap_or("").to_owned()
    }
    #[test]
    fn bot_embed_and_component_only_messages_have_a_visible_body_and_disabled_actions() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, state) = message_fixture();
        let mut embed = notdiscord_core::parsing::message(&serde_json::json!({
            "id":"300","channel_id":"200","author":{"id":"100","username":"Dyno","bot":true},"content":"",
            "embeds":[{"title":"Server information","description":"Welcome to this server","fields":[{"name":"Rules","value":"Be kind"}],"footer":{"text":"Automated notice"}}]
        }));
        embed.author.avatar_url = None;
        let output = message_frame(&mut app, &ctx, &embed, &state, vec![]);
        for text in ["Dyno", "BOT", "Server information", "Welcome to this server", "Rules", "Be kind", "Automated notice"] { painted_text(&output, text); }
        let mut component = notdiscord_core::parsing::message(&serde_json::json!({
            "id":"301","channel_id":"200","flags":32768,"author":{"id":"100","username":"Dyno","bot":true},
            "components":[{"type":17,"components":[{"type":10,"content":"Component-only body"},{"type":1,"components":[{"type":2,"label":"Approve","style":1,"custom_id":"not-an-executable-command"}]}]}]
        }));
        component.author.avatar_url = None;
        let output = message_frame(&mut app, &ctx, &component, &state, vec![]);
        painted_text(&output, "Component-only body");
        let button = painted_text(&output, "Approve").center();
        for pressed in [true, false] { message_frame(&mut app, &ctx, &component, &state, pointer(button, pressed, egui::PointerButton::Primary)); }
        assert!(commands.try_recv().is_err(), "custom-ID controls must never submit bot interactions");
        assert_eq!(app.pictures.pending_fixture_requests(), 0);
        embed.webhook_id = "900".into(); embed.author.is_bot = false;
        let webhook = message_frame(&mut app, &ctx, &embed, &state, vec![]);
        painted_text(&webhook, "WEBHOOK");
    }
    #[test]
    fn bot_component_media_uses_one_card_and_deleted_rich_content_starts_no_download() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, state) = message_fixture();
        let url = "https://media.discordapp.net/attachments/10/20/synthetic.gif?ex=ffffffff";
        let alias = "https://cdn.discordapp.com/attachments/10/20/synthetic.gif?ex=eeeeeeee";
        let mut message = notdiscord_core::parsing::message(&serde_json::json!({
            "id":"300","channel_id":"200","flags":32768,"author":{"id":"100","username":"Synthetic Bot","bot":true},
            "attachments":[{"id":"20","filename":"synthetic.gif","url":alias,"content_type":"image/gif","width":2,"height":2}],
            "components":[{"type":10,"content":alias},{"type":12,"items":[{"media":{"url":url}},{"media":{"url":alias}}]}]
        }));
        message.author.avatar_url = None;
        let id = app.pictures.seed_sized_fixture(&ctx, url, 384);
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let cards = output.shapes.iter().filter(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) => mesh.texture_id == id,
            egui::Shape::Rect(rect) => rect.fill_texture_id() == id, _ => false,
        }).count();
        assert_eq!(cards, 1, "content, attachments and component aliases must share one displayed media card");
        assert_eq!(app.pictures.pending_fixture_requests(), 0);
        message.deleted = true;
        message.embeds = vec![ChatEmbed { title: "Known deleted bot text".into(), ..Default::default() }];
        let deleted = message_frame(&mut app, &ctx, &message, &state, vec![]);
        assert!(deleted.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text == "Known deleted bot text" && text.galley.job.sections.iter().all(|section| section.format.color == Color32::from_rgb(242,63,67)))));
        assert_eq!(app.pictures.pending_fixture_requests(), 0);
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn thumb_drag_keeps_its_position_and_latest_uses_the_measured_end_without_fetching() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (template, mut state) = message_fixture();
        state.messages = (0..80).map(|index| ChatMessage {
            id:(1000 + index).to_string(), content:format!("Thumb row {index:02}"), ..template.clone()
        }).collect();
        // Production temporarily moves AppSnapshot out of self while drawing.
        // The displayed channel must come from the local snapshot parameter.
        app.state = AppSnapshot::welcome();
        for _ in 0..5 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!(app.positions["200"].at_bottom);
        // Following intent can be false while physically at the real bottom.
        app.scroll_bottom = false;
        let bottom = chat_frame(&mut app, &ctx, &state, vec![]);
        let button = painted_text(&bottom, "Volver al inicio ↓").center();
        for pressed in [true, false] {
            app.scroll_bottom = false;
            chat_frame(&mut app, &ctx, &state, pointer(button, pressed, egui::PointerButton::Primary));
        }
        assert!(commands.try_recv().is_err());
        let initial = app.positions["200"].offset;
        let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
        let bar = ctx.style().spacing.scroll;
        let track = egui::pos2(viewport.right() + bar.bar_inner_margin + bar.bar_width * 0.5,
            viewport.top() + viewport.height() * 0.30);
        chat_frame(&mut app, &ctx, &state, pointer(track, true, egui::PointerButton::Primary));
        let chosen = app.positions["200"].offset;
        assert!(chosen > 0. && chosen < initial - 100., "the actual scrollbar must move: {initial} -> {chosen}");
        chat_frame(&mut app, &ctx, &state, pointer(track, false, egui::PointerButton::Primary));
        for _ in 0..15 {
            chat_frame(&mut app, &ctx, &state, vec![]);
            assert!((app.positions["200"].offset - chosen).abs() < 1., "anchor restoration undid the thumb gesture");
        }
        let output = chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(button)]);
        assert_eq!(output.platform_output.cursor_icon, egui::CursorIcon::PointingHand);
        chat_frame(&mut app, &ctx, &state, pointer(button, true, egui::PointerButton::Primary));
        let jumped = chat_frame(&mut app, &ctx, &state, pointer(button, false, egui::PointerButton::Primary));
        let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
        assert!(viewport.contains_rect(painted_text(&jumped, "Thumb row 79")), "the returned frame must already paint the newest row");
        assert!(app.positions["200"].at_bottom);
        assert!(commands.try_recv().is_err(), "the retained latest page must jump locally");
    }
    #[test]
    fn active_middle_scroll_compensates_prepend_and_media_growth_without_a_visible_jump() {
        let (mut app, ctx, _) = ui_harness();
        let (template, mut state) = message_fixture();
        state.messages = (0..80).map(|index| ChatMessage {
            id:(1000 + index).to_string(), content:format!("Motion row {index:02} alpha beta"), ..template.clone()
        }).collect();
        app.scroll_bottom = false;
        app.pending_scroll = Some(700.);
        for _ in 0..4 { chat_frame(&mut app, &ctx, &state, vec![]); }
        let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
        let origin = viewport.center();
        for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(origin, pressed, egui::PointerButton::Middle)); }
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(origin - egui::vec2(0., 100.))]);
        let before = chat_frame(&mut app, &ctx, &state, vec![]);
        let (target, rect) = (0..80).map(|index| {
            let text = format!("Motion row {index:02} alpha beta");
            let rect = painted_text(&before, &text); (text, rect)
        }).find(|(_, rect)| viewport.shrink(35.).contains_rect(*rect)).unwrap();
        let mut changed:Vec<_> = (0..10).map(|index| ChatMessage {
            id:(900 + index).to_string(), content:format!("Prepended row {index}"), ..template.clone()
        }).collect();
        let mut retained = state.messages.to_vec();
        retained[0].content = "Expanded media placeholder\n".repeat(12);
        changed.extend(retained);
        changed.push(ChatMessage { id:"2000".into(), content:"New incoming message".into(), ..template.clone() });
        state.messages = changed.into();
        let after = chat_frame(&mut app, &ctx, &state, vec![]);
        assert!((painted_text(&after, &target).top() - rect.top()).abs() < 30., "new rows/media displaced the reader instead of preserving the anchor");
        let mut previous = app.positions["200"].offset;
        for _ in 0..15 {
            chat_frame(&mut app, &ctx, &state, vec![]);
            let offset = app.positions["200"].offset;
            assert!(offset <= previous + 0.5, "geometry compensation was applied repeatedly: {previous} -> {offset}");
            previous = offset;
        }
        assert!(!app.scroll_bottom);
    }
    #[test]
    fn chat_profile_names_and_plain_url_links_expose_the_hand_cursor() {
        let (mut app, ctx, _) = ui_harness();
        let (mut message, state) = message_fixture();
        message.content = "Visit https://example.invalid/path".into();
        let initial = message_frame(&mut app, &ctx, &message, &state, vec![]);
        for label in ["Fixture author", "https://example.invalid/path"] {
            let point = painted_text(&initial, label).center();
            let output = message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(point)]);
            assert_eq!(output.platform_output.cursor_icon, egui::CursorIcon::PointingHand, "{label}");
        }
    }
    #[test]
    fn history_scroll_from_the_live_end_does_not_bounce_back_during_middle_mode() {
        let (mut app, ctx, _) = ui_harness();
        let (template, mut state) = message_fixture();
        state.messages = (0..80).map(|index| ChatMessage {
            id: (300 + index).to_string(), content: format!("Stable row {index:02} alpha beta gamma"),
            ..template.clone()
        }).collect::<Vec<_>>().into();
        for _ in 0..5 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!(app.positions["200"].at_bottom);
        let initial = app.positions["200"].offset;
        let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
        let origin = egui::pos2(viewport.left() + 180., viewport.center().y);
        for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(origin, pressed, egui::PointerButton::Middle)); }
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(origin - egui::vec2(0., 100.))]);
        let mut previous = app.positions["200"].offset;
        for _ in 0..60 {
            chat_frame(&mut app, &ctx, &state, vec![]);
            let offset = app.positions["200"].offset;
            assert!(offset <= previous + 0.1, "automatic upward scroll bounced: {previous} -> {offset}");
            previous = offset;
        }
        assert!(previous < initial - 100.);
        chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Escape, true)]);
        for _ in 0..3 { chat_frame(&mut app, &ctx, &state, vec![]); }
        let stopped = app.positions["200"].offset;
        let anchor = app.positions["200"].anchor.clone();
        let top = app.positions["200"].anchor_top;
        let mut changed = state.messages.to_vec();
        changed.push(ChatMessage { id:"500".into(), content:"New incoming row".into(), ..template.clone() });
        // A retained media/message above the viewport changes its measured height.
        changed[0].content = "Expanded media placeholder\n".repeat(12);
        state.messages = changed.into();
        for _ in 0..5 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert_eq!(app.positions["200"].anchor, anchor);
        assert!((app.positions["200"].anchor_top - top).abs() < 1.);
        let settled = app.positions["200"].offset;
        assert!(settled >= stopped);
        for _ in 0..10 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!((app.positions["200"].offset - settled).abs() < 0.1);
        assert!(!app.scroll_bottom);
    }
    #[test]
    fn selecting_up_from_the_live_end_stays_monotonic_and_keeps_copied_text() {
        let (mut app, ctx, _) = ui_harness();
        let (template, mut state) = message_fixture();
        state.messages = (0..80).map(|index| ChatMessage {
            id:(300 + index).to_string(), content:format!("Bottom {index:02} alpha beta gamma delta"), ..template.clone()
        }).collect::<Vec<_>>().into();
        let mut frame = chat_frame(&mut app, &ctx, &state, vec![]);
        for _ in 0..4 { frame = chat_frame(&mut app, &ctx, &state, vec![]); }
        let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
        let start = painted_text(&frame, "Bottom 78 alpha beta gamma delta").left_center() + egui::vec2(4., 0.);
        assert!(viewport.contains(start));
        chat_frame(&mut app, &ctx, &state, pointer(start, true, egui::PointerButton::Primary));
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(egui::pos2(start.x, viewport.top()+2.))]);
        let before = copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy]));
        let initial = app.positions["200"].offset;
        let mut previous = initial;
        for _ in 0..60 {
            chat_frame(&mut app, &ctx, &state, vec![]);
            let offset = app.positions["200"].offset;
            assert!(offset <= previous + 0.1, "selection upward scroll bounced: {previous} -> {offset}");
            previous = offset;
        }
        assert!(previous < initial - 100.);
        let after = copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy]));
        assert!(after.len() > before.len() && !before.is_empty());
    }
    #[test]
    fn wheel_over_message_toolbar_scrolls_the_history_but_not_through_a_menu() {
        let (mut app, ctx, _) = ui_harness();
        let (template, mut state) = message_fixture();
        state.messages = (0..80).map(|index| ChatMessage {
            id:(300 + index).to_string(), content:format!("Toolbar row {index:02}"), ..template.clone()
        }).collect::<Vec<_>>().into();
        app.scroll_bottom = false;
        app.pending_scroll = Some(300.);
        let mut frame = chat_frame(&mut app, &ctx, &state, vec![]);
        for _ in 0..4 { frame = chat_frame(&mut app, &ctx, &state, vec![]); }
        let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
        let text = (0..80).map(|index| painted_text(&frame, &format!("Toolbar row {index:02}")))
            .find(|rect| viewport.contains_rect(*rect)).unwrap();
        for _ in 0..2 { chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(text.center())]); }
        let region = ctx.data(|data| data.get_temp::<MessageActionRegion>(egui::Id::new("message-actions-region"))).unwrap();
        for _ in 0..2 { chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(region.more.center())]); }
        let start = app.positions["200"].offset;
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::MouseWheel {
            unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.,-160.),modifiers:egui::Modifiers::NONE
        }]);
        for _ in 0..18 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!(app.positions["200"].offset > start + 120.);
        let frame = chat_frame(&mut app, &ctx, &state, vec![]);
        let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
        let text = (0..80).map(|index| painted_text(&frame, &format!("Toolbar row {index:02}")))
            .find(|rect| viewport.contains_rect(*rect)).unwrap();
        for _ in 0..2 { chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(text.center())]); }
        let region = ctx.data(|data| data.get_temp::<MessageActionRegion>(egui::Id::new("message-actions-region"))).unwrap();
        for pressed in [true,false] { chat_frame(&mut app, &ctx, &state, pointer(region.more.center(),pressed,egui::PointerButton::Primary)); }
        assert!(egui::Popup::is_any_open(&ctx));
        let stopped = app.positions["200"].offset;
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::MouseWheel {
            unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.,-160.),modifiers:egui::Modifiers::NONE
        }]);
        for _ in 0..10 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!((app.positions["200"].offset - stopped).abs() < 0.1);
    }
    #[test]
    fn held_text_selection_recomputes_its_endpoint_after_wheel_and_stationary_edge_scroll() {
        let (mut app, ctx, _) = ui_harness();
        let (template, mut state) = message_fixture();
        app.scroll_bottom = false;
        state.messages = (0..80).map(|index| ChatMessage { id: (300 + index).to_string(),
            content: format!("Selectable {index:02} alpha beta gamma delta"), ..template.clone() }).collect::<Vec<_>>().into();
        let first = chat_frame(&mut app, &ctx, &state, vec![]);
        let text = painted_text(&first, "Selectable 00 alpha beta gamma delta");
        let start = text.left_center() + egui::vec2(3., 0.);
        let end = start + egui::vec2(110., 0.);
        chat_frame(&mut app, &ctx, &state, pointer(start, true, egui::PointerButton::Primary));
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(end)]);
        let before = copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy]));
        assert!(!before.is_empty());
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0., -350.), modifiers: egui::Modifiers::NONE }]);
        let mut wheel_offsets = Vec::new();
        for _ in 0..18 {
            chat_frame(&mut app, &ctx, &state, vec![]);
            wheel_offsets.push(app.positions["200"].offset);
        }
        let after_wheel = copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy]));
        assert!(app.positions["200"].offset > 100., "held selection must receive wheel input; observed offsets={wheel_offsets:?}, pending={:?}", app.pending_scroll);
        assert!(after_wheel.len() > before.len(), "wheel must extend copied text: {before:?} -> {after_wheel:?}");
        let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
        let bottom = egui::pos2(start.x, viewport.bottom() - 2.);
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(bottom)]);
        let offset = app.positions["200"].offset;
        let initial_edge = copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy]));
        for _ in 0..75 { chat_frame(&mut app, &ctx, &state, vec![]); }
        let after_edge = copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy]));
        assert!(app.positions["200"].offset > offset + 100.);
        assert!(after_edge.len() > initial_edge.len(), "stationary pointer must extend the endpoint as rows move");
        chat_frame(&mut app, &ctx, &state, pointer(bottom, false, egui::PointerButton::Primary));
        for _ in 0..3 { chat_frame(&mut app, &ctx, &state, vec![]); }
        let stopped = app.positions["200"].offset;
        for _ in 0..15 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!((stopped - app.positions["200"].offset).abs() < 0.1);
        let final_copy = copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy]));
        assert!(!final_copy.is_empty());
    }
    #[test]
    fn middle_history_scroll_toggles_stops_with_escape_and_leaves_interactive_children_alone() {
        let (mut app, ctx, _) = ui_harness();
        let (template, mut state) = message_fixture();
        app.scroll_bottom = false;
        state.messages = (0..60).map(|index| ChatMessage { id: (300 + index).to_string(),
            content: format!("Middle scroll row {index}"), ..template.clone() }).collect::<Vec<_>>().into();
        chat_frame(&mut app, &ctx, &state, vec![]);
        let origin = egui::pos2(450., 180.);
        for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(origin, pressed, egui::PointerButton::Middle)); }
        assert!(ctx.data(|data| data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"))).unwrap().origin.is_some());
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(origin + egui::vec2(0., 100.))]);
        for _ in 0..30 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!(app.positions["200"].offset > 50.);
        for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(origin, pressed, egui::PointerButton::Middle)); }
        assert!(ctx.data(|data| data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"))).unwrap().origin.is_none());
        for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(origin, pressed, egui::PointerButton::Middle)); }
        chat_frame(&mut app, &ctx, &state, vec![key_event(egui::Key::Escape, true)]);
        assert!(ctx.data(|data| data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"))).unwrap().origin.is_none());
        let (mut app, ctx, _) = ui_harness();
        let mut message = template;
        message.reply_preview = Some(MessageReplyPreview { message_id:"250".into(), channel_id:"200".into(), content:"Interactive original".into(), ..Default::default() });
        state.messages = vec![message].into();
        let frame = chat_frame(&mut app, &ctx, &state, vec![]);
        let child = painted_text(&frame, "Interactive original").center();
        for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(child, pressed, egui::PointerButton::Middle)); }
        assert!(ctx.data(|data| data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"))).unwrap().origin.is_none());
    }
    #[test]
    fn every_custom_emoji_uses_message_hover_then_releases_animation_textures_on_exit() {
        let (mut app, ctx, _) = ui_harness();
        let (mut message, state) = message_fixture();
        app.prefs.allow_gif_playback = true;
        message.content = "Hover this sentence <a:First:401> and <a:Second:402>".into();
        let mut animated_ids = Vec::new();
        for id in ["401", "402"] {
            let url = StatusEmoji { id: id.into(), name: "Synthetic".into(), animated: true }.image_url().unwrap();
            app.pictures.seed_sized_fixture(&ctx, &url.replace("&animated=true", ""), 64);
            animated_ids.extend(app.pictures.seed_emoji_animation_fixture(&ctx, &url, 2));
        }
        let first = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let sentence = painted_text(&first, "Hover this sentence ").left_center() + egui::vec2(4., 0.);
        app.pictures.begin_frame();
        let hovered = message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(sentence)]);
        app.pictures.finish_frame(false);
        assert_eq!(app.pictures.custom_animation_metrics()["requested"], 2);
        assert_eq!(app.pictures.custom_animation_metrics()["entries"], 2);
        for id in [animated_ids[0], animated_ids[2]] {
            assert!(hovered.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) => mesh.texture_id == id,
                egui::Shape::Rect(rect) => rect.fill_texture_id() == id, _ => false,
            }), "all emojis should play even though pointer is over ordinary text");
        }
        app.pictures.begin_frame();
        let exited = message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(egui::pos2(650., 450.))]);
        app.pictures.finish_frame(false);
        let metrics = app.pictures.custom_animation_metrics();
        assert_eq!(metrics["entries"], 0); assert_eq!(metrics["bytes"], 0); assert_eq!(metrics["pending"], 0);
        assert!(!exited.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if animated_ids.contains(&rect.fill_texture_id()))));
        assert_eq!(metrics["worker"], false, "injected fixtures must never initialize an HTTP worker");
    }
    #[test]
    fn double_clicking_another_message_prepares_a_reply_and_preserves_the_draft() {
        for case in ["text", "background", "forwarded", "denied", "deleted", "search"] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (mut message, mut state) = message_fixture();
            state.current_user = Some(Person { id: "101".into(), name: "Local".into(), ..Default::default() });
            message.forwarded = case == "forwarded";
            message.deleted = case == "deleted";
            state.channel_access.can_send = case != "denied";
            if case == "search" { state.view = "search".into(); }
            let attachment = PathBuf::from("synthetic-draft.bin");
            app.drafts.insert("200".into(), Draft { text: "Keep this draft".into(), paths: vec![attachment.clone()], revision: 7, ..Default::default() });
            let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
            let text = painted_text(&output, "Fixture body");
            let position = if case == "background" {
                let row = ctx.data(|data|data.get_temp::<(egui::Rect,bool,bool,bool)>(egui::Id::new("fixture-message-response"))).unwrap().0;
                egui::pos2(row.right()-10., text.center().y)
            } else { text.center() };
            for _ in 0..2 {
                for pressed in [true, false] { message_frame(&mut app, &ctx, &message, &state, pointer(position, pressed, egui::PointerButton::Primary)); }
            }
            let draft = &app.drafts["200"];
            assert_eq!(draft.text, "Keep this draft");
            assert_eq!(draft.paths, vec![attachment]);
            if matches!(case, "denied"|"deleted"|"search") {
                assert!(draft.reply.is_none(), "{case}");
                assert_eq!(draft.revision, 7);
            } else {
                assert_eq!(draft.reply.as_ref().map(|reply|reply.message_id.as_str()), Some("300"), "{case}");
                assert_eq!(draft.revision, 8);
            }
            assert!(app.dialog.is_none());
            assert!(commands.try_recv().is_err(), "a reply shortcut only prepares a draft");
        }
    }
    #[test]
    fn near_recent_messages_follow_new_arrivals_but_older_reading_and_edits_keep_the_anchor() {
        for distance in [100., 500.] {
            let (mut app, updates, mut commands) = super::super::tests::harness();
            let ctx = app.ctx.clone();
            let (template, mut state) = message_fixture();
            state.current_user = Some(Person { id: "101".into(), ..Default::default() });
            state.messages = (0..60).map(|index|ChatMessage { id:(1000+index).to_string(), content:format!("Arrival row {index}"),
                timestamp:template.timestamp+chrono::Duration::seconds(index), ..template.clone() }).collect();
            updates.send(UiUpdate::Snapshot(state.clone())).unwrap();app.process_updates(&ctx);
            for _ in 0..3 { chat_frame(&mut app, &ctx, &state, vec![]); }
            let end = app.positions["200"].offset;
            assert!(end > 600.);
            app.scroll_bottom=false;app.pending_scroll=Some(end-distance);app.restore_anchor=None;
            for _ in 0..3 { chat_frame(&mut app, &ctx, &state, vec![]); }
            let before = app.positions["200"].offset;
            assert!((app.positions["200"].distance_to_bottom.unwrap()-distance).abs()<1.);
            let anchor=app.positions["200"].anchor.clone();
            let mut next=state.clone();let mut messages=next.messages.to_vec();
            messages.push(ChatMessage {id:"1060".into(),content:"New received message".into(),
                timestamp:template.timestamp+chrono::Duration::seconds(60),..template.clone()});next.messages=messages.into();
            updates.send(UiUpdate::Snapshot(next.clone())).unwrap();app.process_updates(&ctx);
            for _ in 0..3 { chat_frame(&mut app, &ctx, &next, vec![]); }
            if distance < 180. { assert!(app.positions["200"].at_bottom); }
            else { assert_eq!(app.positions["200"].anchor,anchor);assert!((app.positions["200"].offset-before).abs()<1.); }
            assert!(commands.try_recv().is_err(),"following a locally retained tail does not request more history");

            app.scroll_bottom=false;
            app.positions.get_mut("200").unwrap().distance_to_bottom=Some(80.);
            app.positions.get_mut("200").unwrap().at_bottom=false;
            let mut edited=next.clone();let mut messages=edited.messages.to_vec();messages.last_mut().unwrap().content="Edited tail".into();edited.messages=messages.into();
            assert!(!app.follows_nearby_new_messages(&ctx,&edited),"editing the last message is not an arrival");
            let mut next_page=next.clone();next_page.focus_message_version+=1;
            assert!(!app.follows_nearby_new_messages(&ctx,&next_page),"jump navigation owns its position");
            let mut arriving=next.clone();let mut messages=arriving.messages.to_vec();
            messages.push(ChatMessage{id:"1061".into(),timestamp:template.timestamp+chrono::Duration::seconds(61),..template.clone()});arriving.messages=messages.into();
            let scope=egui::Id::new(("message-scroll", &next.selected_channel_id, "chat"));
            ctx.data_mut(|data|data.insert_temp(egui::Id::new("history-middle-scroll"),
                HistoryMiddleScroll{scope:Some(scope),origin:Some(egui::pos2(100.,100.)),..Default::default()}));
            assert!(!app.follows_nearby_new_messages(&ctx,&arriving),"a latched middle-scroll remains manual after button release");
            assert!(ctx.data(|data|data.get_temp::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll"))).unwrap().origin.is_some());
            ctx.data_mut(|data|data.remove::<HistoryMiddleScroll>(egui::Id::new("history-middle-scroll")));
            let mut older=next.clone();older.channels=vec![NavItem{id:"200".into(),last_message_id:"9999".into(),..Default::default()}];
            app.state=older;assert!(!app.near_recent_history(&ctx,&app.state),"the bottom of an old page is not the newest chat");
        }
    }
    #[test]
    fn own_message_double_click_edits_and_delete_click_uses_the_confirmation_flow() {
        for shortcut in ["double", "delete", "other", "denied", "link"] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (mut message, mut state) = message_fixture();
            state.current_user = Some(message.author.clone());
            if shortcut == "other" { state.current_user.as_mut().unwrap().id = "another".into(); }
            if shortcut == "denied" { state.channel_access.can_send = false; }
            if shortcut == "link" { message.reply_preview = Some(MessageReplyPreview { message_id:"250".into(), channel_id:"200".into(),
                content:"Reply child".into(), author:Some(message.author.clone()), ..Default::default() }); }
            let first = message_frame(&mut app, &ctx, &message, &state, vec![]);
            let position = painted_text(&first, if shortcut == "link" {"Reply child"} else {"Fixture body"}).center();
            let mut press = pointer(position, true, egui::PointerButton::Primary);
            if shortcut != "double" { press.insert(0, key_event(egui::Key::Delete, true)); }
            message_frame(&mut app, &ctx, &message, &state, press);
            message_frame(&mut app, &ctx, &message, &state, pointer(position, false, egui::PointerButton::Primary));
            if shortcut == "double" {
                for pressed in [true, false] { message_frame(&mut app, &ctx, &message, &state, pointer(position, pressed, egui::PointerButton::Primary)); }
                assert!(matches!(&app.dialog, Some(Dialog::Prompt { action, .. }) if action.kind == "edit" && action.id == message.id));
            } else if shortcut == "delete" {
                assert!(matches!(&app.dialog, Some(Dialog::Confirm { action, .. }) if action.kind == "delete" && action.id == message.id));
            } else {
                assert!(app.dialog.is_none(), "{shortcut}");
            }
            // The delete shortcut opens a reviewable confirmation; it never
            // directly submits a destructive action.
            while let Ok(action) = commands.try_recv() { assert_ne!(action.kind, "delete"); }
        }
    }
    #[test]
    fn selection_scroll_is_progressive_and_stops_outside_the_edge_or_after_release() {
        let viewport = egui::Rect::from_min_size(egui::pos2(10., 10.), egui::vec2(400., 300.));
        let slow = selection_scroll_velocity(viewport, egui::pos2(50., 40.), true);
        let fast = selection_scroll_velocity(viewport, egui::pos2(50., 12.), true);
        assert!(slow < 0. && fast < slow && fast >= -650.);
        assert!(selection_scroll_velocity(viewport, egui::pos2(50., 309.), true) > 600.);
        for (point, active) in [(egui::pos2(50., 150.), true), (egui::pos2(500., 12.), true), (egui::pos2(50., 12.), false)] {
            assert_eq!(selection_scroll_velocity(viewport, point, active), 0.);
        }
    }
    #[test]
    fn wheel_scrolling_keeps_selection_when_its_origin_leaves_the_viewport() {
        let (mut app, ctx, _) = ui_harness();
        let (template, mut state) = message_fixture();
        app.scroll_bottom = false;
        state.messages = (0..30).map(|index| ChatMessage { id: (300+index).to_string(),
            content: format!("Selectable {index} alpha beta gamma"), ..template.clone() }).collect::<Vec<_>>().into();
        let first = chat_frame(&mut app, &ctx, &state, vec![]);
        let text = painted_text(&first, "Selectable 0 alpha beta gamma");
        let start = text.left_center() + egui::vec2(3., 0.);
        let end = start + egui::vec2(115., 0.);
        chat_frame(&mut app, &ctx, &state, pointer(start, true, egui::PointerButton::Primary));
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(end)]);
        chat_frame(&mut app, &ctx, &state, pointer(end, false, egui::PointerButton::Primary));
        assert!(ctx.data(|data| data.get_temp::<HistorySelection>(egui::Id::new("history-selection"))).unwrap().anchor.is_some());
        chat_frame(&mut app, &ctx, &state, vec![egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0., -500.), modifiers: egui::Modifiers::NONE }]);
        for _ in 0..15 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!(app.positions["200"].offset > 100.);
        assert!(ctx.data(|data| data.get_temp::<HistorySelection>(egui::Id::new("history-selection"))).unwrap().anchor.is_some());
    }
    #[test]
    fn observed_versions_render_above_current_and_deleted_content_stays_red() {
        let (mut app, ctx, _) = ui_harness();
        let (mut message, state) = message_fixture();
        message.edited = true; message.previous_contents = vec!["Observed earlier".into()];
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let previous = painted_text(&output, "Observed earlier");
        let current = painted_text(&output, "Fixture body");
        assert!(previous.bottom() <= current.top());
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text == "Observed earlier" && text.galley.job.sections.iter().all(|section| section.format.italics && section.format.color == app.theme.muted))));
        message.deleted = true;
        let deleted = message_frame(&mut app, &ctx, &message, &state, vec![]);
        painted_text(&deleted, "Mensaje eliminado");
        assert!(deleted.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text == "Fixture body" && text.galley.job.sections.iter().all(|section| section.format.color == Color32::from_rgb(242,63,67)))));
    }
    #[test]
    fn live_server_nickname_updates_old_message_and_reply_labels_but_preserves_webhooks_and_forwards(){
        let(mut app,ctx,_)=ui_harness();let(mut message,mut state)=message_fixture();
        state.selected_guild_id="10".into();message.author.username="Account".into();message.author.display_name="Global".into();
        state.guild_nicknames.insert(message.author.id.clone(),"Server nickname".into());
        let output=message_frame(&mut app,&ctx,&message,&state,vec![]);painted_text(&output,"Server nickname");
        state.guild_nicknames.insert(message.author.id.clone(),"Updated nickname".into());
        let output=message_frame(&mut app,&ctx,&message,&state,vec![]);painted_text(&output,"Updated nickname");
        let reply=MessageReplyPreview{message_id:message.id.clone(),channel_id:message.channel_id.clone(),author:Some(message.author.clone()),..Default::default()};
        assert_eq!(reply_display_name(&reply,reply.author.as_ref().unwrap(),&state),"Updated nickname");
        message.webhook_id="99".into();message.author.name="Webhook override".into();assert_eq!(message_display_name(&message,&state),"Webhook override");
        message.webhook_id.clear();message.forwarded=true;message.author.name="Forwarded origin".into();assert_eq!(message_display_name(&message,&state),"Forwarded origin");
        message.forwarded=false;state.selected_guild_id.clear();assert_eq!(message_display_name(&message,&state),"Forwarded origin");
    }
    #[test]
    fn everyone_completion_preserves_the_literal_mention_and_rendering_needs_server_metadata() {
        let mut draft = Draft { text: "Hi @ev".into(), ..Default::default() };
        insert_mention(&mut draft, 3, 6, &Person { id: "everyone".into(), name: "everyone".into(), ..Default::default() });
        assert_eq!(outgoing_mentions(&draft), "Hi @everyone ");
        assert!(draft.mentions.is_empty());
        let (mut app, ctx, _) = ui_harness();
        let (mut message, mut state) = message_fixture();
        message.content = "Hello @everyone".into(); state.selected_guild_id = "10".into();
        let plain = message_frame(&mut app, &ctx, &message, &state, vec![]);
        painted_text(&plain, "Hello ");
        painted_text(&plain, "@everyone");
        assert!(!plain.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text == "@everyone" && text.galley.job.sections.iter().any(|section| section.format.background != Color32::TRANSPARENT))));
        message.mentions_everyone = true;
        let mentioned = message_frame(&mut app, &ctx, &message, &state, vec![]);
        painted_text(&mentioned, "@everyone");
        assert!(mentioned.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text == "@everyone" && text.galley.job.sections.iter().any(|section| section.format.background != Color32::TRANSPARENT))));
    }
    #[test]
    fn named_custom_emojis_require_known_unambiguous_ids_and_explicit_tokens_take_priority() {
        let (mut message, mut state) = message_fixture();
        state.selected_guild_id = "10".into();
        state.emojis = vec![CustomEmoji {
            id: "401".into(),
            name: "MoniChibiSpin".into(),
            animated: true,
            guild_id: "10".into(),
            ..Default::default()
        }].into();
        message.content = ":MoniChibiSpin: :unknown: <:MoniChibiSpin:402>".into();
        let captures = message_rich_pattern()
            .captures_iter(&message.content)
            .collect::<Vec<_>>();
        assert_eq!(captures.len(), 3);
        assert_eq!(&captures[0][5], "MoniChibiSpin");
        assert_eq!(&captures[2][4], "402");
        assert!(
            captures[2].get(5).is_none(),
            "full Discord tokens must be parsed before name aliases"
        );
        assert_eq!(
            known_named_emoji("MoniChibiSpin", &message, &state)
                .unwrap()
                .id,
            "402"
        );
        assert!(known_named_emoji("unknown", &message, &state).is_none());
        message.content = ":MoniChibiSpin:".into();
        let known = known_named_emoji("MoniChibiSpin", &message, &state).unwrap();
        assert_eq!((known.id.as_str(), known.animated), ("401", true));
        state.emojis.push(CustomEmoji {
            id: "403".into(),
            ..state.emojis[0].clone()
        });
        assert!(known_named_emoji("MoniChibiSpin", &message, &state).is_none());
        state.selected_guild_id = "11".into();
        assert!(known_named_emoji("MoniChibiSpin", &message, &state).is_none());
        message.content = "<:MoniChibiSpin:402> <a:MoniChibiSpin:403> :MoniChibiSpin:".into();
        assert!(known_named_emoji("MoniChibiSpin", &message, &state).is_none());
    }
    #[test]
    fn reply_mentions_resolve_before_truncation_in_history_and_composer_without_fetching_people() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (mut message, state) = message_fixture();
        let person = Person {
            id: "42".into(),
            name: "Nombre visible".into(),
            ..Default::default()
        };
        let reply = MessageReplyPreview {
            message_id: "250".into(),
            channel_id: "200".into(),
            author: Some(message.author.clone()),
            content: "Hola <@!42> y <@999>".into(),
            mentions: vec![person],
            ..Default::default()
        };
        assert_eq!(
            readable_mentions(&reply.content, &reply.mentions, &state),
            "Hola @Nombre visible y @Usuario"
        );
        message.reply_preview = Some(reply.clone());
        let history = message_frame(&mut app, &ctx, &message, &state, vec![]);
        painted_text(&history, "Hola @Nombre visible y @Usuario");
        app.drafts.insert(
            "200".into(),
            Draft {
                reply: Some(reply),
                ..Default::default()
            },
        );
        let composer = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| app.draw_composer(ui, &state));
        });
        painted_text(&composer, "Hola @Nombre visible y @Usuario");
        assert!(
            commands.try_recv().is_err(),
            "rendering a reply must not fetch a profile or mutate the chat"
        );
    }
    #[test]
    fn mention_highlight_shares_text_line_height_without_button_padding() {
        for size in [14., 17., 22.] {
            let (mut app, ctx, _) = ui_harness();
            app.prefs.chat_size = size;
            let (mut message, state) = message_fixture();
            message.content = "Texto <@100> final".into();
            message.mentions = vec![message.author.clone()];
            let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
            let plain = painted_text(&output, "Texto ");
            let mention = painted_text(&output, "@Fixture author");
            assert!(
                (plain.height() - mention.height()).abs() <= 1.,
                "size={size} plain={plain:?} mention={mention:?}"
            );
            assert!(
                (plain.center().y - mention.center().y).abs() <= 1.,
                "mention baseline moved away from its line"
            );
        }
    }
    #[test]
    fn joins_and_other_channel_activity_use_compact_records_without_user_message_bodies() {
        let (mut app, ctx, _) = ui_harness();
        let (mut message, state) = message_fixture();
        for kind in [1, 2, 4, 5, 7, 8, 9, 10, 11, 12, 18] {
            assert!(is_system_record(kind));
            message.message_type = kind;
            message.content = String::new();
            let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
            let author = painted_text(&output, "Fixture author");
            let description = painted_text(&output, &system_record_description(&message));
            assert!((author.center().y - description.center().y).abs() <= 2.);
        }
        // Additional public system events have neutral descriptions even when
        // the payload omits their optional, event-specific metadata.
        for kind in [
            14, 15, 16, 17, 22, 24, 25, 26, 27, 28, 29, 31, 32, 36, 37, 38, 39, 44, 46,
        ] {
            assert!(
                is_system_record(kind),
                "unclassified public system type {kind}"
            );
            message.message_type = kind;
            message.content = "RAW_BODY_MUST_NOT_LOOK_LIKE_A_USER_MESSAGE".into();
            let description = system_record_description(&message);
            assert!(!description.is_empty());
            assert!(!description.contains(&message.content));
        }
        for kind in [0, 13, 19, 20, 21, 23, 30, 33, 35, 45, 100] {
            assert!(!is_system_record(kind));
        }
    }
    #[test]
    fn disabled_composer_does_not_consume_modal_enter_or_paste() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, state) = message_fixture();
        app.drafts.insert(
            "200".into(),
            Draft {
                text: "Conservar borrador".into(),
                ..Default::default()
            },
        );
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("composer", "200"))));
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::Paste("Sólo para el modal".into()),
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.disable();
                    app.draw_composer(ui, &state);
                });
            },
        );
        assert_eq!(app.drafts["200"].text, "Conservar borrador");
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn prepared_image_card_stacks_preview_name_size_and_remove_with_reserved_height() {
        let (mut app, ctx, _) = ui_harness();
        let (_, state) = message_fixture();
        let root =
            std::env::temp_dir().join(format!("notdiscord-prepared-card-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("preview.png");
        image::RgbaImage::from_pixel(600, 30, image::Rgba([15u8, 90, 160, 255]))
            .save(&path)
            .unwrap();
        app.drafts.insert(
            "200".into(),
            Draft {
                paths: vec![path.clone()],
                ..Default::default()
            },
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.pictures.texture_path(&path, 160).is_none() {
            app.pictures.poll(&ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut expected = 0.;
        let mut actual = 0.;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700., 500.),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    expected = composer_height(ui, app.drafts.get("200"), app.prefs.chat_size);
                    let top = ui.cursor().top();
                    app.draw_composer(ui, &state);
                    actual = ui.cursor().top() - top;
                });
            },
        );
        let name = painted_text(&output, "preview.png");
        let remove = painted_text(&output, "Quitar");
        let card_rects = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.rect.height() > 40. && rect.rect.width() < 500. => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            actual <= expected + 9.,
            "actual={actual} expected={expected} name={name:?} remove={remove:?} cards={card_rects:?}"
        );
        assert!(name.top() > PREPARED_PREVIEW_HEIGHT);
        assert!(remove.top() > name.bottom());
        assert!(remove.right() - name.left() < PREPARED_CARD_WIDTH);
        assert!(expected > 200. && expected < 300.);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn prepared_image_click_uses_shared_viewer_and_close_preserves_the_entire_draft() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, state) = message_fixture();
        app.prefs.send_typing = false;
        let root = std::env::temp_dir().join(format!("notdiscord-draft-viewer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("local.png");
        image::RgbaImage::from_pixel(1200, 800, image::Rgba([15u8, 90, 160, 255])).save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let other = root.join("document.txt");
        std::fs::write(&other, "Offline attachment").unwrap();
        let reply = MessageReplyPreview { message_id: "250".into(), channel_id: "200".into(), content: "Original reply".into(), ..Default::default() };
        app.drafts.insert("200".into(), Draft { text: "Do not send".into(), paths: vec![path.clone(), other.clone()], reply: Some(reply.clone()), revision: 7, ..Default::default() });
        app.owned_temporary.insert(path.clone());
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.pictures.texture_path(&path, 160).is_none() {
            app.pictures.poll(&ctx);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let composer = |app: &mut NotDiscordUi, events| ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.))), events, ..Default::default()
        }, |ctx| { egui::CentralPanel::default().show(ctx, |ui| app.draw_composer(ui, &state)); });
        let output = composer(&mut app, vec![]);
        let slot = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if (rect.rect.width() - PREPARED_CARD_WIDTH).abs() < 0.1
                && (rect.rect.height() - PREPARED_PREVIEW_HEIGHT).abs() < 0.1 => Some(rect.rect),
            _ => None,
        }).unwrap();
        for pressed in [true, false] { composer(&mut app, pointer(slot.center(), pressed, egui::PointerButton::Primary)); }
        let viewer = app.viewer.as_ref().unwrap();
        assert_eq!(viewer.local_path.as_ref(), Some(&path));
        assert!(viewer.url.is_empty(), "a local attachment never becomes a remote URL");
        let view = |app: &mut NotDiscordUi, events| ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.))), events, ..Default::default()
        }, |ctx| app.draw_viewer(ctx));
        let deadline = Instant::now() + Duration::from_secs(3);
        while app.pictures.texture_path(&path, 1600).is_none() {
            app.pictures.poll(&ctx);
            view(&mut app, vec![]);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        for _ in 0..3 { view(&mut app, vec![]); }
        let output = view(&mut app, vec![]);
        let zoom = painted_text(&output, "+").center();
        for pressed in [true, false] { view(&mut app, pointer(zoom, pressed, egui::PointerButton::Primary)); }
        assert!(app.viewer.as_ref().unwrap().zoom > 1., "local preview keeps the shared zoom controls");
        view(&mut app, vec![key_event(egui::Key::Escape, true)]);
        assert!(app.viewer.is_none());
        let draft = &app.drafts["200"];
        assert_eq!(draft.text, "Do not send");
        assert_eq!(draft.paths, vec![path.clone(), other.clone()]);
        assert_eq!(draft.reply.as_ref(), Some(&reply));
        assert_eq!(draft.revision, 7);
        assert!(app.owned_temporary.contains(&path));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(commands.try_recv().is_err(), "viewing does not send, type or fetch account data");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(other).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn real_reply_is_highlighted_only_for_its_original_author() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (_, mut state) = message_fixture();
        let mut message = notdiscord_core::parsing::message(&serde_json::json!({
            "id":"300", "channel_id":"200", "type":19, "author":{"id":"101", "username":"Sender"},
            "content":"A reply without a textual mention", "message_reference":{"message_id":"250", "channel_id":"200"},
            "referenced_message":{"id":"250", "channel_id":"200", "author":{"id":"100", "username":"Recipient"}, "content":"Original"}
        }));
        let highlighted = |output: &egui::FullOutput| output.shapes.iter().any(|shape|
            matches!(&shape.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == personal_reply_accent() && stroke.width == 3.));
        state.current_user = Some(Person { id: "100".into(), name: "Recipient".into(), ..Default::default() });
        assert!(highlighted(&message_frame(&mut app, &ctx, &message, &state, vec![])));
        state.current_user.as_mut().unwrap().id = "102".into();
        assert!(!highlighted(&message_frame(&mut app, &ctx, &message, &state, vec![])));
        state.current_user.as_mut().unwrap().id = "100".into();
        message.reply_preview.as_mut().unwrap().message_id = "251".into();
        assert!(!highlighted(&message_frame(&mut app, &ctx, &message, &state, vec![])), "an unrelated preview cannot identify the recipient");
        message.reply_to = None;
        message.content = "<@100> Recipient quoted Original".into();
        assert!(!highlighted(&message_frame(&mut app, &ctx, &message, &state, vec![])), "a mention or copied text is not a reply reference");
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn cached_reply_author_is_authoritative_and_unresolved_references_do_not_highlight_everyone() {
        let (_, mut state) = message_fixture();
        state.current_user = Some(Person { id: "100".into(), ..Default::default() });
        let mut message = ChatMessage { id: "300".into(), channel_id: "200".into(), reply_to: Some("250".into()),
            reply_preview: Some(MessageReplyPreview { message_id: "250".into(), channel_id: "200".into(), unavailable: true, ..Default::default() }), ..Default::default() };
        assert!(!replies_to_current_user(&message, &state));
        state.messages.push(ChatMessage { id: "250".into(), channel_id: "200".into(), author: state.current_user.clone().unwrap(), ..Default::default() });
        assert!(replies_to_current_user(&message, &state));
        state.messages[0].author.id = "102".into();
        message.reply_preview.as_mut().unwrap().author = state.current_user.clone();
        message.reply_preview.as_mut().unwrap().unavailable = false;
        assert!(!replies_to_current_user(&message, &state), "known original wins over a stale preview");
        state.messages[0].author.id.clear();
        assert!(replies_to_current_user(&message, &state), "an unknown tombstone author does not override a resolved real preview");
        state.messages[0].author.id = "100".into();
        message.forwarded = true;
        assert!(!replies_to_current_user(&message, &state));
        message.forwarded = false;
        message.channel_id = "201".into();
        assert!(!replies_to_current_user(&message, &state));
    }
    #[test]
    fn narrow_scaled_composer_reserves_two_prepared_cards_and_editor() {
        let (mut app, ctx, _) = ui_harness();
        let (_, mut state) = message_fixture();
        state.voice_connected = true;
        state.messages.clear();
        state.view = "chat".into();
        state.call_videos = vec![CallVideo {
            id: 1,
            active: true,
            label: "Local".into(),
            kind: "camera".into(),
            ..Default::default()
        }];
        ctx.set_zoom_factor(1.5);
        app.drafts.insert(
            "200".into(),
            Draft {
                paths: vec![PathBuf::from("wide.png"), PathBuf::from("tall.png")],
                ..Default::default()
            },
        );
        for height in [360., 400., 480.] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(526., height),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let bottom = ui.max_rect().bottom();
                        app.draw_chat(ui, &state);
                        let editor = ctx
                            .read_response(egui::Id::new(("composer", &state.selected_channel_id)))
                            .unwrap()
                            .rect;
                        assert!(
                            editor.bottom() <= bottom,
                            "editor={editor:?} bottom={bottom}"
                        );
                    });
                },
            );
        }
    }
    #[test]
    fn pinned_history_uses_compact_call_layout_and_links_the_original_while_call_icon_is_shared() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (mut message, state) = message_fixture();
        message.message_type = 3;
        ctx.style_mut(|style| style.animation_time = 0.);
        let call = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let call_rect = ctx
            .data(|data| {
                data.get_temp::<(egui::Rect, bool, bool, bool)>(egui::Id::new(
                    "fixture-message-response",
                ))
            })
            .unwrap()
            .0;
        assert!(call.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Path(path) if path.points.len()==14 && matches!(path.stroke.color,egui::epaint::ColorMode::Solid(color) if color==presence_color("online")))));
        message.message_type = 6;
        message.reply_to = Some("250".into());
        message_frame(&mut app, &ctx, &message, &state, vec![]);
        let pin = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let pin_rect = ctx
            .data(|data| {
                data.get_temp::<(egui::Rect, bool, bool, bool)>(egui::Id::new(
                    "fixture-message-response",
                ))
            })
            .unwrap()
            .0;
        assert!(pin_rect.height() < 55.);
        assert!((pin_rect.height() - call_rect.height()).abs() < 4.);
        assert!(pin.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Path(path) if path.closed && path.points.len()==6)));
        // Wrapped inline labels retain leading space in Galley::size; click the
        // glyph bounds instead of the blank space before the displayed link.
        let original = pin
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "un mensaje" => Some(
                    text.galley
                        .mesh_bounds
                        .translate(text.pos.to_vec2())
                        .center(),
                ),
                _ => None,
            })
            .unwrap();
        let hovered = message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            vec![egui::Event::PointerMoved(original)],
        );
        assert_eq!(
            hovered.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand
        );
        for pressed in [true, false] {
            message_frame(
                &mut app,
                &ctx,
                &message,
                &state,
                pointer(original, pressed, egui::PointerButton::Primary),
            );
        }
        let action = commands.try_recv().unwrap();
        assert_eq!(
            (action.kind.as_str(), action.id.as_str()),
            ("jump-message", "250")
        );
    }
    #[test]
    fn pinned_message_hover_action_has_red_slash_and_unpinned_action_does_not() {
        let (mut app, ctx, _) = ui_harness();
        let (mut message, mut state) = message_fixture();
        state.can_pin_messages = true;
        message.pinned = true;
        ctx.style_mut(|style| style.animation_time = 0.);
        let first = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let position = painted_text(&first, "Fixture body").center();
        let mut output = message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            vec![egui::Event::PointerMoved(position)],
        );
        for _ in 0..2 {
            output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        }
        let slash = |output: &egui::FullOutput| {
            output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::LineSegment{points,stroke} if stroke.color==Color32::from_rgb(242,63,67) && points[1].x>points[0].x && points[1].y<points[0].y))
        };
        assert!(slash(&output));
        message.pinned = false;
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        assert!(!slash(&output));
    }
    fn chat_frame(
        app: &mut NotDiscordUi,
        ctx: &egui::Context,
        state: &AppSnapshot,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700., 500.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                prepare_composer_input(ctx);
                egui::CentralPanel::default().show(ctx, |ui| app.draw_chat(ui, state));
            },
        )
    }
    #[test]
    fn return_navigation_stays_above_long_results_at_each_zoom_and_preserves_chat_position() {
        for view in ["search", "history", "pins"] {
            for zoom in [0.8, 1., 1.6] {
                let (mut app, ctx, mut commands) = ui_harness();
                let (template, mut state) = message_fixture();
                ctx.set_zoom_factor(zoom);
                state.view = view.into();
                state.messages = (0..80).map(|index| ChatMessage {
                    id: (300 + index).to_string(), content: format!("Result {index} alpha beta gamma"),
                    ..template.clone()
                }).collect::<Vec<_>>().into();
                state.pinned_messages = state.messages.iter().cloned().collect();
                app.scroll_bottom = false;
                app.positions.insert("200".into(), ChatPosition {
                    offset: 240., at_bottom: false, distance_to_bottom: None, anchor: "310".into(), anchor_top: -3.,
                });
                let frame = |app: &mut NotDiscordUi, events: Vec<egui::Event>| {
                    ctx.run(egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520., 300.))),
                        events, ..Default::default()
                    }, |ctx| { egui::CentralPanel::default().show(ctx, |ui| app.draw_chat(ui, &state)); })
                };
                let initial = frame(&mut app, vec![]);
                let before = painted_text(&initial, "← Volver al chat");
                let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
                assert!(before.bottom() < viewport.top(), "{view}, zoom={zoom}: header must precede the scroll viewport");
                assert!(ctx.content_rect().contains_rect(before));
                frame(&mut app, vec![egui::Event::PointerMoved(viewport.center()), egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0., -1400.), modifiers: egui::Modifiers::NONE,
                }]);
                let mut scrolled = frame(&mut app, vec![]);
                for _ in 0..15 { scrolled = frame(&mut app, vec![]); }
                let after = painted_text(&scrolled, "← Volver al chat");
                assert!((after.top() - before.top()).abs() < 0.1);
                assert!(ctx.content_rect().contains_rect(after));
                let button = after.center();
                frame(&mut app, pointer(button, true, egui::PointerButton::Primary));
                frame(&mut app, pointer(button, false, egui::PointerButton::Primary));
                let action = commands.try_recv().expect("the fixed back button must remain clickable");
                assert_eq!(action.kind, if view == "pins" { "pins" } else { "return-chat" });
                assert!(commands.try_recv().is_err());
                let position = &app.positions["200"];
                assert_eq!((position.offset, position.at_bottom, position.anchor.as_str(), position.anchor_top), (240., false, "310", -3.));
            }
        }
    }
    #[test]
    fn latest_scroll_is_applied_with_its_page_after_coalesced_delivery() {
        for lag in [0, 1, 3] {
            let (mut app, ctx, _) = ui_harness();
            let (template, mut old) = message_fixture();
            old.view = "chat".into();
            old.messages = (0..45)
                .map(|n| ChatMessage {
                    id: (1000 + n).to_string(),
                    content: format!("Old row {n}"),
                    ..template.clone()
                })
                .collect();
            old.direct_messages = vec![NavItem {
                id: "200".into(),
                last_message_id: "2099".into(),
                ..Default::default()
            }];
            let (slot_tx, slot_rx) = std::sync::mpsc::channel();
            app.updates = slot_rx;
            slot_tx.send(UiUpdate::Snapshot(old.clone())).unwrap();
            app.process_updates(&ctx);
            app.scroll_bottom = false;
            app.pending_scroll = Some(0.);
            for _ in 0..3 {
                chat_frame(&mut app, &ctx, &old, vec![]);
            }
            assert!(!app.positions["200"].at_bottom);
            // The actor can coalesce the page for several UI frames. Rendering
            // the old page in this interval must not consume its scroll intent.
            let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
            slot_tx
                .send(UiUpdate::LatestSnapshot(slot.clone()))
                .unwrap();
            app.process_updates(&ctx);
            for _ in 0..lag {
                chat_frame(&mut app, &ctx, &old, vec![]);
                app.process_updates(&ctx);
            }
            let mut latest = old.clone();
            latest.latest_messages_version = 1;
            latest.messages = (0..45)
                .map(|n| ChatMessage {
                    id: (2055 + n).to_string(),
                    content: format!("Latest row {n}"),
                    ..template.clone()
                })
                .collect();
            *slot.lock().unwrap() = Some(latest.clone());
            app.process_updates(&ctx);
            assert!(app.restore_anchor.is_none());
            for _ in 0..3 {
                chat_frame(&mut app, &ctx, &latest, vec![]);
            }
            assert!(
                app.positions["200"].at_bottom,
                "lag={lag} offset={} pending={:?} restore={:?}",
                app.positions["200"].offset, app.pending_scroll, app.restore_anchor
            );
            assert!(latest_window(&ctx, &latest));
        }
    }
    #[test]
    fn compact_history_controls_jump_to_latest_and_disable_only_at_the_correct_endpoints() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (template, mut state) = message_fixture();
        state.messages = (0..30)
            .map(|index| ChatMessage {
                id: (1000 + index).to_string(),
                content: format!("Fixture history row {index}"),
                timestamp: template.timestamp + chrono::Duration::minutes(index),
                ..template.clone()
            })
            .collect();
        state.direct_messages = vec![NavItem {
            id: "200".into(),
            last_message_id: "1099".into(),
            ..Default::default()
        }];
        state.has_older_messages = false;
        for _ in 0..3 {
            chat_frame(&mut app, &ctx, &state, vec![]);
        }
        let output = chat_frame(&mut app, &ctx, &state, vec![]);
        let older = painted_text(&output, "↑ Mensajes anteriores");
        let latest = painted_text(&output, "Volver al inicio ↓");
        let menu = painted_text(&output, "···");
        assert!((older.center().y - latest.center().y).abs() < 2.);
        assert!((menu.center().y - latest.center().y).abs() < 2.);
        assert!(older.height() < 20.);
        assert!(latest.left() > older.right());
        assert!(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="↓ Ir al presente")));
        for pressed in [true, false] {
            chat_frame(
                &mut app,
                &ctx,
                &state,
                pointer(older.center(), pressed, egui::PointerButton::Primary),
            );
        }
        assert!(commands.try_recv().is_err());
        assert!(app.scroll_bottom); // Bottom of a historical page is not the present.
        for pressed in [true, false] {
            chat_frame(
                &mut app,
                &ctx,
                &state,
                pointer(latest.center(), pressed, egui::PointerButton::Primary),
            );
        }
        assert_eq!(commands.try_recv().unwrap().kind, "latest-messages");
        state.messages = (0..30)
            .map(|index| ChatMessage {
                id: (1070 + index).to_string(),
                content: format!("Fixture present row {index}"),
                timestamp: template.timestamp + chrono::Duration::minutes(70 + index),
                ..template.clone()
            })
            .collect();
        app.state = state.clone();
        app.scroll_bottom = false;
        app.restore_anchor = Some(("1000".into(), -10.));
        app.focus_id = "1005".into();
        app.prepare_latest_scroll();
        assert!(app.restore_anchor.is_none());
        assert!(app.focus_id.is_empty());
        for _ in 0..3 {
            chat_frame(&mut app, &ctx, &state, vec![]);
        }
        assert!(app.positions["200"].at_bottom);
        assert!(latest_window(&ctx, &state));
        let output = chat_frame(&mut app, &ctx, &state, vec![]);
        let latest = painted_text(&output, "Volver al inicio ↓").center();
        for pressed in [true, false] {
            chat_frame(
                &mut app,
                &ctx,
                &state,
                pointer(latest, pressed, egui::PointerButton::Primary),
            );
        }
        assert!(commands.try_recv().is_err());
        state.has_older_messages = true;
        // The loaded historical boundary must actually be in view. Merely
        // having an older server page is not permission to fetch from below.
        let output = chat_frame(&mut app, &ctx, &state, vec![]);
        let older = painted_text(&output, "↑ Mensajes anteriores").center();
        for pressed in [true, false] {
            chat_frame(&mut app, &ctx, &state, pointer(older, pressed, egui::PointerButton::Primary));
        }
        assert!(commands.try_recv().is_err());
        app.scroll_bottom = false;
        app.pending_scroll = Some(0.);
        for _ in 0..3 { chat_frame(&mut app, &ctx, &state, vec![]); }
        assert!(app.positions["200"].offset <= 1.);
        let output = chat_frame(&mut app, &ctx, &state, vec![]);
        let older = painted_text(&output, "↑ Mensajes anteriores").center();
        for pressed in [true, false] {
            chat_frame(
                &mut app,
                &ctx,
                &state,
                pointer(older, pressed, egui::PointerButton::Primary),
            );
        }
        assert_eq!(commands.try_recv().unwrap().kind, "older");
    }
    #[test]
    fn call_box_precedes_the_single_compact_history_toolbar_without_a_menu_only_strip() {
        let (mut app, ctx, _) = ui_harness();
        let (message, mut state) = message_fixture();
        state.messages = vec![message].into();
        state.voice_connecting = true;
        state.call_channel_id = state.selected_channel_id.clone();
        state.call_guild_id = state.selected_guild_id.clone();
        state.call_channel_name = "Fixture call".into();
        state.call_status_label = "Conectando".into();
        let output = chat_frame(&mut app, &ctx, &state, vec![]);
        let title = painted_text(&output, "Llamada · Fixture call");
        let older = painted_text(&output, "↑ Mensajes anteriores");
        let menu = painted_text(&output, "···");
        assert!(
            title.top() < 35.,
            "A menu-only row must not precede the call: {}",
            title.top()
        );
        assert!(older.top() > title.bottom());
        assert!((older.center().y - menu.center().y).abs() < 2.);
    }
    #[test]
    fn call_records_distinguish_direction_and_only_use_known_outcomes() {
        let (mut message, _) = message_fixture();
        message.message_type = 3;
        message.call_started_at = Some(message.timestamp);
        message.call_ended_at = Some(message.timestamp + chrono::Duration::seconds(17));
        let (text, detail, negative) = call_record_description(&message, "100");
        assert!(text.contains("unos segundos"));
        assert!(detail.contains("saliente"));
        assert!(!negative);
        assert!(!text.contains("minuto"));
        // No participant data cannot prove a missed or rejected call.
        assert!(!call_record_description(&message, "other").2);
        message.call_participants_known = true;
        assert!(
            call_record_description(&message, "other")
                .0
                .contains("perdida")
        );
        message.call_outcome = "rejected".into();
        assert!(
            call_record_description(&message, "other")
                .0
                .contains("rechazada")
        );
        message.call_outcome.clear();
        message.call_ended_at = Some(message.timestamp + chrono::Duration::seconds(60));
        assert!(
            call_record_description(&message, "100")
                .0
                .contains("1 minuto.")
        );
        message.call_ended_at = Some(message.timestamp - chrono::Duration::seconds(1));
        assert!(
            call_record_description(&message, "100")
                .1
                .contains("no disponible")
        );
    }
    #[test]
    fn real_mentions_are_blue_and_clickable_but_plain_at_text_is_not() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (mut message, state) = message_fixture();
        message.content = "Hola @Fixture author".into();
        let plain = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let blue = |output: &egui::FullOutput| {
            output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Rect(rect) => rect.fill == Color32::from_rgb(61, 78, 166),
                egui::Shape::Text(text) => text
                    .galley
                    .job
                    .sections
                    .iter()
                    .any(|section| section.format.background == Color32::from_rgb(61, 78, 166)),
                _ => false,
            })
        };
        assert!(!blue(&plain));
        message.content = "Hola <@100>".into();
        message.mentions = vec![message.author.clone()];
        let _ = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let real = message_frame(&mut app, &ctx, &message, &state, vec![]);
        assert!(blue(&real));
        let position = painted_text(&real, "@Fixture author").center();
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(position, true, egui::PointerButton::Primary),
        );
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(position, false, egui::PointerButton::Primary),
        );
        let action = commands.try_recv().unwrap();
        assert_eq!(
            (action.kind.as_str(), action.id.as_str()),
            ("profile", "100")
        );
        assert!(
            !activity(&PresenceActivity {
                kind: 2,
                name: "Spotify".into(),
                ..Default::default()
            })
            .starts_with('♫')
        );
    }
    #[test]
    fn dragging_scrollbar_keeps_rows_and_composer_stable_and_preserves_anchor_on_prepend() {
        let (mut app, ctx, _) = ui_harness();
        ctx.style_mut(|style| style.spacing.scroll = egui::style::ScrollStyle::solid());
        let (template, mut state) = message_fixture();
        state.messages = (0..60)
            .map(|i| ChatMessage {
                id: format!("{}", 1000 + i),
                author: Person {
                    id: format!("author{i}"),
                    ..template.author.clone()
                },
                timestamp: template.timestamp + chrono::Duration::minutes(i),
                content: format!("Row {i} {}", "long fixture ".repeat((i % 5 + 1) as usize)),
                attachments: if i % 7 == 0 {
                    vec![ChatAttachment {
                        name: "image.png".into(),
                        url: "https://example.invalid/fixture.png".into(),
                        width: 480,
                        height: 640,
                        content_type: Some("image/png".into()),
                        ..Default::default()
                    }]
                } else {
                    vec![]
                },
                ..template.clone()
            })
            .collect();
        let mut time = 0.;
        let mut frame =
            |app: &mut NotDiscordUi, state: &AppSnapshot, events: Vec<egui::Event>, width: f32| {
                time += 0.025;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 500.),
                        )),
                        events,
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| app.draw_chat(ui, state));
                    },
                )
            };
        for _ in 0..3 {
            let _ = frame(&mut app, &state, vec![], 700.);
        }
        app.scroll_bottom = false;
        app.pending_scroll = Some(0.);
        for _ in 0..3 {
            let _ = frame(&mut app, &state, vec![], 700.);
        }
        let start = app.positions["200"].offset;
        let point = egui::pos2(689., 120.);
        let _ = frame(
            &mut app,
            &state,
            vec![egui::Event::PointerMoved(point)],
            700.,
        );
        let _ = frame(
            &mut app,
            &state,
            pointer(point, true, egui::PointerButton::Primary),
            700.,
        );
        let end = egui::pos2(689., 280.);
        let _ = frame(&mut app, &state, vec![egui::Event::PointerMoved(end)], 700.);
        let outside = egui::pos2(850., 320.);
        let _ = frame(&mut app, &state, vec![egui::Event::PointerMoved(outside)], 700.);
        assert!(app.pending_scroll.is_none(), "an owned scrollbar drag must not restore an old anchor outside its track");
        let outside_offset = app.positions["200"].offset;
        for _ in 0..3 {
            let _ = frame(&mut app, &state, vec![], 700.);
            assert!((app.positions["200"].offset - outside_offset).abs() < 0.5);
        }
        let _ = frame(&mut app, &state, vec![egui::Event::PointerMoved(end)], 700.);
        let _ = frame(
            &mut app,
            &state,
            pointer(end, false, egui::PointerButton::Primary),
            700.,
        );
        for _ in 0..3 {
            let _ = frame(&mut app, &state, vec![], 700.);
        }
        let offset = app.positions["200"].offset;
        assert!(
            offset > start + 100.,
            "The actual scrollbar must move the viewport: {start} -> {offset}"
        );
        for _ in 0..8 {
            let _ = frame(&mut app, &state, vec![], 700.);
            assert!(
                (app.positions["200"].offset - offset).abs() < 0.5,
                "Scroll must not drift with media rows"
            );
        }
        let position = &app.positions["200"];
        let anchor = position.anchor.clone();
        let top = position.anchor_top;
        let older = (0..8)
            .map(|i| ChatMessage {
                id: format!("{}", 900 + i),
                timestamp: template.timestamp - chrono::Duration::minutes(8 - i),
                content: format!("Earlier {i}"),
                ..template.clone()
            })
            .collect::<Vec<_>>();
        state.messages.splice(0..0, older);
        app.restore_anchor = Some((anchor.clone(), top));
        for _ in 0..3 {
            let _ = frame(&mut app, &state, vec![], 700.);
        }
        assert_eq!(app.positions["200"].anchor, anchor);
        assert!((app.positions["200"].anchor_top - top).abs() < 1.);
        for _ in 0..3 {
            let _ = frame(&mut app, &state, vec![], 520.);
        }
        assert_eq!(app.positions["200"].anchor, anchor);
    }
    #[test]
    fn mention_remains_readable_until_send() {
        let mut d = Draft {
            text: "Hola @Br".into(),
            ..Default::default()
        };
        let p = Person {
            id: "42".into(),
            name: "Bright".into(),
            ..Default::default()
        };
        insert_mention(&mut d, 5, 8, &p);
        assert_eq!(d.text, "Hola @Bright ");
        assert_eq!(outgoing_mentions(&d), "Hola <@42> ");
        let old = d.text.clone();
        d.text = "Hola @Brighter ".into();
        adjust_mentions(&old, &d.text, &mut d.mentions);
        assert!(d.mentions.is_empty());
    }
    #[test]
    fn contextual_token() {
        assert_eq!(mention_token("Hola @B", 7), Some((5, "B".into())));
        assert!(mention_token("mail@x", 6).is_none());
    }
    #[test]
    fn bitmap_shortcut_survives_egui_key_consumption_and_does_not_repeat() {
        assert!(paste_gesture(true, true, true, false, false, false));
        assert!(paste_gesture(true, true, false, true, false, false));
        assert!(paste_gesture(true, true, false, false, true, false));
        assert!(!paste_gesture(true, true, true, true, false, true));
        assert!(!paste_gesture(false, true, true, true, true, false));
        assert!(!paste_gesture(true, false, false, true, false, false));
    }
    fn ui_harness() -> (
        NotDiscordUi,
        egui::Context,
        tokio::sync::mpsc::UnboundedReceiver<UiAction>,
    ) {
        let ctx = egui::Context::default();
        let (commands, receive) = tokio::sync::mpsc::unbounded_channel();
        let (_, updates) = std::sync::mpsc::channel();
        let app = NotDiscordUi::from_context(
            &ctx,
            std::env::temp_dir().join(format!("notdiscord-chat-{}", uuid::Uuid::new_v4())),
            commands,
            updates,
        );
        (app, ctx, receive)
    }
    fn message_fixture() -> (ChatMessage, AppSnapshot) {
        let state = AppSnapshot {
            view: "chat".into(),
            selected_channel_id: "200".into(),
            signed_in: true,
            channel_access: ChannelAccess::private(),
            ..Default::default()
        };
        let message = ChatMessage {
            id: "300".into(),
            channel_id: "200".into(),
            author: Person {
                id: "100".into(),
                name: "Fixture author".into(),
                ..Default::default()
            },
            timestamp: "2026-10-20T10:00:00Z".parse().unwrap(),
            content: "Fixture body".into(),
            ..Default::default()
        };
        (message, state)
    }
    #[test]
    fn sticker_codes_render_from_real_metadata_and_reply_draft_uses_shared_summary() {
        let (mut app, ctx, _) = ui_harness();
        let (_, state) = message_fixture();
        let message = notdiscord_core::parsing::message(&serde_json::json!({
            "id":"300","channel_id":"200","author":{"id":"100","username":"Sticker sender"},
            "sticker_items":[{"id":"301","name":"wave"},{"id":"302","name":"hug"}]
        }));
        let output = message_frame(&mut app, &ctx, &message, &state, Vec::new());
        let wave = painted_text(&output, ":wave:");
        let hug = painted_text(&output, ":hug:");
        assert!(hug.top() >= wave.bottom() - 1.);
        app.reply_to(&message);
        assert_eq!(
            app.drafts["200"].reply.as_ref().unwrap().content,
            ":wave: :hug:"
        );
        let mut forwarded = message.clone();
        forwarded.forwarded = true;
        forwarded.content = "Original accessible content".into();
        app.reply_to(&forwarded);
        assert_eq!(
            app.drafts["200"].reply.as_ref().unwrap().content,
            "Reenvió un mensaje"
        );
        let output = message_frame(&mut app, &ctx, &forwarded, &state, Vec::new());
        let _ = painted_text(&output, "Sticker sender");
        let _ = painted_text(&output, "Original accessible content");
    }
    fn message_frame(
        app: &mut NotDiscordUi,
        ctx: &egui::Context,
        message: &ChatMessage,
        state: &AppSnapshot,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700., 500.),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = app.draw_message(ui, message, state, false);
                    let report = (
                        response.rect,
                        response.hovered(),
                        response.secondary_clicked(),
                        response.context_menu_opened(),
                    );
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new("fixture-message-response"), report)
                    });
                });
            },
        )
    }
    fn painted_text(output: &egui::FullOutput, needle: &str) -> egui::Rect {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    text.galley
                        .job
                        .text
                        .contains(needle)
                        .then(|| egui::Rect::from_min_size(text.pos, text.galley.size()))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Missing painted fixture text: {needle}"))
    }
    fn pointer(pos: egui::Pos2, pressed: bool, button: egui::PointerButton) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn actual_reply_child_receives_click_and_exposes_hand_cursor() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (mut message, state) = message_fixture();
        message.reply_preview = Some(MessageReplyPreview {
            message_id: "250".into(),
            channel_id: "200".into(),
            content: "Reply target fixture".into(),
            ..Default::default()
        });
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let pos = painted_text(&output, "Reply target fixture").center();
        let hover = message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            vec![egui::Event::PointerMoved(pos)],
        );
        assert_eq!(
            hover.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand
        );
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(pos, true, egui::PointerButton::Primary),
        );
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(pos, false, egui::PointerButton::Primary),
        );
        let action = commands.try_recv().expect("reply click must dispatch jump");
        assert_eq!(
            (
                action.kind.as_str(),
                action.channel_id.as_str(),
                action.id.as_str()
            ),
            ("jump-message", "200", "250")
        );
        assert_eq!(app.focus_id, "250");
        assert!(app.focus_scroll_pending);
    }
    #[test]
    fn reply_preview_scales_with_chat_size_and_retains_hover_and_original_message_jump() {
        let mut heights = Vec::new();
        for size in [14., 22.] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (mut message, state) = message_fixture();
            app.prefs.chat_size = size;
            message.reply_preview = Some(MessageReplyPreview {
                message_id: "250".into(),
                channel_id: "200".into(),
                author: Some(Person {
                    id: "120".into(),
                    name: "Original sender".into(),
                    ..Default::default()
                }),
                content: "Archivo adjunto".into(),
                ..Default::default()
            });
            let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
            let preview = painted_text(&output, "Archivo adjunto");
            heights.push(preview.height());
            let text = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text.contains("Archivo adjunto") => {
                        Some(text)
                    }
                    _ => None,
                })
                .unwrap();
            assert!(text.galley.job.sections.iter().all(|section| {
                (section.format.font_id.size - reply_preview_font_size(size)).abs() < 0.01
            }));
            let hover = message_frame(
                &mut app,
                &ctx,
                &message,
                &state,
                vec![egui::Event::PointerMoved(preview.center())],
            );
            assert_eq!(
                hover.platform_output.cursor_icon,
                egui::CursorIcon::PointingHand
            );
            for pressed in [true, false] {
                message_frame(
                    &mut app,
                    &ctx,
                    &message,
                    &state,
                    pointer(preview.center(), pressed, egui::PointerButton::Primary),
                );
            }
            let action = commands.try_recv().unwrap();
            assert_eq!(
                (
                    action.kind.as_str(),
                    action.id.as_str(),
                    action.channel_id.as_str()
                ),
                ("jump-message", "250", "200")
            );
        }
        assert!(
            heights[1] > heights[0] * 1.3,
            "Reply preview must grow with chat text: {heights:?}"
        );
    }
    #[test]
    fn actual_attachment_child_opens_viewer_without_parent_stealing_click() {
        let (mut app, ctx, _) = ui_harness();
        let (mut message, state) = message_fixture();
        message.attachments.push(ChatAttachment {
            name: "Fixture image.png".into(),
            url: "https://example.invalid/not-allowed.png".into(),
            content_type: Some("image/png".into()),
            width: 2,
            height: 1,
            ..Default::default()
        });
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let pos = painted_text(&output, "Imagen no disponible").center();
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(pos, true, egui::PointerButton::Primary),
        );
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(pos, false, egui::PointerButton::Primary),
        );
        assert_eq!(
            app.viewer.as_ref().map(|v| v.url.as_str()),
            Some("https://example.invalid/not-allowed.png")
        );
    }
    #[test]
    fn row_background_keeps_context_menu_after_child_hit_fix() {
        let (mut app, ctx, _) = ui_harness();
        let (message, state) = message_fixture();
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let body = painted_text(&output, "Fixture body");
        let row = ctx
            .data(|d| {
                d.get_temp::<(egui::Rect, bool, bool, bool)>(egui::Id::new(
                    "fixture-message-response",
                ))
            })
            .unwrap();
        let pos = egui::pos2(row.0.right() - 10., body.center().y);
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(pos, true, egui::PointerButton::Secondary),
        );
        let _output = message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(pos, false, egui::PointerButton::Secondary),
        );
        assert!(
            ctx.data(
                |d| d.get_temp::<(egui::Rect, bool, bool, bool)>(egui::Id::new(
                    "fixture-message-response"
                ))
            )
            .unwrap()
            .3
        );
        // Popup first performs an invisible sizing pass; verify its painted menu
        // on the next frame as a real window does after the requested repaint.
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let _ = painted_text(&output, "Responder");
    }
    #[test]
    fn composer_placeholder_uses_muted_opaque_color_in_each_chat_context() {
        for kind in ["dm", "group", "text"] {
            let (mut app, ctx, _) = ui_harness();
            let (_, mut state) = message_fixture();
            state.selected_channel_kind = kind.into();
            if kind == "text" {
                state.selected_guild_id = "900".into();
            }
            app.theme.apply(&ctx, &app.prefs);
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.draw_composer(ui, &state));
            });
            let hint = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == "Escribe un mensaje…" => {
                        Some(text)
                    }
                    _ => None,
                })
                .expect("empty composer must render its hint");
            assert!(
                hint.galley
                    .job
                    .sections
                    .iter()
                    .all(|section| section.format.color == app.theme.muted)
            );
            assert_ne!(app.theme.muted, app.theme.ink);
            assert_eq!(app.theme.muted.a(), 255);
        }
    }
    #[test]
    fn typing_preference_requires_real_nonblank_edits_and_repaint_never_sends() {
        for (enabled, text, expected) in [
            (false, "hello", false),
            (true, "   ", false),
            (true, "hello", true),
        ] {
            let (mut app, ctx, mut actions) = ui_harness();
            let (_, state) = message_fixture();
            app.prefs.send_typing = enabled;
            let draw = |app: &mut NotDiscordUi, events| {
                let _ = ctx.run(
                    egui::RawInput {
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| app.draw_composer(ui, &state));
                    },
                );
            };
            draw(&mut app, vec![]);
            ctx.memory_mut(|memory| {
                memory.request_focus(egui::Id::new(("composer", &state.selected_channel_id)))
            });
            draw(&mut app, vec![egui::Event::Text(text.into())]);
            assert_eq!(app.drafts[&state.selected_channel_id].text, text);
            let action = actions.try_recv().ok();
            assert_eq!(
                action.as_ref().map(|action| action.kind.as_str()),
                expected.then_some("typing")
            );
            draw(&mut app, vec![]);
            assert!(
                actions.try_recv().is_err(),
                "Repainting unchanged text cannot publish typing"
            );
        }
    }
    #[test]
    fn composer_one_row_grows_with_text_and_never_crosses_allocated_width() {
        let (mut app, ctx, _) = ui_harness();
        let (_, state) = message_fixture();
        app.drafts.insert(
            "200".into(),
            Draft {
                text: "short".into(),
                ..Default::default()
            },
        );
        let mut height = 0.;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500., 500.),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    height = composer_height(ui, app.drafts.get("200"), app.prefs.chat_size);
                    app.draw_composer(ui, &state);
                });
            },
        );
        let editor = ctx
            .read_response(egui::Id::new(("composer", &state.selected_channel_id)))
            .unwrap();
        let gif = painted_text(&output, "GIF");
        assert!(
            (gif.center().y - editor.rect.center().y).abs() < 12.,
            "GIF must be beside the editor"
        );
        assert!(gif.right() < 500. && editor.rect.right() < 500.);
        app.drafts.get_mut("200").unwrap().text =
            "first line\nsecond line\nthird line\nfourth line".into();
        let mut expanded = 0.;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500., 500.),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    expanded = composer_height(ui, app.drafts.get("200"), app.prefs.chat_size);
                    app.draw_composer(ui, &state);
                });
            },
        );
        assert!(expanded > height && expanded < 200.);
    }
    #[test]
    fn unavailable_reply_is_not_a_link_and_never_requests_original() {
        let (mut app, ctx, mut commands) = ui_harness();
        let (mut message, state) = message_fixture();
        message.reply_preview = Some(MessageReplyPreview {
            message_id: "250".into(),
            channel_id: "200".into(),
            deleted: true,
            ..Default::default()
        });
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let pos = painted_text(&output, "Mensaje eliminado").center();
        let hover = message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            vec![egui::Event::PointerMoved(pos)],
        );
        assert_ne!(
            hover.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand
        );
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(pos, true, egui::PointerButton::Primary),
        );
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(pos, false, egui::PointerButton::Primary),
        );
        assert!(commands.try_recv().is_err());
        assert!(app.focus_id.is_empty());
    }
    #[test]
    fn grouping_matches_reference_boundaries_and_reply_system_exclusions() {
        let (previous, _) = message_fixture();
        let mut message = previous.clone();
        message.id = "301".into();
        message.timestamp = previous.timestamp + chrono::Duration::minutes(4);
        assert!(can_group(&previous, &message));
        message.timestamp = previous.timestamp + chrono::Duration::minutes(5);
        assert!(!can_group(&previous, &message));
        message.timestamp = previous.timestamp - chrono::Duration::seconds(1);
        assert!(!can_group(&previous, &message));
        message.timestamp = previous.timestamp;
        message.reply_to = Some("old".into());
        assert!(!can_group(&previous, &message));
        message.reply_to = None;
        message.deleted = true;
        assert!(!can_group(&previous, &message));
        message.deleted = false;
        message.message_type = 3;
        assert!(!can_group(&previous, &message));
        message.message_type = 0;
        message.channel_id = "other".into();
        assert!(!can_group(&previous, &message));
        message.channel_id = previous.channel_id.clone();
        message.author.id = "other".into();
        assert!(!can_group(&previous, &message));
    }
    #[test]
    fn continuation_hides_duplicate_avatar_name_and_shows_time_only_on_hover() {
        let (mut app, ctx, _) = ui_harness();
        let (message, mut state) = message_fixture();
        let mut next = message.clone();
        next.id = "301".into();
        next.timestamp += chrono::Duration::minutes(1);
        next.content = "Continuation fixture".into();
        state.messages = vec![message, next.clone()].into();
        let frame = |app: &mut NotDiscordUi, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700., 500.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.draw_chat(ui, &state));
                },
            )
        };
        let _ = frame(&mut app, vec![]);
        let output = frame(&mut app, vec![]);
        let composer_bottom = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Rect(rect) = &shape.shape {
                    (rect.fill == app.theme.surface && rect.rect.width() > 600.)
                        .then_some(rect.rect.bottom())
                } else {
                    None
                }
            })
            .max_by(f32::total_cmp)
            .expect("painted composer frame");
        // CentralPanel has eight points of inset. The composer must reach that
        // bottom edge instead of reserving a second, unused toolbar underneath.
        assert!(
            (492. - composer_bottom).abs() <= 3.,
            "unused strip below composer: {}",
            492. - composer_bottom
        );
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(t) = &shape.shape {
                    Some(t.galley.job.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(text.iter().filter(|&&t| t == "Fixture author").count(), 1);
        assert!(!text.contains(&local_time(&next.timestamp).as_str()));
        let pos = painted_text(&output, "Continuation fixture").center();
        let hover = frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
        let _ = painted_text(&hover, &local_time(&next.timestamp));
    }
    #[test]
    fn hover_toolbar_has_reference_actions_and_reaction_button_remains_clickable() {
        let (mut app, ctx, _) = ui_harness();
        let (message, mut state) = message_fixture();
        state.current_user = Some(message.author.clone());
        state.can_pin_messages = true;
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let pos = painted_text(&output, "Fixture body").center();
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            vec![egui::Event::PointerMoved(pos)],
        );
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        for glyph in ["↩", "↪", "☺", "✎", "×"] {
            let _ = painted_text(&output, glyph);
        }
        let reaction = painted_text(&output, "☺").center();
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(reaction, true, egui::PointerButton::Primary),
        );
        message_frame(
            &mut app,
            &ctx,
            &message,
            &state,
            pointer(reaction, false, egui::PointerButton::Primary),
        );
        assert_eq!(
            app.picker.as_ref().and_then(|p| p.react.as_ref()),
            Some(&("200".into(), "300".into()))
        );
    }
}
