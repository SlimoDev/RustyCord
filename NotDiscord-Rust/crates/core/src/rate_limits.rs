//! Bounded, monotonic cooldowns shared by all clones of one authenticated API.
//! This is load control, not a model of human timing or an anti-detection layer.
use reqwest::{Method, header::HeaderMap};
use serde_json::Value;
use std::{collections::HashMap, sync::Mutex, time::Duration};
use tokio::time::Instant;

const MAX_ROUTES: usize = 256;
const MAX_COOLDOWNS: usize = 512;
#[derive(Clone)]
pub(crate) struct Route {
    pattern: String,
    major: String,
}
impl Route {
    pub(crate) fn new(method: &Method, path: &str) -> Self {
        let parts: Vec<_> = path.split('?').next().unwrap_or("").split('/').collect();
        let mut major = String::new();
        for (i, part) in parts.iter().enumerate() {
            if matches!(*part, "channels" | "guilds" | "webhooks") && i + 1 < parts.len() {
                major = format!("{part}:{}", parts[i + 1]);
                if *part == "webhooks" && i + 2 < parts.len() {
                    major.push(':');
                    major.push_str(parts[i + 2]);
                }
                break;
            }
        }
        let normalized = parts
            .iter()
            .enumerate()
            .map(|(i, part)| {
                if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
                    ":id"
                } else if i > 0 && parts[i - 1] == "reactions" {
                    ":emoji"
                } else {
                    *part
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        Self {
            pattern: format!("{} {normalized}", method.as_str()),
            major,
        }
    }
    fn provisional(&self) -> String {
        format!("route:{}|{}", self.pattern, self.major)
    }
}
#[derive(Default)]
struct State {
    global: Option<Instant>,
    indefinite: bool,
    aliases: HashMap<String, String>,
    deadlines: HashMap<String, Instant>,
}
#[derive(Default)]
pub(crate) struct RateLimits {
    state: Mutex<State>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wait {
    Ready,
    Until(Instant),
    Indefinite,
}
impl RateLimits {
    pub(crate) fn wait(&self, route: &Route) -> Wait {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.indefinite {
            return Wait::Indefinite;
        }
        let now = Instant::now();
        state.deadlines.retain(|_, until| *until > now);
        if state.global.is_some_and(|until| until <= now) {
            state.global = None;
        }
        let key = key(&state, route);
        match state
            .global
            .into_iter()
            .chain(state.deadlines.get(&key).copied())
            .chain(state.deadlines.get(&route.provisional()).copied())
            .max()
        {
            Some(until) if until > now => Wait::Until(until),
            _ => Wait::Ready,
        }
    }
    /// Observe success as well as errors: remaining=0 can impose a wait before
    /// the next request, without first making a request that returns 429.
    pub(crate) fn observe(
        &self,
        route: &Route,
        status: u16,
        headers: &HeaderMap,
        body: Option<&Value>,
    ) -> Option<Duration> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        state.deadlines.retain(|_, until| *until > now);
        let provisional = route.provisional();
        // Preserve the maximum already known for this route when the server
        // changes its bucket assignment. Assignments also include the major
        // resource: another channel must not overwrite this channel's bucket.
        let prior = state.deadlines.get(&key(&state, route)).copied();
        if let Some(prior) = prior {
            record(&mut state, provisional.clone(), prior);
        }
        let bucket = header(headers, "x-ratelimit-bucket")
            .filter(|v| !v.is_empty() && v.len() <= 256 && v.bytes().all(|b| b.is_ascii_graphic()));
        let mut overflow = false;
        if let Some(bucket) = bucket {
            if state.aliases.contains_key(&provisional) || state.aliases.len() < MAX_ROUTES {
                state.aliases.insert(provisional.clone(), bucket.to_owned());
            } else {
                overflow = true;
            }
        }
        let key = key(&state, route);
        if let Some(prior) = state.deadlines.get(&provisional).copied() {
            record(&mut state, key.clone(), prior);
        }
        let exhausted =
            header(headers, "x-ratelimit-remaining").and_then(|v| v.parse::<u64>().ok()) == Some(0);
        // Relative reset-after is authoritative. The absolute timestamp is a
        // fallback and uses the server Date when present to avoid local skew.
        // All resulting waits are converted to Instant exactly once.
        let reset_after = header(headers, "x-ratelimit-reset-after").and_then(seconds);
        let reset = reset_after.or_else(|| {
            header(headers, "x-ratelimit-reset").and_then(|value| absolute_delay(value, headers))
        });
        let retry_header = header(headers, "retry-after").and_then(|s| {
            seconds(s).or_else(|| {
                chrono::DateTime::parse_from_rfc2822(s).ok().map(|at| {
                    (at.with_timezone(&chrono::Utc) - reference_time(headers))
                        .to_std()
                        .unwrap_or_default()
                })
            })
        });
        let retry_body = body
            .and_then(|v| v["retry_after"].as_f64())
            .and_then(duration);
        let oversized_number = |value: f64| {
            value.is_finite() && value > 0. && Duration::try_from_secs_f64(value).is_err()
        };
        if (status == 429 || exhausted)
            && (["retry-after", "x-ratelimit-reset-after"]
                .iter()
                .any(|name| {
                    header(headers, name)
                        .and_then(|v| v.parse::<f64>().ok())
                        .is_some_and(oversized_number)
                })
                || reset_after.is_none()
                    && header(headers, "x-ratelimit-reset")
                        .and_then(|v| v.parse::<f64>().ok())
                        .is_some_and(oversized_number)
                || body
                    .and_then(|v| v["retry_after"].as_f64())
                    .is_some_and(oversized_number))
        {
            state.indefinite = true;
            return None;
        }
        let delay = if status == 429 {
            Some(
                retry_header
                    .into_iter()
                    .chain(retry_body)
                    .chain(reset)
                    .max()
                    .unwrap_or(Duration::from_secs(5))
                    .max(Duration::from_millis(100)),
            )
        } else if exhausted {
            reset
        } else {
            None
        };
        if let Some(delay) = delay {
            let Some(until) = now.checked_add(delay) else {
                state.indefinite = true;
                return None;
            };
            let global = overflow
                || header(headers, "x-ratelimit-scope") == Some("global")
                || matches!(header(headers, "x-ratelimit-global"), Some("true" | "1"))
                || body.is_some_and(|v| v["global"].as_bool() == Some(true));
            if global {
                state.global = Some(state.global.map_or(until, |old| old.max(until)));
            } else {
                record(&mut state, provisional, until);
                record(&mut state, key, until);
            }
        }
        delay
    }
}
fn key(state: &State, route: &Route) -> String {
    state
        .aliases
        .get(&route.provisional())
        .map(|bucket| format!("bucket:{bucket}|{}", route.major))
        .unwrap_or_else(|| route.provisional())
}
fn record(state: &mut State, key: String, until: Instant) {
    if state.deadlines.contains_key(&key) || state.deadlines.len() < MAX_COOLDOWNS {
        extend(&mut state.deadlines, key, until);
    } else {
        // Never evict an active wait to make room for a new key.
        state.global = Some(state.global.map_or(until, |old| old.max(until)));
    }
}
fn extend(map: &mut HashMap<String, Instant>, key: String, until: Instant) {
    map.entry(key)
        .and_modify(|old| *old = (*old).max(until))
        .or_insert(until);
}
fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}
fn seconds(s: &str) -> Option<Duration> {
    s.parse::<f64>().ok().and_then(duration)
}
fn reference_time(headers: &HeaderMap) -> chrono::DateTime<chrono::Utc> {
    header(headers, "date")
        .and_then(|value| chrono::DateTime::parse_from_rfc2822(value).ok())
        .map(|date| date.with_timezone(&chrono::Utc))
        .unwrap_or_else(chrono::Utc::now)
}
fn absolute_delay(value: &str, headers: &HeaderMap) -> Option<Duration> {
    let reset = value.parse::<f64>().ok()?;
    if !reset.is_finite() || reset < 0. {
        return None;
    }
    let now = reference_time(headers);
    let epoch = now.timestamp() as f64 + f64::from(now.timestamp_subsec_nanos()) / 1_000_000_000.;
    duration((reset - epoch).max(0.))
}
fn duration(seconds: f64) -> Option<Duration> {
    if !seconds.is_finite() || seconds < 0. {
        return None;
    }
    Duration::try_from_secs_f64(seconds).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn headers(values: &[(&str, &str)]) -> HeaderMap {
        values
            .iter()
            .map(|(k, v)| {
                (
                    reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                    v.parse().unwrap(),
                )
            })
            .collect()
    }
    #[tokio::test(start_paused = true)]
    async fn global_fractional_and_long_wait_survives_the_request() {
        let rates = RateLimits::default();
        let first = Route::new(&Method::GET, "channels/123/messages?limit=50");
        let second = Route::new(&Method::GET, "users/@me");
        rates.observe(
            &first,
            429,
            &headers(&[("retry-after", "300.25"), ("x-ratelimit-scope", "global")]),
            None,
        );
        let Wait::Until(deadline) = rates.wait(&second) else {
            panic!()
        };
        assert_eq!(deadline - Instant::now(), Duration::from_secs_f64(300.25));
        tokio::time::advance(Duration::from_secs(300)).await;
        assert!(matches!(rates.wait(&first), Wait::Until(_)));
        tokio::time::advance(Duration::from_millis(250)).await;
        assert_eq!(rates.wait(&first), Wait::Ready);
    }
    #[tokio::test(start_paused = true)]
    async fn bucket_shares_methods_and_routes_but_keeps_major_resources_separate() {
        let rates = RateLimits::default();
        let get = Route::new(&Method::GET, "channels/123/messages");
        let post = Route::new(&Method::POST, "channels/123/messages");
        let other = Route::new(&Method::POST, "channels/456/messages");
        rates.observe(&get, 200, &headers(&[("x-ratelimit-bucket", "same")]), None);
        rates.observe(
            &post,
            200,
            &headers(&[
                ("x-ratelimit-bucket", "same"),
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset-after", "1.25"),
            ]),
            None,
        );
        assert!(matches!(rates.wait(&get), Wait::Until(_)));
        assert_eq!(rates.wait(&other), Wait::Ready);
        tokio::time::advance(Duration::from_millis(1250)).await;
        assert_eq!(rates.wait(&get), Wait::Ready);
    }
    #[tokio::test(start_paused = true)]
    async fn invalid_delays_are_safe_and_a_new_observation_never_shortens_cooldown() {
        let rates = RateLimits::default();
        let route = Route::new(&Method::POST, "channels/123/messages");
        rates.observe(
            &route,
            429,
            &headers(&[("retry-after", "NaN")]),
            Some(&serde_json::json!({"retry_after":-1})),
        );
        let Wait::Until(first) = rates.wait(&route) else {
            panic!()
        };
        assert_eq!(first - Instant::now(), Duration::from_secs(5));
        rates.observe(&route, 429, &headers(&[("retry-after", "0.25")]), None);
        assert_eq!(rates.wait(&route), Wait::Until(first));
    }
    #[tokio::test(start_paused = true)]
    async fn bucket_rebinding_cannot_erase_another_major_or_an_active_route_wait() {
        let rates = RateLimits::default();
        let first = Route::new(&Method::GET, "channels/123/messages");
        let other = Route::new(&Method::GET, "channels/456/messages");
        rates.observe(
            &first,
            429,
            &headers(&[("x-ratelimit-bucket", "bucket-a"), ("retry-after", "5.25")]),
            None,
        );
        let original = rates.wait(&first);
        rates.observe(
            &other,
            200,
            &headers(&[
                ("x-ratelimit-bucket", "bucket-b"),
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset-after", "1"),
            ]),
            None,
        );
        assert_eq!(rates.wait(&first), original);
        assert_eq!(
            rates.wait(&other),
            Wait::Until(Instant::now() + Duration::from_secs(1))
        );
        // Even a reassignment of the original route retains its maximum wait.
        rates.observe(
            &first,
            200,
            &headers(&[
                ("x-ratelimit-bucket", "bucket-c"),
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset-after", "0.25"),
            ]),
            None,
        );
        assert_eq!(rates.wait(&first), original);
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(rates.wait(&other), Wait::Ready);
        assert_eq!(rates.wait(&first), original);
    }
    #[tokio::test(start_paused = true)]
    async fn absolute_reset_falls_back_to_server_date_and_becomes_monotonic() {
        let date = "Wed, 21 Oct 2015 07:28:00 GMT";
        let epoch = chrono::DateTime::parse_from_rfc2822(date)
            .unwrap()
            .timestamp();
        let reset = format!("{}.5", epoch + 2);
        for reset_after in [None, Some("NaN")] {
            let rates = RateLimits::default();
            let route = Route::new(&Method::GET, "channels/123/messages");
            let mut values = headers(&[
                ("date", date),
                ("x-ratelimit-reset", &reset),
                ("x-ratelimit-remaining", "0"),
            ]);
            if let Some(value) = reset_after {
                values.insert("x-ratelimit-reset-after", value.parse().unwrap());
            }
            rates.observe(&route, 200, &values, None);
            let expected = Instant::now() + Duration::from_millis(2500);
            assert_eq!(rates.wait(&route), Wait::Until(expected));
            tokio::time::advance(Duration::from_secs(2)).await;
            assert_eq!(rates.wait(&route), Wait::Until(expected));
            tokio::time::advance(Duration::from_millis(500)).await;
            assert_eq!(rates.wait(&route), Wait::Ready);
        }
        let rates = RateLimits::default();
        let route = Route::new(&Method::GET, "users/@me");
        rates.observe(
            &route,
            429,
            &headers(&[
                ("date", date),
                ("retry-after", "Wed, 21 Oct 2015 07:28:04 GMT"),
            ]),
            None,
        );
        assert_eq!(
            rates.wait(&route),
            Wait::Until(Instant::now() + Duration::from_secs(4))
        );
    }
    #[tokio::test(start_paused = true)]
    async fn relative_reset_is_authoritative_over_absolute_clock_metadata() {
        let rates = RateLimits::default();
        let route = Route::new(&Method::GET, "users/@me");
        rates.observe(
            &route,
            200,
            &headers(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset-after", "1.25"),
                ("date", "Wed, 21 Oct 2015 07:28:00 GMT"),
                ("x-ratelimit-reset", "1e300"),
            ]),
            None,
        );
        assert_eq!(
            rates.wait(&route),
            Wait::Until(Instant::now() + Duration::from_millis(1250))
        );
    }
    #[tokio::test(start_paused = true)]
    async fn bounded_scheduler_promotes_overflow_without_evicting_an_active_wait() {
        let rates = RateLimits::default();
        let oldest = Route::new(&Method::GET, "channels/1/messages");
        for channel in 1..=600 {
            let route = Route::new(&Method::GET, &format!("channels/{channel}/messages"));
            rates.observe(
                &route,
                429,
                &headers(&[("x-ratelimit-bucket", "shared"), ("retry-after", "300")]),
                None,
            );
        }
        assert_eq!(
            rates.wait(&oldest),
            Wait::Until(Instant::now() + Duration::from_secs(300))
        );
        assert!(matches!(
            rates.wait(&Route::new(&Method::GET, "users/@me")),
            Wait::Until(_)
        ));
        let state = rates.state.lock().unwrap();
        assert!(state.aliases.len() <= MAX_ROUTES);
        assert!(state.deadlines.len() <= MAX_COOLDOWNS);
    }
}
