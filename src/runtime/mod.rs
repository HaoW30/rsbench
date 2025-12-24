//! Runtime module
//!
//! Execution engines for database operations.

mod async_runtime;
mod blocking;

pub use async_runtime::AsyncRuntime;
pub use blocking::BlockingRuntime;

use crate::config::RuntimeMode;
use crate::metrics::MetricsCollector;
use crate::pool::ConnectionPool;
use crate::workload::Operation;
use crate::{Result, RuntimeError};
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

/// Factory for creating runtime instances
pub struct RuntimeFactory;

impl RuntimeFactory {
    pub fn create(
        mode: &RuntimeMode,
        pool: Arc<ConnectionPool>,
        metrics: Arc<MetricsCollector>,
    ) -> Result<Box<dyn RuntimeEngine>> {
        match mode {
            RuntimeMode::Async {
                max_connections,
                backpressure_threshold,
                ..
            } => Ok(Box::new(AsyncRuntime::new(
                pool,
                *max_connections,
                *backpressure_threshold,
                metrics,
            ))),
            RuntimeMode::Blocking { threads } => {
                Ok(Box::new(BlockingRuntime::new(pool, *threads, metrics)))
            }
        }
    }
}
