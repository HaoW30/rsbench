# Event Module - Quick Reference

## Overview

**Purpose**: Watch external systems and generate events to control RSBench dynamically

**Architecture**: Parallel module (not nested in Scenario)

```
External Systems → Event Sources → EventManager → mpsc → Scenario
```

---

## Event Sources

| Source | Milestone | Purpose | Use Case |
|--------|-----------|---------|----------|
| **Timer** | M0 | Time-based events | Daily traffic patterns, scheduled rate changes |
| **K8s Watcher** | M1 | Pod/deployment events | Failover testing, rolling upgrades |
| **Webhook** | M1 | HTTP API events | Chaos Mesh integration, manual control |
| **Custom** | M2+ | User-defined | Prometheus alerts, custom tools |

---

## M0 Implementation (4-6 hours)

### Files to Create

```
src/event/
├── mod.rs              (~200 lines) - EventManager, public API
├── sources/
│   ├── mod.rs          (~50 lines)  - EventSource trait
│   └── timer.rs        (~150 lines) - Timer event source
└── config.rs           (~100 lines) - YAML config structs

tests/event_timer_test.rs (~200 lines) - Integration tests
```

### Core Types

```rust
// EventManager - orchestrates sources
pub struct EventManager {
    sources: Vec<Box<dyn EventSource>>,
}

impl EventManager {
    pub fn new(config: EventConfig) -> Self;
    pub async fn start(&mut self) -> Result<mpsc::UnboundedReceiver<Event>>;
    pub async fn shutdown(&mut self) -> Result<()>;
}

// EventSource trait - implemented by all sources
#[async_trait]
pub trait EventSource: Send + Sync {
    fn name(&self) -> &str;
    async fn watch(&mut self, tx: mpsc::UnboundedSender<Event>, shutdown: broadcast::Receiver<()>) -> Result<()>;
    async fn health_check(&self) -> bool;
}

// TimerEventSource - M0 implementation
pub struct TimerEventSource {
    schedule: Vec<TimerEvent>,
}

struct TimerEvent {
    at: Duration,
    event: Event,
}
```

### Configuration Example

```yaml
# scenarios/daily_pattern.yaml
scenario:
  executor:
    type: constant-rate
    rate: 100
    duration: 3600s
  workload:
    file: ../workloads/oltp_read_write.yaml

events:
  - type: timer
    schedule:
      - at: 0s
        event: { type: rate_change, rate: 100 }
      - at: 300s
        event: { type: rate_change, rate: 1000 }
      - at: 900s
        event: { type: rate_change, rate: 5000 }
      - at: 2700s
        event: { type: rate_change, rate: 1000 }
      - at: 3600s
        event: { type: phase_transition, phase: shutdown }
```

### Usage

```rust
// main.rs integration
let event_config = config.events;
let mut event_manager = EventManager::new(event_config);
let event_rx = event_manager.start().await?;

let mut executor = ScenarioExecutor::new(scenario_config);
executor.attach_event_stream(event_rx);

tokio::spawn(async move {
    executor.execute().await
});

// Shutdown
event_manager.shutdown().await?;
```

---

## M1 Extensions (10-12 hours)

### K8s Watcher (6 hours)

**Dependencies**:
```toml
kube = { version = "0.88", optional = true }
k8s-openapi = { version = "0.21", optional = true }
```

**Config**:
```yaml
events:
  - type: k8s
    kubeconfig: ~/.kube/config
    namespace: tidb-cluster
    resources:
      - type: pod
        name: tidb-*
        events:
          - Deleted: { action: log_timestamp }
          - Ready: { action: log_timestamp }
```

**Use Case**: Failover testing
```
kubectl delete pod tidb-0
  ↓
K8s watcher → Event(K8sEvent { resource: "tidb-0", event: "Deleted" })
  ↓
Scenario logs timestamp
  ↓
Metrics show error spike
  ↓
Correlate: Failover duration = 15.3s
```

### Webhook Listener (4 hours)

**Dependencies**:
```toml
axum = { version = "0.7", optional = true }
```

**Config**:
```yaml
events:
  - type: webhook
    listen: 0.0.0.0:9090
    auth:
      type: bearer_token
      token: ${WEBHOOK_TOKEN}
```

**API**:
```bash
# Change rate
curl -X POST http://localhost:9090/event/rate_change \
  -H "Content-Type: application/json" \
  -d '{"rate": 5000}'

# Pause
curl -X POST http://localhost:9090/event/phase_transition \
  -H "Content-Type: application/json" \
  -d '{"phase": "pause"}'

# Custom event
curl -X POST http://localhost:9090/event/custom \
  -H "Content-Type: application/json" \
  -d '{"type": "chaos_start", "experiment": "pod-kill"}'
```

