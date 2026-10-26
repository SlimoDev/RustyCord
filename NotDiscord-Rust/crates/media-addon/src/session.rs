use crate::protocol::{Credentials, MediaError, Result};
use crate::settings::AudioSettings;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct ShareSelection {
    pub source: String,
    pub height: u32,
    pub fps: u32,
    pub audio: bool,
    pub loopback: bool,
    pub pid: u32,
}
impl ShareSelection {
    pub fn parse(p: &Value) -> Result<Self> {
        let source = crate::protocol::string(&p["sourceId"], "sourceId", 512)?;
        let parts: Vec<_> = source.split(':').collect();
        if !(parts.len() == 2 || parts.len() == 3)
            || !matches!(parts[0], "screen" | "screen-handle" | "window")
            || parts[1..].iter().any(|s| s.is_empty())
        {
            return Err(MediaError::new(
                "INVALID_SOURCE",
                "Choose a source returned by listSources.",
            ));
        }
        let height = p["quality"]["height"]
            .as_u64()
            .filter(|x| *x > 0)
            .unwrap_or(720) as u32;
        let fps = p["quality"]["fps"]
            .as_u64()
            .filter(|x| *x > 0)
            .unwrap_or(30) as u32;
        if ![480, 720, 1080, 1440].contains(&height) || ![15, 30, 60].contains(&fps) {
            return Err(MediaError::new(
                "INVALID_QUALITY",
                "Unsupported screen sharing quality.",
            ));
        }
        let audio = p["audio"]["enabled"].as_bool().unwrap_or(false);
        let loopback = p["audio"]["loopback"].as_bool().unwrap_or(false);
        let pid = p["audio"]["pid"].as_u64().unwrap_or(0);
        if audio && (pid > u32::MAX as u64 || !loopback && pid == 0) {
            return Err(MediaError::new(
                "INVALID_AUDIO_SOURCE",
                "Window audio requires a valid source process; system audio must be explicitly selected.",
            ));
        }
        Ok(Self {
            source,
            height,
            fps,
            audio,
            loopback,
            pid: pid as u32,
        })
    }
}

