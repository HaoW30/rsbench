//! Async runtime implementation (primary mode)

use super::{OperationResult, RuntimeEngine, RuntimeStats};
use crate::metrics::MetricsCollector;
use crate::pool::ConnectionPool;
use crate::workload::Operation;
use crate::{Result, RuntimeError};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

/// Async runtime (primary mode)
pub struct AsyncRuntime {
    pool: Arc<ConnectionPool>,
    semaphore: Arc<Semaphore>,
    backpressure_monitor: BackpressureMonitor,
    metrics: Arc<MetricsCollector>,
}

impl AsyncRuntime {
    pub fn new(
        pool: Arc<ConnectionPool>,
        max_connections: usize,
        backpressure_threshold: f64,
        metrics: Arc<MetricsCollector>,
    ) -> Self {
        Self {
            pool,
            semaphore: Arc::new(Semaphore::new(max_connections)),
            backpressure_monitor: BackpressureMonitor::new(backpressure_threshold),
            metrics,
        }
    }
}

#[async_trait::async_trait]
impl RuntimeEngine for AsyncRuntime {
    async fn submit(&self, op: Operation) -> Result<OperationResult> {
        // 1. Acquire semaphore (backpressure control)
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| RuntimeError::PoolExhausted)?;

        // 2. Check backpressure
        let stats = self.stats();
        if self.backpressure_monitor.is_saturated(&stats) {
            self.metrics.record_backpressure_event();
        }

        // 3. Get connection
        let mut conn = self.pool.get().await?;

        // 4. Execute operation
        let start = Instant::now();
        let result = conn.execute(&op.sql, &op.params).await;
        let duration = start.elapsed();

        // 5. Record metrics
        self.metrics.record_operation(&op.name, duration, &result);

        // 6. Return result
        match result {
            Ok(query_result) => Ok(OperationResult {
                success: true,
                duration,
                rows_affected: query_result.rows_affected,
                error: None,
            }),
            Err(e) => Ok(OperationResult {
                success: false,
                duration,
                rows_affected: 0,
                error: Some(e.to_string()),
            }),
        }
    }

    fn stats(&self) -> RuntimeStats {
        let pool_stats = self.pool.stats();
        let pool_utilization = if pool_stats.total_connections > 0 {
            pool_stats.active_connections as f64 / pool_stats.total_connections as f64
        } else {
            0.0
        };

        let backpressure_active = self.backpressure_monitor.threshold < pool_utilization;

        RuntimeStats {
            active_connections: pool_stats.active_connections,
            queued_operations: self.semaphore.available_permits(),
            pool_utilization,
            backpressure_active,
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Wait for all permits to be returned
        let available = self.semaphore.available_permits();
        if available > 0 {
            let _ = self.semaphore.acquire_many(available as u32).await;
        }
        Ok(())
    }
}

/// Backpressure monitor
struct BackpressureMonitor {
    threshold: f64,
}

impl BackpressureMonitor {
    fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    fn is_saturated(&self, stats: &RuntimeStats) -> bool {
        stats.pool_utilization > self.threshold
    }
}
