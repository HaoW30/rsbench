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

### Module Organization

```
┌─────────────────────────────────────────────────────────────┐
│                         CLI Module                          │
│  Parse args, load config, init system                      │
└────────────────────────────┬────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────┐
│                     Config Module                           │
│  Config validation, defaults, types                         │
└──────┬──────────────────────┬───────────────────────────────┘
       │                      │
       ▼                      ▼
┌──────────────┐      ┌──────────────────────┐
│   Workload   │      │   Scenario Module    │
│    Module    │◄─────│  (Orchestrator)      │
└──────┬───────┘      └──────┬───────────────┘
       │                     │
       │                     ▼
       │              ┌──────────────────────┐
       │              │  Rate Limiter Module │  ← TIME CONTROL
       │              └──────────────────────┘
       │                     │
       │                     ▼
       │              ┌──────────────────────┐
       │              │   Runtime Module     │
       │              │  (Async/Blocking)    │
       └──────────────┤                      │
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
- **Trait**: `Workload` - all workload types implement this
- **Implementations**:
  - `OltpReadWrite` (builtin, sysbench equivalent)
  - `LuaWorkload` (optional, feature-gated)
- **Key Method**: `next_operation(&mut self, ctx: &ExecutionContext) -> Result<Operation>`
- **Critical**: Must be deterministic (same ExecutionContext → same Operation)

### 3. Rate Limiter Module (`src/rate_limiter.rs`)
- **Algorithm**: Token bucket
- **Key Method**: `async fn acquire(&mut self) -> Result<Permit>`
- **Critical**: Time-driven, yields when no tokens (never busy-waits)
- **Dynamic**: Can adjust rate for ramping executors

### 4. Runtime Module (`src/runtime/`)
- **Trait**: `RuntimeEngine`
- **Implementations**:
  - `AsyncRuntime` (primary): Backpressure-aware, high throughput
  - `BlockingRuntime` (compatibility): Sysbench-style
- **Critical**:
  - Track backpressure events
  - Never hide saturation
  - Semaphore limits in-flight operations

### 5. Connection Pool Module (`src/pool/`)
- **Purpose**: Manage database connections
- **M0**: Simplified (creates new connections each time)
- **Future**: Full deadpool integration with health checking
- **Critical**: Provide backpressure signals via stats

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
- **Purpose**: Orchestrate workload execution
- **Executors** (M0):
  - `ConstantRate`: Fixed ops/sec
  - `RampingRate`: Staged rate changes
- **Critical**: Time-driven scheduling via rate limiter

## Milestone Status

### Milestone 0 (Current): MVP Foundation
**Goal**: Drop-in sysbench replacement with time-driven, rate-based execution

**Status**: ✅ Structure Complete, 🚧 Implementation In Progress

**Deliverables**:
- ✅ Rate-based execution (open model)
- ✅ Async runtime with backpressure monitoring
- ✅ MySQL driver
- ✅ HDR histogram metrics
- ✅ Sysbench Lua compatibility (structure)
- 🚧 Text and JSON output (structure complete)
- 🚧 OLTP workload data loading
- 🚧 Full connection pooling

**Success Criteria**:
- [ ] Can run sysbench oltp_read_write equivalent
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

### Creating a New Workload
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

---

**Project Version**: M0 Alpha
**Last Updated**: 2025-12-24
**Status**: Module structure complete, implementation in progress