pub struct Session {
    pub id: u64,
    pub credentials: Credentials,
    pub settings: AudioSettings,
    pub closed: bool,
    pub local_ready: bool,
    pub connected: bool,
    pub session_description: bool,
    pub dave_ready: bool,
    pub seq: i64,
    pub ssrc: u64,
    pub members: HashMap<String, Value>,
    pub volumes: HashMap<String, f64>,
    pub volume_requests: HashMap<String, u64>,
    pub speaking: HashMap<String, bool>,
    pub transitions: HashSet<u16>,
    pub epoch: u64,
    pub failed_mls: u8,
    pub mode: String,
    pub native_ready: Value,
    pub codecs: Vec<Value>,
    pub video_codec: Option<Value>,
    pub receive_requested_ssrc: Option<u64>,
    pub camera_receive_users: HashSet<String>,
    pub camera_requested_ssrcs: HashSet<u64>,
    pub camera_subscription_sent: bool,
    pub camera_sink_wants: Value,
    pub streams: Vec<Value>,
    pub camera_requested: bool,
    pub camera_token: u64,
    pub operation_token: u64,
    pub camera_active: bool,
    pub camera_request: Option<Value>,
    pub camera_deadline: Option<Instant>,
    pub ptt_active: bool,
    pub transmit_gate: bool,
    pub input_observed_at: Option<Instant>,
    pub manual_release_at: Option<Instant>,
    pub local_vad: bool,
    pub share_selection: Option<ShareSelection>,
    pub suspended_share: Option<ShareSelection>,
    pub capture_active: bool,
    pub clearing_source: bool,
    pub soundshare_requested: bool,
    pub soundshare_attached: bool,
    pub connect_deadline: Option<Instant>,
    pub udp_deadline: Option<Instant>,
    pub dave_deadline: Option<Instant>,
    pub heartbeat_interval: Option<Duration>,
    pub next_heartbeat: Option<Instant>,
    pub heartbeat_pending: Option<Instant>,
}
impl Session {
    pub fn new(id: u64, credentials: Credentials, settings: AudioSettings) -> Self {
        let volumes=settings.user_volumes.clone();
        Self {
            id,
            credentials,
            settings,
            closed: false,
            local_ready: false,
            connected: false,
            session_description: false,
            dave_ready: false,
            seq: -1,
            ssrc: 0,
            members: HashMap::new(),
            volumes,
            volume_requests: HashMap::new(),
            speaking: HashMap::new(),
            transitions: HashSet::new(),
            epoch: 0,
            failed_mls: 0,
            mode: String::new(),
            native_ready: Value::Null,
            codecs: vec![],
            video_codec: None,
            receive_requested_ssrc: None,
            camera_receive_users: HashSet::new(),
            camera_requested_ssrcs: HashSet::new(),
            camera_subscription_sent: false,
            camera_sink_wants: Value::Null,
            streams: vec![],
            camera_requested: false,
            camera_token: 0,
            operation_token: 0,
            camera_active: false,
            camera_request: None,
            camera_deadline: None,
            ptt_active: false,
            transmit_gate: false,
            input_observed_at: None,
            manual_release_at: None,
            local_vad: false,
            share_selection: None,
            suspended_share: None,
            capture_active: false,
            clearing_source: false,
            soundshare_requested: false,
            soundshare_attached: false,
            connect_deadline: Some(Instant::now() + Duration::from_secs(30)),
            udp_deadline: None,
            dave_deadline: None,
            heartbeat_interval: None,
            next_heartbeat: None,
            heartbeat_pending: None,
        }
    }
    pub fn stream(&self) -> bool {
        self.credentials.stream
    }
    pub fn receive(&self) -> bool {
        self.stream() && self.credentials.stream_owner != self.credentials.user_id
    }
    pub fn kind(&self) -> &str {
        if self.receive() {
            "receive"
        } else if self.stream() {
            "share"
        } else {
            "voice"
        }
    }
    pub fn state(&self) -> &str {
        if self.closed {
            "disconnected"
        } else if self.connected {
            "connected"
        } else if self.local_ready && self.members.is_empty() {
            "waiting-peer"
        } else if self.local_ready || self.ssrc > 0 {
            "negotiating"
        } else {
            "connecting"
        }
    }
    pub fn state_event(&self, state: &str, reason: Option<MediaError>) -> Value {
        let mut v = json!({"event":if self.receive(){"watchState"}else if self.stream(){"shareState"}else{"voiceState"},"mediaSession":self.id,"state":state,"kind":self.kind(),"transportReady":self.local_ready,"encryptionReady":self.connected,"streamKey":self.credentials.stream_key,"userId":self.credentials.stream_owner,"token":self.operation_token});
        if !self.stream() {
            v["cameraAvailable"] = json!(
                self.connected
                    && self
                        .video_codec
                        .as_ref()
                        .is_some_and(|codec| codec["encode"] == true)
                    && !self.streams.is_empty()
            );
        }
        if let Some(reason) = reason {
            v["reason"] = reason.json();
        }
        v
    }
    pub fn local_mic_active(&self) -> bool {
        self.local_ready && !self.connected && !self.closed && !self.stream()
    }
    pub fn send_mute(&self) -> bool {
        self.stream() || !self.connected || self.credentials.self_mute || self.credentials.self_deaf
    }
    pub fn begin_epoch(&mut self) {
        self.epoch += 1;
        self.dave_ready = false;
        self.connected = false;
        self.reset_input_activity();
        self.transitions.clear();
        self.dave_deadline = None;
        if let Some(selection) = self.share_selection.take() {
            self.suspended_share = Some(selection);
        }
        self.capture_active = false;
        self.soundshare_requested = false;
        self.soundshare_attached = false;
    }
    pub fn reset_input_activity(&mut self) {
        self.transmit_gate=false;self.input_observed_at=None;self.manual_release_at=None;
    }
    pub fn input_activity(&mut self,now:Instant,above:bool,has_signal:bool,eligible:bool)->bool {
        self.input_observed_at=Some(now);
        if !eligible {self.manual_release_at=None;return false;}
        if self.settings.ptt {self.manual_release_at=None;return self.ptt_active&&has_signal;}
        if above {self.manual_release_at=Some(now+Duration::from_millis(250));return true;}
        self.manual_release_at.is_some_and(|deadline|now<deadline)
    }
    pub fn input_expired(&self,now:Instant)->bool {
        // The host ticks every 100 ms: a 400 ms stale deadline bounds normal
        // shutdown latency to approximately half a second without new samples.
        self.input_observed_at.is_none_or(|at|now.saturating_duration_since(at)>=Duration::from_millis(400))
            || !self.settings.ptt&&self.manual_release_at.is_none_or(|at|now>=at)
    }
    pub fn update_wait(&mut self, now: Instant) {
        if self.closed || !self.local_ready || self.connected {
            return;
        }
        self.connect_deadline = None;
        if self.members.is_empty() {
            self.dave_deadline = None;
        } else if self.dave_deadline.is_none() {
            self.dave_deadline = Some(now + Duration::from_secs(30));
        }
    }
    pub fn establish(&mut self) -> bool {
        if self.closed || self.connected || !self.session_description || !self.dave_ready {
            return false;
        }
        self.local_ready = true;
        self.connected = true;
        self.local_vad = false;
        self.connect_deadline = None;
        self.dave_deadline = None;
        true
    }
    pub fn cancel_share(&mut self) {
        self.share_selection = None;
        self.suspended_share = None;
        self.capture_active = false;
        self.soundshare_requested = false;
        self.soundshare_attached = false;
    }
    pub fn add_user(&mut self, id: &str, ssrc: u64) -> bool {
        if id == self.credentials.user_id
            || id.is_empty()
            || id.len() > 20
            || !id.bytes().all(|c| c.is_ascii_digit())
        {
            return false;
        }
        let previous = self.members.get(id);
        if previous.is_none() && self.members.len() >= 256 {
            return false;
        }
        let fresh = previous.is_none();
        let user = json!({"id":id,"ssrc":if ssrc>0{ssrc}else{previous.and_then(|v|v["ssrc"].as_u64()).unwrap_or(0)},"videoSsrc":previous.map(|v|v["videoSsrc"].clone()).unwrap_or(json!(0)),"videoSsrcs":previous.map(|v|v["videoSsrcs"].clone()).unwrap_or(json!([])),"rtxSsrc":previous.map(|v|v["rtxSsrc"].clone()).unwrap_or(json!(0)),"mute":false,"volume":self.volumes.get(id).copied().or_else(||previous.and_then(|p|p["volume"].as_f64())).unwrap_or(1.)});
        self.members.insert(id.to_owned(), user);
        fresh
    }
    pub fn video_user(&mut self, id: &str, audio: u64, video: u64, rtx: u64, streams: &Value) {
        self.add_user(id, audio);
        let Some(user) = self.members.get_mut(id) else {
            return;
        };
        let videos: Vec<u64> = streams
            .as_array()
            .filter(|streams| !streams.is_empty())
            .map(|streams| {
                streams
                    .iter()
                    .take(8)
                    .filter_map(|stream| {
                        if stream["active"] == false {
                            return None;
                        }
                        stream["ssrc"]
                            .as_u64()
                            .filter(|ssrc| *ssrc > 0 && *ssrc <= u32::MAX as u64)
                    })
                    .collect()
            })
            .unwrap_or_else(|| {
                (video > 0 && video <= u32::MAX as u64)
                    .then_some(video)
                    .into_iter()
                    .collect()
            });
        user["videoSsrc"] = json!(videos.first().copied().unwrap_or(0));
        user["videoSsrcs"] = json!(videos);
        user["rtxSsrc"] = json!(if rtx > 0 && rtx <= u32::MAX as u64 {
            rtx
        } else {
            videos
                .first()
                .copied()
                .filter(|value| *value < u32::MAX as u64)
                .map(|value| value + 1)
                .unwrap_or(0)
        });
    }
    pub fn speaking_event(&mut self, id: &str, active: bool) -> Option<Value> {
        if self.stream()
            || id.is_empty()
            || id.len() > 20
            || !id.bytes().all(|c| c.is_ascii_digit())
            || !self.speaking.contains_key(id) && self.speaking.len() >= 256
            || self.speaking.get(id) == Some(&active)
        {
            return None;
        }
        self.speaking.insert(id.to_owned(), active);
        Some(
            json!({"event":"speaking","userId":id,"self":id==self.credentials.user_id,"active":active}),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> Session {
        Session::new(1,Credentials::parse(&json!({"userId":"1","channelId":"2","sessionId":"s","voiceToken":"t","endpoint":"x.discord.media"}),false).unwrap(),AudioSettings::default())
    }
    #[test]
    fn missing_video_rtx_uses_shipped_engine_ssrc_pairing_without_overflow() {
        let mut s = session();
        s.video_user("3", 7, 20, 0, &json!([]));
        assert_eq!(s.members["3"]["rtxSsrc"], 21);
        s.video_user("3", 7, 20, 99, &json!([]));
        assert_eq!(s.members["3"]["rtxSsrc"], 99);
        s.video_user("3", 7, 0, 0, &json!([]));
        assert_eq!(s.members["3"]["rtxSsrc"], 0);
        s.video_user("3", 7, u32::MAX as u64, 0, &json!([]));
        assert_eq!(s.members["3"]["rtxSsrc"], 0);
    }
    #[test]
    fn manual_release_hold_is_bounded_and_all_guards_reset_it_immediately() {
        let mut s=session();let now=Instant::now();
        assert!(!s.input_activity(now,false,true,true));
        assert!(s.input_activity(now,true,true,true));
        assert!(s.input_activity(now+Duration::from_millis(249),false,true,true));
        assert!(s.input_expired(now+Duration::from_millis(250)));
        assert!(!s.input_activity(now+Duration::from_millis(250),false,true,true));
        assert!(s.input_activity(now+Duration::from_millis(300),true,true,true));
        assert!(!s.input_activity(now+Duration::from_millis(301),true,true,false));
        assert!(!s.input_activity(now+Duration::from_millis(302),false,true,true));
        s.reset_input_activity();assert!(s.input_expired(now+Duration::from_millis(303)));
    }
    #[test]
    fn ptt_ignores_manual_threshold_and_stale_input_expires_in_half_a_second() {
        let mut s=session();let now=Instant::now();s.settings.ptt=true;
        assert!(!s.input_activity(now,true,true,true));s.ptt_active=true;
        assert!(s.input_activity(now,false,true,true));
        assert!(!s.input_expired(now+Duration::from_millis(399)));
        assert!(s.input_expired(now+Duration::from_millis(400)));
        s.ptt_active=false;assert!(!s.input_activity(now+Duration::from_millis(401),true,true,true));
    }
    #[test]
    fn solo_is_local_not_network_ready() {
        let mut s = session();
        s.local_ready = true;
        s.session_description = true;
        s.update_wait(Instant::now());
        assert_eq!(s.state(), "waiting-peer");
        assert!(s.local_mic_active());
        assert!(s.send_mute());
        assert!(s.dave_deadline.is_none());
        assert!(!s.establish());
        assert_eq!(s.state_event(s.state(), None)["encryptionReady"], false);
    }
    #[test]
    fn peers_start_bounded_encryption_wait() {
        let mut s = session();
        s.local_ready = true;
        s.add_user("3", 8);
        let now = Instant::now();
        s.update_wait(now);
        assert_eq!(s.dave_deadline, Some(now + Duration::from_secs(30)));
        s.members.clear();
        s.update_wait(now);
        assert!(s.dave_deadline.is_none());
    }
    #[test]
    fn epoch_revokes_send_and_preserves_only_selection() {
        let mut s = session();
        s.local_ready = true;
        s.session_description = true;
        s.dave_ready = true;
        assert!(s.establish());
        assert!(!s.send_mute());
        s.share_selection = Some(
            ShareSelection::parse(
                &json!({"sourceId":"window:12","audio":{"enabled":true,"pid":12}}),
            )
            .unwrap(),
        );
        s.capture_active = true;
        s.soundshare_requested = true;
        s.transitions.insert(8);
        s.begin_epoch();
        assert!(s.send_mute());
        assert!(!s.capture_active);
        assert!(s.transitions.is_empty());
        assert_eq!(s.suspended_share.as_ref().unwrap().source, "window:12");
        s.cancel_share();
        assert!(s.suspended_share.is_none());
    }
    #[test]
    fn source_and_audio_are_explicit() {
        assert!(ShareSelection::parse(&json!({"sourceId":"http:evil"})).is_err());
        assert!(
            ShareSelection::parse(&json!({"sourceId":"window:12","audio":{"enabled":true}}))
                .is_err()
        );
        assert!(ShareSelection::parse(&json!({"sourceId":"screen-handle:12","audio":{"enabled":true,"loopback":true,"pid":0}})).is_ok());
        assert!(
            ShareSelection::parse(&json!({"sourceId":"window:12","quality":{"fps":120}})).is_err()
        );
    }
    #[test]
    fn dedup_speaking() {
        let mut s = session();
        assert!(s.speaking_event("1", true).is_some());
        assert!(s.speaking_event("1", true).is_none());
        assert!(s.speaking_event("1", false).is_some());
    }
    #[test]
    fn remote_video_ssrcs_survive_audio_presence_and_are_cleared_only_explicitly() {
        let mut s = session();
        s.video_user("3", 7, 90, 91, &json!([{"ssrc":90},{"ssrc":100}]));
        s.add_user("3", 7);
        s.add_user("3", 0);
        assert_eq!(s.members["3"]["videoSsrcs"], json!([90, 100]));
        assert_eq!(s.members["3"]["rtxSsrc"], 91);
        s.video_user("3", 7, 90, 91, &json!([{"ssrc":90,"active":false}]));
        assert_eq!(s.members["3"]["videoSsrcs"], json!([]));
        s.video_user("3", 7, 0, 0, &json!([]));
        assert_eq!(s.members["3"]["videoSsrcs"], json!([]));
    }
    #[test]
    fn receive_only_stream_stays_muted_even_after_dave() {
        let mut s = session();
        s.credentials.stream = true;
        s.credentials.stream_owner = "3".into();
        s.credentials.stream_key = "call:2:3".into();
        s.local_ready = true;
        s.dave_ready = true;
        s.session_description = true;
        assert!(s.establish());
        assert!(s.receive() && s.send_mute());
        assert_eq!(s.kind(), "receive");
        assert_eq!(s.state_event("connected", None)["event"], "watchState");
        assert!(!s.capture_active && s.suspended_share.is_none());
    }
    #[test]
    fn initial_share_selection_survives_first_epoch() {
        let mut s = session();
        s.suspended_share =
            Some(ShareSelection::parse(&json!({"sourceId":"screen-handle:12"})).unwrap());
        s.begin_epoch();
        assert_eq!(
            s.suspended_share.as_ref().unwrap().source,
            "screen-handle:12"
        );
        s.begin_epoch();
        assert!(s.suspended_share.is_some());
        s.cancel_share();
        s.begin_epoch();
        assert!(s.suspended_share.is_none());
    }
    #[test]
    fn call_settings_do_not_unmute_before_dave() {
        let mut s = session();
        s.local_ready = true;
        s.credentials.self_mute = false;
        s.settings.ptt = false;
        assert!(s.send_mute());
        s.session_description = true;
        s.dave_ready = true;
        assert!(s.establish());
        s.credentials.self_deaf = true;
        assert!(s.send_mute());
        s.credentials.self_deaf = false;
        assert!(!s.send_mute());
    }
    #[test]
    fn no_false_peer_for_self_or_invalid_id() {
        let mut s = session();
        assert!(!s.add_user("1", 5));
        assert!(!s.add_user("not-user", 8));
        assert!(s.members.is_empty());
        assert!(s.add_user("3", 8));
        s.volumes.insert("3".into(), 0.5);
        assert!(!s.add_user("3", 0));
        assert_eq!(s.members["3"]["ssrc"], 8);
        assert_eq!(s.members["3"]["volume"], 0.5);
    }
    #[test]
    fn repeated_updates_dont_extend_peer_timeout() {
        let mut s = session();
        s.local_ready = true;
        s.add_user("3", 4);
        let now = Instant::now();
        s.update_wait(now);
        let deadline = s.dave_deadline;
        s.update_wait(now + Duration::from_secs(10));
        assert_eq!(deadline, s.dave_deadline);
        s.session_description = true;
        s.dave_ready = true;
        assert!(s.establish());
        assert!(s.dave_deadline.is_none());
    }
}
