# Runtime Module Design Document

**Module:** Runtime Engine
**Version:** 1.0
**Status:** Design + Implementation Planning
**Last Updated:** 2025-12-27

---

## Table of Contents

1. [Overview](#overview)
2. [Current State Analysis](#current-state-analysis)
3. [Design Goals](#design-goals)
4. [Architecture](#architecture)
5. [Detailed Design](#detailed-design)
6. [Implementation Plan](#implementation-plan)
7. [Testing Strategy](#testing-strategy)
8. [Performance Targets](#performance-targets)
9. [Future Enhancements](#future-enhancements)

---

## Overview

### Purpose

The Runtime module is the **execution engine** for database operations in RSBench. It sits between the Scenario module (which generates operations) and the Database drivers (which execute them).

**Key Responsibilities:**
1. **Execute operations** submitted by the Scenario module
2. **Control concurrency** via semaphore-based limiting
3. **Monitor backpressure** and track client saturation
4. **Integrate with connection pool** for database connectivity
5. **Collect metrics** for every operation
6. **Graceful shutdown** with proper cleanup

### Design Philosophy

```
┌─────────────────────────────────────────────────────────┐
│  "The Runtime module MUST NEVER hide client saturation" │
│                                                          │
│  If the client is the bottleneck, the user MUST know.  │
└─────────────────────────────────────────────────────────┘
```

**Core Principles:**
1. **Backpressure Visibility** - Client saturation is observable, never hidden
2. **Non-Blocking Execution** - Async I/O for high concurrency
3. **Simple & Focused** - Does one thing well: execute operations with monitoring
4. **No Coordinated Omission** - Works with time-driven scenario execution

---

## Current State Analysis

### What We Have (M0)

**File Structure:**
```
src/runtime/
├── mod.rs              # Trait + factory function
└── async_runtime.rs    # AsyncRuntime implementation
```

**Key Components:**

1. **RuntimeEngine Trait** (`mod.rs:18-27`)
```rust
#[async_trait::async_trait]
pub trait RuntimeEngine: Send + Sync {
    async fn submit(&self, op: Operation) -> Result<OperationResult>;
    fn stats(&self) -> RuntimeStats;
    async fn shutdown(&mut self) -> Result<()>;
}
```

2. **AsyncRuntime** (`async_runtime.rs:13-34`)
```rust
pub struct AsyncRuntime {
    pool: Arc<ConnectionPool>,
    semaphore: Arc<Semaphore>,              // Concurrency control
    backpressure_monitor: BackpressureMonitor,
    metrics: Arc<MetricsCollector>,
}
```

3. **BackpressureMonitor** (`async_runtime.rs:108-121`)
```rust
struct BackpressureMonitor {
    threshold: f64,  // e.g., 0.8 = 80% pool utilization
}
```

### What Works Well ✅

1. **Clean separation of concerns** - Runtime doesn't know about scenarios or workloads
2. **Async-first design** - Non-blocking I/O for high concurrency
3. **Backpressure monitoring** - Tracks when client is saturated
4. **Semaphore-based limiting** - Prevents unbounded concurrency
5. **Simple trait** - Easy to test and extend

### Gaps & Improvements Needed 🔧

| Issue | Impact | Priority |
|-------|--------|----------|
| **No unit tests in async_runtime.rs** | Can't verify correctness | HIGH |
| **Backpressure logic is simplistic** | Only checks pool utilization, not semaphore pressure | MEDIUM |
| **No timeout support** | Operations can hang indefinitely | MEDIUM |
| **Shutdown doesn't wait for in-flight ops** | May drop operations on shutdown | LOW |
| **Stats queued_operations is misleading** | Returns available permits, not queued | LOW |
| **No retry logic** | Transient errors fail immediately | LOW (M1) |

---

## Design Goals

### Milestone 0 (Current)

**Primary Goal:** Reliable, backpressure-aware operation execution

**Success Criteria:**
- [ ] Execute database operations correctly
- [ ] Detect client saturation (>80% pool utilization OR semaphore exhausted)
- [ ] Record backpressure events accurately
- [ ] Handle errors gracefully
- [ ] Support 10K+ concurrent operations via async tasks
- [ ] Comprehensive unit tests (80%+ coverage)
- [ ] Integration tests with real/mock database

**Out of Scope:**
- Retry logic (M1)
- Operation timeouts (M1)
- Circuit breakers (M2)
- Adaptive concurrency (M2)

### Milestone 1 (Future)

**Enhanced Features:**
- Operation timeouts (configurable per operation type)
- Retry logic for transient errors
- Better backpressure metrics (semaphore queue depth)
- Graceful degradation under pressure

### Milestone 2 (Future)

**Advanced Features:**
- Circuit breakers for failing databases
- Adaptive concurrency control
- Per-operation-type semaphores
- Resource quotas and fairness

---

## Architecture

### High-Level Flow

```
┌─────────────────┐
│  Scenario       │
│  Module         │
└────────┬────────┘
         │ submit(Operation)
         ▼
┌─────────────────────────────────────────────────────────┐
│              Runtime Module (AsyncRuntime)              │
│                                                          │
│  1. Acquire Semaphore Permit                            │
│     └─> Controls max concurrent operations              │
│                                                          │
│  2. Check Backpressure                                  │
│     ├─> Pool utilization > threshold?                   │
│     └─> If YES → record_backpressure_event()            │
│                                                          │
│  3. Get Connection from Pool                            │
│     └─> ConnectionPool.get()                            │
│                                                          │
│  4. Execute Operation                                   │
│     ├─> Start timer                                     │
│     ├─> conn.execute(sql, params)                       │
│     └─> Measure duration                                │
│                                                          │
│  5. Record Metrics                                      │
│     └─> metrics.record_operation(name, duration, result)│
│                                                          │
│  6. Return Result                                       │
│     └─> OperationResult { success, duration, ... }      │
│                                                          │
└─────────────────────────────────────────────────────────┘
         │
         ▼
    MetricsCollector
```

### Component Interactions

```
┌──────────────┐      submit()      ┌─────────────────┐
│   Scenario   │ ──────────────────> │  RuntimeEngine  │
└──────────────┘                     │   (trait)       │
                                     └────────┬────────┘
                                              │
                                              │ implements
                                              ▼
                                     ┌─────────────────┐
                                     │  AsyncRuntime   │
                                     └────────┬────────┘
                                              │
                   ┌──────────────────────────┼──────────────────────────┐
                   │                          │                          │
                   ▼                          ▼                          ▼
          ┌────────────────┐       ┌───────────────────┐       ┌────────────────┐
          │   Semaphore    │       │ ConnectionPool    │       │ Backpressure   │
          │   (concurrency)│       │   (DB conns)      │       │   Monitor      │
          └────────────────┘       └───────────────────┘       └────────────────┘
                                              │                          │
                                              ▼                          ▼
                                     ┌───────────────────┐       ┌────────────────┐
                                     │ DatabaseDriver    │       │ MetricsCollector│
                                     └───────────────────┘       └────────────────┘
```

### Data Flow

```
Operation (from Scenario)
    ↓
[Semaphore Acquire] ← Backpressure Point #1 (concurrency limit)
    ↓
[Backpressure Check] → If saturated, record event
    ↓
[Pool.get()] ← Backpressure Point #2 (connection limit)
    ↓
[Execute SQL]
    ↓
[Record Metrics]
    ↓
OperationResult (back to Scenario)
```

---

## Detailed Design

### Component 1: RuntimeEngine Trait

**Purpose:** Interface for all runtime implementations

**Current Design:**
```rust
#[async_trait::async_trait]
pub trait RuntimeEngine: Send + Sync {
    async fn submit(&self, op: Operation) -> Result<OperationResult>;
    fn stats(&self) -> RuntimeStats;
    async fn shutdown(&mut self) -> Result<()>;
}
```

**Design Decisions:**

| Decision | Rationale |
|----------|-----------|
| **`&self` for submit()** | Allows sharing via `Arc`, multiple tasks can submit concurrently |
| **Async trait** | Supports async I/O in implementations |
| **Returns `OperationResult`** | Always returns result, errors go into `error` field (not Rust Result) |
| **`&mut self` for shutdown()** | Mutable access enforces exclusive shutdown |

**Improvements (M0):**

None needed - trait is minimal and well-designed.

**Future Considerations (M1+):**

```rust
// Potential additions (M1):
async fn submit_with_timeout(&self, op: Operation, timeout: Duration) -> Result<OperationResult>;
async fn health_check(&self) -> HealthStatus;
```

### Component 2: AsyncRuntime

**Purpose:** Async execution engine with backpressure monitoring

**Current Structure:**
```rust
pub struct AsyncRuntime {
    pool: Arc<ConnectionPool>,          // Database connections
    semaphore: Arc<Semaphore>,          // Concurrency limiter
    backpressure_monitor: BackpressureMonitor,  // Saturation detector
    metrics: Arc<MetricsCollector>,     // Operation metrics
}
```

**Detailed Analysis:**

#### Field 1: `pool: Arc<ConnectionPool>`

**Purpose:** Provides database connections for operation execution

**Characteristics:**
- Shared across all operations (Arc)
- Managed by connection pool module
- Pool has its own limits (min_size, max_size)

**Interaction:**
```rust
let mut conn = self.pool.get().await?;  // May block if pool exhausted
```

#### Field 2: `semaphore: Arc<Semaphore>`

**Purpose:** Limits concurrent in-flight operations

**Why needed:** Prevents unbounded concurrency that could:
- Exhaust memory (too many pending futures)
- Overwhelm database (connection pool exhaustion)
- Create thundering herd

**Configuration:**
- `max_connections` parameter in constructor
- Typical values: 100-1000 (matches or exceeds pool size)

**Mechanism:**
```rust
let _permit = self.semaphore.acquire().await?;
// Permit automatically released on drop
```

**Key Invariant:**
```
semaphore.max_permits >= pool.max_size
```
Otherwise pool is bottleneck, semaphore does nothing.

#### Field 3: `backpressure_monitor: BackpressureMonitor`

**Purpose:** Detect when client (RSBench) is the bottleneck

**Current Logic:**
```rust
fn is_saturated(&self, stats: &RuntimeStats) -> bool {
    stats.pool_utilization > self.threshold  // e.g., > 0.8
}
```

**Problems with Current Design:**

| Issue | Example | Fix |
|-------|---------|-----|
| **Only checks pool** | Semaphore exhausted but pool at 50% → not detected | Also check semaphore pressure |
| **Doesn't track duration** | Sustained saturation vs transient spike → same | Track sustained vs transient |
| **Binary threshold** | 79% = OK, 81% = backpressure → too simplistic | Use smoothing/hysteresis |

**Improved Design (M0):**

```rust
struct BackpressureMonitor {
    pool_threshold: f64,         // e.g., 0.8
    semaphore_threshold: f64,    // e.g., 0.9 (available < 10%)
}

impl BackpressureMonitor {
    fn is_saturated(&self, stats: &RuntimeStats) -> bool {
        // Check BOTH pool AND semaphore
        let pool_saturated = stats.pool_utilization > self.pool_threshold;
        let sem_saturated = stats.semaphore_utilization() > self.semaphore_threshold;

        pool_saturated || sem_saturated
    }
}
```

Where `RuntimeStats` includes:
```rust
pub struct RuntimeStats {
    pub active_connections: usize,
    pub queued_operations: usize,      // ACTUAL queued (not available permits)
    pub pool_utilization: f64,
    pub semaphore_utilization: f64,    // NEW: (max - available) / max
    pub backpressure_active: bool,
}
```

#### Field 4: `metrics: Arc<MetricsCollector>`

**Purpose:** Record operation outcomes for reporting

**What's Recorded:**
- Operation name
- Duration
- Success/failure
- Error message (if failed)
- Backpressure events

**Timing:**
```rust
let start = Instant::now();
let result = conn.execute(&op.sql, &op.params).await;
let duration = start.elapsed();
self.metrics.record_operation(&op.name, duration, &result);
```

### Component 3: OperationResult

**Purpose:** Encapsulate operation outcome

**Current Design:**
```rust
pub struct OperationResult {
    pub success: bool,
    pub duration: Duration,
    pub rows_affected: u64,
    pub error: Option<String>,
}
```

**Design Note:** Always returns `Ok(OperationResult)`, errors go in `error` field.

**Why?** Scenario module treats database errors as data (to be counted), not control flow.

**Improvements (M0):**

Consider adding:
```rust
pub struct OperationResult {
    pub success: bool,
    pub duration: Duration,
    pub rows_affected: u64,
    pub error: Option<String>,
    pub retry_count: u32,         // NEW: How many retries (M1)
    pub waited_for_backpressure: bool,  // NEW: Did we wait for semaphore?
}
```

### Component 4: RuntimeStats

**Purpose:** Provide real-time runtime health metrics

**Current Design:**
```rust
pub struct RuntimeStats {
    pub active_connections: usize,
    pub queued_operations: usize,  // MISLEADING - actually available permits
    pub pool_utilization: f64,
    pub backpressure_active: bool,
}
```

**Problems:**

1. **`queued_operations` is wrong** - returns `semaphore.available_permits()`, not queued count
2. **Missing semaphore metrics** - can't see semaphore pressure
3. **Binary backpressure flag** - no indication of severity

**Improved Design (M0):**

```rust
pub struct RuntimeStats {
    pub active_connections: usize,
    pub total_connections: usize,    // NEW: Pool capacity
    pub pool_utilization: f64,

    pub active_operations: usize,    // NEW: In-flight ops (max - available)
    pub max_operations: usize,       // NEW: Semaphore capacity
    pub semaphore_utilization: f64,  // NEW: active / max

    pub backpressure_active: bool,
    pub backpressure_severity: f64,  // NEW: 0.0-1.0, how bad is it
}
```

---

## Implementation Plan

### Phase 0: Analysis & Documentation ✅ CURRENT

**Goal:** Understand current state, document design

**Tasks:**
- [x] Analyze existing implementation
- [x] Identify gaps and improvements
- [x] Create this design document
- [ ] Review with stakeholders

**Deliverables:**
- This document

---

### Phase 1: Enhanced Backpressure Monitoring (M0)

**Goal:** Improve backpressure detection to catch semaphore saturation

**Tasks:**

1. **Update RuntimeStats** (`src/runtime/mod.rs`)
   - Add `semaphore_utilization: f64`
   - Add `active_operations: usize`
   - Add `max_operations: usize`
   - Fix `queued_operations` to return actual queued count

2. **Enhance BackpressureMonitor** (`src/runtime/async_runtime.rs`)
   - Add `semaphore_threshold` field
   - Update `is_saturated()` to check BOTH pool AND semaphore
   - Add unit tests for new logic

3. **Update AsyncRuntime.stats()** (`src/runtime/async_runtime.rs`)
   - Calculate semaphore metrics correctly
   - Return enhanced RuntimeStats

**Code Changes:**

```rust
// src/runtime/mod.rs
pub struct RuntimeStats {
    // Existing
    pub active_connections: usize,
    pub pool_utilization: f64,
    pub backpressure_active: bool,

    // NEW
    pub semaphore_active: usize,      // In-flight operations
    pub semaphore_capacity: usize,    // Max concurrent operations
    pub semaphore_utilization: f64,   // active / capacity
}
```

```rust
// src/runtime/async_runtime.rs
struct BackpressureMonitor {
    pool_threshold: f64,         // e.g., 0.8
    semaphore_threshold: f64,    // e.g., 0.9
}

impl BackpressureMonitor {
    fn new(pool_threshold: f64, semaphore_threshold: f64) -> Self {
        Self { pool_threshold, semaphore_threshold }
    }

    fn is_saturated(&self, stats: &RuntimeStats) -> bool {
        let pool_saturated = stats.pool_utilization > self.pool_threshold;
        let sem_saturated = stats.semaphore_utilization > self.semaphore_threshold;
        pool_saturated || sem_saturated
    }
}
```

**Testing:**
- Unit test: semaphore saturated but pool OK → backpressure detected
- Unit test: pool saturated but semaphore OK → backpressure detected
- Unit test: both OK → no backpressure
- Unit test: both saturated → backpressure detected

**Success Criteria:**
- [x] RuntimeStats includes semaphore metrics
- [x] BackpressureMonitor checks both pool and semaphore
- [x] Tests verify new logic
- [x] No performance regression

---

### Phase 2: Comprehensive Unit Tests (M0)

**Goal:** Achieve 80%+ test coverage for runtime module

**Current State:** Only 3 tests in `mod.rs` (test data structures, not logic)

**Test Categories:**

#### 2.1 AsyncRuntime Tests

Create `src/runtime/async_runtime.rs` test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Test 1: Basic operation submission
    #[tokio::test]
    async fn test_submit_operation_success() {
        // Mock pool, metrics
        // Submit operation
        // Verify: metrics recorded, result correct
    }

    // Test 2: Semaphore limits concurrency
    #[tokio::test]
    async fn test_semaphore_limits_concurrency() {
        // Create runtime with max_connections=2
        // Submit 10 operations concurrently
        // Verify: only 2 execute at once
    }

    // Test 3: Backpressure detection (pool)
    #[tokio::test]
    async fn test_backpressure_detected_pool_saturated() {
        // Mock pool with high utilization
        // Submit operation
        // Verify: backpressure event recorded
    }

    // Test 4: Backpressure detection (semaphore)
    #[tokio::test]
    async fn test_backpressure_detected_semaphore_saturated() {
        // Exhaust semaphore
        // Submit operation
        // Verify: backpressure event recorded
    }

    // Test 5: Error handling
    #[tokio::test]
    async fn test_database_error_recorded() {
        // Mock connection that fails
        // Submit operation
        // Verify: result.success = false, error recorded
    }

    // Test 6: Stats correctness
    #[test]
    fn test_stats_calculation() {
        // Create runtime
        // Verify stats match expectations
    }

    // Test 7: Graceful shutdown
    #[tokio::test]
    async fn test_shutdown_waits_for_operations() {
        // Submit operations
        // Call shutdown
        // Verify: waits for completion
    }
}
```

#### 2.2 BackpressureMonitor Tests

```rust
#[cfg(test)]
mod backpressure_tests {
    // Test 1: Pool threshold
    #[test]
    fn test_pool_saturation_detection() {
        let monitor = BackpressureMonitor::new(0.8, 0.9);
        let stats = RuntimeStats {
            pool_utilization: 0.85,  // > threshold
            semaphore_utilization: 0.5,
            ..
        };
        assert!(monitor.is_saturated(&stats));
    }

    // Test 2: Semaphore threshold
    #[test]
    fn test_semaphore_saturation_detection() { ... }

    // Test 3: Both OK
    #[test]
    fn test_no_saturation_when_under_threshold() { ... }

    // Test 4: Edge cases (exactly at threshold)
    #[test]
    fn test_threshold_boundary_conditions() { ... }
}
```

**Test Infrastructure:**

Create mock helpers:
```rust
// tests/common/mock_runtime.rs
pub struct MockConnectionPool {
    connections: Arc<Mutex<VecDeque<MockConnection>>>,
    stats: PoolStats,
}

pub struct MockConnection {
    responses: VecDeque<Result<QueryResult>>,
}
```

**Success Criteria:**
- [ ] 15+ unit tests covering all code paths
- [ ] 80%+ code coverage
- [ ] All tests pass
- [ ] Tests run in <1 second

---

### Phase 3: Integration Tests (M0)

**Goal:** End-to-end tests with real/mock database

**Test Structure:**

Create `tests/integration/runtime_integration_test.rs`:

```rust
#[tokio::test]
async fn test_runtime_with_real_pool() {
    // Create real connection pool (MySQL)
    // Create AsyncRuntime
    // Submit operations
    // Verify: operations execute, metrics recorded
}

#[tokio::test]
async fn test_backpressure_under_load() {
    // Create runtime with small pool (max=5)
    // Submit 100 operations concurrently
    // Verify: backpressure events > 0
}

#[tokio::test]
async fn test_concurrent_workers() {
    // Spawn 100 tasks submitting operations
    // Verify: all complete, no data races
}
```

**Success Criteria:**
- [ ] 5+ integration tests
- [ ] Tests pass with real MySQL connection
- [ ] Tests pass with mock connection
- [ ] Backpressure correctly detected under load

---

### Phase 4: Performance Validation (M0)

**Goal:** Verify runtime meets performance targets

**Benchmarks to Add:**

Create `benches/runtime_bench.rs`:

```rust
fn bench_submit_overhead(c: &mut Criterion) {
    // Measure overhead of submit() path
    // Target: <10μs per submit
}

fn bench_concurrent_throughput(c: &mut Criterion) {
    // Measure throughput with many concurrent operations
    // Target: 100K ops/sec
}

fn bench_backpressure_check_overhead(c: &mut Criterion) {
    // Measure cost of backpressure monitoring
    // Target: <1μs
}
```

**Success Criteria:**
- [ ] Submit overhead <10μs
- [ ] Supports 100K+ ops/sec
- [ ] Backpressure check <1μs
- [ ] No memory leaks under sustained load

---

### Phase 5: Documentation Polish (M0)

**Goal:** Complete documentation for runtime module

**Tasks:**

1. **Rustdoc comments**
   - Every public type documented
   - Every public method documented
   - Examples for common patterns

2. **Module-level documentation**
   - Explain async execution model
   - Explain backpressure monitoring
   - Provide usage examples

3. **Update CLAUDE.md**
   - Document runtime design decisions
   - Add best practices

**Success Criteria:**
- [ ] All public items have rustdoc comments
- [ ] `cargo doc --no-deps --open` shows complete docs
- [ ] Examples compile and run

---

## Testing Strategy

### Unit Tests (Target: 80%+ coverage)

**What to Test:**

1. **Operation submission**
   - Success path
   - Error handling
   - Metrics recording

2. **Semaphore behavior**
   - Concurrency limiting
   - Permit acquisition/release

3. **Backpressure detection**
   - Pool saturation
   - Semaphore saturation
   - Threshold edge cases

4. **Stats calculation**
   - Pool metrics
   - Semaphore metrics
   - Utilization formulas

5. **Shutdown**
   - Waits for in-flight ops
   - Releases resources

### Integration Tests

**What to Test:**

1. **Real database interaction**
   - MySQL connection
   - Operation execution
   - Error propagation

2. **Concurrent load**
   - 100+ concurrent tasks
   - Backpressure under load
   - No data races

3. **Long-running stability**
   - 10K+ operations
   - No memory leaks
   - Consistent performance

### Property Tests (Optional)

**What to Test:**

1. **Concurrency limits**
   - Never exceed `max_connections`
   - Semaphore invariant holds

2. **Metrics correctness**
   - `total_ops = success + failed`
   - Backpressure events match actual saturation

---

## Performance Targets

### Throughput

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Single-threaded** | 10K ops/sec | Benchmark with no-op operations |
| **Multi-threaded** | 100K+ ops/sec | 100 concurrent tasks, measure aggregate |
| **Sustained load** | 50K ops/sec for 60s | Integration test, monitor memory |

### Latency

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Submit overhead** | <10μs | Benchmark: submit() with no-op connection |
| **Backpressure check** | <1μs | Benchmark: is_saturated() call |
| **Stats calculation** | <5μs | Benchmark: stats() method |

### Resource Usage

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Memory per operation** | <1 KB | Measure heap before/after submit |
| **Memory leak** | 0 | Long-running test, monitor RSS |
| **Semaphore fairness** | FIFO order | Test with multiple waiters |

### Backpressure Accuracy

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Detection latency** | <10ms | Time from saturation to event recording |
| **False positives** | <1% | Count events when not actually saturated |
| **False negatives** | 0% | Never miss actual saturation |

---

## Future Enhancements

### Milestone 1

**1. Operation Timeouts**

```rust
pub struct AsyncRuntime {
    // Existing fields...
    operation_timeout: Duration,  // NEW
}

async fn submit(&self, op: Operation) -> Result<OperationResult> {
    tokio::time::timeout(self.operation_timeout, async {
        // ... existing submit logic
    }).await
    .map_err(|_| RuntimeError::Timeout(self.operation_timeout))?
}
```

**2. Retry Logic**

```rust
pub struct RetryConfig {
    max_retries: u32,
    backoff: ExponentialBackoff,
    retryable_errors: Vec<String>,  // e.g., "connection lost"
}

async fn submit_with_retry(&self, op: Operation) -> Result<OperationResult> {
    // Retry transient errors
}
```

**3. Better Backpressure Metrics**

```rust
pub struct BackpressureMetrics {
    pub total_events: u64,
    pub sustained_events: u64,      // Saturated for >1 second
    pub max_queue_depth: usize,     // Peak semaphore queue
    pub time_saturated_ms: u64,     // Total time in backpressure
}
```

### Milestone 2

**1. Circuit Breaker**

```rust
pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitState>>,  // Open/Closed/HalfOpen
    failure_threshold: f64,
    recovery_timeout: Duration,
}
```

**2. Adaptive Concurrency**

```rust
pub struct AdaptiveSemaphore {
    current_permits: AtomicUsize,
    min_permits: usize,
    max_permits: usize,
    // Adjust based on latency/errors
}
```

**3. Per-Operation-Type Quotas**

```rust
pub struct QuotaManager {
    quotas: HashMap<String, Semaphore>,  // "read" -> 80%, "write" -> 20%
}
```

---

## Appendices

### Appendix A: Backpressure Detection Algorithm

**Current (M0):**
```
IF pool_utilization > 0.8 THEN
    backpressure_active = TRUE
END
```

**Improved (M0 v2):**
```
pool_pressure = pool_utilization > pool_threshold
sem_pressure = semaphore_utilization > semaphore_threshold

IF pool_pressure OR sem_pressure THEN
    backpressure_active = TRUE
    record_backpressure_event()
END
```

**Future (M1):**
```
pool_pressure = EWMA(pool_utilization) > pool_threshold
sem_pressure = EWMA(semaphore_utilization) > sem_threshold
sustained = pressure_duration > 1_second

IF (pool_pressure OR sem_pressure) AND sustained THEN
    backpressure_active = TRUE
    backpressure_severity = MAX(pool_pressure, sem_pressure)
    record_backpressure_event(severity, duration)
END
```

### Appendix B: Concurrency Model

**Why Semaphore Instead of Thread Pool?**

Traditional approach (sysbench):
```
Thread Pool (N threads)
  ↓
Each thread executes operations sequentially
  ↓
Concurrency = N threads
```

**Problem:** Threads block on I/O → wasted resources

RSBench approach:
```
Tokio Runtime (M OS threads, M << N)
  ↓
Async tasks (N tasks, N can be 10K+)
  ↓
Semaphore limits concurrent operations
  ↓
Concurrency = semaphore permits
```

**Benefits:**
- 10K concurrent operations with 8 OS threads
- Non-blocking I/O → no wasted CPU
- Memory efficient (~2 KB per task vs ~8 MB per thread)

### Appendix C: Error Handling Philosophy

**Design Decision:** Operation errors go in `OperationResult`, not Rust `Result`

**Rationale:**

```rust
// ❌ BAD: Database errors as Rust errors
async fn submit(&self, op: Operation) -> Result<OperationResult> {
    conn.execute(&op.sql).await?  // Database error propagates
}

// Problem: Scenario module treats database errors as failures
// But we want to COUNT errors, not FAIL on errors
```

```rust
// ✅ GOOD: Database errors as data
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

// Scenario module can count errors, measure error rate
// Only INFRA errors (pool exhausted, etc.) become Rust errors
```

### Appendix D: Shutdown Semantics

**Current Behavior:**
```rust
async fn shutdown(&mut self) -> Result<()> {
    let available = self.semaphore.available_permits();
    if available > 0 {
        let _ = self.semaphore.acquire_many(available as u32).await;
    }
    Ok(())
}
```

**Problem:** Doesn't wait for in-flight operations

**Improved (M1):**
```rust
async fn shutdown(&mut self) -> Result<()> {
    // 1. Stop accepting new operations
    self.accepting_operations.store(false, Ordering::SeqCst);

    // 2. Wait for all in-flight operations
    let max_permits = self.semaphore.capacity();
    let _ = self.semaphore.acquire_many(max_permits).await;

    // 3. Close connection pool
    self.pool.close().await?;

    Ok(())
}
```

---

**End of Runtime Design Document**
