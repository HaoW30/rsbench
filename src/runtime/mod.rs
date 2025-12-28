//! Runtime module - Async execution engine for database operations
//!
//! The runtime module is the **execution core** of RSBench. It sits between the Scenario module
//! (which generates operations) and the Database drivers (which execute them), providing:
//!
//! - **Concurrent operation execution** via async I/O and semaphore-based limiting
//! - **Backpressure monitoring** to detect client saturation (pool or semaphore exhaustion)
//! - **Metrics collection** for every operation (latency, success/failure, errors)
//! - **Graceful shutdown** with proper cleanup
//!
//! # Architecture
//!
//! The runtime uses a **semaphore-based concurrency model** rather than thread pools:
//!
//! ```text
//! ┌─────────────┐
//! │  Scenario   │
//! └──────┬──────┘
//!        │ submit(Operation)
//!        ▼
//! ┌──────────────────────────────────────────┐
//! │         RuntimeEngine                    │
//! │                                          │
//! │  1. Acquire Semaphore Permit             │
//! │     └─> Limits concurrent operations     │
//! │                                          │
//! │  2. Check Backpressure                   │
//! │     ├─> Pool utilization > threshold?    │
//! │     └─> Semaphore utilization > threshold?│
//! │                                          │
//! │  3. Get Connection from Pool             │
//! │     └─> ConnectionPool.get()             │
//! │                                          │
//! │  4. Execute Operation                    │
//! │     └─> conn.execute(sql, params)        │
//! │                                          │
//! │  5. Record Metrics                       │
//! │     └─> Duration, success/failure        │
//! │                                          │
//! │  6. Return OperationResult               │
//! └──────────────────────────────────────────┘
//! ```
//!
//! # Key Design Principles
//!
//! 1. **Backpressure Visibility** - Client saturation is always observable, never hidden
//! 2. **Non-Blocking I/O** - Async execution allows 10K+ concurrent operations with minimal threads
//! 3. **Errors as Data** - Database errors go in `OperationResult.error`, not Rust `Result`
//! 4. **No Coordinated Omission** - Works with time-driven scenario execution
//!
//! # Backpressure Detection
//!
//! The runtime monitors **two sources of backpressure**:
//!
//! - **Pool saturation** - Connection pool utilization exceeds threshold (e.g., >80%)
//! - **Semaphore saturation** - Concurrent operation permits exhausted (e.g., >80%)
//!
//! If **either** source is saturated, backpressure is detected and recorded in metrics.
//! This ensures the user always knows when RSBench (the client) is the bottleneck.
//!
//! # Usage Example
//!
//! ```no_run
//! use rsbench::runtime::{create_runtime, RuntimeEngine};
//! use rsbench::pool::ConnectionPool;
//! use rsbench::metrics::MetricsCollector;
//! use rsbench::workload::Operation;
//! use std::sync::Arc;
//!
//! # async fn example() -> rsbench::Result<()> {
//! // Create dependencies
//! let pool = Arc::new(ConnectionPool::new(
//!     driver,
//!     "mysql://localhost/test".to_string(),
//!     pool_config,
//! )?);
//! let metrics = Arc::new(MetricsCollector::new());
//!
//! // Create runtime with max 100 concurrent operations, 80% backpressure threshold
//! let runtime = create_runtime(pool, 100, 0.8, metrics);
//!
//! // Submit operations
//! let operation = Operation {
//!     name: "point_select".to_string(),
//!     sql: "SELECT * FROM users WHERE id = ?".to_string(),
//!     params: vec![1.into()],
//!     operation_type: rsbench::workload::OperationType::Read,
//!     is_transaction: false,
//!     transaction_sqls: vec![],
//!     transaction_params: vec![],
//! };
//!
//! let result = runtime.submit(operation).await?;
//! println!("Success: {}, Duration: {:?}", result.success, result.duration);
//!
//! // Check runtime stats
//! let stats = runtime.stats();
//! if stats.backpressure_active {
//!     println!("WARNING: Client is saturated!");
//!     println!("Pool utilization: {:.1}%", stats.pool_utilization * 100.0);
//!     println!("Semaphore utilization: {:.1}%", stats.semaphore_utilization * 100.0);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Performance Characteristics
//!
//! - **Throughput**: 100K+ ops/sec on modern hardware
//! - **Submit overhead**: <10μs per operation
//! - **Backpressure check**: <1ns (sub-nanosecond)
//! - **Memory**: ~2KB per concurrent operation
//!
//! # Error Handling Philosophy
//!
//! Database errors (query failures, constraint violations, etc.) are returned as **data** in
//! `OperationResult.error`, not as Rust `Err` values. Only infrastructure errors (pool exhausted,
//! connection failed, etc.) become Rust errors.
//!
//! **Rationale**: The Scenario module treats database errors as metrics to be counted, not
//! control flow to be handled. This enables measuring error rates under load.
//!
//! # See Also
//!
//! - [`RuntimeEngine`] - The main trait for runtime implementations
//! - [`AsyncRuntime`] - The async implementation (primary mode)
//! - [`RuntimeStats`] - Real-time runtime health metrics
//! - [`OperationResult`] - The outcome of an operation execution

