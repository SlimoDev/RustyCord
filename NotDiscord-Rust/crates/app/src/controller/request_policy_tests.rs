//! Pure fixtures and synthetic controller state only. No API request, login,
//! browser or device is started by these regressions.
use super::*;

#[tokio::test]
async fn sidebar_fetches_only_missing_components_and_preserves_empty_complete_lists() {
    for (seed, expected) in [
        (json!({"guilds":[],"dms":[],"relationships":[]}), vec![]),
        (
            json!({"guilds":null,"dms":[],"relationships":[]}),
            vec!["users/@me/guilds?limit=200"],
        ),
        (
            Value::Null,
            vec![
                "users/@me/guilds?limit=200",
                "users/@me/channels",
                "users/@me/relationships",
            ],
        ),
    ] {
        let calls = std::cell::RefCell::new(Vec::new());
        let value = request_policy::load_sidebar(seed, |path| {
            calls.borrow_mut().push(path);
            std::future::ready(Ok(json!([])))
        })
        .await
        .unwrap();
        assert_eq!(calls.into_inner(), expected);
        assert!(
            value["guilds"].is_array()
                && value["dms"].is_array()
                && value["relationships"].is_array()
        );
    }
    assert!(
        request_policy::load_sidebar(Value::Null, |_| std::future::ready(Ok(
            json!({"not":"a list"})
        )))
        .await
        .is_err()
    );
}

#[tokio::test]
async fn sidebar_fallback_replays_concurrent_creates_updates_and_removals() {
    let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
    controller.sidebar_inflight = true;
    controller.sidebar_seed = json!({"guilds":[],"dms":[],"relationships":[]});
    controller
        .dispatch(
            "CHANNEL_CREATE",
            json!({"id":"20","type":3,"recipients":[{"id":"42","username":"fixture"}]}),
        )
        .await
        .unwrap();
    controller
        .dispatch(
            "CHANNEL_UPDATE",
            json!({"id":"20","type":3,"name":"New name"}),
        )
        .await
        .unwrap();
    controller
        .dispatch("RELATIONSHIP_REMOVE", json!({"id":"43"}))
        .await
        .unwrap();
    controller.complete(UiAction::new("__sidebar"),json!({"guilds":[],"dms":[],"relationships":[{"type":1,"user":{"id":"43","username":"removed"}}]})).await.unwrap();
    assert_eq!(controller.state.direct_messages.len(), 1);
    assert_eq!(controller.state.direct_messages[0].kind, "group");
    assert_eq!(controller.state.direct_messages[0].name, "New name");
    assert_eq!(controller.sidebar_seed["dms"][0]["name"], "New name");
    assert_eq!(
        controller.sidebar_seed["dms"][0]["recipients"][0]["id"],
        "42"
    );
    assert!(controller.state.relationships.is_empty());
    assert!(controller.sidebar_deltas.is_empty() && !controller.sidebar_inflight);
    assert!(
        replies.try_recv().is_err(),
        "Fixture reconciliation must not launch HTTP jobs"
    );
}

#[test]
fn a_new_ready_rejects_an_old_catalog_completion_and_secrets_are_not_kept_in_deltas() {
    let (controller, _, _) = Controller::test_fixture_with_replies();
    let done = Completed {
        generation: controller.generation,
        nav: None,
        action: UiAction {
            kind: "__sidebar".into(),
            extra: controller.sidebar_epoch.wrapping_add(1).to_string(),
            ..Default::default()
        },
        result: Ok(Value::Null),
        credential: None,
    };
    assert!(!controller.accepts(&done));
    let delta=request_policy::catalog_delta("CHANNEL_CREATE",&json!({"id":"20","type":1,"token":"synthetic-private","recipients":[{"id":"42","username":"fixture","email":"synthetic-private"}]})).unwrap();
    assert!(!delta.to_string().contains("synthetic-private"));
    assert!(
        request_policy::catalog_delta("GUILD_DELETE", &json!({"id":"10","unavailable":true}))
            .is_none()
    );
}

