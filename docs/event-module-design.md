# Event Module Design

**Version**: 1.0
**Date**: 2026-01-07
**Status**: Design (Not Yet Implemented)

---

## Table of Contents

1. [Overview](#1-overview)
2. [Current State](#2-current-state)
3. [Architecture](#3-architecture)
4. [Event Sources](#4-event-sources)
5. [Event Types](#5-event-types)
6. [Communication Protocol](#6-communication-protocol)
7. [Implementation Phases](#7-implementation-phases)
8. [API Specification](#8-api-specification)
9. [Configuration](#9-configuration)
10. [Testing Strategy](#10-testing-strategy)
11. [Performance Requirements](#11-performance-requirements)
12. [Use Cases](#12-use-cases)
13. [Future Enhancements](#13-future-enhancements)

---

## 1. Overview

### Purpose

The Event Module is a **parallel, independent module** that watches external systems and generates events to control RSBench scenario execution dynamically. It enables:

- **Lifecycle Testing**: Coordinate workload with Kubernetes pod restarts, upgrades, failovers
- **Chaos Engineering**: Integrate with Chaos Mesh, litmus, etc.
- **Time-Based Testing**: Simulate daily traffic patterns (night → morning → peak → evening)
- **Manual Control**: Webhook API for human operators or CI/CD systems

### Key Design Principles

1. **Parallel Architecture** - Event Module runs alongside Scenario Module (not nested)
2. **Separation of Concerns** - Event watching is independent from workload execution
3. **Composability** - Scenario can run without Event Module (standalone mode)
4. **Testability** - Modules can be tested independently
5. **Non-Blocking** - Event watching never blocks workload execution
6. **Low Overhead** - Minimal CPU/memory impact (<1% overhead)

### Architectural Position

```
┌──────────────────────────────────────────────────────────┐
│                      RSBench Main                        │
└───────┬──────────────────────────────┬───────────────────┘
        │                              │
        ▼                              ▼
┌──────────────────┐          ┌──────────────────┐
│ Scenario Module  │◄─────────│  Event Module    │
│ (Consumer)       │  mpsc    │  (Producer)      │
└──────────────────┘ channel  └─────────┬────────┘
                                        │
                                        ├─► Timer Events (M0)
                                        ├─► K8s Watcher (M1)
                                        ├─► Webhook Listener (M1)
                                        └─► Custom Sources (M2+)
```

**Communication**: Unidirectional `tokio::mpsc` channel from Event Module → Scenario Module

---

## 2. Current State

### What Exists ✅

**Event Infrastructure in Scenario Module** (`src/scenario.rs`):

```rust
/// Events for dynamic scenario control
pub enum Event {
    RateChange(u64),                  // Change target rate
    PhaseTransition(Phase),           // Pause, resume, shutdown
    MetricsSnapshot,                  // Trigger metrics collection
    Custom(serde_json::Value),        // Application-defined
}

pub enum Phase {
    Pause,
    Resume,
    Shutdown,
}

impl ScenarioExecutor {
    pub fn attach_event_stream(&mut self, rx: mpsc::Receiver<Event>);
    async fn handle_events(&mut self) -> Result<()>;
}
```

**Tests**:
- ✅ `test_handle_events_pause_resume`
- ✅ `test_handle_events_rate_change`
- ✅ `test_handle_events_metrics_snapshot`
- ✅ `test_handle_events_custom_event`

### What's Missing ❌

1. **Event Module** - No producer of events exists
2. **Event Sources** - No timer, K8s watcher, webhook listener
3. **Event Configuration** - No YAML config for event sources
4. **Event Correlation** - No timestamp tracking for latency correlation
5. **Distributed Coordination** - No leader-only event reception

---

## 3. Architecture

### 3.1 Module Structure

```
src/event/
├── mod.rs              - Public API, EventManager
├── sources/
│   ├── mod.rs          - Event source trait
│   ├── timer.rs        - Timer-based events (M0)
│   ├── k8s.rs          - Kubernetes watcher (M1)
│   └── webhook.rs      - HTTP webhook listener (M1)
├── config.rs           - Event configuration
└── correlation.rs      - Event timestamp tracking (M1)
```

### 3.2 Core Components

#### EventManager

Central orchestrator that:
1. Spawns event sources as independent async tasks
2. Multiplexes events from all sources into single channel
3. Manages source lifecycle (start, stop, health check)
4. Provides graceful shutdown

```rust
pub struct EventManager {
    sources: Vec<Box<dyn EventSource>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl EventManager {
    pub fn new(config: EventConfig) -> Self;
    pub async fn start(&mut self) -> mpsc::Receiver<Event>;
    pub async fn shutdown(&mut self) -> Result<()>;
}
```

#### EventSource Trait

Common interface for all event sources:

```rust
#[async_trait]
pub trait EventSource: Send + Sync {
    /// Unique name for this source
    fn name(&self) -> &str;

    /// Start watching for events
    async fn watch(
        &mut self,
        tx: mpsc::Sender<Event>,
        shutdown: broadcast::Receiver<()>,
    ) -> Result<()>;

    /// Health check (returns true if healthy)
    async fn health_check(&self) -> bool;
}
```

### 3.3 Event Flow

```
External System (K8s, Timer, Webhook)
        │
        ▼
┌──────────────────┐
│  Event Source    │  (Timer, K8s Watcher, Webhook Listener)
│  (async task)    │
└────────┬─────────┘
         │ EventSource::watch()
         ▼
┌──────────────────┐
│  EventManager    │  Multiplexes from all sources
└────────┬─────────┘
         │ mpsc::channel
         ▼
┌──────────────────┐
│ ScenarioExecutor │  Consumes events
│  handle_events() │
└──────────────────┘
         │
         ▼
    Rate Limiter / Workload / Runtime
```

### 3.4 Why Parallel, Not Nested?

**✅ Parallel Design (RSBench Choice)**:
```rust
// main.rs
let event_manager = EventManager::new(config.events);
let event_rx = event_manager.start().await?;

let mut executor = ScenarioExecutor::new(scenario_config);
executor.attach_event_stream(event_rx);

tokio::spawn(async move { executor.execute().await });
// Event manager runs independently
```

**❌ Nested Design (Rejected)**:
```rust
// Scenario would own EventManager (tight coupling)
let mut executor = ScenarioExecutor::new(scenario_config);
executor.set_event_sources(event_sources); // ❌ Tight coupling
executor.execute().await;
```

**Advantages of Parallel Design**:
1. **Separation of Concerns** - Event watching decoupled from workload execution
2. **Composability** - Can run scenario without events (optional)
3. **Testability** - Test event sources independently with mock consumers
4. **Reusability** - Same EventManager works with any mpsc consumer
5. **Clarity** - Module boundaries are explicit

---

## 4. Event Sources

### 4.1 Timer Events (M0)

**Purpose**: Time-based phase transitions

**Use Cases**:
- Simulate daily traffic patterns (ramp up 9am → peak noon → ramp down 6pm)
- Warmup period before measurement
- Scheduled rate changes

**Configuration**:
```yaml
events:
  - type: timer
    schedule:
      - at: 0s
        event:
          type: rate_change
          rate: 100
      - at: 30s
        event:
          type: rate_change
          rate: 1000
      - at: 60s
        event:
          type: rate_change
          rate: 5000
      - at: 120s
        event:
          type: phase_transition
          phase: shutdown
```

**Implementation**:
```rust
pub struct TimerEventSource {
    schedule: Vec<TimerEvent>,
}

struct TimerEvent {
    at: Duration,        // Time offset from scenario start
    event: Event,        // Event to emit
}

#[async_trait]
impl EventSource for TimerEventSource {
    async fn watch(&mut self, tx: mpsc::Sender<Event>, mut shutdown: broadcast::Receiver<()>) -> Result<()> {
        let start = Instant::now();

        for timer_event in &self.schedule {
            let delay = timer_event.at.saturating_sub(start.elapsed());

            tokio::select! {
                _ = tokio::time::sleep(delay) => {
                    tx.send(timer_event.event.clone()).await?;
                }
                _ = shutdown.recv() => {
                    return Ok(());
                }
            }
        }

        Ok(())
    }
}
```

**Complexity**: Low (100 lines)
**Dependencies**: None
**Time Estimate**: 1-2 hours

---

### 4.2 Kubernetes Watcher (M1)

**Purpose**: Watch Kubernetes resources and correlate with workload behavior

**Use Cases**:
- **Failover Testing**: Detect pod deletion, measure error spike duration
- **Rolling Upgrade Testing**: Reduce load when upgrade starts
- **Scale-Out Testing**: Increase load when new pods become ready
- **Node Failure Testing**: Correlate node failure with latency

**Events Watched**:
- Pod events: `Created`, `Ready`, `Deleted`, `Failed`
- Deployment events: `ScalingUp`, `ScalingDown`, `RollingUpdate`
- Node events: `NotReady`, `Ready`

**Configuration**:
```yaml
events:
  - type: k8s
    kubeconfig: ~/.kube/config
    namespace: default
    resources:
      - type: pod
        name: tidb-*      # Glob pattern
        events:
          - Deleted:
              action: log_timestamp   # Record for correlation
          - Ready:
              action: log_timestamp

      - type: deployment
        name: tidb
        events:
          - RollingUpdate:
              action: rate_change
              rate: 100       # Reduce load during upgrade
```

**Implementation**:
```rust
use kube::{Client, Api, runtime::watcher};
use k8s_openapi::api::core::v1::Pod;

pub struct K8sEventSource {
    client: Client,
    namespace: String,
    watchers: Vec<ResourceWatcher>,
}

struct ResourceWatcher {
    resource_type: ResourceType,
    name_pattern: String,
    event_mappings: HashMap<K8sEventType, EventAction>,
}

enum EventAction {
    LogTimestamp,           // Just record, don't change workload
    RateChange(u64),        // Change rate
    PhaseTransition(Phase), // Pause/resume
}

#[async_trait]
impl EventSource for K8sEventSource {
    async fn watch(&mut self, tx: mpsc::Sender<Event>, mut shutdown: broadcast::Receiver<()>) -> Result<()> {
        let api: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let watcher = watcher(api, Default::default());

        tokio::pin!(watcher);

        loop {
            tokio::select! {
                Some(event) = watcher.try_next() => {
                    match event {
                        Ok(kube::runtime::watcher::Event::Applied(pod)) => {
                            // Check if pod matches patterns
                            if self.matches_pattern(&pod) {
                                if let Some(event) = self.map_k8s_event_to_rsbench(&pod) {
                                    tx.send(event).await?;
                                }
                            }
                        }
                        Ok(kube::runtime::watcher::Event::Deleted(pod)) => {
                            // Handle deletion
                        }
                        Err(e) => {
                            tracing::error!("K8s watch error: {}", e);
                        }
                        _ => {}
                    }
                }
                _ = shutdown.recv() => {
                    return Ok(());
                }
            }
        }
    }
}
```

**Complexity**: Medium (300-400 lines)
**Dependencies**: `kube = "0.88"`, `k8s-openapi = "0.21"`
**Time Estimate**: 4-6 hours

---

### 4.3 Webhook Listener (M1)

**Purpose**: Receive events from external systems via HTTP

**Use Cases**:
- **Chaos Mesh Integration**: Receive chaos experiment start/end events
- **Prometheus Alerts**: Trigger rate changes based on alerts
- **Manual Control**: Human operator triggers via `curl`
- **CI/CD Integration**: Build system triggers test phases

**API Endpoints**:
```
POST /event/rate_change
  Body: {"rate": 5000}

POST /event/phase_transition
  Body: {"phase": "pause"}

POST /event/custom
  Body: {"type": "chaos_start", "experiment": "pod-kill"}

GET /health
  Returns: {"status": "healthy", "uptime": "3600s"}
```

**Configuration**:
```yaml
events:
  - type: webhook
    listen: 0.0.0.0:9090
    auth:
      type: bearer_token
      token: ${WEBHOOK_TOKEN}  # From env var
    endpoints:
      - path: /event/rate_change
        method: POST
        event_type: rate_change
      - path: /event/phase_transition
        method: POST
        event_type: phase_transition
      - path: /event/custom
        method: POST
        event_type: custom
```

**Implementation**:
```rust
use axum::{Router, Json, extract::State};
use serde::{Deserialize, Serialize};

pub struct WebhookEventSource {
    listen_addr: SocketAddr,
    auth_token: Option<String>,
}

#[derive(Deserialize)]
struct RateChangeRequest {
    rate: u64,
}

#[derive(Deserialize)]
struct PhaseTransitionRequest {
    phase: String,  // "pause", "resume", "shutdown"
}

#[async_trait]
impl EventSource for WebhookEventSource {
    async fn watch(&mut self, tx: mpsc::Sender<Event>, mut shutdown: broadcast::Receiver<()>) -> Result<()> {
        let app_state = AppState { event_tx: tx.clone() };

        let app = Router::new()
            .route("/event/rate_change", post(handle_rate_change))
            .route("/event/phase_transition", post(handle_phase_transition))
            .route("/event/custom", post(handle_custom))
            .route("/health", get(health_check))
            .with_state(app_state);

        let listener = tokio::net::TcpListener::bind(self.listen_addr).await?;

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown.recv().await.ok();
            })
            .await?;

        Ok(())
    }
}

async fn handle_rate_change(
    State(state): State<AppState>,
    Json(req): Json<RateChangeRequest>,
) -> impl IntoResponse {
    state.event_tx.send(Event::RateChange(req.rate)).await.ok();
    (StatusCode::OK, "Rate change scheduled")
}
```

**Complexity**: Medium (200-300 lines)
**Dependencies**: `axum = "0.7"`, `tokio = { version = "1", features = ["net"] }`
**Time Estimate**: 3-4 hours

---

## 5. Event Types

### 5.1 Event Enum (Existing)

Already defined in `src/scenario.rs`:

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

### 5.2 Event Metadata (M1 Extension)

Add metadata for correlation and observability:

```rust
pub struct EventMetadata {
    pub timestamp: SystemTime,
    pub source: String,           // "timer", "k8s", "webhook"
    pub correlation_id: String,   // For distributed tracing
}

pub enum Event {
    RateChange {
        rate: u64,
        metadata: EventMetadata,
    },
    PhaseTransition {
        phase: Phase,
        metadata: EventMetadata,
    },
    MetricsSnapshot {
        metadata: EventMetadata,
    },

    // M1: K8s-specific events
    K8sEvent {
        resource_type: String,    // "Pod", "Deployment", "Node"
        resource_name: String,
        event_type: String,       // "Deleted", "Ready", "Failed"
        metadata: EventMetadata,
    },

    Custom {
        data: serde_json::Value,
        metadata: EventMetadata,
    },
}
```

**Note**: This is a breaking change. For M0, we'll keep the existing simple enum.

---

## 6. Communication Protocol

### 6.1 Channel Configuration

```rust
// Unbounded channel (events are infrequent)
let (tx, rx) = mpsc::unbounded_channel::<Event>();

// OR bounded channel with backpressure handling
let (tx, rx) = mpsc::channel::<Event>(100);  // 100-event buffer
```

**Choice**: **Unbounded** for M0/M1
- Events are infrequent (1-10/min, not thousands/sec)
- Dropping events is worse than buffering
- Scenario checks events non-blocking (`try_recv()`), so no deadlock risk

### 6.2 Non-Blocking Consumption

Scenario module already uses non-blocking recv:

```rust
// In ScenarioExecutor::execute()
async fn handle_events(&mut self) -> Result<()> {
    if let Some(ref mut rx) = self.event_rx {
        while let Ok(event) = rx.try_recv() {  // ← Non-blocking
            match event {
                Event::RateChange(rate) => self.rate_limiter.set_rate(rate),
                // ...
            }
        }
    }
    Ok(())
}
```

**Key Property**: Event processing never blocks workload execution.

### 6.3 Graceful Shutdown

```rust
// EventManager shutdown sequence
pub async fn shutdown(&mut self) -> Result<()> {
    // 1. Signal all sources to stop
    self.shutdown_tx.send(()).ok();

    // 2. Wait for sources to finish (with timeout)
    tokio::time::timeout(
        Duration::from_secs(5),
        self.join_handles.join_all(),
    ).await?;

    Ok(())
}
```

---

## 7. Implementation Phases

### M0: Timer Events (Foundation)

**Goal**: Basic event infrastructure with timer source

**Deliverables**:
1. ✅ Event enum (already exists in `src/scenario.rs`)
2. ✅ Scenario event handling (already exists)
3. 🚧 Event Module structure (`src/event/mod.rs`)
4. 🚧 EventSource trait
5. 🚧 EventManager
6. 🚧 TimerEventSource
7. 🚧 Event configuration (YAML)
8. 🚧 Integration tests

**Files to Create**:
- `src/event/mod.rs` (~200 lines)
- `src/event/sources/mod.rs` (~50 lines)
- `src/event/sources/timer.rs` (~150 lines)
- `src/event/config.rs` (~100 lines)
- `tests/event_timer_test.rs` (~200 lines)

**Time Estimate**: 4-6 hours

**Success Criteria**:
- [ ] Can schedule events at specific times
- [ ] Events delivered to scenario via channel
- [ ] Scenario responds to timer events (rate change, phase transition)
- [ ] Graceful shutdown works
- [ ] All tests pass

---

### M1: K8s Watcher + Webhook

**Goal**: External system integration

**Deliverables**:
1. K8s event source with pod/deployment watching
2. Webhook listener with HTTP API
3. Event correlation tracking
4. Event metadata (timestamp, source)
5. Integration tests for both sources

**Files to Create**:
- `src/event/sources/k8s.rs` (~400 lines)
- `src/event/sources/webhook.rs` (~300 lines)
- `src/event/correlation.rs` (~150 lines)
- `tests/event_k8s_test.rs` (~300 lines)
- `tests/event_webhook_test.rs` (~200 lines)

**Dependencies to Add**:
```toml
[dependencies]
kube = { version = "0.88", optional = true }
k8s-openapi = { version = "0.21", optional = true }
axum = { version = "0.7", optional = true }

[features]
k8s = ["kube", "k8s-openapi"]
webhook = ["axum"]
```

**Time Estimate**: 10-12 hours

**Success Criteria**:
- [ ] Can watch K8s pods and detect deletions
- [ ] Can receive webhook HTTP requests
- [ ] Events correlate with metrics timestamps
- [ ] All tests pass (including K8s integration test with kind)

---

### M2: Advanced Features

**Deliverables**:
1. Event filtering (only emit events matching criteria)
2. Event aggregation (batch multiple events)
3. Event replay (record/replay for testing)
4. Custom event sources (plugin API)
5. Distributed leader-only event reception

**Time Estimate**: 8-10 hours

---

## 8. API Specification

### 8.1 Public API (M0)

```rust
// src/event/mod.rs

/// Event manager that orchestrates multiple event sources
pub struct EventManager {
    sources: Vec<Box<dyn EventSource>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl EventManager {
    /// Create from configuration
    pub fn new(config: EventConfig) -> Self;

    /// Start all event sources, returns receiver for scenario to consume
    pub async fn start(&mut self) -> Result<mpsc::UnboundedReceiver<Event>>;

    /// Stop all event sources gracefully
    pub async fn shutdown(&mut self) -> Result<()>;

    /// Check health of all sources
    pub async fn health(&self) -> Vec<SourceHealth>;
}

/// Trait for event sources
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

/// Health status for event source
pub struct SourceHealth {
    pub name: String,
    pub healthy: bool,
    pub last_event: Option<SystemTime>,
    pub error_count: u64,
}
```

### 8.2 Configuration API (M0)

```yaml
# scenarios/my_test.yaml
scenario:
  # ... executor config ...

events:
  - type: timer
    schedule:
      - at: 0s
        event:
          type: rate_change
          rate: 100
      - at: 30s
        event:
          type: rate_change
          rate: 1000
      - at: 60s
        event:
          type: phase_transition
          phase: shutdown
```

Rust structs:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventConfig {
    pub sources: Vec<EventSourceConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum EventSourceConfig {
    #[serde(rename = "timer")]
    Timer {
        schedule: Vec<TimerEventConfig>,
    },

    #[cfg(feature = "k8s")]
    #[serde(rename = "k8s")]
    K8s {
        kubeconfig: Option<PathBuf>,
        namespace: String,
        resources: Vec<K8sResourceConfig>,
    },

    #[cfg(feature = "webhook")]
    #[serde(rename = "webhook")]
    Webhook {
        listen: SocketAddr,
        auth: Option<AuthConfig>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimerEventConfig {
    pub at: String,  // "0s", "30s", "1m", "1h"
    pub event: EventTypeConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum EventTypeConfig {
    #[serde(rename = "rate_change")]
    RateChange { rate: u64 },

    #[serde(rename = "phase_transition")]
    PhaseTransition { phase: String },

    #[serde(rename = "metrics_snapshot")]
    MetricsSnapshot,
}
```

---

## 9. Configuration

### 9.1 M0 Configuration Example

**Simple timer-based events**:

```yaml
# scenarios/daily_traffic_pattern.yaml
scenario:
  executor:
    type: constant-rate
    rate: 100  # Will be overridden by events
    duration: 3600s  # 1 hour

  workload:
    type: declarative
    file: ../workloads/oltp_read_write.yaml

events:
  - type: timer
    schedule:
      # Warmup: 100 ops/sec for 5 minutes
      - at: 0s
        event:
          type: rate_change
          rate: 100

      # Morning ramp: Increase to 1000 ops/sec
      - at: 300s   # 5 minutes
        event:
          type: rate_change
          rate: 1000

      # Noon peak: 5000 ops/sec
      - at: 900s   # 15 minutes
        event:
          type: rate_change
          rate: 5000

      # Evening ramp down: 1000 ops/sec
      - at: 2700s  # 45 minutes
        event:
          type: rate_change
          rate: 1000

      # Night: 100 ops/sec
      - at: 3300s  # 55 minutes
        event:
          type: rate_change
          rate: 100

      # Shutdown
      - at: 3600s  # 60 minutes
        event:
          type: phase_transition
          phase: shutdown
```

### 9.2 M1 Configuration Example

**K8s failover testing**:

```yaml
# scenarios/failover_test.yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 600s  # 10 minutes

  workload:
    type: declarative
    file: ../workloads/oltp_read_write.yaml

events:
  - type: k8s
    kubeconfig: ~/.kube/config
    namespace: tidb-cluster
    resources:
      # Watch TiDB pods
      - type: pod
        name: tidb-*
        events:
          - Deleted:
              action: log_timestamp
              metadata:
                experiment: failover_test
          - Ready:
              action: log_timestamp

      # Watch TiKV pods
      - type: pod
        name: tikv-*
        events:
          - Deleted:
              action: log_timestamp
```

**Webhook-triggered chaos engineering**:

```yaml
# scenarios/chaos_test.yaml
scenario:
  executor:
    type: constant-rate
    rate: 5000
    duration: 1800s  # 30 minutes

  workload:
    type: declarative
    file: ../workloads/oltp_read_write.yaml

events:
  # Listen for Chaos Mesh events
  - type: webhook
    listen: 0.0.0.0:9090
    auth:
      type: bearer_token
      token: ${CHAOS_WEBHOOK_TOKEN}
    endpoints:
      - path: /chaos/start
        method: POST
        event_type: custom
      - path: /chaos/end
        method: POST
        event_type: custom
      - path: /control/pause
        method: POST
        event_type: phase_transition
      - path: /control/resume
        method: POST
        event_type: phase_transition
```

### 9.3 Combined Configuration

```yaml
# scenarios/comprehensive_test.yaml
events:
  # Timer for structured test phases
  - type: timer
    schedule:
      - at: 0s
        event:
          type: rate_change
          rate: 1000
      - at: 300s
        event:
          type: metrics_snapshot

  # K8s for failover correlation
  - type: k8s
    namespace: default
    resources:
      - type: pod
        name: db-*
        events:
          - Deleted:
              action: log_timestamp

  # Webhook for manual control
  - type: webhook
    listen: 0.0.0.0:9090
```

---

## 10. Testing Strategy

### 10.1 Unit Tests

**EventManager**:
```rust
#[tokio::test]
async fn test_event_manager_creation() {
    let config = EventConfig { sources: vec![] };
    let manager = EventManager::new(config);
    assert_eq!(manager.sources.len(), 0);
}

#[tokio::test]
async fn test_event_manager_start_stop() {
    let config = EventConfig {
        sources: vec![
            EventSourceConfig::Timer { schedule: vec![] }
        ]
    };
    let mut manager = EventManager::new(config);

    let rx = manager.start().await.unwrap();
    manager.shutdown().await.unwrap();
}
```

**TimerEventSource**:
```rust
#[tokio::test]
async fn test_timer_emits_events_at_scheduled_times() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

    let mut timer = TimerEventSource::new(vec![
        TimerEvent {
            at: Duration::from_millis(10),
            event: Event::RateChange(1000),
        },
        TimerEvent {
            at: Duration::from_millis(20),
            event: Event::RateChange(2000),
        },
    ]);

    tokio::spawn(async move {
        timer.watch(tx, shutdown_rx).await.unwrap();
    });

    // Should receive first event around 10ms
    let event1 = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await.unwrap().unwrap();
    assert!(matches!(event1, Event::RateChange(1000)));

    // Should receive second event around 20ms
    let event2 = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await.unwrap().unwrap();
    assert!(matches!(event2, Event::RateChange(2000)));
}
```

### 10.2 Integration Tests

**End-to-End Timer Test**:
```rust
#[tokio::test]
async fn test_scenario_responds_to_timer_events() {
    // Setup scenario
    let scenario_config = ScenarioConfig { /* ... */ };
    let mut executor = ScenarioExecutor::new(scenario_config);

    // Setup event manager with timer
    let event_config = EventConfig {
        sources: vec![
            EventSourceConfig::Timer {
                schedule: vec![
                    TimerEventConfig {
                        at: "100ms".to_string(),
                        event: EventTypeConfig::RateChange { rate: 2000 },
                    },
                ],
            },
        ],
    };

    let mut event_manager = EventManager::new(event_config);
    let event_rx = event_manager.start().await.unwrap();
    executor.attach_event_stream(event_rx);

    // Run scenario briefly
    tokio::time::timeout(Duration::from_millis(200), executor.execute()).await.ok();

    // Verify rate was changed
    // (Would need to expose rate_limiter for testing or check metrics)
}
```

**K8s Integration Test** (M1):
```rust
#[tokio::test]
#[cfg(feature = "k8s")]
async fn test_k8s_watches_pod_deletion() {
    // Requires kind cluster or mock K8s API server
    // Create test pod
    // Delete test pod
    // Verify event emitted
}
```

### 10.3 Mock Event Sources

```rust
pub struct MockEventSource {
    name: String,
    events: Vec<Event>,
    delay: Duration,
}

#[async_trait]
impl EventSource for MockEventSource {
    fn name(&self) -> &str {
        &self.name
    }

    async fn watch(&mut self, tx: mpsc::UnboundedSender<Event>, mut shutdown: broadcast::Receiver<()>) -> Result<()> {
        for event in &self.events {
            tokio::select! {
                _ = tokio::time::sleep(self.delay) => {
                    tx.send(event.clone())?;
                }
                _ = shutdown.recv() => {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    async fn health_check(&self) -> bool {
        true
    }
}
```

---

## 11. Performance Requirements

### 11.1 Overhead Targets

| Metric | Target | Rationale |
|--------|--------|-----------|
| CPU overhead | <1% | Event watching should not impact workload |
| Memory overhead | <10 MB | Per event source |
| Event latency | <10ms | From source to scenario |
| Max event rate | 1000 events/sec | More than enough for realistic scenarios |

### 11.2 Benchmarks

```rust
// benches/event_bench.rs

#[bench]
fn bench_timer_event_emission(b: &mut Bencher) {
    // Measure time from scheduled time to channel send
}

#[bench]
fn bench_event_manager_throughput(b: &mut Bencher) {
    // Measure events/sec through EventManager
}

#[bench]
fn bench_event_channel_latency(b: &mut Bencher) {
    // Measure mpsc channel send → recv latency
}
```

---

## 12. Use Cases

### 12.1 Failover Testing

**Scenario**: Measure database failover time

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 5000
    duration: 600s

events:
  - type: k8s
    namespace: tidb-cluster
    resources:
      - type: pod
        name: tidb-0
        events:
          - Deleted:
              action: log_timestamp
              correlation_id: failover_test
          - Ready:
              action: log_timestamp
              correlation_id: failover_test
```

**Workflow**:
1. RSBench starts at 5000 ops/sec
2. Human operator: `kubectl delete pod tidb-0`
3. K8s watcher detects deletion → logs timestamp
4. Workload experiences errors (tracked in metrics)
5. K8s watcher detects pod Ready → logs timestamp
6. Calculate: Failover duration = Ready timestamp - Deleted timestamp
7. Correlate: Error spike duration ≈ Failover duration

**Expected Output**:
```
[Event] 15:04:32.123 - K8s Pod Deleted: tidb-0
[Metrics] 15:04:32.150 - Error rate: 0% → 85%
[Metrics] 15:04:47.890 - Error rate: 85% → 5%
[Event] 15:04:48.234 - K8s Pod Ready: tidb-0
[Analysis] Failover duration: 16.1s (48.234 - 32.123)
[Analysis] Error spike duration: 15.7s (47.890 - 32.150)
```

---

### 12.2 Daily Traffic Pattern Simulation

**Scenario**: Simulate 24-hour traffic pattern in 1 hour

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 100
    duration: 3600s

events:
  - type: timer
    schedule:
      - at: 0s
        event: { type: rate_change, rate: 100 }      # Night: 100 ops/sec
      - at: 900s   # 15min
        event: { type: rate_change, rate: 1000 }     # Morning: 1K ops/sec
      - at: 1800s  # 30min
        event: { type: rate_change, rate: 5000 }     # Noon peak: 5K ops/sec
      - at: 2700s  # 45min
        event: { type: rate_change, rate: 2000 }     # Evening: 2K ops/sec
      - at: 3300s  # 55min
        event: { type: rate_change, rate: 500 }      # Late: 500 ops/sec
```

---

### 12.3 Chaos Engineering with Webhook

**Scenario**: Coordinate with Chaos Mesh

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 10000
    duration: 1800s

events:
  - type: webhook
    listen: 0.0.0.0:9090
```

**Chaos Mesh Workflow**:
```bash
# Start RSBench
rsbench --scenario scenarios/chaos_test.yaml &

# Start chaos experiment
kubectl apply -f chaos/pod-kill.yaml

# Chaos Mesh webhook notification
curl -X POST http://rsbench:9090/event/custom \
  -H "Content-Type: application/json" \
  -d '{"type": "chaos_start", "experiment": "pod-kill"}'

# Wait 60s

# Chaos Mesh cleanup notification
curl -X POST http://rsbench:9090/event/custom \
  -H "Content-Type: application/json" \
  -d '{"type": "chaos_end", "experiment": "pod-kill"}'
```

**RSBench logs correlation**:
```
[Event] 15:05:00.000 - Custom: chaos_start (pod-kill)
[Metrics] 15:05:00.123 - Error rate: 0% → 42%
[Metrics] 15:05:15.456 - Error rate: 42% → 2%
[Event] 15:06:00.000 - Custom: chaos_end (pod-kill)
[Analysis] Chaos impact: 15.3s error spike
```

---

## 13. Future Enhancements (M2+)

### 13.1 Event Filtering

**Goal**: Only emit events matching criteria

```yaml
events:
  - type: k8s
    namespace: default
    resources:
      - type: pod
        name: tidb-*
        events:
          - Deleted:
              action: log_timestamp
              filter:
                labels:
                  app: tidb
                  environment: production
```

### 13.2 Event Aggregation

**Goal**: Batch multiple events to reduce noise

```yaml
events:
  - type: k8s
    aggregation:
      window: 5s
      max_events: 10
      strategy: latest  # or "all", "first"
```

### 13.3 Event Replay

**Goal**: Record events and replay for testing

```rust
pub struct EventRecorder {
    pub fn record(&mut self, event: Event);
    pub fn save(&self, path: &Path) -> Result<()>;
    pub fn load(path: &Path) -> Result<Vec<Event>>;
}

pub struct ReplayEventSource {
    events: Vec<(Duration, Event)>,  // (offset, event)
}
```

### 13.4 Custom Event Sources

**Goal**: Plugin API for user-defined sources

```rust
#[async_trait]
pub trait CustomEventSource: EventSource {
    fn init(&mut self, config: serde_json::Value) -> Result<()>;
}

// User implementation
pub struct PrometheusAlertSource;

#[async_trait]
impl CustomEventSource for PrometheusAlertSource {
    fn init(&mut self, config: serde_json::Value) -> Result<()> {
        // Parse Prometheus webhook config
    }
}
```

---

## Implementation Summary

### M0 Phase Plan (4-6 hours)

1. **Hour 1-2**: Module structure and EventManager
   - Create `src/event/mod.rs`
   - Implement EventManager
   - EventSource trait
   - Unit tests

2. **Hour 3-4**: TimerEventSource
   - Implement `src/event/sources/timer.rs`
   - Event scheduling logic
   - Unit tests

3. **Hour 5**: Configuration
   - YAML parsing for timer events
   - Integration with scenario config
   - Config validation

4. **Hour 6**: Integration tests and documentation
   - End-to-end test
   - Update CLAUDE.md
   - Usage examples

**Deliverables**:
- ✅ Event Module foundation
- ✅ Timer event source
- ✅ Configuration support
- ✅ Tests passing
- ✅ Documentation

### M1 Phase Plan (10-12 hours)

**K8s Watcher** (6 hours):
- Hour 1-2: Basic pod watching
- Hour 3-4: Event mapping and filtering
- Hour 5-6: Integration tests (with kind)

**Webhook Listener** (4 hours):
- Hour 1-2: HTTP server with axum
- Hour 3: Authentication and endpoints
- Hour 4: Integration tests

**Event Correlation** (2 hours):
- Hour 1: Timestamp tracking
- Hour 2: Correlation analysis

---

## Conclusion

The Event Module is a **critical enabler** for modern database testing practices:

✅ **Lifecycle Testing** - Coordinate with K8s pod lifecycles
✅ **Chaos Engineering** - Integrate with chaos tools
✅ **Observability** - Correlate events with metrics
✅ **Flexibility** - Timer, K8s, Webhook, Custom sources

**Key Design Wins**:
1. Parallel architecture (not nested) → Clean separation of concerns
2. EventSource trait → Easy to add new sources
3. Non-blocking communication → Zero impact on workload
4. Optional → Can run scenarios without events

**Next Steps**:
1. ✅ Review and approve design
2. 🚧 Implement M0 (Timer events) - 4-6 hours
3. 🚧 Implement M1 (K8s + Webhook) - 10-12 hours
4. 🚧 Test with real workloads
5. 🚧 Document usage patterns

Ready to proceed with implementation! 🚀
