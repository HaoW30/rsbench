# RSBench Project Context for Claude

> **Purpose**: This file provides essential context for AI assistants working on the RSBench project.
> Last Updated: 2025-12-24

## Project Overview

**RSBench** is a modern, time-driven database testing tool built in Rust - the next generation of sysbench.

### Mission
Replace sysbench by addressing fundamental limitations in traditional database testing tools while maintaining backward compatibility. Purpose-built for **distributed SQL databases** and **cloud-native workloads**.

### Key Problems Solved
1. **Coordinated Omission**: Traditional tools hide latency under load
2. **Hidden Backpressure**: Client-side bottlenecks are invisible
3. **Thread-Based Load**: Conflates concurrency with load
4. **Single-Node Assumptions**: No support for distributed databases
5. **Implicit Behavior**: Phases and load changes are emergent, not intentional

## Core Design Principles (CRITICAL - Always Follow)

### 0. Start Simple, Stay Extensible (FOUNDATIONAL)

**Philosophy**: Simplicity is a feature, not a limitation. Complexity is a cost, not a benefit.

**Golden Rules**:
1. **Implement the minimum required for current milestone**
   - Don't build for hypothetical future requirements
   - Don't add "just in case" features
   - Don't create abstractions before you have 3+ concrete use cases

2. **Simple > Clever**
   - Obvious code beats clever code
   - Linear flow beats complex abstractions
   - Direct implementation beats generic framework

3. **Extensible Through Interfaces**
   - Use traits for extension points (DatabaseDriver, Workload, RuntimeEngine)
   - Keep interfaces minimal and focused
   - Add to interface only when needed, never "just in case"

4. **Avoid Over-Engineering**
   ❌ **DON'T**:
   - Add configuration options that aren't used yet
   - Create helper functions for one-time operations
   - Build abstractions for code that's only used once
   - Add "flexibility" that isn't required by current milestone
   - Create generic solutions for specific problems

   ✅ **DO**:
   - Solve the immediate problem directly
   - Copy-paste is better than wrong abstraction
   - Three uses → then abstract
   - Add complexity only when removing it costs more

5. **M0 First, M1 Later**
   - M0: Single scenario, single endpoint, constant rate → **Keep it simple**
   - M1: Multi-scenario, multi-endpoint → **Then add complexity**
   - Don't implement M1 features in M0 "to save time later"

**Examples of Good Simplicity**:

```rust
// ✅ GOOD - M0: Simple, direct
pub struct ConnectionPool {
    driver: Arc<dyn DatabaseDriver>,
    config: PoolConfig,
    connection_string: String,
}

impl ConnectionPool {
    pub async fn get(&self) -> Result<PooledConnection> {
        // M0: Just create new connection
        let conn = self.driver.connect(&config).await?;
        Ok(PooledConnection { inner: conn })
    }
}

// ❌ BAD - M0: Over-engineered for future
pub struct ConnectionPool<T: PoolingStrategy> {
    driver: Arc<dyn DatabaseDriver>,
    strategy: T,
    health_checker: Box<dyn HealthChecker>,
    load_balancer: Option<Box<dyn LoadBalancer>>,
    // ... 10 more fields for "flexibility"
}
```

```rust
// ✅ GOOD - M0: Single scenario, keep it simple
pub async fn execute(&mut self) -> Result<ScenarioResult> {
    match &self.config.executor {
        ConstantRate { rate, duration, .. } =>
            self.execute_constant_rate(*rate, *duration).await,
        RampingRate { stages, .. } =>
            self.execute_ramping_rate(stages).await,
    }
}

// ❌ BAD - M0: Building for multi-scenario too early
pub async fn execute(&mut self) -> Result<Vec<ScenarioResult>> {
    let orchestrator = ScenarioOrchestrator::new()
        .with_dependency_graph(self.build_dag())
        .with_synchronization(self.sync_strategy.clone())
        .with_coordinator(self.coordinator.as_ref());
    // ... 50 lines of complex orchestration not needed until M2
}
```

**When to Add Complexity**:
- ✅ When current milestone requirements demand it
- ✅ When you have 3+ concrete use cases for abstraction
- ✅ When technical debt is actively blocking progress
- ✅ When simplicity would cause correctness issues

**When NOT to Add Complexity**:
- ❌ "We might need this later"
- ❌ "This makes it more flexible"
- ❌ "It's more elegant this way"
- ❌ "To avoid refactoring in the future"

**Remember**:
- **Simple code is easier to debug** - You'll spend more time debugging than writing
- **Simple code is easier to change** - Requirements will change
- **Simple code is easier to understand** - Future you will thank present you
- **You can always add complexity later** - But removing it is painful

**Milestone 0 Mantra**:
> "If it's not required for M0 success criteria, don't build it yet."

---

### 1. Time Is the Primary Control Plane
- Load defined by **time and rate**, NOT by threads
- Work scheduled by clock, not thread speed
- Threads/tasks are implementation details
- **Rate limiter** controls pacing, always

### 2. Backpressure Is a Signal, Not a Failure
- Client-side pressure is **observable and tracked**
- **NEVER hide** overload behind blocking calls
- Distinguish client limits from database limits
- Backpressure events are first-class metrics

### 3. The Client Must Never Lie
- Client limits are **explicitly tracked**
- Saturation is **visible in metrics**
- If client becomes bottleneck, user must know
- No coordinated omission, ever

### 4. Database Workloads Are Stateful
- Model sessions, transactions, prepared statements
- Support read/write routing and hot partitions
- Workloads behave like real applications

### 5. Test-as-Code Is the Default
- Workloads are **version-controlled artifacts**
- Declarative when possible, programmable when necessary
- Tests can be reviewed, diffed, reproduced

### 6. Explicit Phases Instead of Implicit Behavior
- Phase boundaries are **clear and deterministic**
- Load changes are intentional, not emergent
- Nothing important happens by accident

### 7. Simplicity Beats Distributed Cleverness
- Workers are independent
- Coordination is minimal and time-based
- No shared state in hot path

### 8. Determinism for Reproducibility
- Same seed + same config = same operation sequence
- Seeded RNG (ChaCha8) throughout
- No wall-clock in operation generation

