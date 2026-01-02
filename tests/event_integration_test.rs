//! Integration tests for Event Module
//!
//! Tests the complete flow from EventManager → Channel → Scenario

use rsbench::event::EventManager;
use rsbench::event::config::{EventConfig, EventSourceConfig, TimerEventConfig, EventTypeConfig};
use rsbench::scenario::Event;
use rsbench::workload::{Workload, Operation, OperationType, ExecutionContext, PrepareContext};
use rsbench::Result;
use std::time::Duration;
use tokio::sync::mpsc;

// Mock workload for testing
struct MockWorkload {
    operation_count: usize,
}

impl MockWorkload {
    fn new() -> Self {
        Self { operation_count: 0 }
    }
}

#[async_trait::async_trait]
impl Workload for MockWorkload {
    async fn prepare(&mut self, _ctx: &mut PrepareContext<'_>) -> Result<()> {
        Ok(())
    }

    fn next_operation(&mut self, _ctx: &ExecutionContext) -> Result<Operation> {
        self.operation_count += 1;
        Ok(Operation {
            name: "test_op".to_string(),
            sql: "SELECT 1".to_string(),
            params: vec![],
            operation_type: OperationType::Read,
            is_transaction: false,
            transaction_sqls: vec![],
            transaction_params: vec![],
        })
    }

    fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &str {
        "mock"
    }
}

