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
