//! Timer-based event source
//!
//! Emits events at scheduled times for time-driven scenario control.

use super::EventSource;
use crate::scenario::{Event, Phase};
use crate::Result;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info};

/// Timer-based event source
///
/// Emits events at scheduled times relative to when the source starts watching.
///
/// # Example
///
/// ```rust,ignore
/// use rsbench::event::sources::timer::{TimerEventSource, ScheduledEvent};
/// use rsbench::scenario::Event;
/// use std::time::Duration;
///
/// let source = TimerEventSource::new(vec![
///     ScheduledEvent {
///         at: Duration::from_secs(30),
///         event: Event::RateChange(1000),
///     },
///     ScheduledEvent {
///         at: Duration::from_secs(60),
///         event: Event::RateChange(5000),
///     },
/// ]);
/// ```
pub struct TimerEventSource {
    schedule: Vec<ScheduledEvent>,
    name: String,
}

/// A scheduled event to emit at a specific time
#[derive(Debug, Clone)]
pub struct ScheduledEvent {
    /// Time offset from when watch() is called
    pub at: Duration,
    /// Event to emit
    pub event: Event,
}

impl TimerEventSource {
    /// Create a new timer event source with the given schedule
    pub fn new(schedule: Vec<ScheduledEvent>) -> Self {
        Self {
            schedule,
            name: "timer".to_string(),
        }
    }

    /// Create with custom name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }
}

#[async_trait::async_trait]
impl EventSource for TimerEventSource {
    fn name(&self) -> &str {
        &self.name
    }

    async fn watch(
        &mut self,
        tx: mpsc::UnboundedSender<Event>,
        mut shutdown: broadcast::Receiver<()>,
    ) -> Result<()> {
        let start = Instant::now();
        info!("[Event/{}] Starting timer event source with {} events", self.name, self.schedule.len());

        for (idx, scheduled) in self.schedule.iter().enumerate() {
            // Calculate how long to sleep
            let elapsed = start.elapsed();
            let delay = scheduled.at.saturating_sub(elapsed);

            debug!(
                "[Event/{}] Event {}/{}: Waiting {:?} until {:?}",
                self.name,
                idx + 1,
                self.schedule.len(),
                delay,
                scheduled.at
            );

            // Wait until scheduled time or shutdown
            tokio::select! {
                _ = tokio::time::sleep(delay) => {
                    // Time to emit event
                    debug!(
                        "[Event/{}] Emitting event {}/{}: {:?}",
                        self.name,
                        idx + 1,
                        self.schedule.len(),
                        event_type_name(&scheduled.event)
                    );

                    if let Err(e) = tx.send(scheduled.event.clone()) {
                        tracing::error!("[Event/{}] Failed to send event: {}", self.name, e);
                        return Err(crate::Error::Event(format!("Failed to send event: {}", e)));
                    }

                    info!(
                        "[Event/{}] Event {}/{} emitted at {:?}",
                        self.name,
                        idx + 1,
                        self.schedule.len(),
                        start.elapsed()
                    );
                }
                _ = shutdown.recv() => {
                    info!("[Event/{}] Shutdown signal received, stopping", self.name);
                    return Ok(());
                }
            }
        }

        info!("[Event/{}] All {} events emitted", self.name, self.schedule.len());
        Ok(())
    }

    async fn health_check(&self) -> bool {
        // Timer source is always healthy if constructed
        true
    }
}

