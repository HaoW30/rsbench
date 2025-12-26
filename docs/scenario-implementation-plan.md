# Scenario Module Implementation Plan

**Module**: `src/scenario.rs`
**Target**: Milestone 0 (M0) - MVP Foundation
**Estimated Effort**: 40-60 hours
**Dependencies**: Runtime, Workload, RateLimiter, Metrics modules

---

## Overview

The Scenario Module is the **central orchestrator** that coordinates:
- Workload execution with rate control
- Runtime integration for operation submission
- Metrics collection and aggregation
- Lifecycle management (prepare → execute → cleanup)

---

## Prerequisites (Must Be Complete First)

**Module Status Check**:

1. ✅ **Rate Limiter** (`src/rate_limiter.rs`)
   - [x] Token bucket implementation
   - [x] Dynamic rate adjustment
   - [ ] Performance: <10μs per acquire()

2. ✅ **Runtime** (`src/runtime/`)
   - [x] AsyncRuntime with semaphore
   - [x] Backpressure monitoring
   - [ ] BlockingRuntime (deprecated, optional)

3. ✅ **Workload** (`src/workload/`)
   - [x] Workload trait
   - [x] DeclarativeWorkload (design complete)
   - [ ] DeclarativeWorkload (implementation pending)
   - [ ] OltpReadWrite (for testing)

4. ✅ **Metrics** (`src/metrics/`)
   - [x] MetricsCollector interface
   - [ ] HDR histogram integration
   - [ ] Lock-free recording

5. ✅ **Config** (`src/config/`)
   - [x] ScenarioConfig parsing
   - [x] ExecutorConfig enum

**Decision**: Proceed with Scenario implementation using **mocks** where dependencies are incomplete.

---

## Implementation Phases

### Phase 1: Core Data Structures (4-6 hours)

**Goal**: Define all types and interfaces

#### 1.1 ScenarioExecutor Struct

**File**: `src/scenario.rs`

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub struct ScenarioExecutor {
    // Configuration
    config: ScenarioConfig,

    // Dependencies (injected)
    workload: Box<dyn Workload>,
    runtime: Arc<dyn RuntimeEngine>,
    metrics: Arc<MetricsCollector>,

    // Execution state
    rate_limiter: RateLimiter,
    event_rx: Option<mpsc::Receiver<Event>>,

    // Internal state
    paused: bool,
    shutdown_requested: bool,
}

impl ScenarioExecutor {
    pub fn new(
        config: ScenarioConfig,
        workload: Box<dyn Workload>,
        runtime: Arc<dyn RuntimeEngine>,
        metrics: Arc<MetricsCollector>,
    ) -> Result<Self> {
        let rate_limiter = match &config.executor {
            ExecutorConfig::ConstantRate { rate, .. } => {
                RateLimiter::new(*rate)?
            }
            ExecutorConfig::RampingRate { stages, .. } => {
                RateLimiter::new(stages[0].target_rate)?
            }
            ExecutorConfig::ClosedLoop { .. } => {
                // No rate limiter needed for closed-loop
                RateLimiter::new(u64::MAX)?
            }
        };

        Ok(Self {
            config,
            workload,
            runtime,
            metrics,
            rate_limiter,
            event_rx: None,
            paused: false,
            shutdown_requested: false,
        })
    }

    pub fn attach_event_stream(&mut self, rx: mpsc::Receiver<Event>) {
        self.event_rx = Some(rx);
    }
}
```

**Tasks**:
- [ ] Define ScenarioExecutor struct
- [ ] Implement `new()` constructor
- [ ] Implement `attach_event_stream()` method
- [ ] Add builder pattern if needed

**Estimated**: 2 hours

---

#### 1.2 ScenarioResult Struct

```rust
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub duration: Duration,
    pub operations_completed: u64,
    pub operations_failed: u64,
    pub metrics_snapshot: MetricsSnapshot,

    // Additional metadata
    pub start_time: Instant,
    pub end_time: Instant,
    pub backpressure_events: u64,
}

