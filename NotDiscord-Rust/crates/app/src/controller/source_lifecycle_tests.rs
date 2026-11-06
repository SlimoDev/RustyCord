//! Actor-level source transitions with synthetic bridges and an offline writer.
//! None of these fixtures opens a media process, device, socket or user account.
use super::*;

const SCREEN: &str = "call:200:300";
const OWN_SCREEN: &str = "call:200:100";

struct CallFixture {
    c: Controller,
    updates: std::sync::mpsc::Receiver<UiUpdate>,
    replies: mpsc::UnboundedReceiver<Completed>,
    main: MediaBridge,
    main_calls: Arc<Mutex<Vec<String>>>,
    wire: mpsc::Receiver<Value>,
    gateway: GatewayHandle,
    writer: tokio::task::JoinHandle<()>,
}
impl Drop for CallFixture {
    fn drop(&mut self) {
        self.gateway.stop();
        self.writer.abort();
    }
}
impl CallFixture {
    fn new() -> Self {
        let (mut c, updates, replies) = Controller::test_fixture_with_replies();
        c.state.current_user = Some(Person { id: "100".into(), name: "Synthetic self".into(), ..Default::default() });
        c.state.signed_in = true;
        c.state.notification_generation = 9;
        c.state.voice_connected = true;
        c.state.voice_encryption_ready = true;
        c.voice_channel = "200".into();
        c.voice_session = "SYNTHETIC-SESSION".into();
        c.voice_attempt = 7;
        c.voice_lifecycle.media_epoch = 3;
        c.voice_lifecycle.native_voice = Some(10);
        c.call_people.insert("100".into(), CallParticipant { user: c.state.current_user.clone().unwrap(), is_self: true, ..Default::default() });
        c.call_people.insert("300".into(), CallParticipant {
            user: Person { id: "300".into(), name: "Synthetic peer".into(), ..Default::default() },
            self_video: true, screen_sharing: true, camera_announcement_known: true,
            screen_announcement_known: true, ..Default::default()
        });
        c.state.call_videos.push(CallVideo { id: 40, user_id: "300".into(), kind: "camera".into(), active: true, status: "playing".into(), ..Default::default() });
        c.voice_lifecycle.video_sessions.insert(40, 10);
        let (main, main_calls) = MediaBridge::fake(|_, _| Ok(json!({})));
        c.media = Some(main.clone());
        let (gateway, wire, writer) = GatewayHandle::offline_recorder(SafetyGate::default());
        c.gateway = Some(gateway.clone());
        c.refresh_call();
        Self { c, updates, replies, main, main_calls, wire, gateway, writer }
    }
    fn receiver(&mut self, session: u64) -> (u64, u64) {
        let (receiver, _) = MediaBridge::fake_receiver(move |command, _| match command {
            "watchStream" => Ok(json!({"session":session})),
            "stopWatchStream" | "shutdown" | "configureReceiver" | "setVideoVisible" | "setVideoStreamVisible" => Ok(json!({})),
            _ => panic!("Unexpected synthetic receiver command: {command}"),
        }, self.main.video_frames());
        let id = receiver.fake_video_id();
        self.c.voice_lifecycle.receiver_epoch += 1;
        let epoch = self.c.voice_lifecycle.receiver_epoch;
        self.c.voice_lifecycle.receiver = Some(receiver);
        (id, epoch)
    }
    fn watching(&mut self) -> (u64, u64) {
        let (id, epoch) = self.receiver(80);
        self.c.voice_lifecycle.watch_attempt = 41;
        self.c.voice_lifecycle.watches.insert(SCREEN.into(), json!({
            "owner":"300", "attempt":41, "requested":true, "started":true,
            "nativeSession":80, "phase":"frames-received", "server":"400", "channel":"401"
        }));
        self.c.voice_lifecycle.view_intents.insert("200:300".into(), ViewIntent {
            channel: "200".into(), owner: "300".into(), kind: "screen".into(),
            attempt: 7, error: String::new(), waiting_since: None,
        });
        self.c.state.call_videos.push(CallVideo { id, user_id: "300".into(), kind: "screen".into(), stream_key: SCREEN.into(), active: true, view_requested: true, status: "playing".into(), ..Default::default() });
        self.c.voice_lifecycle.video_sessions.insert(id, 80);
        self.c.refresh_call();
        (id, epoch)
    }
    fn ended_sounds(&self) -> Vec<Value> {
        self.updates.try_iter().filter_map(|update| match update {
            UiUpdate::Data(kind, value) if kind == "sound" && value["event"] == "stream_ended" => Some(value),
            _ => None,
        }).collect()
    }
    fn voice_and_camera_survive(&self) {
        assert!(self.c.state.voice_connected && self.c.state.voice_encryption_ready);
        assert_eq!(self.c.voice_channel, "200");
        assert_eq!(self.c.voice_attempt, 7);
        assert_eq!(self.c.voice_lifecycle.native_voice, Some(10));
        assert!(self.c.media.as_ref().unwrap().same_instance(&self.main));
        assert_eq!(self.c.call_people.len(), 2);
        assert!(self.c.call_people["300"].self_video);
        let camera = self.c.state.call_videos.iter().find(|video| video.id == 40).unwrap();
        assert!(camera.active && camera.kind == "camera" && camera.status == "playing");
        assert!(!self.main_calls.lock().unwrap().iter().any(|command| matches!(command.as_str(), "disconnectVoice" | "shutdown" | "stopCamera")));
    }
    fn no_screen_controls_or_error_tile(&self) {
        assert!(!self.c.view_source_available("200", "300", "screen"));
        assert!(!self.c.state.call_videos.iter().any(|video| video.stream_key == SCREEN));
        assert!(!self.c.voice_lifecycle.watches.contains_key(SCREEN));
        assert!(!self.c.voice_lifecycle.pending_views.get("200:300").is_some_and(|view| view.kind == "screen"));
        assert!(!self.c.voice_lifecycle.view_intents.get("200:300").is_some_and(|view| view.kind == "screen"));
    }
    async fn completed(&mut self, id: &str) -> Completed {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let done = self.replies.recv().await.expect("synthetic actor completion channel closed");
                if done.action.id == id { return done; }
                assert!(self.c.accepts(&done));
                self.c.complete(done.action, done.result.unwrap()).await.unwrap();
            }
        }).await.unwrap_or_else(|_| panic!("Missing actor completion {id}"))
    }
    async fn finish(&mut self, id: &str) {
        let done = self.completed(id).await;
        assert!(self.c.accepts(&done));
        self.c.complete(done.action, done.result.unwrap()).await.unwrap();
    }
    async fn payload(&mut self, opcode: u64) -> Value {
        let value = tokio::time::timeout(Duration::from_secs(2), self.wire.recv()).await.unwrap().unwrap();
        assert_eq!(value["op"], opcode, "unexpected offline Gateway operation: {value}");
        value
    }
    async fn stop_ack(&mut self) {
        self.c.voice_dispatch("STREAM_DELETE", &json!({"stream_key":SCREEN,"reason":"user_requested"})).await.unwrap();
    }
}

