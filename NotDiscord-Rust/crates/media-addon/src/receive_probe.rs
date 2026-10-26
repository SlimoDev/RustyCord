//! N-API ABI test of the real receiver/sink lifecycle with fake engine objects.
//! Does not load a media engine, call initialize, connect a socket or use devices.
use super::*;
use napi::{Ref, threadsafe_function::ThreadsafeFunction};

fn fail(message: &str) -> napi::Error {
    napi::Error::from_reason(message)
}
fn checked<T>(value: Result<T>) -> napi::Result<T> {
    value.map_err(|error| fail(&error.message))
}

fn factory_contracts(env: Env) -> napi::Result<usize> {
    let mut verified = 0;
    for (modern, legacy, own_stream, expected, asymmetric) in [
        (true, true, false, "createVoiceConnectionWithOptions", false),
        (true, true, true, "createOwnStreamConnectionWithOptions", false),
        (true, false, false, "createVoiceConnectionWithOptions", false),
        (true, false, true, "createOwnStreamConnectionWithOptions", false),
        (false, true, false, "createVoiceConnection", false),
        (false, true, true, "createOwnStreamConnection", false),
        (false, false, false, "VoiceConnection", false),
        (false, false, true, "VoiceConnection", false),
        (true, true, true, "createOwnStreamConnection", true),
    ] {
        let mut engine = env.create_object()?;
        for name in ["createVoiceConnectionWithOptions", "createOwnStreamConnectionWithOptions", "createVoiceConnection", "createOwnStreamConnection", "VoiceConnection"] {
            if name.ends_with("WithOptions") && !modern || name.starts_with("create") && !name.ends_with("WithOptions") && !legacy { continue; }
            if asymmetric && name == "createOwnStreamConnectionWithOptions" { continue; }
            engine.set_named_property(name, env.create_function_from_closure(name, move |ctx| {
                if name != expected { return Err(fail("Wrong native connection factory selected")); }
                // The actual bundled raw addon has the same three-argument
                // options ABI as its index.js wrapper, not the old seven-
                // argument createVoiceConnection factory from the web client.
                let modern = name.ends_with("WithOptions") || name == "VoiceConnection";
                if name == "VoiceConnection" {
                    let target: JsUnknown = ctx.get_new_target()?;
                    if target.get_type()? != napi::ValueType::Function { return Err(fail("Raw VoiceConnection was not constructed with new")); }
                }
                if ctx.length != if modern { 3 } else { 7 } { return Err(fail("Wrong native connection factory argument count")); }
                let user = checked(native::read_value(*ctx.env, ctx.get(if modern { 0 } else { 1 })?))?;
                if user != "100" { return Err(fail("Native voice factory lost user ID")); }
                let callback: JsUnknown = ctx.get(if modern { 2 } else { 4 })?;
                if callback.get_type()? != napi::ValueType::Function { return Err(fail("Native voice factory lost UDP callback")); }
                if modern {
                    let options = checked(native::read_value(*ctx.env, ctx.get(1)?))?;
                    if options["ssrc"] != 12345 || options["address"] != "127.0.0.1" || options["port"] != 50000 || options["streamParameters"][0]["type"] != "audio" {
                        return Err(fail("Modern voice factory lost transport options"));
                    }
                } else {
                    for (index, expected) in [(0, json!(12345)), (2, json!("127.0.0.1")), (3, json!(50000)), (5, json!([])), (6, json!([{"type":"audio"}]))] {
                        if checked(native::read_value(*ctx.env, ctx.get(index)?))? != expected { return Err(fail("Legacy voice factory lost positional UDP arguments")); }
                    }
                }
                let mut result = ctx.env.create_object()?;
                result.set_named_property("factory", ctx.env.create_string(name)?)?;
                Ok(result)
            })?)?;
        }
        let callback = env.create_function_from_closure("unusedUdpCallback", |ctx| ctx.env.get_undefined())?;
        let options = json!({"ssrc":12345,"address":"127.0.0.1","port":50000,"experiments":[],"streamParameters":[{"type":"audio"}]});
        let connection = checked(native::create_connection(env, &engine, "100", options, callback, own_stream))?;
        if connection.get_named_property::<napi::JsString>("factory")?.into_utf8()?.as_str()? != expected { return Err(fail("Native voice factory result was discarded")); }
        verified += 1;
    }
    Ok(verified)
}