impl ScenarioResult {
    pub fn success_rate(&self) -> f64 {
        let total = self.operations_completed + self.operations_failed;
        if total == 0 {
            return 0.0;
        }
        self.operations_completed as f64 / total as f64
    }
}
```

**Tasks**:
- [ ] Define ScenarioResult struct
- [ ] Implement helper methods (success_rate, throughput, etc.)
- [ ] Add serialization (serde)

**Estimated**: 1 hour

---

#### 1.3 ExecutionContext Struct

```rust
#[derive(Debug, Clone, Copy)]
pub struct ExecutionContext {
    pub worker_id: usize,
    pub iteration: u64,
    pub elapsed: Duration,
}
```

**Tasks**:
- [ ] Define ExecutionContext
- [ ] Document usage in workload integration

**Estimated**: 0.5 hours

---

### Phase 2: Open-Loop Executors (12-16 hours)

**Goal**: Implement constant-rate and ramping-rate executors

#### 2.1 Constant Rate Executor

**File**: `src/scenario.rs`

```rust
impl ScenarioExecutor {
    async fn execute_constant_rate(
        &mut self,
        rate: u64,
        duration: Duration,
        max_connections: usize,
    ) -> Result<ScenarioResult> {
        let start = Instant::now();
        let end_time = start + duration;
        let mut iteration = 0u64;

        // Set rate limiter
        self.rate_limiter.set_rate(rate);

        // Main execution loop
        while Instant::now() < end_time {
            // Check for shutdown/pause events
            self.handle_events().await?;
            if self.shutdown_requested {
                break;
            }
            if self.paused {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            // Acquire rate limit token
            self.rate_limiter.acquire().await?;

            // Generate operation from workload
            let ctx = ExecutionContext {
                worker_id: 0,  // Single-threaded for M0
                iteration,
                elapsed: start.elapsed(),
            };
            let operation = self.workload.next_operation(&ctx)?;

            // Submit operation (fire-and-forget)
            let runtime = self.runtime.clone();
            tokio::spawn(async move {
                let _ = runtime.submit(operation).await;
                // Errors are recorded in metrics, not propagated
            });

            iteration += 1;
        }

        // Wait for in-flight operations (brief cooldown)
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Collect metrics
        let metrics_snapshot = self.metrics.snapshot();
        let backpressure_events = self.runtime.stats().backpressure_events;

        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: metrics_snapshot.total_operations,
            operations_failed: metrics_snapshot.total_errors,
            metrics_snapshot,
            start_time: start,
            end_time: Instant::now(),
            backpressure_events,
        })
    }
}
```

**Tasks**:
- [ ] Implement execute_constant_rate()
- [ ] Handle event checking (handle_events helper)
- [ ] Integrate with rate limiter
- [ ] Integrate with workload
- [ ] Integrate with runtime
- [ ] Add graceful shutdown support
- [ ] Add pause/resume support

**Estimated**: 6 hours

---

#### 2.2 Ramping Rate Executor

```rust
impl ScenarioExecutor {
    async fn execute_ramping_rate(
        &mut self,
        stages: &[RateStage],
    ) -> Result<ScenarioResult> {
        let start = Instant::now();
        let mut iteration = 0u64;
        let mut current_stage_index = 0;

        // Main execution loop
        loop {
            // Check if we need to transition to next stage
            let elapsed = start.elapsed();
            let current_stage = self.get_current_stage(stages, elapsed, &mut current_stage_index)?;

            if current_stage.is_none() {
                break;  // All stages complete
            }
            let stage = current_stage.unwrap();

            // Update rate limiter for current stage
            self.rate_limiter.set_rate(stage.target_rate);

            // Execute at current rate
            self.handle_events().await?;
            if self.shutdown_requested {
                break;
            }
            if self.paused {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            self.rate_limiter.acquire().await?;

            let ctx = ExecutionContext {
                worker_id: 0,
                iteration,
                elapsed,
            };
            let operation = self.workload.next_operation(&ctx)?;

            let runtime = self.runtime.clone();
            tokio::spawn(async move {
                let _ = runtime.submit(operation).await;
            });

            iteration += 1;
        }

        // Collect results
        tokio::time::sleep(Duration::from_millis(100)).await;
        let metrics_snapshot = self.metrics.snapshot();
        let backpressure_events = self.runtime.stats().backpressure_events;

        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: metrics_snapshot.total_operations,
            operations_failed: metrics_snapshot.total_errors,
            metrics_snapshot,
            start_time: start,
            end_time: Instant::now(),
            backpressure_events,
        })
    }

    fn get_current_stage<'a>(
        &self,
        stages: &'a [RateStage],
        elapsed: Duration,
        current_index: &mut usize,
    ) -> Result<Option<&'a RateStage>> {
        let mut cumulative_duration = Duration::from_secs(0);

        for (index, stage) in stages.iter().enumerate() {
            cumulative_duration += stage.duration;
            if elapsed < cumulative_duration {
                *current_index = index;
                return Ok(Some(stage));
            }
        }

        Ok(None)  // All stages complete
    }
}
```

**Tasks**:
- [ ] Implement execute_ramping_rate()
- [ ] Implement get_current_stage() helper
- [ ] Handle stage transitions
- [ ] Update rate limiter dynamically
- [ ] Test stage boundaries

**Estimated**: 6 hours

---

### Phase 3: Closed-Loop Executor (10-14 hours)

**Goal**: Implement sysbench-compatible closed-loop execution

#### 3.1 Closed-Loop Implementation

```rust
impl ScenarioExecutor {
    async fn execute_closed_loop(
        &mut self,
        workers: usize,
        duration: Duration,
        think_time: Option<Duration>,
    ) -> Result<ScenarioResult> {
        let start = Instant::now();
        let end_time = start + duration;
        let mut handles = vec![];

        // Spawn worker tasks
        for worker_id in 0..workers {
            let runtime = self.runtime.clone();
            let workload = self.create_worker_workload(worker_id)?;
            let think_time = think_time;
            let end_time = end_time;

            let handle = tokio::spawn(async move {
                let mut iteration = 0u64;
                let start = Instant::now();

                while Instant::now() < end_time {
                    let ctx = ExecutionContext {
                        worker_id,
                        iteration,
                        elapsed: start.elapsed(),
                    };

                    let operation = match workload.next_operation(&ctx) {
                        Ok(op) => op,
                        Err(_) => break,  // Error generating operation
                    };

                    // KEY: Await completion (closed-loop)
                    let _ = runtime.submit(operation).await;

                    // Optional think time (like sysbench --think-time)
                    if let Some(delay) = think_time {
                        tokio::time::sleep(delay).await;
                    }

                    iteration += 1;
                }

                Ok::<u64, Error>(iteration)
            });

            handles.push(handle);
        }

        // Wait for all workers to complete
        let mut total_iterations = 0u64;
        for handle in handles {
            match handle.await {
                Ok(Ok(iterations)) => total_iterations += iterations,
                Ok(Err(e)) => tracing::warn!("Worker failed: {}", e),
                Err(e) => tracing::warn!("Worker panicked: {}", e),
            }
        }

        // Collect metrics
        tokio::time::sleep(Duration::from_millis(100)).await;
        let metrics_snapshot = self.metrics.snapshot();
        let backpressure_events = self.runtime.stats().backpressure_events;

        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: metrics_snapshot.total_operations,
            operations_failed: metrics_snapshot.total_errors,
            metrics_snapshot,
            start_time: start,
            end_time: Instant::now(),
            backpressure_events,
        })
    }

    fn create_worker_workload(&self, worker_id: usize) -> Result<Box<dyn Workload>> {
        // Clone workload with different seed for determinism
        // Each worker has independent RNG
        self.workload.clone_with_seed(worker_id as u64)
    }
}
```

**Tasks**:
- [ ] Implement execute_closed_loop()
- [ ] Implement create_worker_workload() helper
- [ ] Handle worker task spawning
- [ ] Handle worker completion
- [ ] Add think_time support
- [ ] Test determinism (same seed → same ops per worker)

**Estimated**: 8 hours

---

### Phase 4: Main Execute Method (4-6 hours)

**Goal**: Dispatch to appropriate executor based on config

#### 4.1 Main Execute Entry Point

```rust
impl ScenarioExecutor {
    pub async fn execute(&mut self) -> Result<ScenarioResult> {
        // Call prepare on workload
        let mut prepare_ctx = PrepareContext {
            // ... context for prepare phase
        };
        self.workload.prepare(&mut prepare_ctx).await?;

        // Dispatch to appropriate executor
        let result = match &self.config.executor {
            ExecutorConfig::ConstantRate { rate, duration, max_connections } => {
                self.execute_constant_rate(*rate, *duration, *max_connections).await?
            }
            ExecutorConfig::RampingRate { stages, max_connections, .. } => {
                self.execute_ramping_rate(stages).await?
            }
            ExecutorConfig::ClosedLoop { workers, duration, think_time } => {
                self.execute_closed_loop(*workers, *duration, *think_time).await?
            }
        };

        // Call cleanup on workload (optional)
        // self.workload.cleanup().await?;

        Ok(result)
    }
}
```

**Tasks**:
- [ ] Implement execute() entry point
- [ ] Call workload.prepare()
- [ ] Dispatch to correct executor
- [ ] Handle errors gracefully
- [ ] Add logging/tracing

**Estimated**: 3 hours

---

### Phase 5: Event Handling (6-8 hours)

**Goal**: Integrate with Event Module

#### 5.1 Event Handler

```rust
impl ScenarioExecutor {
    async fn handle_events(&mut self) -> Result<()> {
        if let Some(event_rx) = &mut self.event_rx {
            // Non-blocking check for events
            while let Ok(event) = event_rx.try_recv() {
                match event {
                    Event::RateChange(new_rate) => {
                        self.rate_limiter.set_rate(new_rate);
                        tracing::info!("Rate changed to {} ops/sec", new_rate);
                    }
                    Event::PhaseTransition(Phase::Pause) => {
                        self.paused = true;
                        tracing::info!("Scenario paused");
                    }
                    Event::PhaseTransition(Phase::Resume) => {
                        self.paused = false;
                        tracing::info!("Scenario resumed");
                    }
                    Event::PhaseTransition(Phase::Shutdown) => {
                        self.shutdown_requested = true;
                        tracing::info!("Graceful shutdown requested");
                        return Ok(());
                    }
                    Event::MetricsSnapshot => {
                        let snapshot = self.metrics.snapshot();
                        tracing::info!("Intermediate snapshot: {:?}", snapshot);
                    }
                    Event::Custom(data) => {
                        tracing::debug!("Custom event received: {:?}", data);
                    }
                    Event::K8sEvent { .. } => {
                        // Just log for now, correlation in M1
                        tracing::info!("K8s event received");
                    }
                }
            }
        }
        Ok(())
    }
}
```

**Tasks**:
- [ ] Implement handle_events()
- [ ] Handle RateChange event
- [ ] Handle PhaseTransition events
- [ ] Handle shutdown gracefully
- [ ] Add logging for all events

**Estimated**: 4 hours

---

### Phase 6: Testing (10-14 hours)

**Goal**: Comprehensive test coverage

#### 6.1 Unit Tests

**File**: `src/scenario.rs` (in `#[cfg(test)]` module)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scenario_result_creation() {
        let result = ScenarioResult {
            duration: Duration::from_secs(60),
            operations_completed: 1000,
            operations_failed: 10,
            // ... other fields
        };
        assert_eq!(result.success_rate(), 0.99);
    }

    #[test]
    fn test_scenario_result_success_rate() {
        let result = ScenarioResult {
            operations_completed: 950,
            operations_failed: 50,
            // ... other fields
        };
        assert!((result.success_rate() - 0.95).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_constant_rate_executor_basic() {
        // Create mock workload
        let workload = Box::new(MockWorkload::new());
        let runtime = Arc::new(MockRuntime::new());
        let metrics = Arc::new(MetricsCollector::new());

        let config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 10,
                duration: Duration::from_secs(1),
                max_connections: 10,
            },
            workload: WorkloadConfig::Mock,
        };

        let mut executor = ScenarioExecutor::new(config, workload, runtime, metrics)?;
        let result = executor.execute().await?;

        // Should have ~10 operations (±1 for timing)
        assert!(result.operations_completed >= 9);
        assert!(result.operations_completed <= 11);
    }

    #[tokio::test]
    async fn test_ramping_rate_transitions() {
        // Test that rate changes between stages
        // ...
    }

    #[tokio::test]
    async fn test_closed_loop_worker_independence() {
        // Test that workers execute independently
        // Test determinism (same seed → same operations)
        // ...
    }

    #[tokio::test]
    async fn test_event_handling_rate_change() {
        let (tx, rx) = mpsc::channel(10);
        // ... create executor
        executor.attach_event_stream(rx);

        // Send rate change event
        tx.send(Event::RateChange(2000)).await.unwrap();

        // Execute and verify rate changed
        // ...
    }
}
```

**Tasks**:
- [ ] Test ScenarioResult helpers
- [ ] Test constant rate executor
- [ ] Test ramping rate executor
- [ ] Test closed-loop executor
- [ ] Test event handling
- [ ] Test graceful shutdown
- [ ] Test pause/resume
- [ ] Test error propagation

**Estimated**: 8 hours

---

#### 6.2 Integration Tests

**File**: `tests/scenario_integration_test.rs`

```rust
#[tokio::test]
async fn test_scenario_end_to_end() {
    // Full end-to-end test with real components
    // Use in-memory database or mock
    // Verify metrics collection
    // Verify backpressure detection
}