## Architecture Summary

### Key Architectural Innovations

**1. Worker Model: Async Tasks, Not OS Threads**

RSBench uses **Tokio async tasks (green threads)** for workers, NOT OS threads like sysbench:

```
User Config: workers: 100
      ↓
100 Tokio async tasks (~2KB stack each)
      ↓
Scheduled on 8 OS threads (tokio runtime)
      ↓
Non-blocking async I/O
```

**Comparison:**

| Aspect | Sysbench --threads=100 | RSBench workers: 100 |
|--------|------------------------|----------------------|
| **OS Threads** | 100 | 4-8 (tokio runtime) |
| **Memory** | ~800 MB (thread stacks) | ~20 MB (async tasks) |
| **Context Switches** | High (100 threads) | Low (8 threads) |
| **I/O Model** | Blocking (thread waits) | Async (task yields) |
| **Scalability Limit** | ~500 threads (OS limit) | ~10K workers (memory limit) |

**Why This Matters:**
- Simulate 1000 concurrent users with only 8 OS threads
- 40x less memory than sysbench for same concurrency
- No context switch overhead
- Can scale to 10K+ workers for extreme load tests

**2. Backpressure Awareness: Client Saturation is Visible**

Traditional tools **hide backpressure** by blocking threads. RSBench **makes it visible**:

**What is Backpressure?**
- Backpressure = RSBench (client) is saturated, NOT the database
- Occurs when connection pool exhausted or runtime semaphore full
- Invalid test results - you're measuring client limits, not database performance

**How RSBench Detects Backpressure:**
1. **Pool Utilization Monitoring**: Tracks when pool > 80% utilized (configurable)
2. **Semaphore Saturation**: Detects when no permits available
3. **Explicit Metrics**: backpressure_events counter, backpressure_percentage

**Example Metrics:**
```
Client Metrics:
  Backpressure Events: 5420    ⚠️ HIGH - Results may be invalid!
  Backpressure %: 9.0%
  Pool Utilization p99: 98.2%  ⚠️ Client saturated
```

**Interpreting Results:**
- **0-1% backpressure**: ✅ Valid results - measuring database
- **1-5%**: ⚠️ Minor skew - consider increasing max_connections
- **5-20%**: ❌ Invalid - measuring client limits, not database
- **>20%**: ❌ Completely invalid - significantly increase max_connections

**Why This Matters:**
- Sysbench hides client saturation → you don't know if results are valid
- RSBench shows backpressure → you know when to increase client capacity
- Distinguish client bottlenecks from database bottlenecks
- Prevents coordinated omission problem

**3. M:N Distributed Architecture: Multi-Region Testing with Loose Coordination**

For distributed SQL databases (TiDB, CockroachDB, YugabyteDB), RSBench supports **M clients → N database endpoints**:

```
M = 3 Clients (1 leader + 2 workers)
      ↓
N = 3 Database Endpoints (us-west, us-east, eu-west)

Leader (Client 1):
  - Discovers all M worker clients
  - Assigns worker_id ranges to each client
  - Orchestrates phase transitions (prepare → execute → collect)
  - Aggregates metrics from all workers

Workers (Client 2, 3):
  - Receive endpoint assignments from leader
  - Execute workload INDEPENDENTLY (no further coordination)
  - Report results back to leader

Total: 150 async task workers across 3 clients → 3 regions
```

**Critical Design: Loose Coordination, NOT Tight Sync**

RSBench uses **loose coordination** for simplicity and fault tolerance:

```
✅ LOOSE COORDINATION (RSBench):
  - Only 3 sync points: prepare → execute → collect
  - Workers run independently after initial assignment
  - No clock synchronization required (NTP not needed)
  - Worker failure doesn't block others
  - Simple gRPC calls (no consensus protocols)
  - Zero hot path overhead

❌ TIGHT SYNC (Traditional Tools):
  - Global barriers at every operation/batch
  - Clock synchronization required (NTP, PTP)
  - Worker failure blocks all workers
  - Complex consensus (Raft, Paxos)
  - High coordination overhead
```

**What IS Coordinated:**
1. Phase boundaries (3 sync points total)
2. Routing assignments (once at startup)
3. Metrics aggregation (once at end)

**What IS NOT Coordinated:**
1. Operation execution (each worker runs at its own pace)
2. Clock synchronization (slight drift 1-2s is acceptable)
3. Workload state (independent RNG per worker)
4. Connection management (each worker has own pool)

**Why Loose Coordination:**
- **Simplicity**: ~200 lines vs ~1000+ lines for tight sync
- **Fault Tolerance**: Worker failure doesn't halt test
- **Performance**: Zero hot path overhead (no barriers during execution)
- **Realism**: Real clients don't synchronize perfectly

**Routing Strategies:**
1. **Region Affinity**: Workers 0-49 → Region 1, 50-99 → Region 2, 100-149 → Region 3
   - Models real-world geographic distribution
2. **Cross-Region**: Distribute operations across all regions
   - Tests distributed consensus, cross-region transactions
3. **Read-Write Split**: Reads to all regions, writes to primary
   - Models read-heavy workloads, tests read scaling

**Why This Matters:**
- Test multi-region latency characteristics
- Simulate real-world geographic distribution
- Coordinate failover testing across regions
- Measure cross-region transaction overhead
- Per-endpoint metrics show region-specific performance

**4. Event Module: Parallel External Event Integration**

RSBench has a **parallel Event Module** that runs alongside the Scenario Module (not nested inside it):

```
Architecture:
  ┌──────────────┐        ┌──────────────┐
  │   Scenario   │        │    Event     │  ← PARALLEL MODULES
  │   Module     │◄───────│   Module     │
  └──────────────┘  mpsc  └──────┬───────┘
                          channel│
                                 ├─► K8s Watcher
                                 ├─► Webhook Listener
                                 └─► Timer Events
```

**Why Parallel, Not Nested:**
- ✅ **Separation of Concerns**: Event watching is independent from workload execution
- ✅ **Composability**: Can run Scenario without Event Module (standalone mode)
- ✅ **Testability**: Can test modules independently
- ✅ **Reusability**: Same Event Module works with any Scenario executor

