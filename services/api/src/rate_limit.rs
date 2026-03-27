use std::time::{Duration, Instant};

use dashmap::DashMap;

/// Default maximum attempts before rate limiting kicks in.
pub const DEFAULT_MAX_ATTEMPTS: usize = 5;
/// Default sliding window duration for rate limiting.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(15 * 60);

/// In-memory rate limiter keyed by a string identifier (e.g. email address).
///
/// Uses a sliding window: tracks timestamps of recent attempts per key and rejects
/// new attempts once the count reaches `max_attempts` within `window`.
/// Expired entries are pruned lazily on each check — keys that are never
/// re-checked accumulate until the process restarts (acceptable for LAN-only;
/// add a periodic sweep if reused on public-facing endpoints).
pub struct RateLimiter {
    attempts: DashMap<String, Vec<Instant>>,
    max_attempts: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_attempts: usize, window: Duration) -> Self {
        Self {
            attempts: DashMap::new(),
            max_attempts,
            window,
        }
    }

    /// Check whether `key` is under the rate limit. If so, record the attempt
    /// and return `true`. If the limit is exceeded, return `false` without
    /// recording an additional attempt.
    pub fn check_and_record(&self, key: &str) -> bool {
        let now = Instant::now();
        let normalized = key.to_lowercase();
        let mut entry = self.attempts.entry(normalized).or_default();

        entry.retain(|t| now.duration_since(*t) < self.window);

        if entry.len() >= self.max_attempts {
            return false;
        }

        entry.push(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_attempts_under_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));

        assert!(limiter.check_and_record("user@example.com"));
        assert!(limiter.check_and_record("user@example.com"));
        assert!(limiter.check_and_record("user@example.com"));
    }

    #[test]
    fn rejects_attempts_at_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));

        for _ in 0..3 {
            assert!(limiter.check_and_record("user@example.com"));
        }

        assert!(!limiter.check_and_record("user@example.com"));
        assert!(!limiter.check_and_record("user@example.com"));
    }

    #[test]
    fn keys_are_case_insensitive() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));

        assert!(limiter.check_and_record("User@Example.COM"));
        assert!(limiter.check_and_record("user@example.com"));
        assert!(!limiter.check_and_record("USER@EXAMPLE.COM"));
    }

    #[test]
    fn different_keys_are_independent() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));

        assert!(limiter.check_and_record("alice@example.com"));
        assert!(!limiter.check_and_record("alice@example.com"));

        // Different key should still be allowed
        assert!(limiter.check_and_record("bob@example.com"));
    }

    #[test]
    fn window_expiry_resets_count() {
        let limiter = RateLimiter::new(2, Duration::from_millis(50));

        assert!(limiter.check_and_record("user@example.com"));
        assert!(limiter.check_and_record("user@example.com"));
        assert!(!limiter.check_and_record("user@example.com"));

        // Wait for the window to expire
        std::thread::sleep(Duration::from_millis(60));

        // Should be allowed again
        assert!(limiter.check_and_record("user@example.com"));
        assert!(limiter.check_and_record("user@example.com"));
        assert!(!limiter.check_and_record("user@example.com"));
    }

    #[test]
    fn rejected_attempt_does_not_extend_window() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));

        assert!(limiter.check_and_record("user@example.com"));
        assert!(limiter.check_and_record("user@example.com"));

        // These should all fail but not push new timestamps
        for _ in 0..10 {
            assert!(!limiter.check_and_record("user@example.com"));
        }

        // Verify internals: only 2 entries, not 12
        let entry = limiter.attempts.get("user@example.com").unwrap();
        assert_eq!(entry.len(), 2);
    }
}
