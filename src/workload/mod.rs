//! Workload module
//!
//! Defines workload abstraction and implementations.

mod declarative;

#[cfg(feature = "lua")]
mod lua;

pub use declarative::DeclarativeWorkload;

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

    /// Is this a transaction (multiple SQL statements)
    #[allow(dead_code)]
    pub is_transaction: bool,

    /// Transaction SQL statements (if is_transaction is true)
    #[allow(dead_code)]
    pub transaction_sqls: Vec<String>,

    /// Transaction parameters (parallel to transaction_sqls)
    #[allow(dead_code)]
    pub transaction_params: Vec<Vec<Value>>,
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
            // Declarative workload (primary method)
            WorkloadConfig::Declarative { file, definition, overrides } => {
                Self::create_declarative(file.as_ref(), definition.as_ref(), overrides.as_ref(), seed)
            }

            // Lua script
            WorkloadConfig::Lua { script } => Self::create_lua(script, seed),
        }
    }

    fn create_declarative(
        file: Option<&std::path::PathBuf>,
        definition: Option<&serde_yaml::Value>,
        overrides: Option<&serde_yaml::Value>,
        seed: u64,
    ) -> Result<Box<dyn Workload>> {
        // Load from file or inline definition
        if let Some(path) = file {
            Ok(Box::new(DeclarativeWorkload::from_file_with_overrides(path, overrides, seed)?))
        } else if let Some(def) = definition {
            // Convert serde_yaml::Value to string and parse
            let yaml_str = serde_yaml::to_string(def).map_err(|e| {
                crate::Error::Workload(format!("Failed to serialize inline definition: {}", e))
            })?;
            Ok(Box::new(DeclarativeWorkload::from_yaml_with_overrides(&yaml_str, overrides, seed)?))
        } else {
            Err(crate::Error::Workload(
                "Declarative workload must specify either 'file' or 'definition'".into()
            ))
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
            is_transaction: false,
            transaction_sqls: Vec::new(),
            transaction_params: Vec::new(),
        };

        assert_eq!(op.name, "test_op");
        assert_eq!(op.sql, "SELECT * FROM test");
        assert_eq!(op.params.len(), 1);
        assert!(!op.is_transaction);

        match op.operation_type {
            OperationType::Read => {},
            _ => panic!("Expected Read operation type"),
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
