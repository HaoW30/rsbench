//! Event Module - Dynamic scenario control via external events
//!
//! The Event Module is a parallel, independent module that watches external systems
//! and generates events to control RSBench scenario execution dynamically.
//!
//! # Architecture
//!
//! ```text
//! External Systems → Event Sources → EventManager → mpsc → Scenario
//! ```
//!
//! The Event Module runs **alongside** (not inside) the Scenario Module, providing
//! clean separation of concerns and composability.
//!
//! # Event Sources
//!
//! - **Timer** (M0): Time-based events for scheduled rate changes
//! - **K8s Watcher** (M1): Kubernetes pod/deployment lifecycle events
//! - **Webhook** (M1): HTTP API for external control
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use rsbench::event::{EventManager, EventConfig};
//! use rsbench::scenario::ScenarioExecutor;
//!
//! // Create event manager from configuration
//! let event_config = EventConfig { /* ... */ };
//! let mut event_manager = EventManager::new(event_config);
//!
//! // Start event sources, get receiver
//! let event_rx = event_manager.start().await?;
//!
//! // Attach to scenario
//! let mut executor = ScenarioExecutor::new(scenario_config);
//! executor.attach_event_stream(event_rx);
//!
//! // Run scenario with event control
//! executor.execute().await?;
//!
//! // Graceful shutdown
//! event_manager.shutdown().await?;
//! ```

pub mod config;
pub mod sources;

use crate::scenario::{Event, Phase};
use crate::Result;
use config::{EventConfig, EventSourceConfig, EventTypeConfig};
use sources::timer::{ScheduledEvent, TimerEventSource};
use sources::EventSource;
use std::time::SystemTime;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// Event manager that orchestrates multiple event sources
///
/// The EventManager:
/// 1. Creates event sources from configuration
/// 2. Spawns each source as an independent async task
/// 3. Multiplexes events from all sources into a single channel
/// 4. Manages graceful shutdown of all sources
///
/// # Example
///
/// ```rust,ignore
/// let config = EventConfig {
///     sources: vec![
///         EventSourceConfig::Timer { schedule: vec![...] }
///     ]
/// };
///
/// let mut manager = EventManager::new(config);
/// let event_rx = manager.start().await?;
///
/// // Use event_rx with scenario...
///
/// manager.shutdown().await?;
/// ```
pub struct EventManager {
    sources: Vec<Box<dyn EventSource>>,
    shutdown_tx: broadcast::Sender<()>,
    join_handles: Vec<JoinHandle<Result<()>>>,
}

impl EventManager {
    /// Create a new event manager from configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Event configuration with sources to enable
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = EventConfig::default();
    /// let manager = EventManager::new(config);
    /// ```
    pub fn new(config: EventConfig) -> Result<Self> {
        let (shutdown_tx, _shutdown_rx) = broadcast::channel(16);
        let mut sources: Vec<Box<dyn EventSource>> = Vec::new();

        // Create event sources from configuration
        for source_config in config.sources {
            match source_config {
                EventSourceConfig::Timer { schedule } => {
                    let timer_source = Self::create_timer_source(schedule)?;
                    sources.push(Box::new(timer_source));
                }
                // Future: K8s, Webhook, etc.
            }
        }

        info!("[EventManager] Created with {} event source(s)", sources.len());

        Ok(Self {
            sources,
            shutdown_tx,
            join_handles: Vec::new(),
        })
    }

