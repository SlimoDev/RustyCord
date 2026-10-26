use super::*;
#[tokio::test]
async fn edited_and_deleted_payloads_survive_navigation_cache_eviction_reload_and_pagination() {
    let (mut c,_)=Controller::test_fixture();selected(&mut c);
    let mut original=message("100");original.content="First observed version".into();original.author.id="30".into();original.reply_to=Some("90".into());
    original.attachments.push(ChatAttachment{id:"40".into(),url:"https://cdn.discordapp.com/attachments/1/2/known.png".into(),..Default::default()});
    c.cache.replace("20",vec![original]);
    c.dispatch("MESSAGE_UPDATE",json!({"id":"100","channel_id":"20","content":"Last observed version"})).await.unwrap();
    c.dispatch("MESSAGE_DELETE",json!({"id":"100","channel_id":"20"})).await.unwrap();
    c.act(UiAction::id("view","friends")).await.unwrap();
    for channel in ["21","22","23","24"] {let mut other=message(channel);other.channel_id=channel.into();c.cache.upsert(other);}
    assert!(c.cache.get("20").is_empty());
    selected(&mut c);
    for _ in 0..3 {
        let mut page=UiAction::new("__present");page.channel_id="20".into();
        c.complete(page,json!([{"id":"100","channel_id":"20","content":"Last observed version","author":{"id":"30","username":"Known author"}}])).await.unwrap();
        let retained=c.state.messages.iter().filter(|message|message.id=="100").collect::<Vec<_>>();assert_eq!(retained.len(),1);
        let retained=retained[0];assert!(retained.deleted&&retained.edited);assert_eq!(retained.content,"Last observed version");
        assert_eq!(retained.previous_contents,vec!["First observed version"]);assert_eq!(retained.author.id,"30");
        assert_eq!(retained.reply_to.as_deref(),Some("90"));assert_eq!(retained.attachments.len(),1);
        c.status("Ventana minimizada/restaurada; se recibió una actualización independiente");c.refresh_messages();
        assert_eq!(c.state.messages.iter().find(|message|message.id=="100").unwrap().previous_contents,vec!["First observed version"]);
    }
    let mut older=UiAction::id("__older","offline-page-token");older.channel_id="20".into();
    c.complete(older,json!([{"id":"99","channel_id":"20","content":"Older received"},{"id":"100","channel_id":"20","content":"Last observed version"}])).await.unwrap();
    assert_eq!(c.state.messages.iter().filter(|message|message.id=="100").count(),1);
    assert!(c.state.messages.iter().any(|message|message.id=="99"&&!message.deleted));
    c.reset_session_view();assert!(c.features.history("20",100).unwrap().is_empty());
    assert!(c.features.restore_session("20",vec![]).is_empty());
}
#[tokio::test]
async fn edits_restore_previous_text_after_focus_refresh_without_a_delete_and_unseen_deletes_never_fabricate_content() {
    let (mut c,_)=Controller::test_fixture();selected(&mut c);c.cache.replace("20",vec![message("100")]);
    c.dispatch("MESSAGE_UPDATE",json!({"id":"100","channel_id":"20","content":"Edited while visible"})).await.unwrap();
    for full_reload in [false,true,false] {
        c.act(UiAction{kind:"chat-visibility".into(),channel_id:"20".into(),text:"hidden".into(),..Default::default()}).await.unwrap();
        let page=json!([{"id":"100","channel_id":"20","content":"Edited while visible","edited_timestamp":"2026-10-23T01:00:00Z"}]);
        if full_reload {
            c.complete(UiAction::id("__messages","20"),json!({"channel":{"id":"20","type":1},"messages":page})).await.unwrap();
        } else {
            let mut refresh=UiAction::new("__present");refresh.channel_id="20".into();c.complete(refresh,page).await.unwrap();
        }
        c.act(UiAction{kind:"chat-visibility".into(),channel_id:"20".into(),id:"100".into(),text:"reading".into(),..Default::default()}).await.unwrap();
        assert_eq!(c.state.messages[0].previous_contents,vec!["Fixture"]);assert!(c.state.messages[0].edited);
    }
    c.dispatch("MESSAGE_DELETE",json!({"id":"999","channel_id":"20"})).await.unwrap();
    assert!(!c.state.messages.iter().any(|message|message.id=="999"));
    assert!(!c.features.history("20",100).unwrap().iter().any(|message|message.id=="999"));
    c.cache.clear();c.loading_channel="20".into();
    c.dispatch("MESSAGE_DELETE",json!({"id":"101","channel_id":"20"})).await.unwrap();
    c.complete(UiAction::id("__messages","20"),json!({"channel":{"id":"20","type":1},"messages":[{"id":"101","channel_id":"20","content":"Received concurrently with deletion"}]})).await.unwrap();
    assert!(c.state.messages.iter().any(|message|message.id=="101"&&message.deleted&&message.content=="Received concurrently with deletion"));
    let mut again=UiAction::new("__present");again.channel_id="20".into();c.complete(again,json!([])).await.unwrap();
    assert!(c.state.messages.is_empty());
    assert!(c.features.history("20",100).unwrap().iter().any(|message|message.id=="101"&&message.deleted));
}
#[tokio::test]
async fn older_page_request_tokens_finish_on_success_busy_end_wrong_channel_and_error() {
    let (mut c,updates)=Controller::test_fixture();selected(&mut c);
    let action=|kind:&str,token:&str,channel:&str|UiAction{kind:kind.into(),id:token.into(),channel_id:channel.into(),..Default::default()};
    c.state.busy=true;c.act(action("older","busy-token","20")).await.unwrap();
    c.state.busy=false;c.state.has_older_messages=false;c.act(action("older","end-token","20")).await.unwrap();
    let invalid=action("older","cross-token","21");let error=c.act(invalid.clone()).await.unwrap_err();c.action_error(&invalid,error);
    c.complete(action("__older","success-token","20"),json!([])).await.unwrap();
    c.completion_error(&action("__older","error-token","20"),"Synthetic offline failure".into());
    let completed=updates.try_iter().filter_map(|update|match update {UiUpdate::Data(name,value)if name=="history-page-result"=>Some((s(&value,"requestId"),s(&value,"channelId"))),_=>None}).collect::<Vec<_>>();
    assert_eq!(completed,vec![("busy-token".into(),"20".into()),("end-token".into(),"20".into()),("cross-token".into(),"21".into()),("success-token".into(),"20".into()),("error-token".into(),"20".into())]);
}
#[tokio::test]
async fn pagination_and_visible_rows_stay_inside_the_loaded_server_interval() {
    let (mut c,_)=Controller::test_fixture();selected(&mut c);
    c.features.record(&message("100"),true).unwrap();
    c.cache.replace("20",(1000..1050).map(|id|{let mut row=message(&id.to_string());row.timestamp=format!("2026-10-23T10:{:02}:{:02}Z",id/60,id%60).parse().unwrap();row}).collect());
    c.refresh_messages();
    assert_eq!(c.state.messages.first().unwrap().id,"1000");
    assert_eq!(c.older_page_route("20"),"channels/20/messages?limit=50&before=1000");
    let page=(950..1000).map(|id|json!({"id":id.to_string(),"channel_id":"20","content":"Known older server row","timestamp":format!("2026-10-23T10:{:02}:{:02}Z",id/60,id%60)})).collect::<Vec<_>>();
    let mut completion=UiAction::id("__older","synthetic-pagination-frontier");completion.channel_id="20".into();
    c.complete(completion,json!(page)).await.unwrap();
    assert_eq!(c.older_page_route("20"),"channels/20/messages?limit=50&before=950");
    assert!(!c.state.messages.iter().any(|message|message.id=="100"));
    assert!(c.features.history("20",100).unwrap().iter().any(|message|message.id=="100"&&message.deleted));
    assert!(c.api.is_none()&&c.gateway.is_none());
}
#[tokio::test]
async fn known_pin_and_search_payloads_outside_the_chat_cache_keep_edits_deletions_and_metadata() {
    for view in ["pins","search"] {
        let (mut c,_)=Controller::test_fixture();selected(&mut c);c.state.view=view.into();
        let mut known=message("100");known.author.id="30".into();known.reply_to=Some("90".into());
        known.attachments.push(ChatAttachment{id:"40".into(),url:"https://cdn.discordapp.com/attachments/1/2/known.png".into(),..Default::default()});
        if view=="pins" {known.pinned=true;c.state.pinned_messages=vec![known.clone()];}
        c.state.messages=vec![known].into();assert!(c.cache.get("20").is_empty());
        c.cache.replace("20",vec![message("1000")]);
        c.dispatch("MESSAGE_UPDATE",json!({"id":"100","channel_id":"20","content":"Known latest edit"})).await.unwrap();
        c.dispatch("MESSAGE_DELETE",json!({"id":"100","channel_id":"20"})).await.unwrap();
        assert_eq!(c.older_page_route("20"),"channels/20/messages?limit=50&before=1000");
        c.cache.clear();c.state.view="chat".into();
        let mut refresh=UiAction::new("__present");refresh.channel_id="20".into();c.complete(refresh,json!([])).await.unwrap();
        assert!(c.state.messages.is_empty());
        let history=c.features.history("20",100).unwrap();let restored=history.iter().rev().find(|message|message.id=="100"&&message.deleted).unwrap();
        assert!(restored.deleted&&restored.edited);assert_eq!(restored.content,"Known latest edit");
        assert!(history.iter().any(|message|message.id=="100"&&message.content=="Fixture"));assert_eq!(restored.author.id,"30");
        assert_eq!(restored.reply_to.as_deref(),Some("90"));assert_eq!(restored.attachments.len(),1);
        assert!(c.api.is_none()&&c.gateway.is_none());
    }
}
#[tokio::test]
async fn voice_action_errors_remain_visible_until_explicitly_dismissed() {
    let (mut c, _) = Controller::test_fixture();
    c.action_error(&UiAction::new("voice"), "No se pudo abrir la conexión nativa.".into());
    c.status("Actividad actualizada");
    assert_eq!(c.state.voice_error, "No se pudo abrir la conexión nativa.");
    c.act(UiAction::new("dismiss-voice-error")).await.unwrap();
    assert!(c.state.voice_error.is_empty());
}
#[test]
fn retired_member_views_release_their_buffers_and_keep_old_snapshots_unchanged() {
    let (mut c, _) = Controller::test_fixture();
    let member = ChannelMember { user:Person{id:"2".into(),name:"Synthetic member".into(),..Default::default()}, ..Default::default() };
    c.guild_member_slots = vec![Some(member.clone()); 512];
    c.state.guild_members = vec![member.clone();512].into();
    c.state.group_members = vec![member;64];
    c.state.group_recipient_candidates = vec![Person::default();128];
    c.state.pinned_messages = vec![message("80");50];
    c.state.guild_member_groups = vec![GuildMemberGroup::default();32];
    c.state.search_people = vec![Person::default();512];
    let old = c.state.clone();
    c.reset_navigation();
    assert_eq!(c.state.guild_members.capacity(),0);
    assert_eq!(c.guild_member_slots.capacity(),0);
    assert_eq!(c.state.group_members.capacity(),0);
    assert_eq!(c.state.group_recipient_candidates.capacity(),0);
    assert_eq!(c.state.pinned_messages.capacity(),0);
    assert_eq!(c.state.guild_member_groups.capacity(),0);
    assert_eq!(c.state.search_people.capacity(),0);
    assert_eq!(old.guild_members.len(),512);
    assert_eq!(old.guild_members[0].user.name,"Synthetic member");
}
#[tokio::test]
async fn disconnect_releases_identity_tables_and_preserves_explicit_audio_choices() {
    let (mut c, _) = Controller::test_fixture();
    for index in 0..512 {
        let id=(index+1).to_string();
        c.people.insert(id.clone(),Person{id:id.clone(),name:"Synthetic person".into(),..Default::default()});
        c.channels.insert(id.clone(),json!({"id":id,"type":1}));
    }
    c.state.self_mute=true;
    c.state.self_deaf=true;
    c.features.record(&message("80"), true).unwrap();
    assert_eq!(c.features.history("20", 100).unwrap().len(), 1);
    assert!(c.people.capacity()>=512 && c.channels.capacity()>=512);
    c.disconnect().await;
    assert_eq!(c.people.capacity(),0);
    assert_eq!(c.channels.capacity(),0);
    assert!(c.state.self_mute && c.state.self_deaf);
    assert!(!c.state.signed_in);
    assert!(c.features.history("20", 100).unwrap().is_empty());
}
#[tokio::test]
async fn present_page_and_scroll_version_are_published_as_one_snapshot() {
    let (mut c, updates) = Controller::test_fixture();
    selected(&mut c);
    c.state.busy = true;
    let mut action = UiAction::new("__present");
    action.channel_id = "20".into();
    c.complete(action,json!([{"id":"81","channel_id":"20","content":"Latest","author":{"id":"31","username":"Fixture"}}])).await.unwrap();
    assert!(!c.state.busy);
    assert_eq!(c.state.latest_messages_version, 1);
    assert_eq!(c.state.messages.last().unwrap().id, "81");
    assert!(
        !updates
            .try_iter()
            .any(|update| matches!(update,UiUpdate::Data(name,_) if name=="present")),
        "No early scroll event may race the coalesced page"
    );
}
#[tokio::test]
async fn confirmed_unpin_and_gateway_unpin_update_pins_view_without_refetch() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    let mut first = message("51");
    first.pinned = true;
    let mut second = message("52");
    second.pinned = true;
    c.cache.replace("20", vec![first.clone(), second.clone()]);
    c.state.view = "pins".into();
    c.state.pinned_messages = vec![first, second];
    c.state.messages = c.state.pinned_messages.clone().into();
    let mut action = UiAction::id("__message-pin", "51");
    action.channel_id = "20".into();
    action.text = "false".into();
    c.complete(action, json!({})).await.unwrap();
    assert_eq!(
        c.state
            .pinned_messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["52"]
    );
    assert_eq!(c.state.messages.len(), 1);
    assert!(
        !c.cache
            .get("20")
            .iter()
            .find(|message| message.id == "51")
            .unwrap()
            .pinned
    );
    c.dispatch(
        "MESSAGE_UPDATE",
        json!({"id":"52","channel_id":"20","pinned":false}),
    )
    .await
    .unwrap();
    assert!(c.state.pinned_messages.is_empty() && c.state.messages.is_empty());
}
#[tokio::test]
async fn dm_sidebar_profile_is_read_only_scoped_and_failure_does_not_release_history() {
    let (mut controller, updates) = Controller::test_fixture();
    selected(&mut controller);
    controller.state.selected_channel_kind = "dm".into();
    controller.state.direct_messages[0].recipient_id = "31".into();
    let mut action = UiAction::id("__dm_sidebar_profile", "31");
    action.channel_id = "20".into();
    controller
        .complete(
            action.clone(),
            json!({"user":{"id":"31","username":"Fixture"},"user_profile":{"bio":"Synthetic bio"}}),
        )
        .await
        .unwrap();
    assert_eq!(controller.state.view, "chat");
    assert!(controller.state.profile_user.is_none());
    assert!(updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,value) if kind=="dm-sidebar-profile"&&value["profile"]["User"]["Id"]=="31")));
    controller.state.busy = true;
    controller.completion_error(&action, "Synthetic failure".into());
    assert!(controller.state.busy);
    assert!(updates.try_iter().any(|update|matches!(update,UiUpdate::Data(kind,value) if kind=="dm-sidebar-profile"&&value["error"].is_string())));
    controller.state.selected_channel_id = "21".into();
    controller
        .complete(action, json!({"user":{"id":"31"}}))
        .await
        .unwrap();
    assert!(updates.try_iter().next().is_none());
}