#[tokio::test]
async fn explicit_publisher_false_before_or_after_delete_ends_only_screen_and_sounds_once_even_without_a_decoder() {
    for watching in [false, true] {
        for delete_first in [false, true] {
            let mut f = CallFixture::new();
            let old_receiver = watching.then(|| f.watching());
            let publication = f.c.voice_lifecycle.sources.revision(SCREEN).unwrap();
            if delete_first {
                f.c.voice_dispatch("STREAM_DELETE", &json!({"stream_key":SCREEN,"reason":"unknown"})).await.unwrap();
                assert!(f.ended_sounds().is_empty(), "a subscription DELETE alone cannot prove publisher withdrawal");
            }
            f.c.voice_dispatch("VOICE_STATE_UPDATE", &json!({"user_id":"300","channel_id":"200","self_stream":false})).await.unwrap();
            if !delete_first {
                f.c.voice_dispatch("STREAM_DELETE", &json!({"stream_key":SCREEN,"reason":"unknown"})).await.unwrap();
            }
            f.no_screen_controls_or_error_tile();
            assert!(!f.c.call_people["300"].screen_sharing);
            assert!(f.c.call_people["300"].screen_view_unavailable);
            assert_eq!(f.c.voice_lifecycle.sources.phase(SCREEN), Some(stream_sources::Phase::Ended));
            let sounds = f.ended_sounds();
            assert_eq!(sounds.len(), 1, "watching={watching}, delete_first={delete_first}");
            assert_eq!(sounds[0]["account_id"], "100");
            assert_eq!(sounds[0]["session_generation"], 9);
            assert_eq!(sounds[0]["voice_attempt"], 7);
            assert_eq!(sounds[0]["publication"], publication);
            f.c.voice_dispatch("VOICE_STATE_UPDATE", &json!({"user_id":"300","self_stream":false})).await.unwrap();
            if let Some((id, epoch)) = old_receiver {
                f.c.media_event(json!({"event":"receiver-host","epoch":epoch,"value":{"event":"videoStream","id":id,"streamKey":SCREEN,"userId":"300","kind":"screen","token":41,"state":"playing","active":true}})).await.unwrap();
            }
            assert!(f.ended_sounds().is_empty(), "duplicate metadata and stale decoder cannot repeat semantic end");
            f.no_screen_controls_or_error_tile();
            f.voice_and_camera_survive();
        }
    }
}