#[tokio::test]
async fn member_demand_tracks_real_context_and_navigation_clears_it_without_jobs() {
    let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
    controller.state.signed_in = true;
    controller.state.view = "chat".into();
    controller.state.selected_guild_id = "10".into();
    controller.state.selected_channel_id = "20".into();
    controller.state.channel_access.can_view_members = true;
    let mut visible = UiAction {
        kind: "member-demand".into(),
        id: "20".into(),
        channel_id: "20".into(),
        extra: "10".into(),
        text: "visible".into(),
    };
    controller.act(visible.clone()).await.unwrap();
    assert_eq!(controller.member_demand, Some(("10".into(), "20".into())));
    visible.channel_id = "21".into();
    controller.act(visible.clone()).await.unwrap();
    assert_eq!(controller.member_demand, Some(("10".into(), "20".into())));
    visible.channel_id = "20".into();
    visible.text = "hidden".into();
    controller.act(visible.clone()).await.unwrap();
    assert!(controller.member_demand.is_none());
    visible.text = "visible".into();
    controller.state.channel_access.can_view_members = false;
    controller.act(visible.clone()).await.unwrap();
    assert!(controller.member_demand.is_none());
    controller.state.channel_access.can_view_members = true;
    controller.act(visible).await.unwrap();
    let previous_context = controller.state.member_context_version;
    controller.reset_navigation();
    assert!(controller.member_demand.is_none());
    assert_ne!(controller.state.member_context_version, previous_context);
    assert!(replies.try_recv().is_err());
}

#[test]
fn reconnect_reconciles_hidden_or_replaced_member_demand_once_ready() {
    for desired in [
        (String::new(), String::new(), 99),
        ("11".into(), "21".into(), 199),
    ] {
        let old = Some(("10".into(), "20".into(), 99));
        let mut accepted = old.clone();
        // Hiding or changing panels while disconnected cannot acknowledge the
        // new intent; the last subscription may still be replayed by RESUME.
        request_policy::queue_member_subscription(&mut accepted, desired.clone(), false, |_| {
            panic!("A disconnected Gateway must not receive a command")
        });
        assert_eq!(accepted, old);
        let mut attempts = 0;
        request_policy::queue_member_subscription(&mut accepted, desired.clone(), true, |_| {
            attempts += 1;
            false
        });
        assert_eq!(accepted, old, "Full queues must leave the intent pending");
        request_policy::queue_member_subscription(
            &mut accepted,
            desired.clone(),
            true,
            |command| {
                attempts += 1;
                let GatewayCommand::SubscribeGuild {
                    guild_id,
                    channel_id,
                    range_end,
                } = command
                else {
                    panic!("Only a member subscription may be reconciled")
                };
                assert_eq!((guild_id, channel_id, range_end), desired);
                true
            },
        );
        assert_eq!(accepted, Some(desired.clone()));
        request_policy::queue_member_subscription(&mut accepted, desired, true, |_| {
            panic!("Subsequent ticks must not repeat an accepted intent")
        });
        assert_eq!(attempts, 2);
    }
}

#[tokio::test]
async fn typing_preference_is_local_and_login_json_cannot_import_a_secret() {
    let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
    assert!(
        controller.send_typing,
        "Legacy settings preserve typing choice"
    );
    let mut action = UiAction::new("ui-settings");
    action.text = json!({"SendTyping":false}).to_string();
    controller.act(action).await.unwrap();
    assert!(!controller.send_typing);
    assert!(replies.try_recv().is_err());
    assert!(
        controller
            .complete(
                UiAction::new("__credential"),
                json!({"credential":"synthetic-never-sent-secret"})
            )
            .await
            .is_err()
    );
    assert!(controller.api.is_none() && controller.pending_credential.is_none());
}

#[tokio::test]
async fn logout_reports_failed_session_removal_after_disconnect() {
    use std::os::windows::fs::OpenOptionsExt;
    let (mut controller, _, _) = Controller::test_fixture_with_replies();
    controller.state.signed_in = true;
    let path = controller.data.join("session.dpapi");
    std::fs::write(&path, b"synthetic-not-a-session").unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    assert!(controller.act(UiAction::new("logout")).await.is_err());
    assert!(!controller.state.signed_in && controller.api.is_none());
    assert!(path.exists());
    drop(held);
    controller.act(UiAction::new("logout")).await.unwrap();
    assert!(!path.exists());
}