#[tokio::test]
async fn older_history_repeated_while_loading_or_exhausted_never_queues_api_work() {
    let (mut controller, _) = Controller::test_fixture();
    selected(&mut controller);
    controller.state.has_older_messages = true;
    controller.state.busy = true;
    controller.act(UiAction::new("older")).await.unwrap();
    controller.state.busy = false;
    controller.state.has_older_messages = false;
    controller.act(UiAction::new("older")).await.unwrap();
    assert!(controller.api.is_none());
}
#[test]
fn failed_background_read_ack_does_not_release_navigation_busy() {
    let (mut controller, _) = Controller::test_fixture();
    controller.state.busy = true;
    controller.state.status = "Cargando canal".into();
    controller.completion_error(
        &UiAction::new("__marked-read"),
        "Synthetic ACK failure".into(),
    );
    assert!(controller.state.busy);
    assert_eq!(controller.state.status, "Cargando canal");
}
fn selected(c: &mut Controller) {
    c.state.signed_in = true;
    c.state.current_user = Some(Person {
        id: "1".into(),
        name: "Fixture self".into(),
        ..Default::default()
    });
    c.state.selected_channel_id = "20".into();
    c.state.view = "chat".into();
    c.state.channel_access = ChannelAccess::private();
    c.state.direct_messages.push(NavItem {
        id: "20".into(),
        kind: "dm".into(),
        ..Default::default()
    });
}
fn message(id: &str) -> ChatMessage {
    ChatMessage {
        id: id.into(),
        channel_id: "20".into(),
        content: "Fixture".into(),
        ..Default::default()
    }
}
fn guild_channel(id: &str, kind: &str, visible: bool, history: bool) -> NavItem {
    NavItem {
        id: id.into(),
        name: format!("Fixture {id}"),
        kind: kind.into(),
        access: Some(ChannelAccess {
            can_view: visible,
            can_read_history: history,
            ..ChannelAccess::private()
        }),
        ..Default::default()
    }
}

