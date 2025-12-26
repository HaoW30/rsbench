# Executor Pattern Architectural Decision

**Date**: 2024-12-26
**Status**: ✅ Approved
**Impact**: Major architectural simplification

---

## Decision Summary

**Replace BlockingRuntime with Closed-Loop Executor pattern in the Scenario module.**

### TL;DR

| Aspect | Before | After |
|--------|--------|-------|
| **Abstraction** | Runtime modes (Async, Blocking) | Executor patterns (Open-loop, Closed-loop) |
| **Implementations** | 2 runtimes (AsyncRuntime, BlockingRuntime) | 1 runtime (AsyncRuntime), 3+ executors |
| **Sysbench --threads** | BlockingRuntime (wrong semantics) | Closed-loop executor (correct semantics) |
| **Code complexity** | Higher (dual runtime paths) | Lower (single runtime, pluggable executors) |
| **Memory (100 workers)** | ~800 MB (if truly blocking) | ~20 MB (async tasks) |
| **Worker model** | Confused (blocking wrapper around async) | Clear (always async tasks) |

---

## Problem Statement

### Current BlockingRuntime Issues

1. **Wrong Semantics**:
   - Sysbench `--threads=N` = N workers executing operations **sequentially** (wait for completion)
   - Current BlockingRuntime = Async operations wrapped in `spawn_blocking` threads
   - Scenario still does `tokio::spawn(runtime.submit(op))` (fire-and-forget)
   - **Result**: No semantic difference from AsyncRuntime!

2. **Wrong Abstraction Layer**:
   ```
   What matters?              | Should be controlled by?
   ---------------------------|-------------------------
   Submit and wait vs fire    | Executor (Scenario)
   Fixed workers vs unbounded | Executor (Scenario)
   Rate limiting              | Executor (Scenario)
   Async vs blocking I/O      | Runtime (Implementation detail)
   ```

3. **Confusing Configuration**:
   ```yaml
   runtime:
     type: blocking
     threads: 16
   scenario:
     executor:
       type: constant-rate
       rate: 1000
   ```
   This combines:
   - Blocking threads (runtime layer)
   - Time-driven fire-and-forget (executor layer)
   - Overhead with no benefit

---

## Proposed Solution

### Closed-Loop Executor Pattern

Move execution pattern control to the **Scenario** module:

```rust
pub enum ExecutorConfig {
    // Open-loop executors (M0 existing)
    ConstantRate { rate: u64, duration: Duration },
    RampingRate { stages: Vec<RateStage> },

    // Closed-loop executor (M0 new)
    ClosedLoop {
        workers: usize,              // Like sysbench --threads
        duration: Duration,
        think_time: Option<Duration>,
    },
}
```

**Implementation in Scenario Module**:

```rust
async fn execute_closed_loop(
    &mut self,
    workers: usize,
    duration: Duration,
) -> Result<ScenarioResult> {
    let mut handles = vec![];

    for worker_id in 0..workers {
        let runtime = self.runtime.clone();  // Always AsyncRuntime
        let workload = self.create_worker_workload(worker_id)?;

        // Each worker is a tokio async task
        let handle = tokio::spawn(async move {
            while Instant::now() < end_time {
                let op = workload.next_operation(&ctx)?;

                // KEY: Await completion (closed-loop)
                runtime.submit(op).await?;

                iteration += 1;
            }
        });

        handles.push(handle);
    }

    // Wait for all workers
    for handle in handles {
        handle.await?;
    }
}
```

**Configuration**:

```yaml
# Sysbench --threads=16 equivalent
runtime:
  type: async
  workers: 8           # Tokio OS threads
  max_connections: 100

scenario:
  executor:
    type: closed-loop
    workers: 16        # Async tasks (green threads)
    duration: 300s
```

---

## Worker Architecture

### What is a Worker?

**Workers are Tokio async tasks (green threads), NOT OS threads.**

```
User Config: workers: 100
      ↓
100 Tokio async tasks (~2KB stack each)
      ↓
Scheduled on 8 OS threads (tokio runtime)
      ↓
Non-blocking async I/O
```