#[tokio::test]
async fn test_event_manager_integration_with_scenario() {
    // Create event config with timer events
    let event_config = EventConfig {
        sources: vec![
            EventSourceConfig::Timer {
                schedule: vec![
                    TimerEventConfig {
                        at: "100ms".to_string(),
                        event: EventTypeConfig::RateChange { rate: 500 },
                    },
                    TimerEventConfig {
                        at: "200ms".to_string(),
                        event: EventTypeConfig::RateChange { rate: 1000 },
                    },
                ],
            },
        ],
    };

    // Start event manager
    let mut event_manager = EventManager::new(event_config).unwrap();
    let event_rx = event_manager.start().await.unwrap();

    // Verify we can receive events
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();

    // Spawn task to forward events
    tokio::spawn(async move {
        let mut event_rx = event_rx;
        while let Some(event) = event_rx.recv().await {
            tx.send(event).ok();
        }
    });

    // Should receive first event
    let event1 = tokio::time::timeout(
        Duration::from_millis(300),
        rx.recv()
    ).await.unwrap().unwrap();
    assert!(matches!(event1, Event::RateChange(500)));

    // Should receive second event
    let event2 = tokio::time::timeout(
        Duration::from_millis(300),
        rx.recv()
    ).await.unwrap().unwrap();
    assert!(matches!(event2, Event::RateChange(1000)));

    // Shutdown
    event_manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_timer_events_schedule_multiple_rate_changes() {
    let event_config = EventConfig {
        sources: vec![
            EventSourceConfig::Timer {
                schedule: vec![
                    TimerEventConfig {
                        at: "20ms".to_string(),
                        event: EventTypeConfig::RateChange { rate: 100 },
                    },
                    TimerEventConfig {
                        at: "40ms".to_string(),
                        event: EventTypeConfig::RateChange { rate: 500 },
                    },
                    TimerEventConfig {
                        at: "60ms".to_string(),
                        event: EventTypeConfig::RateChange { rate: 1000 },
                    },
                    TimerEventConfig {
                        at: "80ms".to_string(),
                        event: EventTypeConfig::MetricsSnapshot,
                    },
                ],
            },
        ],
    };

    let mut event_manager = EventManager::new(event_config).unwrap();
    let mut event_rx = event_manager.start().await.unwrap();

    // Collect all events
    let mut events = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(
        Duration::from_millis(150),
        event_rx.recv()
    ).await {
        events.push(event);
    }

    // Should have received all 4 events
    assert_eq!(events.len(), 4);

    assert!(matches!(events[0], Event::RateChange(100)));
    assert!(matches!(events[1], Event::RateChange(500)));
    assert!(matches!(events[2], Event::RateChange(1000)));
    assert!(matches!(events[3], Event::MetricsSnapshot));

    event_manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_event_manager_handles_all_event_types() {
    let event_config = EventConfig {
        sources: vec![
            EventSourceConfig::Timer {
                schedule: vec![
                    TimerEventConfig {
                        at: "10ms".to_string(),
                        event: EventTypeConfig::RateChange { rate: 1000 },
                    },
                    TimerEventConfig {
                        at: "20ms".to_string(),
                        event: EventTypeConfig::PhaseTransition {
                            phase: "pause".to_string(),
                        },
                    },
                    TimerEventConfig {
                        at: "30ms".to_string(),
                        event: EventTypeConfig::PhaseTransition {
                            phase: "resume".to_string(),
                        },
                    },
                    TimerEventConfig {
                        at: "40ms".to_string(),
                        event: EventTypeConfig::MetricsSnapshot,
                    },
                    TimerEventConfig {
                        at: "50ms".to_string(),
                        event: EventTypeConfig::Custom {
                            data: serde_json::json!({"test": "data"}),
                        },
                    },
                ],
            },
        ],
    };

    let mut event_manager = EventManager::new(event_config).unwrap();
    let mut event_rx = event_manager.start().await.unwrap();

    // Receive all events
    let mut events = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(
        Duration::from_millis(100),
        event_rx.recv()
    ).await {
        events.push(event);
    }

    assert_eq!(events.len(), 5);

    // Verify event types
    assert!(matches!(events[0], Event::RateChange(1000)));
    assert!(matches!(events[1], Event::PhaseTransition(_)));
    assert!(matches!(events[2], Event::PhaseTransition(_)));
    assert!(matches!(events[3], Event::MetricsSnapshot));
    assert!(matches!(events[4], Event::Custom(_)));

    event_manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_event_manager_graceful_shutdown() {
    let event_config = EventConfig {
        sources: vec![
            EventSourceConfig::Timer {
                schedule: vec![
                    TimerEventConfig {
                        at: "50ms".to_string(),
                        event: EventTypeConfig::RateChange { rate: 1000 },
                    },
                    TimerEventConfig {
                        at: "10s".to_string(),  // Long delay
                        event: EventTypeConfig::RateChange { rate: 2000 },
                    },
                ],
            },
        ],
    };

    let mut event_manager = EventManager::new(event_config).unwrap();
    let mut event_rx = event_manager.start().await.unwrap();

    // Receive first event
    let event = tokio::time::timeout(
        Duration::from_millis(200),
        event_rx.recv()
    ).await.unwrap().unwrap();
    assert!(matches!(event, Event::RateChange(1000)));

    // Shutdown before second event
    let shutdown_result = tokio::time::timeout(
        Duration::from_millis(500),
        event_manager.shutdown()
    ).await;

    // Should complete quickly
    assert!(shutdown_result.is_ok());
    assert!(shutdown_result.unwrap().is_ok());

    // Should not receive second event
    let no_event = tokio::time::timeout(
        Duration::from_millis(100),
        event_rx.recv()
    ).await;
    assert!(no_event.is_err() || no_event.unwrap().is_none());
}

#[tokio::test]
async fn test_empty_event_config() {
    // Empty config is valid - manager starts but emits no events
    let event_config = EventConfig::default();
    assert!(event_config.is_empty());

    let mut event_manager = EventManager::new(event_config).unwrap();
    let _event_rx = event_manager.start().await.unwrap();

    // Shutdown should work even with no sources
    event_manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_config_from_yaml() {
    let yaml = r#"
sources:
  - type: timer
    schedule:
      - at: "100ms"
        event:
          type: rate_change
          rate: 500
      - at: "200ms"
        event:
          type: phase_transition
          phase: "shutdown"
"#;

    let event_config: EventConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(event_config.source_count(), 1);

    let mut event_manager = EventManager::new(event_config).unwrap();
    let mut event_rx = event_manager.start().await.unwrap();

    // Receive events
    let event1 = tokio::time::timeout(
        Duration::from_millis(300),
        event_rx.recv()
    ).await.unwrap().unwrap();
    assert!(matches!(event1, Event::RateChange(500)));

    let event2 = tokio::time::timeout(
        Duration::from_millis(300),
        event_rx.recv()
    ).await.unwrap().unwrap();
    assert!(matches!(event2, Event::PhaseTransition(_)));

    event_manager.shutdown().await.unwrap();
}
