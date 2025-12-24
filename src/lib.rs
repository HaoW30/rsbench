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
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Null,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_variants() {
        let int_val = Value::Int(42);
        let float_val = Value::Float(3.14);
        let string_val = Value::String("test".to_string());
        let bytes_val = Value::Bytes(vec![1, 2, 3]);
        let null_val = Value::Null;

        // Test pattern matching
        match int_val {
            Value::Int(v) => assert_eq!(v, 42),
            _ => panic!("Expected Int variant"),
        }

        match float_val {
            Value::Float(v) => assert!((v - 3.14).abs() < f64::EPSILON),
            _ => panic!("Expected Float variant"),
        }

        match string_val {
            Value::String(ref s) => assert_eq!(s, "test"),
            _ => panic!("Expected String variant"),
        }

        match bytes_val {
            Value::Bytes(ref b) => assert_eq!(b, &vec![1, 2, 3]),
            _ => panic!("Expected Bytes variant"),
        }

        match null_val {
            Value::Null => {},
            _ => panic!("Expected Null variant"),
        }
    }

    #[test]
    fn test_value_clone() {
        let original = Value::String("test".to_string());
        let cloned = original.clone();

        assert_eq!(original, cloned);
    }

    #[test]
    fn test_error_display() {
        let config_err = Error::Config("invalid config".to_string());
        assert_eq!(config_err.to_string(), "Configuration error: invalid config");

        let workload_err = Error::Workload("workload failed".to_string());
        assert_eq!(workload_err.to_string(), "Workload error: workload failed");

        let metrics_err = Error::Metrics("metrics error".to_string());
        assert_eq!(metrics_err.to_string(), "Metrics error: metrics error");
    }

    #[test]
    fn test_runtime_error_conversion() {
        let runtime_err = RuntimeError::PoolExhausted;
        let error: Error = runtime_err.into();

        match error {
            Error::Runtime(RuntimeError::PoolExhausted) => {},
            _ => panic!("Expected Runtime error variant"),
        }
    }

    #[test]
    fn test_database_error_conversion() {
        let db_err = DatabaseError::Connection("connection failed".to_string());
        let error: Error = db_err.into();

        match error {
            Error::Database(DatabaseError::Connection(_)) => {},
            _ => panic!("Expected Database error variant"),
        }
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let error: Error = io_err.into();

        match error {
            Error::Io(_) => {},
            _ => panic!("Expected IO error variant"),
        }
    }

    #[test]
    fn test_runtime_error_display() {
        let pool_exhausted = RuntimeError::PoolExhausted;
        assert_eq!(pool_exhausted.to_string(), "Pool exhausted");

        let timeout = RuntimeError::Timeout(Duration::from_secs(5));
        assert_eq!(timeout.to_string(), "Operation timeout after 5s");

        let conn_failed = RuntimeError::ConnectionFailed("test".to_string());
        assert_eq!(conn_failed.to_string(), "Connection failed: test");

        let backpressure = RuntimeError::BackpressureSaturation;
        assert_eq!(backpressure.to_string(), "Backpressure saturation");
    }

    #[test]
    fn test_database_error_display() {
        let conn_err = DatabaseError::Connection("conn error".to_string());
        assert_eq!(conn_err.to_string(), "Connection error: conn error");

        let query_err = DatabaseError::Query("query error".to_string());
        assert_eq!(query_err.to_string(), "Query execution error: query error");

        let tx_err = DatabaseError::Transaction("tx error".to_string());
        assert_eq!(tx_err.to_string(), "Transaction error: tx error");

        let driver_err = DatabaseError::DriverNotFound("mysql".to_string());
        assert_eq!(driver_err.to_string(), "Driver not found: mysql");
    }
}