    /// Start all event sources
    ///
    /// Spawns each event source as an independent async task and returns a receiver
    /// that will receive events from all sources.
    ///
    /// # Returns
    ///
    /// Returns a receiver channel that the scenario can use to receive events.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let event_rx = manager.start().await?;
    /// executor.attach_event_stream(event_rx);
    /// ```
    pub async fn start(&mut self) -> Result<mpsc::UnboundedReceiver<Event>> {
        let (tx, rx) = mpsc::unbounded_channel();

        info!("[EventManager] Starting {} event source(s)", self.sources.len());

        for mut source in self.sources.drain(..) {
            let name = source.name().to_string();
            let tx_clone = tx.clone();
            let shutdown_rx = self.shutdown_tx.subscribe();

            info!("[EventManager] Spawning event source: {}", name);

            let handle = tokio::spawn(async move {
                let result = source.watch(tx_clone, shutdown_rx).await;

                match &result {
                    Ok(()) => info!("[EventManager] Event source '{}' completed successfully", name),
                    Err(e) => error!("[EventManager] Event source '{}' failed: {}", name, e),
                }

                result
            });

            self.join_handles.push(handle);
        }

        info!("[EventManager] All event sources started");
        Ok(rx)
    }

    /// Shutdown all event sources gracefully
    ///
    /// Sends shutdown signal to all sources and waits for them to complete.
    /// Times out after 5 seconds if sources don't complete.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// manager.shutdown().await?;
    /// ```
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("[EventManager] Initiating shutdown of {} source(s)", self.join_handles.len());

        // Send shutdown signal to all sources
        if let Err(e) = self.shutdown_tx.send(()) {
            warn!("[EventManager] Failed to send shutdown signal: {}", e);
        }

        // Wait for all sources to complete (with timeout)
        let timeout = std::time::Duration::from_secs(5);
        let shutdown_fut = async {
            for (idx, handle) in self.join_handles.drain(..).enumerate() {
                match handle.await {
                    Ok(Ok(())) => {
                        info!("[EventManager] Source {} shut down successfully", idx + 1);
                    }
                    Ok(Err(e)) => {
                        error!("[EventManager] Source {} failed: {}", idx + 1, e);
                    }
                    Err(e) => {
                        error!("[EventManager] Source {} join error: {}", idx + 1, e);
                    }
                }
            }
        };

        match tokio::time::timeout(timeout, shutdown_fut).await {
            Ok(()) => {
                info!("[EventManager] All sources shut down gracefully");
                Ok(())
            }
            Err(_) => {
                error!("[EventManager] Shutdown timeout after {:?}", timeout);
                Err(crate::Error::Event("Shutdown timeout".to_string()))
            }
        }
    }

    /// Check health of all event sources
    ///
    /// # Returns
    ///
    /// Vector of health status for each source
    pub async fn health(&self) -> Vec<SourceHealth> {
        // For M0, we can't check health of already-started sources
        // This would require keeping references to the sources
        // For now, return empty vec
        // M1 can improve this by keeping Arc references
        vec![]
    }

    /// Create timer event source from configuration
    fn create_timer_source(schedule: Vec<config::TimerEventConfig>) -> Result<TimerEventSource> {
        let mut events = Vec::new();

        for timer_config in schedule {
            // Parse duration
            let at = config::parse_duration(&timer_config.at)
                .map_err(|e| crate::Error::Config(format!("Invalid duration '{}': {}", timer_config.at, e)))?;

            // Convert config event to runtime event
            let event = match timer_config.event {
                EventTypeConfig::RateChange { rate } => Event::RateChange(rate),
                EventTypeConfig::PhaseTransition { phase } => {
                    let phase_enum = match phase.as_str() {
                        "pause" => Phase::Pause,
                        "resume" => Phase::Resume,
                        "shutdown" => Phase::Shutdown,
                        _ => return Err(crate::Error::Config(format!("Invalid phase: {}", phase))),
                    };
                    Event::PhaseTransition(phase_enum)
                }
                EventTypeConfig::MetricsSnapshot => Event::MetricsSnapshot,
                EventTypeConfig::Custom { data } => Event::Custom(data),
            };

            events.push(ScheduledEvent { at, event });
        }

        Ok(TimerEventSource::new(events))
    }
}

