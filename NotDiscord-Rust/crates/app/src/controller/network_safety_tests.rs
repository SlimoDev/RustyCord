//! Synthetic controllers only. A stopped gate is installed before any action;
//! no test starts a real connection, account session, media host or browser.
use super::*;

const CREDENTIAL: &str = "synthetic-credential-never-sent-over-network";

fn install_blocked_session(controller: &mut Controller, state: SafetyState) {
    controller.safety_credential_key = crate::network_safety::credential_key(CREDENTIAL);
    *controller.safety_identity.lock().unwrap() = Some("42".into());
    let gate = controller.session_safety_gate();
    gate.block(state);
    controller.api = Some(Api::new_with_safety(CREDENTIAL.into(), gate).unwrap());
    controller.state.current_user = Some(Person {
        id: "42".into(),
        ..Default::default()
    });
    controller.state.signed_in = true;
    controller.state.selected_channel_id = "20".into();
    controller.state.selected_channel_kind = "dm".into();
    controller.state.view = "chat".into();
}

#[tokio::test]
async fn stopped_session_rejects_repeated_mutations_before_jobs_but_keeps_local_features() {
    let (mut controller, updates, mut replies) = Controller::test_fixture_with_replies();
    install_blocked_session(&mut controller, SafetyState::VerificationRequired);
    let message = parsing::message(
        &json!({"id":"51","channel_id":"20","content":"Synthetic cached message","author":{"id":"42"}}),
    );
    controller.cache.replace("20", vec![message.clone()]);
    controller.state.messages = vec![message].into();
    controller.pending_credential = Some(zeroize::Zeroizing::new(CREDENTIAL.into()));
    controller.state.busy = true;
    controller.friend_pending.insert("synthetic-user".into());
    for _ in 0..3 {
        for kind in [
            "send-prepared",
            "add-friend",
            "typing",
            "call-dm",
            "start-share",
            "forward",
        ] {
            let action = UiAction {
                kind: kind.into(),
                channel_id: "20".into(),
                extra: json!({"submissionId":"draft-1","revision":7}).to_string(),
                ..Default::default()
            };
            let error = controller.act(action.clone()).await.unwrap_err();
            controller.action_error(&action, error);
        }
    }
    assert!(
        replies.try_recv().is_err(),
        "Repeated clicks must not create HTTP or media jobs"
    );
    assert_eq!(controller.cache.get("20").len(), 1);
    assert_eq!(
        controller.state.messages[0].content,
        "Synthetic cached message"
    );
    assert!(controller.pending_credential.is_none() && controller.friend_pending.is_empty());
    assert!(!controller.state.busy && controller.gateway.is_none() && controller.media.is_none());
    assert_eq!(
        controller.state.network_safety,
        SafetyState::VerificationRequired
    );
    assert!(updates.try_iter().any(|update| matches!(update, UiUpdate::Data(kind, data) if kind == "send-error" && data["submissionId"] == "draft-1")));

    controller.act(UiAction::id("pin-dm", "20")).await.unwrap();
    controller.act(UiAction::new("ui-settings")).await.unwrap();
    controller
        .act(UiAction::id("invite-accept", "synthetic-invite"))
        .await
        .unwrap();
    controller
        .act(UiAction::new("open-security-help"))
        .await
        .unwrap();
    assert!(replies.try_recv().is_err(), "Local actions must stay local");
    assert_eq!(
        controller.api.as_ref().unwrap().safety().state(),
        SafetyState::VerificationRequired
    );
}

#[tokio::test]
async fn critical_state_survives_legacy_error_strings_and_a_late_login_completion() {
    let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
    install_blocked_session(&mut controller, SafetyState::VerificationRequired);
    controller.completion_error(
        &UiAction::new("__typing"),
        "Synthetic ordinary error".into(),
    );
    controller.sync_network_safety().await;
    let message = controller.state.network_safety_message.clone();
    controller.error("Another dismissible error".into());
    controller.sync_network_safety().await;
    assert_eq!(
        controller.state.network_safety,
        SafetyState::VerificationRequired
    );
    assert_eq!(controller.state.network_safety_message, message);
    controller.pending_credential = Some(zeroize::Zeroizing::new(CREDENTIAL.into()));
    assert!(
        controller
            .complete(UiAction::new("__login"), json!({"user":{"id":"42"}}))
            .await
            .is_err()
    );
    assert!(
        !controller.data.join("session.dpapi").exists(),
        "A late reply cannot save or start a stopped session"
    );
    assert!(controller.gateway.is_none() && replies.try_recv().is_err());
}

#[tokio::test]
async fn verification_is_saved_before_logout_and_known_credentials_stop_before_startup_get() {
    let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
    install_blocked_session(&mut controller, SafetyState::VerificationRequired);
    controller.disconnect().await;
    assert_eq!(
        controller
            .safety_store
            .initial_state(&crate::network_safety::credential_key(CREDENTIAL)),
        SafetyState::VerificationRequired
    );
    controller.connect(CREDENTIAL.into());
    assert_eq!(
        controller.api.as_ref().unwrap().safety().state(),
        SafetyState::VerificationRequired
    );
    assert_eq!(
        controller.state.network_safety,
        SafetyState::VerificationRequired
    );
    assert!(
        replies.try_recv().is_err(),
        "Preflight must not schedule even an identity request"
    );
    assert!(controller.gateway.is_none());
}