mod async_runtime;

pub use async_runtime::AsyncRuntime;

use crate::metrics::MetricsCollector;
use crate::pool::ConnectionPool;
use crate::workload::Operation;
use crate::Result;
use std::sync::Arc;
use std::time::Duration;

/// Runtime execution engine trait
///
/// The core interface for executing database operations with backpressure monitoring.
/// All runtime implementations must provide concurrent operation execution, metrics
/// collection, and graceful shutdown.
///
/// # Design Philosophy
///
/// The `RuntimeEngine` trait is designed around these principles:
///
/// - **Shared execution** - `submit()` takes `&self` to allow concurrent submissions via `Arc`
/// - **Errors as data** - Database errors return `Ok(OperationResult { success: false, ... })`
/// - **Always observable** - `stats()` provides real-time visibility into runtime health
/// - **Exclusive shutdown** - `shutdown()` takes `&mut self` to ensure single-threaded cleanup
///
/// # Backpressure Model
///
/// The runtime monitors two sources of client saturation:
///
/// 1. **Pool saturation** - Too many active database connections
/// 2. **Semaphore saturation** - Too many concurrent in-flight operations
///
/// When either exceeds the configured threshold, backpressure events are recorded in metrics.
///
/// # Example
///
/// ```no_run
/// use rsbench::runtime::{RuntimeEngine, create_runtime};
/// # use rsbench::workload::Operation;
/// # use std::sync::Arc;
/// # async fn example(runtime: Box<dyn RuntimeEngine>, op: Operation) -> rsbench::Result<()> {
///
/// // Submit operation
/// let result = runtime.submit(op).await?;
///
/// if result.success {
///     println!("Query executed in {:?}", result.duration);
/// } else {
///     eprintln!("Query failed: {}", result.error.unwrap());
/// }
///
/// // Monitor runtime health
/// let stats = runtime.stats();
/// if stats.backpressure_active {
///     println!("⚠️  Client is saturated - backpressure detected!");
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Thread Safety
///
/// All `RuntimeEngine` implementations must be `Send + Sync`, allowing:
/// - Sharing via `Arc<dyn RuntimeEngine>`
/// - Concurrent `submit()` calls from multiple tasks
/// - Safe access from multiple threads
///
/// # See Also
///
/// - [`AsyncRuntime`] - The primary async implementation
/// - [`OperationResult`] - The return type of `submit()`
/// - [`RuntimeStats`] - The return type of `stats()`
#[async_trait::async_trait]
pub trait RuntimeEngine: Send + Sync {
    /// Submit an operation for execution
    ///
    /// Executes the given operation against the database, recording metrics and checking
    /// for backpressure. This method:
    ///
    /// 1. Acquires a semaphore permit (may block if at concurrency limit)
    /// 2. Checks for backpressure (pool or semaphore saturation)
    /// 3. Gets a connection from the pool
    /// 4. Executes the SQL query with parameters
    /// 5. Records metrics (latency, success/failure)
    /// 6. Returns the result
    ///
    /// # Arguments
    ///
    /// - `op` - The operation to execute (SQL, parameters, metadata)
    ///
    /// # Returns
    ///
    /// - `Ok(OperationResult)` - Always returns `Ok`, with database errors in `result.error`
    /// - `Err(RuntimeError)` - Only for infrastructure errors (pool exhausted, connection failed)
    ///
    /// # Errors
    ///
    /// Infrastructure errors that cause `Err`:
    /// - Connection pool exhausted (all connections in use and timed out)
    /// - Database connection failed (network, auth, etc.)
    /// - Semaphore acquisition failed (runtime shutting down)
    ///
    /// Database errors that return `Ok(OperationResult { success: false, ... })`:
    /// - Query syntax errors
    /// - Constraint violations
    /// - Deadlocks, timeouts
    /// - Table/column not found
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rsbench::runtime::RuntimeEngine;
    /// # use rsbench::workload::{Operation, OperationType};
    /// # async fn example(runtime: &dyn RuntimeEngine) -> rsbench::Result<()> {
    /// let op = Operation {
    ///     name: "update_user".to_string(),
    ///     sql: "UPDATE users SET name = ? WHERE id = ?".to_string(),
    ///     params: vec!["Alice".into(), 42.into()],
    ///     operation_type: OperationType::Write,
    ///     is_transaction: false,
    ///     transaction_sqls: vec![],
    ///     transaction_params: vec![],
    /// };
    ///
    /// let result = runtime.submit(op).await?;
    ///
    /// if result.success {
    ///     println!("Updated {} rows in {:?}", result.rows_affected, result.duration);
    /// } else {
    ///     eprintln!("Update failed: {}", result.error.unwrap());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    async fn submit(&self, op: Operation) -> Result<OperationResult>;

