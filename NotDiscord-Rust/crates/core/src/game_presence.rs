//! Bounded playing activity shared by local RPC and the Gateway. RPC input is
//! data only: it cannot supply application identity, URLs or invitation secrets.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const TEXT_LIMIT: usize = 128;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePresenceTimestamps {
    pub start: Option<u64>,
    pub end: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePresenceAssets {
    pub large_image: Option<String>,
    pub large_text: Option<String>,
    pub small_image: Option<String>,
    pub small_text: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePresence {
    pub name: String,
    pub application_id: Option<String>,
    pub details: Option<String>,
    pub state: Option<String>,
    /// Gateway timestamps are milliseconds; the RPC constructor converts seconds.
    pub timestamps_ms: Option<GamePresenceTimestamps>,
    pub assets: Option<GamePresenceAssets>,
}

fn valid_text(value: &str, required: bool) -> bool {
    (!required || !value.trim().is_empty())
        && value.chars().count() <= TEXT_LIMIT
        && !value.chars().any(char::is_control)
}

fn valid_snowflake(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|id| id != 0)
}

fn optional_text(object: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(Value::String(value)) if valid_text(value, false) => {
            let value = value.trim();
            Ok((!value.is_empty()).then(|| value.to_owned()))
        }
        _ => Err(format!("El campo de actividad {key} no es válido.")),
    }
}

fn rpc_time(object: &Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|seconds| *seconds != 0)
            .and_then(|seconds| seconds.checked_mul(1000))
            .filter(|milliseconds| *milliseconds <= MAX_SAFE_INTEGER)
            .map(Some)
            .ok_or_else(|| format!("La marca temporal de actividad {key} no es válida.")),
    }
}

fn image_id(object: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None => Ok(None),
        Some(Value::String(value)) if value.len() <= 300 && !value.chars().any(char::is_control) => {
            // Symbolic asset keys (including rpcs3_logo), mp: identifiers and
            // external URLs require resolution. They never trigger a fetch here.
            Ok(valid_snowflake(value).then(|| value.clone()))
        }
        _ => Err(format!("La imagen de actividad {key} no es válida.")),
    }
}

impl GamePresence {
    pub fn from_name(name: &str) -> Result<Self, String> {
        if !valid_text(name, true) { return Err("El nombre de actividad no es válido.".into()); }
        let result = Self { name: name.trim().to_owned(), ..Self::default() };
        result.validate()?;
        Ok(result)
    }

