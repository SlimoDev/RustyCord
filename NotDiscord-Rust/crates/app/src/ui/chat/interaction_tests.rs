fn interaction_text_cursor(output: &egui::FullOutput, needle: &str, index: usize) -> egui::Pos2 {
    output.shapes.iter().find_map(|shape| {
        if let egui::Shape::Text(text) = &shape.shape {
            (text.galley.job.text == needle).then(||
                text.pos + text.galley.pos_from_cursor(egui::text::CCursor::new(index)).center().to_vec2())
        } else { None }
    }).unwrap_or_else(|| panic!("Missing cursor fixture text: {needle}"))
}
#[test]
fn url_galleys_copy_exactly_between_plain_runs_and_only_a_click_opens_them() {
    let (mut app, ctx, mut commands) = ui_harness();
    let (mut message, mut state) = message_fixture();
    message.content = "antes https://example.test/path después".into();
    state.messages = vec![message].into();
    for _ in 0..3 { chat_frame(&mut app, &ctx, &state, vec![]); }
    let first = chat_frame(&mut app, &ctx, &state, vec![]);
    let before = painted_text(&first, "antes ");
    let url = painted_text(&first, "https://example.test/path");
    let after = painted_text(&first, " después");
    let hover = chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(url.center())]);
    assert_eq!(hover.platform_output.cursor_icon, egui::CursorIcon::PointingHand);
    let start = before.left_center() + egui::vec2(0.1, 0.);
    let end = after.right_center() + egui::vec2(0.1, 0.);
    let mut outputs = vec![chat_frame(&mut app, &ctx, &state, pointer(start, true, egui::PointerButton::Primary))];
    outputs.push(chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(url.center())]));
    outputs.push(chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(end)]));
    outputs.push(chat_frame(&mut app, &ctx, &state, pointer(end, false, egui::PointerButton::Primary)));
    let copied = chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy]);
    assert_eq!(copied_history(&copied), "antes https://example.test/path después");
    outputs.push(copied);
    assert!(outputs.iter().all(|output| !output.platform_output.commands.iter()
        .any(|command| matches!(command, egui::OutputCommand::OpenUrl(_)))));
    assert!(commands.try_recv().is_err());
    // Starting directly on the URL also drags text rather than opening it.
    // Horizontal wrapping galleys include indentation for preceding widgets.
    // Hit the URL's first glyph rather than the bounding galley's left edge.
    let start = interaction_text_cursor(&first, "https://example.test/path", 0) + egui::vec2(0.1, 0.);
    chat_frame(&mut app, &ctx, &state, pointer(start, true, egui::PointerButton::Primary));
    chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(end)]);
    let released = chat_frame(&mut app, &ctx, &state, pointer(end, false, egui::PointerButton::Primary));
    assert!(!released.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::OpenUrl(_))));
    assert_eq!(copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy])),
        "https://example.test/path después");
    chat_frame(&mut app, &ctx, &state, pointer(url.center(), true, egui::PointerButton::Primary));
    let clicked = chat_frame(&mut app, &ctx, &state, pointer(url.center(), false, egui::PointerButton::Primary));
    assert!(clicked.platform_output.commands.iter().any(|command|
        matches!(command, egui::OutputCommand::OpenUrl(open) if open.url == "https://example.test/path")));
}

#[test]
fn rich_link_titles_participate_in_the_same_history_selection() {
    let (mut app, ctx, _) = ui_harness();
    let (mut message, mut state) = message_fixture();
    message.content = "antes".into();
    message.embeds = vec![ChatEmbed { title: "Título enlazado".into(),
        url: "https://example.test/embed".into(), description: "después".into(), ..Default::default() }];
    state.messages = vec![message].into();
    for _ in 0..3 { chat_frame(&mut app, &ctx, &state, vec![]); }
    let output = chat_frame(&mut app, &ctx, &state, vec![]);
    let start = painted_text(&output, "antes").left_center();
    let end = painted_text(&output, "después").right_center();
    chat_frame(&mut app, &ctx, &state, pointer(start, true, egui::PointerButton::Primary));
    chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(end)]);
    let released = chat_frame(&mut app, &ctx, &state, pointer(end, false, egui::PointerButton::Primary));
    assert!(!released.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::OpenUrl(_))));
    assert_eq!(copied_history(&chat_frame(&mut app, &ctx, &state, vec![egui::Event::Copy])),
        "antes\nTítulo enlazado\ndespués");
}

