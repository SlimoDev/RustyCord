//! One native incoming-call window. Dismissing it is local; only explicit
//! Accept/Reject dispatch commands. Focus is requested once per ringing event.
use super::*;

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("notdiscord-incoming-call")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct NativePass {
        input: egui::RawInput,
        output: Vec<egui::FullOutput>,
        builders: Vec<egui::ViewportBuilder>,
    }
    fn native_renderer(ctx: &egui::Context) -> Arc<Mutex<NativePass>> {
        ctx.set_embed_viewports(false);
        let pass = Arc::new(Mutex::new(NativePass::default()));
        let shared = pass.clone();
        egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
            let mut input = {
                let mut data = shared.lock().unwrap();
                data.builders.push(viewport.builder.clone());
                std::mem::take(&mut data.input)
            };
            input.viewport_id = viewport.ids.this;
            input.screen_rect = Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(460., 420.),
            ));
            input.viewports.entry(viewport.ids.this).or_default().parent =
                Some(viewport.ids.parent);
            let output = ctx.run(input, |ctx| (viewport.viewport_ui_cb)(ctx));
            shared.lock().unwrap().output.push(output);
        });
        pass
    }
    fn state() -> AppSnapshot {
        AppSnapshot {
            signed_in: true,
            demo: true,
            self_presence: "online".into(),
            current_user: Some(Person {
                id: "1".into(),
                name: "Synthetic self".into(),
                ..Default::default()
            }),
            incoming_calls: vec![IncomingCall {
                channel_id: "50".into(),
                instance_id: 1,
                channel_name: "Synthetic group".into(),
                kind: "group".into(),
                ringing: true,
                caller: Some(Person {
                    id: "42".into(),
                    name: "Synthetic caller".into(),
                    ..Default::default()
                }),
                participants: vec![Person {
                    id: "42".into(),
                    name: "Synthetic caller".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    fn frame(
        app: &mut NotDiscordUi,
        state: &AppSnapshot,
        pass: &Arc<Mutex<NativePass>>,
        events: Vec<egui::Event>,
        close: bool,
        minimized: bool,
    ) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        {
            let mut data = pass.lock().unwrap();
            data.builders.clear();
            data.output.clear();
            data.input.events = events;
            if close {
                data.input
                    .viewports
                    .entry(viewport_id())
                    .or_default()
                    .events
                    .push(egui::ViewportEvent::Close);
            }
        }
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 700.),
            )),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .minimized = Some(minimized);
        ctx.run(input, |ctx| app.draw_incoming_popup(ctx, state))
    }
    fn focuses(output: &egui::FullOutput, pass: &Arc<Mutex<NativePass>>) -> usize {
        let count = |out: &egui::FullOutput| {
            out.viewport_output
                .iter()
                .filter(|(id, _)| **id == viewport_id())
                .flat_map(|(_, v)| &v.commands)
                .filter(|c| matches!(c, egui::ViewportCommand::Focus))
                .count()
        };
        count(output) + pass.lock().unwrap().output.iter().map(count).sum::<usize>()
    }
    fn pointer(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn native_popup_requires_explicit_demo_permission_and_respects_dnd_and_mute() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        let pass = native_renderer(&app.ctx);
        let mut state = state();
        frame(&mut app, &state, &pass, vec![], false, false);
        assert!(pass.lock().unwrap().builders.is_empty());
        app.enable_synthetic_incoming_popup();
        state.incoming_calls[0].instance_id = 2;
        state.self_presence = "dnd".into();
        frame(&mut app, &state, &pass, vec![], false, false);
        assert!(pass.lock().unwrap().builders.is_empty());
        state.self_presence = "online".into();
        frame(&mut app, &state, &pass, vec![], false, false);
        assert!(
            pass.lock().unwrap().builders.is_empty(),
            "Leaving DND must not foreground an old event"
        );
        state.incoming_calls[0].instance_id = 3;
        state.incoming_calls[0].muted = true;
        frame(&mut app, &state, &pass, vec![], false, false);
        assert!(pass.lock().unwrap().builders.is_empty());
        state.incoming_calls[0].instance_id = 4;
        state.incoming_calls[0].muted = false;
        let output = frame(&mut app, &state, &pass, vec![], false, true);
        assert_eq!(focuses(&output, &pass), 1);
        let builder = pass.lock().unwrap().builders[0].clone();
        assert_eq!(builder.window_level, Some(egui::WindowLevel::AlwaysOnTop));
        assert_eq!(builder.active, Some(true));
        assert!(
            builder.position.is_none(),
            "A minimized root must not position the popup at its iconic bounds"
        );
        let output = frame(&mut app, &state, &pass, vec![], false, true);
        assert_eq!(focuses(&output, &pass), 0);
        assert_eq!(pass.lock().unwrap().builders[0].active, Some(true));
        assert!(commands.try_recv().is_err());
    }
    #[test]
    fn native_popup_buttons_dispatch_only_explicit_channel_actions_and_ignore_rearms() {
        for (choice, command) in [
            (Choice::Accept, Some("join-call")),
            (Choice::Reject, Some("reject-call")),
            (Choice::Ignore, None),
        ] {
            let (mut app, _, mut commands) = super::super::tests::harness();
            app.enable_synthetic_incoming_popup();
            let pass = native_renderer(&app.ctx);
            let mut state = state();
            for _ in 0..2 {
                frame(&mut app, &state, &pass, vec![], false, false);
            }
            let (rect, clip, id) =
                app.ctx
                    .data(|data| {
                        data.get_temp::<(egui::Rect, egui::Rect, egui::ViewportId)>(egui::Id::new(
                            ("incoming-popup-button", choice.label()),
                        ))
                    })
                    .unwrap();
            assert_eq!(id, viewport_id());
            assert!(
                clip.contains_rect(rect),
                "{} clipped: {rect:?} {clip:?}",
                choice.label()
            );
            frame(
                &mut app,
                &state,
                &pass,
                pointer(rect.center(), true),
                false,
                false,
            );
            frame(
                &mut app,
                &state,
                &pass,
                pointer(rect.center(), false),
                false,
                false,
            );
            if let Some(expected) = command {
                let action = commands.try_recv().unwrap();
                assert_eq!(
                    (
                        action.kind.as_str(),
                        action.id.as_str(),
                        action.channel_id.as_str()
                    ),
                    (expected, "50", "50")
                );
                assert_eq!(
                    serde_json::from_str::<Value>(&action.extra).unwrap()["instanceId"],
                    1
                );
                assert!(app.incoming_popup.pending.is_some());
                frame(
                    &mut app,
                    &state,
                    &pass,
                    pointer(rect.center(), true),
                    false,
                    false,
                );
                frame(
                    &mut app,
                    &state,
                    &pass,
                    pointer(rect.center(), false),
                    false,
                    false,
                );
                assert!(commands.try_recv().is_err(), "Pending action repeated");
                state.incoming_calls[0].ringing = false;
                frame(&mut app, &state, &pass, vec![], false, false);
                assert!(app.incoming_popup.pending.is_none());
                assert!(app.incoming_popup.active.is_none());
            } else {
                assert!(
                    commands.try_recv().is_err(),
                    "Ignore emitted a network command"
                );
                assert!(
                    state.incoming_calls[0].ringing,
                    "Ignore changed backend ringing"
                );
                for _ in 0..3 {
                    frame(&mut app, &state, &pass, vec![], false, false);
                    assert!(pass.lock().unwrap().builders.is_empty());
                }
                state.incoming_calls[0].instance_id = 2;
                let output = frame(&mut app, &state, &pass, vec![], false, false);
                assert_eq!(
                    focuses(&output, &pass),
                    1,
                    "A new call in the same channel did not rearm"
                );
            }
        }
    }
    #[test]
    fn closing_native_popup_or_escape_ignores_without_rejecting_and_clears_pending() {
        for native_close in [true, false] {
            let (mut app, _, mut commands) = super::super::tests::harness();
            app.enable_synthetic_incoming_popup();
            let pass = native_renderer(&app.ctx);
            let mut state = state();
            frame(&mut app, &state, &pass, vec![], false, false);
            app.incoming_popup.pending = Some(Pending {
                key: RingKey::of(&state.incoming_calls[0]),
                choice: Choice::Accept,
                since: Instant::now(),
            });
            let events = if native_close {
                vec![]
            } else {
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]
            };
            frame(&mut app, &state, &pass, events, native_close, false);
            assert!(commands.try_recv().is_err());
            assert!(app.incoming_popup.active.is_none());
            assert!(app.incoming_popup.pending.is_none());
            assert!(state.incoming_calls[0].ringing);
            for _ in 0..3 {
                frame(&mut app, &state, &pass, vec![], false, false);
            }
            assert!(pass.lock().unwrap().builders.is_empty());
            state.incoming_calls.clear();
            frame(&mut app, &state, &pass, vec![], false, false);
            state.incoming_calls = super::tests::state().incoming_calls;
            let output = frame(&mut app, &state, &pass, vec![], false, false);
            assert_eq!(focuses(&output, &pass), 1);
        }
    }
    #[test]
    fn panel_pending_is_shared_and_joining_from_elsewhere_closes_popup_locally() {
        let (mut app, _, mut commands) = super::super::tests::harness();
        app.enable_synthetic_incoming_popup();
        let pass = native_renderer(&app.ctx);
        let mut state = state();
        frame(&mut app, &state, &pass, vec![], false, false);
        app.ctx.data_mut(|data| {
            data.insert_temp(
                egui::Id::new("incoming-call-pending"),
                ("50".to_owned(), Instant::now()),
            )
        });
        frame(&mut app, &state, &pass, vec![], false, false);
        let (rect, _, _) = app
            .ctx
            .data(|data| {
                data.get_temp::<(egui::Rect, egui::Rect, egui::ViewportId)>(egui::Id::new((
                    "incoming-popup-button",
                    "Aceptar",
                )))
            })
            .unwrap();
        frame(
            &mut app,
            &state,
            &pass,
            pointer(rect.center(), true),
            false,
            false,
        );
        frame(
            &mut app,
            &state,
            &pass,
            pointer(rect.center(), false),
            false,
            false,
        );
        assert!(
            commands.try_recv().is_err(),
            "A panel request was duplicated by the popup"
        );
        state.call_channel_id = "50".into();
        state.voice_connecting = true;
        let output = frame(&mut app, &state, &pass, vec![], false, false);
        assert_eq!(focuses(&output, &pass), 0);
        assert!(app.incoming_popup.active.is_none());
        assert!(app.incoming_popup.pending.is_none());
        assert!(
            state.incoming_calls[0].ringing,
            "Closing the local popup changed backend ringing"
        );
        assert!(
            commands.try_recv().is_err(),
            "Closing on join emitted reject-call"
        );
    }
}
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RingKey {
    channel: String,
    instance: u64,
    legacy_start: i64,
    legacy_nanos: u32,
}
impl RingKey {
    fn of(call: &IncomingCall) -> Self {
        Self {
            channel: call.channel_id.clone(),
            instance: call.instance_id,
            legacy_start: if call.instance_id == 0 {
                call.started_at.timestamp()
            } else {
                0
            },
            legacy_nanos: if call.instance_id == 0 {
                call.started_at.timestamp_subsec_nanos()
            } else {
                0
            },
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Choice {
    Accept,
    Reject,
    Ignore,
}
impl Choice {
    fn label(self) -> &'static str {
        match self {
            Self::Accept => "Aceptar",
            Self::Reject => "Rechazar",
            Self::Ignore => "Ignorar",
        }
    }
    fn command(self) -> Option<&'static str> {
        match self {
            Self::Accept => Some("join-call"),
            Self::Reject => Some("reject-call"),
            Self::Ignore => None,
        }
    }
}
struct Pending {
    key: RingKey,
    choice: Choice,
    since: Instant,
}
#[derive(Default)]
pub(super) struct IncomingPopup {
    account: String,
    seen: HashSet<RingKey>,
    focus_ready: HashSet<RingKey>,
    suppressed: HashSet<RingKey>,
    active: Option<RingKey>,
    pending: Option<Pending>,
    allow_demo: bool,
    wait_closed: bool,
    active_on_create: bool,
    initial_position: Option<egui::Pos2>,
}
impl IncomingPopup {
    pub(super) fn audible_call<'a>(&self, state: &'a AppSnapshot) -> Option<&'a IncomingCall> {
        state.incoming_calls.iter().find(|call| call.ringing && !muted(state, call)
            && !self.suppressed.contains(&RingKey::of(call))
            && self.pending(&RingKey::of(call)).is_none()
            && !(state.call_channel_id == call.channel_id && (state.voice_connecting || state.voice_connected)))
    }
    fn reset(&mut self, account: String) {
        self.account = account;
        self.seen.clear();
        self.focus_ready.clear();
        self.suppressed.clear();
        self.active = None;
        self.pending = None;
        self.active_on_create = false;
        self.initial_position = None;
    }
    fn pending(&self, key: &RingKey) -> Option<Choice> {
        self.pending
            .as_ref()
            .filter(|p| &p.key == key && p.since.elapsed() < Duration::from_secs(30))
            .map(|p| p.choice)
    }
}
fn muted(state: &AppSnapshot, call: &IncomingCall) -> bool {
    call.muted
        || state
            .direct_messages
            .iter()
            .chain(&state.channels)
            .any(|item| item.id == call.channel_id && item.is_muted)
}
impl NotDiscordUi {
    /// Explicitly permits native call windows for synthetic Demo fixtures.
    /// Normal previews/fixtures cannot create or focus this window by default.
    #[doc(hidden)]
    pub fn enable_synthetic_incoming_popup(&mut self) {
        self.incoming_popup.allow_demo = true;
    }
    fn close_incoming_popup(&mut self, ctx: &egui::Context) {
        self.incoming_popup.active = None;
        self.incoming_popup.pending = None;
        self.incoming_popup.active_on_create = false;
        self.incoming_popup.initial_position = None;
        self.incoming_popup.wait_closed = true;
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Close);
        ctx.request_repaint();
    }
    pub(super) fn draw_incoming_popup(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        let account = state
            .current_user
            .as_ref()
            .map(|p| p.id.clone())
            .unwrap_or_else(|| {
                if state.signed_in {
                    "signed-in".into()
                } else {
                    String::new()
                }
            });
        if account != self.incoming_popup.account {
            let was_open = self.incoming_popup.active.is_some();
            self.incoming_popup.reset(account);
            if was_open {
                self.close_incoming_popup(ctx);
            }
        }
        let calls: Vec<_> = state
            .incoming_calls
            .iter()
            .filter(|call| call.ringing && !call.channel_id.is_empty())
            .take(32)
            .collect();
        let live: HashSet<_> = calls.iter().map(|call| RingKey::of(call)).collect();
        self.incoming_popup.seen.retain(|key| live.contains(key));
        self.incoming_popup
            .focus_ready
            .retain(|key| live.contains(key));
        self.incoming_popup
            .suppressed
            .retain(|key| live.contains(key));
        let enabled = state.signed_in
            && state.self_presence != "dnd"
            && ((!state.demo && self.preview.is_none())
                || self.incoming_popup.allow_demo && state.demo);
        for call in &calls {
            let key = RingKey::of(call);
            let already_joining = state.call_channel_id == call.channel_id
                && (state.voice_connecting || state.voice_connected);
            if !enabled || muted(state, call) || already_joining {
                self.incoming_popup.suppressed.insert(key.clone());
                self.incoming_popup.focus_ready.remove(&key);
            } else if !self.incoming_popup.seen.contains(&key) {
                self.incoming_popup.focus_ready.insert(key.clone());
            }
            self.incoming_popup.seen.insert(key);
        }
        if self.incoming_popup.wait_closed {
            self.incoming_popup.wait_closed = false;
            ctx.request_repaint();
            return;
        }
        if self.incoming_popup.active.as_ref().is_some_and(|key| {
            !enabled || !live.contains(key) || self.incoming_popup.suppressed.contains(key)
        }) {
            self.close_incoming_popup(ctx);
            return;
        }
        if !enabled {
            return;
        }
        let call = self
            .incoming_popup
            .active
            .as_ref()
            .and_then(|key| calls.iter().find(|call| RingKey::of(call) == *key).copied())
            .or_else(|| {
                calls
                    .iter()
                    .copied()
                    .find(|call| !self.incoming_popup.suppressed.contains(&RingKey::of(call)))
            });
        let Some(call) = call.cloned() else {
            return;
        };
        let key = RingKey::of(&call);
        let mut focus_now = self.incoming_popup.focus_ready.remove(&key);
        if self.incoming_popup.active.as_ref() != Some(&key) {
            self.incoming_popup.active = Some(key.clone());
            self.incoming_popup.active_on_create = focus_now;
            self.incoming_popup.pending = None;
            self.incoming_popup.initial_position = ctx.input(|input| {
                (!input.viewport().minimized.unwrap_or(false))
                    .then_some(input.viewport().outer_rect.or(input.viewport().inner_rect))
                    .flatten()
                    .filter(|rect| rect.is_finite() && rect.is_positive())
                    .map(|rect| rect.center() - egui::vec2(230., 210.))
            });
        }
        if self
            .incoming_popup
            .pending
            .as_ref()
            .is_some_and(|p| p.since.elapsed() >= Duration::from_secs(30))
        {
            self.incoming_popup.pending = None;
        }
        let pending = self.incoming_popup.pending(&key).or_else(|| {
            ctx.data(|data| {
                data.get_temp::<(String, Instant)>(egui::Id::new("incoming-call-pending"))
            })
            .filter(|(channel, since)| {
                channel == &call.channel_id && since.elapsed() < Duration::from_secs(30)
            })
            .map(|_| Choice::Accept)
        });
        let mut builder = egui::ViewportBuilder::default()
            .with_title("NotDiscord · Llamada entrante")
            .with_inner_size([460., 420.])
            .with_min_inner_size([330., 300.])
            .with_clamp_size_to_monitor_size(true)
            .with_resizable(true)
            .with_taskbar(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            // Changing this builder property every frame recreates a winit
            // window. Retain the initial value for this window's lifetime.
            .with_active(self.incoming_popup.active_on_create);
        if let Some(position) = self.incoming_popup.initial_position {
            // Keep initial placement stable across layout passes. egui emits
            // a position command only when this builder value changes.
            builder = builder.with_position(position);
        }
        let mut choice = None;
        ctx.show_viewport_immediate(viewport_id(), builder, |popup_ctx, class| {
            if class != egui::ViewportClass::Embedded { super::autoscroll::begin_frame(popup_ctx); }
            if focus_now && class != egui::ViewportClass::Embedded {
                popup_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                focus_now = false;
            }
            if popup_ctx.input(|input| {
                input.viewport().close_requested() || input.key_pressed(egui::Key::Escape)
            }) {
                choice.get_or_insert(Choice::Ignore);
                if class != egui::ViewportClass::Embedded { super::autoscroll::end_frame(popup_ctx); }
                return;
            }
            if class == egui::ViewportClass::Embedded {
                let mut open = true;
                super::dialogs::centered_dialog(
                    popup_ctx,
                    "Llamada entrante",
                    "incoming-call-embedded",
                    egui::vec2(460., 420.),
                )
                .open(&mut open)
                .vscroll(false)
                .show(popup_ctx, |ui| {
                    ui.allocate_ui(
                        egui::vec2(
                            ui.available_width(),
                            (ui.available_height() - 65.).max(100.),
                        ),
                        |ui| self.incoming_popup_body(ui, &call),
                    );
                    choice = self.incoming_popup_actions(ui, pending).or(choice);
                });
                if !open {
                    choice.get_or_insert(Choice::Ignore);
                }
            } else {
                egui::TopBottomPanel::bottom("incoming-popup-actions")
                    .frame(egui::Frame::new().fill(self.theme.panel).inner_margin(12.))
                    .show(popup_ctx, |ui| {
                        // Native viewports may perform another egui layout
                        // pass. Never erase a click detected by an earlier pass.
                        choice = self.incoming_popup_actions(ui, pending).or(choice);
                    });
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(self.theme.panel).inner_margin(16.))
                    .show(popup_ctx, |ui| self.incoming_popup_body(ui, &call));
            }
            if pending.is_some() {
                popup_ctx.request_repaint_after(Duration::from_millis(100));
            }
            if class != egui::ViewportClass::Embedded { super::autoscroll::end_frame(popup_ctx); }
        });
        if let Some(choice) = choice {
            if choice == Choice::Ignore {
                self.incoming_popup.suppressed.insert(key.clone());
                self.incoming_popup.focus_ready.remove(&key);
                self.close_incoming_popup(ctx);
            } else if pending.is_none() {
                let now = Instant::now();
                self.incoming_popup.pending = Some(Pending {
                    key,
                    choice,
                    since: now,
                });
                ctx.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("incoming-call-pending"),
                        (call.channel_id.clone(), now),
                    )
                });
                self.send(
                    choice.command().unwrap(),
                    &call.channel_id,
                    "",
                    serde_json::json!({"instanceId":call.instance_id}).to_string(),
                    &call.channel_id,
                );
                ctx.request_repaint();
            }
        }
    }
    fn incoming_popup_body(&mut self, ui: &mut egui::Ui, call: &IncomingCall) {
        let caller = call
            .caller
            .clone()
            .or_else(|| call.participants.first().cloned())
            .unwrap_or(Person {
                name: call.channel_name.clone(),
                avatar_url: call.icon_url.clone(),
                ..Default::default()
            });
        ui.vertical_centered(|ui| {
            ui.add_space(6.);
            self.avatar(ui, &caller, "", 64.);
            ui.heading("Llamada entrante");
            ui.add(egui::Label::new(RichText::new(&caller.name).strong()).wrap());
            if !call.channel_name.is_empty() && call.channel_name != caller.name {
                ui.add(
                    egui::Label::new(RichText::new(&call.channel_name).color(self.theme.muted))
                        .wrap(),
                );
            }
            ui.small(if call.kind == "group" {
                "Llamada de grupo"
            } else {
                "Mensaje directo"
            });
        });
        ui.separator();
        ui.label(format!("Participantes · {}", call.participants.len()));
        theme::vertical_scroll(ui)
            .id_salt("incoming-popup-participants")
            .show(ui, |ui| {
                for person in call.participants.iter().take(25) {
                    ui.horizontal(|ui| {
                        self.avatar(ui, person, "", 28.);
                        ui.add(egui::Label::new(&person.name).truncate())
                            .on_hover_text(&person.name);
                    });
                }
            });
    }
    fn incoming_popup_actions(
        &mut self,
        ui: &mut egui::Ui,
        pending: Option<Choice>,
    ) -> Option<Choice> {
        if let Some(choice) = pending {
            ui.small(if choice == Choice::Accept {
                "Procesando solicitud…"
            } else {
                "Rechazando…"
            });
        }
        let mut selected = None;
        ui.horizontal_wrapped(|ui| {
            let width = ((ui.available_width() - ui.spacing().item_spacing.x * 2.) / 3.).max(78.);
            for choice in [Choice::Ignore, Choice::Reject, Choice::Accept] {
                let mut button = egui::Button::new(choice.label()).min_size(egui::vec2(width, 36.));
                if choice == Choice::Accept {
                    button = button.fill(self.theme.accent_surface);
                }
                let response =
                    ui.add_enabled(pending.is_none() || choice == Choice::Ignore, button);
                #[cfg(test)]
                {
                    let viewport = ui.ctx().viewport_id();
                    let clip = ui.clip_rect();
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            egui::Id::new(("incoming-popup-button", choice.label())),
                            (response.rect, clip, viewport),
                        )
                    });
                }
                if response.clicked() {
                    selected = Some(choice);
                }
            }
        });
        selected
    }
}