**Communication**: Event Module → Scenario via `tokio::mpsc` channel

**Event Sources:**
1. **K8s Watcher** (M1+): Watch pod/deployment events (failover, upgrades)
2. **Webhook Listener** (M1+): Receive events from Chaos Mesh, Prometheus, custom tools
3. **Timer Events** (M0): Time-based phase transitions

**How Scenario Reacts:**
```rust
// Scenario checks for events (non-blocking)
if let Ok(event) = event_rx.try_recv() {
    match event {
        Event::RateChange(rate) => self.rate_limiter.set_rate(rate),
        Event::PhaseTransition(Phase::Pause) => self.paused = true,
        Event::K8sEvent { .. } => /* record timestamp for correlation */,
    }
}
```

**Use Cases:**
- Failover testing: Monitor K8s pod deletion, correlate with latency spikes
- Rolling upgrade testing: Reduce load during upgrades
- Chaos engineering: Receive events from Chaos Mesh, adjust load
- Time-based patterns: Simulate daily traffic (night → morning → peak → evening)

**Key Design Point**: In distributed mode, **only leader** receives events and broadcasts phase changes to workers

### Module Organization

```
┌─────────────────────────────────────────────────────────────┐
│                         CLI Module                          │
│  Parse args, load infrastructure config + scenario         │
└────────────────────────────┬────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────┐
│                     Config Module                           │
│  InfrastructureConfig (config/) + ScenarioFile (scenarios/) │
│  Declarative workload loading (workloads/)                  │
└──────┬──────────────────────┬───────────────────────────────┘
       │                      │
       ▼                      ▼
┌──────────────────┐   ┌──────────────────────┐
│ Workload Module  │   │   Scenario Module    │
│ DeclarativeWL    │◄──│  (Orchestrator)      │
│ LuaWL (optional) │   │  HOW/WHEN to execute │
│ WHAT operations  │   └──────┬───────────────┘
└──────────────────┘          │
                              ▼
                       ┌──────────────────────┐
                       │  Rate Limiter Module │  ← TIME CONTROL
                       └──────────────────────┘
                              │
                              ▼
                       ┌──────────────────────┐
                       │   Runtime Module     │
                       │  (Async/Blocking)    │
                       └──────┬───────────────┘
                              │
                              ▼
                       ┌──────────────────────┐
                       │ Connection Pool      │
                       │     Module           │
                       └──────┬───────────────┘
                              │
                              ▼
                       ┌──────────────────────┐
                       │  Driver Module       │
                       │  (MySQL/Postgres)    │
                       └──────┬───────────────┘
                              │
                              ▼
                       [    Database    ]

[Parallel: Metrics Module, Event Module]

┌─────────────────────────────────────────────────────────────┐
│                     Directory Structure                     │
│                                                              │
│  config/              - Infrastructure configs (DB, runtime)│
│  scenarios/           - Test scenarios (executor, workload) │
│  workloads/           - Declarative YAML workloads          │
│                                                              │
│  Separation: Same workload → different rates/durations      │
│              Same scenario → different DB environments      │
└─────────────────────────────────────────────────────────────┘
```

### Execution Flow (Time-Driven)

```
ScenarioExecutor::execute()
 │
 ├─> Workload.prepare()
 │
 └─> LOOP (until duration elapsed):
      │
      ├─> RateLimiter.acquire()  ← TIME CONTROL POINT
      │    (blocks if no tokens available)
      │
      ├─> Workload.next_operation(ExecutionContext)
      │    (deterministic, seeded RNG)
      │
      ├─> Runtime.submit(Operation)
      │    │
      │    ├─> Semaphore.acquire()  ← BACKPRESSURE POINT
      │    │
      │    ├─> BackpressureMonitor.check()
      │    │    (record if saturated, DON'T HIDE)
      │    │
      │    ├─> ConnectionPool.get()
      │    │
      │    ├─> Connection.execute(sql, params)
      │    │
      │    └─> MetricsCollector.record_operation()
      │
      └─> iteration++
```

## Module Details

### 1. Config Module (`src/config/`)
- **Purpose**: Load, validate, and manage configuration
- **Key Types**: `ToolConfig`, `DatabaseConfig`, `RuntimeConfig`, `ScenarioConfig`
- **Formats**: YAML/TOML files, CLI args
- **Critical**: Immutable after loading, strong typing prevents invalid states

### 2. Workload Module (`src/workload/`)

**Purpose**: Generate database operations - the "WHAT" of testing

- **Trait**: `Workload` - all workload types implement this
- **Implementations**:
  - `DeclarativeWorkload` (PRIMARY - YAML-based, transparent, fully configurable)
  - `LuaWorkload` (for complex scenarios, optional, feature-gated)
  - `OltpReadWrite` (DEPRECATED - use declarative workloads instead)
- **Key Method**: `next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation>`
- **Critical**: Must be deterministic (same ExecutionContext → same Operation)

#### Workload vs Scenario (CRITICAL DISTINCTION)

**Workload = WHAT operations to run**
- Defines the business logic (SQL queries, transactions)
- Generates operation parameters (which IDs, which tables)
- Controls data distribution (uniform, zipfian, etc.)
- Manages schema (table definitions, columns, indexes)
- Is deterministic (same seed → same operations)

**Scenario = HOW/WHEN to execute**
- Controls execution lifecycle (prepare → run → cleanup)
- Manages execution rate (1000 ops/sec, ramping, etc.)
- Controls duration and stages
- Manages connections and runtime
- Coordinates timing across workers

**Analogy**: Workload is the CHEF (decides what to cook), Scenario is the MANAGER (decides when to cook, how fast to serve)

**Example**:
```yaml
# scenarios/my_test.yaml
scenario:
  executor:              # ← SCENARIO controls HOW/WHEN
    type: constant-rate
    rate: 1000          # Execute at 1000 ops/sec
    duration: 60s       # For 60 seconds

  workload:             # ← WORKLOAD controls WHAT
    type: declarative
    file: workloads/oltp_read_write.yaml  # Use this WHAT
```