#[test]
fn preferred_guild_channel_uses_visible_text_order_and_skips_non_chat_or_denied_entries() {
    let channels = vec![
        guild_channel("90", "category", true, true),
        guild_channel("91", "voice", true, true),
        guild_channel("92", "forum", true, true),
        guild_channel("93", "thread", true, true),
        guild_channel("94", "text", false, true),
        guild_channel("95", "text", true, false),
        guild_channel("200", "text", true, true),
        guild_channel("100", "text", true, true),
    ];
    assert_eq!(
        preferred_guild_channel(&channels, "").as_deref(),
        Some("200")
    );
    assert_eq!(
        preferred_guild_channel(&channels, "100").as_deref(),
        Some("100")
    );
    for invalid in ["missing", "90", "91", "92", "94", "95"] {
        assert_eq!(
            preferred_guild_channel(&channels, invalid).as_deref(),
            Some("200")
        );
    }
    assert_eq!(preferred_guild_channel(&channels[..6], "missing"), None);
}

#[test]
fn remembered_channels_are_independent_per_guild_and_reset_with_session() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    c.state.direct_messages.clear();
    c.state.selected_guild_id = "10".into();
    c.state.channels = vec![
        guild_channel("100", "text", true, true),
        guild_channel("101", "text", true, true),
    ];
    // Raw ACL is deliberately unavailable: selection updates navigation but
    // cannot start a REST history request in this synthetic controller.
    c.select_channel("101").unwrap();
    c.state.selected_guild_id = "20".into();
    c.state.channels = vec![
        guild_channel("200", "text", true, true),
        guild_channel("201", "text", true, true),
    ];
    c.select_channel("200").unwrap();
    assert_eq!(
        c.last_guild_channels.get("10").map(String::as_str),
        Some("101")
    );
    assert_eq!(
        c.last_guild_channels.get("20").map(String::as_str),
        Some("200")
    );
    assert_eq!(
        preferred_guild_channel(&c.state.channels, c.last_guild_channels.get("20").unwrap())
            .as_deref(),
        Some("200")
    );
    c.save_navigation();
    let saved: Navigation = storage::read_json(&c.data.join("navigation.json"), 8192);
    assert_eq!(saved.guild_id, "20");
    assert!(
        saved.channel_id.is_empty(),
        "A guild's last channel must stay in session memory only"
    );
    c.connect("invalid".into());
    assert!(c.last_guild_channels.is_empty());
    assert!(c.gateway.is_none() && c.media.is_none());
}