#[tokio::test]
async fn local_stop_watch_orders_native_and_gateway_close_without_withdrawing_the_offer_or_playing_stream_end() {
    let mut f = CallFixture::new();
    f.watching();
    let publication = f.c.voice_lifecycle.sources.revision(SCREEN);
    f.c.media_action(&UiAction::id("stop-watch-stream", SCREEN)).await.unwrap();
    assert!(f.c.call_people["300"].screen_sharing);
    assert!(f.c.view_source_available("200", "300", "screen"));
    assert!(!f.c.voice_lifecycle.watches.contains_key(SCREEN));
    assert!(!f.c.state.call_videos.iter().any(|video| video.stream_key == SCREEN));
    let release = f.payload(19).await;
    assert_eq!(release["d"]["stream_key"], SCREEN);
    f.finish("watch-gateway-stop").await;
    // The local close completion may arrive before or after the writer.
    if f.c.voice_lifecycle.receiver_closing.is_some() { f.finish("receiver").await; }
    f.stop_ack().await;
    assert!(!f.c.voice_lifecycle.watch_stops.contains_key(SCREEN));
    assert_eq!(f.c.voice_lifecycle.sources.revision(SCREEN), publication);
    assert_eq!(f.c.voice_lifecycle.sources.phase(SCREEN), Some(stream_sources::Phase::Active));
    assert!(f.c.view_source_available("200", "300", "screen"));
    assert!(f.ended_sounds().is_empty());
    f.voice_and_camera_survive();
}

#[tokio::test]
async fn a_new_publication_reusing_the_key_survives_the_old_viewer_stop_ack_and_native_completion() {
    let mut f = CallFixture::new();
    let (_, old_epoch) = f.watching();
    let old_publication = f.c.voice_lifecycle.sources.revision(SCREEN).unwrap();
    let mut old_native = UiAction::id("__voice-data", "watch-start");
    old_native.channel_id = SCREEN.into(); old_native.text = "41".into();
    let mut old_meta = f.c.meta(); old_meta["receiver"] = json!(old_epoch); old_native.extra = old_meta.to_string();
    f.c.media_action(&UiAction::id("stop-watch-stream", SCREEN)).await.unwrap();
    f.payload(19).await;
    f.finish("watch-gateway-stop").await;
    if f.c.voice_lifecycle.receiver_closing.is_some() { f.finish("receiver").await; }
    f.c.voice_dispatch("VOICE_STATE_UPDATE", &json!({"user_id":"300","self_stream":false})).await.unwrap();
    f.c.voice_dispatch("VOICE_STATE_UPDATE", &json!({"user_id":"300","self_stream":true})).await.unwrap();
    let new_publication = f.c.voice_lifecycle.sources.revision(SCREEN).unwrap();
    assert_ne!(old_publication, new_publication);
    f.receiver(81);
    let mut watch = UiAction::id("watch-stream", "300"); watch.channel_id = "200".into();
    f.c.media_action(&watch).await.unwrap();
    let attempt = f.c.voice_lifecycle.watches[SCREEN]["attempt"].as_u64().unwrap();
    assert_ne!(attempt, 41);
    assert_eq!(f.c.voice_lifecycle.watches[SCREEN]["requested"], false);
    assert!(f.wire.try_recv().is_err(), "replacement waits for the old stop acknowledgment");
    f.stop_ack().await;
    f.payload(20).await;
    f.finish("watch-gateway-start").await;
    f.c.voice_dispatch("STREAM_CREATE", &json!({"stream_key":SCREEN,"rtc_server_id":"400","rtc_channel_id":"401"})).await.unwrap();
    f.c.voice_dispatch("STREAM_SERVER_UPDATE", &json!({"stream_key":SCREEN,"endpoint":"synthetic.invalid","token":"SYNTHETIC-OFFLINE-ONLY"})).await.unwrap();
    f.finish("watch-start").await;
    f.c.voice_complete(old_native, wrapped(Err("late old receiver failure".into()))).await.unwrap();
    assert_eq!(f.c.voice_lifecycle.watches[SCREEN]["attempt"], attempt);
    assert_eq!(f.c.voice_lifecycle.watches[SCREEN]["nativeSession"], 81);
    assert_eq!(f.c.voice_lifecycle.sources.revision(SCREEN), Some(new_publication));
    assert!(f.c.call_people["300"].screen_sharing && !f.c.call_people["300"].screen_view_unavailable);
    assert_eq!(f.ended_sounds().len(), 1);
    f.voice_and_camera_survive();
}