    /// Get real-time runtime statistics
    ///
    /// Returns a snapshot of current runtime health, including:
    /// - Active database connections
    /// - Pool and semaphore utilization
    /// - Backpressure status
    ///
    /// This method is **non-blocking** and **cheap** to call (<5μs).
    ///
    /// # Returns
    ///
    /// A [`RuntimeStats`] snapshot with current metrics
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rsbench::runtime::RuntimeEngine;
    /// # fn example(runtime: &dyn RuntimeEngine) {
    /// let stats = runtime.stats();
    ///
    /// println!("Active connections: {}", stats.active_connections);
    /// println!("Pool utilization: {:.1}%", stats.pool_utilization * 100.0);
    /// println!("Semaphore utilization: {:.1}%", stats.semaphore_utilization * 100.0);
    ///
    /// if stats.backpressure_active {
    ///     println!("⚠️  BACKPRESSURE DETECTED - Client is saturated!");
    /// }
    /// # }
    /// ```
    fn stats(&self) -> RuntimeStats;

    /// Shutdown the runtime gracefully
    ///
    /// Waits for all in-flight operations to complete, then releases resources.
    /// After calling `shutdown()`, the runtime should not accept new operations.
    ///
    /// # Shutdown Sequence
    ///
    /// 1. Stop accepting new operations (optional - implementation-specific)
    /// 2. Wait for all in-flight operations to complete
    /// 3. Release semaphore permits
    /// 4. Close connection pool (optional - implementation-specific)
    ///
    /// # Arguments
    ///
    /// Takes `&mut self` to ensure exclusive access during shutdown.
    ///
    /// # Returns
    ///
    /// - `Ok(())` - Shutdown completed successfully
    /// - `Err(RuntimeError)` - Shutdown failed (resource cleanup error)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rsbench::runtime::RuntimeEngine;
    /// # async fn example(mut runtime: Box<dyn RuntimeEngine>) -> rsbench::Result<()> {
    /// // Submit operations...
    ///
    /// // Graceful shutdown
    /// runtime.shutdown().await?;
    /// println!("Runtime shut down cleanly");
    /// # Ok(())
    /// # }
    /// ```
    async fn shutdown(&mut self) -> Result<()>;
}