#[test]
fn revoked_or_deleted_remembered_channel_falls_back_and_empty_guild_clears_context() {
    let (mut c, _) = Controller::test_fixture();
    c.state.selected_guild_id = "10".into();
    c.state.selected_channel_id = "300".into();
    c.state.channels = vec![
        guild_channel("200", "text", true, true),
        guild_channel("300", "text", false, false),
    ];
    assert_eq!(
        preferred_guild_channel(&c.state.channels, "300").as_deref(),
        Some("200")
    );
    c.recover_guild_navigation().unwrap();
    assert_eq!(c.state.selected_channel_id, "200");
    c.state.channels.clear();
    c.state.messages.push(message("10"));
    c.recover_guild_navigation().unwrap();
    assert!(c.state.selected_channel_id.is_empty() && c.state.messages.is_empty());
    assert_eq!(c.state.channel_name, "No hay canales de texto accesibles");
    assert!(!c.state.channel_access.can_view && !c.state.channel_access.can_read_history);
}

#[tokio::test]
async fn startup_restores_guild_but_ignores_saved_channel_and_selects_first_accessible_text() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    c.state.direct_messages.clear();
    c.state.selected_channel_id.clear();
    c.state.guilds.push(NavItem {
        id: "10".into(),
        kind: "guild".into(),
        ..Default::default()
    });
    storage::save_json(
        &c.data.join("navigation.json"),
        &Navigation {
            account_id: "1".into(),
            guild_id: "10".into(),
            channel_id: "999".into(),
            last_private_channel_id: String::new(),
        },
    )
    .unwrap();
    c.restore_navigation();
    assert_eq!(c.state.selected_guild_id, "10");
    assert!(c.restore_channel.is_empty());
    assert!(c.last_guild_channels.is_empty());
    let result=c.complete(UiAction::id("__guild","10"),json!({
        "guild":{"id":"10","owner_id":"1","roles":[]},"member":{"user":{"id":"1"},"roles":[]},
        "channels":[{"id":"90","guild_id":"10","type":4,"name":"Category","position":0},
            {"id":"91","guild_id":"10","type":2,"name":"Voice","position":0},
            {"id":"92","guild_id":"10","type":15,"name":"Forum","position":1},
            {"id":"200","guild_id":"10","type":0,"name":"First","position":2},
            {"id":"999","guild_id":"10","type":0,"name":"Saved last session","position":3}]
    })).await;
    // No API was installed. The selected channel is still observable before
    // its attempted history load correctly stops at the authentication guard.
    assert_eq!(result.unwrap_err(), "Inicia sesión para continuar.");
    assert_eq!(c.state.selected_channel_id, "200");
    assert_eq!(
        c.last_guild_channels.get("10").map(String::as_str),
        Some("200")
    );
    assert!(c.gateway.is_none() && c.media.is_none());
}

