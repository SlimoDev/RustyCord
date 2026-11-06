//! One bounded, serial recovery episode for Gateway -> voice -> current viewer.
//! This is admission state only: it never starts tasks, retries I/O or stores
//! account/channel identifiers. Protocol handshakes remain with their owners.
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryPart {
    Gateway,
    Voice,
    Viewer,
}
impl RecoveryPart {
    fn bit(self) -> u8 {
        match self {
            Self::Gateway => 1,
            Self::Voice => 2,
            Self::Viewer => 4,
        }
    }
}
struct Episode {
    id: u64,
    used: u8,
    active: Option<RecoveryPart>,
    failed: bool,
}
struct State {
    next: u64,
    episode: Option<Episode>,
    available: bool,
    established_once: bool,
    gateway_ready: bool,
    acknowledged: bool,
    stable_since: Option<Instant>,
    blocked_until: Option<Instant>,
    stable: Duration,
    cooldown: Duration,
}
impl Default for State {
    fn default() -> Self {
        Self {
            next: 0,
            episode: None,
            available: false,
            established_once: false,
            gateway_ready: false,
            acknowledged: false,
            stable_since: None,
            blocked_until: None,
            stable: Duration::from_secs(60),
            cooldown: Duration::from_secs(30),
        }
    }
}
#[derive(Default)]
pub(super) struct RecoveryBudget {
    state: Mutex<State>,
}
impl RecoveryBudget {
    pub(super) fn configure(&self, stable: Duration, cooldown: Duration) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.stable = stable;
        state.cooldown = cooldown;
    }
    pub(super) fn gateway_ready(&self, now: Instant) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.established_once {
            state.established_once = true;
            // The first genuinely established session earns one recovery. An
            // explicit attempt after a failed initial open does not earn two.
            state.available = state.episode.is_none();
        }
        state.gateway_ready = true;
        state.stable_since = Some(now);
    }
    pub(super) fn acknowledge(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .acknowledged = true;
    }
    pub(super) fn gateway_lost(&self, now: Instant) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        Self::refresh(&mut state, now);
        state.gateway_ready = false;
        state.acknowledged = false;
        state.stable_since = None;
        // A second Gateway loss during any compound episode is a failure, not
        // permission to append another Gateway/voice/viewer burst.
        if state.episode.is_some() {
            Self::fail(&mut state, now);
        }
    }
    fn refresh(state: &mut State, now: Instant) {
        if state.gateway_ready
            && state.acknowledged
            && state
                .episode
                .as_ref()
                .is_none_or(|episode| episode.active.is_none())
            && state
                .stable_since
                .is_some_and(|since| now.saturating_duration_since(since) >= state.stable)
            && state.blocked_until.is_none_or(|until| now >= until)
        {
            state.episode = None;
            state.available = true;
            state.blocked_until = None;
        }
    }
    fn fail(state: &mut State, now: Instant) {
        if let Some(episode) = &mut state.episode {
            episode.failed = true;
            // Invalidate its authority immediately but retain the lease until
            // its owner observes real cancellation/completion. A cooldown is
            // not proof that an old media task stopped emitting.
        }
        state.available = false;
        state.blocked_until = Some(state.blocked_until.unwrap_or(now).max(now + state.cooldown));
        // A stable Gateway from *before* a media failure cannot rearm it.
        state.stable_since = state.gateway_ready.then_some(now);
    }
    pub(super) fn try_part(
        self: &Arc<Self>,
        part: RecoveryPart,
        now: Instant,
    ) -> Option<RecoveryPermit> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        Self::refresh(&mut state, now);
        if part == RecoveryPart::Gateway && state.gateway_ready
            || part != RecoveryPart::Gateway && !state.gateway_ready
            || state.blocked_until.is_some_and(|until| now < until)
        {
            return None;
        }
        if let Some(episode) = &state.episode {
            if episode.failed || episode.active.is_some() || episode.used >= part.bit() {
                return None;
            }
        } else {
            if !state.available {
                return None;
            }
            state.available = false;
            state.next = state.next.checked_add(1)?;
            state.episode = Some(Episode {
                id: state.next,
                used: 0,
                active: None,
                failed: false,
            });
        }
        state.stable_since = None;
        let episode = state.episode.as_mut()?;
        episode.used |= part.bit();
        episode.active = Some(part);
        Some(RecoveryPermit {
            owner: self.clone(),
            episode: episode.id,
            part,
            completed: false,
        })
    }
    /// Called only after the parked Gateway owner offered an explicit attempt;
    /// old clicks never reach this method and cannot reset its cooldown.
    pub(super) fn manual_gateway(self: &Arc<Self>, now: Instant) -> Option<RecoveryPermit> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.gateway_ready
            || state.blocked_until.is_some_and(|until| now < until)
            || state
                .episode
                .as_ref()
                .is_some_and(|episode| episode.active.is_some())
        {
            return None;
        }
        state.next = state.next.checked_add(1)?;
        state.available = false;
        state.stable_since = None;
        let id = state.next;
        state.episode = Some(Episode {
            id,
            used: RecoveryPart::Gateway.bit(),
            active: Some(RecoveryPart::Gateway),
            failed: false,
        });
        Some(RecoveryPermit {
            owner: self.clone(),
            episode: id,
            part: RecoveryPart::Gateway,
            completed: false,
        })
    }
    pub(super) fn wait(&self, now: Instant) -> Duration {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .blocked_until
            .map(|until| until.saturating_duration_since(now))
            .unwrap_or_default()
    }
    fn finish(&self, id: u64, part: RecoveryPart, success: bool, now: Instant) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let Some(episode) = &mut state.episode else {
            return false;
        };
        if episode.id != id || episode.active != Some(part) {
            return false;
        }
        let current = !episode.failed;
        episode.active = None;
        if !current {
            Self::fail(&mut state, now);
            return false;
        }
        if success {
            state.stable_since = state.gateway_ready.then_some(now);
        } else {
            Self::fail(&mut state, now);
        }
        true
    }
}