### Comparison Table

| Aspect | Sysbench --threads=100 | RSBench workers: 100 |
|--------|------------------------|----------------------|
| **OS Threads** | 100 | 4-8 (tokio runtime) |
| **Memory** | ~800 MB (thread stacks) | ~200 KB (async tasks) |
| **Context Switches** | High (100 threads) | Low (8 threads) |
| **I/O Model** | Blocking (thread waits) | Async (task yields) |
| **Scalability Limit** | ~500 threads (OS limit) | ~10K workers (memory limit) |

### Scalability Analysis

| Executor | Workers | OS Threads | Throughput | Bottleneck |
|----------|---------|------------|------------|------------|
| Open-loop (M0) | 1 | 4-8 | ~50K/s | Rate limiter |
| Closed-loop (M0) | 100 | 4-8 | ~10K/s | Latency (10ms avg) |
| Closed-loop (M0) | 1000 | 8-16 | ~100K/s | Latency |
| Open-loop (M1) | 8 | 8-16 | ~500K/s | CPU (parallel) |

---

## Benefits

### 1. Simpler Architecture

**Before** (2 runtimes):
```
Scenario → RuntimeFactory → { AsyncRuntime, BlockingRuntime }
                                    ↓              ↓
                            Connection Pool  Connection Pool
```

**After** (1 runtime, multiple executors):
```
Scenario → Executor Pattern → RuntimeFactory → AsyncRuntime
    ↓                                                ↓
ConstantRate                                 Connection Pool
RampingRate
ClosedLoop
```

### 2. True Sysbench Compatibility

| Sysbench | RSBench (Before) | RSBench (After) |
|----------|------------------|-----------------|
| `--threads=16` | BlockingRuntime (wrong) | `workers: 16` (correct) |
| Sequential per worker | ❌ Fire-and-forget | ✅ Await completion |
| Natural backpressure | ❌ Hidden | ✅ Throughput = workers/latency |

### 3. Better Resource Efficiency

```
Sysbench --threads=1000:
  - 1000 OS threads
  - ~8 GB memory
  - High context switch overhead

RSBench workers: 1000:
  - 8-16 OS threads
  - ~50 MB memory
  - Low overhead (async multiplexing)
```

### 4. More Flexible

Can combine patterns:
```yaml
# Closed-loop WITH rate limiting (future M1)
executor:
  type: closed-loop
  workers: 16
  max_rate: 1000  # Cap throughput even if latency allows more
```

---

## Migration Path

### Phase 1: M0 Completion
1. ✅ Document the decision (this doc)
2. ⏭️ Implement `ClosedLoop` executor in `src/scenario.rs`
3. ⏭️ Add `ClosedLoop` to `ExecutorConfig` enum
4. ⏭️ Update tests and examples
5. ⏭️ Mark BlockingRuntime as deprecated in code comments

### Phase 2: M1
1. ⏭️ Remove `BlockingRuntime` from `src/runtime/blocking.rs`
2. ⏭️ Remove from `RuntimeFactory`
3. ⏭️ Update all documentation
4. ⏭️ Remove test fixtures

### Phase 3: Communication
1. ✅ Update README.md architecture section
2. ✅ Update scenario-design.md with worker architecture
3. ✅ Add Sysbench migration guide (Appendix C)
4. ✅ Update project_structure.md
5. ⏭️ Blog post explaining the decision

---

## Sysbench Compatibility Story

### Command Mapping

```bash
# Sysbench
sysbench oltp_read_write \
  --threads=16 \
  --time=300 \
  run

# RSBench (equivalent)
rsbench run \
  --config config.yaml \
  --workers 16 \
  --duration 300s

# Config file
scenario:
  executor:
    type: closed-loop
    workers: 16        # == sysbench --threads=16
    duration: 300s     # == sysbench --time=300
```

### Workload Mapping