#[tokio::test]
async fn group_members_action_is_recognized_and_group_reply_populates_members() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    let guard = c.act(UiAction::new("group-members")).await.unwrap_err();
    assert_eq!(guard, "Abre un grupo para consultar sus miembros.");
    c.state.selected_channel_kind = "group".into();
    assert_eq!(
        c.act(UiAction::new("group-members")).await.unwrap_err(),
        "Inicia sesión para continuar."
    );
    c.state.selected_guild_id.clear();
    let reply=c.group_refresh_action("20");
    c.complete(reply,json!({"id":"20","type":3,"owner_id":"2","recipients":[{"id":"2","username":"Fixture member"}]})).await.unwrap();
    assert_eq!(c.state.selected_channel_kind, "group");
    assert!(
        c.state
            .group_members
            .iter()
            .any(|m| m.user.id == "2" && m.is_owner)
    );
    assert!(c.state.group_members.iter().any(|m| m.user.id == "1"));
    assert!(c.gateway.is_none() && c.media.is_none());
}

#[tokio::test]
async fn reply_jump_to_loaded_message_highlights_without_network() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    c.state.messages.push(message("31"));
    c.act(UiAction {
        kind: "jump-message".into(),
        id: "31".into(),
        channel_id: "20".into(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert_eq!(c.state.focus_message_id, "31");
    assert_eq!(c.state.focus_message_version, 1);
    assert!(c.api.is_none() && c.gateway.is_none());
    assert!(
        c.act(UiAction {
            kind: "jump-message".into(),
            id: "31".into(),
            channel_id: "99".into(),
            ..Default::default()
        })
        .await
        .is_err()
    );
}

#[tokio::test]
async fn search_original_can_switch_only_to_a_known_accessible_channel_in_the_current_guild() {
    let (mut c,_)=Controller::test_fixture();selected(&mut c);
    c.state.selected_guild_id="10".into();c.state.view="search".into();
    c.state.channels=vec![guild_channel("20","text",true,true),guild_channel("21","text",true,true)];
    c.guild_acl=Some(GuildAccess::from_values(&json!({"id":"10","owner_id":"999","roles":[{"id":"10","permissions":"66560"}]}),&json!({"roles":[]}),"1"));
    c.channels.insert("21".into(),json!({"id":"21","guild_id":"10","type":0,"name":"Target","permission_overwrites":[{"id":"10","type":0,"allow":"0","deny":"65536"}]}));
    let mut hit=message("31");hit.channel_id="21".into();c.state.messages=vec![hit].into();
    let action=UiAction{kind:"jump-message".into(),id:"31".into(),channel_id:"21".into(),..Default::default()};
    assert!(c.act(action.clone()).await.unwrap_err().contains("permiso"));
    assert_eq!(c.state.selected_channel_id,"20");assert_eq!(c.state.view,"search");
    c.channels.get_mut("21").unwrap()["permission_overwrites"]=json!([]);
    let mut unknown=action.clone();unknown.id="999".into();assert!(c.act(unknown).await.is_err());assert_eq!(c.state.selected_channel_id,"20");
    assert_eq!(c.act(action.clone()).await.unwrap_err(),"Inicia sesión para continuar.");
    assert_eq!(c.state.selected_channel_id,"21");assert_eq!(c.state.view,"chat");
    assert!(c.state.channel_access.can_read_history);assert!(c.api.is_none()&&c.gateway.is_none());
    let mut completion=action;completion.kind="__jump-message".into();
    c.complete(completion,json!([{"id":"31","channel_id":"21","content":"Known original"}])).await.unwrap();
    assert_eq!(c.state.focus_message_id,"31");assert_eq!(c.state.messages[0].content,"Known original");
}

#[tokio::test]
async fn reply_jump_from_pins_restores_chat_but_missing_target_preserves_view() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    c.state.view = "pins".into();
    let action = UiAction {
        kind: "__jump-message".into(),
        id: "31".into(),
        channel_id: "20".into(),
        ..Default::default()
    };
    assert!(c.complete(action.clone(), json!([])).await.is_err());
    assert_eq!(c.state.view, "pins");
    c.complete(action,json!([{"id":"31","channel_id":"20","content":"Original","author":{"id":"2"},"timestamp":"2026-10-20T12:00:00Z"}])).await.unwrap();
    assert_eq!(c.state.view, "chat");
    assert_eq!(c.state.focus_message_id, "31");
    assert!(c.state.messages.iter().any(|message| message.id == "31"));
}

#[tokio::test]
async fn startup_reads_chosen_presence_before_preparing_identify() {
    let requests = std::cell::RefCell::new(Vec::new());
    let prepared = prepare_session(|path| {
        requests.borrow_mut().push(path);
        std::future::ready(Ok(match path {
            "users/@me" => json!({"id":"1","username":"Fixture self"}),
            "users/@me/settings" => json!({"status":"invisible","unrelatedSecret":"discard"}),
            "users/@me/settings-proto/1" => {
                json!({"settings":notdiscord_core::settings_proto::visibility_delta(false)})
            }
            _ => panic!("Opening a session must only issue the three expected GETs"),
        }))
    })
    .await
    .unwrap();
    assert_eq!(
        requests.into_inner(),
        [
            "users/@me",
            "users/@me/settings",
            "users/@me/settings-proto/1"
        ]
    );
    assert!(!prepared.to_string().contains("unrelatedSecret"));
    let (mut c, _) = Controller::test_fixture();
    assert_eq!(
        c.apply_initial_settings(&prepared).as_deref(),
        Some("invisible")
    );
    assert_eq!(c.state.self_presence, "invisible");
    assert!(!c.state.game_activity_enabled);
    assert!(c.state.game_activity_available && c.state.game_activity_configured);
    assert!(c.gateway.is_none() && c.media.is_none());
}

#[tokio::test]
async fn startup_unknown_presence_never_defaults_to_online() {
    for unavailable in [false, true] {
        let prepared = prepare_session(|path| {
            std::future::ready(if path == "users/@me" {
                Ok(json!({"id":"1"}))
            } else if unavailable {
                Err("Synthetic unavailable settings".into())
            } else {
                Ok(json!({"status":"offline","settings":null}))
            })
        })
        .await
        .unwrap();
        let (mut c, _) = Controller::test_fixture();
        assert_eq!(c.apply_initial_settings(&prepared), None);
        assert!(c.state.self_presence.is_empty());
        assert!(!c.state.game_activity_available && !c.state.game_activity_configured);
    }
}

#[tokio::test]
async fn startup_proto_status_overrides_legacy_and_bad_account_aborts_settings() {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode([
        0x5a, 13, 0x0a, 11, 0x0a, 9, b'i', b'n', b'v', b'i', b's', b'i', b'b', b'l', b'e',
    ]);
    let prepared = prepare_session(|path| {
        std::future::ready(Ok(match path {
            "users/@me" => json!({"id":"1"}),
            "users/@me/settings" => json!({"status":"online"}),
            _ => json!({"settings":encoded}),
        }))
    })
    .await
    .unwrap();
    let (mut c, _) = Controller::test_fixture();
    assert_eq!(
        c.apply_initial_settings(&prepared).as_deref(),
        Some("invisible")
    );
    let mut requests = 0;
    assert!(
        prepare_session(|path| {
            requests += 1;
            assert_eq!(path, "users/@me");
            std::future::ready(Ok(json!({"id":"invalid"})))
        })
        .await
        .is_err()
    );
    assert_eq!(requests, 1);
}

#[tokio::test]
async fn session_transitions_preserve_explicit_mute_and_deaf() {
    for (mute, deaf) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut c, _) = Controller::test_fixture();
        c.state.self_mute = mute;
        c.state.self_deaf = deaf;
        // An invalid synthetic credential exercises reset without any HTTP task.
        c.connect("invalid".into());
        assert_eq!((c.state.self_mute, c.state.self_deaf), (mute, deaf));
        c.disconnect().await;
        assert_eq!((c.state.self_mute, c.state.self_deaf), (mute, deaf));
        assert!(c.gateway.is_none() && c.media.is_none());
    }
}

