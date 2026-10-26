//! Choose by source time, not by the number of paints. The caller limits redraws
//! to 30 FPS while missed frames are skipped without slowing the animation.
use std::time::Duration;
pub(super) fn source_delay(numerator_ms: u32, denominator: u32) -> Duration {
    if numerator_ms == 0 {
        return Duration::from_millis(20);
    }
    Duration::from_nanos(
        (u64::from(numerator_ms) * 1_000_000 / u64::from(denominator.max(1))).max(1),
    )
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
                Duration::from_nanos((end - position).min(u128::from(u64::MAX)) as u64),
            );
        }
    }
    (0, Duration::from_millis(100))
}