#[tokio::test]
async fn full_or_unauthorized_subscriptions_become_a_notice_without_retry_control_or_error_video() {
    for (reason, expected_notice) in [("stream_full", "límite"), ("unauthorized", "no autorizó")] {
        let mut f = CallFixture::new(); f.watching();
        f.c.voice_dispatch("STREAM_DELETE", &json!({"stream_key":SCREEN,"reason":reason})).await.unwrap();
        f.no_screen_controls_or_error_tile();
        assert!(f.c.call_people["300"].screen_sharing, "a failed viewer subscription does not falsify publisher metadata");
        assert!(f.c.call_people["300"].screen_view_unavailable);
        assert!(f.c.state.status.contains(expected_notice), "{}", f.c.state.status);
        assert_eq!(f.c.voice_lifecycle.sources.phase(SCREEN), Some(stream_sources::Phase::Unavailable));
        assert!(f.c.voice_lifecycle.view_recovery.is_none());
        let mut retry = UiAction::id("watch-stream", "300"); retry.channel_id = "200".into();
        assert!(f.c.media_action(&retry).await.is_err());
        for _ in 0..4 { f.c.tick_screen_recovery().await.unwrap(); }
        assert!(f.wire.try_recv().is_err(), "terminal subscription rejection cannot start a polling/retry loop");
        assert!(f.ended_sounds().is_empty());
        f.voice_and_camera_survive();
    }
}

