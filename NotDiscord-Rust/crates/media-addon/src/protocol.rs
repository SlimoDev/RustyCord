use serde_json::{Value, json};

pub const MAX_FRAME: usize = 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaError {
    pub code: &'static str,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, MediaError>;
impl MediaError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn json(&self) -> Value {
        json!({"code":self.code,"message":self.message})
    }
    pub fn internal() -> Self {
        Self::new(
            "MEDIA_FAILED",
            "The media operation failed. No credentials were logged.",
        )
    }
}
pub fn string(v: &Value, field: &str, max: usize) -> Result<String> {
    let value = v
        .as_str()
        .ok_or_else(|| MediaError::new("INVALID_ARGUMENT", format!("Invalid {field}.")))?;
    if value.is_empty()
        || value.encode_utf16().count() > max
        || value.chars().any(|c| matches!(c, '\r' | '\n' | '\0'))
    {
        return Err(MediaError::new(
            "INVALID_ARGUMENT",
            format!("Invalid {field}."),
        ));
    }
    Ok(value.to_owned())
}
pub fn snowflake(v: &Value, field: &str) -> Result<String> {
    let s = string(v, field, 20)?;
    if !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err(MediaError::new(
            "INVALID_ARGUMENT",
            format!("Invalid {field}."),
        ));
    }
    Ok(s)
}
pub fn endpoint(v: &Value) -> Result<String> {
    let s = string(v, "endpoint", 256)?;
    let url = if s.starts_with("wss://") {
        s
    } else {
        format!("wss://{s}")
    };
    let mut u = url::Url::parse(&url)
        .map_err(|_| MediaError::new("INVALID_ENDPOINT", "Invalid voice endpoint."))?;
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let allowed = ["discord.media", "discord.gg", "discordapp.com"]
        .iter()
        .any(|d| host.ends_with(&format!(".{d}")));
    if u.scheme() != "wss"
        || !u.username().is_empty()
        || u.password().is_some()
        || !allowed
        || !host
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'-')
        || u.path() != "/"
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err(MediaError::new(
            "INVALID_ENDPOINT",
            "The endpoint is not an allowed Discord voice host.",
        ));
    }
    u.set_query(Some("v=8"));
    Ok(u.to_string())
}
#[derive(Clone)]
pub struct Credentials {
    pub user_id: String,
    pub channel_id: String,
    pub original_channel_id: String,
    pub server_id: String,
    pub session_id: String,
    pub voice_token: String,
    pub endpoint: String,
    pub self_mute: bool,
    pub self_deaf: bool,
    pub stream: bool,
    pub stream_owner: String,
    pub stream_key: String,
}
impl Credentials {
    pub fn parse(p: &Value, stream: bool) -> Result<Self> {
        let server_id = snowflake(
            if stream {
                &p["rtcServerId"]
            } else {
                p.get("guildId")
                    .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                    .unwrap_or(&p["channelId"])
            },
            "serverId",
        )?;
        let original_channel_id = snowflake(&p["channelId"], "channelId")?;
        let channel_id = if stream {
            match p
                .get("streamChannelId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                Some(s) => snowflake(&json!(s), "streamChannelId")?,
                None => server_id
                    .parse::<u128>()
                    .ok()
                    .and_then(|n| n.checked_sub(1))
                    .ok_or_else(|| MediaError::new("INVALID_ARGUMENT", "Invalid streamChannelId."))?
                    .to_string(),
            }
        } else {
            original_channel_id.clone()
        };
        Ok(Self {
            user_id: snowflake(&p["userId"], "userId")?,
            channel_id,
            original_channel_id,
            server_id,
            session_id: string(&p["sessionId"], "sessionId", 512)?,
            voice_token: string(&p["voiceToken"], "voiceToken", 2048)?,
            endpoint: endpoint(&p["endpoint"])?,
            self_mute: p["selfMute"].as_bool().unwrap_or(false),
            self_deaf: p["selfDeaf"].as_bool().unwrap_or(false),
            stream,
            stream_owner: if stream {
                p.get("streamOwnerId")
                    .map(|id| snowflake(id, "streamOwnerId"))
                    .transpose()?
                    .unwrap_or(snowflake(&p["userId"], "userId")?)
            } else {
                String::new()
            },
            stream_key: p
                .get("streamKey")
                .map(|key| string(key, "streamKey", 128))
                .transpose()?
                .unwrap_or_default(),
        })
    }
}
pub fn volume(value: &Value) -> Result<f64> {
    let n = value
        .as_f64()
        .ok_or_else(|| MediaError::new("INVALID_VOLUME", "Volume must be from 0 to 400."))?;
    if !n.is_finite() || !(0.0..=400.0).contains(&n) {
        return Err(MediaError::new(
            "INVALID_VOLUME",
            "Volume must be from 0 to 400.",
        ));
    }
    Ok(n / 100.0)
}
pub fn user_volume(value: &Value) -> Result<f64> {
    value.as_f64().filter(|value|value.is_finite() && (0.0..=500.0).contains(value))
        .map(|value|value/100.)
        .ok_or_else(||MediaError::new("INVALID_VOLUME","Participant volume must be from 0 to 500%."))
}
pub fn user_volumes(value:&Value)->Result<std::collections::HashMap<String,f64>> {
    let values=value.as_object().filter(|values|values.len()<=256)
        .ok_or_else(||MediaError::new("INVALID_VOLUME","Participant volumes must be an object with at most 256 users."))?;
    values.iter().map(|(user,value)|Ok((snowflake(&json!(user),"userId")?,user_volume(value)?))).collect()
}
pub fn choose_encryption(server: &Value, native: &Value) -> Result<String> {
    for mode in ["aead_aes256_gcm_rtpsize", "aead_xchacha20_poly1305_rtpsize"] {
        if server
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x == mode))
            && native
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == mode))
        {
            return Ok(mode.to_owned());
        }
    }
    Err(MediaError::new(
        "UNSUPPORTED_ENCRYPTION",
        "No supported authenticated transport encryption mode.",
    ))
}
#[derive(Debug, Clone)]
pub struct Frame {
    pub op: u64,
    pub data: Value,
    pub bytes: Vec<u8>,
    pub seq: Option<u64>,
}
pub fn json_frame(data: &str) -> Result<Frame> {
    if data.len() > MAX_FRAME {
        return Err(MediaError::new("OVERSIZE_FRAME", "Voice frame too large."));
    }
    let v: Value = serde_json::from_str(data)
        .map_err(|_| MediaError::new("INVALID_FRAME", "Invalid voice frame."))?;
    Ok(Frame {
        op: v["op"]
            .as_u64()
            .ok_or_else(|| MediaError::new("INVALID_FRAME", "Invalid voice opcode."))?,
        data: v["d"].clone(),
        bytes: vec![],
        seq: v["seq"].as_u64(),
    })
}
pub fn binary_frame(data: &[u8]) -> Result<Frame> {
    if data.len() < 3 || data.len() > MAX_FRAME {
        return Err(MediaError::new(
            "INVALID_FRAME",
            "Invalid binary voice frame.",
        ));
    }
    Ok(Frame {
        seq: Some(u16::from_be_bytes([data[0], data[1]]) as u64),
        op: data[2] as u64,
        data: Value::Null,
        bytes: data[3..].to_vec(),
    })
}
pub fn client_binary(op: u8, data: &[u8]) -> Result<Vec<u8>> {
    if data.len() > MAX_FRAME - 1 {
        return Err(MediaError::new("OVERSIZE_FRAME", "Voice frame too large."));
    }
    let mut out = Vec::with_capacity(data.len() + 1);
    out.push(op);
    out.extend_from_slice(data);
    Ok(out)
}
pub fn transition(data: &[u8]) -> Result<(u16, Vec<u8>)> {
    if data.len() < 3 {
        return Err(MediaError::new("INVALID_FRAME", "Invalid MLS transition."));
    }
    Ok((u16::from_be_bytes([data[0], data[1]]), data[2..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints() {
        assert!(endpoint(&json!("example.com")).is_err());
        assert!(
            endpoint(&json!("us-east123.discord.media"))
                .unwrap()
                .ends_with("?v=8")
        );
        assert!(endpoint(&json!("evil.discord.media.example.com")).is_err());
        assert!(endpoint(&json!("wss://user@x.discord.media")).is_err());
        assert!(endpoint(&json!("wss://x.discord.media/a")).is_err());
    }
    #[test]
    fn frames_and_transition() {
        let f = binary_frame(&[0, 9, 29, 0, 4, 88]).unwrap();
        assert_eq!(f.seq, Some(9));
        assert_eq!(transition(&f.bytes).unwrap(), (4, vec![88]));
        assert_eq!(client_binary(26, &[8, 9]).unwrap(), vec![26, 8, 9]);
        assert!(binary_frame(&[1]).is_err());
    }
    #[test]
    fn authenticated_modes_only() {
        assert!(
            choose_encryption(&json!(["xsalsa20_poly1305"]), &json!(["xsalsa20_poly1305"]))
                .is_err()
        );
        assert_eq!(
            choose_encryption(
                &json!(["aead_aes256_gcm_rtpsize"]),
                &json!(["aead_aes256_gcm_rtpsize"])
            )
            .unwrap(),
            "aead_aes256_gcm_rtpsize"
        );
    }
    #[test]
    fn stream_uses_distinct_rtc_channel() {
        let p = json!({"userId":"1","channelId":"99","rtcServerId":"18446744073709551616","sessionId":"s","voiceToken":"t","endpoint":"x.discord.media"});
        let c = Credentials::parse(&p, true).unwrap();
        assert_eq!(c.channel_id, "18446744073709551615");
        assert_eq!(c.original_channel_id, "99");
    }
    #[test]
    fn volumes() {
        assert_eq!(volume(&json!(100)).unwrap(), 1.0);
        assert!(volume(&json!(401)).is_err());
        assert!(volume(&json!("100")).is_err());
        assert_eq!(user_volume(&json!(100)).unwrap(),1.);
        assert_eq!(user_volume(&json!(500)).unwrap(),5.);
        assert_eq!(user_volume(&json!(0)).unwrap(),0.);
        assert_eq!(user_volume(&json!(25)).unwrap(),0.25);
        for invalid in [json!(501),json!(-1),json!("100"),Value::Null] {assert!(user_volume(&invalid).is_err());}
    }
}