#[test]
fn pins_and_search_toolbars_survive_pointer_transition_and_jump_to_their_original() {
    for view in ["pins", "search"] {
        for allowed in [false, true] {
            let (mut app, ctx, mut commands) = ui_harness();
            let (mut message, mut state) = message_fixture();
            state.view = view.into(); state.can_pin_messages = allowed; message.pinned = true;
            state.messages = vec![message.clone()].into(); state.pinned_messages = vec![message.clone()];
            ctx.style_mut(|style| style.animation_time = 0.);
            let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
            let body = painted_text(&output, "Fixture body").center();
            message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(body)]);
            for _ in 0..3 { message_frame(&mut app, &ctx, &message, &state, vec![]); }
            let cached = ctx.data(|data| data.get_temp::<MessageActionRegion>(egui::Id::new("message-actions-region"))).unwrap();
            let output = message_frame(&mut app, &ctx, &message, &state, vec![egui::Event::PointerMoved(cached.rect.center())]);
            let original = painted_text(&output, "Ir al mensaje original").center();
            for pressed in [true, false] {
                message_frame(&mut app, &ctx, &message, &state, pointer(original, pressed, egui::PointerButton::Primary));
            }
            let action = commands.try_recv().unwrap();
            assert_eq!((action.kind.as_str(), action.id.as_str(), action.channel_id.as_str()),
                ("jump-message", "300", "200"));
            assert_eq!(app.focus_id, "300");
            let slash = |output: &egui::FullOutput| output.shapes.iter().find_map(|shape| {
                if let egui::Shape::LineSegment { points, stroke } = &shape.shape {
                    (stroke.color == Color32::from_rgb(242, 63, 67) && points[1].x > points[0].x && points[1].y < points[0].y)
                        .then_some(egui::pos2((points[0].x + points[1].x) / 2., (points[0].y + points[1].y) / 2.))
                } else { None }
            });
            assert_eq!(slash(&output).is_some(), view == "pins" && allowed);
            if let Some(unpin) = slash(&output) {
                for pressed in [true, false] {
                    message_frame(&mut app, &ctx, &message, &state, pointer(unpin, pressed, egui::PointerButton::Primary));
                }
                let action = commands.try_recv().unwrap();
                assert_eq!((action.kind.as_str(), action.id.as_str()), ("unpin-message", "300"));
                assert_eq!(state.pinned_messages.len(), 1, "only the actor acknowledgement removes a pin");
            }
            assert!(commands.try_recv().is_err());
        }
    }
}

#[test]
fn older_page_clicks_wait_for_the_matching_completion_and_cancel_on_navigation() {
    let (mut app, ctx, mut commands) = ui_harness();
    let (template, mut state) = message_fixture();
    state.has_older_messages = true;
    state.messages = (0..40).map(|index| ChatMessage { id: (1000 + index).to_string(),
        content: format!("Boundary fixture {index}"), timestamp: template.timestamp + chrono::Duration::minutes(index),
        ..template.clone() }).collect();
    for _ in 0..3 { chat_frame(&mut app, &ctx, &state, vec![]); }
    let output = chat_frame(&mut app, &ctx, &state, vec![]);
    let older = painted_text(&output, "↑ Mensajes anteriores").center();
    for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(older, pressed, egui::PointerButton::Primary)); }
    assert!(commands.try_recv().is_err(), "bottom is not the loaded older boundary");
    // An actual wheel gesture reaches the top, then quiet frames retain it.
    let viewport = ctx.data(|data| data.get_temp::<egui::Rect>(egui::Id::new("history-viewport"))).unwrap();
    chat_frame(&mut app, &ctx, &state, vec![egui::Event::PointerMoved(viewport.center()),
        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0., 100000.), modifiers: egui::Modifiers::NONE }]);
    for _ in 0..5 { chat_frame(&mut app, &ctx, &state, vec![]); }
    assert!(app.positions["200"].offset <= 1.);
    for _ in 0..4 {
        for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(older, pressed, egui::PointerButton::Primary)); }
    }
    let action = commands.try_recv().unwrap();
    assert_eq!((action.kind.as_str(), action.channel_id.as_str()), ("older", "200"));
    assert!(!action.id.is_empty());
    assert!(commands.try_recv().is_err(), "coalesced busy snapshots must not permit duplicate requests");
    finish_history_page_request(&ctx, &serde_json::json!({"requestId":"unrelated","channelId":"200"}));
    assert!(history_page_pending(&ctx, &state));
    finish_history_page_request(&ctx, &serde_json::json!({"requestId":action.id,"channelId":"200"}));
    assert!(!history_page_pending(&ctx, &state));
    // Publish the newly enabled widget before pressing it: egui hit testing
    // correctly uses the previous frame's enabled widget geometry.
    chat_frame(&mut app, &ctx, &state, vec![]);
    for pressed in [true, false] { chat_frame(&mut app, &ctx, &state, pointer(older, pressed, egui::PointerButton::Primary)); }
    assert_eq!(commands.try_recv().unwrap().kind, "older");
    state.view = "search".into();
    chat_frame(&mut app, &ctx, &state, vec![]);
    assert!(!history_page_pending(&ctx, &state));
}