#[tokio::test]
async fn receiver_failure_recovers_the_selected_screen_once_then_removes_the_failed_control_without_touching_voice() {
    let mut f = CallFixture::new();
    let (_, epoch) = f.watching();
    f.c.media_event(json!({"event":"receiver-host","epoch":epoch,"value":{"event":"fatal","error":{"code":"HOST_PROCESS_EXIT","exitCode":-1}}})).await.unwrap();
    assert!(f.c.voice_lifecycle.view_recovery.is_some());
    assert_eq!(f.c.voice_lifecycle.sources.phase(SCREEN), Some(stream_sources::Phase::Recovering));
    assert!(!f.c.state.call_videos.iter().any(|video| video.stream_key == SCREEN));
    f.payload(19).await;
    f.finish("watch-gateway-stop").await;
    if f.c.voice_lifecycle.receiver_closing.is_some() { f.finish("receiver").await; }
    f.stop_ack().await;
    let (_, replacement_epoch) = f.receiver(82);
    f.c.tick_screen_recovery().await.unwrap();
    assert_eq!(f.payload(20).await["d"]["stream_key"], SCREEN);
    f.finish("watch-gateway-start").await;
    f.c.voice_dispatch("STREAM_CREATE", &json!({"stream_key":SCREEN,"rtc_server_id":"400","rtc_channel_id":"401"})).await.unwrap();
    f.c.voice_dispatch("STREAM_SERVER_UPDATE", &json!({"stream_key":SCREEN,"endpoint":"synthetic.invalid","token":"SYNTHETIC-OFFLINE-ONLY"})).await.unwrap();
    f.finish("watch-start").await;
    for _ in 0..4 { f.c.tick_screen_recovery().await.unwrap(); }
    assert!(f.wire.try_recv().is_err(), "maintenance cannot start a second simultaneous recovery");
    f.c.media_event(json!({"event":"receiver-host","epoch":replacement_epoch,"value":{"event":"fatal","error":{"code":"HOST_CONTROL_CLOSED"}}})).await.unwrap();
    f.no_screen_controls_or_error_tile();
    assert!(f.c.voice_lifecycle.view_recovery.is_none());
    assert_eq!(f.c.voice_lifecycle.sources.phase(SCREEN), Some(stream_sources::Phase::Unavailable));
    assert!(!f.c.state.status.is_empty());
    f.payload(19).await;
    f.finish("watch-gateway-stop").await;
    if f.c.voice_lifecycle.receiver_closing.is_some() { f.finish("receiver").await; }
    f.stop_ack().await;
    for _ in 0..4 { f.c.tick_screen_recovery().await.unwrap(); }
    assert!(f.wire.try_recv().is_err(), "exhaustion must remain quiet rather than opening another receiver");
    assert!(f.ended_sounds().is_empty(), "receiver failure is not publisher withdrawal");
    f.voice_and_camera_survive();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_share_publishes_immediately_and_waits_for_both_native_completion_and_gateway_ack_in_either_order() {
    for ack_first in [false, true] {
        let mut f = CallFixture::new();
        let (release, pending) = std::sync::mpsc::channel::<()>();
        let (main, calls) = MediaBridge::fake(move |command, _| {
            assert_eq!(command, "stopShare", "stopping a screen must preserve camera and main voice");
            pending.recv_timeout(Duration::from_secs(3)).expect("test releases its held native stop");
            Ok(json!({}))
        });
        f.main = main.clone(); f.main_calls = calls; f.c.media = Some(main);
        f.c.state.screen_sharing = true; f.c.state.camera_enabled = true;
        f.c.stream_source = "window:SYNTHETIC".into(); f.c.stream_key = OWN_SCREEN.into();
        f.c.stream_server = "400".into(); f.c.stream_started = true;
        f.c.voice_lifecycle.share_attempt = 5; f.c.voice_lifecycle.native_share = Some(90);
        f.c.state.call_videos.push(CallVideo { id: 50, user_id: "100".into(), local: true, kind: "screen".into(), stream_key: OWN_SCREEN.into(), active: true, ..Default::default() });
        f.c.state.call_videos.push(CallVideo { id: 51, user_id: "100".into(), local: true, kind: "camera".into(), active: true, ..Default::default() });
        f.c.refresh_call();
        tokio::time::timeout(Duration::from_secs(1), f.c.media_action(&UiAction::new("stop-share"))).await.expect("actor must not await native stopShare").unwrap();
        assert!(!f.c.state.screen_sharing && !f.c.state.screen_preparing && !f.c.state.screen_paused);
        assert!(f.c.stream_source.is_empty() && f.c.stream_key.is_empty());
        assert!(!f.c.state.call_videos.iter().any(|video| video.stream_key == OWN_SCREEN));
        let snapshot = f.c.latest_snapshot.lock().unwrap().clone().unwrap();
        assert!(!snapshot.screen_sharing && !snapshot.call_videos.iter().any(|video| video.stream_key == OWN_SCREEN));
        assert!(f.c.state.camera_enabled && f.c.state.call_videos.iter().any(|video| video.id == 51 && video.active));
        let token = f.c.voice_lifecycle.share_closing.as_ref().unwrap().token;
        f.c.media_action(&UiAction::new("stop-share")).await.unwrap();
        assert_eq!(f.c.voice_lifecycle.share_closing.as_ref().unwrap().token, token);
        let mut start = UiAction::id("start-share", "window:SYNTHETIC"); start.text = "{}".into();
        assert!(f.c.media_action(&start).await.unwrap_err().contains("cierre"));
        let mut stale = UiAction::id("__source-close", "share"); stale.text = (token - 1).to_string();
        f.c.complete(stale, json!({"localClosed":true,"written":true})).await.unwrap();
        assert!(f.c.voice_lifecycle.share_closing.is_some());
        if ack_first {
            f.c.voice_dispatch("STREAM_DELETE", &json!({"stream_key":OWN_SCREEN,"reason":"user_requested"})).await.unwrap();
            assert!(f.c.voice_lifecycle.share_closing.is_some(), "ACK alone cannot release a still-live native job");
        }
        release.send(()).unwrap();
        assert_eq!(f.payload(19).await["d"]["stream_key"], OWN_SCREEN);
        f.finish("share").await;
        if !ack_first {
            assert!(f.c.voice_lifecycle.share_closing.is_some(), "writer/native completion alone cannot invent a remote ACK");
            f.c.voice_dispatch("STREAM_DELETE", &json!({"stream_key":OWN_SCREEN,"reason":"user_requested"})).await.unwrap();
        }
        assert!(f.c.voice_lifecycle.share_closing.is_none());
        assert_eq!(f.main_calls.lock().unwrap().as_slice(), ["stopShare"]);
        f.voice_and_camera_survive();
    }
}
