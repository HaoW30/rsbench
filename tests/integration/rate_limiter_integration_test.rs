//! Rate limiter integration tests
//!
//! End-to-end tests verifying long-running stability and real-world usage patterns.

use rsbench::rate_limiter::RateLimiter;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_long_running_stability() {
    // Run for 10 seconds to verify stability
    let limiter = Arc::new(RateLimiter::new(10_000));
    let duration = Duration::from_secs(10);

    let mut handles = vec![];
    for _ in 0..10 {
        let lim = limiter.clone();
        handles.push(tokio::spawn(async move {
            let start = Instant::now();
            let mut count = 0u64;

            while start.elapsed() < duration {
                lim.acquire().await;
                count += 1;
            }

            count
        }));
    }

    // Collect results
    let mut total = 0u64;
    for handle in handles {
        total += handle.await.unwrap();
    }

    // Should have executed ~100K operations (10K/sec × 10sec)
    // Allow ±10% tolerance
    let expected = 10_000 * 10;
    let lower_bound = (expected as f64 * 0.90) as u64;
    let upper_bound = (expected as f64 * 1.10) as u64;

    assert!(
        total >= lower_bound && total <= upper_bound,
        "Total operations {} not in range [{}, {}]",
        total,
        lower_bound,
        upper_bound
    );
}

#[tokio::test]
async fn test_sustained_high_rate() {
    // Test sustained high rate (100K ops/sec)
    let limiter = Arc::new(RateLimiter::new(100_000));
    let duration = Duration::from_secs(2);

    let start = Instant::now();
    let mut count = 0u64;

    while start.elapsed() < duration {
        limiter.acquire().await;
        count += 1;
    }

    let actual_rate = (count as f64) / start.elapsed().as_secs_f64();

    // Should achieve close to 100K ops/sec
    // Allow ±10% tolerance
    assert!(
        actual_rate >= 90_000.0 && actual_rate <= 110_000.0,
        "Rate {:.2} not in expected range [90000, 110000]",
        actual_rate
    );
}

#[tokio::test]
async fn test_dynamic_rate_ramping() {
    // Simulate ramping rate scenario
    let limiter = Arc::new(RateLimiter::new(1000));

    let rates = vec![1000, 2000, 5000, 10000, 5000, 2000, 1000];
    let mut total_ops = 0u64;

    for rate in rates {
        limiter.set_rate(rate);

        // Run for 500ms at each rate
        let start = Instant::now();
        let mut count = 0;
        while start.elapsed() < Duration::from_millis(500) {
            limiter.acquire().await;
            count += 1;
        }

        let actual_rate = (count as f64) / 0.5;
        let error = ((actual_rate - rate as f64) / rate as f64).abs();

        // Allow 20% error due to timing variance and rate transitions
        assert!(
            error < 0.20,
            "Rate {} error {:.2}% exceeds 20% tolerance",
            rate,
            error * 100.0
        );

        total_ops += count;
    }

    // Should have completed several thousand operations total
    assert!(total_ops > 5000, "Total ops {} too low", total_ops);
}

#[tokio::test]
async fn test_concurrent_rate_changes() {
    // Test changing rate while many tasks are acquiring
    let limiter = Arc::new(RateLimiter::new(5000));
    let barrier = Arc::new(tokio::sync::Barrier::new(21)); // 20 workers + 1 controller

    let mut handles = vec![];

    // Spawn 20 worker tasks
    for _ in 0..20 {
        let lim = limiter.clone();
        let bar = barrier.clone();
        handles.push(tokio::spawn(async move {
            bar.wait().await;

            // Acquire for 2 seconds
            let start = Instant::now();
            let mut count = 0;
            while start.elapsed() < Duration::from_secs(2) {
                lim.acquire().await;
                count += 1;
            }
            count
        }));
    }

    // Controller task: change rate while workers are running
    let lim = limiter.clone();
    let bar = barrier.clone();
    let controller = tokio::spawn(async move {
        bar.wait().await;

        tokio::time::sleep(Duration::from_millis(500)).await;
        lim.set_rate(10_000);

        tokio::time::sleep(Duration::from_millis(500)).await;
        lim.set_rate(2_500);

        tokio::time::sleep(Duration::from_millis(500)).await;
        lim.set_rate(7_500);
    });

    // Wait for all tasks to complete
    let mut total = 0u64;
    for handle in handles {
        total += handle.await.unwrap();
    }
    controller.await.unwrap();

    // Total should be reasonable (avg rate ~6K ops/sec for 2 sec with 20 tasks)
    // Very wide tolerance due to rate changes and concurrency
    assert!(total > 5000, "Total ops {} too low", total);
    assert!(total < 50000, "Total ops {} too high", total);
}

#[tokio::test]
async fn test_mixed_batch_and_single_load() {
    // Real-world scenario: Mix of single and batch acquisitions
    let limiter = Arc::new(RateLimiter::new(20_000));

    let mut handles = vec![];

    // 10 tasks doing single acquire
    for _ in 0..10 {
        let lim = limiter.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..500 {
                lim.acquire().await;
            }
        }));
    }

    // 10 tasks doing batch acquire
    for _ in 0..10 {
        let lim = limiter.clone();
        handles.push(tokio::spawn(async move {
            for _ in 0..50 {
                lim.acquire_many(10).await;
            }
        }));
    }

    // All should complete successfully
    for handle in handles {
        handle.await.unwrap();
    }

    // Total: 10*500 + 10*50*10 = 5000 + 5000 = 10,000 operations
}
