//! Presentation follows source time. Missed redraws skip expired frames rather
//! than extending every delay, and the refresh ceiling does not slow the media.
use std::time::{Duration, Instant};

pub(super) struct AnimationClock {
    started: Instant,
    presented: Instant,
    index: usize,
}
impl AnimationClock {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            started: now,
            presented: now,
            index: 0,
        }
    }
    pub(super) fn reset(&mut self, now: Instant) {
        *self = Self::new(now);
    }
    pub(super) fn frame(
        &mut self,
        now: Instant,
        delays: impl Iterator<Item = Duration> + Clone,
        fps: u32,
    ) -> (usize, Duration) {
        let (desired, boundary) = source_frame(delays, now.saturating_duration_since(self.started));
        let interval = Duration::from_nanos(1_000_000_000 / u64::from(fps.clamp(1, 30)));
        let until_refresh = interval.saturating_sub(now.saturating_duration_since(self.presented));
        if desired != self.index {
            if !until_refresh.is_zero() {
                return (self.index, until_refresh);
            }
            self.index = desired;
            self.presented = now;
        }
        (
            self.index,
            boundary.max(until_refresh).max(Duration::from_millis(1)),
        )
    }
}
pub(super) fn source_frame(
    delays: impl Iterator<Item = Duration> + Clone,
    elapsed: Duration,
) -> (usize, Duration) {
    let period: Duration = delays.clone().sum();
    if period.is_zero() {
        return (0, Duration::from_millis(100));
    }
    let position = elapsed.as_nanos() % period.as_nanos();
    let mut end = 0u128;
    for (index, delay) in delays.enumerate() {
        end = end.saturating_add(delay.as_nanos());
        if position < end {
            return (
                index,
                Duration::from_nanos(end.saturating_sub(position).min(u128::from(u64::MAX)) as u64),
            );
        }
    }
    (0, Duration::from_millis(100))
}