/// Health status for an event source
#[derive(Debug, Clone)]
pub struct SourceHealth {
    pub name: String,
    pub healthy: bool,
    pub last_event: Option<SystemTime>,
    pub error_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_manager_creation_empty() {
        let config = EventConfig::default();
        let manager = EventManager::new(config);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_event_manager_creation_with_timer() {
        let config = EventConfig {
            sources: vec![
                EventSourceConfig::Timer {
                    schedule: vec![
                        config::TimerEventConfig {
                            at: "10s".to_string(),
                            event: EventTypeConfig::RateChange { rate: 1000 },
                        },
                    ],
                },
            ],
        };

        let manager = EventManager::new(config);
        assert!(manager.is_ok());
    }

    #[test]
    fn test_event_manager_invalid_duration() {
        let config = EventConfig {
            sources: vec![
                EventSourceConfig::Timer {
                    schedule: vec![
                        config::TimerEventConfig {
                            at: "invalid".to_string(),
                            event: EventTypeConfig::RateChange { rate: 1000 },
                        },
                    ],
                },
            ],
        };

        let manager = EventManager::new(config);
        assert!(manager.is_err());
    }

    #[test]
    fn test_event_manager_invalid_phase() {
        let config = EventConfig {
            sources: vec![
                EventSourceConfig::Timer {
                    schedule: vec![
                        config::TimerEventConfig {
                            at: "10s".to_string(),
                            event: EventTypeConfig::PhaseTransition {
                                phase: "invalid_phase".to_string(),
                            },
                        },
                    ],
                },
            ],
        };

        let manager = EventManager::new(config);
        assert!(manager.is_err());
    }

    #[tokio::test]
    async fn test_event_manager_start_empty() {
        let config = EventConfig::default();
        let mut manager = EventManager::new(config).unwrap();

        let rx = manager.start().await;
        assert!(rx.is_ok());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_event_manager_start_with_timer() {
        let config = EventConfig {
            sources: vec![
                EventSourceConfig::Timer {
                    schedule: vec![
                        config::TimerEventConfig {
                            at: "50ms".to_string(),
                            event: EventTypeConfig::RateChange { rate: 1000 },
                        },
                    ],
                },
            ],
        };

        let mut manager = EventManager::new(config).unwrap();
        let mut rx = manager.start().await.unwrap();

        // Should receive event
        let event = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            rx.recv()
        ).await.unwrap().unwrap();

        assert!(matches!(event, Event::RateChange(1000)));

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_event_manager_multiple_events() {
        let config = EventConfig {
            sources: vec![
                EventSourceConfig::Timer {
                    schedule: vec![
                        config::TimerEventConfig {
                            at: "20ms".to_string(),
                            event: EventTypeConfig::RateChange { rate: 1000 },
                        },
                        config::TimerEventConfig {
                            at: "40ms".to_string(),
                            event: EventTypeConfig::RateChange { rate: 2000 },
                        },
                        config::TimerEventConfig {
                            at: "60ms".to_string(),
                            event: EventTypeConfig::MetricsSnapshot,
                        },
                    ],
                },
            ],
        };

        let mut manager = EventManager::new(config).unwrap();
        let mut rx = manager.start().await.unwrap();

        // Receive all three events
        let event1 = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            rx.recv()
        ).await.unwrap().unwrap();
        assert!(matches!(event1, Event::RateChange(1000)));

        let event2 = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            rx.recv()
        ).await.unwrap().unwrap();
        assert!(matches!(event2, Event::RateChange(2000)));

        let event3 = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            rx.recv()
        ).await.unwrap().unwrap();
        assert!(matches!(event3, Event::MetricsSnapshot));

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_event_manager_shutdown_before_events() {
        let config = EventConfig {
            sources: vec![
                EventSourceConfig::Timer {
                    schedule: vec![
                        config::TimerEventConfig {
                            at: "10s".to_string(),  // Long delay
                            event: EventTypeConfig::RateChange { rate: 1000 },
                        },
                    ],
                },
            ],
        };

        let mut manager = EventManager::new(config).unwrap();
        let _rx = manager.start().await.unwrap();

        // Shutdown before event fires
        let shutdown_result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            manager.shutdown()
        ).await;

        assert!(shutdown_result.is_ok());
        assert!(shutdown_result.unwrap().is_ok());
    }
}
