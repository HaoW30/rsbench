//! Connection pool performance benchmarks
//!
//! Measures pool checkout latency, concurrent throughput, and saturation behavior.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rsbench::config::PoolConfig;
use rsbench::driver::{Connection, ConnectionConfig, DatabaseDriver, DriverCapabilities, QueryResult};
use rsbench::pool::ConnectionPool;
use rsbench::{Result, Value};
use std::sync::Arc;
use std::time::Duration;

/// Mock connection for benchmarking (instant operations)
struct BenchConnection;

#[async_trait::async_trait]
impl Connection for BenchConnection {
    async fn execute(&mut self, _sql: &str, _params: &[Value]) -> Result<QueryResult> {
        // Instant execution for benchmarking
        Ok(QueryResult {
            rows_affected: 1,
            last_insert_id: None,
        })
    }

    async fn begin(&mut self) -> Result<()> {
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        Ok(())
    }

    async fn rollback(&mut self) -> Result<()> {
        Ok(())
    }

    async fn ping(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Mock driver for benchmarking (instant connections)
struct BenchDriver;

#[async_trait::async_trait]
impl DatabaseDriver for BenchDriver {
    fn name(&self) -> &str {
        "bench"
    }

    async fn connect(&self, _config: &ConnectionConfig) -> Result<Box<dyn Connection + Send>> {
        // Instant connection for benchmarking
        Ok(Box::new(BenchConnection))
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            supports_transactions: true,
            supports_prepared_statements: true,
        }
    }
}

/// Benchmark single-threaded connection checkout latency
///
/// Target: <10μs (p50), <100μs (p99)
fn benchmark_connection_checkout_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("checkout_latency");

    for pool_size in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(pool_size),
            &pool_size,
            |b, &pool_size| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let driver = Arc::new(BenchDriver);
                let config = PoolConfig {
                    max_size: pool_size,
                    min_size: 0,
                    connection_timeout: Duration::from_secs(30),
                    idle_timeout: Duration::from_secs(600),
                };

                let pool = ConnectionPool::new(driver, "bench://localhost".to_string(), config)
                    .unwrap();

                b.to_async(&rt).iter(|| async {
                    let conn = pool.get().await.unwrap();
                    black_box(conn);
                    // Connection automatically returned on drop
                });
            },
        );
    }

    group.finish();
}

/// Benchmark concurrent pool access throughput
///
/// Target: 100K+ checkouts/sec
fn benchmark_concurrent_pool_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_throughput");

    for num_tasks in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_tasks),
            &num_tasks,
            |b, &num_tasks| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let driver = Arc::new(BenchDriver);
                let config = PoolConfig {
                    max_size: 50,
                    min_size: 0,
                    connection_timeout: Duration::from_secs(30),
                    idle_timeout: Duration::from_secs(600),
                };

                let pool = Arc::new(
                    ConnectionPool::new(driver, "bench://localhost".to_string(), config)
                        .unwrap(),
                );

                b.to_async(&rt).iter(|| async {
                    let handles: Vec<_> = (0..num_tasks)
                        .map(|_| {
                            let pool = pool.clone();
                            tokio::spawn(async move {
                                let conn = pool.get().await.unwrap();
                                black_box(conn);
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

/// Benchmark pool behavior under saturation
///
/// Tests checkout performance when pool is at capacity
fn benchmark_pool_saturation(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_saturation");
    group.sample_size(50); // Reduce sample size for saturation tests

    for pool_size in [5, 10, 20] {
        group.bench_with_input(
            BenchmarkId::from_parameter(pool_size),
            &pool_size,
            |b, &pool_size| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let driver = Arc::new(BenchDriver);
                let config = PoolConfig {
                    max_size: pool_size,
                    min_size: 0,
                    connection_timeout: Duration::from_secs(5),
                    idle_timeout: Duration::from_secs(600),
                };

                let pool = Arc::new(
                    ConnectionPool::new(driver, "bench://localhost".to_string(), config)
                        .unwrap(),
                );

                b.to_async(&rt).iter(|| async {
                    // Saturate the pool by checking out all connections
                    let mut conns = Vec::new();
                    for _ in 0..pool_size {
                        conns.push(pool.get().await.unwrap());
                    }

                    // Measure checkout when pool is saturated
                    // This will block until a connection is available
                    let start = std::time::Instant::now();
                    drop(conns.pop()); // Free one connection
                    let conn = pool.get().await.unwrap();
                    let elapsed = start.elapsed();

                    black_box(conn);
                    black_box(elapsed);

                    // Clean up remaining connections
                    drop(conns);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark pool stats collection overhead
fn benchmark_pool_stats_overhead(c: &mut Criterion) {
    let driver = Arc::new(BenchDriver);
    let config = PoolConfig {
        max_size: 50,
        min_size: 0,
        connection_timeout: Duration::from_secs(30),
        idle_timeout: Duration::from_secs(600),
    };

    let pool = ConnectionPool::new(driver, "bench://localhost".to_string(), config)
        .unwrap();

    c.bench_function("pool_stats", |b| {
        b.iter(|| {
            let stats = pool.stats();
            black_box(stats);
        });
    });
}

/// Benchmark connection lifecycle (checkout + execute + return)
fn benchmark_connection_lifecycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("connection_lifecycle");

    for num_queries in [1, 5, 10] {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_queries),
            &num_queries,
            |b, &num_queries| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let driver = Arc::new(BenchDriver);
                let config = PoolConfig {
                    max_size: 50,
                    min_size: 0,
                    connection_timeout: Duration::from_secs(30),
                    idle_timeout: Duration::from_secs(600),
                };

                let pool = ConnectionPool::new(driver, "bench://localhost".to_string(), config)
                    .unwrap();

                b.to_async(&rt).iter(|| async {
                    let mut conn = pool.get().await.unwrap();
                    for _ in 0..num_queries {
                        let result = conn.execute("SELECT 1", &[]).await.unwrap();
                        black_box(result);
                    }
                    // Connection returned on drop
                });
            },
        );
    }

    group.finish();
}

/// Benchmark pool warm-up performance
fn benchmark_pool_warmup(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_warmup");
    group.sample_size(20); // Reduce sample size for warm-up tests

    for min_size in [5, 10, 20] {
        group.bench_with_input(
            BenchmarkId::from_parameter(min_size),
            &min_size,
            |b, &min_size| {
                let rt = tokio::runtime::Runtime::new().unwrap();

                b.to_async(&rt).iter(|| async {
                    let driver = Arc::new(BenchDriver);
                    let config = PoolConfig {
                        max_size: 50,
                        min_size,
                        connection_timeout: Duration::from_secs(30),
                        idle_timeout: Duration::from_secs(600),
                        };

                    let pool = ConnectionPool::new(driver, "bench://localhost".to_string(), config)
                        .unwrap();

                    // Measure warm-up time
                    let start = std::time::Instant::now();
                    pool.warm_up().await.unwrap();
                    let elapsed = start.elapsed();

                    black_box(elapsed);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_connection_checkout_latency,
    benchmark_concurrent_pool_access,
    benchmark_pool_saturation,
    benchmark_pool_stats_overhead,
    benchmark_connection_lifecycle,
    benchmark_pool_warmup
);
criterion_main!(benches);
