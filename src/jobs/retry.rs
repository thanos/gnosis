//! Progressive backoff for automatic job retries.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Controls how many times a failed job is retried and how long to wait.
///
/// Delay after attempt `n` (1-based, already incremented on claim):
/// `min(max_delay, base_delay * 2^(n-1))`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Total attempts including the first run. `1` disables retries.
    pub max_attempts: u32,
    /// Initial backoff after the first failure.
    pub base_delay_ms: u64,
    /// Cap on backoff delay.
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 250,
            max_delay_ms: 30_000,
        }
    }
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, base_delay_ms: u64, max_delay_ms: u64) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            base_delay_ms: base_delay_ms.max(1),
            max_delay_ms: max_delay_ms.max(1),
        }
    }

    /// Whether another attempt should be scheduled after `attempts` completed tries.
    pub fn should_retry(self, attempts: u32) -> bool {
        attempts < self.max_attempts
    }

    /// Progressive (exponential) delay after a failed attempt numbered `attempts`.
    pub fn delay_after(self, attempts: u32) -> Duration {
        let exp = attempts.saturating_sub(1).min(20);
        let scaled = self.base_delay_ms.saturating_mul(1u64 << exp);
        Duration::from_millis(scaled.min(self.max_delay_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progressive_backoff_doubles_until_cap() {
        let p = RetryPolicy::new(5, 100, 1_000);
        assert_eq!(p.delay_after(1).as_millis(), 100);
        assert_eq!(p.delay_after(2).as_millis(), 200);
        assert_eq!(p.delay_after(3).as_millis(), 400);
        assert_eq!(p.delay_after(4).as_millis(), 800);
        assert_eq!(p.delay_after(5).as_millis(), 1000); // capped
        assert!(!p.should_retry(5));
        assert!(p.should_retry(4));
    }

    #[test]
    fn max_attempts_one_disables_retry() {
        let p = RetryPolicy::new(1, 100, 1_000);
        assert!(!p.should_retry(1));
    }
}