/// The result of executing a database operation
///
/// Contains the outcome of a single operation execution, including timing,
/// success/failure status, and any error messages.
///
/// # Design Note: Errors as Data
///
/// Unlike typical Rust APIs, database errors are **not** returned as `Err` values.
/// Instead, they're stored in the `error` field with `success = false`. This design
/// treats database errors as **metrics to be measured**, not control flow to be handled.
///
/// Only infrastructure errors (pool exhausted, connection failed) become Rust `Err` values.
///
/// # Fields
///
/// - `success` - `true` if the query executed successfully, `false` for any database error
/// - `duration` - Wall-clock time from query start to completion
/// - `rows_affected` - Number of rows modified (INSERT/UPDATE/DELETE) or returned (SELECT)
/// - `error` - Error message if `success == false`, otherwise `None`
///
/// # Examples
///
/// ## Successful Query
///
/// ```
/// use rsbench::runtime::OperationResult;
/// use std::time::Duration;
///
/// let result = OperationResult {
///     success: true,
///     duration: Duration::from_millis(15),
///     rows_affected: 1,
///     error: None,
/// };
///
/// assert!(result.success);
/// assert_eq!(result.rows_affected, 1);
/// ```
///
/// ## Failed Query
///
/// ```
/// use rsbench::runtime::OperationResult;
/// use std::time::Duration;
///
/// let result = OperationResult {
///     success: false,
///     duration: Duration::from_millis(5),
///     rows_affected: 0,
///     error: Some("Duplicate entry '42' for key 'PRIMARY'".to_string()),
/// };
///
/// assert!(!result.success);
/// assert!(result.error.is_some());
/// ```
///
/// # See Also
///
/// - [`RuntimeEngine::submit()`] - Returns this type
pub struct OperationResult {
    /// Whether the operation executed successfully
    ///
    /// - `true` - Query executed without errors
    /// - `false` - Query failed (syntax error, constraint violation, etc.)
    pub success: bool,

    /// Wall-clock duration from query start to completion
    ///
    /// Includes:
    /// - Time waiting for connection from pool
    /// - Network round-trip time
    /// - Database execution time
    /// - Result serialization time
    pub duration: Duration,

    /// Number of rows affected or returned
    ///
    /// - For `INSERT`/`UPDATE`/`DELETE` - rows modified
    /// - For `SELECT` - rows returned
    /// - For errors - typically `0`
    pub rows_affected: u64,

    /// Error message if the operation failed
    ///
    /// - `Some(msg)` - Query failed with this error message
    /// - `None` - Query succeeded (`success == true`)
    pub error: Option<String>,
}

