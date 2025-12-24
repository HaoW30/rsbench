//! RSBench - Modern Database Testing Tool
//!
//! A time-driven, rate-based database benchmarking tool built in Rust.
//! Designed to address fundamental limitations in traditional tools like sysbench.
//!
//! # Core Principles
//!
//! - **Time as the primary control plane**: Load defined by rate, not threads
//! - **Backpressure visibility**: Client saturation is observable, never hidden
//! - **Deterministic execution**: Same seed produces same operations
//! - **Async-first**: High throughput with backpressure awareness

use std::time::Duration;

// Public modules
pub mod config;
pub mod workload;
pub mod rate_limiter;
pub mod runtime;
pub mod pool;
pub mod driver;
pub mod metrics;
pub mod scenario;
pub mod cli;

// Re-export commonly used types
pub use config::{ConfigLoader, ConfigSource, ToolConfig};
pub use workload::{Workload, WorkloadFactory, Operation};
pub use runtime::{RuntimeEngine, RuntimeFactory};
pub use metrics::{MetricsCollector, MetricsSnapshot};
pub use scenario::ScenarioExecutor;
pub use driver::DriverRegistry;

/// Result type for the entire tool
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Workload error: {0}")]
    Workload(String),

    #[error("Runtime error: {0}")]
    Runtime(#[from] RuntimeError),

    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),

    #[error("Metrics error: {0}")]
    Metrics(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Runtime-specific errors
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Pool exhausted")]
    PoolExhausted,

    #[error("Operation timeout after {0:?}")]
    Timeout(Duration),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Backpressure saturation")]
    BackpressureSaturation,
}

/// Database-specific errors
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Query execution error: {0}")]
    Query(String),

    #[error("Transaction error: {0}")]
    Transaction(String),

    #[error("Driver not found: {0}")]
    DriverNotFound(String),
}

/// SQL parameter value
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Null,
}