The same workload can run in multiple scenarios:
- Development: 100 ops/sec for 10s (quick smoke test)
- Staging: 1000 ops/sec for 60s (standard test)
- Production: 10000 ops/sec for 300s (capacity test)

#### Declarative Workload Design

**Primary Format**: YAML files in `workloads/` directory

**Key Benefits**:
1. **Transparency**: Users see exactly what operations are executed
2. **Configurability**: Every aspect is customizable (schema, SQL, distributions)
3. **No Code Required**: Create workloads without writing Rust
4. **Version Control**: Workloads are text files, easy to diff and review
5. **Sysbench Compatibility**: Full feature parity with all sysbench parameters

**Workload Structure**:
```yaml
workload:
  name: my_workload
  description: "Optional description"

  # Schema definition
  schema:
    tables:
      - name: sbtest
        count: 10              # Creates sbtest1..sbtest10
        row_count: 10000       # Rows per table
        columns:
          - name: id
            type: INT
            primary_key: true
          - name: k
            type: INT
            index: k_idx
          - name: c
            type: CHAR(120)

  # Operations with weights
  operations:
    - name: point_select
      weight: 60              # 60% of operations
      type: read
      sql: "SELECT c FROM sbtest{table_id} WHERE id = ?"
      parameters:
        - name: table_id
          distribution:
            type: round_robin
            range: [1, "${table_count}"]
        - name: id
          distribution:
            type: uniform
            range: [1, "${row_count}"]

    - name: update_non_index
      weight: 40              # 40% of operations
      type: write
      sql: "UPDATE sbtest{table_id} SET c = ? WHERE id = ?"
      parameters:
        - name: table_id
          distribution:
            type: round_robin
            range: [1, "${table_count}"]
        - name: c
          generator:
            type: string
            template: "{iteration:0>120}"
        - name: id
          distribution:
            type: uniform
            range: [1, "${row_count}"]
```

**Built-in Declarative Workloads**:
- `workloads/oltp_read_write.yaml` - Balanced 60/40 read/write (default sysbench)
- `workloads/oltp_read_only.yaml` - Read-only queries (various SELECT patterns)
- `workloads/oltp_write_only.yaml` - Write-only (UPDATE, DELETE, INSERT)
- `workloads/oltp_point_select.yaml` - Pure point select (100% reads)

**Distribution Strategies**:
- `uniform` - Random uniform distribution
- `round_robin` - Deterministic round-robin (for table selection)
- `zipfian` - Zipfian distribution (hot keys)
- `gaussian` - Normal distribution
- `sequential` - Sequential access

**Parameter Generators**:
- `integer` - Integer values from distribution
- `string` - String generation with templates
- `decimal` - Decimal/float values
- `choice` - Pick from predefined list

**Overriding Workload Parameters**:
```yaml
# scenarios/custom_test.yaml
scenario:
  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml
    overrides:
      schema:
        tables:
          - count: 20          # Override: 20 tables instead of 10
            row_count: 100000  # Override: 100k rows instead of 10k
      operations:
        - name: point_select
          weight: 90          # Override: 90% reads instead of 60%
        - name: update_non_index
          weight: 10          # Override: 10% writes instead of 40%
```

**Migration from Builtin to Declarative**:

❌ **Old (Deprecated)**:
```yaml
workload:
  type: builtin
  name: oltp_read_write
  table_count: 10
  table_size: 10000
```

✅ **New (Recommended)**:
```yaml
workload:
  type: declarative
  file: workloads/oltp_read_write.yaml
  overrides:
    schema:
      tables:
        - count: 10
          row_count: 10000
```

**When to Use Lua vs Declarative**:
- **Declarative**: 80% of use cases (standard OLTP patterns, custom queries)
- **Lua**: Complex scenarios (conditional logic, stateful transactions, advanced correlation)

See `docs/workload-design.md` for complete specification.

### 3. Rate Limiter Module (`src/rate_limiter.rs`)
- **Algorithm**: Token bucket
- **Key Method**: `async fn acquire(&mut self) -> Result<Permit>`
- **Critical**: Time-driven, yields when no tokens (never busy-waits)
- **Dynamic**: Can adjust rate for ramping executors

### 4. Runtime Module (`src/runtime/`)

**Purpose**: Async execution engine for database operations with dual-source backpressure monitoring

**Status**: ✅ M0 Complete - Phase 1-5 finished (2025-12-27)

#### Key Components

- **Trait**: `RuntimeEngine` - Core interface for operation execution
- **Implementation**: `AsyncRuntime` - Primary async implementation with Tokio
- **Internal**: `BackpressureMonitor` - Dual-source saturation detection

#### Architecture Philosophy

The runtime uses a **semaphore-based concurrency model** instead of thread pools:

```
Traditional (sysbench):
  N threads → N concurrent operations
  Threads block on I/O → wasted CPU
  ~8 MB memory per thread

RSBench (AsyncRuntime):
  M OS threads (M << N) → 10K+ async tasks
  Tasks yield on I/O → efficient CPU use
  ~2 KB memory per task
  Semaphore limits concurrency
```

**Why semaphores?**
- Decouple concurrency from load (time-driven execution)
- Prevent resource exhaustion (memory, connections)
- Enable high throughput (100K+ ops/sec)

#### Critical Design Decision: Dual-Source Backpressure Monitoring

**The Problem**: Traditional tools only monitor connection pool utilization, missing semaphore saturation.

**Scenario where this matters**:
```
Pool:      100 connections, 50 in use  → 50% utilization ✅
Semaphore: 100 permits,     95 in use  → 95% utilization ⚠️

Old approach (pool-only):  No backpressure detected ❌
New approach (dual-source): Backpressure detected ✅
```

**Implementation**:
```rust
fn is_saturated(&self, stats: &RuntimeStats) -> bool {
    let pool_saturated = stats.pool_utilization > self.pool_threshold;
    let sem_saturated = stats.semaphore_utilization > self.semaphore_threshold;
    pool_saturated || sem_saturated  // Check BOTH
}
```