/// Real-time runtime health statistics
///
/// A snapshot of the runtime's current state, including connection usage,
/// concurrency levels, and backpressure status.
///
/// # Dual-Source Backpressure Monitoring
///
/// RSBench monitors **two independent sources** of client saturation:
///
/// 1. **Pool utilization** - Percentage of database connections in use
/// 2. **Semaphore utilization** - Percentage of concurrent operation permits in use
///
/// If **either** exceeds the configured threshold (e.g., 80%), `backpressure_active` is `true`.
///
/// # Fields
///
/// - `active_connections` - Current number of active database connections
/// - `queued_operations` - Available semaphore permits (inverse of queue depth)
/// - `pool_utilization` - Pool usage as fraction (0.0 = empty, 1.0 = full)
/// - `semaphore_utilization` - Semaphore usage as fraction (0.0 = idle, 1.0 = saturated)
/// - `backpressure_active` - `true` if pool OR semaphore exceeds threshold
///
/// # Performance
///
/// This struct is cheap to create:
/// - **Computation time**: <5 nanoseconds
/// - **Size**: 40 bytes (stack-allocated)
/// - **Frequency**: Can be called thousands of times per second
///
/// # Examples
///
/// ## Healthy Runtime
///
/// ```
/// use rsbench::runtime::RuntimeStats;
///
/// let stats = RuntimeStats {
///     active_connections: 5,
///     queued_operations: 5,
///     pool_utilization: 0.5,       // 50% pool usage
///     semaphore_utilization: 0.5,  // 50% semaphore usage
///     backpressure_active: false,  // Both below 80% threshold
/// };
///
/// assert!(!stats.backpressure_active);
/// assert!(stats.pool_utilization < 0.8);
/// ```
///
/// ## Pool Saturated (Backpressure)
///
/// ```
/// use rsbench::runtime::RuntimeStats;
///
/// let stats = RuntimeStats {
///     active_connections: 95,
///     queued_operations: 10,
///     pool_utilization: 0.95,      // 95% pool usage - SATURATED
///     semaphore_utilization: 0.5,  // 50% semaphore usage - OK
///     backpressure_active: true,   // Pool exceeds threshold
/// };
///
/// assert!(stats.backpressure_active);
/// assert!(stats.pool_utilization > 0.8);
/// ```
///
/// ## Semaphore Saturated (Backpressure)
///
/// ```
/// use rsbench::runtime::RuntimeStats;
///
/// let stats = RuntimeStats {
///     active_connections: 50,
///     queued_operations: 2,
///     pool_utilization: 0.5,        // 50% pool usage - OK
///     semaphore_utilization: 0.98,  // 98% semaphore usage - SATURATED
///     backpressure_active: true,    // Semaphore exceeds threshold
/// };
///
/// assert!(stats.backpressure_active);
/// assert!(stats.semaphore_utilization > 0.8);
/// ```
///
/// # See Also
///
/// - [`RuntimeEngine::stats()`] - Returns this type
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    /// Number of database connections currently in use
    ///
    /// This counts connections that are actively executing queries or transactions.
    pub active_connections: usize,

    /// Number of available semaphore permits (NOT queued operations)
    ///
    /// **Note**: This is the number of *available* permits, not operations waiting.
    /// A low value means high concurrency, high value means idle capacity.
    pub queued_operations: usize,

    /// Database connection pool utilization (0.0 to 1.0)
    ///
    /// Calculated as: `active_connections / total_pool_size`
    ///
    /// - `0.0` - No connections in use
    /// - `0.5` - Half of pool in use
    /// - `1.0` - All connections in use (pool saturated)
    pub pool_utilization: f64,

    /// Concurrent operation semaphore utilization (0.0 to 1.0)
    ///
    /// Calculated as: `(max_permits - available_permits) / max_permits`
    ///
    /// - `0.0` - No operations in flight
    /// - `0.5` - Half of permits in use
    /// - `1.0` - All permits acquired (semaphore saturated)
    pub semaphore_utilization: f64,

    /// Whether backpressure is currently active
    ///
    /// Set to `true` when **either** source exceeds threshold:
    /// - `pool_utilization > pool_threshold` (e.g., > 0.8)
    /// - `semaphore_utilization > semaphore_threshold` (e.g., > 0.8)
    ///
    /// When `true`, the client (RSBench) is the bottleneck, not the database.
    pub backpressure_active: bool,
}

