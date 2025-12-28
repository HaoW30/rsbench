//! Rate limiter performance benchmarks
//!
//! Measures token bucket performance characteristics.

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rsbench::rate_limiter::RateLimiter;
use std::sync::Arc;

fn benchmark_single_threaded_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_threaded");

    for rate in [1_000, 10_000, 100_000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(rate),
            &rate,
            |b, &rate| {
                let limiter = RateLimiter::new(rate);

                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        black_box(limiter.acquire().await);
                    });
            },
        );
    }

    group.finish();
}

fn benchmark_concurrent_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent");

    for num_tasks in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_tasks),
            &num_tasks,
            |b, &num_tasks| {
                let limiter = Arc::new(RateLimiter::new(100_000));

                b.to_async(tokio::runtime::Runtime::new().unwrap())
                    .iter(|| async {
                        let handles: Vec<_> = (0..num_tasks)
                            .map(|_| {
                                let lim = limiter.clone();
                                tokio::spawn(async move {
                                    black_box(lim.acquire().await);
                                })
                            })
                            .collect();

                        futures::future::join_all(handles).await;
                    });
            },
        );
    }

    group.finish();
}

fn benchmark_rate_accuracy(c: &mut Criterion) {
    c.bench_function("rate_accuracy_10k", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap())
            .iter(|| async {
            let limiter = RateLimiter::new(10_000);
            let start = std::time::Instant::now();

            for _ in 0..1000 {
                limiter.acquire().await;
            }

            let elapsed = start.elapsed();
            black_box(elapsed);
        });
    });
}

fn benchmark_batch_vs_single(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("batch_vs_single");

    for batch_size in [10, 50, 100] {
        // Benchmark single acquisitions
        group.bench_with_input(
            BenchmarkId::new("single", batch_size),
            &batch_size,
            |b, &batch_size| {
                let limiter = RateLimiter::new(100_000);

                b.to_async(&rt).iter(|| async {
                    for _ in 0..batch_size {
                        black_box(limiter.acquire().await);
                    }
                });
            },
        );

        // Benchmark batch acquisition
        group.bench_with_input(
            BenchmarkId::new("batch", batch_size),
            &batch_size,
            |b, &batch_size| {
                let limiter = RateLimiter::new(100_000);

                b.to_async(&rt).iter(|| async {
                    black_box(limiter.acquire_many(batch_size).await);
                });
            },
        );
    }

    group.finish();
}

fn benchmark_rate_change_overhead(c: &mut Criterion) {
    let limiter = RateLimiter::new(1000);

    c.bench_function("rate_change", |b| {
        let mut rate = 1000;
        b.iter(|| {
            rate = if rate == 1000 { 2000 } else { 1000 };
            black_box(limiter.set_rate(rate));
        });
    });
}

fn benchmark_available_permits(c: &mut Criterion) {
    let limiter = RateLimiter::new(10_000);

    c.bench_function("available_permits", |b| {
        b.iter(|| {
            black_box(limiter.available_permits());
        });
    });
}

criterion_group!(
    benches,
    benchmark_single_threaded_throughput,
    benchmark_concurrent_throughput,
    benchmark_rate_accuracy,
    benchmark_batch_vs_single,
    benchmark_rate_change_overhead,
    benchmark_available_permits
);
criterion_main!(benches);
