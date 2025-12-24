//! Rate limiter module
//!
//! Token bucket-based rate limiting for time-driven execution.

use crate::Result;
use std::time::{Duration, Instant};

/// Token bucket rate limiter
pub struct RateLimiter {
    rate: u64,
    capacity: f64,
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    /// Create new rate limiter
    pub fn new(rate: u64) -> Self {
        let capacity = (rate as f64 * 2.0).max(1.0); // Allow small bursts
        Self {
            rate,
            capacity,
            tokens: capacity,
            last_refill: Instant::now(),
        }
    }

    /// Acquire permit to submit one operation
    pub async fn acquire(&mut self) -> Result<Permit> {
        loop {
            self.refill();

            if self.tokens >= 1.0 {
                self.tokens -= 1.0;
                return Ok(Permit);
            }

            // Wait for next refill period
            let wait_time = Duration::from_micros(1_000_000 / self.rate);
            tokio::time::sleep(wait_time).await;
        }
    }

    /// Change rate dynamically (for ramping)
    pub fn set_rate(&mut self, new_rate: u64) {
        self.rate = new_rate;
        self.capacity = (new_rate as f64 * 2.0).max(1.0);
        // Adjust tokens proportionally
        self.tokens = self.tokens.min(self.capacity);
    }

    /// Get current rate
    pub fn current_rate(&self) -> u64 {
        self.rate
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let new_tokens = elapsed.as_secs_f64() * self.rate as f64;

        if new_tokens > 0.0 {
            self.tokens = (self.tokens + new_tokens).min(self.capacity);
            self.last_refill = now;
        }
    }
}

/// Permit to submit one operation
pub struct Permit;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let mut limiter = RateLimiter::new(100);

        // Should be able to acquire immediately
        let _permit = limiter.acquire().await.unwrap();

        assert!(limiter.tokens < limiter.capacity);
    }

    #[tokio::test]
    async fn test_rate_change() {
        let mut limiter = RateLimiter::new(100);
        assert_eq!(limiter.current_rate(), 100);

        limiter.set_rate(200);
        assert_eq!(limiter.current_rate(), 200);
    }
}