fn reaction_test_frame(app: &mut NotDiscordUi, ctx: &egui::Context, state: &AppSnapshot,
    time: f64, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(egui::RawInput { time: Some(time),
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700., 500.))),
        events, ..Default::default() }, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.draw_chat(ui, state));
    })
}
fn reaction_test_result(action: &UiAction, error: &str) -> Value {
    let extra: Value = serde_json::from_str(&action.extra).unwrap();
    serde_json::json!({"requestId":extra["requestId"],"channelId":action.channel_id,
        "messageId":action.id,"emoji":action.text,"kind":extra["kind"],
        "users":[Person { id:"701".into(),name:"Fixture reactor".into(),..Default::default() }],
        "hasMore":false,"error":error})
}
#[test]
fn reaction_people_wait_for_stationary_hover_reuse_ttl_and_ignore_cancelled_or_foreign_results() {
    let (mut app, ctx, mut commands) = ui_harness();
    let (mut message, mut state) = message_fixture();
    message.reactions = vec![MessageReaction { emoji: StatusEmoji { name: "♥".into(), ..Default::default() },
        count: 7, ..Default::default() }];
    state.messages = vec![message.clone()].into();
    ctx.style_mut(|style| style.animation_time = 0.);
    reaction_test_frame(&mut app, &ctx, &state, 0., vec![]);
    let output = reaction_test_frame(&mut app, &ctx, &state, 0.05, vec![]);
    let position = painted_text(&output, "7").center();
    reaction_test_frame(&mut app, &ctx, &state, 0.1, vec![egui::Event::PointerMoved(position)]);
    reaction_test_frame(&mut app, &ctx, &state, 1., vec![]);
    assert!(commands.try_recv().is_err(), "moving over reactions does not fetch their readers");
    // Four points of travel restart the dwell; the threshold is measured from
    // its origin, so a sequence of tiny movements cannot bypass it.
    let position = position + egui::vec2(4., 0.);
    reaction_test_frame(&mut app, &ctx, &state, 1.1, vec![egui::Event::PointerMoved(position)]);
    reaction_test_frame(&mut app, &ctx, &state, 1.5, vec![]);
    assert!(commands.try_recv().is_err());
    reaction_test_frame(&mut app, &ctx, &state, 2.4, vec![]);
    let action = commands.try_recv().unwrap();
    assert_eq!((action.kind.as_str(), action.id.as_str(), action.text.as_str(), action.channel_id.as_str()),
        ("reaction-users", "300", "♥", "200"));
    // An egui popup has an invisible sizing pass before its first paint.
    let loading = reaction_test_frame(&mut app, &ctx, &state, 2.5, vec![]);
    painted_text(&loading, "Cargando usuarios");
    assert!(commands.try_recv().is_err(), "one stationary hover sends one query");
    let mut foreign = reaction_test_result(&action, ""); foreign["channelId"] = "other".into();
    reaction_users_result(&ctx, &foreign);
    let still_loading = reaction_test_frame(&mut app, &ctx, &state, 2.6, vec![]);
    painted_text(&still_loading, "Cargando usuarios");
    reaction_users_result(&ctx, &reaction_test_result(&action, ""));
    let loaded = reaction_test_frame(&mut app, &ctx, &state, 2.7, vec![]);
    painted_text(&loaded, "Fixture reactor");
    reaction_test_frame(&mut app, &ctx, &state, 2.8, vec![egui::Event::PointerMoved(egui::pos2(650., 350.))]);
    reaction_test_frame(&mut app, &ctx, &state, 3., vec![egui::Event::PointerMoved(position)]);
    let cached = reaction_test_frame(&mut app, &ctx, &state, 4.3, vec![]);
    painted_text(&cached, "Fixture reactor");
    assert!(commands.try_recv().is_err(), "rehover within the TTL reuses the bounded session cache");
    reaction_test_frame(&mut app, &ctx, &state, 33., vec![]);
    let expired = commands.try_recv().unwrap();
    reaction_test_frame(&mut app, &ctx, &state, 33.1, vec![egui::Event::PointerMoved(egui::pos2(650., 350.))]);
    reaction_users_result(&ctx, &reaction_test_result(&expired, ""));
    reaction_test_frame(&mut app, &ctx, &state, 33.2, vec![egui::Event::PointerMoved(position)]);
    reaction_test_frame(&mut app, &ctx, &state, 34.5, vec![]);
    let retried = commands.try_recv().unwrap();
    reaction_users_result(&ctx, &reaction_test_result(&retried, "Consulta no disponible"));
    let failed = reaction_test_frame(&mut app, &ctx, &state, 34.6, vec![]);
    painted_text(&failed, "Consulta no disponible");
    state.notification_generation += 1;
    reaction_test_frame(&mut app, &ctx, &state, 34.7, vec![]);
    let pending = reaction_test_frame(&mut app, &ctx, &state, 36., vec![]);
    let renewed = commands.try_recv().unwrap();
    assert_ne!(renewed.extra, retried.extra);
    painted_text(&pending, "Cargando usuarios");
    assert!(commands.try_recv().is_err());
}

