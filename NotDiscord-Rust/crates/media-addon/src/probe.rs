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
struct Probe {
    env: Env,
    engine: Option<ObjectRef>,
    conn: Option<ObjectRef>,
    done: Arc<AtomicBool>,
    report: Arc<Mutex<Value>>,
    path: PathBuf,
    stdout_only: bool,
}
fn err() -> napi::Error {
    napi::Error::from_reason("Synthetic native probe failed.")
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
        let (mut conn, mut engine, report, path, stdout_only) = {
            let mut p = probe.borrow_mut();
            (
                p.conn.take(),
                p.engine.take(),
                p.report.clone(),
                p.path.clone(),
                p.stdout_only,
            )
        };
        let mut destroyed = false;
        if let Some(conn) = conn.take() {
            if let Ok(obj) = conn.object(*ctx.env) {
                destroyed = native::call_json(*ctx.env, &obj, "destroy", vec![]).is_ok();
            }
            conn.release(*ctx.env);
        }
        if let Some(engine) = engine.take() {
            engine.release(*ctx.env);
        }
        let mut report = report.lock().unwrap();
        report["NativeReferenceReleased"] = json!(destroyed);
        report["Success"] = json!(success && destroyed);
        report["Stage"] = json!(if success && destroyed { "complete" } else { "failed" });
        if stdout_only {
            println!("{}", serde_json::to_string(&*report).unwrap());
        } else {
            let _ = std::fs::write(path, serde_json::to_vec_pretty(&*report).unwrap());
        }
        std::process::exit(if success && destroyed { 0 } else { 1 });
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
        json!({"Success":false,"Stage":"initialize","NativeEngineLoaded":true,"NativeInitialized":false,"NativeConnectionAbi":"user-options-callback","FakeAudioCapture":true,"FakeVideoCapture":true,"InputDevice":"disabled","OutputDevice":"disabled","OutputVolume":0,"LoopbackOnly":true,"WebSocketConnections":0,"DiscordConnections":0,"RealAccount":false,"RealMicrophone":false,"RealCamera":false,"ScreenCapture":false,"KeyMaterialPersisted":false,"NativeCallback":false,"MlsKeyPackageBytes":0,"EncryptionModes":0,"DiscoveryPackets":0}),
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
        std::thread::sleep(Duration::from_secs(8));
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
                            finish(key_own.clone(), count > 0)?;
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