    /// `application_id` and `name` come from the caller's confirmed local
    /// application identity, never the activity object's same-named fields.
    pub fn from_rpc(application_id: &str, name: &str, activity: &Value) -> Result<Self, String> {
        if !valid_snowflake(application_id) {
            return Err("La identidad de la aplicación RPC no es válida.".into());
        }
        let object = activity.as_object().ok_or("La actividad RPC debe ser un objeto.")?;
        if object.get("type").is_some_and(|value| value.as_u64() != Some(0)) {
            return Err("Sólo se admite actividad de juego de tipo Playing.".into());
        }
        let mut result = Self::from_name(name)?;
        result.application_id = Some(application_id.to_owned());
        result.details = optional_text(object, "details")?;
        result.state = optional_text(object, "state")?;
        if let Some(raw) = object.get("timestamps") {
            let object = raw.as_object().ok_or("Las marcas temporales RPC no son válidas.")?;
            let timestamps = GamePresenceTimestamps { start: rpc_time(object, "start")?, end: rpc_time(object, "end")? };
            if timestamps.start.is_some() || timestamps.end.is_some() { result.timestamps_ms = Some(timestamps); }
        }
        if let Some(raw) = object.get("assets") {
            let object = raw.as_object().ok_or("Las imágenes RPC no son válidas.")?;
            let assets = GamePresenceAssets {
                large_image: image_id(object, "large_image")?,
                large_text: optional_text(object, "large_text")?,
                small_image: image_id(object, "small_image")?,
                small_text: optional_text(object, "small_text")?,
            };
            if assets != GamePresenceAssets::default() { result.assets = Some(assets); }
        }
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !valid_text(&self.name, true)
            || self.application_id.as_deref().is_some_and(|id| !valid_snowflake(id))
            || [&self.details, &self.state].into_iter().flatten().any(|text| !valid_text(text, true))
        {
            return Err("El nombre, la identidad o el texto de actividad no son válidos.".into());
        }
        if let Some(times) = &self.timestamps_ms {
            if [times.start, times.end].into_iter().flatten().any(|value| value == 0 || value > MAX_SAFE_INTEGER)
                || times.start.zip(times.end).is_some_and(|(start, end)| end < start)
            {
                return Err("Las marcas temporales de actividad no son válidas.".into());
            }
        }
        if let Some(assets) = &self.assets {
            if self.application_id.is_none()
                || [&assets.large_image, &assets.small_image].into_iter().flatten().any(|id| !valid_snowflake(id))
                || [&assets.large_text, &assets.small_text].into_iter().flatten().any(|text| !valid_text(text, true))
            {
                return Err("Las imágenes o sus textos de actividad no son válidos.".into());
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut parts = vec![self.name.as_str()];
        for text in [&self.details, &self.state].into_iter().flatten() {
            if !parts.contains(&text.as_str()) { parts.push(text); }
        }
        parts.join(" · ")
    }

    pub fn payload(&self) -> Result<Value, String> {
        self.validate()?;
        let mut payload = json!({"name":self.name,"type":0});
        for (key, value) in [("application_id", &self.application_id), ("details", &self.details), ("state", &self.state)] {
            if let Some(value) = value { payload[key] = json!(value); }
        }
        if let Some(times) = &self.timestamps_ms {
            let mut values = Map::new();
            for (key, value) in [("start", times.start), ("end", times.end)] {
                if let Some(value) = value { values.insert(key.into(), json!(value)); }
            }
            if !values.is_empty() { payload["timestamps"] = Value::Object(values); }
        }
        if let Some(assets) = &self.assets {
            let mut values = Map::new();
            for (key, value) in [("large_image", &assets.large_image), ("large_text", &assets.large_text), ("small_image", &assets.small_image), ("small_text", &assets.small_text)] {
                if let Some(value) = value { values.insert(key.into(), json!(value)); }
            }
            if !values.is_empty() { payload["assets"] = Value::Object(values); }
        }
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const RPCS3: &str = "424004941485572097";

    #[test]
    fn rpc_maps_only_confirmed_identity_and_safe_metadata_to_gateway_milliseconds() {
        let game = GamePresence::from_rpc(RPCS3, "RPCS3", &json!({
            "name":"Spoofed name","application_id":"999","type":0,"details":"Synthetic game","state":"Playing",
            "timestamps":{"start":1_700_000_000u64,"end":1_700_000_060u64},
            "assets":{"large_image":"351371005538729000","large_text":"Synthetic game","small_image":"rpcs3_logo","small_url":"https://example.invalid"},
            "url":"https://example.invalid","secrets":{"join":"private"},"session_id":"forged","flags":999,"buttons":[{"url":"https://example.invalid"}]
        })).unwrap();
        assert_eq!(game.payload().unwrap(), json!({"name":"RPCS3","application_id":RPCS3,"type":0,"details":"Synthetic game","state":"Playing","timestamps":{"start":1_700_000_000_000u64,"end":1_700_000_060_000u64},"assets":{"large_image":"351371005538729000","large_text":"Synthetic game"}}));
        assert_eq!(game.summary(), "RPCS3 · Synthetic game · Playing");
    }

    #[test]
    fn name_only_preserves_the_legacy_wire_shape_without_invented_identity_or_timers() {
        assert_eq!(GamePresence::from_name(" Synthetic Game ").unwrap().payload().unwrap(), json!({"name":"Synthetic Game","type":0}));
        let game = GamePresence::from_rpc(RPCS3, "RPCS3", &json!({"details":"Idle","assets":{"large_image":"rpcs3_logo","small_image":"https://example.invalid/image.png"}})).unwrap();
        assert_eq!(game.payload().unwrap(), json!({"name":"RPCS3","type":0,"application_id":RPCS3,"details":"Idle"}));
    }

    #[test]
    fn malformed_known_fields_and_unbounded_timestamps_fail_closed() {
        for value in [json!([]),json!({"type":1}),json!({"type":null}),json!({"details":12}),json!({"state":"bad\ntext"}),json!({"details":"x".repeat(129)}),json!({"timestamps":null}),json!({"timestamps":{"start":0}}),json!({"timestamps":{"start":-1}}),json!({"timestamps":{"start":1.5}}),json!({"timestamps":{"start":u64::MAX}}),json!({"timestamps":{"start":30,"end":20}}),json!({"assets":[]}),json!({"assets":{"large_image":3}})] {
            assert!(GamePresence::from_rpc(RPCS3,"RPCS3",&value).is_err(),"{value}");
        }
        for id in ["", "0", "bad", "18446744073709551616"] { assert!(GamePresence::from_rpc(id,"Game",&json!({})).is_err()); }
        for name in ["", " \t ", "bad\0name", &"界".repeat(129)] { assert!(GamePresence::from_name(name).is_err()); }
    }

    #[test]
    fn wire_validation_cannot_be_bypassed_by_mutating_a_typed_value() {
        let mut game = GamePresence::from_name("Game").unwrap();
        game.application_id=Some("0".into());assert!(game.payload().is_err());
        game.application_id=Some(RPCS3.into());game.assets=Some(GamePresenceAssets{large_image:Some("https://example.invalid".into()),..Default::default()});assert!(game.payload().is_err());
        game.assets=None;game.timestamps_ms=Some(GamePresenceTimestamps{start:Some(MAX_SAFE_INTEGER+1),end:None});assert!(game.payload().is_err());
    }
}