**Performance validated** (from benches/runtime_bench.rs):
- Backpressure check: 863-871 picoseconds (~0.87 ns)
- Overhead vs pool-only: +6 picoseconds (1.8%)
- Throughput: Billions of checks per second

#### Error Handling Philosophy: Errors as Data

**Key Decision**: Database errors are NOT Rust `Err` values.

```rust
// ✅ CORRECT: Database errors as data
async fn submit(&self, op: Operation) -> Result<OperationResult> {
    match conn.execute(&op.sql).await {
        Ok(result) => Ok(OperationResult { success: true, ... }),
        Err(e) => Ok(OperationResult {
            success: false,
            error: Some(e.to_string()),
            ...
        }),
    }
}

// ❌ WRONG: Database errors as control flow
async fn submit(&self, op: Operation) -> Result<OperationResult> {
    conn.execute(&op.sql).await?  // Error propagates up
}
```

**Why?** The Scenario module treats database errors as **metrics to measure**, not control flow to handle. Only infrastructure errors (pool exhausted, connection failed) become Rust `Err`.

#### Best Practices

**1. Configuration Guidelines**

```rust
// Recommended: max_connections = 2-10x pool size
let runtime = create_runtime(
    pool.clone(),
    500,    // High concurrency for OLTP workloads
    0.8,    // Alert at 80% saturation
    metrics,
);

// Conservative: For heavy queries or limited resources
let runtime = create_runtime(
    pool.clone(),
    50,     // Lower concurrency
    0.7,    // Earlier backpressure detection
    metrics,
);
```

**2. Monitoring Runtime Health**

```rust
let stats = runtime.stats();

// Check for client saturation
if stats.backpressure_active {
    eprintln!("⚠️  CLIENT IS THE BOTTLENECK!");
    eprintln!("Pool: {:.1}%", stats.pool_utilization * 100.0);
    eprintln!("Semaphore: {:.1}%", stats.semaphore_utilization * 100.0);
}
```

**3. Shutdown Gracefully**

```rust
// Wait for in-flight operations before shutdown
runtime.shutdown().await?;
```

#### Performance Targets (All Met ✅)

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Throughput | 100K+ ops/sec | 100K+ | ✅ |
| Submit overhead | <10μs | <10μs | ✅ |
| Backpressure check | <1ns | 0.87ns | ✅ 11x better |
| Stats calculation | <100ns | 5ns | ✅ 20x better |
| Memory per operation | <10KB | ~2KB | ✅ 5x better |

#### Testing & Validation

- **Unit tests**: 18/18 passing (src/runtime/)
- **Integration tests**: 13/13 passing (tests/integration/runtime_integration_test.rs)
- **Benchmarks**: benches/runtime_bench.rs (4 benchmark groups)
- **Documentation**: Full rustdoc coverage with examples

#### Key Invariants (Must Maintain)

1. **Backpressure visibility** - Client saturation is ALWAYS observable
2. **Non-blocking I/O** - All operations use async/await
3. **Errors as data** - Database errors in OperationResult, not Rust Result
4. **Dual-source monitoring** - Check both pool AND semaphore
5. **Semaphore >= pool** - `max_connections >= pool.max_size` (otherwise pool is bottleneck)

#### Future Enhancements (M1+)

**M1 Features**:
- Operation timeouts (configurable per operation type)
- Retry logic for transient errors
- Better backpressure metrics (sustained vs transient)

**M2 Features**:
- Circuit breakers for failing databases
- Adaptive concurrency control
- Per-operation-type semaphores

#### References

- Design: `docs/runtime-design.md`
- Performance validation: `docs/runtime-phase1-performance.md`
- Rustdoc: `cargo doc --no-deps --open` → `rsbench::runtime`

### 5. Connection Pool Module (`src/pool/`)

**Purpose**: Production-grade connection pooling with health monitoring and backpressure visibility

**Status**: ✅ **M1 Complete** - Full deadpool integration with comprehensive testing

#### Architecture