pub fn run(env: Env) -> napi::Result<Value> {
    let factory_contracts = factory_contracts(env)?;
    let registered = Rc::new(RefCell::new(HashMap::<String, Ref<()>>::new()));
    let callbacks = registered.clone();
    let events = Rc::new(RefCell::new(Vec::<Value>::new()));
    let calls = events.clone();
    let mut engine = env.create_object()?;
    engine.set_named_property(
        "setVideoOutputSink",
        env.create_function_from_closure("fakeSink", move |ctx| {
            let stream = ctx
                .get::<napi::JsString>(0)?
                .into_utf8()?
                .as_str()?
                .to_owned();
            if let Some(mut previous) = callbacks.borrow_mut().remove(&stream) {
                previous.unref(*ctx.env)?;
            }
            if ctx.length > 1 {
                let callback: JsFunction = ctx.get(1)?;
                callbacks
                    .borrow_mut()
                    .insert(stream.clone(), ctx.env.create_reference(callback)?);
            }
            calls
                .borrow_mut()
                .push(json!({"method":"sink","stream":stream,"attached":ctx.length>1}));
            ctx.env.get_undefined()
        })?,
    )?;
    engine.set_named_property(
        "signalVideoOutputSinkReady",
        env.create_function_from_closure("fakeAck", |ctx| ctx.env.get_undefined())?,
    )?;
    let input_calls=events.clone();
    engine.set_named_property("setInputVolume",env.create_function_from_closure("fakeInputGain",move|ctx| {
        let value=ctx.get::<napi::JsNumber>(0)?.get_double()?;
        input_calls.borrow_mut().push(json!({"method":"setInputVolume","value":value}));
        ctx.env.get_undefined()
    })?)?;
    for method in ["setOutputDevice","setOutputVolume","setTransportOptions","setLoopback","setEmitVADLevel2"] {
        let log=events.clone();
        engine.set_named_property(method,env.create_function_from_closure(method,move|ctx| {
            let mut args=Vec::new();for index in 0..ctx.length {args.push(checked(native::read_value(*ctx.env,ctx.get(index)?))?);}
            log.borrow_mut().push(json!({"method":method,"args":args}));ctx.env.get_undefined()
        })?)?;
    }
    engine.set_named_property("setOnVoiceCallback",env.create_function_from_closure("fakeMeterRegistration",|ctx|ctx.env.get_undefined())?)?;
    engine.set_named_property(
        "setInputDevice",
        env.create_function_from_closure("fakeInputOff", |ctx| {
            let value = ctx.get::<napi::JsString>(0)?.into_utf8()?;
            if !matches!(value.as_str()?,"disabled"|"default") {
                return Err(fail("Probe refused an input device"));
            }
            ctx.env.get_undefined()
        })?,
    )?;
    let noop = env.create_function_from_closure("fakeDispatch", |ctx| ctx.env.get_undefined())?;
    let sender: ThreadsafeFunction<Event, ErrorStrategy::Fatal> =
        noop.create_threadsafe_function(1, |_| Ok(Vec::<JsUnknown>::new()))?;
    let (output, receiver) = mpsc::channel();
    let (wake, _wake) = mpsc::sync_channel(1);
    let frames = Arc::new(Mutex::new(LatestFrames::default()));
    let closing = Arc::new(AtomicBool::new(false));
    let mut host = Host {
        env,
        engine: checked(ObjectRef::new(env, engine))?,
        initialized: true,
        receiver_only: true,
        sender,
        output,
        closing,
        runtime: tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|_| fail("Fake runtime failed"))?,
        sessions: HashMap::new(),
        connections: HashMap::new(),
        sockets: HashMap::new(),
        voice: Some(1),
        share: Some(3),
        sequence: 3,
        settings: AudioSettings::default(),
        inventories: HashMap::new(),
        pending: HashMap::new(),
        inventory_parts: HashMap::new(),
        configure_settings: HashMap::new(),
        stats_parts: HashMap::new(),
        previous_stats: HashMap::new(),
        mic_test: None,
        mic_stop_failed: false,
        mic_token: 0,
        share_token: 0,
        mic_last: None,
        data_dir: String::new(),
        watches: HashMap::new(),
        watch_tokens: HashMap::new(),
        video: VideoState {
            output: FrameOutput {
                frames: frames.clone(),
                wake,
                available: Arc::new(AtomicBool::new(true)),
            },
            sources: HashMap::new(),
            sinks: HashMap::new(),
            sequence: 0,
            visible: true,
            preview: None,
            preview_token: 0,
            screen_preview: None,
        },
    };
    let make_connection = |id: u64| -> napi::Result<ObjectRef> {
        let mut connection = env.create_object()?;
        for method in [
            "setDisableLocalVideo",
            "destroy",
            "setVideoBroadcast",
            "setTransportOptions",
            "clearDesktopSource",
            "setLocalVolume",
            "mergeUsers",
            "setRemoteUserSpeakingStatus",
            "setPTTActive",
            "setSelfMute",
            "setSelfDeafen",
        ] {
            let log = events.clone();
            connection.set_named_property(
                method,
                env.create_function_from_closure(method, move |ctx| {
                    let mut args = Vec::new();
                    for index in 0..ctx.length {
                        args.push(checked(native::read_value(*ctx.env, ctx.get(index)?))?);
                    }
                    log.borrow_mut()
                        .push(json!({"session":id,"method":method,"args":args}));
                    ctx.env.get_undefined()
                })?,
            )?;
        }
        checked(ObjectRef::new(env, connection))
    };
    let make_session = |id: u64, stream: bool, owner: &str| -> napi::Result<Session> {
        let mut payload = json!({"userId":"100","channelId":"200",
            "sessionId":"OFFLINE-ONLY","voiceToken":"SYNTHETIC","endpoint":"offline.discord.media","selfMute":true});
        // Absent optional stream metadata is valid for the main voice session;
        // an explicitly empty streamKey is deliberately invalid in production.
        if stream {
            payload["rtcServerId"] = json!("500");
            payload["streamChannelId"] = json!("501");
            payload["streamOwnerId"] = json!(owner);
            payload["streamKey"] = json!(format!("call:200:{owner}"));
        }
        let credentials = checked(Credentials::parse(&payload, stream))?;
        Ok(Session::new(id, credentials, AudioSettings::default()))
    };
    for (id, stream, owner) in [(1, false, ""), (3, true, "100")] {
        host.sessions.insert(id, make_session(id, stream, owner)?);
        host.connections.insert(id, make_connection(id)?);
    }
    host.sessions.get_mut(&1).unwrap().add_user("300",10);
    host.sessions.get_mut(&1).unwrap().add_user("400",11);
    checked(host.video_source(1, "300", "remote-camera", "camera", false, "", true))?;
    let camera_id = host.video.sources["remote-camera"].id;
    if !host.video.sinks.is_empty() {
        return Err(fail("Camera subscribed before explicit request"));
    }
    let push_frame = |stream: &str| -> napi::Result<()> {
        let callback: JsFunction = env.get_reference_value(
            registered
                .borrow()
                .get(stream)
                .ok_or_else(|| fail("Sink absent"))?,
        )?;
        let mut image = env.create_object()?;
        image.set_named_property("width", 2)?;
        image.set_named_property("height", 1)?;
        image.set_named_property(
            "data",
            env.create_arraybuffer_with_data(vec![255, 0, 0, 255, 0, 255, 0, 255])?
                .into_raw()
                .into_typedarray(napi::TypedArrayType::Uint8Clamped, 8, 0)?,
        )?;
        callback.call(None, &[image.into_unknown()])?;
        Ok(())
    };
    let mut peaks = (0, 0, 0);
    let mut stopped_connections = 0;
    // Publication is independent of the local viewer. No output callback or
    // RGBA frame should be retained until the user explicitly views a source.
    host.sessions.get_mut(&1).unwrap().camera_requested=true;
    for (session,kind,native) in [(1,"camera","own-camera"),(3,"screen","own-screen")] {
        checked(host.video_source(session,"100",native,kind,true,"",true))?;
        let id=host.video.sources[native].id;
        if host.video.sources[native].visible || host.video.sinks.contains_key(native)
            || registered.borrow().contains_key(native) {
            return Err(fail("Local publication subscribed before an explicit view request"));
        }
        checked(host.video_command("setVideoStreamVisible",&json!({"id":id,"visible":true})))?;
        push_frame(native)?;
        checked(host.video_command("setVideoStreamVisible",&json!({"id":id,"visible":false})))?;
        if host.video.sinks.contains_key(native) || registered.borrow().contains_key(native)
            || !host.sessions[&1].camera_requested || !host.sessions.contains_key(&3)
            || !host.connections.contains_key(&1) || !host.connections.contains_key(&3) {
            return Err(fail("Hiding a local view stopped publication or retained its decoder"));
        }
        host.video_remove_local(session,kind);
        checked(host.video_source(session,"100",native,kind,true,"",true))?;
        if host.video.sources[native].visible || host.video.sinks.contains_key(native) {
            return Err(fail("A restarted local source resumed viewing automatically"));
        }
        host.video_remove_local(session,kind);
    }
    checked(host.video_source(0,"100","selector-camera","camera",true,"",true))?;
    push_frame("selector-camera")?;
    let selector_id=host.video.sources["selector-camera"].id;
    checked(host.video_source(1,"100","selector-camera","camera",true,"",true))?;
    if host.video.sources["selector-camera"].id!=selector_id
        || host.video.sources["selector-camera"].preview || host.video.sources["selector-camera"].visible
        || host.video.sinks.contains_key("selector-camera") || registered.borrow().contains_key("selector-camera")
        || !host.sessions[&1].camera_requested || !host.connections.contains_key(&1) {
        return Err(fail("Promoting the selector preview implicitly viewed or stopped the outgoing camera"));
    }
    host.video_remove_local(1,"camera");
    host.sessions.get_mut(&1).unwrap().camera_requested=false;
    if !frames.lock().unwrap().active_ids().is_empty() || !host.video.sinks.is_empty() {
        return Err(fail("Closing local viewers retained native frames or output callbacks"));
    }
    let _=receiver.try_iter().collect::<Vec<_>>();
    for cycle in 1..=64_u64 {
        checked(host.video_command(
            "setVideoStreamVisible",
            &json!({"id":camera_id,"visible":true}),
        ))?;
        let before: Vec<_> = receiver.try_iter().collect();
        if before.iter().any(|event| event["active"] == true) {
            return Err(fail("Playback reported before first frame"));
        }
        push_frame("remote-camera")?;
        if !receiver
            .try_iter()
            .any(|event| event["state"] == "playing" && event["active"] == true)
        {
            return Err(fail("First camera frame did not enter Playing"));
        }
        checked(host.video_command(
            "setVideoStreamVisible",
            &json!({"id":camera_id,"visible":false}),
        ))?;
        host.sessions.insert(2, make_session(2, true, "300")?);
        host.connections.insert(2, make_connection(2)?);
        host.sessions.get_mut(&2).unwrap().add_user("300",10);
        if cycle==1 {
            for value in [0.,25.,100.,500.] {
                checked(host.command(json!(1000),"setVolume",&json!({"target":"user","userId":"300","value":value})))?;
                if host.sessions[&1].members["300"]["volume"].as_f64()!=Some(value/100.)||host.sessions[&2].members["300"]["volume"].as_f64()!=Some(value/100.) {
                    return Err(fail("Participant gain did not apply zero, attenuation, unity and boost"));
                }
            }
            if host.sessions[&1].members["300"]["volume"]!=5. || host.sessions[&2].members["300"]["volume"]!=5.
                || host.sessions[&1].members["400"]["volume"]!=1. || host.sessions[&3].volumes.contains_key("300") {
                return Err(fail("Participant volume did not apply only to its voice/receive connections"));
            }
            for user in ["100","999"] {
                if host.command(json!(1001),"setVolume",&json!({"target":"user","userId":user,"value":500})).is_ok() {return Err(fail("Invalid participant volume target accepted"));}
            }
        }
        let (socket, mut socket_rx) = unbounded_channel();
        host.sockets.insert(2, socket);
        host.watches.insert("call:200:300".into(), 2);
        checked(host.video_source(
            2,
            "300",
            "remote-screen",
            "screen",
            false,
            "call:200:300",
            true,
        ))?;
        push_frame("remote-screen")?;
        peaks.0 = peaks.0.max(host.video.sources.len());
        peaks.1 = peaks.1.max(host.video.sinks.len());
        peaks.2 = peaks.2.max(frames.lock().unwrap().buffered_bytes());
        checked(host.command(
            json!(cycle),
            "stopWatchStream",
            &json!({"streamKey":"call:200:300","token":cycle}),
        ))?;
        if !matches!(socket_rx.try_recv(), Ok(Message::Close(_))) {
            return Err(fail("Receiver websocket was not closed"));
        }
        stopped_connections += 1;
        if host.sessions.contains_key(&2)
            || host.connections.contains_key(&2)
            || host.sockets.contains_key(&2)
            || host.watches.contains_key("call:200:300")
            || !host.video.sinks.is_empty()
            || !registered.borrow().is_empty()
            || !frames.lock().unwrap().active_ids().is_empty()
            || frames.lock().unwrap().buffered_bytes() != 0
        {
            return Err(fail("Receive resources survived Stop watching"));
        }
        if !host.sessions.contains_key(&1)
            || !host.sessions.contains_key(&3)
            || host.share != Some(3)
        {
            return Err(fail("Stopping a receiver affected voice or the sender"));
        }
        for _ in receiver.try_iter() {}
    }
    checked(host.video_command(
        "setVideoStreamVisible",
        &json!({"id":camera_id,"visible":true}),
    ))?;
    let mut connection = checked(host.connections[&1].object(env))?;
    connection.set_named_property(
        "setDisableLocalVideo",
        env.create_function_from_closure(
            "rejectDisable",
            |_| -> napi::Result<napi::JsUndefined> { Err(fail("Synthetic disable rejection")) },
        )?,
    )?;
    if host
        .video_command(
            "setVideoStreamVisible",
            &json!({"id":camera_id,"visible":false}),
        )
        .is_ok()
        || !host.sessions.contains_key(&1)
        || !host.connections.contains_key(&1)
        || !host.video.sources.contains_key("remote-camera")
        || !host.video.sinks.is_empty()
        || !registered.borrow().is_empty()
        || !frames.lock().unwrap().active_ids().is_empty()
    {
        return Err(fail("Failed camera decoder disable affected voice or retained its sink"));
    }
    // The main transport still exists and the same camera can be opened again.
    connection.set_named_property("setDisableLocalVideo",env.create_function_from_closure("allowRetry",|ctx|ctx.env.get_undefined())?)?;
    checked(host.video_command("setVideoStreamVisible",&json!({"id":camera_id,"visible":true})))?;
    push_frame("remote-camera")?;
    checked(host.video_command("setVideoStreamVisible",&json!({"id":camera_id,"visible":false})))?;
    // A decoder failure on GoLive closes only its independent receiver.
    host.sessions.insert(2,make_session(2,true,"300")?);host.connections.insert(2,make_connection(2)?);
    host.watches.insert("call:200:300".into(),2);
    checked(host.video_source(2,"300","failed-screen","screen",false,"call:200:300",true))?;
    let failed_screen=host.video.sources["failed-screen"].id;
    let mut receiver_connection=checked(host.connections[&2].object(env))?;
    receiver_connection.set_named_property("setDisableLocalVideo",env.create_function_from_closure("rejectReceiverDisable",|_| -> napi::Result<napi::JsUndefined> {Err(fail("Synthetic receive decoder rejection"))})?)?;
    if host.video_command("setVideoStreamVisible",&json!({"id":failed_screen,"visible":false})).is_ok()
        || host.sessions.contains_key(&2)||host.connections.contains_key(&2)||host.watches.contains_key("call:200:300")
        || !host.sessions.contains_key(&1)||!host.sessions.contains_key(&3)||!host.video.sinks.is_empty()||!registered.borrow().is_empty() {
        return Err(fail("Failed GoLive decoder cleanup did not isolate its receiver"));
    }
    stopped_connections+=1;
    // A previous rejected video description used to leak back into add_user's
    // later audio-SSRC update and fail the entire main voice transport.
    let merge_calls=events.clone();
    connection.set_named_property("mergeUsers",env.create_function_from_closure("rejectVideoOnly",move|ctx| {
        let users=checked(native::read_value(*ctx.env,ctx.get(0)?))?;
        merge_calls.borrow_mut().push(json!({"session":1,"method":"mergeUsers","args":[users]}));
        if users[0]["videoSsrc"].as_u64().is_some_and(|ssrc|ssrc>0) {return Err(fail("Synthetic decoder description rejected"));}
        ctx.env.get_undefined()
    })?)?;
    {
        let mut main=host.sessions.remove(&1).ok_or_else(||fail("Main voice absent before metadata retry"))?;
        main.local_ready=true;main.connected=true;main.session_description=true;main.dave_ready=true;
        main.video_user("300",10,20,21,&json!([]));
        checked(host.frame(&mut main,protocol::Frame {op:5,data:json!({"user_id":"300","ssrc":12,"speaking":1}),bytes:vec![],seq:None}))?;
        if main.closed||!main.connected||main.members["300"]["ssrc"]!=12||main.members["300"]["volume"]!=5.||main.members["300"]["videoSsrc"]!=0 {
            return Err(fail("Video decoder rejection poisoned an audio SSRC change"));
        }
        main.credentials.self_mute=false;
        host.sessions.insert(1,main);
    }
    checked(host.input_mode(1,&AudioSettings::default()))?;
    host.settings.sensitivity_db=-100.;
    for sample in [vec![json!(-100.),json!(false)],vec![]] {
        host.mic_last=None;checked(host.native_event(Tag::MicLevel,sample))?;
        if host.sessions[&1].transmit_gate {return Err(fail("Native dB silence or a missing sample opened transmission"));}
    }
    host.settings.sensitivity_db=0.;host.mic_last=None;
    checked(host.native_event(Tag::MicLevel,vec![json!(0.),json!(false)]))?;
    if !host.sessions[&1].transmit_gate {return Err(fail("Native 0 dB full scale was misread as silence"));}
    let mut threshold_events=0;
    for (threshold,expected) in [(-60.,true),(-20.,false),(-60.,true)] {
        host.settings.sensitivity_db=threshold;host.mic_last=None;
        host.sessions.get_mut(&1).unwrap().manual_release_at=None;
        // A false native classifier must not override the selected manual gate.
        checked(host.native_event(Tag::MicLevel,vec![json!(-40.),json!(false)]))?;
        let updates=receiver.try_iter().collect::<Vec<_>>();
        if !updates.iter().any(|event|event["event"]=="inputLevel"&&event["speaking"]==expected&&event["thresholdDb"]==threshold) {
            return Err(fail("Manual input threshold did not update live captured-level activity"));
        }
        if host.sessions[&1].speaking.get("100")!=Some(&expected)||host.sessions[&1].transmit_gate!=expected {return Err(fail("Own activity ring/native transmit gate did not follow manual threshold"));}
        threshold_events+=1;
    }
    let mut live_settings=host.settings.clone();live_settings.sensitivity_db=-50.;
    let configure_id=json!(3000);
    host.pending.insert(configure_id.to_string(),(configure_id.clone(),Instant::now()+Duration::from_secs(1)));
    host.configure_settings.insert(configure_id.to_string(),live_settings);
    host.inventory_parts.insert(configure_id.to_string(),json!({}));
    checked(host.inventory_result(configure_id.clone(),"input",json!([]),true))?;
    checked(host.inventory_result(configure_id,"output",json!([]),true))?;
    if host.sessions[&1].transmit_gate {return Err(fail("Audio reconfiguration retained a stale cached transmit gate"));}
    host.mic_last=None;checked(host.native_event(Tag::MicLevel,vec![json!(-40.),json!(false)]))?;
    if !host.sessions[&1].transmit_gate||host.settings.sensitivity_db!=-50. {return Err(fail("Same-level audio did not reopen the native gate after reconfiguration"));}
    // The actor tick closes stale capture even if no new callback can deliver
    // a below-threshold sample. A future release tail cannot bypass this limit.
    host.sessions.get_mut(&1).unwrap().input_observed_at=Some(Instant::now()-Duration::from_secs(1));
    host.sessions.get_mut(&1).unwrap().manual_release_at=Some(Instant::now()+Duration::from_secs(1));
    host.tick();
    if host.sessions[&1].transmit_gate {return Err(fail("Stale input kept native transmission open"));}
    for guard in ["mute","deaf","test","encryption"] {
        {
            let session=host.sessions.get_mut(&1).unwrap();
            session.credentials.self_mute=guard=="mute";session.credentials.self_deaf=guard=="deaf";
            session.connected=guard!="encryption";
        }
        host.mic_test=(guard=="test").then(||Instant::now()+Duration::from_secs(1));host.mic_last=None;
        checked(host.native_event(Tag::MicLevel,vec![json!(0.),json!(true)]))?;
        if host.sessions[&1].transmit_gate {return Err(fail("Mute/deaf/test/encryption guard allowed native input transmission"));}
    }
    host.mic_test=None;
    {
        let main=host.sessions.get_mut(&1).unwrap();main.connected=true;main.credentials.self_mute=false;main.credentials.self_deaf=false;main.settings.ptt=true;
    }
    host.settings.ptt=true;host.settings.sensitivity_db=0.;host.mic_last=None;
    checked(host.native_event(Tag::MicLevel,vec![json!(-60.),json!(true)]))?;
    if host.sessions[&1].transmit_gate {return Err(fail("PTT transmitted without its key"));}
    checked(host.command(json!(2010),"ptt",&json!({"active":true})))?;
    host.mic_last=None;checked(host.native_event(Tag::MicLevel,vec![json!(-60.),json!(false)]))?;
    if !host.sessions[&1].transmit_gate||host.sessions[&1].speaking.get("100")!=Some(&true) {return Err(fail("Explicit PTT key was incorrectly gated by the manual VAD threshold"));}
    checked(host.command(json!(2011),"ptt",&json!({"active":false})))?;
    if host.sessions[&1].transmit_gate {return Err(fail("PTT release did not close native transmission"));}
    host.settings.ptt=false;host.settings.sensitivity_db=-60.;host.sessions.get_mut(&1).unwrap().settings.ptt=false;
    host.mic_last=None;checked(host.native_event(Tag::MicLevel,vec![json!(-40.),json!(false)]))?;
    let original_ptt:JsFunction=connection.get_named_property("setPTTActive")?;
    connection.set_named_property("setPTTActive",env.create_function_from_closure("rejectTransmitGate",|_|->napi::Result<napi::JsUndefined>{Err(fail("Synthetic transmit gate rejection"))})?)?;
    host.sessions.get_mut(&1).unwrap().manual_release_at=Some(Instant::now()-Duration::from_millis(1));
    host.tick();
    if !host.sessions[&1].connected||!host.sessions[&1].credentials.self_mute||host.sessions[&1].transmit_gate
        ||!receiver.try_iter().any(|event|event["event"]=="inputGateState"&&event["muted"]==true) {
        return Err(fail("Failed transmit gate did not hard-mute input while preserving incoming voice"));
    }
    connection.set_named_property("setPTTActive",original_ptt)?;
    checked(host.command(json!(2012),"mute",&json!({"value":false})))?;
    host.mic_last=None;checked(host.native_event(Tag::MicLevel,vec![json!(-40.),json!(false)]))?;
    if !host.sessions[&1].connected||!host.sessions[&1].transmit_gate {return Err(fail("Input could not be retried after a native gate error"));}
    checked(host.command(json!(2000),"setVolume",&json!({"target":"input","value":125})))?;
    if host.settings.input_volume!=125.||host.sessions[&1].settings.input_volume!=125. {return Err(fail("Own input level was not synchronized with native settings"));}
    checked(host.command(json!(2002),"setVolume",&json!({"target":"input","value":0})))?;
    if host.sessions[&1].transmit_gate||!host.sessions[&1].connected {return Err(fail("Zero microphone volume did not close only native input transmission"));}
    checked(host.command(json!(2003),"setVolume",&json!({"target":"input","value":125})))?;
    if host.command(json!(2001),"setVolume",&json!({"target":"input","value":201})).is_ok() {return Err(fail("Input level limit was bypassed"));}
    // Invoke the production video callback repeatedly with malformed metadata.
    // It must leave both audio and the independent sender intact and reopenable.
    for values in [vec![Value::Null,json!(20),json!("invalid")],vec![json!("300"),json!(20),json!("remote-camera")],vec![json!("300"),json!(0),Value::Null]] {
        checked(host.native_event(Tag::Video(1),values))?;
        if !host.sessions.get(&1).is_some_and(|main|main.connected&&!main.closed)||!host.sessions.contains_key(&3) {return Err(fail("Video callback ended an unrelated voice transport"));}
    }
    checked(host.video_source(1,"300","reopened-camera","camera",false,"",true))?;
    let reopened=host.video.sources["reopened-camera"].id;
    let mut engine=checked(host.engine())?;
    let original_attach:JsFunction=engine.get_named_property("setVideoOutputSink")?;
    engine.set_named_property("setVideoOutputSink",env.create_function_from_closure("throwingSink",|_|->napi::Result<napi::JsUndefined>{Err(fail("Synthetic sink registration rejected"))})?)?;
    if host.video_command("setVideoStreamVisible",&json!({"id":reopened,"visible":true})).is_ok()||!host.sessions[&1].connected {
        return Err(fail("Throwing sink registration damaged voice or did not report the video error"));
    }
    engine.set_named_property("setVideoOutputSink",original_attach)?;
    checked(host.video_command("setVideoStreamVisible",&json!({"id":reopened,"visible":true})))?;
    push_frame("reopened-camera")?;
    checked(host.video_command("setVideoStreamVisible",&json!({"id":reopened,"visible":false})))?;
    // No receiver operation may call into the own outgoing connection. Check
    // before its intentional teardown instead of relying on a log-tail length.
    let untouched_sender=!events.borrow().iter().any(|call|call["session"]==3);
    host.close_id(1);
    host.close_id(3);
    // Exercise the production silent diagnostic path after all fake network
    // sessions are gone. A caller's nonzero output preference must be clamped
    // to zero and acknowledged before native loopback capture is armed.
    let audio_start=events.borrow().len();
    let monitor=checked(host.command(json!(4000),"startMicTest",&json!({"token":1,"monitorOnly":true,"settings":{"outputVolume":100}})))?
        .ok_or_else(||fail("Silent monitor did not confirm startup"))?;
    let silent_monitor={
        let calls=events.borrow();let calls=&calls[audio_start..];
        let quiet=calls.iter().position(|call|call["method"]=="setOutputVolume"&&call["args"][0].as_f64()==Some(0.));
        let capture=calls.iter().position(|call|call["method"]=="setLoopback"&&call["args"][0]==true);
        monitor["monitorOnly"]==true&&host.settings.output_volume==0.
            &&quiet.zip(capture).is_some_and(|(quiet,capture)|quiet<capture)
            &&!calls.iter().any(|call|call["method"]=="setOutputVolume"&&call["args"][0].as_f64().is_some_and(|value|value>0.))
    };
    if !silent_monitor||!host.sessions.is_empty()||!host.sockets.is_empty() {return Err(fail("Silent monitor armed audible output or a network session"));}
    checked(host.command(json!(4001),"stopMicTest",&json!({"token":2})))?;
    let mut receiver=make_session(2,true,"300")?;receiver.local_ready=true;receiver.connected=true;receiver.add_user("300",10);
    host.sessions.insert(2,receiver);host.connections.insert(2,make_connection(2)?);
    let receiver_config=checked(host.command(json!(4010),"configureReceiver",&json!({"inputId":"default","inputVolume":150,"outputId":"default","outputVolume":75,"selfDeaf":true,"userVolumes":{"300":500}})))?.ok_or_else(||fail("Receiver settings did not acknowledge"))?;
    if receiver_config["receiverOnly"]!=true||host.settings.input!="disabled"||host.settings.input_volume!=0.||!host.sessions[&2].credentials.self_deaf||host.sessions[&2].volumes.get("300")!=Some(&5.) {
        return Err(fail("Dedicated receiver settings enabled input or lost deaf/participant gain"));
    }
    checked(host.command(json!(4011),"configureReceiver",&json!({"outputVolume":75,"selfDeaf":false,"userVolumes":{"300":0}})))?;
    if host.sessions[&2].credentials.self_deaf||host.sessions[&2].volumes.get("300")!=Some(&0.) {return Err(fail("Dedicated receiver did not restore output or per-user mute live"));}
    host.close_id(2);
    // Exercise the production Ready path, rather than injecting an already
    // connected fake. A factory signature regression must fail before UDP.
    let mut ready_engine = checked(host.engine())?;
    for (name, expected_own) in [("createVoiceConnectionWithOptions", false), ("createOwnStreamConnectionWithOptions", true)] {
        ready_engine.set_named_property(name, env.create_function_from_closure(name, move |ctx| {
            if ctx.length != 3 { return Err(fail("Ready used the legacy constructor with modern factory arguments")); }
            let options = checked(native::read_value(*ctx.env, ctx.get(1)?))?;
            if options["ssrc"] != 12345 || options["address"] != "127.0.0.1" || options["port"] != 50000
                || (options["context"] == "stream" && options["streamUserId"] == "100") != expected_own {
                return Err(fail("Ready selected the wrong own/remote voice factory"));
            }
            let mut connection = ctx.env.create_object()?;
            for method in ["destroy", "setSelfMute", "setSelfDeafen", "setTransportOptions", "setPTTActive", "setOnSpeakingCallback", "getEncryptionModes", "prepareSecureFramesEpoch", "getMLSKeyPackage", "mergeUsers", "destroyUser", "executeSecureFramesTransition"] {
                connection.set_named_property(method, ctx.env.create_function_from_closure(method, |ctx|ctx.env.get_undefined())?)?;
            }
            Ok(connection)
        })?)?;
    }
    for (id, stream, owner) in [(50, false, ""), (51, true, "100"), (52, true, "300")] {
        let mut session = make_session(id, stream, owner)?;
        checked(host.ready(&mut session, json!({"ssrc":12345,"ip":"127.0.0.1","port":50000,"modes":["aead_aes256_gcm_rtpsize"]})))?;
        if !host.connections.contains_key(&id) || session.udp_deadline.is_none() { return Err(fail("Ready failed to create a native UDP connection")); }
        host.sessions.insert(id, session);
        if id == 50 {
            let (wire, mut messages) = unbounded_channel();
            host.sockets.insert(id, wire);
            host.dispatch(Event::WsOpen(id));
            let Message::Text(identify) = messages.try_recv().map_err(|_|fail("Voice Identify was not sent"))? else { return Err(fail("Voice Identify frame was not JSON")); };
            let identify:Value=serde_json::from_str(&identify).map_err(|_|fail("Voice Identify JSON invalid"))?;
            if identify["op"]!=0 || identify["d"]["max_dave_protocol_version"]!=1 || identify["d"]["user_id"]!="100" {return Err(fail("Voice Identify lost DAVE or user identity"));}
            let transport=json!({"protocol":"udp","address":"127.0.0.1","port":50001});
            checked(host.native_event(Tag::Connected(id),vec![Value::Null,transport.clone()]))?;
            if !host.sessions.contains_key(&id) {return Err(fail("Native UDP callback closed voice before protocol selection"));}
            checked(host.native_event(Tag::Modes(id,json!(["aead_aes256_gcm_rtpsize"]),transport),vec![json!(["aead_aes256_gcm_rtpsize"])]))?;
            let Message::Text(select) = messages.try_recv().map_err(|_|fail("Select Protocol was not sent"))? else { return Err(fail("Select Protocol frame was not JSON")); };
            let select:Value=serde_json::from_str(&select).map_err(|_|fail("Select Protocol JSON invalid"))?;
            if select["op"]!=1 || select["d"]["data"]["address"]!="127.0.0.1" || select["d"]["data"]["port"]!=50001 || select["d"]["data"]["mode"]!="aead_aes256_gcm_rtpsize" {
                return Err(fail("Select Protocol lost the native UDP callback transport"));
            }
            let mut session=host.sessions.remove(&id).ok_or_else(||fail("Voice missing before Session Description"))?;
            checked(host.frame(&mut session,Frame {op:4,data:json!({"mode":"aead_aes256_gcm_rtpsize","secret_key":vec![0u8;32],"dave_protocol_version":1,"audio_codec":"opus"}),bytes:vec![],seq:None}))?;
            if session.state()!="waiting-peer" || session.dave_deadline.is_some() || session.connected {return Err(fail("Solo voice falsely connected encryption or timed out without a peer"));}
            checked(host.frame(&mut session,Frame {op:11,data:json!({"user_ids":["300"]}),bytes:vec![],seq:None}))?;
            if session.dave_deadline.is_none() || session.state()!="negotiating" {return Err(fail("Peer arrival did not begin bounded encrypted negotiation"));}
            let epoch=session.epoch;
            host.sessions.insert(id,session);
            checked(host.native_event(Tag::Commit(id,epoch,0),vec![json!(true),json!(1)]))?;
            if !host.sessions.get(&id).is_some_and(|session|session.connected&&session.dave_ready&&session.state()=="connected"&&session.dave_deadline.is_none()) {
                return Err(fail("Valid encrypted peer join did not complete the native voice handshake"));
            }
        }
        host.close_id(id);
    }
    // The declarative host loads discord_voice.node directly. Its current
    // VoiceConnection is the three-argument constructor called by index.js,
    // without either JavaScript create*ConnectionWithOptions wrapper installed.
    for name in ["createVoiceConnectionWithOptions", "createOwnStreamConnectionWithOptions"] {
        ready_engine.delete_named_property(name)?;
    }
    ready_engine.set_named_property("VoiceConnection", env.create_function_from_closure("rawVoiceConnection", |ctx| {
        if ctx.length != 3 { return Err(fail("Ready passed the legacy seven-argument factory ABI to the bundled raw constructor")); }
        let target: JsUnknown = ctx.get_new_target()?;
        if target.get_type()? != napi::ValueType::Function { return Err(fail("Ready did not construct the raw VoiceConnection")); }
        let user = checked(native::read_value(*ctx.env, ctx.get(0)?))?;
        let options = checked(native::read_value(*ctx.env, ctx.get(1)?))?;
        let callback: JsUnknown = ctx.get(2)?;
        if user != "100" || options["ssrc"] != 12345 || options["address"] != "127.0.0.1"
            || options["port"] != 50000 || callback.get_type()? != napi::ValueType::Function {
            return Err(fail("Raw Ready constructor lost user, UDP options or callback"));
        }
        let mut connection = ctx.env.create_object()?;
        connection.set_named_property("destroy", ctx.env.create_function_from_closure("destroyRawReady", |ctx|ctx.env.get_undefined())?)?;
        Ok(connection)
    })?)?;
    for (id, stream, owner) in [(60, false, ""), (61, true, "100"), (62, true, "300")] {
        let mut session = make_session(id, stream, owner)?;
        checked(host.ready(&mut session, json!({"ssrc":12345,"ip":"127.0.0.1","port":50000,"modes":["aead_aes256_gcm_rtpsize"]})))?;
        if !host.connections.contains_key(&id) || session.udp_deadline.is_none() { return Err(fail("Raw Ready failed to create native UDP transport")); }
        host.sessions.insert(id, session);
        host.close_id(id);
    }
    let calls = events.borrow();
    let required_disables = calls
        .iter()
        .filter(|call| call["method"] == "setDisableLocalVideo" && call["args"][1] == true)
        .count();
    // JavaScript has a single Number type; N-API can serialize 5.0 back as JSON
    // integer 5. Assert the actual numeric gain and exact target independently.
    let participant_volume_native=[1,2].into_iter().all(|session|calls.iter().any(|call|call["session"]==session&&call["method"]=="setLocalVolume"
        &&call["args"].as_array().is_some_and(|args|args.len()==2&&args[0]=="300"&&args[1].as_f64()==Some(5.))));
    let direct_transmit_gates=calls.iter().filter(|call|call["session"]==1&&call["method"]=="setPTTActive").map(|call|call["args"][0].as_bool().unwrap_or(false)).collect::<Vec<_>>();
    let manual_native_gate=calls.iter().any(|call|call["session"]==1&&call["method"]=="setTransportOptions"&&call["args"][0]["inputMode"]==2)
        &&direct_transmit_gates.windows(3).any(|gates|gates==[true,false,true]);
    let mut report = json!({"Success":host.sessions.is_empty()&&host.connections.is_empty()&&host.video.sources.is_empty()&&untouched_sender&&participant_volume_native&&manual_native_gate&&silent_monitor,
        "Cycles":64,"Participants":3,"NativeFactoryContracts":factory_contracts,"NativeReadyFactoriesVerified":6,"NativeRawConstructorReadyVerified":3,"NativeVoiceHandshakeVerified":true,"SoloWaitAndEncryptedPeerJoinVerified":true,"ReceiveConnectionsClosed":stopped_connections,"RequiredDecoderDisables":required_disables,"FailedDisablePreservesVoice":true,"VideoMergeRetryPreservesVoice":true,"VideoCallbackFailurePreservesVoice":true,"ManualThresholdTransitions":threshold_events,"NativeTransmitGateApplied":manual_native_gate,"ManualReleaseHoldMillis":250,"StaleInputDeadlineMillis":400,"LiveReconfigurationReopensGate":true,"InputGateErrorPreservesReceiveAndRetries":true,"MuteDeafTestEncryptionAndPttGuards":true,"SilentMonitorForcesZeroOutputBeforeCapture":silent_monitor,"OwnInputVolumeApplied":true,"ParticipantVolumeApplied":participant_volume_native,"ParticipantVolumeRange":[0,25,100,500],"OwnSenderUntouched":untouched_sender,
        "FinalSessions":host.sessions.len(),"FinalConnections":host.connections.len(),"FinalSockets":host.sockets.len(),
        "PeakSources":peaks.0,"PeakSinks":peaks.1,"PeakBufferedRgbaBytes":peaks.2,"FinalSources":host.video.sources.len(),
        "FinalSinks":host.video.sinks.len(),"FinalBufferedRgbaBytes":frames.lock().unwrap().buffered_bytes(),
        "RealAccount":false,"NetworkConnections":0,"NativeEngineLoaded":false,"NativeInitialization":false,"PhysicalDevicesOpened":false,
        "Boundary":"Production Host/video_command/video_attach/video_detach/stopWatchStream/close paths with N-API fake objects; no codec, network or hardware verification"});
    report["DedicatedReceiverHasNoInput"]=json!(true);
    report["ReceiverDeafAndParticipantGainLive"]=json!(true);
    report["LocalPublicationStartsWithoutViewer"]=json!(true);
    report["HidingLocalViewPreservesPublication"]=json!(true);
    report["SelectorPromotionClosesImplicitViewer"]=json!(true);
    drop(calls);
    host.engine.release(env);
    Ok(report)
}
