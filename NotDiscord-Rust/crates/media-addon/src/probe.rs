//! Compiled only with native-probe. Synthetic capture and localhost UDP only;
//! no WebSocket, Discord endpoint, account token, real device, or media frames.
use crate::native::{self, ObjectRef};
use napi::{Env, JsFunction, JsObject, JsUnknown};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
const VERIFIED_FAKE_BACKEND: &str = "7c41c446a2ef29fa04d8f62a00193416cebebb562caa8a4d4626839729833607";
struct Probe {
    env: Env,
    engine: Option<ObjectRef>,
    conn: Option<ObjectRef>,
    own_stream: Option<ObjectRef>,
    udp_port: u16,
    input_callbacks: Arc<std::sync::atomic::AtomicU64>,
    synthetic_input: bool,
    driver_verified_fake_backend: bool,
    stats_sequence: u64,
    stats_pending: Option<u64>,
    stats_stopped: bool,
    done: Arc<AtomicBool>,
    report: Arc<Mutex<Value>>,
    path: PathBuf,
    stdout_only: bool,
}
fn err() -> napi::Error {
    napi::Error::from_reason("Synthetic native probe failed.")
}
/// An initialize flag alone does not authorize selecting Default or a physical
/// GUID. Require both explicit native metadata and a synthetic-only identity.
fn synthetic_input(devices: &Value) -> Option<String> {
    devices.as_array()?.iter().find_map(|device| {
        let id = device["guid"].as_str().or_else(|| device["id"].as_str())?;
        let declared = device["isFake"] == true || device["isSynthetic"] == true;
        (declared && matches!(id, "fake" | "fake_audio_capture" | "fake-audio-input" | "synthetic-audio-input"))
            .then(|| id.to_owned())
    })
}
fn driver_verified_fake(value: Option<&str>) -> bool { value == Some(VERIFIED_FAKE_BACKEND) }
fn claim_stats_token(pending: &mut Option<u64>, token: u64) -> bool {
    if *pending != Some(token) { return false; }
    *pending = None;
    true
}
fn object(own: &Rc<RefCell<Probe>>, stream: bool) -> napi::Result<JsObject> {
    let p = own.borrow();
    (if stream {p.own_stream.as_ref()} else {p.conn.as_ref()})
        .ok_or_else(err)?.object(p.env).map_err(|_| err())
}
fn engine_object(own: &Rc<RefCell<Probe>>) -> napi::Result<JsObject> {
    let p = own.borrow();
    p.engine.as_ref().ok_or_else(err)?.object(p.env).map_err(|_| err())
}
fn video_delta(active: bool) -> Value {
    json!({"streamParameters":[{"type":"video","rid":"100","ssrc":12346,"rtxSsrc":12347,
        "quality":100,"active":active,"maxBitrate":2500000,"maxFrameRate":30,
        "maxResolution":{"type":"fixed","width":640,"height":360}}],
        "encodingVideoWidth":640,"encodingVideoHeight":360,"encodingVideoFrameRate":30})
}
fn start_continuity(own: Rc<RefCell<Probe>>) -> napi::Result<()> {
    let env=own.borrow().env;
    own.borrow().report.lock().unwrap()["Stage"]=json!("inspect-synthetic-input-catalog");
    own.borrow().report.lock().unwrap()["Limitations"]=json!([
        "No DAVE peer transition or remote audibility is established by this private loopback fixture.",
        "PTT setter acceptance is not a getter of the native transmit gate."
    ]);
    let callback_own=own.clone();
    let devices=env.create_function_from_closure("nativeSyntheticInputCatalog",move |ctx| {
        let value=native::read_value(*ctx.env,ctx.get::<JsUnknown>(0)?).map_err(|_|err())?;
        let devices=if let Some(text)=value.as_str() {serde_json::from_str(text).unwrap_or(Value::Null)} else {value};
        let verified_fake=callback_own.borrow().driver_verified_fake_backend;
        // The driver hashes the actual loaded artifact before setting this
        // opt-in. That exact backend's FakeADM branch is documented by parser,
        // consumer and factory evidence; it has no physical fallback.
        let device=synthetic_input(&devices).or_else(||verified_fake.then(||"default".to_owned()));
        {
            let p=callback_own.borrow();let mut report=p.report.lock().unwrap();
            report["SyntheticInputAvailable"]=json!(device.is_some());
            report["DriverVerifiedFakeBackend"]=json!(verified_fake);
            report["InputCatalogEntries"]=json!(devices.as_array().map(Vec::len).unwrap_or(0));
            if device.is_none() {
                report["CaptureLimitation"]=json!("NO_EXPLICIT_SYNTHETIC_INPUT: microphone remains disabled; no Default, numeric index or physical GUID is selected.");
            }
        }
        if let Some(device)=device {
            let engine=engine_object(&callback_own)?;
            let count=callback_own.borrow().input_callbacks.clone();
            let meter=ctx.env.create_function_from_closure("nativeSyntheticInputCount",move |ctx| {
                // Count callback delivery only; never retain samples or arguments.
                count.fetch_add(1,Ordering::Relaxed);ctx.env.get_undefined()
            })?;
            native::call(*ctx.env,&engine,"setOnVoiceCallback",&[meter.into_unknown()]).map_err(|_|err())?;
            native::call_json(*ctx.env,&engine,"setEmitVADLevel2",vec![json!(true)]).map_err(|_|err())?;
            native::call_json(*ctx.env,&engine,"setInputVolume",vec![json!(1)]).map_err(|_|err())?;
            native::call_json(*ctx.env,&engine,"setInputDevice",vec![json!(device)]).map_err(|_|err())?;
            // Opening a gate is restricted to the positively identified fake
            // input, on the fixture's localhost-only connection.
            let conn=object(&callback_own,false)?;
            if verified_fake {
                // Only the hash-bound FakeADM opt-in may lift deafen. Output
                // remains on "disabled", volume zero, with no remote peer.
                native::call_json(*ctx.env,&conn,"setSelfDeafen",vec![json!(false)]).map_err(|_|err())?;
            }
            native::call_json(*ctx.env,&conn,"setSelfMute",vec![json!(false)]).map_err(|_|err())?;
            native::call_json(*ctx.env,&conn,"setPTTActive",vec![json!(true),json!(false),json!(false)]).map_err(|_|err())?;
            let mut p=callback_own.borrow_mut();p.synthetic_input=true;
            p.report.lock().unwrap()["SyntheticInputActivated"]=json!(true);
            p.report.lock().unwrap()["InputDevice"]=json!(if verified_fake {"hash-verified-FakeADM-logical-default"} else {"explicit-native-synthetic-entry"});
        }
        schedule_sample(callback_own.clone(),0)?;
        ctx.env.get_undefined()
    })?;
    native::call(env,&engine_object(&own)?,"getInputDevices",&[devices.into_unknown()]).map_err(|_|err())?;
    Ok(())
}
fn schedule_sample(own: Rc<RefCell<Probe>>, phase: u8) -> napi::Result<()> {
    let env=own.borrow().env;
    let callback=env.create_function_from_closure("nativeContinuitySettle",move |ctx| {
        if !own.borrow().done.load(Ordering::Acquire) {sample_stats(own.clone(),phase)?;}
        ctx.env.get_undefined()
    })?;
    let timer:JsFunction=env.get_global()?.get_named_property("setTimeout")?;
    timer.call(None,&[callback.into_unknown(),env.create_uint32(350)?.into_unknown()])?;
    Ok(())
}
fn sample_label(phase:u8)->&'static str {
    match phase {0=>"baseline",1=>"video-delta",2=>"own-stream-created",3=>"own-stream-destroyed",_=>"full-descriptor-restored"}
}
fn record_sample(own:&Rc<RefCell<Probe>>,phase:u8,method:&str,raw:Option<Value>,reason:&str)->napi::Result<()> {
    let usable=raw.as_ref().is_some_and(Value::is_object);
    let stats=match raw {Some(raw) if usable=>crate::stats::sanitize(&raw,"negotiating",false),_=>crate::stats::unavailable("negotiating",reason)};
    let p=own.borrow();let mut report=p.report.lock().unwrap();
    report["Samples"].as_array_mut().ok_or_else(err)?.push(json!({"Phase":sample_label(phase),
        "NativeStatsMethod":method,"NativeStatsObject":usable,"InputCallbacks":p.input_callbacks.load(Ordering::Relaxed),"Stats":stats}));
    Ok(())
}
fn claim_sample(own:&Rc<RefCell<Probe>>,token:u64)->bool {
    let mut p=own.borrow_mut();
    !p.done.load(Ordering::Acquire)&&claim_stats_token(&mut p.stats_pending,token)
}
fn sample_stats(own: Rc<RefCell<Probe>>, phase: u8) -> napi::Result<()> {
    let env=own.borrow().env;
    own.borrow().report.lock().unwrap()["Stage"]=json!(sample_label(phase));
    if own.borrow().stats_stopped {
        record_sample(&own,phase,"none",None,"NATIVE_STATS_SERIES_STOPPED")?;
        return advance_continuity(own,phase);
    }
    let conn=object(&own,false)?;
    let function=|name:&str|conn.get_named_property::<JsUnknown>(name).ok()
        .is_some_and(|value|value.get_type().ok()==Some(napi::ValueType::Function));
    let method=if function("getFilteredStats") {"getFilteredStats"} else if function("getStats") {"getStats"} else {"none"};
    if method=="none" {
        own.borrow_mut().stats_stopped=true;
        record_sample(&own,phase,method,None,"NATIVE_STATS_METHOD_UNAVAILABLE")?;
        return advance_continuity(own,phase);
    }
    let token={let mut p=own.borrow_mut();p.stats_sequence=p.stats_sequence.wrapping_add(1).max(1);let token=p.stats_sequence;p.stats_pending=Some(token);token};
    let stats_own=own.clone();
    let callback=env.create_function_from_closure("nativeContinuityStats",move |ctx| {
        if !claim_sample(&stats_own,token) {return ctx.env.get_undefined();}
        let mut raw=Value::Null;
        for index in 0..ctx.length.min(4) {
            let value=native::read_value(*ctx.env,ctx.get::<JsUnknown>(index)?).map_err(|_|err())?;
            let value=if let Some(text)=value.as_str() {serde_json::from_str(text).unwrap_or(Value::Null)} else {value};
            if value.is_object() && (value.get("transport").is_some()||value.get("outbound").is_some()) {raw=value;break;}
        }
        if !raw.is_object() {stats_own.borrow_mut().stats_stopped=true;}
        record_sample(&stats_own,phase,method,raw.is_object().then_some(raw),"NATIVE_STATS_SHAPE_UNAVAILABLE")?;
        advance_continuity(stats_own.clone(),phase)?;
        ctx.env.get_undefined()
    })?;
    let timeout_own=own.clone();
    let timeout=env.create_function_from_closure("nativeContinuityStatsDeadline",move |ctx| {
        if claim_sample(&timeout_own,token) {
            // The unfinished native read is not assumed cancelled. Stop the
            // entire stats series, keep later phase samples unavailable, and
            // let only this phase's owner advance the mutation sequence once.
            timeout_own.borrow_mut().stats_stopped=true;
            record_sample(&timeout_own,phase,method,None,"NATIVE_STATS_CALLBACK_TIMEOUT")?;
            advance_continuity(timeout_own.clone(),phase)?;
        }
        ctx.env.get_undefined()
    })?;
    let timer:JsFunction=env.get_global()?.get_named_property("setTimeout")?;
    timer.call(None,&[timeout.into_unknown(),env.create_uint32(800)?.into_unknown()])?;
    let args=if method=="getFilteredStats" {vec![env.create_uint32(7)?.into_unknown(),callback.into_unknown()]} else {vec![callback.into_unknown()]};
    {let p=own.borrow();let mut report=p.report.lock().unwrap();report["StatsQueries"]=json!(report["StatsQueries"].as_u64().unwrap_or(0)+1);}
    if native::call(env,&conn,method,&args).is_err()&&claim_sample(&own,token) {
        own.borrow_mut().stats_stopped=true;
        record_sample(&own,phase,method,None,"NATIVE_STATS_CALL_FAILED")?;
        advance_continuity(own,phase)?;
    }
    Ok(())
}
fn advance_continuity(own: Rc<RefCell<Probe>>, phase: u8) -> napi::Result<()> {
    let env=own.borrow().env;
    match phase {
        0=>{
            native::call_json(env,&object(&own,false)?,"setTransportOptions",vec![video_delta(true)]).map_err(|_|err())?;
            schedule_sample(own,1)
        }
        1=>{
            let stream_own=own.clone();
            let connected=env.create_function_from_closure("nativeSyntheticOwnStream",move |ctx| {
                if stream_own.borrow().done.load(Ordering::Acquire) {return ctx.env.get_undefined();}
                let error=native::read_value(*ctx.env,ctx.get::<JsUnknown>(0)?).map_err(|_|err())?;
                let info=native::read_value(*ctx.env,ctx.get::<JsUnknown>(1)?).map_err(|_|err())?;
                if !(error.is_null()||error==false||error==0||error=="") || info["protocol"]!="udp" || info["address"]!="127.0.0.1" {
                    finish(stream_own.clone(),false)?;return ctx.env.get_undefined();
                }
                let conn=object(&stream_own,true)?;
                native::call_json(*ctx.env,&conn,"setSelfMute",vec![json!(true)]).map_err(|_|err())?;
                native::call_json(*ctx.env,&conn,"setSelfDeafen",vec![json!(true)]).map_err(|_|err())?;
                stream_own.borrow().report.lock().unwrap()["OwnStreamConnected"]=json!(true);
                schedule_sample(stream_own.clone(),2)?;ctx.env.get_undefined()
            })?;
            let port=own.borrow().udp_port;
            let options=json!({"ssrc":22345,"address":"127.0.0.1","port":port,"context":"stream",
                "streamUserId":"100000000000000001","experiments":[],"qosEnabled":true,
                "streamParameters":[{"type":"audio","ssrc":22345,"rid":"","maxBitrate":64000,"soundshare":true},
                {"type":"video","rid":"100","ssrc":22346,"rtxSsrc":22347,"quality":100,"active":false}]});
            let conn=native::create_connection(env,&engine_object(&own)?,"100000000000000001",options,connected,true).map_err(|_|err())?;
            own.borrow_mut().own_stream=Some(ObjectRef::new(env,conn).map_err(|_|err())?);
            Ok(())
        }
        2=>{
            let reference=own.borrow_mut().own_stream.take().ok_or_else(err)?;
            let conn=reference.object(env).map_err(|_|err())?;
            let destroyed=native::call_json(env,&conn,"destroy",vec![]).is_ok();reference.release(env);
            own.borrow().report.lock().unwrap()["OwnStreamDestroyAccepted"]=json!(destroyed);
            if !destroyed {finish(own,false)} else {schedule_sample(own,3)}
        }
        3=>{
            let mut options=video_delta(false);
            options["streamParameters"].as_array_mut().ok_or_else(err)?.insert(0,json!({"type":"audio","ssrc":12345,"rid":"","maxBitrate":64000,"soundshare":false}));
            native::call_json(env,&object(&own,false)?,"setTransportOptions",vec![options]).map_err(|_|err())?;
            schedule_sample(own,4)
        }
        _=>{
            let p=own.borrow();let mut report=p.report.lock().unwrap();
            let samples=report["Samples"].as_array().ok_or_else(err)?;
            let capture=p.synthetic_input&&samples.len()==5&&samples.windows(2).all(|pair|
                pair[0]["InputCallbacks"].as_u64().zip(pair[1]["InputCallbacks"].as_u64()).is_some_and(|(a,b)|b>a));
            // Both fresh capture and actual native outgoing audio counters are
            // required. Missing/null fields never turn into a successful zero.
            let transmit=capture&&samples.windows(2).all(|pair|
                pair[0]["Stats"]["outbound"]["audio"]["packets"].as_f64()
                    .zip(pair[1]["Stats"]["outbound"]["audio"]["packets"].as_f64()).is_some_and(|(a,b)|b>a));
            report["NativeCaptureContinuityVerified"]=json!(capture);
            report["NativeAudioContinuityVerified"]=json!(transmit);
            report["NativeMutationSequenceCompleted"]=json!(true);
            drop(report);drop(p);
            finish(own,true)
        }
    }
}
#[cfg(test)]
mod continuity_policy_tests {
    use super::*;
    #[test]
    fn native_device_catalog_requires_explicit_fake_identity_and_metadata() {
        for device in [json!({"guid":"default","isFake":true}),json!({"id":0,"isFake":true}),
            json!({"guid":"{physical-guid}","name":"Fake audio device","isFake":true}),
            json!({"id":"fake","name":"Fake Audio Capture"}),json!({"id":"synthetic-audio-input","isFake":false})] {
            assert!(synthetic_input(&json!([device])).is_none());
        }
        assert_eq!(synthetic_input(&json!([{"id":"fake_audio_capture","isSynthetic":true}])),Some("fake_audio_capture".into()));
    }
    #[test]
    fn fake_backend_opt_in_is_exact_hash_bound_and_old_stats_callbacks_cannot_advance_twice() {
        for value in [None,Some(""),Some("1"),Some("7c41c446"),Some("7c41c446a2ef29fa04d8f62a00193416cebebb562caa8a4d4626839729833607\n")] {
            assert!(!driver_verified_fake(value));
        }
        assert!(driver_verified_fake(Some(VERIFIED_FAKE_BACKEND)));
        let mut pending=Some(3);
        assert!(claim_stats_token(&mut pending,3));assert!(!claim_stats_token(&mut pending,3));
        pending=Some(4);assert!(!claim_stats_token(&mut pending,3));assert_eq!(pending,Some(4));
        assert!(claim_stats_token(&mut pending,4));
    }
}
fn finish(probe: Rc<RefCell<Probe>>, success: bool) -> napi::Result<()> {
    let (env, done) = {
        let p = probe.borrow();
        (p.env, p.done.clone())
    };
    if done.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let callback = env.create_function_from_closure("finishSyntheticProbe", move |ctx| {
        let (mut conn, mut own_stream, mut engine, report, path, stdout_only) = {
            let mut p = probe.borrow_mut();
            (
                p.conn.take(),
                p.own_stream.take(),
                p.engine.take(),
                p.report.clone(),
                p.path.clone(),
                p.stdout_only,
            )
        };
        let mut destroyed = false;
        let mut stream_destroyed = true;
        if let Some(conn) = own_stream.take() {
            stream_destroyed = conn.object(*ctx.env).ok().is_some_and(|obj|
                native::call_json(*ctx.env, &obj, "destroy", vec![]).is_ok());
            conn.release(*ctx.env);
        }
        if let Some(conn) = conn.take() {
            if let Ok(obj) = conn.object(*ctx.env) {
                destroyed = native::call_json(*ctx.env, &obj, "destroy", vec![]).is_ok();
            }
            conn.release(*ctx.env);
        }
        if let Some(engine) = engine.take() {
            if let Ok(obj)=engine.object(*ctx.env) {
                let _=native::call_json(*ctx.env,&obj,"setInputDevice",vec![json!("disabled")]);
                let _=native::call_json(*ctx.env,&obj,"setOutputDevice",vec![json!("disabled")]);
            }
            engine.release(*ctx.env);
        }
        let mut report = report.lock().unwrap();
        report["NativeReferenceReleased"] = json!(destroyed);
        report["OwnStreamReferenceReleased"] = json!(stream_destroyed);
        report["Success"] = json!(success && destroyed && stream_destroyed);
        report["Stage"] = json!(if success && destroyed && stream_destroyed { "complete" } else { "failed" });
        if stdout_only {
            println!("{}", serde_json::to_string(&*report).unwrap());
        } else {
            let _ = std::fs::write(path, serde_json::to_vec_pretty(&*report).unwrap());
        }
        std::process::exit(if success && destroyed && stream_destroyed { 0 } else { 1 });
        #[allow(unreachable_code)]
        ctx.env.get_undefined()
    })?;
    let timer: JsFunction = env.get_global()?.get_named_property("setTimeout")?;
    timer.call(
        None,
        &[
            callback.into_unknown(),
            env.create_uint32(100)?.into_unknown(),
        ],
    )?;
    Ok(())
}
pub fn run(env: Env, engine: JsObject, directory: String) -> napi::Result<()> {
    // Explicit diagnostic-only switch. The production addon has no probe
    // exports, and the user's current validation must not create reports.
    let stdout_only = std::env::var("NOTDISCORD_PROBE_STDOUT").as_deref() == Ok("1");
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).map_err(|_| err())?;
    let path = directory.join("native-mls-probe.json");
    let done = Arc::new(AtomicBool::new(false));
    let report = Arc::new(Mutex::new(
        json!({"Success":false,"Stage":"initialize","NativeEngineLoaded":true,"NativeInitialized":false,"NativeConnectionAbi":"user-options-callback","FakeAudioCapture":true,"FakeVideoCapture":true,"InputDevice":"disabled","OutputDevice":"disabled","OutputVolume":0,"LoopbackOnly":true,"WebSocketConnections":0,"DiscordConnections":0,"RealAccount":false,"RealMicrophone":false,"RealCamera":false,"ScreenCapture":false,"KeyMaterialPersisted":false,"NativeCallback":false,"MlsKeyPackageBytes":0,"EncryptionModes":0,"DiscoveryPackets":0,"NativeMlsProbeVerified":false,"NativeMutationSequenceCompleted":false,"NativeCaptureContinuityVerified":false,"NativeAudioContinuityVerified":false,"RemoteAudibilityVerified":false,"SyntheticInputAvailable":false,"SyntheticInputActivated":false,"Samples":[]}),
    ));
    for field in ["NativeTransportFieldsValid", "NativeAudioTransportConfigured", "NativeInputModeConfigured", "NativeTransmitGateClosed"] {
        report.lock().unwrap()[field] = json!(false);
    }
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").map_err(|_| err())?;
    udp.set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|_| err())?;
    let port = udp.local_addr().map_err(|_| err())?.port();
    let closed = done.clone();
    let udp_report = report.clone();
    std::thread::spawn(move || {
        let mut bytes = [0u8; 2048];
        while !closed.load(Ordering::Relaxed) {
            if let Ok((n, peer)) = udp.recv_from(&mut bytes) {
                if peer.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
                    continue;
                }
                if n == 74 && bytes[..4] == [0, 1, 0, 70] {
                    let mut response = [0u8; 74];
                    response[..4].copy_from_slice(&[0, 2, 0, 70]);
                    response[4..8].copy_from_slice(&bytes[4..8]);
                    response[8..17].copy_from_slice(b"127.0.0.1");
                    response[72..].copy_from_slice(&peer.port().to_be_bytes());
                    let _ = udp.send_to(&response, peer);
                    let mut r = udp_report.lock().unwrap();
                    r["DiscoveryPackets"] = json!(r["DiscoveryPackets"].as_u64().unwrap_or(0) + 1);
                }
            }
        }
    });
    let timeout_done = done.clone();
    let timeout_report = report.clone();
    let timeout_path = path.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        if !timeout_done.swap(true, Ordering::SeqCst) {
            let mut r = timeout_report.lock().unwrap();
            r["Failure"] = json!("NATIVE_PROBE_TIMEOUT");
            if stdout_only {
                println!("{}", serde_json::to_string(&*r).unwrap());
            } else {
                let _ = std::fs::write(timeout_path, serde_json::to_vec_pretty(&*r).unwrap());
            }
            std::process::exit(1);
        }
    });
    let allocator = env.create_function_from_closure("allocateSyntheticImage", |ctx| {
        let width = ctx.get::<napi::JsNumber>(0)?.get_uint32()?;
        let height = ctx.get::<napi::JsNumber>(1)?.get_uint32()?;
        let len = width as usize * height as usize * 4;
        if len > 4 * 1024 * 1024 {
            return Err(err());
        }
        let mut value = ctx.env.create_object()?;
        value.set_named_property("width", width)?;
        value.set_named_property("height", height)?;
        value.set_named_property(
            "data",
            ctx.env
                .create_arraybuffer(len)?
                .into_raw()
                .into_typedarray(napi::TypedArrayType::Uint8Clamped, len, 0)?,
        )?;
        Ok(value)
    })?;
    native::call(
        env,
        &engine,
        "setImageDataAllocator",
        &[allocator.into_unknown()],
    )
    .map_err(|_| err())?;
    native::call_json(env,&engine,"initialize",vec![json!({"audioSubsystem":"experimental","logLevel":-1,"dataDirectory":directory,"logDirectory":directory,"maxLogBytes":65536,"offloadAdmControls":true,"asyncVideoInputDeviceInit":true,"useFakeAudioCapture":true,"useFakeVideoCapture":true})]).map_err(|_|err())?;
    report.lock().unwrap()["NativeInitialized"] = json!(true);
    report.lock().unwrap()["Stage"] = json!("configure-disabled-devices");
    native::call_json(env, &engine, "setInputDevice", vec![json!("disabled")])
        .map_err(|_| err())?;
    native::call_json(env, &engine, "setOutputVolume", vec![json!(0)])
        .map_err(|_| err())?;
    native::call_json(env, &engine, "setOutputDevice", vec![json!("disabled")])
        .map_err(|_| err())?;
    native::call_json(env, &engine, "setVideoInputDevice", vec![json!("disabled")])
        .map_err(|_| err())?;
    let probe = Rc::new(RefCell::new(Probe {
        env,
        engine: Some(ObjectRef::new(env, engine).map_err(|_| err())?),
        conn: None,
        own_stream: None,
        udp_port: port,
        input_callbacks: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        synthetic_input: false,
        driver_verified_fake_backend: driver_verified_fake(std::env::var("NOTDISCORD_VERIFIED_FAKE_BACKEND").ok().as_deref()),
        stats_sequence: 0,
        stats_pending: None,
        stats_stopped: false,
        done,
        report,
        path,
        stdout_only,
    }));
    let own = probe.clone();
    let connected = env.create_function_from_closure("nativeSyntheticConnection", move |ctx| {
        let error = native::read_value(*ctx.env, ctx.get::<JsUnknown>(0)?).map_err(|_| err())?;
        let info = native::read_value(*ctx.env, ctx.get::<JsUnknown>(1)?).map_err(|_| err())?;
        // These exact fields feed Host::select_protocol. An arbitrary object
        // from a native callback is not enough to prove a usable UDP result.
        let transport_valid = info["protocol"] == "udp"
            && info["address"] == "127.0.0.1"
            && info["port"].as_u64().is_some_and(|port| (1..=65535).contains(&port));
        if !(error.is_null() || error == false || error == 0 || error == "") || !transport_valid {
            finish(own.clone(), false)?;
            return ctx.env.get_undefined();
        }
        let conn = {
            let p = own.borrow();
            p.report.lock().unwrap()["NativeCallback"] = json!(true);
            p.report.lock().unwrap()["NativeTransportFieldsValid"] = json!(true);
            p.report.lock().unwrap()["Stage"] = json!("native-udp-connected");
            p.conn
                .as_ref()
                .ok_or_else(err)?
                .object(*ctx.env)
                .map_err(|_| err())?
        };
        native::call_json(*ctx.env, &conn, "setSelfMute", vec![json!(true)]).map_err(|_| err())?;
        native::call_json(*ctx.env, &conn, "setSelfDeafen", vec![json!(true)])
            .map_err(|_| err())?;
        own.borrow().report.lock().unwrap()["Stage"] = json!("configure-native-audio-transport");
        // Match the pre-DAVE production voice setup without ever opening the
        // transmit gate or enabling a capture/output device. The same settings
        // mode helper keeps this ABI check aligned with manual sensitivity.
        let settings = crate::settings::AudioSettings::default();
        let mut transport = json!({"selfMute":true,"fec":true,"packetLossRate":0.3,"qos":true,"encodingVoiceBitRate":64000,"callBitRate":600000,"callMinBitRate":16000,"callMaxBitRate":10000000,"attenuation":false,"minimumJitterBufferLevel":0,"postponeDecodeLevel":100});
        for (key, value) in settings.mode().as_object().ok_or_else(err)? {
            transport[key] = value.clone();
        }
        native::call_json(*ctx.env, &conn, "setTransportOptions", vec![transport])
            .map_err(|_| err())?;
        own.borrow().report.lock().unwrap()["NativeAudioTransportConfigured"] = json!(true);
        own.borrow().report.lock().unwrap()["Stage"] = json!("close-native-transmit-gate");
        native::call_json(*ctx.env, &conn, "setPTTActive", vec![json!(false), json!(false), json!(false)])
            .map_err(|_| err())?;
        own.borrow().report.lock().unwrap()["NativeTransmitGateClosed"] = json!(true);
        native::call_json(*ctx.env, &conn, "setTransportOptions", vec![settings.mode()])
            .map_err(|_| err())?;
        own.borrow().report.lock().unwrap()["NativeInputModeConfigured"] = json!(true);
        let modes_own = own.clone();
        let modes = ctx
            .env
            .create_function_from_closure("nativeSyntheticModes", move |ctx| {
                let values =
                    native::read_value(*ctx.env, ctx.get::<JsUnknown>(0)?).map_err(|_| err())?;
                let count = values.as_array().map(Vec::len).unwrap_or(0);
                let conn = {
                    let p = modes_own.borrow();
                    p.report.lock().unwrap()["EncryptionModes"] = json!(count);
                    p.report.lock().unwrap()["Stage"] = json!("encryption-modes");
                    p.conn
                        .as_ref()
                        .ok_or_else(err)?
                        .object(*ctx.env)
                        .map_err(|_| err())?
                };
                if count == 0 {
                    finish(modes_own.clone(), false)?;
                    return ctx.env.get_undefined();
                }
                let mode=crate::protocol::choose_encryption(&values,&values).map_err(|_|err())?;
                // Fixture-only key and codec state, sent exclusively to our
                // localhost UDP peer. No key bytes enter the report. DAVE is
                // still prepared below; no passthrough/downgrade is enabled.
                native::call_json(*ctx.env,&conn,"setTransportOptions",vec![json!({
                    "encryptionSettings":{"mode":mode,"secretKey":(0..32).collect::<Vec<u8>>()},
                    "audioEncoder":{"type":120,"name":"opus","freq":48000,"pacsize":960,"channels":1,"rate":64000},
                    "audioDecoders":[{"type":120,"name":"opus","freq":48000,"channels":2,"params":{"stereo":"1"}}]
                })]).map_err(|_|err())?;
                modes_own.borrow().report.lock().unwrap()["NativeAudioEncoderConfigured"]=json!(true);
                modes_own.borrow().report.lock().unwrap()["TransportEncryptionConfigured"]=json!(true);
                native::call_json(
                    *ctx.env,
                    &conn,
                    "prepareSecureFramesEpoch",
                    vec![json!("1"), json!(1), json!("100000000000000002")],
                )
                .map_err(|_| err())?;
                let key_own = modes_own.clone();
                let key =
                    ctx.env
                        .create_function_from_closure("nativeSyntheticMlsKey", move |ctx| {
                            let values = native::read_value(*ctx.env, ctx.get::<JsUnknown>(0)?)
                                .map_err(|_| err())?;
                            let count = values.as_array().map(Vec::len).unwrap_or(0);
                            key_own.borrow().report.lock().unwrap()["MlsKeyPackageBytes"] =
                                json!(count);
                            drop(values);
                            if count == 0 {finish(key_own.clone(), false)?;} else {
                                key_own.borrow().report.lock().unwrap()["NativeMlsProbeVerified"] = json!(true);
                                start_continuity(key_own.clone())?;
                            }
                            ctx.env.get_undefined()
                        })?;
                native::call(*ctx.env, &conn, "getMLSKeyPackage", &[key.into_unknown()])
                    .map_err(|_| err())?;
                ctx.env.get_undefined()
            })?;
        native::call(
            *ctx.env,
            &conn,
            "getEncryptionModes",
            &[modes.into_unknown()],
        )
        .map_err(|_| err())?;
        ctx.env.get_undefined()
    })?;
    let engine = probe
        .borrow()
        .engine
        .as_ref()
        .unwrap()
        .object(env)
        .map_err(|_| err())?;
    let options = json!({"ssrc":12345,"address":"127.0.0.1","port":port,"context":"default","experiments":[],"qosEnabled":true,"streamParameters":[{"type":"audio","ssrc":12345,"rid":"","maxBitrate":64000,"soundshare":false}]});
    // Inspect immediately before the call, after initialize, using the same
    // function predicate and voice-path ordering as native::create_connection.
    let function = |name: &str| engine.get_named_property::<JsUnknown>(name).ok()
        .is_some_and(|value| value.get_type().ok() == Some(napi::ValueType::Function));
    let entry = if function("createVoiceConnectionWithOptions") { "modern-factory" }
        else if function("createVoiceConnection") { "legacy-factory" }
        else if function("VoiceConnection") { "raw-constructor" }
        else { "unavailable" };
    probe.borrow().report.lock().unwrap()["NativeConnectionEntry"] = json!(entry);
    probe.borrow().report.lock().unwrap()["Stage"] = json!("constructing-connection");
    let conn = native::create_connection(env, &engine, "100000000000000001", options, connected, false)
        .map_err(|_| err())?;
    probe.borrow_mut().conn = Some(ObjectRef::new(env, conn).map_err(|_| err())?);
    probe.borrow().report.lock().unwrap()["Stage"] = json!("waiting-for-local-udp-discovery");
    Ok(())
}
