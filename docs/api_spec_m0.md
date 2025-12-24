# RSBench Milestone 0 - API Specification

## Document Overview

**Purpose**: Detailed API specification for all Milestone 0 modules
**Audience**: Implementation developers
**Status**: Implementation Ready

This document provides the complete API surface for Milestone 0, including:
- Module interfaces and traits
- Data structures and types
- Function signatures
- Error handling
- Usage examples

## Table of Contents

1. [Core Types and Errors](#1-core-types-and-errors)
2. [Config Module](#2-config-module)
3. [Workload Module](#3-workload-module)
4. [Rate Limiter Module](#4-rate-limiter-module)
5. [Runtime Module](#5-runtime-module)
6. [Connection Pool Module](#6-connection-pool-module)
7. [Driver Module](#7-driver-module)
8. [Metrics Module](#8-metrics-module)
9. [Scenario Module](#9-scenario-module)
10. [CLI Module](#10-cli-module)

---

## 1. Core Types and Errors

### 1.1 Common Types

```rust
// lib.rs - Core types used across modules

use std::time::Duration;

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
```

---

## 2. Config Module

**Module**: `rsbench::config`
**File**: `src/config/mod.rs`

### 2.1 Public API

```rust
use std::path::PathBuf;
use std::time::Duration;
use serde::{Deserialize, Serialize};

/// Configuration loader - entry point for loading config
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from various sources
    pub fn load(source: ConfigSource) -> Result<ToolConfig> {
        todo!("Load and validate configuration")
    }

    /// Validate configuration consistency
    pub fn validate(config: &ToolConfig) -> Result<()> {
        todo!("Validate all config constraints")
    }

    /// Merge CLI args with file config (precedence: CLI > file > defaults)
    pub fn merge(file_config: ToolConfig, cli_args: CliArgs) -> ToolConfig {
        todo!("Merge configurations with precedence")
    }

    /// Apply default values to incomplete config
    pub fn with_defaults(config: ToolConfig) -> ToolConfig {
        todo!("Apply sensible defaults")
    }
}

/// Configuration source
pub enum ConfigSource {
    /// Load from YAML file
    File(PathBuf),
    /// Parse from YAML string
    Yaml(String),
    /// From CLI arguments only
    CliArgs(CliArgs),
}

/// Root configuration structure (M0 simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub database: DatabaseConfig,
    pub runtime: RuntimeConfig,
    pub scenario: ScenarioConfig,
    pub determinism: DeterminismConfig,
    pub output: OutputConfig,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Driver name ("mysql")
    pub driver: String,

    /// Connection string (e.g., "mysql://user:pass@host:port/db")
    pub connection_string: String,

    /// Connection pool configuration
    pub pool: PoolConfig,
}

/// Connection pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Minimum pool size
    #[serde(default = "default_min_size")]
    pub min_size: usize,

    /// Maximum pool size
    #[serde(default = "default_max_size")]
    pub max_size: usize,

    /// Connection timeout
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: Duration,

    /// Idle connection timeout
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: Duration,
}

fn default_min_size() -> usize { 10 }
fn default_max_size() -> usize { 100 }
fn default_connection_timeout() -> Duration { Duration::from_secs(10) }
fn default_idle_timeout() -> Duration { Duration::from_secs(300) }

/// Runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub mode: RuntimeMode,
}

/// Runtime execution mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuntimeMode {
    /// Blocking mode (sysbench compatibility)
    Blocking {
        #[serde(default = "default_threads")]
        threads: usize,
    },
    /// Async mode (primary, backpressure-aware)
    Async {
        #[serde(default = "default_workers")]
        workers: usize,

        #[serde(default = "default_max_connections")]
        max_connections: usize,

        #[serde(default = "default_backpressure_threshold")]
        backpressure_threshold: f64,
    },
}

fn default_threads() -> usize { 16 }
fn default_workers() -> usize { num_cpus::get() }
fn default_max_connections() -> usize { 100 }
fn default_backpressure_threshold() -> f64 { 0.8 }

/// Scenario configuration (M0: single scenario only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioConfig {
    /// Executor configuration
    pub executor: ExecutorConfig,

    /// Workload definition
    pub workload: WorkloadConfig,
}

/// Executor configuration (M0: constant-rate and ramping-rate)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ExecutorConfig {
    ConstantRate {
        /// Operations per second
        rate: u64,

        /// Test duration
        #[serde(with = "humantime_serde")]
        duration: Duration,

        /// Maximum concurrent connections
        #[serde(default = "default_max_connections")]
        max_connections: usize,
    },
    RampingRate {
        /// Rate stages
        stages: Vec<RateStage>,

        /// Preallocate connections
        #[serde(default = "default_prealloc")]
        prealloc_connections: usize,

        /// Maximum concurrent connections
        #[serde(default = "default_max_connections")]
        max_connections: usize,
    },
}

fn default_prealloc() -> usize { 50 }

/// Rate stage for ramping executor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateStage {
    #[serde(with = "humantime_serde")]
    pub duration: Duration,
    pub target_rate: u64,
}

/// Workload configuration (M0: builtin or Lua)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WorkloadConfig {
    /// Built-in OLTP workload
    Builtin {
        name: String,  // "oltp_read_write"
        #[serde(default = "default_table_count")]
        table_count: usize,
        #[serde(default = "default_table_size")]
        table_size: usize,
    },
    /// Lua script (sysbench compatible)
    Lua {
        script: PathBuf,
    },
}

fn default_table_count() -> usize { 10 }
fn default_table_size() -> usize { 10000 }

/// Determinism configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterminismConfig {
    /// RNG seed
    pub seed: u64,

    /// Strict mode (fail on non-deterministic operations)
    #[serde(default)]
    pub strict_mode: bool,
}

impl Default for DeterminismConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            strict_mode: false,
        }
    }
}

/// Output configuration (M0: text or JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output format
    pub format: OutputFormat,

    /// Output file (None = stdout)
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
}

/// CLI arguments structure
pub struct CliArgs {
    pub config_file: Option<PathBuf>,
    pub database_url: Option<String>,
    pub rate: Option<u64>,
    pub duration: Option<Duration>,
    pub threads: Option<usize>,
    pub output_format: Option<OutputFormat>,
}
```

### 2.2 Configuration Example

```yaml
# M0 example config
database:
  driver: mysql
  connection_string: "mysql://root@localhost:3306/testdb"
  pool:
    min_size: 10
    max_size: 100
    connection_timeout: 10s
    idle_timeout: 5m

runtime:
  type: async
  workers: 8
  max_connections: 100
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 5m
    max_connections: 100

  workload:
    type: builtin
    name: oltp_read_write
    table_count: 10
    table_size: 10000

determinism:
  seed: 12345
  strict_mode: false

output:
  format: json
  file: results.json
```

---

## 3. Workload Module

**Module**: `rsbench::workload`
**File**: `src/workload/mod.rs`

### 3.1 Core Trait

```rust
use std::time::Duration;

/// Workload trait - all workload types implement this
pub trait Workload: Send + Sync {
    /// Prepare workload (create tables, load data)
    fn prepare(&mut self, ctx: &PrepareContext) -> Result<()>;

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
    pub database: &'a dyn PrepareDatabase,

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
```

### 3.2 Workload Factory

```rust
use crate::config::WorkloadConfig;

/// Factory for creating workload instances
pub struct WorkloadFactory;

impl WorkloadFactory {
    /// Create workload from configuration
    pub fn create(config: &WorkloadConfig, seed: u64) -> Result<Box<dyn Workload>> {
        match config {
            WorkloadConfig::Builtin { name, .. } => {
                Self::create_builtin(name, config, seed)
            }
            WorkloadConfig::Lua { script } => {
                Self::create_lua(script, seed)
            }
        }
    }

    fn create_builtin(
        name: &str,
        config: &WorkloadConfig,
        seed: u64
    ) -> Result<Box<dyn Workload>> {
        match name {
            "oltp_read_write" => Ok(Box::new(OltpReadWrite::new(config, seed)?)),
            _ => Err(Error::Workload(format!("Unknown builtin workload: {}", name))),
        }
    }

    fn create_lua(script: &Path, seed: u64) -> Result<Box<dyn Workload>> {
        Ok(Box::new(LuaWorkload::new(script, seed)?))
    }
}
```

### 3.3 Built-in OLTP Workload

```rust
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// OLTP Read/Write workload (sysbench equivalent)
pub struct OltpReadWrite {
    table_count: usize,
    table_size: usize,
    rng: ChaCha8Rng,
}

impl OltpReadWrite {
    pub fn new(config: &WorkloadConfig, seed: u64) -> Result<Self> {
        let (table_count, table_size) = match config {
            WorkloadConfig::Builtin { table_count, table_size, .. } => {
                (*table_count, *table_size)
            }
            _ => return Err(Error::Workload("Invalid config".into())),
        };

        Ok(Self {
            table_count,
            table_size,
            rng: ChaCha8Rng::seed_from_u64(seed),
        })
    }
}

impl Workload for OltpReadWrite {
    fn prepare(&mut self, ctx: &PrepareContext) -> Result<()> {
        // Create sbtest tables
        for i in 1..=self.table_count {
            let create_sql = format!(
                "CREATE TABLE IF NOT EXISTS sbtest{} (
                    id INT PRIMARY KEY,
                    k INT NOT NULL,
                    c CHAR(120) NOT NULL,
                    pad CHAR(60) NOT NULL,
                    INDEX k_idx (k)
                )",
                i
            );
            ctx.database.execute(&create_sql)?;

            // Insert test data
            // TODO: Bulk insert for performance
        }
        Ok(())
    }

    fn next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation> {
        use rand::Rng;

        // Deterministic operation selection
        let op_type = self.rng.gen_range(0..100);
        let table_id = (ctx.iteration % self.table_count as u64) + 1;
        let row_id = self.rng.gen_range(1..=self.table_size as i64);

        if op_type < 60 {
            // 60% reads
            Ok(Operation {
                name: "point_select".into(),
                sql: format!("SELECT c FROM sbtest{} WHERE id = ?", table_id),
                params: vec![Value::Int(row_id)],
                operation_type: OperationType::Read,
            })
        } else {
            // 40% writes
            Ok(Operation {
                name: "update_non_index".into(),
                sql: format!("UPDATE sbtest{} SET c = ? WHERE id = ?", table_id),
                params: vec![
                    Value::String(format!("{:0<120}", ctx.iteration)),
                    Value::Int(row_id),
                ],
                operation_type: OperationType::Write,
            })
        }
    }

    fn cleanup(&mut self) -> Result<()> {
        // No cleanup needed for M0
        Ok(())
    }

    fn name(&self) -> &str {
        "oltp_read_write"
    }
}
```

### 3.4 Lua Workload (M0 - Basic)

```rust
use mlua::prelude::*;

/// Lua-based workload (sysbench compatibility)
pub struct LuaWorkload {
    lua: Lua,
    rng: ChaCha8Rng,
}

impl LuaWorkload {
    pub fn new(script_path: &Path, seed: u64) -> Result<Self> {
        let lua = Lua::new();
        let script = std::fs::read_to_string(script_path)?;

        // Load script
        lua.load(&script).exec()
            .map_err(|e| Error::Workload(format!("Lua error: {}", e)))?;

        Ok(Self {
            lua,
            rng: ChaCha8Rng::seed_from_u64(seed),
        })
    }
}

impl Workload for LuaWorkload {
    fn prepare(&mut self, ctx: &PrepareContext) -> Result<()> {
        // Call Lua prepare() function if exists
        let prepare: LuaFunction = self.lua.globals().get("prepare")?;
        prepare.call::<_, ()>(())
            .map_err(|e| Error::Workload(format!("Lua prepare error: {}", e)))?;
        Ok(())
    }

    fn next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation> {
        // Call Lua event() function
        let event: LuaFunction = self.lua.globals().get("event")?;
        let result: LuaTable = event.call(())?;

        // Extract operation from Lua table
        let sql: String = result.get("sql")?;
        let name: String = result.get("name").unwrap_or_else(|_| "lua_op".into());

        Ok(Operation {
            name,
            sql,
            params: vec![],  // M0: basic support only
            operation_type: OperationType::Read,  // M0: simplified
        })
    }

    fn cleanup(&mut self) -> Result<()> {
        // Call Lua cleanup() if exists
        if let Ok(cleanup) = self.lua.globals().get::<_, LuaFunction>("cleanup") {
            cleanup.call::<_, ()>(())?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "lua_workload"
    }
}
```

---

## 4. Rate Limiter Module

**Module**: `rsbench::rate_limiter`
**File**: `src/rate_limiter.rs`

### 4.1 Public API

```rust
use std::time::{Duration, Instant};
use tokio::time::interval;

/// Token bucket rate limiter
pub struct RateLimiter {
    rate: u64,
    capacity: usize,
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    /// Create new rate limiter
    pub fn new(rate: u64) -> Self {
        let capacity = (rate as usize * 2).max(1);  // Allow small bursts
        Self {
            rate,
            capacity,
            tokens: capacity as f64,
            last_refill: Instant::now(),
        }
    }

    /// Acquire permit to submit one operation
    pub async fn acquire(&mut self) -> Result<Permit> {
        loop {
            self.refill();

            if self.tokens >= 1.0 {
                self.tokens -= 1.0;
                return Ok(Permit);
            }

            // Wait for next refill period
            let wait_time = Duration::from_micros(1_000_000 / self.rate);
            tokio::time::sleep(wait_time).await;
        }
    }

    /// Change rate dynamically (for ramping)
    pub fn set_rate(&mut self, new_rate: u64) {
        self.rate = new_rate;
        self.capacity = (new_rate as usize * 2).max(1);
        // Adjust tokens proportionally
        self.tokens = self.tokens.min(self.capacity as f64);
    }

    /// Get current rate
    pub fn current_rate(&self) -> u64 {
        self.rate
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let new_tokens = (elapsed.as_secs_f64() * self.rate as f64).floor();

        if new_tokens > 0.0 {
            self.tokens = (self.tokens + new_tokens).min(self.capacity as f64);
            self.last_refill = now;
        }
    }
}

/// Permit to submit one operation
pub struct Permit;
```

---

## 5. Runtime Module

**Module**: `rsbench::runtime`
**File**: `src/runtime/mod.rs`

### 5.1 Runtime Trait

```rust
use std::sync::Arc;
use crate::pool::ConnectionPool;
use crate::metrics::MetricsCollector;

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
pub struct RuntimeStats {
    pub active_connections: usize,
    pub queued_operations: usize,
    pub pool_utilization: f64,
    pub backpressure_active: bool,
}
```

### 5.2 Runtime Factory

```rust
use crate::config::RuntimeMode;

pub struct RuntimeFactory;

impl RuntimeFactory {
    pub fn create(
        mode: &RuntimeMode,
        pool: Arc<ConnectionPool>,
        metrics: Arc<MetricsCollector>,
    ) -> Result<Box<dyn RuntimeEngine>> {
        match mode {
            RuntimeMode::Async { max_connections, backpressure_threshold, .. } => {
                Ok(Box::new(AsyncRuntime::new(
                    pool,
                    *max_connections,
                    *backpressure_threshold,
                    metrics,
                )))
            }
            RuntimeMode::Blocking { threads } => {
                Ok(Box::new(BlockingRuntime::new(
                    pool,
                    *threads,
                    metrics,
                )))
            }
        }
    }
}
```

### 5.3 Async Runtime

```rust
use tokio::sync::Semaphore;

/// Async runtime (primary mode)
pub struct AsyncRuntime {
    pool: Arc<ConnectionPool>,
    semaphore: Arc<Semaphore>,
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
            backpressure_monitor: BackpressureMonitor::new(backpressure_threshold),
            metrics,
        }
    }
}

#[async_trait::async_trait]
impl RuntimeEngine for AsyncRuntime {
    async fn submit(&self, op: Operation) -> Result<OperationResult> {
        // 1. Acquire semaphore (backpressure control)
        let _permit = self.semaphore.acquire().await
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
        RuntimeStats {
            active_connections: pool_stats.active_connections,
            queued_operations: self.semaphore.available_permits(),
            pool_utilization: pool_stats.active_connections as f64
                / pool_stats.total_connections as f64,
            backpressure_active: self.backpressure_monitor.is_saturated(
                &RuntimeStats {
                    active_connections: pool_stats.active_connections,
                    queued_operations: 0,
                    pool_utilization: pool_stats.active_connections as f64
                        / pool_stats.total_connections as f64,
                    backpressure_active: false,
                }
            ),
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Wait for all permits to be returned
        let _ = self.semaphore.acquire_many(
            self.semaphore.available_permits() as u32
        ).await;
        Ok(())
    }
}

/// Backpressure monitor
struct BackpressureMonitor {
    threshold: f64,
}

impl BackpressureMonitor {
    fn new(threshold: f64) -> Self {
        Self { threshold }
    }

    fn is_saturated(&self, stats: &RuntimeStats) -> bool {
        stats.pool_utilization > self.threshold
    }
}
```

### 5.4 Blocking Runtime (M0 - Simplified)

```rust
use std::sync::Arc;
use crossbeam::channel;

/// Blocking runtime (sysbench compatibility)
pub struct BlockingRuntime {
    pool: Arc<ConnectionPool>,
    metrics: Arc<MetricsCollector>,
    thread_count: usize,
}

impl BlockingRuntime {
    pub fn new(
        pool: Arc<ConnectionPool>,
        thread_count: usize,
        metrics: Arc<MetricsCollector>,
    ) -> Self {
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
        // M0: Simplified - use block_on to convert async to sync
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
        }).await
        .map_err(|e| Error::Runtime(RuntimeError::ConnectionFailed(e.to_string())))?
    }

    fn stats(&self) -> RuntimeStats {
        let pool_stats = self.pool.stats();
        RuntimeStats {
            active_connections: pool_stats.active_connections,
            queued_operations: 0,
            pool_utilization: pool_stats.active_connections as f64
                / pool_stats.total_connections as f64,
            backpressure_active: false,
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}
```

---

## 6. Connection Pool Module

**Module**: `rsbench::pool`
**File**: `src/pool/mod.rs`

### 6.1 Public API

```rust
use std::sync::Arc;
use deadpool::managed::{Manager, Pool, Object};
use crate::driver::DatabaseDriver;

/// Connection pool (M0: single endpoint only)
pub struct ConnectionPool {
    pool: Pool<ConnectionManager>,
}

impl ConnectionPool {
    /// Create new connection pool
    pub fn new(
        driver: Arc<dyn DatabaseDriver>,
        config: &PoolConfig,
    ) -> Result<Self> {
        let manager = ConnectionManager::new(driver, config.clone());
        let pool = Pool::builder(manager)
            .max_size(config.max_size)
            .build()
            .map_err(|e| Error::Database(DatabaseError::Connection(e.to_string())))?;

        Ok(Self { pool })
    }

    /// Get connection from pool
    pub async fn get(&self) -> Result<PooledConnection> {
        let conn = self.pool.get().await
            .map_err(|e| RuntimeError::ConnectionFailed(e.to_string()))?;
        Ok(PooledConnection { inner: conn })
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        let status = self.pool.status();
        PoolStats {
            total_connections: status.size,
            active_connections: status.size - status.available,
            idle_connections: status.available,
            pending_requests: 0,  // M0: simplified
        }
    }
}

/// Pooled connection wrapper
pub struct PooledConnection {
    inner: Object<ConnectionManager>,
}

impl PooledConnection {
    /// Execute query
    pub async fn execute(
        &mut self,
        sql: &str,
        params: &[Value],
    ) -> Result<QueryResult> {
        self.inner.execute(sql, params).await
    }
}

/// Pool statistics
pub struct PoolStats {
    pub total_connections: usize,
    pub active_connections: usize,
    pub idle_connections: usize,
    pub pending_requests: usize,
}

/// Connection manager for deadpool
struct ConnectionManager {
    driver: Arc<dyn DatabaseDriver>,
    config: PoolConfig,
}

impl ConnectionManager {
    fn new(driver: Arc<dyn DatabaseDriver>, config: PoolConfig) -> Self {
        Self { driver, config }
    }
}

#[async_trait::async_trait]
impl Manager for ConnectionManager {
    type Type = Box<dyn Connection>;
    type Error = DatabaseError;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        let conn_config = ConnectionConfig {
            connection_string: self.config.connection_string.clone(),
            timeout: self.config.connection_timeout,
        };
        self.driver.connect(&conn_config).await
    }

    async fn recycle(&self, conn: &mut Self::Type) -> deadpool::managed::RecycleResult<Self::Error> {
        // Health check
        conn.ping().await
            .map_err(|e| deadpool::managed::RecycleError::Backend(e))?;
        Ok(())
    }
}
```

---

## 7. Driver Module

**Module**: `rsbench::driver`
**File**: `src/driver/mod.rs`

### 7.1 Driver Trait

```rust
/// Database driver trait
#[async_trait::async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// Driver name
    fn name(&self) -> &str;

    /// Create new connection
    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>>;

    /// Driver capabilities
    fn capabilities(&self) -> DriverCapabilities;
}

/// Connection trait
#[async_trait::async_trait]
pub trait Connection: Send + Sync {
    /// Execute query
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult>;

    /// Begin transaction (M0: basic support)
    async fn begin(&mut self) -> Result<()>;

    /// Commit transaction
    async fn commit(&mut self) -> Result<()>;

    /// Rollback transaction
    async fn rollback(&mut self) -> Result<()>;

    /// Health check
    async fn ping(&mut self) -> Result<()>;
}

/// Query execution result
pub struct QueryResult {
    pub rows_affected: u64,
    pub last_insert_id: Option<u64>,
}

/// Driver capabilities
pub struct DriverCapabilities {
    pub supports_transactions: bool,
    pub supports_prepared_statements: bool,
}

/// Connection configuration
pub struct ConnectionConfig {
    pub connection_string: String,
    pub timeout: Duration,
}
```

### 7.2 MySQL Driver

```rust
use mysql_async::{prelude::*, Pool, Conn, Params};

/// MySQL driver implementation
pub struct MySqlDriver;

impl MySqlDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for MySqlDriver {
    fn name(&self) -> &str {
        "mysql"
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let opts = mysql_async::Opts::from_url(&config.connection_string)
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;

        let pool = Pool::new(opts);
        let conn = pool.get_conn().await
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;

        Ok(Box::new(MySqlConnection { conn }))
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            supports_transactions: true,
            supports_prepared_statements: true,
        }
    }
}

/// MySQL connection
struct MySqlConnection {
    conn: Conn,
}

#[async_trait::async_trait]
impl Connection for MySqlConnection {
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        let mysql_params = convert_params(params);

        let result = self.conn.exec_drop(sql, mysql_params).await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(QueryResult {
            rows_affected: self.conn.affected_rows(),
            last_insert_id: Some(self.conn.last_insert_id()),
        })
    }

    async fn begin(&mut self) -> Result<()> {
        self.conn.query_drop("START TRANSACTION").await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))
    }

    async fn commit(&mut self) -> Result<()> {
        self.conn.query_drop("COMMIT").await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))
    }

    async fn rollback(&mut self) -> Result<()> {
        self.conn.query_drop("ROLLBACK").await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))
    }

    async fn ping(&mut self) -> Result<()> {
        self.conn.ping().await
            .map_err(|e| DatabaseError::Connection(e.to_string()))
    }
}

fn convert_params(params: &[Value]) -> Params {
    let values: Vec<mysql_async::Value> = params.iter().map(|v| match v {
        Value::Int(i) => mysql_async::Value::Int(*i),
        Value::Float(f) => mysql_async::Value::Double(*f),
        Value::String(s) => mysql_async::Value::Bytes(s.as_bytes().to_vec()),
        Value::Bytes(b) => mysql_async::Value::Bytes(b.clone()),
        Value::Null => mysql_async::Value::NULL,
    }).collect();

    Params::Positional(values)
}
```

### 7.3 Driver Registry

```rust
use std::collections::HashMap;

/// Driver registry
pub struct DriverRegistry {
    drivers: HashMap<String, Arc<dyn DatabaseDriver>>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            drivers: HashMap::new(),
        };

        // Register built-in drivers
        #[cfg(feature = "mysql")]
        registry.register(Arc::new(MySqlDriver::new()));

        registry
    }

    pub fn register(&mut self, driver: Arc<dyn DatabaseDriver>) {
        self.drivers.insert(driver.name().to_string(), driver);
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn DatabaseDriver>> {
        self.drivers.get(name)
            .cloned()
            .ok_or_else(|| Error::Database(DatabaseError::DriverNotFound(name.to_string())))
    }
}
```

---

## 8. Metrics Module

**Module**: `rsbench::metrics`
**File**: `src/metrics/mod.rs`

### 8.1 Metrics Collector

```rust
use std::sync::Arc;
use dashmap::DashMap;
use hdrhistogram::Histogram;
use std::sync::atomic::{AtomicU64, Ordering};

/// Metrics collector (lock-free)
pub struct MetricsCollector {
    operation_metrics: DashMap<String, OperationMetrics>,
    backpressure_events: AtomicU64,
    start_time: Instant,
}

impl MetricsCollector {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            operation_metrics: DashMap::new(),
            backpressure_events: AtomicU64::new(0),
            start_time: Instant::now(),
        })
    }

    /// Record operation execution
    pub fn record_operation(
        &self,
        operation_name: &str,
        duration: Duration,
        result: &Result<QueryResult>,
    ) {
        let mut entry = self.operation_metrics
            .entry(operation_name.to_string())
            .or_insert_with(OperationMetrics::new);

        entry.count.fetch_add(1, Ordering::Relaxed);

        if result.is_err() {
            entry.errors.fetch_add(1, Ordering::Relaxed);
        }

        // Record latency in histogram
        let _ = entry.histogram.lock().unwrap()
            .record(duration.as_micros() as u64);
    }

    /// Record backpressure event
    pub fn record_backpressure_event(&self) {
        self.backpressure_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot of all metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        let mut operation_metrics = HashMap::new();

        for entry in self.operation_metrics.iter() {
            let key = entry.key().clone();
            let metrics = entry.value();

            operation_metrics.insert(key, OperationMetricsSnapshot {
                count: metrics.count.load(Ordering::Relaxed),
                errors: metrics.errors.load(Ordering::Relaxed),
                latency_histogram: metrics.histogram.lock().unwrap().clone(),
            });
        }

        MetricsSnapshot {
            operation_metrics,
            backpressure_events: self.backpressure_events.load(Ordering::Relaxed),
            duration: self.start_time.elapsed(),
            timestamp: SystemTime::now(),
        }
    }
}

/// Per-operation metrics (lock-free counters + mutex histogram)
struct OperationMetrics {
    count: AtomicU64,
    errors: AtomicU64,
    histogram: std::sync::Mutex<Histogram<u64>>,
}

impl OperationMetrics {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            histogram: std::sync::Mutex::new(
                Histogram::new(3).unwrap()  // 3 significant digits
            ),
        }
    }
}

/// Immutable metrics snapshot
pub struct MetricsSnapshot {
    pub operation_metrics: HashMap<String, OperationMetricsSnapshot>,
    pub backpressure_events: u64,
    pub duration: Duration,
    pub timestamp: SystemTime,
}

pub struct OperationMetricsSnapshot {
    pub count: u64,
    pub errors: u64,
    pub latency_histogram: Histogram<u64>,
}

impl OperationMetricsSnapshot {
    pub fn success_rate(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            (self.count - self.errors) as f64 / self.count as f64
        }
    }

    pub fn throughput(&self, duration: Duration) -> f64 {
        self.count as f64 / duration.as_secs_f64()
    }
}
```

### 8.2 Output Trait

```rust
/// Metrics output trait
pub trait MetricsOutput: Send + Sync {
    fn export(&mut self, snapshot: &MetricsSnapshot) -> Result<()>;
}

/// Text output (sysbench-compatible)
pub struct TextOutput {
    writer: Box<dyn std::io::Write + Send>,
}

impl TextOutput {
    pub fn new(writer: Box<dyn std::io::Write + Send>) -> Self {
        Self { writer }
    }
}

impl MetricsOutput for TextOutput {
    fn export(&mut self, snapshot: &MetricsSnapshot) -> Result<()> {
        writeln!(self.writer, "RSBench Results:")?;
        writeln!(self.writer, "Total duration: {:?}", snapshot.duration)?;
        writeln!(self.writer)?;

        for (op_name, metrics) in &snapshot.operation_metrics {
            writeln!(self.writer, "Operation: {}", op_name)?;
            writeln!(self.writer, "  Count: {}", metrics.count)?;
            writeln!(self.writer, "  Errors: {}", metrics.errors)?;
            writeln!(self.writer, "  Throughput: {:.2} ops/sec",
                metrics.throughput(snapshot.duration))?;
            writeln!(self.writer, "  Latency:")?;
            writeln!(self.writer, "    p50: {} μs", metrics.latency_histogram.value_at_quantile(0.50))?;
            writeln!(self.writer, "    p95: {} μs", metrics.latency_histogram.value_at_quantile(0.95))?;
            writeln!(self.writer, "    p99: {} μs", metrics.latency_histogram.value_at_quantile(0.99))?;
            writeln!(self.writer)?;
        }

        writeln!(self.writer, "Backpressure events: {}", snapshot.backpressure_events)?;

        Ok(())
    }
}

/// JSON output
pub struct JsonOutput {
    writer: Box<dyn std::io::Write + Send>,
}

impl JsonOutput {
    pub fn new(writer: Box<dyn std::io::Write + Send>) -> Self {
        Self { writer }
    }
}

impl MetricsOutput for JsonOutput {
    fn export(&mut self, snapshot: &MetricsSnapshot) -> Result<()> {
        use serde_json::json;

        let mut operations = serde_json::Map::new();

        for (op_name, metrics) in &snapshot.operation_metrics {
            operations.insert(op_name.clone(), json!({
                "count": metrics.count,
                "errors": metrics.errors,
                "success_rate": metrics.success_rate(),
                "throughput": metrics.throughput(snapshot.duration),
                "latency": {
                    "min": metrics.latency_histogram.min(),
                    "max": metrics.latency_histogram.max(),
                    "mean": metrics.latency_histogram.mean(),
                    "p50": metrics.latency_histogram.value_at_quantile(0.50),
                    "p95": metrics.latency_histogram.value_at_quantile(0.95),
                    "p99": metrics.latency_histogram.value_at_quantile(0.99),
                    "p999": metrics.latency_histogram.value_at_quantile(0.999),
                }
            }));
        }

        let output = json!({
            "timestamp": snapshot.timestamp,
            "duration_secs": snapshot.duration.as_secs_f64(),
            "operations": operations,
            "client_metrics": {
                "backpressure_events": snapshot.backpressure_events,
            }
        });

        serde_json::to_writer_pretty(&mut self.writer, &output)?;
        writeln!(self.writer)?;

        Ok(())
    }
}
```

---

## 9. Scenario Module

**Module**: `rsbench::scenario`
**File**: `src/scenario/mod.rs`

### 9.1 Scenario Executor

```rust
/// Scenario executor - orchestrates workload execution
pub struct ScenarioExecutor {
    config: ScenarioConfig,
    workload: Box<dyn Workload>,
    rate_limiter: RateLimiter,
    runtime: Arc<dyn RuntimeEngine>,
    metrics: Arc<MetricsCollector>,
}

impl ScenarioExecutor {
    pub fn new(
        config: ScenarioConfig,
        workload: Box<dyn Workload>,
        runtime: Arc<dyn RuntimeEngine>,
        metrics: Arc<MetricsCollector>,
    ) -> Self {
        let rate_limiter = match &config.executor {
            ExecutorConfig::ConstantRate { rate, .. } => RateLimiter::new(*rate),
            ExecutorConfig::RampingRate { stages, .. } => {
                RateLimiter::new(stages[0].target_rate)
            }
        };

        Self {
            config,
            workload,
            rate_limiter,
            runtime,
            metrics,
        }
    }

    /// Execute scenario
    pub async fn execute(&mut self) -> Result<ScenarioResult> {
        // Prepare workload
        // TODO: Create prepare context
        // self.workload.prepare(&prepare_ctx)?;

        // Execute based on executor type
        match &self.config.executor {
            ExecutorConfig::ConstantRate { rate, duration, .. } => {
                self.execute_constant_rate(*rate, *duration).await
            }
            ExecutorConfig::RampingRate { stages, .. } => {
                self.execute_ramping_rate(stages).await
            }
        }
    }

    async fn execute_constant_rate(
        &mut self,
        rate: u64,
        duration: Duration,
    ) -> Result<ScenarioResult> {
        let start = Instant::now();
        let end_time = start + duration;
        let mut iteration = 0u64;

        while Instant::now() < end_time {
            // Rate limiting (time-driven)
            self.rate_limiter.acquire().await?;

            // Generate operation
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration,
                elapsed: start.elapsed(),
            };

            let op = self.workload.next_operation(&ctx)?;

            // Submit to runtime (non-blocking)
            let runtime = self.runtime.clone();
            tokio::spawn(async move {
                let _ = runtime.submit(op).await;
            });

            iteration += 1;
        }

        // Collect results
        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: iteration,
            operations_failed: 0,  // M0: simplified
            metrics: self.metrics.snapshot(),
        })
    }

    async fn execute_ramping_rate(
        &mut self,
        stages: &[RateStage],
    ) -> Result<ScenarioResult> {
        let start = Instant::now();
        let mut iteration = 0u64;

        for stage in stages {
            self.rate_limiter.set_rate(stage.target_rate);
            let stage_end = Instant::now() + stage.duration;

            while Instant::now() < stage_end {
                self.rate_limiter.acquire().await?;

                let ctx = ExecutionContext {
                    worker_id: 0,
                    iteration,
                    elapsed: start.elapsed(),
                };

                let op = self.workload.next_operation(&ctx)?;

                let runtime = self.runtime.clone();
                tokio::spawn(async move {
                    let _ = runtime.submit(op).await;
                });

                iteration += 1;
            }
        }

        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: iteration,
            operations_failed: 0,
            metrics: self.metrics.snapshot(),
        })
    }
}

/// Scenario execution result
pub struct ScenarioResult {
    pub duration: Duration,
    pub operations_completed: u64,
    pub operations_failed: u64,
    pub metrics: MetricsSnapshot,
}
```

---

## 10. CLI Module

**Module**: `rsbench::cli`
**File**: `src/cli/mod.rs`

### 10.1 CLI Interface

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rsbench")]
#[command(about = "Modern database testing tool", long_about = None)]
pub struct Cli {
    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Database connection string
    #[arg(long)]
    pub db_url: Option<String>,

    /// Target rate (ops/sec)
    #[arg(long)]
    pub rate: Option<u64>,

    /// Test duration
    #[arg(long, value_parser = parse_duration)]
    pub duration: Option<Duration>,

    /// Number of threads (blocking mode)
    #[arg(long)]
    pub threads: Option<usize>,

    /// Output format (text|json)
    #[arg(long)]
    pub output: Option<String>,

    /// Determinism seed
    #[arg(long)]
    pub seed: Option<u64>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run a workload
    Run {
        /// Workload name or Lua script
        workload: Option<String>,
    },

    /// Prepare database (create tables, load data)
    Prepare {
        workload: String,
    },

    /// Cleanup database
    Cleanup {
        workload: String,
    },
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s)
        .map_err(|e| e.to_string())
}
```

### 10.2 Main Entry Point

```rust
// main.rs

use rsbench::*;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load configuration
    let config = if let Some(config_path) = cli.config {
        ConfigLoader::load(ConfigSource::File(config_path))?
    } else {
        // Build config from CLI args
        build_config_from_cli(&cli)?
    };

    // Validate config
    ConfigLoader::validate(&config)?;

    // Execute command
    match cli.command {
        Commands::Run { .. } => run_scenario(config).await?,
        Commands::Prepare { .. } => prepare_workload(config).await?,
        Commands::Cleanup { .. } => cleanup_workload(config).await?,
    }

    Ok(())
}

async fn run_scenario(config: ToolConfig) -> Result<()> {
    // 1. Create driver registry
    let registry = DriverRegistry::new();
    let driver = registry.get(&config.database.driver)?;

    // 2. Create connection pool
    let pool = Arc::new(ConnectionPool::new(driver, &config.database.pool)?);

    // 3. Create metrics collector
    let metrics = MetricsCollector::new();

    // 4. Create runtime
    let runtime = RuntimeFactory::create(&config.runtime, pool.clone(), metrics.clone())?;
    let runtime = Arc::from(runtime);

    // 5. Create workload
    let mut workload = WorkloadFactory::create(
        &config.scenario.workload,
        config.determinism.seed,
    )?;

    // 6. Create and execute scenario
    let mut executor = ScenarioExecutor::new(
        config.scenario,
        workload,
        runtime,
        metrics.clone(),
    );

    let result = executor.execute().await?;

    // 7. Output results
    let snapshot = result.metrics;
    let mut output: Box<dyn MetricsOutput> = match config.output.format {
        OutputFormat::Text => Box::new(TextOutput::new(Box::new(std::io::stdout()))),
        OutputFormat::Json => Box::new(JsonOutput::new(Box::new(std::io::stdout()))),
    };

    output.export(&snapshot)?;

    Ok(())
}

fn build_config_from_cli(cli: &Cli) -> Result<ToolConfig> {
    // M0: Basic implementation
    todo!("Build config from CLI args")
}

async fn prepare_workload(config: ToolConfig) -> Result<()> {
    todo!("Implement prepare")
}

async fn cleanup_workload(config: ToolConfig) -> Result<()> {
    todo!("Implement cleanup")
}
```

---

## Summary

This API specification provides:

1. **Complete type definitions** for all M0 modules
2. **Trait-based interfaces** for extensibility
3. **Error handling** patterns
4. **Configuration** structures with serde support
5. **Async-first design** with blocking mode compatibility
6. **Deterministic workload** generation with seeded RNG
7. **Backpressure-aware** runtime with visibility
8. **Lock-free metrics** collection with HDR histograms

### Next Steps

1. Create module directory structure
2. Implement each module based on these interfaces
3. Write unit tests for each module
4. Integration tests for end-to-end scenarios
5. Documentation with examples

### Implementation Priority

1. Core types and errors (`lib.rs`)
2. Config module (foundation)
3. Rate limiter (simple, no dependencies)
4. Driver module (MySQL only)
5. Connection pool (depends on driver)
6. Workload module (depends on driver for prepare)
7. Metrics module (independent)
8. Runtime module (depends on pool, metrics)
9. Scenario module (orchestrates all)
10. CLI module (entry point)
