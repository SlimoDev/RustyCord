use super::*;

fn fixture() -> (NotDiscordUi, egui::Context, tokio::sync::mpsc::UnboundedReceiver<UiAction>, AppSnapshot, Person) {
    let (app, _, commands) = super::super::tests::harness();
    let ctx = app.ctx.clone();
    let me = Person { id: "100".into(), name: "Group leader".into(), ..Default::default() };
    let target = Person { id: "300".into(), name: "Selected member".into(), ..Default::default() };
    let state = AppSnapshot { signed_in: true, view: "chat".into(), current_user: Some(me.clone()),
        selected_channel_kind: "group".into(), selected_channel_id: "200".into(), channel_name: "Synthetic group".into(),
        group_owner_id: me.id.clone(), member_context_version: 7,
        group_members: vec![ChannelMember { user: me, is_owner: true, ..Default::default() },
            ChannelMember { user: target.clone(), ..Default::default() }], ..Default::default() };
    (app, ctx, commands, state, target)
}
fn frame(app: &mut NotDiscordUi, ctx: &egui::Context, state: &AppSnapshot, target: &Person, dialog: bool, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 700.))),
        events, ..Default::default() }, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| { if !dialog { app.people_menu(ui, target, state); } });
        if dialog { app.draw_dialog(ctx, state); }
    })
}
fn text(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == label => Some(egui::Rect::from_min_size(text.pos, text.galley.size())),
        _ => None,
    })
}
fn pointer(point: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![egui::Event::PointerMoved(point), egui::Event::PointerButton { pos: point,
        button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]
}

#[test]
fn only_the_current_group_owner_can_remove_an_existing_other_member() {
    let (_, _, _, state, target) = fixture();
    assert!(can_remove(&state, &target.id));
    assert!(!can_remove(&state, "100"));
    assert!(!can_remove(&state, "999"));
    for case in 0..7 {
        let mut denied = state.clone();
        match case {
            0 => denied.group_owner_id = "300".into(),
            1 => denied.selected_guild_id = "400".into(),
            2 => denied.selected_channel_kind = "dm".into(),
            3 => denied.signed_in = false,
            4 => denied.demo = true,
            5 => denied.current_user = None,
            _ => denied.network_safety = notdiscord_core::safety::SafetyState::VerificationRequired,
        }
        assert!(!can_remove(&denied, &target.id), "denied scope {case}");
    }
}

#[test]
fn removing_from_the_member_menu_only_sends_after_confirmation() {
    let (mut app, ctx, mut commands, state, target) = fixture();
    for _ in 0..3 { frame(&mut app, &ctx, &state, &target, false, vec![]); }
    let output = frame(&mut app, &ctx, &state, &target, false, vec![]);
    let point = text(&output, "Eliminar del grupo").unwrap().center();
    for pressed in [true, false] { frame(&mut app, &ctx, &state, &target, false, pointer(point, pressed)); }
    let Some(Dialog::Confirm { description, action, .. }) = &app.dialog else { panic!("Removal needs confirmation"); };
    assert!(description.contains("Selected member") && description.contains("Synthetic group"));
    assert_eq!((action.id.as_str(), action.channel_id.as_str(), action.extra.as_str()), ("300", "200", "7"));
    assert!(commands.try_recv().is_err());
    for _ in 0..3 { frame(&mut app, &ctx, &state, &target, true, vec![]); }
    let output = frame(&mut app, &ctx, &state, &target, true, vec![]);
    let confirm = text(&output, "Confirmar").unwrap().center();
    for pressed in [true, false] { frame(&mut app, &ctx, &state, &target, true, pointer(confirm, pressed)); }
    let sent = commands.try_recv().unwrap();
    assert_eq!((sent.kind.as_str(), sent.id.as_str(), sent.channel_id.as_str(), sent.extra.as_str()),
        ("remove-group-member", "300", "200", "7"));
    assert!(app.dialog.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn ownership_membership_navigation_and_pending_changes_invalidate_confirmation() {
    let (mut app, ctx, mut commands, state, target) = fixture();
    let action = UiAction { kind: "remove-group-member".into(), id: target.id.clone(),
        channel_id: "200".into(), extra: "7".into(), ..Default::default() };
    assert!(action_error(&action, &state).is_none());
    for case in 0..5 {
        let mut changed = state.clone();
        match case {
            0 => changed.group_owner_id = target.id.clone(),
            1 => changed.member_context_version += 1,
            2 => changed.selected_channel_id = "201".into(),
            3 => changed.group_members.retain(|member| member.user.id != target.id),
            _ => changed.group_member_removal_pending_ids.push(target.id.clone()),
        }
        assert!(action_error(&action, &changed).is_some());
        app.confirm("Eliminar del grupo", "Synthetic removal confirmation", action.clone());
        for _ in 0..3 { frame(&mut app, &ctx, &changed, &target, true, vec![]); }
        let output = frame(&mut app, &ctx, &changed, &target, true, vec![]);
        let point = text(&output, "Confirmar").unwrap().center();
        for pressed in [true, false] { frame(&mut app, &ctx, &changed, &target, true, pointer(point, pressed)); }
        assert!(commands.try_recv().is_err());
        assert!(app.dialog.is_some());
        let output = frame(&mut app, &ctx, &changed, &target, true, vec![]);
        let point = text(&output, "Cancelar").unwrap().center();
        for pressed in [true, false] { frame(&mut app, &ctx, &changed, &target, true, pointer(point, pressed)); }
        assert!(app.dialog.is_none());
    }
}

#[test]
fn a_non_owner_never_sees_the_removal_option_in_the_member_menu() {
    let (mut app, ctx, mut commands, mut state, target) = fixture();
    state.group_owner_id = "300".into();
    for _ in 0..3 {
        let output = frame(&mut app, &ctx, &state, &target, false, vec![]);
        assert!(text(&output, "Eliminar del grupo").is_none());
    }
    assert!(commands.try_recv().is_err());
}
