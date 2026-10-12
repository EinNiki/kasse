//! Brute-force protection for the login: too many failed attempts for a user name lock further
//! attempts for a while. Kept in memory (a restart clears it), keyed by lower-cased user name.
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_FAILURES: u32 = 5;
const WINDOW: Duration = Duration::from_secs(5 * 60);
const LOCK: Duration = Duration::from_secs(5 * 60);
/// Upper bound for tracked names so random user names cannot grow memory without limit.
const MAX_ENTRIES: usize = 10_000;

struct Entry {
    failures: u32,
    window_start: Instant,
    locked_until: Option<Instant>,
}

#[derive(Default)]
pub struct LoginLimiter {
    entries: Mutex<HashMap<String, Entry>>,
}

impl LoginLimiter {
    fn key(name: &str) -> String {
        name.trim().to_lowercase()
    }

    /// `Err(remaining)` while the user name is locked.
    pub fn check(&self, name: &str) -> Result<(), Duration> {
        self.check_at(name, Instant::now())
    }

    pub fn record_failure(&self, name: &str) {
        self.record_failure_at(name, Instant::now());
    }

    pub fn record_success(&self, name: &str) {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&Self::key(name));
    }

    pub fn check_at(&self, name: &str, now: Instant) -> Result<(), Duration> {
        let map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&Self::key(name)).and_then(|e| e.locked_until) {
            Some(until) if until > now => Err(until - now),
            _ => Ok(()),
        }
    }

    pub fn record_failure_at(&self, name: &str, now: Instant) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if map.len() >= MAX_ENTRIES {
            map.retain(|_, e| {
                e.locked_until.is_some_and(|u| u > now)
                    || now.duration_since(e.window_start) < WINDOW
            });
            if map.len() >= MAX_ENTRIES {
                return; // still full of active entries: do not grow further
            }
        }
        let e = map.entry(Self::key(name)).or_insert(Entry {
            failures: 0,
            window_start: now,
            locked_until: None,
        });
        if now.duration_since(e.window_start) >= WINDOW {
            e.failures = 0;
            e.window_start = now;
        }
        e.failures += 1;
        if e.failures >= MAX_FAILURES {
            e.locked_until = Some(now + LOCK);
            e.failures = 0;
            e.window_start = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_five_failures_and_unlocks_later() {
        let l = LoginLimiter::default();
        let t0 = Instant::now();
        for _ in 0..4 {
            l.record_failure_at("Anna", t0);
        }
        assert!(
            l.check_at("anna", t0).is_ok(),
            "four failures are still fine"
        );
        l.record_failure_at("anna ", t0);
        let wait = l.check_at("ANNA", t0).unwrap_err();
        assert!(wait <= LOCK && wait > Duration::from_secs(290));
        assert!(l
            .check_at("anna", t0 + LOCK + Duration::from_secs(1))
            .is_ok());
        assert!(
            l.check_at("other", t0).is_ok(),
            "other users are not affected"
        );
    }

    #[test]
    fn old_failures_expire_and_success_resets() {
        let l = LoginLimiter::default();
        let t0 = Instant::now();
        for _ in 0..4 {
            l.record_failure_at("a", t0);
        }
        l.record_failure_at("a", t0 + WINDOW + Duration::from_secs(1));
        assert!(
            l.check_at("a", t0 + WINDOW + Duration::from_secs(1))
                .is_ok(),
            "window restarted"
        );
        for _ in 0..3 {
            l.record_failure_at("b", t0);
        }
        l.record_success("b");
        for _ in 0..4 {
            l.record_failure_at("b", t0);
        }
        assert!(l.check_at("b", t0).is_ok());
    }
}