| Sysbench | RSBench |
|----------|---------|
| `oltp_read_write` | `workloads/oltp_read_write.yaml` |
| `oltp_read_only` | `workloads/oltp_read_only.yaml` |
| `oltp_write_only` | `workloads/oltp_write_only.yaml` |
| `--threads=N` | `executor.workers: N` |
| `--rate=X` | `executor.rate: X` (different executor type) |
| `--time=300` | `executor.duration: 300s` |

---

## Implementation Checklist

### Code Changes

- [ ] Add `ClosedLoop` variant to `ExecutorConfig` enum
- [ ] Implement `execute_closed_loop()` method in `ScenarioExecutor`
- [ ] Add `create_worker_workload()` helper (clone/create per worker)
- [ ] Update `ScenarioExecutor::execute()` dispatch logic
- [ ] Add unit tests for closed-loop executor
- [ ] Add integration tests with mock runtime
- [ ] Mark `BlockingRuntime` as `#[deprecated]`
- [ ] Update configuration examples

### Documentation Updates

- [x] ✅ scenario-design.md: Add worker architecture section (2.4)
- [x] ✅ scenario-design.md: Add closed-loop executor details (11.1)
- [x] ✅ scenario-design.md: Add Sysbench migration guide (Appendix C)
- [x] ✅ scenario-design.md: Update blocking runtime compatibility (5.3.1)
- [x] ✅ README.md: Update architecture section
- [x] ✅ README.md: Update comparison table
- [x] ✅ project_structure.md: Update runtime module description
- [x] ✅ project_structure.md: Update scenario module description
- [x] ✅ project_structure.md: Add executor pattern decision
- [x] ✅ project_structure.md: Update M0 status
- [ ] api_spec_m0.md: Update scenario section with closed-loop
- [ ] Create migration guide for users (if any are using BlockingRuntime)

### Testing

- [ ] Property test: Same seed → same operations (closed-loop)
- [ ] Integration test: Closed-loop throughput = workers/latency
- [ ] Integration test: All workers complete before result
- [ ] Benchmark: Compare closed-loop vs open-loop overhead
- [ ] Sysbench compatibility test: Verify equivalent behavior

---

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Users already using BlockingRuntime | Low | Medium | Provide migration guide, deprecation warnings |
| Closed-loop implementation bugs | Medium | High | Comprehensive testing, gradual rollout |
| Performance regression | Low | Medium | Benchmarking, profiling |
| Breaking config changes | Low | High | Keep deprecated BlockingRuntime in M0, remove in M1 |

---

## Success Metrics

1. **Code Simplicity**: Remove ~100 lines (BlockingRuntime + tests)
2. **Memory Efficiency**: 1000 workers < 100 MB (vs sysbench 8 GB)
3. **Sysbench Compatibility**: 100% command mapping documented
4. **User Satisfaction**: Migration path < 5 minutes for existing users
5. **Performance**: Closed-loop overhead < 5% vs direct async execution

---

## References

- **Scenario Design Doc**: [docs/scenario-design.md](scenario-design.md)
- **Project Structure**: [docs/project_structure.md](project_structure.md)
- **Sysbench Documentation**: https://github.com/akopytov/sysbench
- **Tokio Async Tasks**: https://tokio.rs/tokio/tutorial/spawning

---

## Decision Makers

- **Proposed by**: Architecture review (2024-12-26)
- **Approved by**: Project maintainer
- **Reviewed by**: N/A (early stage project)

---

## Appendix: Code Size Comparison

**Current (with BlockingRuntime)**:
```
src/runtime/blocking.rs:        86 lines
src/runtime/mod.rs:            138 lines (includes both runtimes)
tests/runtime_tests.rs:         50 lines (blocking-specific)
Total:                         274 lines
```

**After (with ClosedLoop executor)**:
```
src/scenario.rs (new method):   40 lines (execute_closed_loop)
src/config/mod.rs (enum):        3 lines (new variant)
tests/scenario_tests.rs:        30 lines (closed-loop tests)
Total:                          73 lines

Net reduction: 201 lines removed
```

---

**End of Document**
