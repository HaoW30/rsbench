//! Blocking runtime implementation (sysbench compatibility)

use super::{OperationResult, RuntimeEngine, RuntimeStats};
use crate::metrics::MetricsCollector;
use crate::pool::ConnectionPool;
use crate::workload::Operation;
use crate::{Result, RuntimeError};
use std::sync::Arc;
use std::time::Instant;

/// Blocking runtime (sysbench compatibility)
pub struct BlockingRuntime {
    pool: Arc<ConnectionPool>,
    metrics: Arc<MetricsCollector>,
    #[allow(dead_code)]
    thread_count: usize,
}

impl BlockingRuntime {
    pub fn new(pool: Arc<ConnectionPool>, thread_count: usize, metrics: Arc<MetricsCollector>) -> Self {
        Self {
            pool,
            metrics,
            thread_count,
        }
    }
}

#[async_trait::async_trait]
impl RuntimeEngine for BlockingRuntime {
    async fn submit(&self, op: Operation) -> Result<OperationResult> {
        // M0: Simplified - use spawn_blocking to convert async to sync
        let pool = self.pool.clone();
        let metrics = self.metrics.clone();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let mut conn = pool.get().await?;
                let start = Instant::now();
                let result = conn.execute(&op.sql, &op.params).await;
                let duration = start.elapsed();

                metrics.record_operation(&op.name, duration, &result);

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
            })
        })
        .await
        .map_err(|e| crate::Error::Runtime(RuntimeError::ConnectionFailed(e.to_string())))?
    }

    fn stats(&self) -> RuntimeStats {
        let pool_stats = self.pool.stats();
        let pool_utilization = if pool_stats.total_connections > 0 {
            pool_stats.active_connections as f64 / pool_stats.total_connections as f64
        } else {
            0.0
        };

        RuntimeStats {
            active_connections: pool_stats.active_connections,
            queued_operations: 0,
            pool_utilization,
            backpressure_active: false,
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}
