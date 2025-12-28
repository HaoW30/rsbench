//! Runtime module
//!
//! Execution engine for database operations.

mod async_runtime;

pub use async_runtime::AsyncRuntime;

use crate::metrics::MetricsCollector;
use crate::pool::ConnectionPool;
use crate::workload::Operation;
use crate::Result;
use std::sync::Arc;
use std::time::Duration;

/// Runtime execution engine trait
#[async_trait::async_trait]
pub trait RuntimeEngine: Send + Sync {
    /// Submit operation for execution
    async fn submit(&self, op: Operation) -> Result<OperationResult>;

    /// Get runtime statistics
    fn stats(&self) -> RuntimeStats;

    /// Shutdown gracefully
    async fn shutdown(&mut self) -> Result<()>;
}

/// Operation execution result
pub struct OperationResult {
    pub success: bool,
    pub duration: Duration,
    pub rows_affected: u64,
    pub error: Option<String>,
}

/// Runtime statistics
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    pub active_connections: usize,
    pub queued_operations: usize,
    pub pool_utilization: f64,
    pub backpressure_active: bool,
}

/// Create a runtime instance from configuration
pub fn create_runtime(
    pool: Arc<ConnectionPool>,
    max_connections: usize,
    backpressure_threshold: f64,
    metrics: Arc<MetricsCollector>,
) -> Box<dyn RuntimeEngine> {
    Box::new(AsyncRuntime::new(
        pool,
        max_connections,
        backpressure_threshold,
        metrics,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_result_success() {
        let result = OperationResult {
            success: true,
            duration: Duration::from_millis(10),
            rows_affected: 5,
            error: None,
        };

        assert!(result.success);
        assert_eq!(result.duration, Duration::from_millis(10));
        assert_eq!(result.rows_affected, 5);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_operation_result_failure() {
        let result = OperationResult {
            success: false,
            duration: Duration::from_millis(5),
            rows_affected: 0,
            error: Some("query failed".to_string()),
        };

        assert!(!result.success);
        assert_eq!(result.duration, Duration::from_millis(5));
        assert_eq!(result.rows_affected, 0);
        assert_eq!(result.error, Some("query failed".to_string()));
    }

    #[test]
    fn test_runtime_stats() {
        let stats = RuntimeStats {
            active_connections: 5,
            queued_operations: 10,
            pool_utilization: 0.75,
            backpressure_active: false,
        };

        assert_eq!(stats.active_connections, 5);
        assert_eq!(stats.queued_operations, 10);
        assert!((stats.pool_utilization - 0.75).abs() < f64::EPSILON);
        assert!(!stats.backpressure_active);
    }

    #[test]
    fn test_runtime_stats_backpressure() {
        let stats = RuntimeStats {
            active_connections: 10,
            queued_operations: 100,
            pool_utilization: 0.95,
            backpressure_active: true,
        };

        assert!(stats.backpressure_active);
        assert!(stats.pool_utilization > 0.9);
    }
}
