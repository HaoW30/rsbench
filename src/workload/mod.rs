//! Workload module
//!
//! Defines workload abstraction and implementations.

mod oltp;

#[cfg(feature = "lua")]
mod lua;

pub use oltp::OltpReadWrite;

#[cfg(feature = "lua")]
pub use lua::LuaWorkload;

use crate::config::WorkloadConfig;
use crate::{Result, Value};
use std::path::Path;
use std::time::Duration;

/// Workload trait - all workload types implement this
pub trait Workload: Send + Sync {
    /// Prepare workload (create tables, load data)
    fn prepare(&mut self, ctx: &mut PrepareContext) -> Result<()>;

    /// Generate next operation (deterministic)
    fn next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation>;

    /// Cleanup workload
    fn cleanup(&mut self) -> Result<()>;

    /// Workload name
    fn name(&self) -> &str;
}

/// Context for preparation phase
pub struct PrepareContext<'a> {
    /// Database connection for setup
    pub database: &'a mut dyn PrepareDatabase,

    /// Determinism seed
    pub seed: u64,

    /// Number of workers
    pub worker_count: usize,
}

/// Database operations available during prepare
pub trait PrepareDatabase {
    fn execute(&mut self, sql: &str) -> Result<()>;
}

/// Context for operation execution
pub struct ExecutionContext {
    /// Worker ID (for deterministic RNG)
    pub worker_id: usize,

    /// Iteration number
    pub iteration: u64,

    /// Elapsed time since scenario start
    pub elapsed: Duration,
}

/// Generated operation
#[derive(Debug, Clone)]
pub struct Operation {
    /// Operation name (for metrics)
    pub name: String,

    /// SQL query
    pub sql: String,

    /// Query parameters
    pub params: Vec<Value>,

    /// Operation type
    pub operation_type: OperationType,
}

#[derive(Debug, Clone, Copy)]
pub enum OperationType {
    Read,
    Write,
}

/// Factory for creating workload instances
pub struct WorkloadFactory;

impl WorkloadFactory {
    /// Create workload from configuration
    pub fn create(config: &WorkloadConfig, seed: u64) -> Result<Box<dyn Workload>> {
        match config {
            WorkloadConfig::Builtin { name, .. } => Self::create_builtin(name, config, seed),
            WorkloadConfig::Lua { script } => Self::create_lua(script, seed),
        }
    }

    fn create_builtin(
        name: &str,
        config: &WorkloadConfig,
        seed: u64,
    ) -> Result<Box<dyn Workload>> {
        match name {
            "oltp_read_write" => Ok(Box::new(OltpReadWrite::new(config, seed)?)),
            _ => Err(crate::Error::Workload(format!(
                "Unknown builtin workload: {}",
                name
            ))),
        }
    }

    fn create_lua(script: &Path, seed: u64) -> Result<Box<dyn Workload>> {
        #[cfg(feature = "lua")]
        {
            Ok(Box::new(LuaWorkload::new(script, seed)?))
        }

        #[cfg(not(feature = "lua"))]
        {
            let _ = (script, seed);
            Err(crate::Error::Workload(
                "Lua support not compiled in. Rebuild with --features lua".into()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_type_variants() {
        let read_op = OperationType::Read;
        let write_op = OperationType::Write;

        // Test pattern matching
        match read_op {
            OperationType::Read => {},
            _ => panic!("Expected Read variant"),
        }

        match write_op {
            OperationType::Write => {},
            _ => panic!("Expected Write variant"),
        }
    }

    #[test]
    fn test_execution_context_creation() {
        let ctx = ExecutionContext {
            worker_id: 1,
            iteration: 42,
            elapsed: Duration::from_secs(10),
        };

        assert_eq!(ctx.worker_id, 1);
        assert_eq!(ctx.iteration, 42);
        assert_eq!(ctx.elapsed, Duration::from_secs(10));
    }

    #[test]
    fn test_operation_creation() {
        let op = Operation {
            name: "test_op".to_string(),
            sql: "SELECT * FROM test".to_string(),
            params: vec![Value::Int(42)],
            operation_type: OperationType::Read,
        };

        assert_eq!(op.name, "test_op");
        assert_eq!(op.sql, "SELECT * FROM test");
        assert_eq!(op.params.len(), 1);

        match op.operation_type {
            OperationType::Read => {},
            _ => panic!("Expected Read operation type"),
        }
    }

    #[test]
    fn test_workload_factory_builtin_oltp() {
        let config = WorkloadConfig::Builtin {
            name: "oltp_read_write".to_string(),
            table_count: 1,
            table_size: 100,
        };

        let result = WorkloadFactory::create(&config, 42);
        assert!(result.is_ok());

        let workload = result.unwrap();
        assert_eq!(workload.name(), "oltp_read_write");
    }

    #[test]
    fn test_workload_factory_unknown_builtin() {
        let config = WorkloadConfig::Builtin {
            name: "unknown_workload".to_string(),
            table_count: 1,
            table_size: 100,
        };

        let result = WorkloadFactory::create(&config, 42);
        assert!(result.is_err());

        match result {
            Err(crate::Error::Workload(msg)) => {
                assert!(msg.contains("Unknown builtin workload"));
                assert!(msg.contains("unknown_workload"));
            }
            _ => panic!("Expected Workload error"),
        }
    }

    #[test]
    #[cfg(not(feature = "lua"))]
    fn test_workload_factory_lua_not_compiled() {
        use std::path::PathBuf;

        let config = WorkloadConfig::Lua {
            script: PathBuf::from("test.lua"),
        };

        let result = WorkloadFactory::create(&config, 42);
        assert!(result.is_err());

        match result {
            Err(crate::Error::Workload(msg)) => {
                assert!(msg.contains("Lua support not compiled in"));
            }
            _ => panic!("Expected Workload error about Lua not compiled"),
        }
    }
}
