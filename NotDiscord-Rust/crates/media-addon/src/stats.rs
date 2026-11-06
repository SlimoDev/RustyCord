use serde_json::{Value, json};
use std::time::{Duration, Instant};
#[path = "../../core/src/voice_diagnostics.rs"]
pub mod voice;
use voice::{AudioCounters, EndReason, InputState, Observation, VoiceRecord, VoiceSample};

/// One optional, explicitly started observation of the main voice connection.
/// The native lease survives timeout/cancel: a later capture must not overlap
/// native work whose completion has not been observed.
#[derive(Default)]
pub struct InputCapture {
    active: Option<ActiveCapture>,
    pending: Option<PendingSample>,
    next_query: u64,
}
struct ActiveCapture {
    capture: u64,
    started: Instant,
    next: Instant,
    samples: u8,
    generation: u32,
    epoch: Option<u64>,
    previous: Option<(Instant, AudioCounters)>,
}
struct PendingSample {
    capture: u64,
    query: u64,
    started: Instant,
    epoch: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePoll {
    Idle,
    Query { capture: u64, query: u64 },
    End { capture: u64, reason: EndReason },
}
impl InputCapture {
    pub fn active(&self) -> bool { self.active.is_some() }
    pub fn pending(&self) -> bool { self.pending.is_some() }
    pub fn start(&mut self, capture: u64, now: Instant) -> bool {
        if capture == 0 || self.active.is_some() || self.pending.is_some() { return false; }
        self.active = Some(ActiveCapture { capture, started: now, next: now, samples: 0,
            generation: 1, epoch: None, previous: None });
        true
    }
    pub fn cancel(&mut self, reason: EndReason) -> Option<VoiceRecord> {
        self.active.take().map(|active| VoiceRecord::End { capture: active.capture, reason })
    }
    pub fn cancel_capture(&mut self, capture: u64, reason: EndReason) -> Option<VoiceRecord> {
        if !self.active.as_ref().is_some_and(|active| active.capture == capture) { return None; }
        self.cancel(reason)
    }
    pub fn poll(&mut self, now: Instant, epoch: u64) -> CapturePoll {
        let Some(active) = self.active.as_ref() else { return CapturePoll::Idle; };
        if self.pending.as_ref().is_some_and(|pending| now.saturating_duration_since(pending.started) >= Duration::from_millis(800)) {
            let capture = active.capture;
            self.active = None;
            return CapturePoll::End { capture, reason: EndReason::StatsTimeout };
        }
        if now.saturating_duration_since(active.started) >= Duration::from_secs(10) {
            let capture = active.capture;
            self.active = None;
            return CapturePoll::End { capture, reason: EndReason::Completed };
        }
        if self.pending.is_some() || active.samples >= 10 || now < active.next { return CapturePoll::Idle; }
        let Some(query) = self.next_query.checked_add(1) else {
            let capture = active.capture;
            self.active = None;
            return CapturePoll::End { capture, reason: EndReason::StatsFailed };
        };
        self.next_query = query;
        let active = self.active.as_mut().unwrap();
        active.samples += 1;
        active.next = now + Duration::from_secs(1);
        self.pending = Some(PendingSample { capture: active.capture, query, started: now, epoch });
        CapturePoll::Query { capture: active.capture, query }
    }
    /// Releases only the exact native lease. Late/duplicate callbacks never
    /// advance a replacement capture or create a second request.
    pub fn complete(&mut self, capture: u64, query: u64, now: Instant, epoch: u64,
        input: InputState, report: Option<&Value>) -> Option<VoiceRecord> {
        if !self.pending.as_ref().is_some_and(|pending| pending.capture == capture && pending.query == query) { return None; }
        let pending = self.pending.take().unwrap();
        let active = self.active.as_mut().filter(|active| active.capture == capture)?;
        if now.saturating_duration_since(pending.started) >= Duration::from_millis(800) {
            return self.cancel(EndReason::StatsTimeout);
        }
        if now.saturating_duration_since(active.started) >= Duration::from_secs(10) {
            return self.cancel(EndReason::Completed);
        }
        let epoch_changed = active.epoch.is_some_and(|previous| previous != epoch) || pending.epoch != epoch;
        let audio = if pending.epoch == epoch { report.map(audio_counters).unwrap_or_default() } else { AudioCounters::default() };
        let counter_reset = active.previous.is_some_and(|(_, previous)| counters_decreased(previous, audio));
        if epoch_changed || counter_reset {
            active.generation = active.generation.saturating_add(1);
            active.previous = None;
        }
        active.epoch = Some(epoch);
        let (interval_ms, delta) = active.previous.map_or((None, AudioCounters::default()), |(at, previous)| {
            let elapsed = now.saturating_duration_since(at).as_millis();
            if elapsed == 0 || elapsed > 10_000 { (None, AudioCounters::default()) }
            else { (Some(elapsed as u16), AudioCounters {
                packets: subtract(audio.packets, previous.packets), bytes: subtract(audio.bytes, previous.bytes),
                frames_captured: subtract(audio.frames_captured, previous.frames_captured),
            }) }
        });
        active.previous = audio.available().then_some((now, audio));
        let observation = observe(&input, audio, delta, epoch_changed || counter_reset);
        Some(VoiceRecord::Sample { sample: VoiceSample { capture, index: active.samples,
            elapsed_ms: now.saturating_duration_since(active.started).as_millis().min(10_000) as u16,
            counter_generation: active.generation, interval_ms, input, audio, delta, observation } })
    }
    /// A failed call is not evidence that queued native work completed. Keep
    /// the lease until its callback or the owning connection is destroyed.
    pub fn rejected(&mut self, capture: u64, query: u64) -> Option<VoiceRecord> {
        if !self.pending.as_ref().is_some_and(|pending| pending.capture == capture && pending.query == query) { return None; }
        self.cancel(EndReason::StatsFailed)
    }
}
fn subtract(current: Option<u64>, previous: Option<u64>) -> Option<u64> { current?.checked_sub(previous?) }
fn counters_decreased(previous: AudioCounters, current: AudioCounters) -> bool {
    [(previous.packets, current.packets), (previous.bytes, current.bytes), (previous.frames_captured, current.frames_captured)]
        .into_iter().any(|(a, b)| matches!((a, b), (Some(a), Some(b)) if b < a))
}
fn counter(value: &Value) -> Option<u64> {
    value.as_f64().filter(|value| value.is_finite() && *value >= 0. && *value <= voice::MAX_COUNTER as f64 && value.fract() == 0.)
        .map(|value| value as u64)
}
/// Accept only sanitized main-connection AUDIO counters. Aggregate transport,
/// screen audio and video counters are deliberately not fallbacks.
pub fn audio_counters(report: &Value) -> AudioCounters {
    if report["sanitized"] != true || report["available"] != true { return AudioCounters::default(); }
    let audio = &report["outbound"]["audio"];
    AudioCounters { packets: counter(&audio["packets"]), bytes: counter(&audio["bytes"]), frames_captured: counter(&audio["framesCaptured"]) }
}
fn observe(input: &InputState, audio: AudioCounters, delta: AudioCounters, reset: bool) -> Observation {
    if input.sample_stale { Observation::InputStale }
    else if !input.desired { Observation::InputIdle }
    else if !input.transport_ready { Observation::TransportPending }
    else if !input.input_eligible { Observation::InputIneligible }
    else if !input.encryption_ready { Observation::EncryptionPending }
    else if !input.gate_open { Observation::GateClosed }
    else if reset { Observation::CounterReset }
    else if audio.packets.is_none() { Observation::CountersUnavailable }
    else { match delta.packets { Some(0) => Observation::AudioPacketsUnchanged,
        Some(_) => Observation::AudioPacketsProgress, None => Observation::CounterBaseline } }
}
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
    fn input() -> InputState {
        InputState { mode: voice::InputMode::ManualVad, callbacks: 20, last_callback_age_ms: Some(10),
            sample_stale: false, vad_detected: true, ptt_held: false, desired: true, gate_requested: true,
            gate_open: true, input_eligible: true, transport_ready: true, encryption_ready: true,
            muted: false, deaf: false, local_test_active: false, input_gain_zero: false }
    }
    fn query(capture: &mut InputCapture, now: Instant, epoch: u64) -> (u64, u64) {
        match capture.poll(now, epoch) { CapturePoll::Query { capture, query } => (capture, query),
            other => panic!("Expected query, got {other:?}") }
    }
    fn sample(record: Option<VoiceRecord>) -> VoiceSample {
        match record { Some(VoiceRecord::Sample { sample }) => sample, other => panic!("Expected sample, got {other:?}") }
    }
    fn report(packets: u64, bytes: u64) -> Value {
        sanitize(&json!({"transport":{"packetsSent":99999}, "outbound":{"audio":{"packetsSent":packets,"bytesSent":bytes}}}), "connected", true)
    }
    #[test]
    fn voluntary_capture_is_bounded_and_never_runs_when_idle() {
        let now = Instant::now(); let mut capture = InputCapture::default();
        assert_eq!(capture.poll(now, 1), CapturePoll::Idle);
        assert!(capture.start(7, now)); assert!(!capture.start(8, now));
        for index in 0..10 {
            let at = now + Duration::from_secs(index);
            let (run, token) = query(&mut capture, at, 1);
            assert_eq!(capture.poll(at, 1), CapturePoll::Idle);
            let result = sample(capture.complete(run, token, at + Duration::from_millis(20), 1, input(), Some(&report(index * 3, index * 60))));
            assert_eq!(result.index, index as u8 + 1); assert!(result.valid());
            assert_eq!(capture.poll(at + Duration::from_millis(999), 1), CapturePoll::Idle);
        }
        assert_eq!(capture.poll(now + Duration::from_secs(10), 1), CapturePoll::End { capture: 7, reason: EndReason::Completed });
        assert_eq!(capture.poll(now + Duration::from_secs(60), 1), CapturePoll::Idle);
    }
    #[test]
    fn timeout_cancel_and_native_error_keep_the_lease_until_exact_completion() {
        let now = Instant::now(); let mut capture = InputCapture::default();
        assert!(capture.start(1, now)); let (run, token) = query(&mut capture, now, 1);
        assert_eq!(capture.poll(now + Duration::from_millis(800), 1), CapturePoll::End { capture: 1, reason: EndReason::StatsTimeout });
        assert!(capture.pending()); assert!(!capture.start(2, now + Duration::from_secs(1)));
        assert!(capture.complete(run, token + 1, now, 1, input(), None).is_none()); assert!(capture.pending());
        assert!(capture.complete(run, token, now + Duration::from_secs(2), 1, input(), None).is_none());
        assert!(!capture.pending()); assert!(capture.start(2, now + Duration::from_secs(2)));
        let (run, token) = query(&mut capture, now + Duration::from_secs(2), 1);
        assert_eq!(capture.cancel(EndReason::Cancelled), Some(VoiceRecord::End { capture: 2, reason: EndReason::Cancelled }));
        assert!(!capture.start(3, now));
        assert!(capture.complete(run, token, now + Duration::from_secs(2), 1, input(), None).is_none());
        assert!(capture.start(3, now + Duration::from_secs(3)));
        let (run, token) = query(&mut capture, now + Duration::from_secs(3), 1);
        assert_eq!(capture.rejected(run, token), Some(VoiceRecord::End { capture: 3, reason: EndReason::StatsFailed }));
        assert!(capture.pending()); assert!(!capture.start(4, now));
    }
    #[test]
    fn only_main_audio_counters_form_deltas_and_rekey_or_reset_rebases() {
        let now = Instant::now(); let mut capture = InputCapture::default(); assert!(capture.start(1, now));
        let (run, token) = query(&mut capture, now, 1);
        let first = sample(capture.complete(run, token, now, 1, input(), Some(&report(5, 100))));
        assert_eq!(first.audio.packets, Some(5)); assert_eq!(first.delta.packets, None);
        let at = now + Duration::from_secs(1); let (run, token) = query(&mut capture, at, 1);
        let next = sample(capture.complete(run, token, at, 1, input(), Some(&report(8, 160))));
        assert_eq!(next.delta.packets, Some(3)); assert_eq!(next.delta.bytes, Some(60));
        assert_eq!(next.observation, Observation::AudioPacketsProgress);
        let at = now + Duration::from_secs(2); let (run, token) = query(&mut capture, at, 1);
        let reset = sample(capture.complete(run, token, at, 1, input(), Some(&report(1, 20))));
        assert_eq!(reset.counter_generation, 2); assert_eq!(reset.delta.packets, None); assert_eq!(reset.observation, Observation::CounterReset);
        let at = now + Duration::from_secs(3); let (run, token) = query(&mut capture, at, 1);
        let rekey = sample(capture.complete(run, token, at, 2, input(), Some(&report(200, 4000))));
        assert_eq!(rekey.counter_generation, 3); assert_eq!(rekey.audio.packets, None); assert_eq!(rekey.delta.packets, None);
        let aggregate = json!({"sanitized":true,"available":true,"transport":{"packetsSent":500},"outbound":{"video":[{"packetsSent":100}]}});
        assert!(!audio_counters(&aggregate).available());
        assert!(!audio_counters(&json!({"outbound":{"audio":{"packets":90}}})).available());
    }
    #[test]
    fn absent_or_fractional_counters_are_unknown_not_zero_and_break_baseline() {
        let now = Instant::now(); let mut capture = InputCapture::default(); assert!(capture.start(1, now));
        for (index, report) in [Some(report(10, 20)), None, Some(report(30, 60))].iter().enumerate() {
            let at = now + Duration::from_secs(index as u64); let (run, token) = query(&mut capture, at, 1);
            let result = sample(capture.complete(run, token, at, 1, input(), report.as_ref()));
            assert_eq!(result.delta.packets, None);
            if index == 1 { assert_eq!(result.audio.packets, None); assert_eq!(result.observation, Observation::CountersUnavailable); }
        }
        assert_eq!(counter(&json!(1.5)), None); assert_eq!(counter(&json!(-1)), None);
        assert_eq!(counter(&json!(9_007_199_254_740_992u64)), None);
        assert_eq!(counter(&json!(0.0)), Some(0));
    }
}