/// Helper to get event type name for logging
fn event_type_name(event: &Event) -> &str {
    match event {
        Event::RateChange(_) => "RateChange",
        Event::PhaseTransition(Phase::Pause) => "Pause",
        Event::PhaseTransition(Phase::Resume) => "Resume",
        Event::PhaseTransition(Phase::Shutdown) => "Shutdown",
        Event::MetricsSnapshot => "MetricsSnapshot",
        Event::Custom(_) => "Custom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_event_source_creation() {
        let schedule = vec![
            ScheduledEvent {
                at: Duration::from_secs(1),
                event: Event::RateChange(1000),
            },
        ];

        let source = TimerEventSource::new(schedule);
        assert_eq!(source.name(), "timer");
    }

    #[test]
    fn test_timer_event_source_custom_name() {
        let source = TimerEventSource::new(vec![])
            .with_name("custom_timer".to_string());
        assert_eq!(source.name(), "custom_timer");
    }

    #[tokio::test]
    async fn test_timer_emits_single_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let schedule = vec![
            ScheduledEvent {
                at: Duration::from_millis(50),
                event: Event::RateChange(1000),
            },
        ];

        let mut source = TimerEventSource::new(schedule);

        // Spawn watch task
        let watch_handle = tokio::spawn(async move {
            source.watch(tx, shutdown_rx).await
        });

        // Should receive event around 50ms
        let event = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed");

        assert!(matches!(event, Event::RateChange(1000)));

        // Wait for watch to complete
        watch_handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_timer_emits_multiple_events_in_order() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let schedule = vec![
            ScheduledEvent {
                at: Duration::from_millis(20),
                event: Event::RateChange(1000),
            },
            ScheduledEvent {
                at: Duration::from_millis(40),
                event: Event::RateChange(2000),
            },
            ScheduledEvent {
                at: Duration::from_millis(60),
                event: Event::RateChange(3000),
            },
        ];

        let mut source = TimerEventSource::new(schedule);

        // Spawn watch task
        let watch_handle = tokio::spawn(async move {
            source.watch(tx, shutdown_rx).await
        });

        // Receive events in order
        let event1 = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await.unwrap().unwrap();
        assert!(matches!(event1, Event::RateChange(1000)));

        let event2 = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await.unwrap().unwrap();
        assert!(matches!(event2, Event::RateChange(2000)));

        let event3 = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await.unwrap().unwrap();
        assert!(matches!(event3, Event::RateChange(3000)));

        // Wait for watch to complete
        watch_handle.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_timer_respects_shutdown_signal() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let schedule = vec![
            ScheduledEvent {
                at: Duration::from_millis(50),
                event: Event::RateChange(1000),
            },
            ScheduledEvent {
                at: Duration::from_secs(10),  // Long delay
                event: Event::RateChange(2000),
            },
        ];

        let mut source = TimerEventSource::new(schedule);

        // Spawn watch task
        let watch_handle = tokio::spawn(async move {
            source.watch(tx, shutdown_rx).await
        });

        // Receive first event
        let event1 = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await.unwrap().unwrap();
        assert!(matches!(event1, Event::RateChange(1000)));

        // Send shutdown before second event
        shutdown_tx.send(()).unwrap();

        // Watch should complete gracefully
        let result = tokio::time::timeout(Duration::from_millis(200), watch_handle)
            .await
            .expect("Watch should complete quickly after shutdown");
        assert!(result.unwrap().is_ok());

        // Should not receive second event
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_timer_handles_empty_schedule() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let mut source = TimerEventSource::new(vec![]);

        // Should complete immediately
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            source.watch(tx, shutdown_rx)
        ).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
    }

    #[tokio::test]
    async fn test_timer_health_check() {
        let source = TimerEventSource::new(vec![]);
        assert!(source.health_check().await);
    }

    #[tokio::test]
    async fn test_timer_emits_different_event_types() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let schedule = vec![
            ScheduledEvent {
                at: Duration::from_millis(20),
                event: Event::RateChange(1000),
            },
            ScheduledEvent {
                at: Duration::from_millis(40),
                event: Event::PhaseTransition(Phase::Pause),
            },
            ScheduledEvent {
                at: Duration::from_millis(60),
                event: Event::MetricsSnapshot,
            },
        ];

        let mut source = TimerEventSource::new(schedule);
        let watch_handle = tokio::spawn(async move {
            source.watch(tx, shutdown_rx).await
        });

        // Verify all event types
        let event1 = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await.unwrap().unwrap();
        assert!(matches!(event1, Event::RateChange(1000)));

        let event2 = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await.unwrap().unwrap();
        assert!(matches!(event2, Event::PhaseTransition(Phase::Pause)));

        let event3 = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await.unwrap().unwrap();
        assert!(matches!(event3, Event::MetricsSnapshot));

        watch_handle.await.unwrap().unwrap();
    }
}
