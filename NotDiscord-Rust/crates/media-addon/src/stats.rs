use serde_json::{Value, json};
fn number(v: &Value, max: f64) -> Value {
    v.as_f64()
        .filter(|n| n.is_finite() && *n >= 0. && *n <= max)
        .map_or(Value::Null, |n| json!(n))
}
fn n(v: &Value) -> Value {
    number(v, 9_007_199_254_740_991.)
}
fn boolean(v: &Value) -> Value {
    v.as_bool().map_or(Value::Null, |b| json!(b))
}
fn codec(v: &Value) -> Value {
    v.as_str()
        .and_then(|s| {
            [
                "opus", "H264", "H265", "VP8", "VP9", "AV1", "AV1X", "rtx", "red",
            ]
            .into_iter()
            .find(|n| n.eq_ignore_ascii_case(s))
        })
        .map_or(Value::Null, |s| json!(s))
}
fn array(v: &Value) -> Vec<&Value> {
    v.as_array().map(|a| a.iter().collect()).unwrap_or_default()
}
fn videos<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    if v["videos"].is_array() {
        array(&v["videos"])
    } else {
        vec![&v[key]]
    }
}
fn sum(values: impl IntoIterator<Item = Value>) -> Value {
    let values: Vec<f64> = values.into_iter().filter_map(|v| v.as_f64()).collect();
    if values.is_empty() {
        Value::Null
    } else {
        n(&json!(values.iter().sum::<f64>()))
    }
}
fn pair(a: Value, b: Value) -> Value {
    match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => n(&json!(a + b)),
        _ => Value::Null,
    }
}
fn rtp(v: &Value, bytes: bool) -> Value {
    if !v.is_object() {
        return Value::Null;
    }
    let mut values = vec![];
    for x in [v, &v["fec"], &v["retransmitted"], &v["transmitted"]] {
        if x.is_object() {
            if bytes {
                values.extend([
                    n(&x["headerBytes"]),
                    n(&x["payloadBytes"]),
                    n(&x["paddingBytes"]),
                ]);
            } else {
                values.push(n(&x["packets"]));
            }
        }
    }
    sum(values)
}
fn crypt(v: &Value, sending: bool) -> Value {
    if sending {
        json!({"success":n(&v["encryptSuccessCount"]),"failure":n(&v["encryptFailureCount"]),"missingKey":n(&v["encryptMissingKeyCount"]),"passthrough":n(&v["passthroughCount"])})
    } else {
        json!({"success":n(&v["decryptSuccessCount"]),"failure":n(&v["decryptFailureCount"]),"missingKey":n(&v["decryptMissingKeyCount"]),"invalidNonce":n(&v["decryptInvalidNonceCount"]),"passthrough":n(&v["passthroughCount"])})
    }
}
fn audio(v: &Value, sending: bool) -> Value {
    if !v.is_object() {
        return Value::Null;
    }
    json!({"codec":codec(&v["codecName"]),"bytes":n(&v[if sending{"bytesSent"}else{"bytesReceived"}]),"packets":n(&v[if sending{"packetsSent"}else{"packetsReceived"}]),"packetsLost":n(&v["packetsLost"]),"bitrateBps":if sending{n(&v["mediaBitrate"])}else{Value::Null},"sampleRateHz":number(&v["currentSampleRate"],384000.),"audioDetected":boolean(&v["speaking"]),"framesCaptured":if sending{n(&v["framesCaptured"])}else{Value::Null},"framesRendered":if sending{n(&v["framesRendered"])}else{Value::Null},"crypto":crypt(v,sending)})
}
fn video_out(v: &Value) -> Value {
    if !v.is_object() {
        return Value::Null;
    }
    let subs = array(&v["substreams"]);
    let p = subs
        .iter()
        .copied()
        .find(|s| {
            s.is_object()
                && !s["isRTX"].as_bool().unwrap_or(false)
                && !s["isFlexFEC"].as_bool().unwrap_or(false)
        })
        .unwrap_or(&Value::Null);
    json!({"codec":codec(&v["codecName"]),"width":number(&p["width"],32768.),"height":number(&p["height"],32768.),"fpsInput":number(&v["inputFrameRate"],1000.),"fpsEncode":number(&v["encodeFrameRate"],1000.),"framesSent":pair(n(&p["frameCounts"]["keyFrames"]),n(&p["frameCounts"]["deltaFrames"])),"framesEncoded":n(&v["framesEncoded"]),"bytesSent":sum(subs.iter().map(|s|rtp(&s["rtpStats"],true))),"packetsSent":sum(subs.iter().map(|s|rtp(&s["rtpStats"],false))),"packetsLost":n(&p["rtcpStats"]["packetsLost"]),"bitrateBps":n(&v["mediaBitrate"]),"targetBitrateBps":n(&v["targetMediaBitrate"]),"cpuLimitedResolution":boolean(&v["cpuLimitedResolution"]),"bandwidthLimitedResolution":boolean(&v["bwLimitedResolution"]),"crypto":crypt(v,true)})
}
fn video_in(v: &Value) -> Value {
    if !v.is_object() {
        return Value::Null;
    }
    json!({"codec":codec(&v["codecName"]),"width":number(&v["width"],32768.),"height":number(&v["height"],32768.),"fpsNetwork":number(&v["networkFrameRate"],1000.),"fpsDecode":number(&v["decodeFrameRate"],1000.),"fpsRender":number(&v["renderFrameRate"],1000.),"framesReceived":pair(n(&v["frameCounts"]["keyFrames"]),n(&v["frameCounts"]["deltaFrames"])),"framesDecoded":n(&v["framesDecoded"]),"bytesReceived":rtp(&v["rtpStats"],true),"packetsReceived":rtp(&v["rtpStats"],false),"packetsLost":n(v["rtpStats"].get("packetsLost").unwrap_or(&v["rtcpStats"]["packetsLost"])),"bitrateBps":n(&v["totalBitrate"]),"crypto":crypt(v,false)})
}
pub fn sanitize(raw: &Value, state: &str, dave: bool) -> Value {
    let t = &raw["transport"];
    let out = &raw["outbound"];
    let incoming = array(&raw["inbound"]);
    let version = number(&t["secureFramesProtocolVersion"], 65535.);
    json!({"sanitized":true,"available":true,"state":if ["connecting","connected","disconnected","negotiating","waiting-peer","unknown"].contains(&state){state}else{"unknown"},"encryption":{"negotiatedByHelper":dave,"nativeDaveProtocolVersion":version,"nativeDaveEnabled":version.as_f64().map(|v|v>0.)},"transport":{"bytesSent":n(&t["bytesSent"]),"bytesReceived":n(&t["bytesReceived"]),"packetsSent":n(&t["packetsSent"]),"packetsReceived":n(&t["packetsReceived"]),"rttMs":n(&t["rtt"]),"sendBandwidthBps":n(&t["sendBandwidth"]),"outboundEstimateBps":n(&t["outboundBitrateEstimate"]),"inboundEstimateBps":n(&t["inboundBitrateEstimate"]),"decryptionFailures":n(&t["decryptionFailures"])},"outbound":{"audio":audio(&out["audio"],true),"video":videos(out,"video").into_iter().map(video_out).filter(|v|!v.is_null()).collect::<Vec<_>>()},"inbound":{"audio":incoming.iter().map(|v|audio(&v["audio"],false)).filter(|v|!v.is_null()).collect::<Vec<_>>(),"video":incoming.iter().flat_map(|v|videos(v,"video")).map(video_in).filter(|v|!v.is_null()).collect::<Vec<_>>()},"interval":{"durationMs":null,"sentBitrateBps":null,"receivedBitrateBps":null,"sentPacketsPerSecond":null,"receivedPacketsPerSecond":null}})
}
pub fn unavailable(state: &str, reason: &str) -> Value {
    json!({"sanitized":true,"available":false,"state":state,"reason":reason,"transport":null,"encryption":null,"outbound":null,"inbound":null,"interval":null})
}
pub fn add_interval(report: &mut Value, previous: &Value, seconds: f64) {
    if !(0.1..=300.).contains(&seconds) {
        return;
    }
    let mut interval = json!({"durationMs":(seconds*1000.).round()});
    for (out, key, factor) in [
        ("sentBitrateBps", "bytesSent", 8.),
        ("receivedBitrateBps", "bytesReceived", 8.),
        ("sentPacketsPerSecond", "packetsSent", 1.),
        ("receivedPacketsPerSecond", "packetsReceived", 1.),
    ] {
        interval[out] = match (report["transport"][key].as_f64(), previous[key].as_f64()) {
            (Some(a), Some(b)) if a >= b => n(&json!((a - b) * factor / seconds)),
            _ => Value::Null,
        };
    }
    report["interval"] = interval;
}
pub fn counters(report: &Value) -> Value {
    let t = &report["transport"];
    json!({"bytesSent":t["bytesSent"],"bytesReceived":t["bytesReceived"],"packetsSent":t["packetsSent"],"packetsReceived":t["packetsReceived"]})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stats_redact_unknown_fields() {
        let s = sanitize(
            &json!({"transport":{"bytesSent":20,"remoteAddress":"private","secureFramesProtocolVersion":1},"outbound":{"audio":{"codecName":"opus","bytesSent":12,"secret":"credential"}},"token":"voice-token"}),
            "connected",
            true,
        );
        let serialized = s.to_string();
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("credential"));
        assert!(!serialized.contains("voice-token"));
        assert_eq!(s["transport"]["bytesSent"], 20.);
        assert_eq!(s["transport"]["bytesReceived"], Value::Null);
        assert_eq!(s["encryption"]["nativeDaveEnabled"], true);
    }
    #[test]
    fn counter_resets_not_negative_rate() {
        let mut s = sanitize(
            &json!({"transport":{"bytesSent":10,"bytesReceived":30}}),
            "connected",
            true,
        );
        add_interval(&mut s, &json!({"bytesSent":20,"bytesReceived":10}), 1.);
        assert!(s["interval"]["sentBitrateBps"].is_null());
        assert_eq!(s["interval"]["receivedBitrateBps"], 160.);
    }
}