#[test]
fn reaction_type_tab_is_clickable_and_waits_for_the_old_slot_without_showing_its_users() {
    let (mut app, ctx, mut commands) = ui_harness();
    let (mut message, mut state) = message_fixture();
    message.reactions = vec![MessageReaction { emoji:StatusEmoji {name:"♥".into(),..Default::default()},
        count:7,burst_me:true,normal_me:Some(false),..Default::default() }];
    state.messages=vec![message].into();
    ctx.style_mut(|style|style.animation_time=0.);
    reaction_test_frame(&mut app,&ctx,&state,0.,vec![]);
    let initial=reaction_test_frame(&mut app,&ctx,&state,0.05,vec![]);
    let reaction=painted_text(&initial,"7").center();
    reaction_test_frame(&mut app,&ctx,&state,0.1,vec![egui::Event::PointerMoved(reaction)]);
    reaction_test_frame(&mut app,&ctx,&state,1.4,vec![]);
    let normal=commands.try_recv().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&normal.extra).unwrap()["kind"],0);
    let opened=reaction_test_frame(&mut app,&ctx,&state,1.5,vec![]);
    let tab=painted_text(&opened,"Superreacciones").center();
    reaction_test_frame(&mut app,&ctx,&state,1.6,vec![egui::Event::PointerMoved(tab)]);
    reaction_test_frame(&mut app,&ctx,&state,1.7,pointer(tab,true,egui::PointerButton::Primary));
    reaction_test_frame(&mut app,&ctx,&state,1.8,pointer(tab,false,egui::PointerButton::Primary));
    let waiting=reaction_test_frame(&mut app,&ctx,&state,1.9,vec![]);
    painted_text(&waiting,"Cargando usuarios");
    assert!(commands.try_recv().is_err(),"switching tabs must not multiply the in-flight read");
    reaction_users_result(&ctx,&reaction_test_result(&normal,""));
    let burst_loading=reaction_test_frame(&mut app,&ctx,&state,2.,vec![]);
    let burst=commands.try_recv().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&burst.extra).unwrap()["kind"],1);
    assert!(!burst_loading.shapes.iter().any(|shape|
        matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text.contains("Fixture reactor"))));
    let mut empty=reaction_test_result(&burst,"");empty["users"]=serde_json::json!([]);
    reaction_users_result(&ctx,&empty);
    let shown=reaction_test_frame(&mut app,&ctx,&state,2.1,vec![]);
    let empty_rect=painted_text(&shown,"Sin usuarios disponibles para este tipo");
    assert!(ctx.content_rect().contains_rect(empty_rect),"the lower-edge reaction tooltip stays in the viewport");
    assert!(commands.try_recv().is_err());
}
#[test]
fn sent_gifs_hide_the_raw_link_but_the_card_can_copy_its_original_url() {
    for url in ["https://tenor.com/view/synthetic-cat-gif-123", "https://media.tenor.com/synthetic/animation.gif"] {
        let (mut app, ctx, mut commands) = ui_harness();
        let (mut message, state) = message_fixture();
        message.content = format!("antes {url} después https://example.test/normal");
        let texture = app.pictures.seed_sized_fixture(&ctx, url, 384);
        for _ in 0..3 { message_frame(&mut app, &ctx, &message, &state, vec![]); }
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text.contains(url))));
        painted_text(&output, "antes"); painted_text(&output, "después");
        painted_text(&output, "https://example.test/normal"); painted_text(&output, "▶ GIF");
        let card = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds().center()),
            _ => None,
        }).expect("The GIF must still render its media preview");
        for pressed in [true, false] {
            message_frame(&mut app, &ctx, &message, &state, pointer(card, pressed, egui::PointerButton::Secondary));
        }
        for _ in 0..3 { message_frame(&mut app, &ctx, &message, &state, vec![]); }
        let output = message_frame(&mut app, &ctx, &message, &state, vec![]);
        let copy = painted_text(&output, "Copiar enlace").center();
        let mut copied = false;
        for pressed in [true, false] {
            let output = message_frame(&mut app, &ctx, &message, &state, pointer(copy, pressed, egui::PointerButton::Primary));
            copied |= output.platform_output.commands.iter().any(|command| matches!(command,
                egui::OutputCommand::CopyText(text) if text == url));
        }
        assert!(copied, "Right click must copy the original GIF link");
        assert!(message.content.contains(url), "The original text remains available for editing and copying");
        assert!(commands.try_recv().is_err());
        assert!(app.viewer.is_none());
    }
}

