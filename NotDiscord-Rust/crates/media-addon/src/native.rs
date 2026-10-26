//! JavaScript objects stay on the Electron main thread. Only owned JSON/bytes
//! and actor messages cross threads; the third-party addon owns codecs/MLS.
use crate::protocol::{MAX_FRAME, MediaError, Result};
use napi::{
    Env, JsArrayBuffer, JsFunction, JsObject, JsTypedArray, JsUnknown, NapiRaw, NapiValue, Ref,
    ValueType,
};
use serde_json::{Value, json};
pub struct ObjectRef(pub Ref<()>);
impl ObjectRef {
    pub fn new(env: Env, obj: JsObject) -> Result<Self> {
        env.create_reference(obj)
            .map(Self)
            .map_err(|_| MediaError::internal())
    }
    pub fn object(&self, env: Env) -> Result<JsObject> {
        env.get_reference_value(&self.0)
            .map_err(|_| MediaError::internal())
    }
    pub fn release(mut self, env: Env) {
        let _ = self.0.unref(env);
    }
}
pub fn json_value(env: Env, value: Value) -> Result<JsUnknown> {
    env.to_js_value(&value).map_err(|_| MediaError::internal())
}
pub fn bytes_value(env: Env, value: Vec<u8>) -> Result<JsUnknown> {
    let len = value.len();
    env.create_arraybuffer_with_data(value)
        .and_then(|a| {
            a.into_raw()
                .into_typedarray(napi::TypedArrayType::Uint8, len, 0)
        })
        .map(|v| v.into_unknown())
        .map_err(|_| MediaError::internal())
}
pub fn read_value(env: Env, value: JsUnknown) -> Result<Value> {
    let mut is_array_buffer = false;
    unsafe {
        napi::sys::napi_is_arraybuffer(env.raw(), value.raw(), &mut is_array_buffer);
    }
    if is_array_buffer {
        let array = unsafe { JsArrayBuffer::from_raw_unchecked(env.raw(), value.raw()) }
            .into_value()
            .map_err(|_| MediaError::internal())?;
        if array.len() > MAX_FRAME {
            return Err(MediaError::new(
                "OVERSIZE_FRAME",
                "Native callback exceeded its limit.",
            ));
        }
        return Ok(json!(array.as_ref()));
    }
    if value.is_typedarray().unwrap_or(false) {
        let array = unsafe { JsTypedArray::from_raw_unchecked(env.raw(), value.raw()) }
            .into_value()
            .map_err(|_| MediaError::internal())?;
        if array.length > MAX_FRAME {
            return Err(MediaError::new(
                "OVERSIZE_FRAME",
                "Native callback exceeded its limit.",
            ));
        }
        let offset = array.byte_offset;
        let length = array.length;
        let buffer = array
            .arraybuffer
            .into_value()
            .map_err(|_| MediaError::internal())?;
        let bytes = buffer.as_ref();
        if offset > bytes.len() || length > bytes.len() - offset {
            return Err(MediaError::internal());
        }
        return Ok(json!(&bytes[offset..offset + length]));
    }
    match value.get_type().map_err(|_| MediaError::internal())? {
        ValueType::Undefined | ValueType::Null => Ok(Value::Null),
        _ => env.from_js_value(value).map_err(|_| MediaError::internal()),
    }
}
pub fn call(env: Env, target: &JsObject, name: &str, args: &[JsUnknown]) -> Result<JsUnknown> {
    #[cfg(feature = "native-probe")]
    if std::env::var_os("NOTDISCORD_MEDIA_TRACE").is_some() {
        eprintln!("rust-native:before:{name}");
    }
    let result = (|| {
        let method: JsFunction = target.get_named_property(name)?;
        method.call(Some(target), args)
    })();
    #[cfg(feature = "native-probe")]
    if std::env::var_os("NOTDISCORD_MEDIA_TRACE").is_some() {
        eprintln!("rust-native:after:{name}:{}", result.is_ok());
    }
    result.map_err(|_| {
        unsafe {
            let mut pending = false;
            napi::sys::napi_is_exception_pending(env.raw(), &mut pending);
            if pending {
                let mut exception = std::ptr::null_mut();
                napi::sys::napi_get_and_clear_last_exception(env.raw(), &mut exception);
            }
        }
        MediaError::new(
            "NATIVE_CALL_FAILED",
            format!("Native multimedia method {name} failed."),
        )
    })
}
pub fn optional(
    env: Env,
    target: &JsObject,
    name: &str,
    args: &[JsUnknown],
) -> Result<Option<JsUnknown>> {
    if !target.has_named_property(name).unwrap_or(false) {
        return Ok(None);
    }
    let value: JsUnknown = target
        .get_named_property(name)
        .map_err(|_| MediaError::internal())?;
    if value.get_type().ok() != Some(ValueType::Function) {
        return Ok(None);
    }
    call(env, target, name, args).map(Some)
}
pub fn call_json(env: Env, target: &JsObject, name: &str, args: Vec<Value>) -> Result<JsUnknown> {
    let args = args
        .into_iter()
        .map(|v| json_value(env, v))
        .collect::<Result<Vec<_>>>()?;
    call(env, target, name, &args)
}
/// Raw discord_voice uses a linear amplitude multiplier: 1 is 100 percent.
/// Keep the same native playback call for live control, source recreation and
/// the account-free PCM playback probe.
pub fn participant_gain(env:Env,connection:&JsObject,user:&str,gain:f64)->Result<()> {
    crate::protocol::snowflake(&json!(user),"userId")?;
    if !gain.is_finite()||!(0.0..=5.0).contains(&gain) {
        return Err(MediaError::new("INVALID_VOLUME","Participant gain must be from 0 to 5."));
    }
    call_json(env,connection,"setLocalVolume",vec![json!(user),json!(gain)]).map(|_|())
}
pub fn merge_participants(env:Env,connection:&JsObject,users:Value)->Result<JsUnknown> {
    let result=call_json(env,connection,"mergeUsers",vec![users.clone()])?;
    if let Some(users)=users.as_array() {for user in users {
        if let Some(user_id)=user["id"].as_str() {
            participant_gain(env,connection,user_id,user["volume"].as_f64().unwrap_or(1.))?;
        }
    }}
    Ok(result)
}
/// Modern JS factories and the bundled raw VoiceConnection constructor take
/// (user, options, callback). Only the older named create*Connection factories
/// use seven positional arguments. The shipped discord_voice/index.js wraps
/// the raw constructor with exactly three arguments; the web client alone is
/// not evidence of the raw addon's ABI.
pub fn create_connection(
    env: Env,
    engine: &JsObject,
    user: &str,
    options: Value,
    callback: JsFunction,
    own_stream: bool,
) -> Result<JsObject> {
    let function = |name: &str| -> bool {
        engine.get_named_property::<JsUnknown>(name).ok()
            .is_some_and(|value| value.get_type().ok() == Some(ValueType::Function))
    };
    let modern = if own_stream && function("createOwnStreamConnectionWithOptions") {
        Some("createOwnStreamConnectionWithOptions")
    } else if !own_stream && function("createVoiceConnectionWithOptions") {
        Some("createVoiceConnectionWithOptions")
    } else {
        None
    };
    if let Some(name) = modern {
        return call(env, engine, name, &[
            json_value(env, json!(user))?,
            json_value(env, options)?,
            callback.into_unknown(),
        ]).and_then(|value| value.coerce_to_object().map_err(|_| MediaError::internal()));
    }
    let legacy = if own_stream && function("createOwnStreamConnection") {
        Some("createOwnStreamConnection")
    } else if !own_stream && function("createVoiceConnection") {
        Some("createVoiceConnection")
    } else {
        None
    };
    if let Some(name) = legacy {
        let args = [
            json_value(env, options["ssrc"].clone())?,
            json_value(env, json!(user))?,
            json_value(env, options["address"].clone())?,
            json_value(env, options["port"].clone())?,
            callback.into_unknown(),
            json_value(env, options["experiments"].clone())?,
            json_value(env, options["streamParameters"].clone())?,
        ];
        return call(env, engine, name, &args)
            .and_then(|value| value.coerce_to_object().map_err(|_| MediaError::internal()));
    }
    let constructor: JsFunction = engine.get_named_property("VoiceConnection")
        .map_err(|_| MediaError::new("VOICE_FACTORY_UNAVAILABLE", "The media engine has no supported voice connection factory."))?;
    let args = [
        json_value(env, json!(user))?,
        json_value(env, options)?,
        callback.into_unknown(),
    ];
    constructor.new_instance(&args).map_err(|_| {
        // Do not leave a native exception pending on the Electron main thread.
        unsafe {
            let mut pending = false;
            napi::sys::napi_is_exception_pending(env.raw(), &mut pending);
            if pending {
                let mut exception = std::ptr::null_mut();
                napi::sys::napi_get_and_clear_last_exception(env.raw(), &mut exception);
            }
        }
        MediaError::new("VOICE_FACTORY_FAILED", "The native voice connection constructor failed.")
    })
}
pub fn optional_json(env: Env, target: &JsObject, name: &str, args: Vec<Value>) -> Result<()> {
    let args = args
        .into_iter()
        .map(|v| json_value(env, v))
        .collect::<Result<Vec<_>>>()?;
    optional(env, target, name, &args).map(|_| ())
}
pub fn property_json(env: Env, target: &JsObject, name: &str) -> Result<Value> {
    let value: JsUnknown = target
        .get_named_property(name)
        .map_err(|_| MediaError::internal())?;
    read_value(env, value)
}
