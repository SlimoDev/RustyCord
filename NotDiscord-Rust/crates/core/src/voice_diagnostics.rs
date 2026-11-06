//! Closed, local microphone telemetry shared with the media helper. No account,
//! endpoint, device identity, samples or native payload is part of this schema.
use serde::{Deserialize, Serialize};

pub const MAX_COUNTER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InputMode { ManualVad, Ptt }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputState {
    pub mode: InputMode,
    pub callbacks: u32,
    pub last_callback_age_ms: Option<u16>,
    pub sample_stale: bool,
    pub vad_detected: bool,
    pub ptt_held: bool,
    pub desired: bool,
    pub gate_requested: bool,
    pub gate_open: bool,
    pub input_eligible: bool,
    pub transport_ready: bool,
    pub encryption_ready: bool,
    pub muted: bool,
    pub deaf: bool,
    pub local_test_active: bool,
    pub input_gain_zero: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioCounters {
    pub packets: Option<u64>,
    pub bytes: Option<u64>,
    pub frames_captured: Option<u64>,
}
impl AudioCounters {
    pub fn available(self) -> bool { self.packets.is_some() || self.bytes.is_some() || self.frames_captured.is_some() }
    pub fn valid(self) -> bool { [self.packets, self.bytes, self.frames_captured].into_iter().flatten().all(|value| value <= MAX_COUNTER) }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Observation {
    InputStale, InputIdle, InputIneligible, TransportPending, EncryptionPending,
    GateClosed, CountersUnavailable, CounterBaseline, CounterReset,
    AudioPacketsProgress, AudioPacketsUnchanged,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceSample {
    /// Ephemeral local capture token, never a user, channel or media-session ID.
    pub capture: u64,
    pub index: u8,
    pub elapsed_ms: u16,
    /// Changes on rekey or decreasing counters. Deltas never cross generations.
    pub counter_generation: u32,
    pub interval_ms: Option<u16>,
    pub input: InputState,
    pub audio: AudioCounters,
    pub delta: AudioCounters,
    pub observation: Observation,
}
impl VoiceSample {
    pub fn valid(&self) -> bool {
        self.capture > 0 && (1..=10).contains(&self.index) && self.elapsed_ms <= 10_000
            && self.counter_generation > 0 && self.interval_ms.is_none_or(|ms| ms > 0 && ms <= 10_000)
            && self.audio.valid() && self.delta.valid()
            && (self.interval_ms.is_some() || !self.delta.available())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EndReason { Completed, Cancelled, ConnectionChanged, StatsTimeout, StatsFailed }

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum VoiceRecord {
    Start { capture: u64 },
    Sample { sample: VoiceSample },
    End { capture: u64, reason: EndReason },
}
impl VoiceRecord {
    pub fn valid(&self) -> bool {
        match self { Self::Sample { sample } => sample.valid(), Self::Start { capture } | Self::End { capture, .. } => *capture > 0 }
    }
}