#[test]
fn session_and_navigation_guard_independent() {
    let (mut c, _) = Controller::test_fixture();
    let mut done = Completed {
        credential: None,
        generation: c.generation,
        nav: Some(c.nav),
        action: UiAction::new("fixture"),
        result: Ok(Value::Null),
    };
    assert!(c.accepts(&done));
    c.cancel_view_requests();
    assert!(!c.accepts(&done));
    done.nav = None;
    assert!(c.accepts(&done));
    c.generation += 1;
    assert!(!c.accepts(&done));
}
#[test]
fn snapshots_coalesce_to_one_latest_value() {
    let (mut c, updates) = Controller::test_fixture();
    for n in 0..1000 {
        c.state.status = n.to_string();
        c.publish();
    }
    assert!(updates.try_recv().is_err());
    c.flush_snapshot(true);
    assert_eq!(
        c.latest_snapshot.lock().unwrap().as_ref().unwrap().status,
        "999"
    );
    c.state.status = "newest".into();
    c.publish();
    c.flush_snapshot(true);
    assert_eq!(
        c.latest_snapshot.lock().unwrap().take().unwrap().status,
        "newest"
    );
    assert!(updates.try_recv().is_err());
}
#[tokio::test]
async fn navigation_cancel_does_not_cancel_global_operation() {
    let (mut c, _, mut replies) = Controller::test_fixture_with_replies();
    c.launch(UiAction::new("nav"), Some(c.nav), async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(Value::Null)
    });
    c.launch(UiAction::new("global"), None, async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(Value::Null)
    });
    c.cancel_view_requests();
    let done = tokio::time::timeout(Duration::from_secs(1), replies.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(done.action.kind, "global");
    tokio::time::sleep(Duration::from_millis(15)).await;
    assert!(replies.try_recv().is_err());
}
#[tokio::test]
async fn stale_profile_does_not_replace_new_profile() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    let old = c.next_request("profile");
    let current = c.next_request("profile");
    c.state.view = "profile".into();
    c.state.profile_user = Some(Person {
        id: "3".into(),
        name: "New".into(),
        ..Default::default()
    });
    c.state.profile_details = Some(ProfileDetails {
        user: c.state.profile_user.clone().unwrap(),
        ..Default::default()
    });
    let mut action = UiAction::id("__profile", "2");
    action.extra = old.to_string();
    c.complete(
        action,
        json!({"profile":{"user":{"id":"2","username":"old"}}}),
    )
    .await
    .unwrap();
    assert_eq!(c.state.profile_user.as_ref().unwrap().id, "3");
    assert!(c.request_current("profile", current));
}
#[tokio::test]
async fn pins_closed_before_reply_stay_closed() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    let version = c.next_request("pins");
    let mut action = UiAction::id("__pins", "20");
    action.extra = version.to_string();
    c.complete(
        action,
        json!({"items":[{"message":{"id":"5","channel_id":"20"}}]}),
    )
    .await
    .unwrap();
    assert_eq!(c.state.view, "chat");
    assert!(c.state.pinned_messages.is_empty());
}
#[tokio::test]
async fn pins_are_newest_first() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    c.state.view = "pins".into();
    let mut action = UiAction::id("__pins", "20");
    action.extra = c.next_request("pins").to_string();
    let value = json!({"items":[{"pinned_at":"2026-10-19T01:00:00Z","message":{"id":"5","channel_id":"20"}},{"pinned_at":"2026-10-20T01:00:00Z","message":{"id":"6","channel_id":"20"}}]});
    c.complete(action, value).await.unwrap();
    assert_eq!(c.state.pinned_messages[0].id, "6");
}
#[tokio::test]
async fn stale_gif_query_is_ignored() {
    let (mut c, updates) = Controller::test_fixture();
    let old = c.next_request("gifs");
    c.next_request("gifs");
    let mut action = UiAction::new("__gifs");
    action.extra = json!({"mode":"trending","limit":20,"requestVersion":old}).to_string();
    c.complete(action, json!([{"url":"https://tenor.com/view/old"}]))
        .await
        .unwrap();
    assert!(updates.try_recv().is_err());
    assert!(c.gif_cache.is_empty());
}
#[test]
fn prepared_nonce_reused_exactly() {
    let value = json!({"submissionId":"0123456789abcdef0123456789abcdef"});
    assert_eq!(prepared_nonce(&value).unwrap(), "0123456789abcdef01234567");
    for id in ["", "0123456789abcdef", "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"] {
        assert!(prepared_nonce(&json!({"submissionId":id})).is_err())
    }
}
#[test]
fn scroll_restoration_uses_around_only_for_explicit_anchor() {
    assert_eq!(restore_anchor(""), None);
    let extra = json!({"anchorMessageId":"123","atBottom":false,"anchorOffset":2.0}).to_string();
    assert_eq!(restore_anchor(&extra).as_deref(), Some("123"));
    assert_eq!(
        message_page_route("20", restore_anchor(&extra).as_deref()),
        "channels/20/messages?limit=50&around=123"
    );
    assert_eq!(
        restore_anchor(&json!({"anchorMessageId":"123","atBottom":true}).to_string()),
        None
    );
    assert!(
        restore_anchor(&json!({"anchorMessageId":"../other","atBottom":false}).to_string())
            .is_none()
    );
}
#[test]
fn gateway_deltas_preserve_edits_and_deletions_during_load() {
    let mut list = vec![message("1"), message("2")];
    apply_delta(
        &mut list,
        "MESSAGE_UPDATE",
        &json!({"id":"1","content":"new"}),
    );
    apply_delta(&mut list, "MESSAGE_DELETE", &json!({"id":"2"}));
    apply_delta(
        &mut list,
        "MESSAGE_CREATE",
        &json!({"id":"3","channel_id":"20","content":"incoming"}),
    );
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].content, "new");
    assert_eq!(list[0].previous_contents, vec!["Fixture"]);
    assert_eq!(list[1].id, "2");
    assert!(list[1].deleted);
    assert_eq!(list[2].id, "3");
}
#[tokio::test]
async fn home_clears_server_context_and_title_selection() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    c.state.selected_guild_id = "10".into();
    c.state.channels.push(NavItem {
        id: "20".into(),
        ..Default::default()
    });
    c.act(UiAction::id("view", "friends")).await.unwrap();
    assert!(c.state.selected_guild_id.is_empty());
    assert!(c.state.selected_channel_id.is_empty());
    assert!(c.state.channels.is_empty());
    assert!(c.state.messages.is_empty());
    assert!(!c.state.channel_access.can_view);
}
#[tokio::test]
async fn profile_back_preserves_pins_context() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    c.state.view = "profile".into();
    c.selected_before_profile = "pins".into();
    c.state.pinned_messages.push(message("5"));
    c.act(UiAction::new("return-chat")).await.unwrap();
    assert_eq!(c.state.view, "pins");
    assert_eq!(c.state.messages[0].id, "5");
}
#[tokio::test]
async fn activity_delta_is_applied_without_promoting_absent_fields() {
    let (mut c, _) = Controller::test_fixture();
    c.state.game_activity_enabled = false;
    c.dispatch(
        "USER_SETTINGS_PROTO_UPDATE",
        json!({"settings":{"type":1,"proto":""},"partial":true}),
    )
    .await
    .unwrap();
    assert!(!c.state.game_activity_enabled);
    c.dispatch("USER_SETTINGS_PROTO_UPDATE",json!({"settings":{"type":1,"proto":notdiscord_core::settings_proto::visibility_delta(true)},"partial":true})).await.unwrap();
    assert!(c.state.game_activity_enabled);
    c.dispatch(
        "SELF_PRESENCE",
        json!({"status":"invisible","show_current_game":false}),
    )
    .await
    .unwrap();
    assert!(!c.state.game_activity_enabled);
    assert_eq!(c.state.self_presence, "invisible");
}
#[tokio::test]
async fn own_forwarded_edit_is_rejected_before_any_request() {
    let (mut c, _) = Controller::test_fixture();
    selected(&mut c);
    c.api = Some(Api::new("synthetic-credential-never-sent".into()).unwrap());
    let mut m = message("4");
    m.author.id = "1".into();
    m.forwarded = true;
    c.cache.upsert(m);
    let result = c
        .act(UiAction {
            id: "4".into(),
            text: "new".into(),
            channel_id: "20".into(),
            ..UiAction::new("edit")
        })
        .await;
    assert!(result.unwrap_err().contains("reenviados"));
}
