# Scenario Module Design Document

## Document Overview

**Module**: `rsbench::scenario`
**Primary File**: `src/scenario.rs`
**Purpose**: Orchestrate workload execution with precise rate control and timing
**Status**: M0 Implementation Complete, Enhancement Roadmap Defined

This document provides a comprehensive design specification for the Scenario module, which serves as the central orchestrator for database benchmark execution in RSBench.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Core Components](#2-core-components)
3. [Executor Patterns](#3-executor-patterns)
4. [Execution Flow](#4-execution-flow)
5. [Integration Points](#5-integration-points)
6. [Configuration Schema](#6-configuration-schema)
7. [Lifecycle Management](#7-lifecycle-management)
8. [Error Handling](#8-error-handling)
9. [Performance Considerations](#9-performance-considerations)
10. [Testing Strategy](#10-testing-strategy)
11. [Future Enhancements](#11-future-enhancements)

---

## 1. Architecture Overview

### 1.1 Module Responsibility

The Scenario module is the **central orchestrator** that:
- Coordinates workload execution according to a defined execution plan
- Enforces rate limits with time-driven scheduling
- Manages the execution lifecycle (prepare → execute → cleanup)
- Collects and aggregates execution results
- Provides visibility into execution progress

### 1.2 Position in Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         CLI Layer                            │
│                      (main.rs, cli_impl.rs)                  │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ↓
┌─────────────────────────────────────────────────────────────┐
│                   Scenario Executor                          │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────────┐   │
│  │  Workload    │  │Rate Limiter  │  │  Execution      │   │
│  │  Generator   │  │              │  │  Controller     │   │
│  └──────────────┘  └──────────────┘  └─────────────────┘   │
└──────────┬─────────────────────┬────────────────────────────┘
           │                     │
           ↓                     ↓
    ┌────────────┐       ┌──────────────┐
    │  Runtime   │       │   Metrics    │
    │  Engine    │       │  Collector   │
    └────────────┘       └──────────────┘
           │
           ↓
    ┌────────────┐
    │Connection  │
    │   Pool     │
    └────────────┘
           │
           ↓
    ┌────────────┐
    │  Database  │
    │   Driver   │
    └────────────┘
```

### 1.3 Design Principles

1. **Time-Driven Execution**: Rate limiting is based on wall-clock time, not completion events
2. **Non-Blocking Submission**: Operations are submitted asynchronously without waiting for completion
3. **Backpressure Awareness**: Monitors runtime saturation and records backpressure events
4. **Deterministic Workload**: Same seed produces identical operation sequences
5. **Configurable Patterns**: Supports multiple execution patterns via executor types
6. **Observable**: Provides real-time statistics and comprehensive result metrics

---

## 2. Core Components

### 2.1 ScenarioExecutor

The main orchestrator struct that manages the entire execution lifecycle.

```rust
pub struct ScenarioExecutor {
    config: ScenarioConfig,           // Scenario configuration
    workload: Box<dyn Workload>,      // Operation generator
    rate_limiter: RateLimiter,        // Rate control mechanism
    runtime: Arc<dyn RuntimeEngine>,  // Operation execution engine
    metrics: Arc<MetricsCollector>,   // Metrics aggregation
}
```

**Responsibilities**:
- Initialize execution components
- Coordinate prepare/execute/cleanup phases
- Enforce rate limits via RateLimiter
- Generate operations via Workload
- Submit operations to Runtime (non-blocking)
- Collect final results from MetricsCollector

**Key Methods**:

```rust
impl ScenarioExecutor {
    // Create new executor
    pub fn new(
        config: ScenarioConfig,
        workload: Box<dyn Workload>,
        runtime: Arc<dyn RuntimeEngine>,
        metrics: Arc<MetricsCollector>,
    ) -> Self;

    // Execute scenario (async main loop)
    pub async fn execute(&mut self) -> Result<ScenarioResult>;

    // Internal: Constant rate execution
    async fn execute_constant_rate(
        &mut self,
        rate: u64,
        duration: Duration,
    ) -> Result<ScenarioResult>;

    // Internal: Ramping rate execution
    async fn execute_ramping_rate(
        &mut self,
        stages: &[RateStage],
    ) -> Result<ScenarioResult>;
}
```

### 2.2 ScenarioResult

Immutable result object containing execution outcomes.

```rust
pub struct ScenarioResult {
    pub duration: Duration,              // Total execution time
    pub operations_completed: u64,       // Operations submitted
    pub operations_failed: u64,          // Failed operations (M0: simplified)
    pub metrics: MetricsSnapshot,        // Complete metrics snapshot
}
```

**Properties**:
- Immutable after creation
- Contains full metrics snapshot for analysis
- Duration includes warmup/cooldown phases
- `operations_completed` counts submitted operations (not necessarily completed)

### 2.3 ExecutionContext

Context passed to workload for deterministic operation generation.

```rust
pub struct ExecutionContext {
    pub worker_id: usize,      // Worker/thread identifier
    pub iteration: u64,        // Monotonic iteration counter
    pub elapsed: Duration,     // Time since scenario start
}
```

**Usage Pattern**:
```rust
let ctx = ExecutionContext {
    worker_id: 0,           // M0: single worker, future: multiple
    iteration: 42,          // Used for deterministic RNG seeding
    elapsed: start.elapsed(), // For time-based operation patterns
};

let operation = workload.next_operation(&ctx)?;
```

### 2.4 Worker Architecture and Scalability

**What is a "Worker"?**

In RSBench, a **worker is a Tokio async task**, not an OS thread.

```rust
// Workers are async tasks, not OS threads
for worker_id in 0..workers {
    let handle = tokio::spawn(async move {  // <-- Tokio async task
        while running {
            let op = workload.next_operation(&ctx)?;
            runtime.submit(op).await;  // Async I/O, yields on await
        }
    });
    handles.push(handle);
}
```

**Tokio's M:N Threading Model:**

```
User Configuration: workers: 100
         ↓
    100 Tokio async tasks (green threads, ~2KB stack each)
         ↓
    Scheduled on N OS threads (tokio runtime workers)
         ↓
    8 actual OS threads (typical: num_cpus)
         ↓
    Non-blocking async I/O
```

**Key Characteristics:**

1. **Lightweight**: Async tasks use ~2KB stack vs ~8MB for OS threads
2. **Non-blocking I/O**: When a worker awaits database response, it yields CPU to other workers
3. **Efficient multiplexing**: 1000 workers can run on 8 OS threads
4. **No thread overhead**: No context switching between OS threads for I/O waits

**Scalability Analysis:**

| Executor Mode | Workers | OS Threads | Max Throughput | Bottleneck |
|---------------|---------|------------|----------------|------------|
| Open-loop (M0) | 1 | 4-8 | ~50K ops/sec | Rate limiter + codegen |
| Closed-loop (M0) | 100 | 4-8 | ~10K ops/sec | Query latency (10ms avg) |
| Closed-loop (M0) | 1000 | 8-16 | ~100K ops/sec | Query latency (10ms avg) |
| Open-loop (M1, multi-worker) | 8 | 8-16 | ~500K ops/sec | CPU (parallel codegen) |

**Scalability Limits:**

**Open-Loop Executor:**
- M0 (single-worker): ~50-100K ops/sec (rate limiter overhead)
- M1 (multi-worker): ~500K-1M ops/sec (parallelized submission)
- Limited by: CPU cycles for RNG + operation generation

**Closed-Loop Executor:**
- Throughput = `workers / average_latency`
- Example: 1000 workers, 10ms latency → 100K ops/sec
- Limited by: Query latency (intentional - this models user concurrency)
- Can scale by adding more workers (cheap: async tasks, not threads)

**Memory Footprint:**

```
Components:
  - Base runtime: ~10 MB
  - Per worker (async task): ~2 KB
  - Per operation type (histogram): ~1 MB
  - Connection pool: ~500 KB per connection

Examples:
  10 workers:    10MB + 20KB + metrics ≈ 15MB
  100 workers:   10MB + 200KB + metrics ≈ 20MB
  1000 workers:  10MB + 2MB + metrics ≈ 25MB
  10000 workers: 10MB + 20MB + metrics ≈ 50MB

(Compare: sysbench --threads=1000 = 1000 OS threads = ~8GB)
```

**Sysbench vs RSBench Worker Comparison:**

| Aspect | Sysbench --threads=100 | RSBench workers: 100 |
|--------|------------------------|----------------------|
| OS Threads | 100 | 4-8 (tokio runtime) |
| Memory | ~800 MB (thread stacks) | ~200 KB (async tasks) |
| Context Switches | High (100 threads) | Low (8 threads) |
| I/O Blocking | Blocking (thread waits) | Non-blocking (task yields) |
| Scalability | Limited (~500 threads) | High (~10K workers) |

**Why This Matters:**

1. **Resource Efficiency**: Test with 1000 concurrent users without 1000 OS threads
2. **True Scalability**: Add workers without linear memory/CPU cost
3. **Better Testing**: Simulate realistic concurrency levels (1000s of users)
4. **Sysbench Compatibility**: Match sysbench's user concurrency semantics with lower overhead

**Configuration Example:**

```yaml
# Closed-loop with 1000 concurrent workers (like 1000 concurrent users)
runtime:
  type: async
  workers: 8           # OS threads for tokio runtime
  max_connections: 1000

scenario:
  executor:
    type: closed-loop
    workers: 1000      # Async tasks (green threads)
    duration: 300s

# This simulates 1000 concurrent users efficiently!
```

---

## 3. Executor Patterns

### 3.1 Constant Rate Executor

Maintains a **fixed target rate** (operations per second) for the entire duration.

**Configuration**:
```yaml
executor:
  type: constant-rate
  rate: 1000          # Target: 1000 ops/sec
  duration: 5m        # Run for 5 minutes
  max_connections: 100
```

**Execution Algorithm**:
```rust
async fn execute_constant_rate(rate: u64, duration: Duration) {
    let start = Instant::now();
    let end_time = start + duration;
    let mut iteration = 0;

    while Instant::now() < end_time {
        // 1. Acquire rate limiter permit (blocks until allowed)
        rate_limiter.acquire().await?;

        // 2. Generate operation with current context
        let ctx = ExecutionContext {
            worker_id: 0,
            iteration,
            elapsed: start.elapsed(),
        };
        let op = workload.next_operation(&ctx)?;

        // 3. Submit to runtime (non-blocking, fire-and-forget)
        let runtime = runtime.clone();
        tokio::spawn(async move {
            let _ = runtime.submit(op).await;
        });

        iteration += 1;
    }

    // 4. Wait briefly for in-flight operations
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 5. Collect final metrics
    Ok(ScenarioResult { ... })
}
```

**Characteristics**:
- **Rate Control**: Token bucket with refill based on wall-clock time
- **Burst Handling**: Token bucket capacity = `2 * rate` to allow small bursts
- **Time-Driven**: Loop exits based on wall time, not operation count
- **Non-Blocking**: Operations spawned in background tasks
- **Deterministic**: Same seed → same operation sequence

**Use Cases**:
- Sustained load testing
- Throughput benchmarking
- Steady-state performance analysis
- SLA validation at fixed load

### 3.2 Ramping Rate Executor

Gradually changes the target rate through multiple **stages**.

**Configuration**:
```yaml
executor:
  type: ramping-rate
  stages:
    - duration: 30s
      target_rate: 100    # Warmup
    - duration: 60s
      target_rate: 500    # Ramp up
    - duration: 120s
      target_rate: 1000   # Peak load
    - duration: 60s
      target_rate: 500    # Ramp down
    - duration: 30s
      target_rate: 100    # Cool down
  prealloc_connections: 50
  max_connections: 200
```

**Execution Algorithm**:
```rust
async fn execute_ramping_rate(stages: &[RateStage]) {
    let start = Instant::now();
    let mut iteration = 0;

    for stage in stages {
        // Update rate limiter to new target
        rate_limiter.set_rate(stage.target_rate);
        let stage_end = Instant::now() + stage.duration;

        // Execute at this rate until stage ends
        while Instant::now() < stage_end {
            rate_limiter.acquire().await?;

            let ctx = ExecutionContext {
                worker_id: 0,
                iteration,
                elapsed: start.elapsed(),
            };
            let op = workload.next_operation(&ctx)?;

            let runtime = runtime.clone();
            tokio::spawn(async move {
                let _ = runtime.submit(op).await;
            });

            iteration += 1;
        }
    }

    // Wait for in-flight operations
    tokio::time::sleep(Duration::from_secs(1)).await;

    Ok(ScenarioResult { ... })
}
```

**Characteristics**:
- **Dynamic Rate Adjustment**: RateLimiter supports runtime rate changes
- **Smooth Transitions**: Token bucket adjusts capacity proportionally
- **Stage Isolation**: Each stage operates independently
- **Flexible Patterns**: Supports warmup, spike, sustained, cooldown patterns

**Use Cases**:
- Capacity testing (find breaking point)
- Spike testing (sudden load increases)
- Soak testing with warmup/cooldown
- Performance profiling across load ranges

### 3.3 Future Executor Patterns (Roadmap)

#### 3.3.1 Closed-Loop Executor (M1)

**Concept**: Maintain a **fixed number of concurrent users**, each executing operations in a loop.

```yaml
executor:
  type: closed-loop
  concurrent_users: 50
  duration: 5m
  think_time: 100ms  # Delay between operations
```

**Characteristics**:
- User-driven load (like JMeter thread groups)
- Natural backpressure (slow queries → lower throughput)
- Realistic for user simulation
- Sysbench compatibility (`--threads` mode)

#### 3.3.2 Arrival Rate Executor (M2)

**Concept**: Operations arrive according to a **Poisson process** (random inter-arrival times).

```yaml
executor:
  type: arrival-rate
  lambda: 1000  # Mean arrival rate (ops/sec)
  duration: 5m
  distribution: poisson  # or: uniform, exponential
```

**Characteristics**:
- More realistic traffic patterns
- Tests queue buildup under stochastic load
- Useful for latency percentile analysis

#### 3.3.3 Script-Based Executor (M3)

**Concept**: Execute a **predefined script** of operations with precise timing.

```yaml
executor:
  type: script
  script_file: scenarios/black_friday.yaml
  # Script defines exact operations and timing
```

**Use Cases**:
- Replay production workloads
- Test specific failure scenarios
- Scheduled maintenance simulations

---

## 4. Execution Flow

### 4.1 Lifecycle Phases

```
┌──────────┐
│  START   │
└────┬─────┘
     │
     ↓
┌─────────────────┐
│  1. PREPARE     │  Create tables, load data
│                 │  (via workload.prepare())
└────┬────────────┘
     │
     ↓
┌─────────────────┐
│  2. EXECUTE     │  Main benchmark loop
│                 │  - Rate limiting
│  ┌───────────┐  │  - Operation generation
│  │ Main Loop │  │  - Runtime submission
│  └───────────┘  │  - Metrics collection
└────┬────────────┘
     │
     ↓
┌─────────────────┐
│  3. COOLDOWN    │  Wait for in-flight ops
│                 │  (1 second delay)
└────┬────────────┘
     │
     ↓
┌─────────────────┐
│  4. COLLECT     │  Snapshot metrics
│                 │  Create ScenarioResult
└────┬────────────┘
     │
     ↓
┌─────────────────┐
│  5. CLEANUP     │  Optional table cleanup
│                 │  (via workload.cleanup())
└────┬────────────┘
     │
     ↓
┌──────────┐
│   END    │
└──────────┘
```

### 4.2 Main Execution Loop (Detailed)

```rust
// Pseudo-code for main loop iteration

loop {
    // Check termination condition
    if Instant::now() >= end_time {
        break;
    }

    // STEP 1: Rate Limiting (blocking)
    // Wait until token bucket allows next operation
    rate_limiter.acquire().await?;
    // ^ This enforces the target rate via time-based token refills

    // STEP 2: Context Creation
    // Provide deterministic context to workload
    let ctx = ExecutionContext {
        worker_id: 0,              // M0: single worker
        iteration: iteration_counter,  // For RNG seeding
        elapsed: start.elapsed(),   // For time-based patterns
    };

    // STEP 3: Operation Generation
    // Workload generates SQL + params deterministically
    let operation = workload.next_operation(&ctx)?;
    // ^ Same seed + same iteration → same operation

    // STEP 4: Runtime Submission (non-blocking)
    // Spawn background task to execute operation
    let runtime = runtime.clone();
    let metrics = metrics.clone();
    tokio::spawn(async move {
        // Runtime handles:
        // - Connection acquisition
        // - Query execution
        // - Metrics recording
        // - Error handling
        let _ = runtime.submit(operation).await;
    });
    // ^ Fire-and-forget, don't wait for completion

    // STEP 5: Progress Tracking
    iteration_counter += 1;

    // Optional: Backpressure Monitoring
    // (done inside runtime.submit())
}
```

### 4.3 Timing Guarantees

**Guaranteed**:
- ✅ Operations submitted at target rate (controlled by RateLimiter)
- ✅ Same seed produces same operation sequence
- ✅ Duration constraint is honored (±1ms precision)

**Not Guaranteed** (by design):
- ❌ Operations complete before next submission (fire-and-forget)
- ❌ All operations finish within duration window
- ❌ Database can sustain the target rate (backpressure may occur)

**Rationale**: Time-driven scheduling simulates real-world scenarios where clients don't wait for prior requests to complete.

---

## 5. Integration Points

### 5.1 Workload Module

**Interface**: `Workload` trait

```rust
pub trait Workload: Send + Sync {
    fn prepare(&mut self, ctx: &mut PrepareContext) -> Result<()>;
    fn next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation>;
    fn cleanup(&mut self) -> Result<()>;
    fn name(&self) -> &str;
}
```

**Scenario's Responsibilities**:
- Call `prepare()` before execution starts (M0: TODO)
- Provide `ExecutionContext` with correct iteration/elapsed values
- Call `next_operation()` in main loop to generate operations
- Call `cleanup()` after execution completes (M0: TODO)

**Workload's Responsibilities**:
- Generate deterministic operations based on context
- Use `ctx.iteration` to seed per-operation RNG
- Return `Operation` with SQL, params, and operation type
- Handle prepare/cleanup database operations

### 5.2 RateLimiter Module

**Interface**: `RateLimiter`

```rust
impl RateLimiter {
    pub fn new(rate: u64) -> Self;
    pub async fn acquire(&mut self) -> Result<Permit>;
    pub fn set_rate(&mut self, new_rate: u64);
    pub fn current_rate(&self) -> u64;
}
```

**Scenario's Usage**:
- Create limiter with initial rate from config
- Call `acquire()` before each operation submission
- For ramping executor: call `set_rate()` when stage changes

**RateLimiter's Guarantees**:
- `acquire()` blocks until token available
- Tokens refill based on wall-clock time
- Rate changes take effect immediately

### 5.3 Runtime Module

**Interface**: `RuntimeEngine` trait

```rust
#[async_trait::async_trait]
pub trait RuntimeEngine: Send + Sync {
    async fn submit(&self, op: Operation) -> Result<OperationResult>;
    fn stats(&self) -> RuntimeStats;
    async fn shutdown(&mut self) -> Result<()>;
}
```

**Scenario's Usage**:
- Spawn async task per operation: `tokio::spawn(runtime.submit(op))`
- Don't await result (fire-and-forget)
- Optional: Poll `stats()` for monitoring (M1)
- Call `shutdown()` during cleanup (M0: TODO)

**Runtime's Responsibilities**:
- Acquire connection from pool
- Execute SQL with parameters
- Record metrics (latency, errors)
- Detect and record backpressure events
- Return operation result

#### 5.3.1 Blocking Runtime Compatibility

The scenario module is **runtime-agnostic** and works with both AsyncRuntime (primary) and BlockingRuntime (legacy).

**Current Implementation** (M0):

Both runtime modes use the **same execution pattern** in scenario:
```rust
// Identical for both AsyncRuntime and BlockingRuntime
tokio::spawn(async move {
    let _ = runtime.submit(op).await;
});
```

**Key Characteristics**:
- **Fire-and-forget submission**: Scenario doesn't wait for operation completion
- **Time-driven rate control**: Rate limiter controls submission timing
- **Runtime-agnostic**: `RuntimeEngine` trait abstracts the execution model

**Runtime Behavior Differences**:

| Aspect | AsyncRuntime | BlockingRuntime |
|--------|--------------|-----------------|
| Execution Model | Tokio async tasks | OS threads via `spawn_blocking` |
| Concurrency Control | Semaphore-based | Thread pool-based |
| Backpressure Detection | Active monitoring | Simplified |
| Connection Management | Async pool operations | Blocking pool operations |
| CPU Utilization | Efficient (event-driven) | Higher (thread context switching) |

**Compatibility Notes**:

1. **Submission Pattern**: Both runtimes implement `async fn submit()`, so scenario code is identical
2. **Rate Limiting**: Works identically for both (time-driven token bucket)
3. **Metrics Collection**: Both use the same lock-free metrics collector
4. **Operation Ordering**: Deterministic workload generation is independent of runtime

**Limitations of Current BlockingRuntime** (M0):

Despite the name, BlockingRuntime doesn't provide **sysbench-style blocking semantics**:
- ❌ Not "threads executing operations sequentially"
- ✅ Just "async operations executed in blocking thread pool"

**Future Direction** (M1+):

The distinction between runtime modes may be **deprecated** in favor of executor patterns:
- **Open-Loop Executor** (current): Time-driven, fire-and-forget → Use AsyncRuntime
- **Closed-Loop Executor** (future): Worker-driven, wait-for-completion → Use AsyncRuntime with different scenario pattern

See **Section 11.1 (Closed-Loop Executor)** for planned sysbench-compatible execution model.

**Configuration Example**:

```yaml
# Async runtime (recommended)
runtime:
  type: async
  max_connections: 100
scenario:
  executor:
    type: constant-rate
    rate: 1000

# Blocking runtime (legacy, sysbench compatibility)
runtime:
  type: blocking
  threads: 16
scenario:
  executor:
    type: constant-rate
    rate: 1000
```

**Recommendation**: Use AsyncRuntime for all new workloads. BlockingRuntime may be removed in M2 once closed-loop executor is implemented.

#### 5.3.2 Backpressure Awareness Deep Dive

**What is Backpressure?**

Backpressure occurs when the **client (RSBench) is saturated**, not the database. This is a critical distinction:

- ✅ **Backpressure**: RSBench can't submit operations fast enough (pool exhausted, semaphore full)
- ❌ **NOT Backpressure**: Database is slow (high query latency)

**Why Backpressure Matters:**

Traditional tools like sysbench **hide backpressure** by blocking threads:
```
Thread blocked → Can't measure latency → Results are invalid
```

RSBench **makes backpressure visible**:
```
Pool saturated → Backpressure event recorded → User knows results are invalid
```

**How Backpressure is Detected:**

RSBench monitors two saturation points:

1. **Connection Pool Utilization**:
   ```rust
   let pool_stats = self.pool.stats();
   let utilization = pool_stats.active_connections as f64 / pool_stats.max_connections as f64;

   if utilization > BACKPRESSURE_THRESHOLD {  // Default: 0.8
       metrics.record_backpressure_event();
   }
   ```

2. **Runtime Semaphore Saturation**:
   ```rust
   // In AsyncRuntime::submit()
   if self.semaphore.available_permits() == 0 {
       metrics.record_backpressure_event();
   }
   ```

**Backpressure Metrics:**

RSBench tracks backpressure as first-class metrics:

| Metric | Type | Description |
|--------|------|-------------|
| `backpressure_events` | Counter | Total number of backpressure events detected |
| `backpressure_rate` | Gauge | Events per second |
| `backpressure_percentage` | Percentage | (backpressure_events / total_operations) × 100 |
| `pool_utilization_p99` | Percentile | 99th percentile pool utilization |

**Example Metrics Output:**

```
Test Results:
  Operations: 60000 total
  Success: 58000 (96.7%)
  Errors: 2000 (3.3%)

  Client Metrics:
    Backpressure Events: 5420    ⚠️ HIGH - Results may be invalid!
    Backpressure Rate: 90.3/sec
    Backpressure %: 9.0%
    Pool Utilization p99: 98.2%  ⚠️ Client saturated

  Latency Metrics:
    p50: 5ms      ⚠️ Don't trust these when backpressure > 1%
    p99: 120ms
    p999: 450ms
```

**Interpreting Backpressure:**

| Backpressure % | Interpretation | Action |
|----------------|----------------|--------|
| 0% - 1% | ✅ No backpressure | Results are valid, database performance is being measured |
| 1% - 5% | ⚠️ Minor backpressure | Results may be slightly skewed, consider increasing max_connections |
| 5% - 20% | ❌ Moderate backpressure | Results are invalid, you're measuring client limits, not database |
| > 20% | ❌ Severe backpressure | Test is completely invalid, significantly increase max_connections |

**Configuration:**

```yaml
runtime:
  type: async
  max_connections: 100          # Increase if backpressure detected
  backpressure_threshold: 0.8   # Alert when pool is 80% utilized

scenario:
  executor:
    type: constant-rate
    rate: 1000                  # Reduce if backpressure occurs
    duration: 60s
```

**Best Practices:**

1. **Always check backpressure metrics** after tests
2. **Increase max_connections** if backpressure > 1%
3. **Don't trust latency percentiles** when backpressure is high
4. **Use backpressure as a signal** to tune client capacity
5. **Distinguish client limits from database limits** - this is RSBench's superpower

**Comparison with Sysbench:**

| Aspect | Sysbench | RSBench |
|--------|----------|---------|
| Backpressure visibility | ❌ Hidden (threads block) | ✅ Explicit metric |
| Client saturation detection | ❌ No detection | ✅ Automatic detection |
| Invalid result warning | ❌ No warning | ✅ Clear indication when backpressure > threshold |
| Tuning guidance | ❌ Trial and error | ✅ Backpressure % guides max_connections tuning |

**Example: Detecting and Fixing Backpressure**

```bash
# Run 1: Initial test
rsbench --scenario scenarios/load_test.yaml

# Output shows:
#   Backpressure Events: 8200 (13.7%)
#   Pool Utilization p99: 99.8%
# ⚠️ Results are INVALID - measuring client, not database

# Run 2: Increase connections
# Edit config.yaml: max_connections: 100 → 500

rsbench --scenario scenarios/load_test.yaml

# Output shows:
#   Backpressure Events: 12 (0.02%)
#   Pool Utilization p99: 45.2%
# ✅ Results are VALID - now measuring database performance
```

**Advanced: Backpressure Event Correlation**

In M1+, backpressure events will be timestamped for correlation with external events:

```
Timeline:
  0s - 30s:  Backpressure: 0%     (normal)
  30s - 32s: Backpressure: 45%    (spike during failover)
  32s - 60s: Backpressure: 0%     (recovered)
```

This allows identifying whether latency spikes are database issues or client saturation.

### 5.4 Metrics Module

**Interface**: `MetricsCollector`

```rust
impl MetricsCollector {
    pub fn new() -> Arc<Self>;
    pub fn record_operation(&self, name: &str, duration: Duration, result: &Result<QueryResult>);
    pub fn record_backpressure_event(&self);
    pub fn snapshot(&self) -> MetricsSnapshot;
}
```

**Scenario's Usage**:
- Pass `Arc<MetricsCollector>` to runtime
- Call `snapshot()` after execution completes
- Include snapshot in `ScenarioResult`

**MetricsCollector's Responsibilities**:
- Lock-free operation recording (called from many async tasks)
- Maintain per-operation histograms
- Count errors and backpressure events
- Produce consistent snapshots

### 5.5 Configuration Module

**Interface**: `ScenarioConfig`

```rust
pub struct ScenarioConfig {
    pub executor: ExecutorConfig,
    pub workload: WorkloadConfig,
}

pub enum ExecutorConfig {
    ConstantRate { rate: u64, duration: Duration, max_connections: usize },
    RampingRate { stages: Vec<RateStage>, prealloc_connections: usize, max_connections: usize },
}
```

**Scenario's Usage**:
- Receive config in constructor
- Dispatch to appropriate executor method based on type
- Extract rate, duration, stages as needed

### 5.6 Event Module Integration

**Purpose**: The Event Module is a **parallel module** that enables external event-driven test orchestration, allowing RSBench to respond to real-world events during test execution.

**Module Position in Architecture**:

```
┌─────────────────────────────────────────────────────────────────┐
│                     RSBench Architecture                         │
│                                                                  │
│  ┌──────────────┐        ┌──────────────┐                      │
│  │   Scenario   │        │    Event     │  ← PARALLEL MODULES  │
│  │   Module     │◄───────│   Module     │                      │
│  └──────┬───────┘  mpsc  └──────┬───────┘                      │
│         │         channel        │                              │
│         │                        │                              │
│         │                        ├─► K8s Watcher               │
│         │                        ├─► Webhook Listener          │
│         │                        └─► Timer Events              │
│         │                                                       │
│         ├──► Runtime ──► Pool ──► Driver ──► Database          │
│         ├──► Workload                                          │
│         └──► Metrics                                           │
│                                                                  │
│  Event Module feeds events to Scenario via async channel       │
│  Scenario reacts to events (phase changes, rate adjustments)   │
└─────────────────────────────────────────────────────────────────┘
```

**Key Design Principle**: Event Module is **independent and parallel** - it runs alongside the Scenario Module, not inside it.

---

#### 5.6.1 Integration Model

**Communication Channel**: Event Module → Scenario Module via `tokio::mpsc` channel

```rust
// Event Module produces events
pub struct EventModule {
    event_tx: mpsc::Sender<Event>,  // Send events to scenario
    watchers: Vec<Box<dyn EventWatcher>>,
}

// Scenario Module consumes events
pub struct ScenarioExecutor {
    event_rx: Option<mpsc::Receiver<Event>>,  // Receive events from event module
    // ... other fields
}
```

**Integration Pattern**:

```rust
// In main.rs or orchestration layer
async fn run_scenario_with_events(config: Config) -> Result<ScenarioResult> {
    // 1. Create event channel
    let (event_tx, event_rx) = mpsc::channel(100);

    // 2. Start Event Module (parallel task)
    let event_module = EventModule::new(config.events, event_tx);
    let event_handle = tokio::spawn(async move {
        event_module.run().await
    });

    // 3. Create Scenario with event receiver
    let mut scenario = ScenarioExecutor::new(
        config.scenario,
        workload,
        runtime,
        metrics,
    );
    scenario.attach_event_stream(event_rx);  // Attach event channel

    // 4. Run scenario (will react to events)
    let result = scenario.execute().await?;

    // 5. Shutdown event module
    event_handle.abort();

    Ok(result)
}
```

**Why Parallel, Not Nested**:
- ✅ **Separation of Concerns**: Event watching is independent from workload execution
- ✅ **Reusability**: Same Event Module works with any Scenario executor
- ✅ **Composability**: Can run Scenario without Event Module (standalone mode)
- ✅ **Testability**: Can test Event Module and Scenario Module independently
- ❌ **Not Nested**: Event Module is not a component inside Scenario Module

---

#### 5.6.2 How Scenario Reacts to Events

**Event-Driven Execution Loop**:

```rust
impl ScenarioExecutor {
    pub async fn execute(&mut self) -> Result<ScenarioResult> {
        let start = Instant::now();
        let mut iteration = 0;

        loop {
            // Check for external events (non-blocking)
            if let Some(event_rx) = &mut self.event_rx {
                if let Ok(event) = event_rx.try_recv() {
                    self.handle_event(event).await?;  // React to event
                }
            }

            // Normal execution flow
            if start.elapsed() >= self.duration {
                break;
            }

            self.rate_limiter.acquire().await?;
            let ctx = ExecutionContext { iteration, elapsed: start.elapsed(), worker_id: 0 };
            let op = self.workload.next_operation(&ctx)?;

            tokio::spawn(async move {
                let _ = runtime.submit(op).await;
            });

            iteration += 1;
        }

        Ok(self.create_result(start.elapsed()))
    }

    async fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::RateChange(new_rate) => {
                // Dynamically adjust rate limiter
                self.rate_limiter.set_rate(new_rate);
                tracing::info!("Rate changed to {} ops/sec due to event", new_rate);
            }
            Event::PhaseTransition(phase) => {
                // Change execution phase
                match phase {
                    Phase::Pause => self.paused = true,
                    Phase::Resume => self.paused = false,
                    Phase::Shutdown => return Err(Error::GracefulShutdown),
                }
            }
            Event::MetricsSnapshot => {
                // Take intermediate snapshot
                let snapshot = self.metrics.snapshot();
                // Optionally export or log
            }
            Event::Custom(data) => {
                // User-defined event handling
                self.handle_custom_event(data)?;
            }
        }
        Ok(())
    }
}
```

**Event Types**:

```rust
pub enum Event {
    // Rate control events
    RateChange(u64),                    // Change ops/sec dynamically

    // Phase control events
    PhaseTransition(Phase),             // Pause, Resume, Shutdown

    // Metrics events
    MetricsSnapshot,                    // Take intermediate snapshot

    // Routing events (distributed mode)
    RoutingChange(RoutingStrategy),     // Change endpoint routing

    // Custom events
    Custom(serde_json::Value),          // User-defined events

    // Infrastructure events (M1+)
    K8sEvent {
        namespace: String,
        resource: String,
        event_type: String,              // pod.delete, deployment.update, etc.
        timestamp: Instant,
    },
}

pub enum Phase {
    Pause,      // Stop submitting operations
    Resume,     // Continue submitting operations
    Shutdown,   // Graceful shutdown
}
```

---

#### 5.6.3 Event Sources

**1. K8s Event Watcher** (M1+):

```rust
pub struct K8sEventWatcher {
    client: kube::Client,
    config: K8sWatchConfig,
    event_tx: mpsc::Sender<Event>,
}

impl K8sEventWatcher {
    pub async fn watch(&self) -> Result<()> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.config.namespace);

        let mut watcher = watcher(pods, Default::default()).boxed();

        while let Some(event) = watcher.next().await {
            match event? {
                WatchEvent::Deleted(pod) => {
                    // Pod deleted - potential failover
                    self.event_tx.send(Event::K8sEvent {
                        namespace: self.config.namespace.clone(),
                        resource: format!("pod/{}", pod.metadata.name.unwrap()),
                        event_type: "delete".to_string(),
                        timestamp: Instant::now(),
                    }).await?;
                }
                WatchEvent::Modified(pod) => {
                    // Pod updated - potential rolling upgrade
                    // ... send event
                }
                _ => {}
            }
        }

        Ok(())
    }
}
```

**Configuration**:

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 600s

  events:
    - source: k8s
      namespace: tidb-cluster
      resources:
        - pods
        - deployments
      watch_events:
        - delete      # Failover
        - update      # Rolling upgrade
      actions:
        - event_type: pod.delete
          action:
            type: observe      # Don't change rate, just record timestamp
        - event_type: deployment.update
          action:
            type: rate_change
            rate: 500          # Reduce load during upgrade
```

**2. Webhook Listener** (M1+):

```rust
pub struct WebhookEventListener {
    port: u16,
    event_tx: mpsc::Sender<Event>,
}

impl WebhookEventListener {
    pub async fn listen(&self) -> Result<()> {
        let app = Router::new()
            .route("/events", post(handle_webhook))
            .with_state(self.event_tx.clone());

        axum::Server::bind(&format!("0.0.0.0:{}", self.port).parse()?)
            .serve(app.into_make_service())
            .await?;

        Ok(())
    }
}

async fn handle_webhook(
    State(tx): State<mpsc::Sender<Event>>,
    Json(payload): Json<WebhookPayload>,
) -> StatusCode {
    let event = Event::Custom(payload.data);
    tx.send(event).await.ok();
    StatusCode::OK
}
```

**Configuration**:

```yaml
scenario:
  events:
    - source: webhook
      port: 8090
      path: /events
      actions:
        - filter: '$.event_type == "chaos_experiment"'
          action:
            type: rate_change
            rate: 2000   # Increase load during chaos experiment
```

**3. Timer Events** (M0):

```rust
pub struct TimerEventSource {
    schedule: Vec<ScheduledEvent>,
    event_tx: mpsc::Sender<Event>,
}

#[derive(Debug)]
pub struct ScheduledEvent {
    pub trigger_at: Duration,   // Offset from test start
    pub event: Event,
}

impl TimerEventSource {
    pub async fn run(&self, start_time: Instant) -> Result<()> {
        for scheduled in &self.schedule {
            let sleep_duration = scheduled.trigger_at.saturating_sub(start_time.elapsed());
            tokio::time::sleep(sleep_duration).await;
            self.event_tx.send(scheduled.event.clone()).await?;
        }
        Ok(())
    }
}
```

**Configuration**:

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 600s

  events:
    - source: timer
      schedule:
        - at: 120s
          action:
            type: rate_change
            rate: 2000       # Ramp up at 2 minutes

        - at: 480s
          action:
            type: rate_change
            rate: 500        # Ramp down at 8 minutes
```

---

#### 5.6.4 Use Cases

**Use Case 1: Failover Testing**

Monitor K8s pod deletions and correlate with latency spikes:

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 300s

  events:
    - source: k8s
      namespace: tidb-cluster
      watch:
        - resource: pods
          event_type: delete
      action:
        type: observe    # Just record timestamp, don't change rate
```

**Result**:
```
Timeline:
  0-60s:     Normal (p99: 5ms, no events)
  60s:       K8s Event: pod/tidb-0 deleted
  60-75s:    Latency spike (p99: 2000ms)  ← Failover in progress
  75-300s:   Recovered (p99: 8ms)

Correlation: Latency spike directly caused by pod deletion (failover)
```

**Use Case 2: Rolling Upgrade Testing**

Reduce load during rolling upgrades:

```yaml
scenario:
  events:
    - source: k8s
      watch:
        - resource: deployments
          event_type: update
      action:
        type: rate_change
        rate: 500       # Reduce to 50% during upgrade

    - source: timer
      schedule:
        - at: 180s
          action:
            type: rate_change
            rate: 1000   # Resume normal load after 3 minutes
```

**Use Case 3: Chaos Engineering Integration**

Receive events from Chaos Mesh or Litmus Chaos:

```yaml
scenario:
  events:
    - source: webhook
      port: 8090
      actions:
        - filter: '$.chaos_type == "network_partition"'
          action:
            type: observe

        - filter: '$.chaos_type == "cpu_stress"'
          action:
            type: rate_change
            rate: 2000   # Increase load during CPU stress
```

**Use Case 4: Time-Based Load Patterns**

Simulate daily traffic patterns:

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 100           # Start low (night)
    duration: 3600s     # 1 hour test

  events:
    - source: timer
      schedule:
        - at: 900s      # 15 min: morning ramp
          action: { type: rate_change, rate: 1000 }

        - at: 1800s     # 30 min: peak hours
          action: { type: rate_change, rate: 5000 }

        - at: 2700s     # 45 min: evening ramp down
          action: { type: rate_change, rate: 500 }
```

---

#### 5.6.5 Benefits of Parallel Event Module

**1. Clean Separation of Concerns**:
- Scenario Module: Workload execution logic
- Event Module: External event monitoring
- No tight coupling between modules

**2. Composability**:
```rust
// Scenario can run standalone
let result = scenario.execute().await?;

// Or with events attached
scenario.attach_event_stream(event_rx);
let result = scenario.execute().await?;
```

**3. Extensibility**:
```rust
// Easy to add new event sources
impl EventWatcher for PrometheusAlertWatcher {
    async fn watch(&self, tx: mpsc::Sender<Event>) -> Result<()> {
        // Watch Prometheus alerts
    }
}
```

**4. Testability**:
```rust
#[tokio::test]
async fn test_scenario_reacts_to_rate_change_event() {
    let (tx, rx) = mpsc::channel(10);
    let mut scenario = create_test_scenario(rx);

    // Inject event
    tx.send(Event::RateChange(2000)).await.unwrap();

    // Verify scenario adjusted rate
    scenario.step().await.unwrap();
    assert_eq!(scenario.rate_limiter.current_rate(), 2000);
}
```

**5. Distributed Mode Compatibility**:
- In distributed mode, **only leader** receives events
- Leader broadcasts phase changes to workers via gRPC
- Workers don't need direct K8s access

---

#### 5.6.6 Implementation Status

**M0** (Current):
- ✅ Timer event source (simple, no external dependencies)
- ✅ Event channel integration in Scenario Module
- ✅ Basic event handling (rate change, phase transition)

**M1** (Future):
- ⏭️ K8s event watcher
- ⏭️ Webhook listener
- ⏭️ Event correlation in metrics output
- ⏭️ Distributed mode event broadcasting (leader → workers)

**M2+** (Advanced):
- ⏭️ Prometheus alert integration
- ⏭️ CloudWatch event integration
- ⏭️ Custom event filters (CEL expressions)
- ⏭️ Event replay for reproducible testing

---

## 6. Configuration Schema

### 6.1 Complete Schema

```yaml
scenario:
  # Executor configuration (required)
  executor:
    type: constant-rate | ramping-rate

    # ========== Constant Rate Fields ==========
    rate: <u64>                 # Operations per second
    duration: <duration>        # e.g., 30s, 5m, 1h
    max_connections: <usize>    # Default: 100

    # ========== Ramping Rate Fields ==========
    stages:                     # List of rate stages
      - duration: <duration>
        target_rate: <u64>
      - duration: <duration>
        target_rate: <u64>
    prealloc_connections: <usize>  # Default: 50
    max_connections: <usize>       # Default: 100

  # Workload configuration (required)
  workload:
    type: declarative | lua
    file: <path>                # For declarative/lua
    # ... (see workload module docs)
```

### 6.2 Validation Rules

**General**:
- `executor` and `workload` are **required**
- `executor.type` must be valid enum value
- Durations must be parseable by `humantime`

**Constant Rate**:
- `rate` must be > 0
- `duration` must be > 0
- `max_connections` must be > 0

**Ramping Rate**:
- `stages` must have at least 1 entry
- Each `stage.duration` must be > 0
- Each `stage.target_rate` must be >= 0 (allows 0 for pauses)
- `prealloc_connections` ≤ `max_connections`

### 6.3 Default Values

```rust
fn default_max_connections() -> usize { 100 }
fn default_prealloc_connections() -> usize { 50 }
```

### 6.4 Example Configurations

#### Basic Constant Rate
```yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml
```

#### Advanced Ramping Load
```yaml
scenario:
  executor:
    type: ramping-rate
    stages:
      # Phase 1: Warmup
      - duration: 30s
        target_rate: 100
      # Phase 2: Gradual ramp
      - duration: 60s
        target_rate: 500
      # Phase 3: Peak load
      - duration: 120s
        target_rate: 2000
      # Phase 4: Spike test
      - duration: 30s
        target_rate: 5000
      # Phase 5: Recovery
      - duration: 60s
        target_rate: 1000
      # Phase 6: Cooldown
      - duration: 30s
        target_rate: 100
    prealloc_connections: 100
    max_connections: 500
  workload:
    type: declarative
    file: workloads/oltp_write_heavy.yaml
```

#### Capacity Testing
```yaml
scenario:
  executor:
    type: ramping-rate
    stages:
      - { duration: 60s, target_rate: 1000 }
      - { duration: 60s, target_rate: 2000 }
      - { duration: 60s, target_rate: 4000 }
      - { duration: 60s, target_rate: 8000 }
      - { duration: 60s, target_rate: 16000 }
    max_connections: 1000
  workload:
    type: declarative
    file: workloads/oltp_point_select.yaml
```

---

## 7. Lifecycle Management

### 7.1 Initialization

**Constructor Pattern**:
```rust
let executor = ScenarioExecutor::new(
    config,        // ScenarioConfig
    workload,      // Box<dyn Workload>
    runtime,       // Arc<dyn RuntimeEngine>
    metrics,       // Arc<MetricsCollector>
);
```

**Initialization Steps**:
1. Store config and components
2. Create RateLimiter with initial rate from config
3. Ready to execute (no expensive operations here)

### 7.2 Prepare Phase (M0: TODO)

**Purpose**: Set up database state (create tables, load data)

**Planned Implementation**:
```rust
pub async fn execute(&mut self) -> Result<ScenarioResult> {
    // 1. Prepare workload
    let prepare_ctx = PrepareContext {
        database: &mut prepare_connection,  // Special connection
        seed: self.config.seed,
        worker_count: 1,  // M0: single worker
    };
    self.workload.prepare(&mut prepare_ctx)?;

    // 2. Execute benchmark
    match &self.config.executor {
        // ...
    }
}
```

**Considerations**:
- Requires dedicated database connection (not from pool)
- Should be idempotent (safe to run multiple times)
- May take significant time (bulk data loading)
- Errors should fail fast (don't start execution)

### 7.3 Execute Phase

**Current Implementation** (M0):
- Time-driven loop
- Fire-and-forget operation submission
- No prepare/cleanup calls yet

**Execution Guarantee**: Loop exits when `Instant::now() >= end_time`

### 7.4 Cooldown Phase

**Purpose**: Allow in-flight operations to complete before collecting metrics

**Current Implementation**:
```rust
// After main loop exits
tokio::time::sleep(Duration::from_secs(1)).await;
```

**Rationale**:
- Operations submitted near end may still be executing
- 1 second allows most operations to complete
- Trade-off: too short → incomplete metrics, too long → unnecessary delay

**Future Enhancement** (M1):
- Track in-flight operation count
- Wait until count reaches 0 (with timeout)
- More accurate metrics collection

### 7.5 Cleanup Phase (M0: TODO)

**Purpose**: Remove test data (optional, configurable)

**Planned Implementation**:
```rust
pub async fn execute(&mut self) -> Result<ScenarioResult> {
    // ... execution ...

    // Cleanup
    if self.config.cleanup_enabled {
        self.workload.cleanup()?;
    }

    Ok(result)
}
```

**Configuration**:
```yaml
scenario:
  cleanup_enabled: false  # Default: false (preserve data)
```

### 7.6 Shutdown

**Purpose**: Gracefully stop execution (for Ctrl+C, etc.)

**Planned Implementation** (M1):
```rust
impl ScenarioExecutor {
    pub async fn shutdown(&mut self) -> Result<()> {
        // 1. Stop accepting new operations
        // 2. Wait for in-flight operations (with timeout)
        self.runtime.shutdown().await?;
        // 3. Cleanup workload if configured
        if self.config.cleanup_enabled {
            self.workload.cleanup()?;
        }
        Ok(())
    }
}
```

---

## 8. Error Handling

### 8.1 Error Types

**Scenario-Specific Errors** (via `RuntimeError`):
```rust
pub enum RuntimeError {
    PoolExhausted,                  // Connection pool saturated
    Timeout(Duration),              // Operation timeout
    ConnectionFailed(String),       // Cannot establish connection
    BackpressureSaturation,         // Chronic backpressure
}
```

**Propagated Errors**:
- `WorkloadError`: From workload.next_operation()
- `DatabaseError`: From runtime.submit() (indirectly)
- `ConfigError`: From invalid configuration

### 8.2 Error Handling Strategy

**During Execution**:
```rust
// Operation generation errors → fail fast
let op = self.workload.next_operation(&ctx)?;  // Propagate error

// Runtime submission errors → log and continue
tokio::spawn(async move {
    if let Err(e) = runtime.submit(op).await {
        // Error is recorded in metrics
        // Don't crash the benchmark
        tracing::warn!("Operation failed: {}", e);
    }
});
```

**Rationale**:
- **Workload errors**: Indicate configuration or logic bugs → fail fast
- **Runtime errors**: Expected (network issues, query errors) → record in metrics
- Benchmark continues to collect data even under partial failure

### 8.3 Failure Modes

| Failure | Behavior | Metrics Impact |
|---------|----------|----------------|
| Workload generation fails | Execution aborts | None (early exit) |
| RateLimiter error | Execution aborts | None (early exit) |
| Runtime submit fails | Logged, execution continues | Error counter incremented |
| Database unreachable | Submit fails, metrics record errors | High error rate visible |
| Pool exhausted | Backpressure events recorded | Backpressure counter incremented |

---

## 9. Performance Considerations

### 9.1 Throughput Optimization

**Current Design**:
- **Non-blocking submission**: `tokio::spawn` allows parallel execution
- **Lock-free metrics**: `DashMap` and `AtomicU64` for concurrent recording
- **Efficient rate limiting**: Token bucket with minimal overhead

**Bottlenecks** (profiled):
1. **RateLimiter.acquire()**: Dominates CPU at high rates (>50K ops/sec)
2. **Workload.next_operation()**: RNG + parameter generation overhead
3. **tokio::spawn()**: Task creation cost at extreme rates

**Optimization Strategies** (M1):
- **Batch submission**: Submit multiple operations per acquire() call
- **Pre-generated operations**: Cache operation sequences
- **Lock-free queue**: Replace tokio::spawn with manual queue

### 9.2 Memory Management

**Current Allocations**:
- `Operation` struct per iteration (transient, dropped after submit)
- Metrics storage grows with unique operation names
- Histogram memory for each operation type

**Memory Profile** (typical 5-minute test):
- Base: ~10 MB (runtime, pools, buffers)
- Per operation type: ~1 MB (histogram)
- Typical total: 20-50 MB

**Future Optimization**:
- Object pooling for `Operation` structs
- Streaming metrics export (don't accumulate all in memory)

### 9.3 Latency Optimization

**Critical Path** (per operation):
```
acquire() → next_operation() → spawn() → (background: submit())
   ↓            ↓                 ↓
 ~10μs        ~5μs             ~2μs       (measured on M1 Mac)
```

**Total submission latency**: ~17μs per operation

**Implication**: Theoretical max rate from single thread ≈ 58K ops/sec

**Scaling Strategy**:
- M0: Single-threaded scenario executor (sufficient for most use cases)
- M1: Multi-threaded executor for extreme rates (>100K ops/sec)

---

## 10. Testing Strategy

### 10.1 Unit Tests

**Test Coverage** (current in `src/scenario.rs`):

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_scenario_result_creation() {
        // Verify ScenarioResult construction
    }

    #[test]
    fn test_scenario_result_success_rate() {
        // Verify success rate calculation
    }
}
```

**Additional Unit Tests** (TODO):
- Executor selection based on config type
- RateLimiter initialization with correct rate
- Context generation with correct iteration/elapsed values
- Error propagation from workload

**Test Pattern**:
```rust
#[test]
fn test_executor_dispatches_to_constant_rate() {
    let config = ScenarioConfig {
        executor: ExecutorConfig::ConstantRate {
            rate: 100,
            duration: Duration::from_secs(1),
            max_connections: 10,
        },
        workload: /* mock */,
    };

    let executor = ScenarioExecutor::new(config, mock_workload(), mock_runtime(), mock_metrics());
    // Verify rate_limiter.current_rate() == 100
}
```

### 10.2 Integration Tests

**Test Scenarios**:

1. **End-to-End Constant Rate**:
   ```rust
   #[tokio::test]
   async fn test_constant_rate_execution() {
       // Setup: mock runtime, mock workload
       // Execute: 10 ops at 10 ops/sec for 1 second
       // Verify: 10 operations submitted, duration ~1s
   }
   ```

2. **End-to-End Ramping Rate**:
   ```rust
   #[tokio::test]
   async fn test_ramping_rate_execution() {
       // Setup: 2 stages (100 ops/s, 200 ops/s)
       // Execute: 30s total
       // Verify: rate changes detected, correct op count per stage
   }
   ```

3. **Backpressure Detection**:
   ```rust
   #[tokio::test]
   async fn test_backpressure_recording() {
       // Setup: saturated pool (slow mock runtime)
       // Execute: high rate
       // Verify: backpressure events > 0
   }
   ```

### 10.3 Property-Based Tests

**Invariants to Test**:

1. **Determinism**:
   ```rust
   #[proptest]
   fn test_deterministic_execution(seed: u64, rate: u64, duration_secs: u64) {
       // Run scenario twice with same seed
       // Verify: identical operation sequences
   }
   ```

2. **Rate Accuracy**:
   ```rust
   #[proptest]
   fn test_rate_accuracy(target_rate: u64 in 10..10000) {
       // Execute for 10 seconds at target_rate
       // Verify: actual_rate within ±5% of target_rate
   }
   ```

3. **Duration Accuracy**:
   ```rust
   #[proptest]
   fn test_duration_accuracy(duration: Duration in 1..60s) {
       // Execute for specified duration
       // Verify: result.duration within ±100ms of target
   }
   ```

### 10.4 Performance Benchmarks

**Benchmark Scenarios** (in `benches/scenario_bench.rs`):

```rust
fn bench_operation_submission_rate(c: &mut Criterion) {
    c.bench_function("submit 1000 ops", |b| {
        b.iter(|| {
            // Measure time to submit 1000 operations
            // (using mock runtime to isolate scenario overhead)
        });
    });
}

fn bench_rate_limiter_overhead(c: &mut Criterion) {
    c.bench_function("rate limiter acquire", |b| {
        let mut limiter = RateLimiter::new(10000);
        b.iter(|| {
            // Measure acquire() latency
            futures::executor::block_on(limiter.acquire())
        });
    });
}
```

---

## 11. Future Enhancements

### 11.1 Closed-Loop Executor (M1) - BlockingRuntime Replacement

**Motivation**: Provide sysbench-style execution semantics without needing a separate BlockingRuntime

**Problem with Current BlockingRuntime**:
The current BlockingRuntime doesn't actually provide sysbench's blocking semantics:
- Sysbench `--threads=N`: N workers, each executes operations **sequentially** (wait for completion)
- RSBench BlockingRuntime: Just wraps async operations in `spawn_blocking` threads
- Both use **fire-and-forget** submission from scenario → no semantic difference

**Better Design - Closed-Loop Executor**:

Instead of runtime mode, implement as an **executor pattern** in the scenario module:

```rust
pub enum ExecutorConfig {
    ConstantRate { /* ... */ },      // Open-loop (current)
    RampingRate { /* ... */ },       // Open-loop (current)
    ClosedLoop {                     // NEW: Sysbench-style
        workers: usize,              // Number of worker threads
        duration: Duration,
        think_time: Option<Duration>, // Delay between operations
    },
}
```

**Implementation**:

```rust
async fn execute_closed_loop(
    &mut self,
    workers: usize,
    duration: Duration,
    think_time: Option<Duration>,
) -> Result<ScenarioResult> {
    let start = Instant::now();
    let end_time = start + duration;

    // Spawn N worker tasks (each executes sequentially)
    let mut handles = vec![];

    for worker_id in 0..workers {
        let runtime = self.runtime.clone();
        let workload = self.create_worker_workload(worker_id)?;
        let think_time = think_time;

        let handle = tokio::spawn(async move {
            let mut iteration = 0u64;

            while Instant::now() < end_time {
                // Generate operation
                let ctx = ExecutionContext {
                    worker_id,
                    iteration,
                    elapsed: start.elapsed(),
                };
                let op = workload.next_operation(&ctx)?;

                // KEY: Wait for completion (closed-loop)
                let _ = runtime.submit(op).await;

                // Optional think time
                if let Some(delay) = think_time {
                    tokio::time::sleep(delay).await;
                }

                iteration += 1;
            }

            Ok::<u64, Error>(iteration)
        });

        handles.push(handle);
    }

    // Wait for all workers
    let mut total_ops = 0;
    for handle in handles {
        total_ops += handle.await??;
    }

    Ok(ScenarioResult {
        duration: start.elapsed(),
        operations_completed: total_ops,
        operations_failed: 0,
        metrics: self.metrics.snapshot(),
    })
}
```

**Key Differences from Open-Loop**:

| Aspect | Open-Loop (current) | Closed-Loop (new) |
|--------|---------------------|-------------------|
| Submission | Fire-and-forget | Wait for completion |
| Rate Control | Explicit (RateLimiter) | Implicit (from latency) |
| Backpressure | Recorded as event | Natural (slows throughput) |
| Concurrency | Unbounded (up to pool limit) | Fixed (N workers) |
| Semantics | Load generator | User simulator |
| Sysbench Equivalent | `--rate=X` mode | `--threads=N` mode |

**Configuration Example**:

```yaml
# Open-loop: Time-driven load generation
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 5m

# Closed-loop: Worker-driven execution (sysbench style)
scenario:
  executor:
    type: closed-loop
    workers: 16              # Like sysbench --threads=16
    duration: 5m
    think_time: 10ms         # Optional delay between ops
```

**Benefits**:

1. **Simpler Architecture**:
   - Remove BlockingRuntime entirely
   - Single runtime (AsyncRuntime) for all cases
   - Execution pattern controlled by executor, not runtime

2. **True Sysbench Compatibility**:
   - Matches sysbench's `--threads` semantics exactly
   - Natural backpressure (slow queries → lower throughput)
   - Realistic user simulation

3. **Better Separation of Concerns**:
   - Runtime = "how to execute operations" (always async)
   - Executor = "when/how to submit operations" (open vs closed loop)

4. **More Flexible**:
   - Can add rate limiting to closed-loop if desired
   - Can combine with think time for realistic user simulation
   - Easier to add other executor patterns

**Migration Path**:

1. M1: Implement `ClosedLoop` executor
2. Deprecate BlockingRuntime, update docs
3. M2: Remove BlockingRuntime code entirely

**Testing**:

```rust
#[tokio::test]
async fn test_closed_loop_execution() {
    // Setup: 4 workers, 2 seconds
    let executor = ScenarioExecutor::new(
        ExecutorConfig::ClosedLoop {
            workers: 4,
            duration: Duration::from_secs(2),
            think_time: None,
        },
        /* ... */
    );

    let result = executor.execute().await?;

    // Verify: Operations completed naturally (no explicit rate)
    // Throughput determined by query latency
    assert!(result.operations_completed > 0);
}
```

### 11.2 Multi-Worker Open-Loop Execution (M1)

**Motivation**: Scale open-loop execution to >100K ops/sec

**Design**:
```rust
pub struct ScenarioExecutor {
    workers: Vec<WorkerHandle>,  // Multiple execution threads
    // Each worker has its own:
    // - RateLimiter (with rate / worker_count)
    // - Workload instance (seeded differently)
    // - ExecutionContext (with unique worker_id)
}
```

**Benefits**:
- Linear scaling with CPU cores
- Higher sustained rates
- Better CPU utilization

**Challenges**:
- Rate distribution across workers
- Workload determinism with multiple workers
- Synchronization overhead

### 11.3 Dynamic Rate Adjustment (M2)

**Motivation**: React to runtime conditions (latency thresholds, error rates)

**Design**:
```yaml
executor:
  type: adaptive-rate
  initial_rate: 1000
  duration: 5m
  adaptation:
    metric: p99_latency
    target: 100ms
    adjust_interval: 10s
    step_size: 100  # ops/sec
```

**Algorithm**:
```rust
loop {
    // Measure current metric
    let p99 = metrics.snapshot().p99_latency();

    // Adjust rate based on target
    if p99 > target {
        rate_limiter.set_rate(current_rate - step_size);  // Decrease load
    } else if p99 < target * 0.8 {
        rate_limiter.set_rate(current_rate + step_size);  // Increase load
    }

    tokio::time::sleep(adjust_interval).await;
}
```

### 11.4 Progress Reporting (M1)

**Motivation**: Real-time visibility into long-running benchmarks

**Design**:
```rust
impl ScenarioExecutor {
    pub fn subscribe_progress(&self) -> ProgressReceiver {
        // Returns channel receiving periodic updates
    }
}

pub struct ProgressUpdate {
    pub elapsed: Duration,
    pub operations_completed: u64,
    pub current_rate: f64,
    pub instant_p99: u64,
    pub backpressure_active: bool,
}
```

**Usage**:
```rust
let mut progress = executor.subscribe_progress();
tokio::spawn(async move {
    while let Some(update) = progress.recv().await {
        println!("Progress: {} ops in {:?}", update.operations_completed, update.elapsed);
    }
});
```

### 11.5 Graceful Shutdown (M1)

**Motivation**: Ctrl+C should produce valid results

**Design**:
```rust
// Signal handler
tokio::select! {
    result = executor.execute() => {
        // Normal completion
    }
    _ = shutdown_signal() => {
        // Graceful shutdown
        executor.shutdown().await?;
        let partial_result = executor.collect_partial_results();
    }
}
```

**Requirements**:
- Stop accepting new operations immediately
- Wait for in-flight operations (with timeout)
- Produce partial metrics snapshot
- Cleanup if configured

### 11.6 Warmup/Cooldown Phases (M2)

**Motivation**: Exclude JIT compilation, cache warming from measurements

**Design**:
```yaml
executor:
  type: constant-rate
  rate: 1000
  warmup: 30s      # Operations not recorded
  duration: 300s   # Measurement phase
  cooldown: 30s    # Allow completion, not recorded
```

**Implementation**:
```rust
// Warmup phase
execute_phase(warmup_duration, record_metrics: false);

// Reset metrics
metrics.reset();

// Measurement phase
execute_phase(duration, record_metrics: true);

// Cooldown phase
execute_phase(cooldown_duration, record_metrics: false);
```

### 11.7 Coordinated Omission Handling (M3)

**Problem**: Current design submits at target rate regardless of completion

**Issue**: If operations are slow, latency measurements are artificially low (we only measure operations that got a connection)

**Solution**:
```rust
// Track when each operation SHOULD have been submitted
let intended_submit_time = Instant::now();
rate_limiter.acquire().await;
let actual_submit_time = Instant::now();

// Record coordinated omission delay
let co_delay = actual_submit_time - intended_submit_time;
metrics.record_coordinated_omission(co_delay);
```

**Reference**: Gil Tene's "How NOT to Measure Latency" (https://www.youtube.com/watch?v=lJ8ydIuPFeU)

### 11.8 Distributed M:N Architecture (M1+)

**Motivation**: Test distributed SQL databases (TiDB, CockroachDB, YugabyteDB) with multi-region awareness

**Architecture**: M clients (1 leader + M-1 workers) → N database endpoints

#### M:N Mapping Model

**Key Concepts**:
- **M clients**: Multiple RSBench instances coordinating to drive load
  - **Scaling**: Can have multiple clients per region/AZ to drive higher QPS
  - **Example**: 3 regions × 2 clients/region = 6 total clients (M=6)
- **1 leader**: Orchestrates phase transitions, aggregates metrics
- **M-1 workers**: Execute workload independently based on leader instructions
- **N endpoints**: Database endpoints (e.g., 3 TiDB regions, 5 CockroachDB nodes)

---

#### Loose Coordination Design Philosophy

**Critical Design Principle**: RSBench uses **loose coordination**, NOT tight synchronization.

**Motivation: Why Loose Coordination?**

Traditional distributed testing tools use tight synchronization:
```
❌ TIGHT SYNC (What Others Do):
  - Workers wait for global barriers
  - All workers must sync at every phase
  - Clock synchronization required (NTP drift issues)
  - Failure of one worker blocks all workers
  - Complex consensus protocols (Raft, Paxos)
  - High coordination overhead
```

RSBench uses loose coordination:
```
✅ LOOSE COORDINATION (RSBench):
  - Workers are independent after initial assignment
  - Phase transitions coordinated, but not synchronized
  - No clock synchronization required
  - Worker failure doesn't block others
  - Simple gRPC communication (no consensus)
  - Minimal coordination overhead
```

**Design Rationale**:

1. **Simplicity Over Perfection**:
   - Exact synchronization is not needed for load testing
   - Slight phase drift (1-2 seconds) doesn't affect results
   - Simple gRPC calls vs. complex distributed consensus
   - Easier to debug, easier to understand

2. **Fault Tolerance**:
   ```rust
   // Tight sync (fragile)
   await_all_workers_ready();  // If one fails, all block forever

   // Loose coordination (resilient)
   notify_workers(Phase::Execute);  // Fire-and-forget, workers handle independently
   ```

3. **Performance**:
   - No global barriers in hot path
   - Workers never wait for each other during execution
   - Only coordination: prepare → execute → collect phases

4. **Realistic Load Generation**:
   - Real-world clients don't synchronize perfectly
   - Slight phase drift models realistic traffic patterns
   - No artificial lockstep behavior

**What IS Coordinated (Minimal)**:

1. **Phase Boundaries** (3 sync points in entire test):
   ```
   Leader → Workers: "PREPARE"   (wait for ready ACK)
   Leader → Workers: "EXECUTE"   (broadcast start time, don't wait)
   Leader → Workers: "COLLECT"   (wait for results)
   ```

2. **Routing Assignments** (once at startup):
   ```
   Leader → Worker 1: "Use workers [0-49] → endpoint 0"
   Leader → Worker 2: "Use workers [50-99] → endpoint 1"
   ```

3. **Metrics Aggregation** (once at end):
   ```
   Leader ← Workers: Send ScenarioResult
   Leader: Merge histograms, sum counters
   ```

**What IS NOT Coordinated (Independent)**:

1. **Operation Execution**:
   - Each worker runs at its own pace
   - No synchronization between operations
   - No global rate limiter (each worker has local rate limiter)

2. **Clock Synchronization**:
   - No NTP requirement
   - Start times are relative, not absolute
   - Slight drift (1-2s) is acceptable and expected

3. **Workload State**:
   - Each worker has independent RNG (seeded differently)
   - No shared state during execution
   - Operations are deterministic per worker, not globally

4. **Connection Management**:
   - Each worker manages its own connection pool
   - No global connection coordination
   - Each worker saturates independently

**Implementation: Loose Coordination via gRPC**

```rust
// Leader side (simple fire-and-forget)
impl LeaderOrchestrator {
    async fn broadcast_phase(&self, phase: Phase) -> Result<()> {
        let workers = self.workers.read().await;

        for worker_addr in workers.iter() {
            let client = WorkerClient::connect(worker_addr.clone()).await?;

            // Fire-and-forget (don't wait for completion)
            tokio::spawn(async move {
                if let Err(e) = client.notify_phase(phase.clone()).await {
                    tracing::warn!("Worker {} failed to receive phase: {}", worker_addr, e);
                    // Don't fail entire test, just log warning
                }
            });
        }

        Ok(())
    }

    async fn collect_results(&self) -> Result<Vec<ScenarioResult>> {
        let workers = self.workers.read().await;
        let mut handles = vec![];

        for worker_addr in workers.iter() {
            let client = WorkerClient::connect(worker_addr.clone()).await?;

            // Parallel collection with timeout
            let handle = tokio::spawn(async move {
                tokio::time::timeout(
                    Duration::from_secs(30),
                    client.get_results()
                ).await
            });

            handles.push(handle);
        }

        // Collect results, skip failed workers
        let mut results = vec![];
        for handle in handles {
            match handle.await {
                Ok(Ok(Ok(result))) => results.push(result),
                _ => {
                    tracing::warn!("Failed to collect result from worker (timeout or error)");
                    // Continue with other workers
                }
            }
        }

        Ok(results)
    }
}

// Worker side (autonomous execution)
impl WorkerExecutor {
    async fn run(&mut self) -> Result<()> {
        // Wait for PREPARE phase
        let assignment = self.wait_for_assignment().await?;
        self.setup(assignment).await?;
        self.send_ready().await?;

        // Wait for EXECUTE phase
        self.wait_for_execute_phase().await?;

        // Execute INDEPENDENTLY (no further coordination)
        let result = self.scenario.execute().await?;

        // Store result for leader to collect later
        self.result = Some(result);

        Ok(())
    }
}
```

**Comparison Table**:

| Aspect | Tight Sync (Traditional) | Loose Coordination (RSBench) |
|--------|--------------------------|------------------------------|
| **Sync Points** | Every operation or batch | 3 total (prepare, execute, collect) |
| **Clock Sync** | Required (NTP, PTP) | Not required |
| **Worker Failure** | Blocks all workers | Logged, test continues |
| **Coordination Protocol** | Consensus (Raft, Paxos) | Simple gRPC calls |
| **Implementation Complexity** | High (1000+ lines) | Low (~200 lines) |
| **Hot Path Overhead** | High (barriers, locks) | Zero (no coordination during execution) |
| **Phase Drift Tolerance** | None (must be exact) | 1-2 seconds acceptable |
| **Realistic Load** | No (lockstep artificial) | Yes (models real clients) |

**Benefits of Loose Coordination**:

1. **Simplicity**:
   - No distributed consensus
   - No clock sync requirements
   - Easy to understand and debug

2. **Fault Tolerance**:
   - Worker failure doesn't halt test
   - Leader failure → workers continue local execution
   - Partial results still useful

3. **Performance**:
   - Zero hot path overhead
   - No barriers or locks
   - Each worker runs at full speed

4. **Scalability**:
   - Coordination overhead is O(1), not O(M×N)
   - Can scale to 10+ clients without performance impact
   - No coordination during execution (only at boundaries)

5. **Realism**:
   - Models real-world client behavior
   - No artificial synchronization
   - Natural load distribution

**Trade-offs (Acceptable)**:

1. **Phase Drift**: Workers may start execution 1-2 seconds apart
   - **Impact**: Negligible for tests > 60 seconds
   - **Mitigation**: Use longer test durations (5+ minutes)

2. **No Global Rate Limiter**: Each worker maintains target rate independently
   - **Impact**: Total rate may fluctuate ±5%
   - **Mitigation**: Use more workers for smoother total rate

3. **Partial Results on Failure**: If worker crashes, leader may not get its results
   - **Impact**: Results from other workers still valid
   - **Mitigation**: Log warnings, report partial metrics

**Why This Is the Right Trade-off**:

For database load testing:
- Exact synchronization **NOT required** (we're measuring database, not testing distributed consensus)
- Simplicity **IS required** (tool must be maintainable and debuggable)
- Fault tolerance **IS required** (long-running tests shouldn't fail due to one worker crash)
- Realism **IS valuable** (real clients don't synchronize perfectly)

Therefore: **Loose coordination is the optimal design**

---

**Example: TiDB 3-Region Test with Multiple Clients per Region**

```
┌─────────────────────────────────────────────────────────────────────┐
│                 Distributed Test Setup (M=6, N=3)                   │
│                                                                      │
│  Region us-west-1          Region us-east-1         Region eu-west-1│
│  ┌────────────┐            ┌────────────┐           ┌────────────┐ │
│  │ Leader     │            │ Worker 2   │           │ Worker 4   │ │
│  │ (Client 1) │            │ (Client 3) │           │ (Client 5) │ │
│  │ 100 workers│            │ 100 workers│           │ 100 workers│ │
│  └──────┬─────┘            └──────┬─────┘           └──────┬─────┘ │
│         │                         │                        │        │
│  ┌──────┴─────┐            ┌──────┴─────┐           ┌──────┴─────┐ │
│  │ Worker 1   │            │ Worker 3   │           │ Worker 5   │ │
│  │ (Client 2) │            │ (Client 4) │           │ (Client 6) │ │
│  │ 100 workers│            │ 100 workers│           │ 100 workers│ │
│  └──────┬─────┘            └──────┬─────┘           └──────┬─────┘ │
│         │                         │                        │        │
│         ▼                         ▼                        ▼        │
│  ┌──────────────┐          ┌──────────────┐        ┌──────────────┐│
│  │ TiDB Region 1│          │ TiDB Region 2│        │ TiDB Region 3││
│  │ (us-west-1)  │          │ (us-east-1)  │        │ (eu-west-1)  ││
│  │ 192.168.1.10 │          │ 192.168.2.10 │        │ 192.168.3.10 ││
│  └──────────────┘          └──────────────┘        └──────────────┘│
│                                                                      │
│  M = 6 clients (2 per region for scaling)                          │
│  N = 3 endpoints (1 per region)                                    │
│  Total: 600 async task workers                                     │
│  Per-region QPS: 2 clients × 1000 ops/sec = 2000 ops/sec           │
│  Total QPS: 6000 ops/sec (2000/region × 3 regions)                 │
└─────────────────────────────────────────────────────────────────────┘
```

**Why Multiple Clients per Region/AZ?**

1. **Higher QPS per Region**:
   - Single client limited by CPU/network (even with async I/O)
   - 2-4 clients per region can saturate database capacity
   - Example: 1 client = 5K ops/sec, 2 clients = 10K ops/sec per region

2. **Fault Tolerance**:
   - If one client crashes in a region, others continue
   - Partial results still provide region-level insights

3. **Resource Limits**:
   - Kubernetes pod resource limits (CPU, memory)
   - Network limits per pod
   - File descriptor limits

4. **Even Distribution**:
   - With 3 regions and 6 clients: perfectly balanced (2 per region)
   - With 3 regions and 5 clients: 2-2-1 distribution (acceptable)

**Scaling Formula**:
```
Total QPS = M clients × QPS per client
Clients per region = M / N (rounded appropriately)
Per-region QPS = (M / N) × QPS per client
```

**Example Scaling**:

| Scenario | Regions (N) | Clients (M) | Clients/Region | QPS/Client | Total QPS |
|----------|-------------|-------------|----------------|------------|-----------|
| Light    | 3           | 3           | 1              | 1000       | 3K        |
| Medium   | 3           | 6           | 2              | 2000       | 12K       |
| Heavy    | 3           | 12          | 4              | 2000       | 24K       |
| Extreme  | 3           | 18          | 6              | 3000       | 54K       |

#### Configuration Schema

**Infrastructure Config** (`config/rsbench.config.yaml`):

```yaml
database:
  driver: mysql
  endpoints:  # N endpoints
    - host: 192.168.1.10  # Region 1 (us-west-1)
      port: 4000
      database: sbtest
      region: us-west-1
      role: readwrite

    - host: 192.168.2.10  # Region 2 (us-east-1)
      port: 4000
      database: sbtest
      region: us-east-1
      role: readwrite

    - host: 192.168.3.10  # Region 3 (eu-west-1)
      port: 4000
      database: sbtest
      region: eu-west-1
      role: readonly      # Read replica

runtime:
  type: async
  max_connections: 200  # Per client
  connection_strategy: multi-endpoint  # Enable M:N routing

distributed:
  mode: leader  # or 'worker' or 'standalone'
  leader_address: "192.168.100.1:8080"  # gRPC address
  worker_id: 1  # Unique worker ID (1 for leader)
```

**Scenario Config** (`scenarios/distributed_load_test.yaml`):

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 3000              # Total rate across all M clients
    duration: 300s
    workers: 150            # Total workers distributed across M clients

  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml

  routing:
    strategy: region-affinity  # Route based on region preference
    affinity_map:
      - worker_range: [0, 49]
        endpoint: 0         # Workers 0-49 → Region 1
      - worker_range: [50, 99]
        endpoint: 1         # Workers 50-99 → Region 2
      - worker_range: [100, 149]
        endpoint: 2         # Workers 100-149 → Region 3
```

#### Routing Strategies

**1. Region Affinity** (default for distributed databases):
```yaml
routing:
  strategy: region-affinity
  affinity_map:
    - worker_range: [0, 49]
      endpoint: 0  # Workers 0-49 prefer Region 1
    - worker_range: [50, 99]
      endpoint: 1  # Workers 50-99 prefer Region 2
```

**Benefits**: Models real-world geographic distribution, tests cross-region latency

**2. Cross-Region** (test distributed transactions):
```yaml
routing:
  strategy: cross-region
  distribution:
    - endpoint: 0
      weight: 33  # 33% to Region 1
    - endpoint: 1
      weight: 33  # 33% to Region 2
    - endpoint: 2
      weight: 34  # 34% to Region 3
```

**Benefits**: Tests distributed consensus, cross-region transaction performance

**3. Read-Write Split**:
```yaml
routing:
  strategy: read-write-split
  read_endpoints: [0, 1, 2]   # All regions for reads
  write_endpoints: [0]         # Only Region 1 for writes
  read_weight: 0.8            # 80% reads
```

**Benefits**: Models real-world read-heavy workloads, tests read scaling

#### Leader Responsibilities

**Leader Instance** runs on one client machine:

1. **Worker Discovery**:
   ```rust
   // Leader discovers all M worker clients
   let workers = discover_workers(config.distributed.worker_addresses).await?;
   // workers = [worker_1, worker_2, worker_3]
   ```

2. **M:N Routing Assignment**:
   ```rust
   // Assign worker_id ranges to each client
   // Client 1 (leader): workers [0-49]   → endpoint 0
   // Client 2: workers [50-99]  → endpoint 1
   // Client 3: workers [100-149] → endpoint 2

   for (client_id, client_addr) in workers.iter().enumerate() {
       let assignment = WorkerAssignment {
           worker_id_start: client_id * 50,
           worker_id_end: (client_id + 1) * 50,
           endpoint_index: client_id % endpoints.len(),
       };
       send_assignment(client_addr, assignment).await?;
   }
   ```

3. **Phase Orchestration**:
   ```rust
   // Leader broadcasts phase transitions
   async fn execute_distributed(&mut self) -> Result<ScenarioResult> {
       // Phase 1: Prepare
       broadcast_phase(Phase::Prepare).await?;
       wait_for_ready().await?;

       // Phase 2: Execute
       broadcast_phase(Phase::Execute { start_time: Instant::now() }).await?;

       // Run local workload
       self.execute_local_workload().await?;

       // Phase 3: Collect
       broadcast_phase(Phase::Collect).await?;
       let worker_results = collect_worker_results().await?;

       // Aggregate results
       aggregate_results(worker_results)
   }
   ```

4. **Metrics Aggregation**:
   ```rust
   fn aggregate_results(worker_results: Vec<ScenarioResult>) -> ScenarioResult {
       let total_ops = worker_results.iter().map(|r| r.operations_completed).sum();
       let total_errors = worker_results.iter().map(|r| r.errors).sum();

       // Merge HDR histograms
       let mut merged_histogram = Histogram::new(...);
       for result in &worker_results {
           merged_histogram.add(&result.latency_histogram)?;
       }

       ScenarioResult {
           operations_completed: total_ops,
           errors: total_errors,
           duration: worker_results[0].duration,  // Same for all workers
           latency_histogram: merged_histogram,
           // ... per-endpoint breakdown
       }
   }
   ```

#### Worker Implementation

**Worker Instance** (on each non-leader client):

```rust
pub struct DistributedScenarioExecutor {
    mode: DistributedMode,
    local_executor: ScenarioExecutor,
    leader_client: Option<LeaderClient>,  // gRPC client to leader
    endpoint_assignment: Option<usize>,   // Which endpoint to use
}

impl DistributedScenarioExecutor {
    pub async fn execute(&mut self) -> Result<ScenarioResult> {
        match self.mode {
            DistributedMode::Leader => self.execute_leader().await,
            DistributedMode::Worker => self.execute_worker().await,
            DistributedMode::Standalone => self.local_executor.execute().await,
        }
    }

    async fn execute_worker(&mut self) -> Result<ScenarioResult> {
        // Wait for leader assignment
        let assignment = self.leader_client.receive_assignment().await?;

        // Configure local executor with assigned endpoint
        self.local_executor.set_endpoint(assignment.endpoint_index);
        self.local_executor.set_worker_id_range(
            assignment.worker_id_start,
            assignment.worker_id_end
        );

        // Wait for execute phase
        self.leader_client.wait_for_phase(Phase::Execute).await?;

        // Execute local workload
        let result = self.local_executor.execute().await?;

        // Send results to leader
        self.leader_client.send_results(result.clone()).await?;

        Ok(result)
    }
}
```

#### Multi-Endpoint Connection Pool

```rust
pub struct MultiEndpointPool {
    endpoints: Vec<EndpointPool>,
    routing_strategy: RoutingStrategy,
}

impl MultiEndpointPool {
    pub async fn get(&self, worker_id: usize) -> Result<PooledConnection> {
        let endpoint_index = self.routing_strategy.select(worker_id);
        self.endpoints[endpoint_index].get().await
    }
}

pub enum RoutingStrategy {
    RegionAffinity(Vec<WorkerRange>),
    CrossRegion(Vec<WeightedEndpoint>),
    ReadWriteSplit { read: Vec<usize>, write: Vec<usize> },
}
```

#### Metrics Breakdown

**Per-Endpoint Metrics**:
```
Endpoint 0 (us-west-1):
  Operations: 50000
  Latency p99: 5ms
  Errors: 12

Endpoint 1 (us-east-1):
  Operations: 50000
  Latency p99: 45ms   ← Higher cross-region latency
  Errors: 8

Endpoint 2 (eu-west-1):
  Operations: 50000
  Latency p99: 95ms   ← Even higher cross-region latency
  Errors: 120         ← Read replica experiencing issues
```

**Per-Worker Metrics** (for debugging):
```
Worker 0 (Client 1): 333 ops, 0 errors
Worker 1 (Client 1): 334 ops, 0 errors
...
Worker 149 (Client 3): 333 ops, 0 errors
```

**Global Aggregated Metrics**:
```
Total Operations: 150000
Total Errors: 140
Global p99 Latency: 95ms
Backpressure Events: 0
```

#### Scenario Module Integration

**DistributedMode Enum**:

```rust
pub enum DistributedMode {
    Standalone,  // M0: Single client, single endpoint
    Leader {     // M1: Orchestrator
        worker_addresses: Vec<String>,
        leader_port: u16,
    },
    Worker {     // M1: Follower
        leader_address: String,
        worker_id: usize,
    },
}
```

**Configuration Flow**:

```rust
impl ScenarioExecutor {
    pub fn new(config: ScenarioConfig) -> Result<Self> {
        let distributed_mode = match &config.distributed {
            None => DistributedMode::Standalone,
            Some(d) if d.mode == "leader" => DistributedMode::Leader { ... },
            Some(d) if d.mode == "worker" => DistributedMode::Worker { ... },
            _ => return Err(Error::InvalidConfig),
        };

        // Create appropriate executor
        match distributed_mode {
            DistributedMode::Standalone => Self::new_standalone(config),
            _ => Self::new_distributed(config, distributed_mode),
        }
    }
}
```

#### Example: Complete TiDB 3-Region Test

**Setup**:
1. Deploy TiDB cluster across 3 regions (us-west-1, us-east-1, eu-west-1)
2. Deploy 3 RSBench clients (one per region, co-located with TiDB)
3. Designate one client as leader

**Client 1 (Leader)** - us-west-1:
```bash
rsbench \
  --config config/rsbench.config.yaml \
  --scenario scenarios/distributed_load_test.yaml \
  --distributed-mode leader \
  --leader-port 8080
```

**Client 2 (Worker)** - us-east-1:
```bash
rsbench \
  --config config/rsbench.config.yaml \
  --scenario scenarios/distributed_load_test.yaml \
  --distributed-mode worker \
  --leader-address 192.168.100.1:8080 \
  --worker-id 2
```

**Client 3 (Worker)** - eu-west-1:
```bash
rsbench \
  --config config/rsbench.config.yaml \
  --scenario scenarios/distributed_load_test.yaml \
  --distributed-mode worker \
  --leader-address 192.168.100.1:8080 \
  --worker-id 3
```

**Execution Flow**:
```
[Leader] Discovering workers... Found 2 workers
[Leader] Assigning worker ranges: [0-49], [50-99], [100-149]
[Leader] Broadcasting PREPARE phase...
[Worker 2] Received assignment: workers [50-99] → endpoint 1
[Worker 3] Received assignment: workers [100-149] → endpoint 2
[Leader] All workers ready. Broadcasting EXECUTE phase...
[All] Executing workload for 300s...
[Leader] Collecting results from workers...
[Leader] Aggregating 3 result sets...
[Leader] Final Results:
  Total Operations: 900000 (3000 ops/sec × 300s)
  Endpoint 0 (us-west-1): 300000 ops, p99: 5ms
  Endpoint 1 (us-east-1): 300000 ops, p99: 45ms
  Endpoint 2 (eu-west-1): 300000 ops, p99: 95ms
```

#### Event Integration (M1)

Distributed mode integrates with event module for advanced scenarios:

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 3000
    duration: 600s

  events:
    - source: k8s
      watch:
        - namespace: tidb
          resource: pod
          event_type: delete  # Failover event
      action:
        phase: observe      # Don't change rate, just observe

    - time: 300s
      action:
        phase: change_routing
        routing:
          strategy: cross-region  # Switch to cross-region after 5min
```

**Failover Testing**:
```
Timeline:
  0-60s:    Normal load (region-affinity)
  60s:      K8s pod delete (simulated failover)
  60-90s:   High latency on endpoint 0 (failover in progress)
  90-600s:  Recovered (TiDB rebalanced)

Metrics show:
  Endpoint 0 latency p99: 5ms → 2000ms → 8ms
  Backpressure events: 0 → 450 → 0
```

#### Benefits of M:N Architecture

1. **Realistic Distributed Testing**: Model real-world multi-region deployments
2. **Scalability**: M clients can drive more load than single client
3. **Region Awareness**: Test region-specific performance characteristics
4. **Failover Testing**: Observe behavior during region failures
5. **Cross-Region Latency**: Measure distributed transaction overhead
6. **Read Scaling**: Test read replica performance
7. **Coordinated Load**: All M clients start/stop simultaneously
8. **Aggregated Metrics**: Single unified result across M clients

---

## Appendix A: Configuration Examples

### A.1 Minimal Configuration
```yaml
scenario:
  executor:
    type: constant-rate
    rate: 100
    duration: 60s
  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml
```

### A.2 Production Load Test
```yaml
scenario:
  executor:
    type: ramping-rate
    stages:
      - duration: 5m
        target_rate: 1000   # Normal load
      - duration: 10m
        target_rate: 2000   # Peak load
      - duration: 5m
        target_rate: 1000   # Return to normal
    max_connections: 500
  workload:
    type: declarative
    file: workloads/production_mix.yaml
```

### A.3 Spike Test
```yaml
scenario:
  executor:
    type: ramping-rate
    stages:
      - duration: 2m
        target_rate: 500    # Baseline
      - duration: 30s
        target_rate: 5000   # SPIKE
      - duration: 2m
        target_rate: 500    # Recovery
    max_connections: 1000
  workload:
    type: declarative
    file: workloads/oltp_read_heavy.yaml
```

---

## Appendix B: Glossary

- **Scenario**: Complete benchmark execution plan (executor + workload + config)
- **Executor**: Strategy for controlling operation submission rate and timing
- **Operation**: Single database action (SQL + parameters)
- **Iteration**: Monotonic counter incremented for each operation submission
- **Rate**: Target operations per second (ops/sec)
- **Stage**: Time period with a fixed target rate (in ramping executor)
- **Backpressure**: Condition where pool/runtime is saturated
- **Coordinated Omission**: Measurement artifact from not accounting for queueing delays
- **Fire-and-Forget**: Submitting operations without waiting for completion
- **Time-Driven**: Scheduling based on wall-clock time vs. completion events
- **Worker**: Tokio async task (green thread), not OS thread
- **Closed-Loop**: Execution model where workers wait for completion before next operation
- **Open-Loop**: Execution model with time-driven fire-and-forget submission

---

## Appendix C: Sysbench Migration Guide

### Complete Command Mapping

**1. Thread-based Workload (--threads)**

```bash
# Sysbench
sysbench oltp_read_write \
  --mysql-host=localhost \
  --mysql-user=root \
  --mysql-password=secret \
  --mysql-db=sbtest \
  --threads=16 \
  --time=300 \
  run

# RSBench (Configuration File - Recommended)
cat > config.yaml <<EOF
database:
  driver: mysql
  connection_string: mysql://root:secret@localhost:3306/sbtest

runtime:
  type: async
  workers: 8           # Tokio OS threads
  max_connections: 100

scenario:
  executor:
    type: closed-loop
    workers: 16        # Equivalent to --threads=16
    duration: 300s     # Equivalent to --time=300

  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml
EOF

rsbench --config config.yaml run

# RSBench (CLI Shorthand - Future M1)
rsbench run \
  --db-url mysql://root:secret@localhost:3306/sbtest \
  --workload oltp_read_write \
  --workers 16 \
  --duration 300s
```

**2. Rate-limited Workload (--rate)**

```bash
# Sysbench
sysbench oltp_read_write \
  --mysql-host=localhost \
  --rate=1000 \
  --time=300 \
  run

# RSBench
cat > config.yaml <<EOF
scenario:
  executor:
    type: constant-rate
    rate: 1000         # Equivalent to --rate=1000
    duration: 300s
  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml
EOF

rsbench --config config.yaml run
```

**3. Combined (--threads + --rate)**

```bash
# Sysbench
sysbench oltp_read_write \
  --threads=16 \
  --rate=1000 \
  --time=300 \
  run

# RSBench (M1 - Future)
scenario:
  executor:
    type: closed-loop
    workers: 16
    max_rate: 1000     # Cap throughput
    duration: 300s
```

### Workload Translation Table

| Sysbench Builtin | RSBench Declarative YAML | Description |
|------------------|--------------------------|-------------|
| `oltp_read_write` | `workloads/oltp_read_write.yaml` | Balanced mix |
| `oltp_read_only` | `workloads/oltp_read_only.yaml` | 100% reads |
| `oltp_write_only` | `workloads/oltp_write_only.yaml` | 100% writes |
| `oltp_point_select` | `workloads/oltp_point_select.yaml` | Point SELECT only |
| `oltp_insert` | `workloads/oltp_insert.yaml` | INSERT only |
| `oltp_update_index` | `workloads/oltp_update_index.yaml` | UPDATE indexed |
| `oltp_update_non_index` | `workloads/oltp_update_non_index.yaml` | UPDATE non-indexed |
| `oltp_delete` | `workloads/oltp_delete.yaml` | DELETE + INSERT |
| `select_random_points` | `workloads/select_random_points.yaml` | 10 random points |
| `select_random_ranges` | `workloads/select_random_ranges.yaml` | Range SELECTs |
| Custom `.lua` | `type: lua, script: path.lua` | Lua compatibility |

### Complete Parameter Mapping

| Sysbench Parameter | RSBench Config Path | Notes |
|--------------------|---------------------|-------|
| `--threads=N` | `scenario.executor.workers: N` | Async tasks, not OS threads |
| `--rate=N` | `scenario.executor.rate: N` | For constant-rate executor |
| `--time=N` | `scenario.executor.duration: Ns` | Supports 30s, 5m, 1h |
| `--db-driver=mysql` | `database.driver: mysql` | |
| `--mysql-host=HOST` | In `database.connection_string` | |
| `--mysql-port=PORT` | In `database.connection_string` | |
| `--mysql-user=USER` | In `database.connection_string` | |
| `--mysql-password=PASS` | In `database.connection_string` | |
| `--mysql-db=DB` | In `database.connection_string` | |
| `--tables=N` | `workload.schema.tables[].count: N` | In workload YAML |
| `--table-size=N` | `workload.schema.tables[].row_count: N` | In workload YAML |
| `--rand-type=uniform` | `parameters[].distribution.type: uniform` | In workload YAML |
| `--rand-type=zipfian` | `parameters[].distribution.type: zipfian` | In workload YAML |
| `--report-interval=N` | (Future M1) | Progress reporting |
| `--percentile=99` | Always included in output | HDR histograms |

### Execution Phase Mapping

| Sysbench Command | RSBench Command | Description |
|------------------|-----------------|-------------|
| `sysbench ... prepare` | `rsbench prepare --config config.yaml` | Create tables, load data |
| `sysbench ... run` | `rsbench run --config config.yaml` | Execute benchmark |
| `sysbench ... cleanup` | `rsbench cleanup --config config.yaml` | Drop tables |

### Key Differences from Sysbench

**1. Configuration Philosophy:**
- **Sysbench**: CLI-first (all parameters as flags)
- **RSBench**: Config-first (YAML/TOML files, version-controlled)

**2. Worker Model:**
- **Sysbench**: `--threads=100` = 100 OS threads (~800MB memory)
- **RSBench**: `workers: 100` = 100 async tasks on 8 OS threads (~20MB)

**3. Workload Definition:**
- **Sysbench**: Built-in workloads (black box)
- **RSBench**: Declarative YAML files (fully transparent and editable)

**4. Metrics Output:**
- **Sysbench**: Text only, limited percentiles
- **RSBench**: JSON + Text, complete HDR histograms (p50, p95, p99, p99.9, p99.99)

**5. Backpressure Visibility:**
- **Sysbench**: Hidden (conflated with database performance)
- **RSBench**: Explicit metric (backpressure_events counter)

### Example: Complete Migration Workflow

```bash
# 1. Sysbench: Prepare phase
sysbench oltp_read_write \
  --mysql-host=localhost \
  --mysql-user=root \
  --mysql-db=sbtest \
  --tables=10 \
  --table-size=100000 \
  prepare

# RSBench equivalent:
# Edit workload to set table count and row count, then:
rsbench prepare --config config.yaml

# 2. Sysbench: Run benchmark
sysbench oltp_read_write \
  --threads=16 \
  --time=300 \
  run

# RSBench equivalent:
rsbench run --config config.yaml

# 3. Sysbench: Cleanup
sysbench oltp_read_write cleanup

# RSBench equivalent:
rsbench cleanup --config config.yaml
```

### Sysbench-Compatible CLI Wrapper (Future)

**For 100% backward compatibility, planned wrapper:**

```bash
# Install wrapper (future)
cargo install rsbench-sysbench

# Use sysbench syntax
rsbench-sysbench oltp_read_write \
  --mysql-host=localhost \
  --mysql-user=root \
  --threads=16 \
  --time=300 \
  run

# Internally translates to RSBench config and executes
```

This provides zero-migration-cost path for existing sysbench users!

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2024-12-25 | Design Doc | Initial comprehensive design document |

---

**End of Document**