/// Retain until the actual protocol result (op9 + safe native/DAVE state for
/// voice, first accepted frame for viewer), never merely an IPC queue ACK.
#[must_use]
pub struct RecoveryPermit {
    owner: Arc<RecoveryBudget>,
    episode: u64,
    part: RecoveryPart,
    completed: bool,
}
impl RecoveryPermit {
    pub fn episode(&self) -> u64 {
        self.episode
    }
    pub fn part(&self) -> RecoveryPart {
        self.part
    }
    pub fn is_current(&self) -> bool {
        let state = self
            .owner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.episode.as_ref().is_some_and(|episode| {
            episode.id == self.episode && episode.active == Some(self.part) && !episode.failed
        })
    }
    pub fn finish(mut self, success: bool) -> bool {
        let applied = self
            .owner
            .finish(self.episode, self.part, success, Instant::now());
        self.completed = true;
        applied
    }
}
impl Drop for RecoveryPermit {
    fn drop(&mut self) {
        if !self.completed {
            self.owner
                .finish(self.episode, self.part, false, Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn established(now: Instant) -> Arc<RecoveryBudget> {
        let budget = Arc::new(RecoveryBudget::default());
        budget.gateway_ready(now);
        budget
    }
    #[test]
    fn one_compound_episode_serializes_gateway_voice_and_viewer_without_new_budgets() {
        let now = Instant::now();
        let budget = established(now);
        budget.gateway_lost(now);
        let gateway = budget.try_part(RecoveryPart::Gateway, now).unwrap();
        let id = gateway.episode();
        assert!(budget.try_part(RecoveryPart::Voice, now).is_none());
        budget.gateway_ready(now);
        assert!(budget.try_part(RecoveryPart::Viewer, now).is_none());
        assert!(gateway.finish(true));
        let voice = budget.try_part(RecoveryPart::Voice, now).unwrap();
        assert_eq!(voice.episode(), id);
        assert!(budget.try_part(RecoveryPart::Voice, now).is_none());
        assert!(budget.try_part(RecoveryPart::Viewer, now).is_none());
        voice.finish(true);
        let viewer = budget.try_part(RecoveryPart::Viewer, now).unwrap();
        assert_eq!(viewer.episode(), id);
        viewer.finish(true);
        for part in [
            RecoveryPart::Gateway,
            RecoveryPart::Voice,
            RecoveryPart::Viewer,
        ] {
            assert!(budget.try_part(part, now).is_none());
        }
    }
    #[test]
    fn any_substep_failure_or_dropped_permit_stops_the_entire_episode() {
        let now = Instant::now();
        let budget = established(now);
        let voice = budget.try_part(RecoveryPart::Voice, now).unwrap();
        drop(voice);
        for part in [
            RecoveryPart::Gateway,
            RecoveryPart::Voice,
            RecoveryPart::Viewer,
        ] {
            assert!(
                budget
                    .try_part(part, now + Duration::from_secs(31))
                    .is_none()
            );
        }
        assert!(!budget.wait(now).is_zero());
        // Passing cooldown by itself never earns another automatic attempt.
    }
    #[test]
    fn gateway_loss_invalidates_media_lease_and_a_late_success_cannot_reopen_it() {
        let now = Instant::now();
        let budget = established(now);
        let voice = budget.try_part(RecoveryPart::Voice, now).unwrap();
        budget.gateway_lost(now);
        assert!(!voice.is_current());
        assert!(
            budget
                .manual_gateway(now + Duration::from_secs(31))
                .is_none(),
            "cooldown cannot release a still-live media lease"
        );
        assert!(!voice.finish(true));
        budget.gateway_ready(now);
        assert!(budget.try_part(RecoveryPart::Viewer, now).is_none());
        assert!(budget.try_part(RecoveryPart::Gateway, now).is_none());
    }
    #[test]
    fn first_failed_open_has_no_auto_attempt_and_stability_rearms_only_after_real_completion() {
        let now = Instant::now();
        let budget = Arc::new(RecoveryBudget::default());
        assert!(budget.try_part(RecoveryPart::Gateway, now).is_none());
        budget.gateway_ready(now);
        let voice = budget.try_part(RecoveryPart::Voice, now).unwrap();
        budget.acknowledge();
        assert!(
            budget
                .try_part(RecoveryPart::Viewer, now + Duration::from_secs(120))
                .is_none()
        );
        voice.finish(false);
        budget.acknowledge();
        assert!(
            budget
                .try_part(RecoveryPart::Voice, now + Duration::from_secs(59))
                .is_none()
        );
        assert!(
            budget
                .try_part(RecoveryPart::Voice, now + Duration::from_secs(61))
                .is_some()
        );
    }
    #[test]
    fn explicit_gateway_attempt_keeps_cooldown_and_does_not_refill_automatic_allowance() {
        let now = Instant::now();
        let budget = Arc::new(RecoveryBudget::default());
        let first = budget.manual_gateway(now).unwrap();
        first.finish(false);
        assert!(budget.manual_gateway(now).is_none());
        let later = now + Duration::from_secs(31);
        let next = budget.manual_gateway(later).unwrap();
        budget.gateway_ready(later);
        next.finish(true);
        budget.gateway_lost(later);
        assert!(budget.try_part(RecoveryPart::Gateway, later).is_none());
    }
}
