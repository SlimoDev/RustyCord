use super::*;
fn fixture()->(NotDiscordUi,egui::Context,tokio::sync::mpsc::UnboundedReceiver<UiAction>,AppSnapshot,Person) {
    let (app,_,commands)=super::super::tests::harness();let ctx=app.ctx.clone();
    let person=Person {id:"300".into(),name:"Selected member".into(),..Default::default()};
    let state=AppSnapshot {signed_in:true,selected_guild_id:"10".into(),selected_channel_id:"20".into(),
        current_user:Some(Person {id:"100".into(),name:"Moderator".into(),..Default::default()}),
        member_moderation:Some(MemberModeration {guild_id:"10".into(),actor_id:"100".into(),user_id:"300".into(),
            revision:7,nickname:"Current nickname".into(),can_ban:true,can_kick:true,can_timeout:true,can_nickname:true,..Default::default()}),
        ..Default::default()};
    (app,ctx,commands,state,person)
}
fn text(output:&egui::FullOutput,label:&str)->egui::Rect {
    output.shapes.iter().find_map(|shape|match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text==label=>Some(egui::Rect::from_min_size(text.pos,text.galley.size())),_=>None,
    }).unwrap_or_else(||panic!("Missing moderation text: {label}"))
}
fn pointer(point:egui::Pos2,pressed:bool)->Vec<egui::Event> {
    vec![egui::Event::PointerMoved(point),egui::Event::PointerButton {pos:point,button:egui::PointerButton::Primary,pressed,modifiers:egui::Modifiers::NONE}]
}
fn menu(app:&mut NotDiscordUi,ctx:&egui::Context,state:&AppSnapshot,person:&Person,events:Vec<egui::Event>)->egui::FullOutput {
    ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(900.,650.))),events,..Default::default()},|ctx| {
        egui::CentralPanel::default().show(ctx,|ui|app.people_menu(ui,person,state));
    })
}
fn dialog(app:&mut NotDiscordUi,ctx:&egui::Context,state:&AppSnapshot,events:Vec<egui::Event>)->egui::FullOutput {
    ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1000.,700.))),events,..Default::default()},|ctx| {
        egui::CentralPanel::default().show(ctx,|_|{});app.draw_dialog(ctx,state);
    })
}
#[test]
fn opening_a_member_context_loads_once_per_scope_without_guessing_permissions() {
    let (mut app,ctx,mut commands,mut state,person)=fixture();state.member_moderation=None;
    for _ in 0..5 {menu(&mut app,&ctx,&state,&person,vec![]);}
    let load=commands.try_recv().unwrap();
    assert_eq!((load.kind.as_str(),load.id.as_str(),load.channel_id.as_str()),("member-moderation-load","300","10"));
    assert!(commands.try_recv().is_err());
    state.selected_guild_id="11".into();menu(&mut app,&ctx,&state,&person,vec![]);
    assert_eq!(commands.try_recv().unwrap().channel_id,"11");
    state.notification_generation+=1;menu(&mut app,&ctx,&state,&person,vec![]);
    assert_eq!(commands.try_recv().unwrap().kind,"member-moderation-load");
    assert!(commands.try_recv().is_err());
}
#[test]
fn banning_from_the_member_menu_requires_a_second_explicit_confirmation_with_target_and_reason() {
    let (mut app,ctx,mut commands,state,person)=fixture();
    menu(&mut app,&ctx,&state,&person,vec![]);
    let output=menu(&mut app,&ctx,&state,&person,vec![]);let ban=text(&output,"Banear del servidor").center();
    for pressed in [true,false] {menu(&mut app,&ctx,&state,&person,pointer(ban,pressed));}
    assert!(matches!(app.dialog,Some(Dialog::Prompt{..})));assert!(commands.try_recv().is_err());
    app.prompt="Repeated harassment".into();
    for _ in 0..3 {dialog(&mut app,&ctx,&state,vec![]);}
    let output=dialog(&mut app,&ctx,&state,vec![]);let next=text(&output,"Continuar").center();
    for pressed in [true,false] {dialog(&mut app,&ctx,&state,pointer(next,pressed));}
    let Some(Dialog::Confirm {description,action,..})=app.dialog.as_ref() else {panic!("Ban must have a confirmation dialog");};
    assert!(description.contains("Selected member")&&description.contains("Repeated harassment"));
    assert_eq!(action.extra,"ban");assert!(commands.try_recv().is_err(),"reason entry cannot ban a member");
    for _ in 0..3 {dialog(&mut app,&ctx,&state,vec![]);}
    let output=dialog(&mut app,&ctx,&state,vec![]);let confirm=text(&output,"Confirmar").center();
    for pressed in [true,false] {dialog(&mut app,&ctx,&state,pointer(confirm,pressed));}
    let action=commands.try_recv().unwrap();let payload:Value=serde_json::from_str(&action.text).unwrap();
    assert_eq!((action.kind.as_str(),action.extra.as_str(),action.id.as_str(),action.channel_id.as_str()),("member-moderation","ban","300","10"));
    assert_eq!(payload["revision"],7);assert_eq!(payload["reason"],"Repeated harassment");assert_eq!(payload["delete_message_seconds"],0);
    assert!(payload.get("targetName").is_none());assert!(app.dialog.is_none());assert!(commands.try_recv().is_err());
}
#[test]
fn a_changed_policy_cannot_confirm_a_prepared_ban_and_cancel_remains_available() {
    let (mut app,ctx,mut commands,mut state,person)=fixture();
    app.open_member_action(&person,state.member_moderation.as_ref().unwrap(),"ban",None);
    let Some(Dialog::Prompt {action,..})=app.dialog.clone() else {unreachable!()};
    app.dialog=app.member_moderation_submission(&action,&state).unwrap().1;
    state.member_moderation.as_mut().unwrap().revision=8;
    state.member_moderation.as_mut().unwrap().can_ban=false;
    for _ in 0..3 {dialog(&mut app,&ctx,&state,vec![]);}
    let output=dialog(&mut app,&ctx,&state,vec![]);let confirm=text(&output,"Confirmar").center();
    for pressed in [true,false] {dialog(&mut app,&ctx,&state,pointer(confirm,pressed));}
    assert!(commands.try_recv().is_err());assert!(matches!(app.dialog,Some(Dialog::Confirm{..})));
    let output=dialog(&mut app,&ctx,&state,vec![]);let cancel=text(&output,"Cancelar").center();
    for pressed in [true,false] {dialog(&mut app,&ctx,&state,pointer(cancel,pressed));}
    assert!(app.dialog.is_none());assert!(commands.try_recv().is_err());
}
#[test]
fn changing_own_nickname_uses_the_explicit_capability_and_its_current_value() {
    let (mut app,ctx,mut commands,mut state,_)=fixture();
    let me=state.current_user.clone().unwrap();
    let info=state.member_moderation.as_mut().unwrap();info.user_id=me.id.clone();info.can_ban=false;info.can_kick=false;info.can_timeout=false;
    menu(&mut app,&ctx,&state,&me,vec![]);let output=menu(&mut app,&ctx,&state,&me,vec![]);
    let point=text(&output,"Cambiar apodo").center();
    for pressed in [true,false] {menu(&mut app,&ctx,&state,&me,pointer(point,pressed));}
    assert_eq!(app.prompt,"Current nickname");app.prompt="Own server nickname".into();
    for _ in 0..3 {dialog(&mut app,&ctx,&state,vec![]);}
    let output=dialog(&mut app,&ctx,&state,vec![]);let next=text(&output,"Continuar").center();
    for pressed in [true,false] {dialog(&mut app,&ctx,&state,pointer(next,pressed));}
    let action=commands.try_recv().unwrap();let payload:Value=serde_json::from_str(&action.text).unwrap();
    assert_eq!((action.id.as_str(),action.extra.as_str()),("100","nickname"));assert_eq!(payload["nick"],"Own server nickname");
    assert!(commands.try_recv().is_err());
    state.member_moderation.as_mut().unwrap().can_nickname=false;
    app.open_member_action(&me,state.member_moderation.as_ref().unwrap(),"nickname",None);
    let Some(Dialog::Prompt {action,..})=app.dialog.as_ref() else {unreachable!()};
    assert!(action_error(action,&state).is_some(),"ChangeNickname must be confirmed even for self");
}