---

## Testing Checklist

### Unit Tests
- [ ] EventManager creation
- [ ] EventManager start/stop
- [ ] TimerEventSource emits events at correct times
- [ ] TimerEventSource handles shutdown gracefully
- [ ] EventSource trait mock implementation

### Integration Tests
- [ ] Scenario receives timer events
- [ ] Scenario responds to rate_change events
- [ ] Scenario responds to phase_transition events
- [ ] Multiple event sources work together
- [ ] Graceful shutdown cleans up all sources

### Performance Tests
- [ ] Event latency < 10ms
- [ ] CPU overhead < 1%
- [ ] Memory overhead < 10MB per source

---

## Implementation Phases

### Phase 1: Module Structure (1-2 hours)
- [ ] Create `src/event/mod.rs`
- [ ] Implement `EventManager`
- [ ] Define `EventSource` trait
- [ ] Basic unit tests

### Phase 2: Timer Source (2 hours)
- [ ] Implement `TimerEventSource`
- [ ] Event scheduling logic
- [ ] Shutdown handling
- [ ] Unit tests

### Phase 3: Configuration (1 hour)
- [ ] Define config structs
- [ ] YAML deserialization
- [ ] Config validation
- [ ] Config tests

### Phase 4: Integration (1 hour)
- [ ] Wire EventManager into main.rs
- [ ] End-to-end integration test
- [ ] Documentation
- [ ] Examples

**Total M0 Time**: 4-6 hours

---

## Design Decisions

### Why Parallel, Not Nested?

✅ **Parallel** (RSBench):
```rust
// Event Module and Scenario Module run independently
let event_rx = event_manager.start().await?;
executor.attach_event_stream(event_rx);
tokio::spawn(executor.execute());
```

❌ **Nested** (Alternative):
```rust
// Scenario owns EventManager (tight coupling)
executor.set_event_sources(sources);
executor.execute().await;
```

**Benefits of Parallel**:
1. Separation of concerns
2. Can run scenario without events
3. Testable independently
4. Reusable across different executors

### Why Unbounded Channel?

- Events are infrequent (1-10/min, not thousands/sec)
- Dropping events is worse than buffering
- Scenario uses `try_recv()` (non-blocking), no deadlock risk

### Why Timer First (M0)?

- Simplest event source (no external dependencies)
- Covers 80% of use cases (daily patterns, scheduled changes)
- Foundation for K8s and Webhook
- Can be tested without external systems

---

## Key Interfaces

### Event Enum (Already Exists)

```rust
pub enum Event {
    RateChange(u64),
    PhaseTransition(Phase),
    MetricsSnapshot,
    Custom(serde_json::Value),
}

pub enum Phase {
    Pause,
    Resume,
    Shutdown,
}
```

### EventSource Trait (To Create)

```rust
#[async_trait]
pub trait EventSource: Send + Sync {
    fn name(&self) -> &str;

    async fn watch(
        &mut self,
        tx: mpsc::UnboundedSender<Event>,
        shutdown: broadcast::Receiver<()>,
    ) -> Result<()>;

    async fn health_check(&self) -> bool;
}
```

### EventManager (To Create)

```rust
pub struct EventManager {
    sources: Vec<Box<dyn EventSource>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl EventManager {
    pub fn new(config: EventConfig) -> Self;
    pub async fn start(&mut self) -> Result<mpsc::UnboundedReceiver<Event>>;
    pub async fn shutdown(&mut self) -> Result<()>;
}
```

---

## Success Criteria

### M0
- [x] Design document complete
- [ ] EventManager implemented
- [ ] TimerEventSource implemented
- [ ] Configuration support
- [ ] Integration tests passing
- [ ] Documentation updated
- [ ] Example scenario works

### M1
- [ ] K8s watcher implemented
- [ ] Webhook listener implemented
- [ ] Event correlation tracking
- [ ] All integration tests passing
- [ ] Real-world testing (failover scenario)

---

## Next Steps

1. ✅ Review design document
2. 🚧 Implement M0 (Timer events)
3. 🚧 Test with example scenarios
4. 🚧 Update CLAUDE.md
5. 🚧 Plan M1 implementation

---

## References

- **Full Design**: `docs/event-module-design.md`
- **Scenario Module**: `src/scenario.rs` (event handling already exists)
- **Example Config**: `scenarios/daily_traffic_pattern.yaml`
- **CLAUDE.md**: Section 4 - Event Module Architecture
