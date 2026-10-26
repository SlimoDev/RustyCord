use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};

fn policy() -> Policy {
    Policy {
        connect: Duration::from_millis(80),
        hello: Duration::from_millis(60),
        ready: Duration::from_millis(80),
        write: Duration::from_millis(60),
        stable: Duration::from_secs(60),
        backoff: Duration::from_millis(3),
        identify_gap: Duration::from_millis(5),
        rate_cooldown: Duration::from_millis(50),
        min_heartbeat: Duration::from_millis(5),
        attempts: 2,
    }
}
fn envelope(command: GatewayCommand) -> Envelope {
    Envelope {
        command,
        done: None,
        game_epoch: 0,
    }
}
fn voice() -> GatewayCommand {
    GatewayCommand::VoiceState {
        guild_id: None,
        channel_id: Some("123".into()),
        mute: true,
        deaf: false,
        video: false,
    }
}
async fn loopback(
    listener: &TcpListener,
) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
    let (tcp, peer) = listener.accept().await.unwrap();
    assert!(peer.ip().is_loopback());
    tokio_tungstenite::accept_async(tcp).await.unwrap()
}
async fn hello(socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) {
    socket
        .send(Message::Text(
            json!({"op":10,"d":{"heartbeat_interval":20}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
}
async fn packet(socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = message {
            return serde_json::from_str(&text).unwrap();
        }
    }
}
async fn handshake(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    op: i64,
) {
    hello(socket).await;
    let auth = packet(socket).await;
    assert_eq!(auth["op"], op);
    assert_eq!(auth["d"]["token"], "offline-synthetic-credential-only");
}
async fn ready(socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) {
    socket.send(Message::Text(json!({"op":0,"s":7,"t":"READY","d":{"session_id":"synthetic-session","user":{"id":"42"}}}).to_string().into())).await.unwrap();
}
fn runner(
    listener: &TcpListener,
    output_capacity: usize,
    test_policy: Policy,
) -> (
    tokio::task::JoinHandle<()>, SafetyGate, CancellationToken, Arc<AtomicBool>,
    mpsc::Receiver<GatewayEvent>, mpsc::Sender<Envelope>,
) {
    runner_with_game_policy(listener,output_capacity,test_policy,Arc::new(GamePresencePolicy::default()))
}
fn runner_with_game_policy(
    listener: &TcpListener,
    output_capacity: usize,
    test_policy: Policy,
    game_policy: Arc<GamePresencePolicy>,
) -> (
    tokio::task::JoinHandle<()>,
    SafetyGate,
    CancellationToken,
    Arc<AtomicBool>,
    mpsc::Receiver<GatewayEvent>,
    mpsc::Sender<Envelope>,
) {
    let (commands, rx) = mpsc::channel(32);
    let (events, output) = mpsc::channel(output_capacity);
    let gate = SafetyGate::default();
    let cancel = CancellationToken::new();
    let is_ready = Arc::new(AtomicBool::new(false));
    let task = tokio::spawn(run(
        Zeroizing::new("offline-synthetic-credential-only".into()),
        None,
        rx,
        events,
        cancel.clone(),
        is_ready.clone(),
        gate.clone(),
        format!("ws://{}/", listener.local_addr().unwrap()),
        test_policy,
        game_policy,
    ));
    (task, gate, cancel, is_ready, output, commands)
}

#[test]
fn closes_preserve_resume_reidentify_and_terminal_decisions() {
    assert_eq!(close_action(4000), CloseAction::Resume);
    assert_eq!(close_action(4999), CloseAction::Resume);
    for code in [1000, 1001, 4007, 4009] {
        assert_eq!(close_action(code), CloseAction::Fresh);
    }
    assert_eq!(close_action(4008), CloseAction::RateLimited);
    assert_eq!(
        close_action(4004),
        CloseAction::Terminal(SafetyState::AuthenticationRequired)
    );
    for code in 4010..=4014 {
        assert_eq!(
            close_action(code),
            CloseAction::Terminal(SafetyState::ProtocolUnsupported)
        );
    }
}
#[test]
fn resumed_subscription_retains_the_unsubscribe_even_if_the_selection_changed_in_queue() {
    let mut wire = SubscriptionState::default();
    let open = GatewayCommand::SubscribeGuild {
        guild_id: "1".into(),
        channel_id: "2".into(),
        range_end: 99,
    };
    let hide = GatewayCommand::SubscribeGuild {
        guild_id: String::new(),
        channel_id: String::new(),
        range_end: 99,
    };
    wire.payload(&open).unwrap();
    wire.sent(&open);
    let pending_hide = wire.payload(&hide).unwrap();
    assert_eq!(pending_hide["d"]["subscriptions"]["1"]["activities"], false);
    assert_eq!(
        wire.payload(&hide).unwrap(),
        pending_hide,
        "A lost flush must retain the same unsubscribe for RESUME"
    );
    wire.sent(&hide);
    assert_eq!(
        wire.payload(&hide).unwrap(),
        pending_hide,
        "Even after a local flush, the peer has not acknowledged the new state"
    );
    let fresh = SubscriptionState::default().payload(&open).unwrap();
    assert_eq!(fresh["d"]["subscriptions"].as_object().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn budget_reserves_health_capacity_and_enforces_presence_window() {
    let mut budget = Budget::default();
    for _ in 0..5 {
        assert_eq!(budget.user_at(true), Instant::now());
        budget.record(true, true);
    }
    assert_eq!(
        budget.user_at(true),
        Instant::now() + Duration::from_secs(20)
    );
    assert_eq!(budget.user_at(false), Instant::now());
    for _ in 5..100 {
        budget.record(true, false);
    }
    assert_eq!(
        budget.user_at(false),
        Instant::now() + Duration::from_secs(60)
    );
    for _ in 0..20 {
        assert!(budget.control_allowed());
        budget.record(false, false);
    }
    assert!(!budget.control_allowed());
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(budget.user_at(true), Instant::now());
    assert!(budget.control_allowed());
}
#[tokio::test(start_paused = true)]
async fn identify_gap_starts_after_wire_flush_or_the_latest_possible_interrupted_flush() {
    let window = IdentifyWindow::default();
    let start = Instant::now();
    assert!(window.until(Duration::from_secs(5)).is_none());
    window.begin(Duration::from_secs(2));
    assert_eq!(
        window.until(Duration::from_secs(5)),
        Some(start + Duration::from_secs(7))
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    window.confirmed();
    assert_eq!(
        window.until(Duration::from_secs(5)),
        Some(start + Duration::from_secs(6))
    );
    window.begin(Duration::from_secs(2));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(
        window.until(Duration::from_secs(5)),
        Some(start + Duration::from_secs(8)),
        "A cancelled or timed-out write keeps its conservative latest-flush bound"
    );
}

#[tokio::test(start_paused = true)]
async fn latest_presence_is_coalesced_without_delaying_an_ordered_voice_command() {
    let mut budget = Budget::default();
    for _ in 0..5 {
        budget.record(true, true);
    }
    let mut pending = VecDeque::new();
    let (done, completion) = oneshot::channel();
    assert!(
        queue(
            &mut pending,
            Envelope {
                command: GatewayCommand::Presence {
                    status: "idle".into(),
                    activity: None
                },
                done: Some(done),
                game_epoch: 0,
            }
        )
        .is_ok()
    );
    assert!(queue(&mut pending, envelope(voice())).is_ok());
    assert!(
        queue(
            &mut pending,
            envelope(GatewayCommand::Presence {
                status: "online".into(),
                activity: None
            })
        )
        .is_ok()
    );
    assert!(completion.await.unwrap().is_err());
    assert_eq!(pending.len(), 2);
    let (index, at) = next_user(&pending, &mut budget).unwrap();
    assert_eq!(index, 0);
    assert_eq!(at, Instant::now());
    assert!(matches!(
        pending[index].command,
        GatewayCommand::VoiceState { .. }
    ));
    assert!(matches!(&pending[1].command,GatewayCommand::Presence{status,..} if status=="online"));
    for _ in 2..USER_QUEUE {
        assert!(queue(&mut pending, envelope(voice())).is_ok());
    }
    assert!(queue(&mut pending, envelope(voice())).is_err());
}

#[tokio::test(start_paused = true)]
async fn rich_and_legacy_presence_share_coalescing_and_the_presence_rate_budget() {
    let mut pending=VecDeque::new();let mut budget=Budget::default();
    for _ in 0..5 {budget.record(true,true);}
    assert!(queue(&mut pending,envelope(GatewayCommand::Presence{status:"online".into(),activity:Some("Old name".into())})).is_ok());
    assert!(queue(&mut pending,envelope(voice())).is_ok());
    let activity=crate::game_presence::GamePresence::from_name("New game").unwrap();
    assert!(queue(&mut pending,envelope(GatewayCommand::RichPresence{status:"online".into(),activity:Some(activity)})).is_ok());
    assert_eq!(pending.len(),2);
    assert!(matches!(pending[0].command,GatewayCommand::VoiceState{..}));
    assert_eq!(next_user(&pending,&mut budget),Some((0,Instant::now())));
    pending.pop_front();
    assert_eq!(next_user(&pending,&mut budget),Some((0,Instant::now()+Duration::from_secs(20))));
    assert!(queue(&mut pending,envelope(GatewayCommand::Presence{status:"idle".into(),activity:None})).is_ok());
    assert_eq!(pending.len(),1);assert!(matches!(pending[0].command,GatewayCommand::Presence{..}));
}

#[test]
fn privacy_epoch_invalidates_queued_and_cached_rich_activity_even_after_reenable() {
    let policy=GamePresencePolicy::default();let activity=crate::game_presence::GamePresence::from_name("Old synthetic game").unwrap();
    let old=GatewayCommand::RichPresence{status:"online".into(),activity:Some(activity.clone())};
    assert!(policy.check(&old,0).is_ok());
    let mut desired=ActivitySelection::Rich(Some(activity),0);let mut requested=true;
    assert_eq!(policy.withdraw(),1);assert!(policy.check(&old,0).is_err());
    assert_eq!(command_payload(&desired.command("online",Some(true),&policy),"").unwrap()["d"]["activities"],json!([]));
    policy.allow();assert_eq!(policy.current(),(2,true));policy.allow();assert_eq!(policy.current(),(2,true));
    assert!(policy.check(&old,0).is_err(),"Reenabling does not authorize a publication captured before withdrawal");
    assert!(synchronize_game_policy(&mut desired,&mut requested,&policy));
    assert_eq!(command_payload(&desired.command("online",Some(true),&policy),"").unwrap()["d"]["activities"],json!([]));
    assert!(policy.check(&old,2).is_ok());
    assert!(matches!(desired,ActivitySelection::Rich(None,2)));
}

#[tokio::test]
async fn delivery_preserves_order_and_refuses_unbounded_backlog_or_bytes() {
    let (tx, mut rx) = mpsc::channel(1);
    let gate = SafetyGate::default();
    let mut delivery = Delivery::new(tx);
    for value in 0..=PENDING_EVENTS {
        assert!(delivery.push(GatewayEvent::Status(value.to_string()), &gate));
    }
    assert!(matches!(rx.recv().await.unwrap(),GatewayEvent::Status(value) if value=="0"));
    let output = delivery.output.clone();
    delivery.send(output.reserve().await.unwrap());
    assert!(matches!(rx.recv().await.unwrap(),GatewayEvent::Status(value) if value=="1"));
    assert!(delivery.push(GatewayEvent::Status("last".into()), &gate));
    assert!(!delivery.push(GatewayEvent::Status("overflow".into()), &gate));
    assert_eq!(gate.state(), SafetyState::SynchronizationRequired);
    let (tx, _rx) = mpsc::channel(1);
    let gate = SafetyGate::default();
    let mut delivery = Delivery::new(tx);
    assert!(delivery.push(GatewayEvent::Status("occupy".into()), &gate));
    assert!(!delivery.push(
        GatewayEvent::Dispatch {
            kind: "bounded".into(),
            data: json!({"body":"x".repeat(PENDING_EVENT_BYTES)})
        },
        &gate
    ));
    assert_eq!(gate.state(), SafetyState::SynchronizationRequired);
}

struct StalledSink;
impl Sink<Message> for StalledSink {
    type Error = ();
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Pending
    }
    fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), ()> {
        panic!("A blocked sink must not reach start_send")
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Pending
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }
}
#[tokio::test(start_paused = true)]
async fn a_stalled_writer_is_bounded_and_cancellable() {
    let safety = SafetyGate::default();
    let cancel = CancellationToken::new();
    let mut sink = StalledSink;
    assert!(
        write(
            &mut sink,
            Message::Text("fixture".into()),
            &safety,
            &cancel,
            Duration::from_secs(2)
        )
        .await
        .is_err()
    );
    cancel.cancel();
    assert!(
        write(
            &mut sink,
            Message::Text("fixture".into()),
            &safety,
            &cancel,
            Duration::from_secs(2)
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn a_stalled_ui_consumer_does_not_stop_heartbeats_or_ack_processing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (task, gate, cancel, is_ready, _output, _commands) = runner(&listener, 1, policy());
    let mut socket = loopback(&listener).await;
    handshake(&mut socket, 2).await;
    ready(&mut socket).await;
    for _ in 0..5 {
        let heartbeat = packet(&mut socket).await;
        assert_eq!(heartbeat["op"], 1);
        socket
            .send(Message::Text(json!({"op":11}).to_string().into()))
            .await
            .unwrap();
    }
    assert!(is_ready.load(Ordering::Acquire));
    assert_eq!(gate.state(), SafetyState::Allowed);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn continuous_incoming_data_cannot_starve_voice_commands_or_heartbeat_acks() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (task, gate, cancel, is_ready, _output, commands) = runner(&listener, 32, policy());
    let mut socket = loopback(&listener).await;
    handshake(&mut socket, 2).await;
    ready(&mut socket).await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while !is_ready.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let (mut sink, mut source) = socket.split();
    let (acks, mut ack_rx) = mpsc::channel(8);
    let flood_cancel = CancellationToken::new();
    let stop = flood_cancel.clone();
    let flood = tokio::spawn(async move {
        let message = Message::Text(
            json!({"op":0,"t":"UNRETAINED_FIXTURE_EVENT","d":{"payload":"x".repeat(256)}})
                .to_string()
                .into(),
        );
        loop {
            let next = if ack_rx.try_recv().is_ok() {
                Message::Text(json!({"op":11}).to_string().into())
            } else {
                message.clone()
            };
            tokio::select! {biased;_=stop.cancelled()=>break,result=sink.send(next)=>if result.is_err(){break;}}
        }
    });
    let (done, sent) = oneshot::channel();
    commands
        .send(Envelope {
            command: voice(),
            done: Some(done),
            game_epoch: 0,
        })
        .await
        .unwrap();
    let mut voice_seen = false;
    let mut heartbeat_count = 0;
    tokio::time::timeout(Duration::from_millis(500), async {
        while !voice_seen || heartbeat_count < 3 {
            let received = source.next().await.unwrap().unwrap();
            if let Message::Text(text) = received {
                let value: Value = serde_json::from_str(&text).unwrap();
                match value["op"].as_i64() {
                    Some(4) => voice_seen = true,
                    Some(1) => {
                        heartbeat_count += 1;
                        acks.send(()).await.unwrap();
                    }
                    _ => {}
                }
            }
        }
    })
    .await
    .unwrap();
    assert!(sent.await.unwrap().is_ok());
    assert_eq!(gate.state(), SafetyState::Allowed);
    assert!(is_ready.load(Ordering::Acquire));
    flood_cancel.cancel();
    cancel.cancel();
    let _ = flood.await;
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn continuously_ready_writer_input_cannot_starve_an_eligible_wire_flush() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/", listener.local_addr().unwrap());
    let client =
        tokio::spawn(async move { tokio_tungstenite::connect_async(url).await.unwrap().0 });
    let mut peer = loopback(&listener).await;
    let socket = client.await.unwrap();
    let (sink, _source) = socket.split();
    let (_controls, control_rx) = mpsc::channel(CONTROL_QUEUE);
    let (users, user_rx) = mpsc::channel(1024);
    users.send(envelope(voice())).await.unwrap();
    for index in 0..1000 {
        users
            .send(envelope(GatewayCommand::Presence {
                status: "online".into(),
                activity: Some(format!("Synthetic pending {index}")),
            }))
            .await
            .unwrap();
    }
    let cancel = CancellationToken::new();
    let safety = SafetyGate::default();
    let task = tokio::spawn(writer(
        sink,
        control_rx,
        user_rx,
        safety.clone(),
        cancel.clone(),
        policy(),
        Arc::new(Mutex::new(SubscriptionState::default())),
        Arc::new(IdentifyWindow::default()),
        Arc::new(GamePresencePolicy::default()),
    ));
    let first = packet(&mut peer).await;
    assert_eq!(first["op"], 4);
    assert!(
        users.capacity() < 1024,
        "An eligible voice command must flush before draining a continuously ready stream of coalescible input"
    );
    assert_eq!(safety.state(), SafetyState::Allowed);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn ping_has_one_automatic_pong_and_safety_then_closes_the_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut test_policy = policy();
    test_policy.hello = Duration::from_secs(2);
    let (task, gate, _cancel, _ready, _output, _commands) = runner(&listener, 8, test_policy);
    let mut socket = loopback(&listener).await;
    socket.send(Message::Ping(vec![7, 8].into())).await.unwrap();
    let pong = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .unwrap();
    assert!(matches!(pong,Some(Ok(Message::Pong(bytes))) if bytes.as_ref()==[7,8]));
    gate.block(SafetyState::VerificationRequired);
    let next = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .unwrap();
    assert!(
        matches!(next, Some(Ok(Message::Close(_)))),
        "No duplicate Pong may hide the close frame: {next:?}"
    );
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn missing_heartbeat_ack_reconnects_with_resume_and_last_sequence() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (task, gate, cancel, is_ready, _output, _commands) = runner(&listener, 32, policy());
    let mut first = loopback(&listener).await;
    handshake(&mut first, 2).await;
    ready(&mut first).await;
    assert_eq!(packet(&mut first).await["op"], 1);
    let mut second = loopback(&listener).await;
    hello(&mut second).await;
    let resume = packet(&mut second).await;
    assert_eq!(resume["op"], 6);
    assert_eq!(resume["d"]["seq"], 7);
    assert_eq!(resume["d"]["session_id"], "synthetic-session");
    second
        .send(Message::Text(
            json!({"op":0,"t":"RESUMED","s":8,"d":{}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while !is_ready.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(gate.state(), SafetyState::Allowed);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn resumed_requires_a_hello_and_a_preexisting_resume_handshake() {
    for fresh_identified in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (task, gate, _cancel, is_ready, mut output, _commands) =
            runner(&listener, 32, policy());
        let mut socket = loopback(&listener).await;
        if fresh_identified {
            handshake(&mut socket, 2).await;
        }
        socket
            .send(Message::Text(
                json!({"op":0,"t":"RESUMED","s":8,"d":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(gate.state(), SafetyState::SynchronizationRequired);
        assert!(!is_ready.load(Ordering::Acquire));
        while let Ok(event) = output.try_recv() {
            assert!(!matches!(event,GatewayEvent::Status(status) if status=="En línea"));
        }
    }
}
async fn next_presence(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) -> Value {
    loop {
        let value = packet(socket).await;
        if value["op"] == 3 {
            return value;
        }
        assert_eq!(value["op"], 1);
        socket
            .send(Message::Text(json!({"op":11}).to_string().into()))
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn fresh_ready_restores_only_requested_activity_and_respects_current_official_status_and_visibility()
 {
    for visible in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (task, gate, cancel, is_ready, _output, commands) = runner(&listener, 32, policy());
        let mut first = loopback(&listener).await;
        handshake(&mut first, 2).await;
        ready(&mut first).await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while !is_ready.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let (done, sent) = oneshot::channel();
        commands
            .send(Envelope {
                command: GatewayCommand::Presence {
                    status: "online".into(),
                    activity: Some("Synthetic Game".into()),
                },
                done: Some(done),
                game_epoch: 0,
            })
            .await
            .unwrap();
        let presence = next_presence(&mut first).await;
        assert_eq!(presence["d"]["activities"][0]["name"], "Synthetic Game");
        assert!(sent.await.unwrap().is_ok());
        first
            .close(Some(CloseFrame {
                code: CloseCode::from(4007),
                reason: "Synthetic fresh-session fixture".into(),
            }))
            .await
            .unwrap();
        let mut second = loopback(&listener).await;
        handshake(&mut second, 2).await;
        second.send(Message::Text(json!({"op":0,"s":8,"t":"READY","d":{"session_id":"fresh-synthetic-session","user":{"id":"42"},"user_settings":{"status":"invisible"},"user_settings_proto":settings_proto::visibility_delta(visible)}}).to_string().into())).await.unwrap();
        let restored = next_presence(&mut second).await;
        assert_eq!(
            restored["d"]["status"], "invisible",
            "The current official status takes precedence over the old requested status"
        );
        assert_eq!(
            restored["d"]["activities"].as_array().unwrap().len(),
            usize::from(visible)
        );
        if visible {
            assert_eq!(restored["d"]["activities"][0]["name"], "Synthetic Game");
        }
        assert_eq!(gate.state(), SafetyState::Allowed);
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn rich_metadata_survives_fresh_ready_and_live_status_changes_clear_hidden_activity() {
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (task,gate,cancel,is_ready,_output,commands)=runner(&listener,32,policy());
    let game=crate::game_presence::GamePresence::from_rpc("424004941485572097","RPCS3",&json!({"details":"Synthetic game","state":"Playing","assets":{"large_image":"351371005538729000"},"timestamps":{"start":1_700_000_000u64}})).unwrap();
    let mut first=loopback(&listener).await;handshake(&mut first,2).await;ready(&mut first).await;
    tokio::time::timeout(Duration::from_secs(1),async{while !is_ready.load(Ordering::Acquire){tokio::task::yield_now().await;}}).await.unwrap();
    commands.send(envelope(GatewayCommand::RichPresence{status:"online".into(),activity:Some(game.clone())})).await.unwrap();
    assert_eq!(next_presence(&mut first).await["d"]["activities"],json!([game.payload().unwrap()]));
    first.close(Some(CloseFrame{code:CloseCode::from(4007),reason:"Synthetic fresh session".into()})).await.unwrap();
    let mut second=loopback(&listener).await;handshake(&mut second,2).await;
    second.send(Message::Text(json!({"op":0,"s":8,"t":"READY","d":{"session_id":"fresh-synthetic-session","user":{"id":"42"},"user_settings":{"status":"idle"},"user_settings_proto":settings_proto::visibility_delta(true)}}).to_string().into())).await.unwrap();
    let restored=next_presence(&mut second).await;assert_eq!(restored["d"]["status"],"idle");assert_eq!(restored["d"]["activities"],json!([game.payload().unwrap()]));
    second.send(Message::Text(json!({"op":0,"s":9,"t":"USER_SETTINGS_UPDATE","d":{"status":"invisible"}}).to_string().into())).await.unwrap();
    let hidden=next_presence(&mut second).await;assert_eq!(hidden["d"]["status"],"invisible");assert_eq!(hidden["d"]["activities"],json!([]));
    second.send(Message::Text(json!({"op":0,"s":10,"t":"USER_SETTINGS_UPDATE","d":{"status":"online"}}).to_string().into())).await.unwrap();
    assert_eq!(next_presence(&mut second).await["d"]["activities"],json!([game.payload().unwrap()]));
    assert_eq!(gate.state(),SafetyState::Allowed);cancel.cancel();tokio::time::timeout(Duration::from_secs(2),task).await.unwrap().unwrap();
}

#[tokio::test]
async fn clearing_during_reconnect_withdraws_cached_rich_activity_before_ready_and_rejects_pre_clear_work() {
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();let shared=Arc::new(GamePresencePolicy::default());
    let mut test_policy=policy();test_policy.backoff=Duration::from_millis(40);
    let(task,gate,cancel,is_ready,_output,commands)=runner_with_game_policy(&listener,32,test_policy,shared.clone());
    let handle=GatewayHandle{inner:Arc::new(Inner{commands:commands.clone(),cancel:cancel.clone(),ready:is_ready.clone(),safety:gate.clone(),game_presence:shared})};
    let old=crate::game_presence::GamePresence::from_rpc("424004941485572097","RPCS3",&json!({"details":"Old synthetic game"})).unwrap();
    let captured_epoch=handle.game_presence_epoch();
    let mut first=loopback(&listener).await;handshake(&mut first,2).await;ready(&mut first).await;
    tokio::time::timeout(Duration::from_secs(1),async{while !is_ready.load(Ordering::Acquire){tokio::task::yield_now().await;}}).await.unwrap();
    handle.enqueue(GatewayCommand::RichPresence{status:"online".into(),activity:Some(old.clone())}).unwrap();
    assert_eq!(next_presence(&mut first).await["d"]["activities"][0]["details"],"Old synthetic game");
    first.close(Some(CloseFrame{code:CloseCode::from(4007),reason:"Synthetic reconnect privacy change".into()})).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1),async{while is_ready.load(Ordering::Acquire){tokio::task::yield_now().await;}}).await.unwrap();
    handle.enqueue_clear_game_presence("online").unwrap();assert!(!handle.is_ready());
    let mut second=loopback(&listener).await;handshake(&mut second,2).await;ready(&mut second).await;
    let restored=next_presence(&mut second).await;assert_eq!(restored["d"]["activities"],json!([]));
    assert!(handle.enqueue(GatewayCommand::RichPresence{status:"online".into(),activity:Some(old.clone())}).is_err());
    handle.allow_game_presence().unwrap();
    assert!(handle.send_rich_presence_for_epoch("online".into(),Some(old),captured_epoch).await.is_err());
    let fresh=crate::game_presence::GamePresence::from_name("Fresh synthetic game").unwrap();
    handle.enqueue(GatewayCommand::RichPresence{status:"online".into(),activity:Some(fresh.clone())}).unwrap();
    assert_eq!(next_presence(&mut second).await["d"]["activities"],json!([fresh.payload().unwrap()]));
    assert_eq!(gate.state(),SafetyState::Allowed);cancel.cancel();tokio::time::timeout(Duration::from_secs(2),task).await.unwrap().unwrap();
}

async fn fresh_session_after(code: u16) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (task, gate, cancel, _ready, _output, _commands) = runner(&listener, 32, policy());
    let mut first = loopback(&listener).await;
    handshake(&mut first, 2).await;
    ready(&mut first).await;
    first
        .close(Some(CloseFrame {
            code: CloseCode::from(code),
            reason: "Synthetic expired session".into(),
        }))
        .await
        .unwrap();
    let mut second = loopback(&listener).await;
    handshake(&mut second, 2).await;
    assert_eq!(gate.state(), SafetyState::Allowed);
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn invalid_sequence_starts_a_fresh_session() {
    fresh_session_after(4007).await;
}
#[tokio::test]
async fn expired_session_starts_a_fresh_session() {
    fresh_session_after(4009).await;
}

#[tokio::test]
async fn ready_then_immediate_failure_does_not_reset_the_reconnection_budget() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (task, gate, _cancel, is_ready, _output, _commands) = runner(&listener, 32, policy());
    for attempt in 0..2 {
        let mut socket = loopback(&listener).await;
        handshake(&mut socket, if attempt == 0 { 2 } else { 6 }).await;
        if attempt == 0 {
            ready(&mut socket).await;
        } else {
            socket
                .send(Message::Text(
                    json!({"op":0,"s":8,"t":"RESUMED","d":{}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
        }
        socket
            .close(Some(CloseFrame {
                code: CloseCode::from(4000),
                reason: "Offline flapping fixture".into(),
            }))
            .await
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(gate.state(), SafetyState::SynchronizationRequired);
    assert!(!is_ready.load(Ordering::Acquire));
}

#[tokio::test]
async fn no_hello_and_no_ready_are_bounded_without_a_real_account() {
    for send_hello in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (task, gate, _cancel, _ready, _output, _commands) = runner(&listener, 32, policy());
        let server = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            for _ in 0..2 {
                let mut socket = loopback(&listener).await;
                clients.spawn(async move {
                    if send_hello {
                        hello(&mut socket).await;
                        while let Some(Ok(message)) = socket.next().await {
                            if let Message::Text(text) = message {
                                let value: Value = serde_json::from_str(&text).unwrap();
                                if value["op"] == 1 {
                                    socket
                                        .send(Message::Text(json!({"op":11}).to_string().into()))
                                        .await
                                        .unwrap();
                                }
                            }
                        }
                    } else {
                        std::future::pending::<()>().await;
                    }
                });
            }
            std::future::pending::<()>().await;
        });
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(gate.state(), SafetyState::SynchronizationRequired);
        server.abort();
    }
}

#[tokio::test]
async fn websocket_handshake_timeout_is_bounded_and_stops_reconnecting() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (task, gate, _cancel, _ready, _output, _commands) = runner(&listener, 32, policy());
    let server = tokio::spawn(async move {
        let mut streams = Vec::new();
        for _ in 0..2 {
            streams.push(listener.accept().await.unwrap().0);
        }
        std::future::pending::<()>().await;
    });
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(gate.state(), SafetyState::SynchronizationRequired);
    server.abort();
}
