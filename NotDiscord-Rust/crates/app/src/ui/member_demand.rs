//! Report only a member surface that was actually drawn in this frame.
use super::*;

fn key(dialog: bool) -> egui::Id {
    egui::Id::new(if dialog {
        "member-demand-dialog"
    } else {
        "member-demand-sidebar"
    })
}
pub(super) fn begin_frame(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.insert_temp(key(false), false);
        data.insert_temp(key(true), false);
    });
}
pub(super) fn drawn(ctx: &egui::Context, dialog: bool) {
    ctx.data_mut(|data| data.insert_temp(key(dialog), true));
}
impl NotDiscordUi {
    pub(super) fn update_member_demand(&mut self, ctx: &egui::Context, state: &AppSnapshot) {
        let account = state
            .current_user
            .as_ref()
            .map(|user| user.id.as_str())
            .unwrap_or("");
        let visible = ctx.data(|data| {
            data.get_temp::<bool>(key(false)).unwrap_or(false)
                || matches!(self.dialog, Some(Dialog::Members))
                    && data.get_temp::<bool>(key(true)).unwrap_or(false)
        });
        let desired = (self.preview.is_none()
            && state.signed_in
            && state.network_safety == notdiscord_core::safety::SafetyState::Allowed
            && state.view == "chat"
            && notdiscord_core::parsing::valid_id(account)
            && notdiscord_core::parsing::valid_id(&state.selected_guild_id)
            && notdiscord_core::parsing::valid_id(&state.selected_channel_id)
            && state.channel_access.known
            && state.channel_access.can_view
            && state.channel_access.can_view_members
            && !ctx.input(|input| input.viewport().minimized.unwrap_or(false))
            && visible)
            .then(|| {
                (
                    account.to_owned(),
                    state.selected_guild_id.clone(),
                    state.selected_channel_id.clone(),
                )
            });
        let revision_changed = ctx.data_mut(|data| {
            let key = egui::Id::new("member-demand-context-version");
            let previous = data.get_temp::<u64>(key);
            data.insert_temp(key, state.member_context_version);
            previous.is_some_and(|previous| previous != state.member_context_version)
        });
        if desired == self.member_demand && !revision_changed {
            return;
        }
        if let Some((old_account, guild, channel)) = self.member_demand.take() {
            // A replaced session owns its own subscriptions and is already
            // cancelled; never send its old channel to the next account.
            if old_account == account {
                self.send("member-demand", &channel, "hidden", &guild, &channel);
            }
        }
        if let Some((_, guild, channel)) = &desired {
            self.send("member-demand", channel, "visible", guild, channel);
        }
        self.member_demand = desired;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> AppSnapshot {
        AppSnapshot {
            signed_in: true,
            current_user: Some(Person {
                id: "42".into(),
                ..Default::default()
            }),
            view: "chat".into(),
            selected_guild_id: "10".into(),
            selected_channel_id: "20".into(),
            selected_channel_kind: "text".into(),
            channel_access: ChannelAccess::private(),
            ..AppSnapshot::welcome()
        }
    }
    fn frame(app: &mut NotDiscordUi, state: &AppSnapshot, width: f32) {
        let ctx = app.ctx.clone();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 850.),
                )),
                ..Default::default()
            },
            |ctx| {
                begin_frame(ctx);
                app.draw_shell(ctx, state);
                app.draw_dialog(ctx, state);
                app.update_member_demand(ctx, state);
            },
        );
    }
    #[test]
    fn empty_member_panel_requests_once_and_hidden_or_narrow_panels_retire_demand() {
        let (mut app, updates, mut actions) = super::super::tests::harness();
        let state = state();
        frame(&mut app, &state, 1400.);
        let action = actions.try_recv().unwrap();
        assert_eq!(
            (&*action.kind, &*action.text, &*action.id, &*action.extra),
            ("member-demand", "visible", "20", "10")
        );
        frame(&mut app, &state, 1400.);
        assert!(actions.try_recv().is_err(), "Repaint must not resubscribe");
        updates
            .send(UiUpdate::VideoFrames(Default::default()))
            .unwrap();
        app.process_updates(&app.ctx.clone());
        app.prefs.show_server_members = false;
        frame(&mut app, &state, 1400.);
        assert_eq!(actions.try_recv().unwrap().text, "hidden");
        app.prefs.show_server_members = true;
        frame(&mut app, &state, 850.);
        assert!(
            actions.try_recv().is_err(),
            "A remembered preference is not a visible surface"
        );
        app.dialog = Some(Dialog::Members);
        frame(&mut app, &state, 850.);
        assert_eq!(actions.try_recv().unwrap().text, "visible");
        app.dialog = None;
        frame(&mut app, &state, 850.);
        assert_eq!(actions.try_recv().unwrap().text, "hidden");
    }
    #[test]
    fn channel_change_retires_old_demand_and_revocation_or_navigation_stops_new_requests() {
        let (mut app, _, mut actions) = super::super::tests::harness();
        let mut state = state();
        frame(&mut app, &state, 1400.);
        actions.try_recv().unwrap();
        state.selected_channel_id = "21".into();
        frame(&mut app, &state, 1400.);
        let old = actions.try_recv().unwrap();
        let new = actions.try_recv().unwrap();
        assert_eq!((&*old.id, &*old.text), ("20", "hidden"));
        assert_eq!((&*new.id, &*new.text), ("21", "visible"));
        state.channel_access.can_view_members = false;
        frame(&mut app, &state, 1400.);
        assert_eq!(actions.try_recv().unwrap().text, "hidden");
        state.channel_access.can_view_members = true;
        state.view = "friends".into();
        frame(&mut app, &state, 1400.);
        assert!(actions.try_recv().is_err());
        state.view = "chat".into();
        state.network_safety = notdiscord_core::safety::SafetyState::VerificationRequired;
        frame(&mut app, &state, 1400.);
        assert!(actions.try_recv().is_err());
    }
    #[test]
    fn reloading_the_same_channel_renews_the_member_surface_once() {
        let (mut app, _, mut actions) = super::super::tests::harness();
        let mut state = state();
        frame(&mut app, &state, 1400.);
        assert_eq!(actions.try_recv().unwrap().text, "visible");
        state.member_context_version += 1;
        frame(&mut app, &state, 1400.);
        assert_eq!(actions.try_recv().unwrap().text, "hidden");
        assert_eq!(actions.try_recv().unwrap().text, "visible");
        frame(&mut app, &state, 1400.);
        assert!(actions.try_recv().is_err());
    }
}
