//! Failure ownership outlives texture eviction. The fixed fingerprint table
//! never forgets an error during this account's lifetime. Collisions defer a
//! fresh request locally; they never claim a remote failure. Explicit Retry
//! grants one named demand, not an eviction or timer-based reset.
use std::hash::{Hash, Hasher};
use std::{sync::Mutex, time::{Duration, Instant}};
// Anonymous CDN requests share a conservative wait. Switching accounts or
// expelling cached content cannot shorten a server's Retry-After.
#[derive(Default)]
struct Wait { until: Option<Instant>, indefinite: bool }
impl Wait {
    fn ready(&self, now: Instant) -> bool { !self.indefinite && self.until.is_none_or(|until|now>=until) }
    fn extend(&mut self, now: Instant, duration: Duration) {
        match now.checked_add(duration) {
            Some(until)=>self.until=Some(self.until.map_or(until,|old|old.max(until))),
            None=>self.indefinite=true,
        }
    }
}
static WAIT: Mutex<Wait> = Mutex::new(Wait { until: None, indefinite: false });
pub(super) fn cdn_ready() -> bool {
    let wait=WAIT.lock().unwrap_or_else(|e|e.into_inner());
    wait.ready(Instant::now())
}
pub(super) fn rate_limited(headers: &reqwest::header::HeaderMap) {
    let raw=headers.get(reqwest::header::RETRY_AFTER).and_then(|value|value.to_str().ok());
    let duration=raw.and_then(|value|value.trim().parse::<u64>().ok().map(Duration::from_secs))
        .or_else(|| raw.and_then(|value|value.trim().parse::<f64>().ok()).filter(|n|n.is_finite()&&*n>=0.)
            .and_then(|n|Duration::try_from_secs_f64(n).ok()))
        .or_else(||raw.and_then(|value|chrono::DateTime::parse_from_rfc2822(value).ok())
            .map(|date|Duration::from_millis((date.timestamp_millis()-chrono::Utc::now().timestamp_millis()).max(0) as u64)))
        .unwrap_or(Duration::from_secs(60)); // local policy when no valid wait is supplied
    let mut wait=WAIT.lock().unwrap_or_else(|e|e.into_inner());
    wait.extend(Instant::now(),duration);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State { Pending, Ready, Failed, CapacityDeferred, Fresh }
pub(super) struct Failures { bits: [u64; 128], retry: Option<String>, recovered: std::collections::HashSet<String> }
impl Default for Failures { fn default() -> Self { Self { bits: [0;128], retry: None, recovered: Default::default() } } }
impl Failures {
    fn positions(key: &str) -> [usize; 2] {
        [0u8,1].map(|salt| { let mut hash=std::collections::hash_map::DefaultHasher::new(); salt.hash(&mut hash); key.hash(&mut hash); hash.finish() as usize % 8192 })
    }
    pub(super) fn failed(&mut self, key: &str) {
        self.recovered.remove(key);
        for at in Self::positions(key) { self.bits[at/64] |= 1 << (at%64); }
    }
    pub(super) fn succeeded(&mut self, key: &str) {
        // Do not store every successful URL. Only fingerprint hits need a
        // bounded exception after a successful explicit retry.
        if self.recovered.len()<256 && Self::positions(key).into_iter().all(|at|self.bits[at/64]&(1<<(at%64))!=0) { self.recovered.insert(key.into()); }
    }
    pub(super) fn state(&self, key: &str, ready: bool, pending: bool, failed: bool) -> State {
        if ready { State::Ready } else if pending { State::Pending } else if failed { State::Failed }
        else if self.retry.as_deref()!=Some(key) && !self.recovered.contains(key) && Self::positions(key).into_iter().all(|at| self.bits[at/64] & (1 << (at%64)) != 0) { State::CapacityDeferred }
        else { State::Fresh }
    }
    pub(super) fn retry(&mut self, key: String) { self.retry=Some(key); }
    pub(super) fn admitted(&mut self, key: &str) { if self.retry.as_deref()==Some(key) { self.retry=None; } }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_shorter_later_wait_or_retry_cannot_advance_the_larger_server_deadline() {
        let now=Instant::now();let mut wait=Wait::default();wait.extend(now,Duration::from_secs(3600));
        wait.extend(now+Duration::from_secs(10),Duration::from_secs(1));
        assert!(!wait.ready(now+Duration::from_secs(3599)));
        assert!(wait.ready(now+Duration::from_secs(3600)));
        wait.extend(now,Duration::MAX);assert!(!wait.ready(now+Duration::from_secs(100_000)));
    }
    #[test]
    fn a_full_reason_cache_still_admits_unrelated_fresh_demand_without_forgetting_old_failures() {
        let mut gate=Failures::default();
        for n in 0..256 { gate.failed(&format!("failed-{n}")); }
        assert_eq!(gate.state("failed-0",false,false,false),State::CapacityDeferred);
        // Fingerprint collisions are reported as local deferral. The fixed
        // table does not turn reason-cache fullness into a global stop.
        let fresh=(0..64).filter(|n|gate.state(&format!("new-source-{n}"),false,false,false)==State::Fresh).count();
        assert!(fresh>0);
    }
    #[test]
    fn eviction_and_unlimited_repaints_cannot_rearm_a_failure_but_named_retry_is_single_use() {
        let mut gate=Failures::default();
        for i in 0..10_000 { gate.failed(&format!("failed-{i}")); }
        for _ in 0..1000 { assert_eq!(gate.state("failed-0",false,false,false),State::CapacityDeferred); }
        gate.retry("failed-0".into());
        assert_eq!(gate.state("failed-0",false,true,false),State::Pending);
        assert_eq!(gate.state("failed-0",false,false,false),State::Fresh);
        gate.admitted("failed-0");
        assert_eq!(gate.state("failed-0",false,false,false),State::CapacityDeferred);
        gate.succeeded("failed-0");
        assert_eq!(gate.state("failed-0",false,false,false),State::Fresh);
        gate.failed("failed-0");
        assert_eq!(gate.state("failed-0",false,false,false),State::CapacityDeferred);
        assert_eq!(gate.state("failed-0",true,false,false),State::Ready);
        assert_eq!(gate.state("failed-0",false,false,true),State::Failed);
        assert_eq!(std::mem::size_of_val(&gate.bits),1024);
    }
}
