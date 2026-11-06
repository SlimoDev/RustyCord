//! Protocol state retained by the host, independently of an individual socket.
//! This module never admits an attempt, creates a connection or owns a timer.
use crate::protocol::{Credentials, Frame, MediaError, Result};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Initial,
    Suspended,
    Prepared,
    AwaitingResumed,
    Live,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResumeObservation {
    Frame,
    Resumed,
}

/// No credentials are copied into this state or persisted. The host retains its
/// existing credentials and native/DAVE connection for a candidate Resume.
pub(crate) struct ResumeState {
    generation: u64,
    seq_ack: i64,
    phase: Phase,
    hello_seen: bool,
}
impl Default for ResumeState {
    fn default() -> Self {
        Self::new(0)
    }
}
impl ResumeState {
    pub(crate) fn new(generation: u64) -> Self {
        Self { generation, seq_ack: -1, phase: Phase::Initial, hello_seen: false }
    }
    pub(crate) fn generation(&self) -> u64 { self.generation }
    pub(crate) fn seq_ack(&self) -> i64 { self.seq_ack }
    pub(crate) fn accepts(&self, generation: u64) -> bool {
        generation == self.generation && self.phase != Phase::Suspended
    }
    pub(crate) fn suspend(&mut self) {
        self.phase = Phase::Suspended;
    }
    /// Admission is external: the shared episode must already own a permit.
    /// Calling this method cannot retry or fall back to an Identify.
    pub(crate) fn begin_resume(&mut self, generation: u64) -> Result<()> {
        if self.phase != Phase::Suspended || generation <= self.generation {
            return Err(invalid_resume());
        }
        self.generation = generation;
        self.phase = Phase::Prepared;
        self.hello_seen = false;
        Ok(())
    }
    /// The complete op7 packet. seq_ack is the last *applied* frame, including
    /// binary DAVE frames, rather than a frame waiting in the actor mailbox.
    pub(crate) fn resume_payload(&self, credentials: &Credentials) -> Result<Value> {
        if self.phase != Phase::Prepared
            || credentials.server_id.is_empty()
            || credentials.session_id.is_empty()
            || credentials.voice_token.is_empty()
        {
            return Err(invalid_resume());
        }
        Ok(json!({"op":7,"d":{
            "server_id":credentials.server_id,
            "session_id":credentials.session_id,
            "token":credentials.voice_token,
            "seq_ack":self.seq_ack,
        }}))
    }
    /// Only after successful queue admission of the one op7. A write failure
    /// subsequently fails the episode; it never emits an Identify alternative.
    pub(crate) fn resume_sent(&mut self) -> Result<()> {
        if self.phase != Phase::Prepared { return Err(invalid_resume()); }
        self.phase = Phase::AwaitingResumed;
        Ok(())
    }
    pub(crate) fn is_resuming(&self) -> bool {
        matches!(self.phase, Phase::Prepared | Phase::AwaitingResumed)
    }
    pub(crate) fn can_transmit(&self, dave_ready: bool, native_connection_alive: bool) -> bool {
        matches!(self.phase, Phase::Initial | Phase::Live)
            && dave_ready && native_connection_alive
    }
    /// Reject an invalid handshake before it can mutate retained native state.
    pub(crate) fn validate(&self, frame: &Frame) -> Result<()> {
        if self.phase == Phase::Suspended || self.is_resuming() && matches!(frame.op, 2 | 4)
            || frame.op == 9 && (self.phase != Phase::AwaitingResumed || !self.hello_seen || !frame.data.is_null()) {
            return Err(invalid_resume());
        }
        Ok(())
    }
    /// Invoke only after the current generation's frame was successfully
    /// applied to native/DAVE state. An unprocessed commit must not be ACKed.
    pub(crate) fn observe(&mut self, frame: &Frame) -> Result<ResumeObservation> {
        self.validate(frame)?;
        if frame.op == 8 { self.hello_seen = true; }
        if self.phase == Phase::Suspended { return Err(invalid_resume()); }
        if self.is_resuming() && matches!(frame.op, 2 | 4) {
            // A Resume is never silently changed into a fresh handshake.
            return Err(invalid_resume());
        }
        let observation = if frame.op == 9 {
            if self.phase != Phase::AwaitingResumed || !frame.data.is_null() {
                return Err(invalid_resume());
            }
            self.phase = Phase::Live;
            ResumeObservation::Resumed
        } else {
            ResumeObservation::Frame
        };
        if let Some(seq) = frame.seq {
            // Messages arrive in order; fixed-width server sequence numbers
            // wrap. max(old, new) would permanently ACK the wrong DAVE epoch.
            self.seq_ack = i64::try_from(seq).map_err(|_| invalid_resume())?;
        }
        Ok(observation)
    }
    /// Used only for a frame admitted before suspension whose native callback
    /// has now completed. This never grants transport readiness while stopped.
    pub(crate) fn observe_applied(&mut self, frame: &Frame) -> Result<ResumeObservation> {
        if self.phase == Phase::Suspended {
            if let Some(seq) = frame.seq { self.seq_ack = i64::try_from(seq).map_err(|_| invalid_resume())?; }
            Ok(ResumeObservation::Frame)
        } else { self.observe(frame) }
    }
}
fn invalid_resume() -> MediaError {
    MediaError::new("VOICE_RESUME_INVALID", "No se pudo reanudar de forma segura la sesión de voz conservada.")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(op: u64, seq: Option<u64>) -> Frame {
        Frame { op, seq, data: Value::Null, bytes: vec![] }
    }
    fn credentials() -> Credentials {
        Credentials::parse(&json!({"userId":"1","channelId":"2","sessionId":"fixture-session",
            "voiceToken":"fixture-voice-token","endpoint":"fixture.discord.media"}), false).unwrap()
    }
    #[test]
    fn buffered_resume_uses_last_applied_sequence_including_wrapping_binary_frames() {
        let mut state = ResumeState::default();
        let mut binary = frame(29, Some(65535)); binary.bytes = vec![1, 2, 3];
        state.observe(&binary).unwrap();
        state.observe(&frame(22, Some(0))).unwrap();
        state.suspend(); state.begin_resume(1).unwrap();
        let packet = state.resume_payload(&credentials()).unwrap();
        assert_eq!(packet, json!({"op":7,"d":{"server_id":"2","session_id":"fixture-session",
            "token":"fixture-voice-token","seq_ack":0}}));
        assert!(!state.can_transmit(true, true));
        state.resume_sent().unwrap();
        assert!(!state.can_transmit(true, true));
        assert!(state.observe(&frame(9, None)).is_err());
        state.observe(&frame(8, None)).unwrap();
        assert_eq!(state.observe(&frame(9, None)).unwrap(), ResumeObservation::Resumed);
        assert!(state.can_transmit(true, true));
        assert!(!state.can_transmit(false, true));
        assert!(!state.can_transmit(true, false));
    }
    #[test]
    fn unsolicited_or_duplicate_resumed_never_grants_permission() {
        let mut state = ResumeState::default();
        assert!(state.observe(&frame(9, None)).is_err());
        state.suspend(); state.begin_resume(1).unwrap();
        assert!(state.observe(&frame(9, None)).is_err());
        assert!(!state.can_transmit(true, true));
        state.resume_sent().unwrap(); state.observe(&frame(8, None)).unwrap(); state.observe(&frame(9, None)).unwrap();
        assert!(state.observe(&frame(9, None)).is_err());
        state.suspend();
        assert!(!state.can_transmit(true, true));
        assert!(!state.accepts(1));
        state.begin_resume(2).unwrap();
        assert!(!state.accepts(1)); assert!(state.accepts(2));
    }
    #[test]
    fn failed_resume_never_prepares_identify_or_accepts_fresh_session_description() {
        let mut state = ResumeState::default();
        state.suspend(); state.begin_resume(1).unwrap();
        assert_eq!(state.resume_payload(&credentials()).unwrap()["d"]["seq_ack"], -1);
        state.resume_sent().unwrap();
        assert!(state.observe(&frame(2, None)).is_err());
        assert!(state.observe(&frame(4, None)).is_err());
        assert!(state.begin_resume(2).is_err());
        state.suspend();
        assert!(!state.can_transmit(true, true));
    }
}
