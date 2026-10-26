#![allow(clippy::missing_safety_doc)]
#[path = "../../core/src/video.rs"]
mod frames;
mod host;
mod native;
#[cfg(feature = "native-probe")]
mod probe;
#[cfg(feature = "native-probe")]
mod probe_video;
#[cfg(feature = "native-probe")]
mod probe_window;
mod protocol;
mod session;
mod settings;
mod stats;
use napi::Env;
use napi_derive::napi;

#[napi]
pub fn protocol_version() -> u32 {
    1
}

#[napi]
pub fn validate_endpoint(endpoint: String) -> napi::Result<String> {
    protocol::endpoint(&serde_json::json!(endpoint))
        .map_err(|e| napi::Error::from_reason(e.message))
}

/// Entry point called once by the declarative Electron loader. The supplied
/// object is the unmodified third-party engine; all policy and IPC live in Rust.
#[napi]
pub fn run(
    env: Env,
    engine: napi::JsObject,
    pipe_name: String,
    data_directory: Option<String>,
) -> napi::Result<()> {
    let process: napi::JsObject = env.get_global()?.get_named_property("process")?;
    let versions: napi::JsObject = process.get_named_property("versions")?;
    if !versions.has_named_property("electron")? {
        return Err(napi::Error::from_reason(
            "The native media engine requires its compatible Electron host.",
        ));
    }
    let data_directory = data_directory.unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_default()
            .join("data/media")
            .to_string_lossy()
            .into_owned()
    });
    host::start(env, engine, pipe_name, data_directory)
}

#[napi]
pub fn sanitize_stats(raw: serde_json::Value, state: String, dave: bool) -> serde_json::Value {
    stats::sanitize(&raw, &state, dave)
}

#[cfg(feature = "native-probe")]
#[napi]
pub fn probe_native_mls(env: Env, engine: napi::JsObject, directory: String) -> napi::Result<()> {
    probe::run(env, engine, directory)
}
/// Diagnostic-only entry that shares the exact native participant playback
/// helpers with Host. It opens no transport, device, account or connection.
#[cfg(feature = "native-probe")]
#[napi]
pub fn probe_participant_audio(env:Env,connection:napi::JsObject,payload:serde_json::Value)->napi::Result<()> {
    let result=match payload["action"].as_str() {
        Some("merge")=>native::merge_participants(env,&connection,payload["users"].clone()).map(|_|()),
        Some("gain")=>protocol::snowflake(&payload["userId"],"userId").and_then(|user|
            protocol::user_volume(&payload["value"]).and_then(|gain|native::participant_gain(env,&connection,&user,gain))),
        _=>Err(protocol::MediaError::new("INVALID_PROBE","Only participant merge/gain are available.")),
    };
    result.map_err(|error|napi::Error::from_reason(error.message))
}
#[cfg(feature = "native-probe")]
#[napi]
pub fn probe_native_video(env: Env, engine: napi::JsObject, directory: String) -> napi::Result<()> {
    probe_video::run(env, engine, directory)
}
/// Runs production receive/sink cleanup against JavaScript fakes, never the
/// third-party engine, network or physical capture/output devices.
#[cfg(feature = "native-probe")]
#[napi]
pub fn probe_receive_lifecycle(env: Env) -> napi::Result<serde_json::Value> {
    host::video::probe_receive(env)
}
#[cfg(feature = "native-probe")]
#[napi]
pub fn prepare_owned_preview() -> napi::Result<String> {
    probe_window::prepare().map_err(napi::Error::from_reason)
}