#[tokio::test]
async fn fatal_gateway_runtime_states_stop_without_marking_an_account_for_verification() {
    for state in [
        SafetyState::ProtocolUnsupported,
        SafetyState::SynchronizationRequired,
    ] {
        let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
        install_blocked_session(&mut controller, state);
        controller.sync_network_safety().await;
        assert_eq!(controller.state.network_safety, state);
        assert!(controller.act(UiAction::new("send")).await.is_err());
        assert_eq!(
            controller
                .safety_store
                .initial_state(&controller.safety_credential_key),
            SafetyState::Allowed,
            "A fresh session can retry a runtime failure without clearing any verification record"
        );
        assert!(
            replies.try_recv().is_err()
                && controller.gateway.is_none()
                && controller.media.is_none()
        );
    }
}

#[tokio::test]
async fn rotated_credential_identity_check_precedes_settings_and_gateway_setup() {
    let (controller, _, _) = Controller::test_fixture_with_replies();
    let original = crate::network_safety::credential_key(CREDENTIAL);
    controller
        .safety_store
        .record(&original, Some("42"), SafetyState::VerificationRequired)
        .unwrap();
    let rotated = crate::network_safety::credential_key("synthetic-new-credential-never-sent");
    let gate = SafetyGate::default();
    let requests = std::cell::RefCell::new(Vec::new());
    let result = prepare_session(|path| {
        requests.borrow_mut().push(path);
        assert_eq!(
            path, "users/@me",
            "No settings fetch may precede the local account check"
        );
        std::future::ready(
            controller
                .safety_store
                .inspect_identity(&rotated, "42", &gate)
                .map(|_| json!({"id":"42"})),
        )
    })
    .await;
    assert!(result.is_err());
    assert_eq!(requests.into_inner(), ["users/@me"]);
    assert_eq!(gate.state(), SafetyState::VerificationRequired);
    assert_eq!(
        controller.safety_store.initial_state(&rotated),
        SafetyState::VerificationRequired
    );
}

#[tokio::test]
async fn a_queued_voice_server_event_cannot_start_native_transport_after_a_stop() {
    let (mut controller, _, mut replies) = Controller::test_fixture_with_replies();
    install_blocked_session(&mut controller, SafetyState::VerificationRequired);
    let (media, calls) = MediaBridge::fake(|_, _| Ok(json!({})));
    controller.media = Some(media.clone());
    controller.voice_channel = "20".into();
    controller.voice_session = "synthetic-voice-session".into();
    controller.state.voice_connecting = true;
    controller.pending_video = true;
    controller
        .dispatch(
            "VOICE_SERVER_UPDATE",
            json!({
                "channel_id":"20", "endpoint":"synthetic.invalid", "token":"synthetic-voice-token"
            }),
        )
        .await
        .unwrap();
    assert!(controller.media.is_none());
    assert!(!controller.state.voice_connecting && !controller.pending_video);
    assert!(controller.voice_session.is_empty() && controller.voice_token.is_empty());
    assert!(
        calls.lock().unwrap().is_empty(),
        "Queued dispatches must not start native voice or capture"
    );
    assert!(
        media.command("connectVoice", json!({})).await.is_err(),
        "The existing native transport is closed"
    );
    assert!(replies.try_recv().is_err());
}

#[tokio::test]
async fn a_late_old_session_block_is_durable_after_logout_clears_actor_identity() {
    let (mut controller, _, _) = Controller::test_fixture_with_replies();
    controller.safety_credential_key = crate::network_safety::credential_key(CREDENTIAL);
    *controller.safety_identity.lock().unwrap() = Some("42".into());
    let gate = controller.session_safety_gate();
    controller.api = Some(Api::new_with_safety(CREDENTIAL.into(), gate.clone()).unwrap());
    controller.login_pending = true;
    controller.disconnect().await;
    assert!(
        !controller.login_pending,
        "Cancellation must release the explicit sign-in action"
    );
    assert!(controller.safety_credential_key.is_empty());
    assert!(controller.safety_identity.lock().unwrap().is_none());
    gate.block(SafetyState::VerificationRequired);
    assert_eq!(
        controller
            .safety_store
            .initial_state(&crate::network_safety::credential_key(CREDENTIAL)),
        SafetyState::VerificationRequired
    );
    let fixture = controller.test_data_directory.take().unwrap();
    drop(gate);
    drop(controller);
    let restarted = crate::network_safety::SafetyStore::load(&fixture.0);
    assert_eq!(
        restarted.initial_state(&crate::network_safety::credential_key(CREDENTIAL)),
        SafetyState::VerificationRequired
    );
}
