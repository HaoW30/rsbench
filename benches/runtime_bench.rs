//! Runtime module performance benchmarks
//!
//! Validates that Phase 1 backpressure monitoring enhancements
//! don't introduce performance overhead.

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rsbench::runtime::RuntimeStats;

// Mock BackpressureMonitor for isolated testing
struct BackpressureMonitor {
    pool_threshold: f64,
    semaphore_threshold: f64,
}

impl BackpressureMonitor {
    fn new(threshold: f64) -> Self {
        Self {
            pool_threshold: threshold,
            semaphore_threshold: threshold,
        }
    }

    fn is_saturated(&self, stats: &RuntimeStats) -> bool {
        let pool_saturated = stats.pool_utilization > self.pool_threshold;
        let sem_saturated = stats.semaphore_utilization > self.semaphore_threshold;
        pool_saturated || sem_saturated
    }
}

/// Benchmark backpressure detection with various saturation states
fn bench_backpressure_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("backpressure_detection");

    let monitor = BackpressureMonitor::new(0.8);

    // Test case 1: Neither saturated (most common case)
    let stats_neither = RuntimeStats {
        active_connections: 5,
        queued_operations: 5,
        pool_utilization: 0.5,
        semaphore_utilization: 0.5,
        backpressure_active: false,
    };

    group.bench_function("neither_saturated", |b| {
        b.iter(|| {
            black_box(monitor.is_saturated(black_box(&stats_neither)))
        })
    });

    // Test case 2: Pool only saturated
    let stats_pool = RuntimeStats {
        active_connections: 9,
        queued_operations: 5,
        pool_utilization: 0.9,
        semaphore_utilization: 0.5,
        backpressure_active: false,
    };

    group.bench_function("pool_saturated", |b| {
        b.iter(|| {
            black_box(monitor.is_saturated(black_box(&stats_pool)))
        })
    });

    // Test case 3: Semaphore only saturated
    let stats_sem = RuntimeStats {
        active_connections: 5,
        queued_operations: 1,
        pool_utilization: 0.5,
        semaphore_utilization: 0.95,
        backpressure_active: false,
    };

    group.bench_function("semaphore_saturated", |b| {
        b.iter(|| {
            black_box(monitor.is_saturated(black_box(&stats_sem)))
        })
    });

    // Test case 4: Both saturated
    let stats_both = RuntimeStats {
        active_connections: 10,
        queued_operations: 0,
        pool_utilization: 0.95,
        semaphore_utilization: 0.98,
        backpressure_active: false,
    };

    group.bench_function("both_saturated", |b| {
        b.iter(|| {
            black_box(monitor.is_saturated(black_box(&stats_both)))
        })
    });

    group.finish();
}

/// Benchmark stats calculation overhead
fn bench_stats_calculation(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_calculation");

    // Benchmark the calculations that would happen in AsyncRuntime::stats()

    // Pool utilization calculation
    group.bench_function("pool_utilization", |b| {
        b.iter(|| {
            let total = black_box(100);
            let active = black_box(75);
            black_box(active as f64 / total as f64)
        })
    });

    // Semaphore utilization calculation
    group.bench_function("semaphore_utilization", |b| {
        b.iter(|| {
            let max_connections: usize = black_box(100);
            let available_permits: usize = black_box(25);
            let used = max_connections.saturating_sub(available_permits);
            black_box(used as f64 / max_connections as f64)
        })
    });

    // Combined stats construction (what AsyncRuntime::stats does)
    group.bench_function("full_stats_construction", |b| {
        b.iter(|| {
            let total: usize = black_box(100);
            let active: usize = black_box(75);
            let max_connections: usize = black_box(100);
            let available_permits: usize = black_box(25);

            let pool_utilization = active as f64 / total as f64;
            let used = max_connections.saturating_sub(available_permits);
            let semaphore_utilization = used as f64 / max_connections as f64;

            black_box(RuntimeStats {
                active_connections: active,
                queued_operations: available_permits,
                pool_utilization,
                semaphore_utilization,
                backpressure_active: false,
            })
        })
    });

    group.finish();
}

/// Benchmark backpressure detection at different thresholds
fn bench_threshold_sensitivity(c: &mut Criterion) {
    let mut group = c.benchmark_group("threshold_sensitivity");

    let stats = RuntimeStats {
        active_connections: 8,
        queued_operations: 2,
        pool_utilization: 0.8,
        semaphore_utilization: 0.8,
        backpressure_active: false,
    };

    for threshold in [0.5, 0.7, 0.8, 0.9, 0.95].iter() {
        let monitor = BackpressureMonitor::new(*threshold);

        group.bench_with_input(
            BenchmarkId::from_parameter(threshold),
            threshold,
            |b, _| {
                b.iter(|| {
                    black_box(monitor.is_saturated(black_box(&stats)))
                })
            },
        );
    }

    group.finish();
}

/// Benchmark comparison: old (pool-only) vs new (dual-source) detection
fn bench_detection_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("detection_overhead");

    let stats = RuntimeStats {
        active_connections: 7,
        queued_operations: 3,
        pool_utilization: 0.7,
        semaphore_utilization: 0.7,
        backpressure_active: false,
    };

    // Old approach: only pool
    group.bench_function("old_pool_only", |b| {
        b.iter(|| {
            black_box(stats.pool_utilization > 0.8)
        })
    });

    // New approach: pool OR semaphore
    group.bench_function("new_dual_source", |b| {
        b.iter(|| {
            let pool_saturated = stats.pool_utilization > 0.8;
            let sem_saturated = stats.semaphore_utilization > 0.8;
            black_box(pool_saturated || sem_saturated)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_backpressure_detection,
    bench_stats_calculation,
    bench_threshold_sensitivity,
    bench_detection_overhead,
);
criterion_main!(benches);
