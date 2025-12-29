//! Rate accuracy property tests
//!
//! Verify that rate limiter maintains target rate within acceptable tolerance.

use proptest::prelude::*;
use rsbench::rate_limiter::RateLimiter;
use std::time::Instant;
use tokio::time::Duration;

proptest! {
    #[test]
    fn rate_never_exceeded(rate in 100..50_000u64) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let limiter = RateLimiter::new(rate);
            let duration = Duration::from_secs(1);

            let mut count = 0u64;
            let start = Instant::now();

            while start.elapsed() < duration {
                limiter.acquire().await;
                count += 1;
            }

            let actual_rate = (count as f64) / start.elapsed().as_secs_f64();

            // Should never exceed target rate (allow 10% tolerance for measurement variance)
            prop_assert!(
                actual_rate <= (rate as f64 * 1.10),
                "Rate {} exceeded target {} by more than 10%",
                actual_rate,
                rate
            );
        });
    }

    #[test]
    fn rate_accuracy_within_tolerance(rate in 1000..20_000u64) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let limiter = RateLimiter::new(rate);
            let duration = Duration::from_secs(2);  // Longer for better accuracy

            let mut count = 0u64;
            let start = Instant::now();

            while start.elapsed() < duration {
                limiter.acquire().await;
                count += 1;
            }

            let actual_rate = (count as f64) / start.elapsed().as_secs_f64();
            let error = ((actual_rate - rate as f64) / rate as f64).abs();

            // Should be within ±5% of target (allowing for system variance)
            prop_assert!(
                error < 0.05,
                "Rate error {:.2}% exceeds 5% tolerance (target={}, actual={:.2})",
                error * 100.0,
                rate,
                actual_rate
            );
        });
    }

    #[test]
    fn rate_change_takes_effect(
        initial in 500..5_000u64,
        new in 500..5_000u64
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let limiter = RateLimiter::new(initial);

            // Run at initial rate for 500ms
            let start = Instant::now();
            let mut count1 = 0;
            while start.elapsed() < Duration::from_millis(500) {
                limiter.acquire().await;
                count1 += 1;
            }

            // Change rate
            limiter.set_rate(new);

            // Run at new rate for 500ms
            let start = Instant::now();
            let mut count2 = 0;
            while start.elapsed() < Duration::from_millis(500) {
                limiter.acquire().await;
                count2 += 1;
            }

            // Verify both rates were respected (allow 20% tolerance)
            let rate1 = (count1 as f64) / 0.5;
            let rate2 = (count2 as f64) / 0.5;

            prop_assert!(
                (rate1 - initial as f64).abs() / initial as f64 < 0.20,
                "Initial rate error too high: expected {}, got {:.2}",
                initial,
                rate1
            );
            prop_assert!(
                (rate2 - new as f64).abs() / new as f64 < 0.20,
                "New rate error too high: expected {}, got {:.2}",
                new,
                rate2
            );

            Ok(())
        });
    }

    #[test]
    fn burst_respects_capacity(
        rate in 100..5_000u64,
        capacity_multiplier in 1..5u64
    ) {
        let capacity = rate * capacity_multiplier;
        let limiter = RateLimiter::with_capacity(rate, capacity);

        // Idle for long time to accumulate tokens
        std::thread::sleep(Duration::from_secs(10));

        // Available permits should not exceed capacity
        let available = limiter.available_permits();
        prop_assert!(
            available <= capacity,
            "Available {} exceeds capacity {}",
            available,
            capacity
        );
    }

    #[test]
    fn batch_acquire_equivalent_to_single(
        rate in 1000..10_000u64,
        batch_size in 10..100u64
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let limiter = RateLimiter::new(rate);

            // Exhaust burst capacity
            for _ in 0..(rate * 2) {
                limiter.acquire().await;
            }

            // Time single acquisitions
            let start = Instant::now();
            for _ in 0..batch_size {
                limiter.acquire().await;
            }
            let single_time = start.elapsed();

            // Wait for refill
            tokio::time::sleep(Duration::from_millis(200)).await;

            // Time batch acquisition
            let start = Instant::now();
            limiter.acquire_many(batch_size).await;
            let batch_time = start.elapsed();

            // Batch should be faster or similar (within 50% tolerance)
            // We can't be too strict here due to timing variance
            prop_assert!(
                batch_time.as_millis() <= single_time.as_millis() + 100,
                "Batch acquisition took longer: batch={}ms, single={}ms",
                batch_time.as_millis(),
                single_time.as_millis()
            );
        });
    }
}
