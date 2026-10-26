//! Compiled only for a synthetic native sink/allocator ABI gate.
use crate::native::{self, ObjectRef};
use napi::{Env, JsObject, JsUnknown};
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
    engine: Option<ObjectRef>,
    device: String,
    report: Arc<Mutex<Value>>,
    path: PathBuf,
    done: Arc<AtomicBool>,
}
fn error() -> napi::Error {
    napi::Error::from_reason("Synthetic video ABI probe failed.")
}
fn finish(env: Env, own: Rc<RefCell<Probe>>, success: bool) -> napi::Result<()> {
    if own.borrow().done.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let callback = env.create_function_from_closure("finishSyntheticVideo", move |ctx| {
        let mut probe = own.borrow_mut();
        if let Some(reference) = probe.engine.take() {
            if let Ok(engine) = reference.object(*ctx.env) {
                let _ = native::call_json(
                    *ctx.env,
                    &engine,
                    "setVideoOutputSink",
                    vec![json!(probe.device)],
                );
                let _ = native::call_json(
                    *ctx.env,
                    &engine,
                    "setVideoInputDevice",
                    vec![json!("disabled")],
                );
            }
            reference.release(*ctx.env);
        }
        probe.report.lock().unwrap()["Success"] = json!(success);
        probe.report.lock().unwrap()["NativeReferenceReleased"] = json!(true);
        let _ = std::fs::write(
            &probe.path,
            serde_json::to_vec_pretty(&*probe.report.lock().unwrap()).unwrap(),
        );
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(75));
            std::process::exit(if success { 0 } else { 1 });
        });
        ctx.env.get_undefined()
    })?;
    let global = env.get_global()?;
    let timer: napi::JsFunction = global.get_named_property("setImmediate")?;
    timer.call(None, &[callback.into_unknown()])?;
    Ok(())
}
pub fn run(env: Env, engine: JsObject, directory: String) -> napi::Result<()> {
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).map_err(|_| error())?;
    let path = directory.join("native-video-probe.json");
    let fake_file = directory.join("synthetic-camera.y4m");
    let mut y4m = b"YUV4MPEG2 W64 H48 F30:1 Ip A1:1 C420\n".to_vec();
    for _ in 0..16 {
        y4m.extend_from_slice(b"FRAME\n");
        y4m.extend(std::iter::repeat_n(80u8, 64 * 48));
        y4m.extend(std::iter::repeat_n(100u8, 64 * 48 / 4));
        y4m.extend(std::iter::repeat_n(150u8, 64 * 48 / 4));
    }
    std::fs::write(&fake_file, y4m).map_err(|_| error())?;
    let report = Arc::new(Mutex::new(
        json!({"Success":false,"FakeVideoCapture":true,"FakeAudioCapture":true,"RealDevicesActivated":false,"NetworkConnections":0,"Frames":0,"Allocator":false,"Acknowledged":false,"NativeReferenceReleased":false}),
    ));
    let done = Arc::new(AtomicBool::new(false));
    let deadline = done.clone();
    let target = path.clone();
    let timeout_report = report.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        if !deadline.swap(true, Ordering::SeqCst) {
            timeout_report.lock().unwrap()["Failure"] = json!("FAKE_VIDEO_CALLBACK_TIMEOUT");
            let _ = std::fs::write(
                target,
                serde_json::to_vec_pretty(&*timeout_report.lock().unwrap()).unwrap(),
            );
            std::process::exit(1);
        }
    });
    let allocator_report = report.clone();
    let allocator = env.create_function_from_closure("allocateFakeVideo", move |ctx| {
        let width = ctx.get::<napi::JsNumber>(0)?.get_uint32()?;
        let height = ctx.get::<napi::JsNumber>(1)?.get_uint32()?;
        if width > 8192 || height > 8192 {
            return Err(error());
        }
        let length = width as u64 * height as u64 * 4;
        if width == 0 || height == 0 || length > 4096 * 2160 * 4 {
            return Err(error());
        }
        allocator_report.lock().unwrap()["Allocator"] = json!(true);
        let mut image = ctx.env.create_object()?;
        image.set_named_property("width", width)?;
        image.set_named_property("height", height)?;
        image.set_named_property(
            "data",
            ctx.env
                .create_arraybuffer(length as usize)?
                .into_raw()
                .into_typedarray(napi::TypedArrayType::Uint8Clamped, length as usize, 0)?,
        )?;
        Ok(image)
    })?;
    native::call(
        env,
        &engine,
        "setImageDataAllocator",
        &[allocator.into_unknown()],
    )
    .map_err(|_| error())?;
    native::call_json(env,&engine,"initialize",vec![json!({"audioSubsystem":"experimental","logLevel":-1,"dataDirectory":directory,"logDirectory":directory,"maxLogBytes":65536,"offloadAdmControls":true,"asyncVideoInputDeviceInit":true,"useFakeVideoCapture":true,"useFileForFakeVideoCapture":fake_file,"useFakeAudioCapture":true})]).map_err(|_|error())?;
    for (method, arg) in [
        ("setInputDevice", "disabled"),
        ("setOutputDevice", "disabled"),
        ("setVideoInputDevice", "disabled"),
    ] {
        native::call_json(env, &engine, method, vec![json!(arg)]).map_err(|_| error())?;
    }
    let own = Rc::new(RefCell::new(Probe {
        engine: Some(ObjectRef::new(env, engine).map_err(|_| error())?),
        device: String::new(),
        report,
        path,
        done,
    }));
    own.borrow().report.lock().unwrap()["SyntheticDetachReleased"] =
        json!(crate::host::video::probe_synthetic_detach(env)?);
    // Prefer cleanup on the JavaScript thread, where N-API references belong.
    // The worker deadline remains a hard stop only if the native loop stalls.
    let timeout_own = own.clone();
    let cleanup = env.create_function_from_closure("syntheticVideoTimeout", move |ctx| {
        if !timeout_own.borrow().done.load(Ordering::Acquire) {
            timeout_own.borrow().report.lock().unwrap()["Failure"] =
                json!("FAKE_VIDEO_CALLBACK_TIMEOUT");
            finish(*ctx.env, timeout_own.clone(), false)?;
        }
        ctx.env.get_undefined()
    })?;
    let timer: napi::JsFunction = env.get_global()?.get_named_property("setTimeout")?;
    timer.call(
        None,
        &[
            cleanup.into_unknown(),
            env.create_uint32(8000)?.into_unknown(),
        ],
    )?;
    // Independently test the actual N-API closure, native sink registration and
    // readiness ABI with our own RGBA. This is not claimed as native decoding.
    let synthetic = own.clone();
    let stream = "__NotDiscord_Synthetic_RGBA_Only__";
    let callback = env.create_function_from_closure("syntheticNapiRgba", move |ctx| {
        let image = ctx.get::<JsObject>(0)?;
        let frame = crate::host::video::read_image(*ctx.env, image, 640, 360);
        let probe = synthetic.borrow();
        let engine = probe
            .engine
            .as_ref()
            .unwrap()
            .object(*ctx.env)
            .map_err(|_| error())?;
        let ack = native::call_json(
            *ctx.env,
            &engine,
            "signalVideoOutputSinkReady",
            vec![json!(stream)],
        )
        .is_ok();
        let mut report = probe.report.lock().unwrap();
        report["SyntheticNapiFrame"] = json!(frame.is_some_and(|(w, h, pixels)| w == 2
            && h == 1
            && pixels == [255, 0, 0, 255, 0, 255, 0, 128]));
        report["NativeReadyAck"] = json!(ack);
        ctx.env.get_undefined()
    })?;
    let mut reference = env.create_reference(callback)?;
    let engine = own
        .borrow()
        .engine
        .as_ref()
        .unwrap()
        .object(env)
        .map_err(|_| error())?;
    let callback: napi::JsFunction = env.get_reference_value(&reference)?;
    native::call(
        env,
        &engine,
        "setVideoOutputSink",
        &[
            native::json_value(env, json!(stream)).map_err(|_| error())?,
            callback.into_unknown(),
            native::json_value(env, json!(true)).map_err(|_| error())?,
        ],
    )
    .map_err(|_| error())?;
    own.borrow().report.lock().unwrap()["NativeSinkRegistered"] = json!(true);
    let mut image = env.create_object()?;
    image.set_named_property("width", 2)?;
    image.set_named_property("height", 1)?;
    image.set_named_property(
        "data",
        env.create_arraybuffer_with_data(vec![255, 0, 0, 255, 0, 255, 0, 128])?
            .into_raw()
            .into_typedarray(napi::TypedArrayType::Uint8Clamped, 8, 0)?,
    )?;
    let callback: napi::JsFunction = env.get_reference_value(&reference)?;
    callback.call(None, &[image.into_unknown()])?;
    native::call_json(env, &engine, "setVideoOutputSink", vec![json!(stream)])
        .map_err(|_| error())?;
    reference.unref(env)?;
    let capture = own.clone();
    let devices = env.create_function_from_closure("syntheticVideoDevices", move |ctx| {
        let values = native::read_value(*ctx.env, ctx.get::<JsUnknown>(0)?).map_err(|_| error())?;
        capture.borrow().report.lock().unwrap()["InventoryCount"] =
            json!(values.as_array().map_or(0, Vec::len));
        let device = values
            .as_array()
            .and_then(|items| {
                items.iter().find(|item| {
                    format!(
                        "{} {}",
                        item["name"].as_str().unwrap_or(""),
                        item["guid"].as_str().unwrap_or("")
                    )
                    .to_ascii_lowercase()
                    .contains("fake")
                })
            })
            .and_then(|item| {
                item["guid"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .or(item["name"].as_str())
            })
            .map(str::to_owned);
        // With an explicit synthetic file and fake capture initialized, a
        // non-inventoried synthetic identity is permitted; never use Default,
        // a numeric device index or a physical GUID as fallback.
        let device = device.unwrap_or_else(|| "__NotDiscord_Y4M_Synthetic_Only_6df4ab19__".into());
        if values.as_array().is_some_and(|items| {
            items.iter().any(|item| {
                (item["name"] == device || item["guid"] == device)
                    && !device.to_ascii_lowercase().contains("fake")
            })
        }) {
            finish(*ctx.env, capture.clone(), false)?;
            return ctx.env.get_undefined();
        }
        capture.borrow().report.lock().unwrap()["SyntheticFileConfigured"] = json!(true);
        capture.borrow_mut().device = device.clone();
        let engine = capture
            .borrow()
            .engine
            .as_ref()
            .unwrap()
            .object(*ctx.env)
            .map_err(|_| error())?;
        let frame_capture = capture.clone();
        let stream = device.clone();
        let frame =
            ctx.env
                .create_function_from_closure("syntheticRawFrame", move |ctx| {
                    let engine = frame_capture
                        .borrow()
                        .engine
                        .as_ref()
                        .and_then(|reference| reference.object(*ctx.env).ok());
                    let result = ctx.get::<JsObject>(0).ok().and_then(|image| {
                        crate::host::video::read_image(*ctx.env, image, 640, 360)
                    });
                    if let Some(engine) = engine {
                        let acknowledged = native::call_json(
                            *ctx.env,
                            &engine,
                            "signalVideoOutputSinkReady",
                            vec![json!(stream)],
                        )
                        .is_ok();
                        frame_capture.borrow().report.lock().unwrap()["Acknowledged"] =
                            json!(acknowledged);
                    }
                    if let Some((width, height, pixels)) = result {
                        let probe = frame_capture.borrow();
                        let mut report = probe.report.lock().unwrap();
                        report["Frames"] = json!(report["Frames"].as_u64().unwrap_or(0) + 1);
                        report["Width"] = json!(width);
                        report["Height"] = json!(height);
                        report["RgbaBytes"] = json!(pixels.len());
                        drop(report);
                        drop(probe);
                        finish(*ctx.env, frame_capture.clone(), true)?;
                    }
                    ctx.env.get_undefined()
                })?;
        native::call(
            *ctx.env,
            &engine,
            "setVideoOutputSink",
            &[
                native::json_value(*ctx.env, json!(device.clone())).map_err(|_| error())?,
                frame.into_unknown(),
                native::json_value(*ctx.env, json!(true)).map_err(|_| error())?,
            ],
        )
        .map_err(|_| error())?;
        native::call_json(
            *ctx.env,
            &engine,
            "setVideoInputDevice",
            vec![json!(device)],
        )
        .map_err(|_| error())?;
        ctx.env.get_undefined()
    })?;
    let engine = own
        .borrow()
        .engine
        .as_ref()
        .unwrap()
        .object(env)
        .map_err(|_| error())?;
    native::call(
        env,
        &engine,
        "getVideoInputDevices",
        &[devices.into_unknown()],
    )
    .map_err(|_| error())?;
    Ok(())
}