#[tokio::test]
async fn test_scenario_with_events() {
    // Test event integration end-to-end
    // Use timer events (M0 available)
    // Verify rate changes
}
```

**Tasks**:
- [ ] End-to-end test with mock database
- [ ] Test with event module integration
- [ ] Test backpressure scenarios
- [ ] Test long-running scenario (30s+)

**Estimated**: 4 hours

---

### Phase 7: Documentation and Polish (4-6 hours)

**Goal**: Code comments, examples, cleanup

#### 7.1 Code Documentation

**Tasks**:
- [ ] Add rustdoc comments to all public methods
- [ ] Add module-level documentation
- [ ] Add usage examples in doc comments
- [ ] Document error conditions

**Estimated**: 3 hours

---

#### 7.2 Example Scenarios

**File**: `examples/basic_scenario.rs`

```rust
use rsbench::*;

#[tokio::main]
async fn main() -> Result<()> {
    // Create workload
    let workload = OltpReadWrite::new(/* ... */);

    // Create runtime
    let runtime = Arc::new(AsyncRuntime::new(/* ... */));

    // Create metrics
    let metrics = Arc::new(MetricsCollector::new());

    // Create scenario
    let config = ScenarioConfig {
        executor: ExecutorConfig::ConstantRate {
            rate: 1000,
            duration: Duration::from_secs(60),
            max_connections: 100,
        },
        workload: WorkloadConfig::Oltp(/* ... */),
    };

    let mut scenario = ScenarioExecutor::new(config, Box::new(workload), runtime, metrics)?;

    // Execute
    let result = scenario.execute().await?;

    println!("Operations: {}", result.operations_completed);
    println!("Success rate: {:.2}%", result.success_rate() * 100.0);
    println!("Duration: {:?}", result.duration);

    Ok(())
}
```

**Tasks**:
- [ ] Create basic example
- [ ] Create ramping rate example
- [ ] Create closed-loop example
- [ ] Create example with events

**Estimated**: 2 hours

---

## Dependencies and Parallel Work

### Can Be Done in Parallel:

1. **Phase 1** (Core Data Structures) - **Independent**
2. **Phase 2** (Open-Loop) + **Phase 3** (Closed-Loop) - **Can be parallel** (different developers)
3. **Phase 6** (Testing) - **Can start early** with mock dependencies

### Sequential Dependencies:

1. **Phase 1** → Must complete before Phase 2, 3, 4
2. **Phase 2 or 3** → Must complete before Phase 4 (main execute)
3. **Phase 4** → Must complete before Phase 5 (event handling)
4. **Phase 5** → Can be done after Phase 2 or 3
5. **Phase 7** → Can be done anytime after Phase 1

---

## Critical Path

**Longest path** (if sequential):
```
Phase 1 (6h) → Phase 2 (12h) → Phase 3 (10h) → Phase 4 (4h) → Phase 5 (6h) → Phase 6 (10h) → Phase 7 (4h)
Total: 52 hours
```

**Optimized path** (with parallelization):
```
Phase 1 (6h) → [Phase 2 (12h) || Phase 3 (10h)] → Phase 4 (4h) → Phase 5 (6h) → Phase 6 (10h) → Phase 7 (4h)
Total: 42 hours (2 developers) or 48 hours (1 developer with smart ordering)
```

---

## Risk Mitigation

### Risk 1: Workload Module Not Ready

**Mitigation**:
- Create `MockWorkload` for testing
- Use simple in-memory operation generator
- Can swap in real workload later

### Risk 2: Runtime Module Performance Issues

**Mitigation**:
- Start with simple implementation
- Add performance optimizations in separate PR
- Benchmark early (Phase 6)

### Risk 3: Event Module Not Ready

**Mitigation**:
- Implement timer events first (simple)
- K8s/webhook events can wait for M1
- Event handling is optional (can run without)

### Risk 4: Metrics Collection Overhead

**Mitigation**:
- Profile early in Phase 6
- Use mock metrics collector if needed
- Optimize hot path separately

---

## Success Criteria

### M0 Completion Checklist:

- [ ] **Constant rate executor** works end-to-end
- [ ] **Ramping rate executor** works with stage transitions
- [ ] **Closed-loop executor** works with multiple workers
- [ ] **Event handling** works with timer events
- [ ] **Tests pass**: 80%+ code coverage
- [ ] **Documentation**: All public APIs documented
- [ ] **Examples**: At least 3 working examples
- [ ] **Performance**: <20μs overhead per operation
- [ ] **Memory**: <100 MB for 1000 workers

---

## Recommended Implementation Order

### Week 1 (Foundations):
1. Day 1-2: Phase 1 (Core Data Structures)
2. Day 3-4: Phase 2 (Constant Rate Executor)
3. Day 5: Start Phase 6 (Unit Tests for Phase 1-2)

### Week 2 (Executors):
1. Day 1-2: Phase 3 (Closed-Loop Executor)
2. Day 3: Phase 4 (Main Execute Method)
3. Day 4: Phase 2 continued (Ramping Rate Executor)
4. Day 5: Phase 6 (Tests for Phase 3-4)

### Week 3 (Events & Polish):
1. Day 1-2: Phase 5 (Event Handling)
2. Day 3: Phase 6 (Integration Tests)
3. Day 4: Phase 7 (Documentation)
4. Day 5: Phase 7 (Examples) + Final cleanup

**Total**: ~15 working days (3 weeks) for 1 developer

---

## Next Steps After M0

1. **Benchmarking** (see `docs/performance-analysis.md`)
2. **Distributed mode** (M1 feature)
3. **Advanced event sources** (K8s, webhooks)
4. **Multi-threaded executor** for extreme throughput
5. **Connection pool optimization**

---

**Last Updated**: 2024-12-26
**Status**: Ready for Implementation
**Estimated Total Effort**: 40-60 hours (1-3 weeks depending on parallelization)
