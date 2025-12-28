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
    max_connections: usize,
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
            max_connections,
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

        // Calculate semaphore utilization
        let available_permits = self.semaphore.available_permits();
        let used_permits = self.max_connections.saturating_sub(available_permits);
        let semaphore_utilization = if self.max_connections > 0 {
            used_permits as f64 / self.max_connections as f64
        } else {
            0.0
        };

        let stats = RuntimeStats {
            active_connections: pool_stats.active_connections,
            queued_operations: available_permits,
            pool_utilization,
            semaphore_utilization,
            backpressure_active: false, // Will be set below
        };

        let backpressure_active = self.backpressure_monitor.is_saturated(&stats);

        RuntimeStats {
            active_connections: pool_stats.active_connections,
            queued_operations: available_permits,
            pool_utilization,
            semaphore_utilization,
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
    pool_threshold: f64,
    semaphore_threshold: f64,
}

impl BackpressureMonitor {
    fn new(threshold: f64) -> Self {
        // Use same threshold for both pool and semaphore by default
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

#[cfg(test)]
mod tests {
    use super::*;

    // Unit tests for BackpressureMonitor
    // (AsyncRuntime integration tests are in tests/integration/runtime_integration_test.rs)

    #[test]
    fn test_backpressure_monitor_creation() {
        let monitor = BackpressureMonitor::new(0.8);
        assert_eq!(monitor.pool_threshold, 0.8);
        assert_eq!(monitor.semaphore_threshold, 0.8);
    }

    #[test]
    fn test_backpressure_monitor_creation_various_thresholds() {
        let monitor1 = BackpressureMonitor::new(0.5);
        assert_eq!(monitor1.pool_threshold, 0.5);
        assert_eq!(monitor1.semaphore_threshold, 0.5);

        let monitor2 = BackpressureMonitor::new(0.9);
        assert_eq!(monitor2.pool_threshold, 0.9);
        assert_eq!(monitor2.semaphore_threshold, 0.9);

        let monitor3 = BackpressureMonitor::new(0.0);
        assert_eq!(monitor3.pool_threshold, 0.0);
        assert_eq!(monitor3.semaphore_threshold, 0.0);

        let monitor4 = BackpressureMonitor::new(1.0);
        assert_eq!(monitor4.pool_threshold, 1.0);
        assert_eq!(monitor4.semaphore_threshold, 1.0);
    }

    #[test]
    fn test_backpressure_monitor_pool_saturation() {
        let monitor = BackpressureMonitor::new(0.8);

        let stats = RuntimeStats {
            active_connections: 9,
            queued_operations: 0,
            pool_utilization: 0.9,  // Above threshold
            semaphore_utilization: 0.5,  // Below threshold
            backpressure_active: false,
        };

        // Should be saturated because pool is above threshold
        assert!(monitor.is_saturated(&stats));
    }

    #[test]
    fn test_backpressure_monitor_semaphore_saturation() {
        let monitor = BackpressureMonitor::new(0.8);

        let stats = RuntimeStats {
            active_connections: 5,
            queued_operations: 0,
            pool_utilization: 0.5,  // Below threshold
            semaphore_utilization: 0.9,  // Above threshold
            backpressure_active: false,
        };

        // Should be saturated because semaphore is above threshold
        assert!(monitor.is_saturated(&stats));
    }

    #[test]
    fn test_backpressure_monitor_no_saturation() {
        let monitor = BackpressureMonitor::new(0.8);

        let stats = RuntimeStats {
            active_connections: 5,
            queued_operations: 0,
            pool_utilization: 0.5,  // Below threshold
            semaphore_utilization: 0.5,  // Below threshold
            backpressure_active: false,
        };

        assert!(!monitor.is_saturated(&stats));
    }

    #[test]
    fn test_backpressure_monitor_boundary_conditions() {
        let monitor = BackpressureMonitor::new(0.8);

        // Exactly at threshold - should not be saturated
        let stats_at = RuntimeStats {
            active_connections: 8,
            queued_operations: 0,
            pool_utilization: 0.8,
            semaphore_utilization: 0.8,
            backpressure_active: false,
        };
        assert!(!monitor.is_saturated(&stats_at));

        // Just above threshold (pool) - should be saturated
        let stats_above = RuntimeStats {
            active_connections: 9,
            queued_operations: 0,
            pool_utilization: 0.801,
            semaphore_utilization: 0.5,
            backpressure_active: false,
        };
        assert!(monitor.is_saturated(&stats_above));

        // Just below threshold - should not be saturated
        let stats_below = RuntimeStats {
            active_connections: 7,
            queued_operations: 0,
            pool_utilization: 0.799,
            semaphore_utilization: 0.799,
            backpressure_active: false,
        };
        assert!(!monitor.is_saturated(&stats_below));
    }

    #[test]
    fn test_backpressure_monitor_edge_cases() {
        let monitor = BackpressureMonitor::new(0.8);

        // Zero utilization
        let stats_zero = RuntimeStats {
            active_connections: 0,
            queued_operations: 0,
            pool_utilization: 0.0,
            semaphore_utilization: 0.0,
            backpressure_active: false,
        };
        assert!(!monitor.is_saturated(&stats_zero));

        // Full utilization
        let stats_full = RuntimeStats {
            active_connections: 10,
            queued_operations: 0,
            pool_utilization: 1.0,
            semaphore_utilization: 1.0,
            backpressure_active: false,
        };
        assert!(monitor.is_saturated(&stats_full));
    }

    #[test]
    fn test_backpressure_monitor_different_thresholds() {
        // Test with different threshold values
        let monitor_low = BackpressureMonitor::new(0.5);
        let monitor_high = BackpressureMonitor::new(0.95);

        let stats = RuntimeStats {
            active_connections: 8,
            queued_operations: 0,
            pool_utilization: 0.8,
            semaphore_utilization: 0.8,
            backpressure_active: false,
        };

        // 0.8 utilization > 0.5 threshold
        assert!(monitor_low.is_saturated(&stats));

        // 0.8 utilization < 0.95 threshold
        assert!(!monitor_high.is_saturated(&stats));
    }

    #[test]
    fn test_backpressure_monitor_both_sources() {
        let monitor = BackpressureMonitor::new(0.8);

        // Both saturated
        let stats_both = RuntimeStats {
            active_connections: 10,
            queued_operations: 0,
            pool_utilization: 0.9,
            semaphore_utilization: 0.95,
            backpressure_active: false,
        };
        assert!(monitor.is_saturated(&stats_both));

        // Only pool saturated
        let stats_pool = RuntimeStats {
            active_connections: 9,
            queued_operations: 5,
            pool_utilization: 0.9,
            semaphore_utilization: 0.5,
            backpressure_active: false,
        };
        assert!(monitor.is_saturated(&stats_pool));

        // Only semaphore saturated
        let stats_sem = RuntimeStats {
            active_connections: 5,
            queued_operations: 1,
            pool_utilization: 0.5,
            semaphore_utilization: 0.95,
            backpressure_active: false,
        };
        assert!(monitor.is_saturated(&stats_sem));

        // Neither saturated
        let stats_neither = RuntimeStats {
            active_connections: 5,
            queued_operations: 5,
            pool_utilization: 0.5,
            semaphore_utilization: 0.5,
            backpressure_active: false,
        };
        assert!(!monitor.is_saturated(&stats_neither));
    }
}