Uses [`deadpool`](https://docs.rs/deadpool) 0.12 with custom `DriverManager` adapter:

```text
ConnectionPool
  │
  ├─> deadpool::Pool<DriverManager>
  │     │
  │     ├─> Connection 1 (idle)
  │     ├─> Connection 2 (active)
  │     └─> Connection N (idle)
  │
  ├─> PoolConfig (max_size, min_size, timeouts)
  └─> Health Metrics (checkouts, errors, lifetime)
```

**Key Components**:
- **`ConnectionPool`** (`mod.rs`): Main pool interface with health monitoring
- **`DriverManager`** (`manager.rs`): Adapter between deadpool and our DatabaseDriver trait
- **`PooledConnection`**: Wrapper that auto-returns connection on drop

#### Why Connection Pooling?

Connection creation is expensive (~10-50ms):
- TCP handshake: ~1-5ms
- Authentication: ~5-20ms
- Session setup: ~5-10ms

Pool checkout is fast (~400ns):
- **1000x faster** than creating new connections
- Amortizes expensive setup across thousands of operations
- Predictable resource usage via max_size limit

#### Configuration

```yaml
database:
  pool:
    min_size: 5              # Pre-warmed connections (baseline concurrency)
    max_size: 100            # Hard limit (prevents DB overload)
    connection_timeout: 5s   # Timeout waiting for available connection
    idle_timeout: 600s       # Close idle connections after 10 minutes
```

**Configuration Guidelines**:
- **min_size**: 10-20% of max_size, matches expected baseline load
- **max_size**: Match DB's max_connections, leave headroom for other clients
- **connection_timeout**: Short (1-5s) for fast-fail, longer (10-30s) for bursty workloads
- **idle_timeout**: Balance reuse vs resource cleanup (5-30 minutes)

#### Health Monitoring

Automatic health maintenance:
1. **Pre-checkout ping**: Every connection pinged before reuse (via `recycle()`)
2. **Broken detection**: Failed pings trigger transparent connection replacement
3. **Error tracking**: All errors logged and counted in pool stats
4. **Observability**: Full tracing integration for debugging

```rust
// Health stats exposed for runtime monitoring
let stats = pool.stats();
println!("Active: {}, Errors: {}, Checkouts: {}",
    stats.active_connections,
    stats.connection_errors,
    stats.total_checkouts
);
```

#### Backpressure Visibility

Pool stats expose critical backpressure signals:
- **`pending_requests > 0`**: Pool saturated, clients waiting
- **`active == max_size`**: Pool at capacity
- **`connection_errors`**: Connection stability issues

Runtime uses these signals for backpressure detection and adaptive load control.

#### Performance Characteristics

Benchmarked with mock driver (`benches/connection_pool_bench.rs`):
- **Checkout latency**: ~400ns (p50/p99) - **24x better than 10μs target**
- **Concurrent throughput**: 476K-690K checkouts/sec - **4.8-6.9x better than 100K target**
- **Pool saturation recovery**: ~10μs per connection (linear scaling)
- **Stats overhead**: ~23ns (essentially free)

Real-world performance is dominated by network/query latency, but pool overhead is negligible.

#### Implementation Decisions

1. **Deadpool over custom implementation**
   - Battle-tested in production (used by actix-web, sqlx, etc.)
   - Handles edge cases (timeouts, lifecycle, recycling)
   - Active maintenance and security updates

2. **Health check on every checkout**
   - Adds ~1ms overhead but prevents broken connection errors
   - Critical for reliability - never hand out stale connections
   - Acceptable tradeoff for database testing tool

3. **Atomic metrics for lock-free stats**
   - `total_checkouts`, `connection_errors` use `AtomicUsize`
   - Stats collection is essentially free (~23ns)
   - No contention on hot paths

4. **Tokio runtime integration**
   - Required `.runtime(Runtime::Tokio1)` for timeout support
   - See GitHub issue [deadpool#195](https://github.com/deadpool-rs/deadpool/issues/195)
   - Enables proper async timeout behavior

#### Transaction Support

`PooledConnection` exposes full transaction API:
```rust
let mut conn = pool.get().await?;
conn.begin().await?;
conn.execute("INSERT INTO users VALUES (?)", &[val]).await?;
conn.commit().await?;  // or rollback()
```

Transactions work transparently through the pooled connection wrapper.

#### Testing

**Unit Tests** (`src/pool/mod.rs`):
- Pool creation and configuration validation
- Connection checkout/return lifecycle
- Health metrics tracking
- Timeout enforcement
- Warm-up behavior

**Integration Tests** (`tests/integration/pool_mysql_test.rs`):
- Real MySQL connection pooling (8 tests)
- Concurrent access patterns
- Connection reuse verification
- Pool saturation behavior
- Transaction support
- Prepared statements

**Benchmarks** (`benches/connection_pool_bench.rs`):
- Checkout latency (single/concurrent)
- Throughput under load
- Pool saturation recovery
- Warm-up performance

**Coverage**: All phases complete (Design → Implementation → Testing → Benchmarks → Documentation)

#### Critical Design Principle

> "The pool is a shared resource - its health MUST be visible to the runtime for backpressure detection"

Every pool operation updates observable metrics. The runtime monitors these signals to detect client-side bottlenecks and distinguish them from database performance issues.

### 6. Driver Module (`src/driver/`)
- **Trait**: `DatabaseDriver`, `Connection`
- **Implementations**: MySQL (M0), PostgreSQL (M1)
- **Registry**: Compile-time registration via feature flags
- **Critical**: Thin adapters, no business logic in drivers

### 7. Metrics Module (`src/metrics/`)
- **Collector**: Lock-free using DashMap + atomics
- **Histograms**: HDR histograms for accurate percentiles
- **Outputs**: Text (sysbench-compatible), JSON
- **Critical**:
  - Track client-side metrics (backpressure events, pool utilization)
  - Separate client bottlenecks from DB performance

### 8. Scenario Module (`src/scenario.rs`)
- **Purpose**: Orchestrate workload execution with pluggable executor patterns
- **Executors**:
  - **Open-Loop** (M0): Time-driven, fire-and-forget submission
    - `ConstantRate`: Fixed ops/sec (e.g., rate: 1000)
    - `RampingRate`: Staged rate changes (capacity testing)
  - **Closed-Loop** (M0/M1): Worker-driven sequential execution
    - Equivalent to sysbench `--threads=N` (each worker waits for completion)
    - Natural backpressure modeling (slow queries → lower throughput)
- **Distributed Mode** (M1+): M:N architecture
  - M clients (1 leader + M-1 workers) coordinate to drive load
  - N database endpoints (multi-region, read replicas)
  - Leader orchestrates phases, aggregates metrics
  - Routing strategies: region-affinity, cross-region, read-write-split
- **Critical**:
  - Time-driven scheduling via rate limiter (open-loop)
  - Worker-driven execution (closed-loop)
  - Event integration for lifecycle testing

## Milestone Status

### Milestone 0 (Current): MVP Foundation
**Goal**: Drop-in sysbench replacement with time-driven, rate-based execution

**Status**: ✅ Structure Complete, 🚧 Implementation In Progress

**Deliverables**:
- ✅ Rate-based execution (open model)
- ✅ Async runtime with backpressure monitoring
- ✅ MySQL driver
- ✅ HDR histogram metrics
- ✅ Declarative workload design (YAML format defined)
- ✅ All sysbench OLTP tests as YAML files
- ✅ Configuration separation (infrastructure vs scenarios)
- ✅ Sysbench Lua compatibility (structure)
- 🚧 DeclarativeWorkload implementation (design complete, code pending)
- 🚧 Text and JSON output (structure complete)
- 🚧 OLTP workload data loading
- 🚧 Full connection pooling

**Recent Progress**:
- ✅ Designed and documented declarative workload system
- ✅ Created 4 declarative YAML workloads (oltp_read_write, oltp_read_only, oltp_write_only, oltp_point_select)
- ✅ Migrated all scenarios to use declarative format
- ✅ Added comprehensive unit tests (109 tests passing)
- ✅ Fully implemented config module with validation and merging
- ✅ Fully implemented CLI module with config loading

**Success Criteria**:
- [ ] Can run sysbench oltp_read_write equivalent (workload design ready, implementation pending)
- [x] Rate-based execution maintains target QPS (implemented)
- [x] Backpressure visible in metrics (implemented)
- [x] No coordinated omission (design prevents)
- [ ] Async mode: 10x higher throughput (to be benchmarked)
- [x] Deterministic: same seed → same operations (implemented)

**Out of Scope for M0**:
- Multi-scenario execution
- Distributed mode
- External events
- Tenant tagging
- Thresholds/checks

### Milestone 1: Distributed Awareness
**Goal**: Multi-region testing with essential workload flexibility

**Key Features**:
- Multi-endpoint routing (read/write split, region-aware)
- K8s event integration
- Declarative YAML workloads
- PostgreSQL driver
- Data generation engine
- Loose coordination distributed mode

## Implementation Guidelines

### When Adding New Code

1. **Start Simple**
   - Does this solve the immediate problem? (Must be yes)
   - Am I building for hypothetical future needs? (Should be no)
   - Is this the simplest solution that works? (Should be yes)
   - Have I tried the obvious approach first? (Should be yes)

2. **Always Consider Time-Driven Nature**
   - Is this controlled by time/rate or by threads? (Must be time)
   - Does this respect the rate limiter?

3. **Always Track Backpressure**
   - If this can saturate, is it visible?
   - Are we hiding client-side limits?

4. **Always Maintain Determinism**
   - Does this use wall-clock time? (Only for pacing, never for operation generation)
   - Does this use unseeded randomness? (Must use ChaCha8Rng with seed)
   - Can this produce different results with same seed? (Should not)

5. **Use Traits Only When Needed**
   - Do I have 2+ concrete implementations? (If no, maybe don't need trait yet)
   - Is this a clear extension point? (DatabaseDriver, Workload, etc.)
   - Am I creating this trait "just in case"? (Don't do this)

6. **Always Prefer Async**
   - Is this I/O-bound? (Use async)
   - Is this CPU-bound and in hot path? (Consider sync, but measure)

### Code Style

- **Error Handling**: Use `Result<T>` everywhere, thiserror for errors
- **Async**: Use `#[async_trait::async_trait]` for async traits
- **Metrics**: Lock-free collection (DashMap, atomics)
- **Logging**: Use `tracing` crate
- **Testing**: Determinism property tests with proptest

### Performance Considerations

**Hot Paths** (Optimize These):
1. Operation generation (`Workload::next_operation`)
2. Rate limiting (`RateLimiter::acquire`)
3. Operation execution (`Runtime::submit`)
4. Metrics recording (`MetricsCollector::record_operation`)

**Optimization Rules**:
- Minimize allocations in hot paths
- Use lock-free data structures
- Avoid cloning (use references)
- Pre-allocate buffers

### Testing Requirements

- **Unit Tests**: Every public function
- **Integration Tests**: End-to-end scenarios
- **Property Tests**: Determinism (same seed → same result)
- **Benchmark Tests**: Performance regression detection

## Key Invariants (Must Never Violate)

1. **Determinism**: `∀ seed, config: execute(seed, config) == execute(seed, config)`
2. **Rate Accuracy**: `actual_rate ≈ target_rate (±5%)` over 10s windows
3. **Backpressure Visibility**: `IF pool_utilization > threshold THEN backpressure_events > 0`
4. **No Coordinated Omission**: Rate limiter ALWAYS controls timing
5. **Time-Driven**: Operation pacing NEVER depends on thread speed

## Common Patterns

### Creating a New Workload (Declarative - Recommended)

**Step 1**: Create YAML file in `workloads/` directory
```yaml
# workloads/my_app.yaml
workload:
  name: my_application
  description: "Custom workload for my application"

  schema:
    tables:
      - name: users
        count: 1
        row_count: 100000
        columns:
          - name: user_id
            type: INT
            primary_key: true
          - name: email
            type: VARCHAR(255)
            index: email_idx
          - name: created_at
            type: TIMESTAMP

  operations:
    - name: get_user_by_id
      weight: 70
      type: read
      sql: "SELECT * FROM users WHERE user_id = ?"
      parameters:
        - name: user_id
          distribution:
            type: uniform
            range: [1, 100000]

    - name: update_email
      weight: 20
      type: write
      sql: "UPDATE users SET email = ? WHERE user_id = ?"
      parameters:
        - name: email
          generator:
            type: string
            template: "user{iteration}@example.com"
        - name: user_id
          distribution:
            type: uniform
            range: [1, 100000]

    - name: find_by_email
      weight: 10
      type: read
      sql: "SELECT * FROM users WHERE email = ?"
      parameters:
        - name: email
          generator:
            type: string
            template: "user{iteration}@example.com"
```

**Step 2**: Use in scenario
```yaml
# scenarios/my_test.yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  workload:
    type: declarative
    file: workloads/my_app.yaml
```

**Step 3**: Run
```bash
rsbench --scenario scenarios/my_test.yaml
```

### Creating a New Workload (Rust - For Complex Scenarios)

**Only needed when declarative YAML is insufficient (e.g., complex stateful logic)**

```rust
pub struct MyWorkload {
    rng: ChaCha8Rng,
    // ... state
}

impl Workload for MyWorkload {
    fn prepare(&mut self, ctx: &mut PrepareContext) -> Result<()> {
        // Setup (create tables, etc.)
    }

    fn next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation> {
        // Generate operation deterministically
        // Use self.rng for randomness, ctx for deterministic state
    }

    fn cleanup(&mut self) -> Result<()> {
        // Cleanup
    }

    fn name(&self) -> &str {
        "my_workload"
    }
}
```

### Adding a New Database Driver
```rust
pub struct MyDbDriver;

#[async_trait::async_trait]
impl DatabaseDriver for MyDbDriver {
    fn name(&self) -> &str { "mydb" }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        // Create connection
    }

    fn capabilities(&self) -> DriverCapabilities {
        // Return capabilities
    }
}

// Register in DriverRegistry::new()
#[cfg(feature = "mydb")]
registry.register(Arc::new(MyDbDriver::new()));
```

### Adding a New Executor Type
```rust
// In ExecutorConfig enum
pub enum ExecutorConfig {
    // ... existing
    MyExecutor {
        param1: u64,
        param2: Duration,
    },
}

// In ScenarioExecutor::execute()
match &self.config.executor {
    // ... existing
    ExecutorConfig::MyExecutor { param1, param2 } => {
        self.execute_my_executor(*param1, *param2).await
    }
}
```

## File References

- **HLD**: `rsbench_hld.txt` - High-level design document
- **MLD**: `rsbench_mld.txt` - Module-level design document
- **API Spec**: `docs/api_spec_m0.md` - Detailed API specification
- **Structure**: `docs/project_structure.md` - Module organization
- **Progress**: `docs/m0_progress_summary.md` - Current status
- **Scenario Design**: `docs/scenario-design.md` - Comprehensive scenario module design
  - Section 2.4: Worker Architecture and Scalability
  - Section 5.3.2: Backpressure Awareness Deep Dive
  - Section 5.6: Event Module Integration (Parallel Module Design)
  - Section 11.1: Closed-Loop Executor (BlockingRuntime Replacement)
  - Section 11.8: Distributed M:N Architecture with Loose Coordination
  - Appendix C: Sysbench Migration Guide
- **Executor Pattern Decision**: `docs/executor-pattern-decision.md` - Architectural Decision Record
- **Documentation Updates**: `docs/DOCUMENTATION_UPDATES.md` - Summary of all doc changes
- **Workload Design**: `docs/workload-design.md` - Declarative workload specification
- **Migration Guide**: `docs/declarative-workload-migration.md` - Builtin to declarative migration
- **Config Guide**: `config/README.md` - Infrastructure configuration guide
- **Scenario Guide**: `scenarios/README.md` - Test scenario guide
- **Workload Guide**: `workloads/README.md` - Workload creation guide

## Quick Commands

```bash
# Build (default: MySQL only)
cargo build

# Build with Lua (requires LuaJIT)
cargo build --features lua

# Run tests
cargo test

# Run with config
cargo run -- --config examples/basic_config.yaml run

# Check without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

## Common Issues & Solutions

### Issue: "LuaJIT not found"
**Solution**: Build without lua feature: `cargo build` (default)
Or install LuaJIT and build with: `cargo build --features lua`

### Issue: "Backpressure not visible"
**Check**:
- Is BackpressureMonitor being called?
- Are events being recorded in MetricsCollector?
- Is threshold set correctly?

### Issue: "Non-deterministic results"
**Check**:
- All RNG uses ChaCha8Rng with seed from config
- No wall-clock time in operation generation
- ExecutionContext used for deterministic state

### Issue: "Rate not maintained"
**Check**:
- RateLimiter being called before each operation?
- Token bucket capacity sufficient?
- Client not saturated? (check backpressure events)

### Issue: "Code is getting complex"
**Solution**:
- Stop and ask: "Is this complexity required for current milestone?"
- If no: Remove it. Do the simplest thing that works.
- If yes: Is there a simpler way? Try the obvious approach first.
- Remember: You can always add complexity later. Removing it is painful.

## Important Notes for Future Sessions

1. **START SIMPLE** - If you're adding complexity, ask "does M0 require this?" If no, don't add it
2. **Always respect time-driven design** - If you're tempted to use threads for load control, you're doing it wrong
3. **Never hide backpressure** - Make it visible, don't abstract it away
4. **Determinism is critical** - Every random decision must be seeded
5. **Avoid over-engineering** - Three uses before abstraction, copy-paste beats wrong abstraction
6. **M0 before M1** - Complete Milestone 0 before adding M1 features. Don't build M1 features early
7. **Read the HLD/MLD** for detailed design rationale if uncertain
8. **Test determinism** - Property tests are essential
9. **Optimize hot paths** - Profile before optimizing elsewhere
10. **Simple > Clever** - Obvious code beats clever code every time
11. **Declarative-First for Workloads** - Use YAML for workloads unless you need complex logic (then use Lua). Don't create Rust workloads unless absolutely necessary.
12. **Workload vs Scenario** - NEVER confuse these:
    - Workload = WHAT operations (SQL, parameters, distributions)
    - Scenario = HOW/WHEN (rate, duration, executor type)
    - Same workload can run at different rates/durations
    - Same scenario can run against different databases
13. **Configuration Separation** - NEVER mix infrastructure config (database, runtime) with scenario config (workload, executor)
14. **Builtin workloads are DEPRECATED** - Always migrate to declarative YAML format. See `docs/declarative-workload-migration.md`

---

**Project Version**: M0 Alpha
**Last Updated**: 2025-12-27
**Status**: Module structure complete, implementation in progress

**Recent Updates (2025-12-27)**:
- ✅ **Removed BlockingRuntime** - Simplified runtime module to async-only execution
  - Deleted `src/runtime/blocking.rs` (86 lines)
  - Removed `RuntimeMode::Blocking` enum variant from config
  - Simplified runtime creation (direct function call instead of factory pattern)
  - Rationale: BlockingRuntime didn't provide true sysbench-style blocking semantics
    - Just wrapped async operations in `spawn_blocking` threads
    - No semantic difference from AsyncRuntime for scenario module (both fire-and-forget)
    - Closed-loop executor (M0/M1) provides the worker-driven sequential execution pattern
  - Benefits: Simpler codebase, clearer semantics, single execution model
  - **Note**: 5 pre-existing ClosedLoop executor test failures (unrelated to this change)
    - Bug: `RateLimiter::new(u64::MAX)` causes overflow in `rate * 2`
    - Fix deferred: Make rate_limiter optional for ClosedLoop executor

**Recent Updates (2025-12-26)**:
- ✅ Added comprehensive backpressure awareness documentation
- ✅ Added M:N distributed architecture design with loose coordination philosophy (M1 feature)
- ✅ Added Event Module integration as parallel module (not nested)
- ✅ Clarified worker model (async tasks vs OS threads)
- ✅ Added executor pattern decision (closed-loop vs open-loop)
- ✅ Created Sysbench migration guide with complete command mapping
- ✅ Documented loose coordination design rationale (simplicity, fault tolerance, performance)
