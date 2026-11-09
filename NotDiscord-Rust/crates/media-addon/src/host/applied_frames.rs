//! Bounded application ledger: receipt is not an ACK of a queued MLS operation.
//! Stores only opcode/sequence and completion tokens, never native payloads.
use super::voice_socket::{ResumeObservation, ResumeState};
use crate::protocol::{Frame, MediaError, Result};
use serde_json::Value;
use std::collections::VecDeque;
use std::time::{Instant,Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Ticket { pub session: u64, pub generation: u64, token: u64 }
struct AppliedFrame { ticket: Ticket, frame: Frame, outstanding: u16, started: Instant }
pub(super) struct Recovery {
    pub protocol: ResumeState,
    frames: VecDeque<AppliedFrame>,
    next: u64,
    pub interrupted: bool,
    pub quiescing: bool,
    pub reason: Option<MediaError>,
    pub close_code: Option<u16>,
    reported: Option<Ticket>,
}
impl Default for Recovery {
    fn default() -> Self {
        Self { protocol: ResumeState::default(), frames: VecDeque::new(), next: 0,
            interrupted: false, quiescing: false, reason: None, close_code: None, reported: None }
    }
}
impl Recovery {
    pub fn begin(&mut self, session: u64, generation: u64, frame: &Frame) -> Result<Ticket> {
        if !self.protocol.accepts(generation) { return Err(invalid()); }
        self.protocol.validate(frame)?;
        if self.frames.len() >= 128 { return Err(invalid()); }
        self.next = self.next.checked_add(1).ok_or_else(invalid)?;
        let ticket = Ticket { session, generation, token: self.next };
        self.frames.push_back(AppliedFrame { ticket, outstanding: 1, started:Instant::now(),
            frame: Frame { op: frame.op, seq: frame.seq, data: if frame.data.is_null() { Value::Null } else { Value::Bool(true) }, bytes: vec![] } });
        Ok(ticket)
    }
    pub fn wait(&mut self, ticket: Ticket) -> Result<()> {
        let pending = self.frames.iter_mut().find(|pending| pending.ticket == ticket).ok_or_else(invalid)?;
        pending.outstanding = pending.outstanding.checked_add(1).filter(|count| *count <= 128).ok_or_else(invalid)?;
        Ok(())
    }
    pub fn contains(&self, ticket: Ticket) -> bool { self.frames.iter().any(|pending| pending.ticket == ticket) }
    pub fn idle(&self) -> bool { self.frames.is_empty() }
    /// A one-off observation, not a callback deadline or recovery permission.
    pub fn diagnostic(&mut self, now:Instant)->Option<(usize,u64,u64)> {
        let first=self.frames.front()?;
        let age=now.saturating_duration_since(first.started);
        if age<Duration::from_secs(5)||self.reported==Some(first.ticket){return None;}
        self.reported=Some(first.ticket);
        Some((self.frames.len(),age.as_millis().min(600_000) as u64,first.frame.op))
    }
    /// During suspension, completed work remains applied to the same retained
    /// native/DAVE connection. Advance its ACK before admitting any new op7.
    pub fn complete(&mut self, ticket: Ticket) -> Result<bool> {
        let pending = self.frames.iter_mut().find(|pending| pending.ticket == ticket).ok_or_else(invalid)?;
        pending.outstanding = pending.outstanding.checked_sub(1).ok_or_else(invalid)?;
        let mut resumed = false;
        while self.frames.front().is_some_and(|pending| pending.outstanding == 0) {
            let pending = self.frames.pop_front().unwrap();
            // observe_applied retains suspension while advancing only the
            // sequence of a frame already accepted before that suspension.
            resumed |= self.protocol.observe_applied(&pending.frame)? == ResumeObservation::Resumed;
        }
        Ok(resumed)
    }
}
fn invalid() -> MediaError { MediaError::new("VOICE_FRAME_APPLY_LIMIT", "No se pudo conservar el orden de aplicación de la señalización de voz.") }

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(op: u64, seq: u64) -> Frame { Frame { op, seq: Some(seq), data: Value::Null, bytes: vec![] } }
    #[test]
    fn missing_callback_reports_once_without_advancing_ack_or_reopening_recovery(){
        let now=Instant::now();let mut recovery=Recovery::default();let ticket=recovery.begin(1,0,&frame(29,10)).unwrap();
        recovery.wait(ticket).unwrap();recovery.complete(ticket).unwrap();
        recovery.frames.front_mut().unwrap().started=now-Duration::from_secs(6);
        assert_eq!(recovery.diagnostic(now),Some((1,6000,29)));
        assert_eq!(recovery.diagnostic(now+Duration::from_secs(3600)),None);
        assert_eq!(recovery.protocol.seq_ack(),-1);assert!(!recovery.idle());
        recovery.complete(ticket).unwrap();assert_eq!(recovery.protocol.seq_ack(),10);
    }
    #[test]
    fn four_hours_of_virtual_rekeys_complete_without_ledger_growth_or_periodic_resume(){
        let mut recovery=Recovery::default();let start=Instant::now();let mut last=0;
        for seconds in 0..(4*60*60) {
            last=seconds as u64%65536;
            let ticket=recovery.begin(1,0,&frame(if seconds%60==0 {29}else{6},last)).unwrap();
            if seconds%60==0 {recovery.wait(ticket).unwrap();recovery.complete(ticket).unwrap();}
            recovery.complete(ticket).unwrap();
            assert!(recovery.idle());assert_eq!(recovery.protocol.generation(),0);
            assert_eq!(recovery.diagnostic(start+Duration::from_secs(seconds)),None);
        }
        assert_eq!(recovery.protocol.seq_ack(),last as i64);
    }
    #[test]
    fn async_mls_completion_cannot_ack_later_frames_or_admit_resume_early() {
        let mut recovery = Recovery::default();
        let commit = recovery.begin(1, 0, &frame(29, 10)).unwrap(); recovery.wait(commit).unwrap();
        recovery.complete(commit).unwrap();
        let later = recovery.begin(1, 0, &frame(6, 11)).unwrap(); recovery.complete(later).unwrap();
        assert_eq!(recovery.protocol.seq_ack(), -1); assert!(!recovery.idle());
        recovery.protocol.suspend(); recovery.complete(commit).unwrap();
        assert_eq!(recovery.protocol.seq_ack(), 11); assert!(recovery.idle());
        assert!(!recovery.protocol.can_transmit(true, true));
        recovery.protocol.begin_resume(1).unwrap();
        assert!(!recovery.protocol.accepts(0));
        assert!(recovery.protocol.validate(&frame(4, 12)).is_err());
    }
    #[test]
    fn ledger_has_a_fixed_budget_and_never_keeps_binary_or_json_payloads() {
        let mut recovery = Recovery::default();
        let mut private = frame(29, 10); private.bytes = vec![1; 1024]; private.data = serde_json::json!({"token":"private"});
        recovery.begin(1, 0, &private).unwrap();
        let stored = &recovery.frames[0].frame;
        assert!(stored.bytes.is_empty()); assert_eq!(stored.data, Value::Bool(true));
        for seq in 1..128 { recovery.begin(1, 0, &frame(6, seq)).unwrap(); }
        assert!(recovery.begin(1, 0, &frame(6, 129)).is_err());
    }
}