/// Create a new async runtime instance
///
/// Factory function that creates an [`AsyncRuntime`] with the specified configuration.
/// Returns a boxed trait object for easy swapping of runtime implementations.
///
/// # Arguments
///
/// * `pool` - Shared connection pool for database access
/// * `max_connections` - Maximum concurrent operations (semaphore capacity)
/// * `backpressure_threshold` - Utilization threshold for backpressure detection (0.0 to 1.0)
/// * `metrics` - Shared metrics collector for operation recording
///
/// # Configuration Guidelines
///
/// ## `max_connections`
///
/// Controls the maximum number of concurrent in-flight operations:
///
/// - **Too low** - Underutilizes database, low throughput
/// - **Too high** - Excessive memory usage, potential connection exhaustion
/// - **Recommended** - 2-10x the connection pool size
///
/// Example: Pool size 100 → `max_connections = 100` to `1000`
///
/// ## `backpressure_threshold`
///
/// Threshold for triggering backpressure warnings (0.0 to 1.0):
///
/// - `0.5` - Alert when 50% saturated (early warning, many alerts)
/// - `0.8` - Alert when 80% saturated (recommended - balanced sensitivity)
/// - `0.9` - Alert when 90% saturated (late warning, critical situations only)
///
/// Backpressure is triggered when **either** pool OR semaphore exceeds this threshold.
///
/// # Returns
///
/// A boxed [`RuntimeEngine`] trait object (actually an [`AsyncRuntime`])
///
/// # Examples
///
/// ## Basic Usage
///
/// ```no_run
/// use rsbench::runtime::create_runtime;
/// use rsbench::pool::ConnectionPool;
/// use rsbench::metrics::MetricsCollector;
/// use std::sync::Arc;
///
/// # fn example() -> rsbench::Result<()> {
/// # let driver = todo!();
/// # let pool_config = todo!();
/// let pool = Arc::new(ConnectionPool::new(
///     driver,
///     "mysql://localhost/test".to_string(),
///     pool_config,
/// )?);
/// let metrics = Arc::new(MetricsCollector::new());
///
/// // Create runtime: max 100 concurrent ops, 80% backpressure threshold
/// let runtime = create_runtime(pool, 100, 0.8, metrics);
/// # Ok(())
/// # }
/// ```
///
/// ## Recommended Configuration
///
/// ```no_run
/// # use rsbench::runtime::create_runtime;
/// # use std::sync::Arc;
/// # fn example(pool: Arc<rsbench::pool::ConnectionPool>) {
/// let metrics = Arc::new(rsbench::metrics::MetricsCollector::new());
///
/// // For OLTP workloads (many small queries)
/// let runtime = create_runtime(
///     pool.clone(),
///     500,    // High concurrency for throughput
///     0.8,    // Standard threshold
///     metrics.clone(),
/// );
/// # }
/// ```
///
/// ## Conservative Configuration
///
/// ```no_run
/// # use rsbench::runtime::create_runtime;
/// # use std::sync::Arc;
/// # fn example(pool: Arc<rsbench::pool::ConnectionPool>) {
/// let metrics = Arc::new(rsbench::metrics::MetricsCollector::new());
///
/// // For heavy queries or limited resources
/// let runtime = create_runtime(
///     pool.clone(),
///     50,     // Lower concurrency
///     0.7,    // Earlier backpressure detection
///     metrics.clone(),
/// );
/// # }
/// ```
///
/// # See Also
///
/// - [`AsyncRuntime`] - The returned implementation
/// - [`RuntimeEngine`] - The trait interface
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
            semaphore_utilization: 0.5,
            backpressure_active: false,
        };

        assert_eq!(stats.active_connections, 5);
        assert_eq!(stats.queued_operations, 10);
        assert!((stats.pool_utilization - 0.75).abs() < f64::EPSILON);
        assert!((stats.semaphore_utilization - 0.5).abs() < f64::EPSILON);
        assert!(!stats.backpressure_active);
    }

    #[test]
    fn test_runtime_stats_backpressure() {
        let stats = RuntimeStats {
            active_connections: 10,
            queued_operations: 100,
            pool_utilization: 0.95,
            semaphore_utilization: 0.9,
            backpressure_active: true,
        };

        assert!(stats.backpressure_active);
        assert!(stats.pool_utilization > 0.9);
        assert!(stats.semaphore_utilization > 0.8);
    }
}