#[tokio::test]
async fn reconnect_catalog_preserves_guild_badges_and_private_channels_stay_out_of_guild_acl() {
    let (mut controller, _, _) = Controller::test_fixture_with_replies();
    controller.state.selected_guild_id = "10".into();
    controller.state.guilds = vec![NavItem {
        id: "10".into(),
        unread: true,
        mention_count: 3,
        is_muted: true,
        ..Default::default()
    }];
    controller
        .channels
        .insert("21".into(), json!({"id":"21","guild_id":"10","type":0}));
    controller.complete(UiAction::new("__sidebar"),json!({"guilds":[{"id":"10","name":"Fixture","permissions":"0"}],"dms":[{"id":"20","type":1,"recipients":[{"id":"42","username":"fixture"}]}],"relationships":[]})).await.unwrap();
    assert_eq!(controller.channels.len(), 1);
    assert!(controller.channels.contains_key("21"));
    assert!(controller.state.guilds[0].unread && controller.state.guilds[0].is_muted);
    assert_eq!(controller.state.guilds[0].mention_count, 3);
    assert_eq!(controller.state.direct_messages[0].id, "20");
}

#[tokio::test]
async fn catalog_delta_overflow_stops_instead_of_silently_discarding_updates() {
    let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
    controller.api = Some(Api::new("synthetic-never-sent-catalog-credential".into()).unwrap());
    controller.sidebar_inflight = true;
    controller.sidebar_seed = json!({"guilds":[],"dms":[],"relationships":[]});
    for index in 0..256 {
        controller.dispatch("CHANNEL_CREATE",json!({"id":(index+100).to_string(),"type":1,"recipients":[{"id":"42","username":"fixture"}]})).await.unwrap();
    }
    assert!(
        controller
            .dispatch(
                "CHANNEL_CREATE",
                json!({"id":"9999","type":1,"recipients":[{"id":"42","username":"fixture"}]})
            )
            .await
            .is_err()
    );
    assert_eq!(
        controller.api.as_ref().unwrap().safety().state(),
        SafetyState::SynchronizationRequired
    );
    assert_eq!(controller.sidebar_deltas.len(), 256);
    assert!(
        replies.try_recv().is_err(),
        "Synthetic events do not create HTTP jobs"
    );
}

#[tokio::test]
async fn a_partial_new_guild_requires_one_selective_fallback_and_keeps_existing_capabilities() {
    let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
    controller.sidebar_seed = json!({"guilds":[{"id":"10","name":"Existing","permissions":"33"}],"dms":[],"relationships":[]});
    controller.state.guilds = controller.sidebar_guilds(&controller.sidebar_seed["guilds"]);
    controller
        .dispatch("GUILD_CREATE", json!({"id":"11","name":"New but partial"}))
        .await
        .unwrap();
    assert!(controller.sidebar_seed["guilds"].is_null());
    assert!(controller.sidebar_wait.is_some());
    assert!(controller.state.guilds[0].can_invite && controller.state.guilds[0].can_manage);
    let calls = std::cell::RefCell::new(Vec::new());
    let reconciled=request_policy::load_sidebar(controller.sidebar_seed.clone(),|path|{
        calls.borrow_mut().push(path);std::future::ready(Ok(json!([{"id":"10","name":"Existing","permissions":"33"},{"id":"11","name":"New but complete","permissions":"1"}])))
    }).await.unwrap();
    assert_eq!(calls.into_inner(), vec!["users/@me/guilds?limit=200"]);
    controller
        .complete(UiAction::new("__sidebar"), reconciled)
        .await
        .unwrap();
    assert_eq!(controller.state.guilds.len(), 2);
    assert!(controller.state.guilds[1].can_invite);
    assert!(replies.try_recv().is_err());
}
