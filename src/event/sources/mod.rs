//! Event sources for dynamic scenario control
//!
//! Event sources watch external systems and emit events to control scenario execution.

use crate::scenario::Event;
use crate::Result;
use tokio::sync::{broadcast, mpsc};

pub mod timer;

/// Trait for event sources
///
/// Event sources are independent async tasks that watch external systems
/// (timers, Kubernetes, webhooks, etc.) and emit events to control scenarios.
///
/// # Example
///
/// ```rust,ignore
/// use rsbench::event::sources::EventSource;
///
/// struct MyEventSource;
///
/// #[async_trait]
/// impl EventSource for MyEventSource {
///     fn name(&self) -> &str {
///         "my_source"
///     }
///
///     async fn watch(
///         &mut self,
///         tx: mpsc::UnboundedSender<Event>,
///         mut shutdown: broadcast::Receiver<()>,
///     ) -> Result<()> {
///         // Watch for events and send to channel
///         Ok(())
///     }
///
///     async fn health_check(&self) -> bool {
///         true
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait EventSource: Send + Sync {
    /// Unique name for this event source
    fn name(&self) -> &str;

    /// Start watching for events
    ///
    /// This method runs as a long-lived async task that watches for events
    /// and sends them to the provided channel. It should return when the
    /// shutdown signal is received.
    ///
    /// # Arguments
    ///
    /// * `tx` - Channel to send events to the scenario
    /// * `shutdown` - Broadcast receiver for shutdown signal
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on graceful shutdown, or an error if something goes wrong.
    async fn watch(
        &mut self,
        tx: mpsc::UnboundedSender<Event>,
        shutdown: broadcast::Receiver<()>,
    ) -> Result<()>;

    /// Check if this event source is healthy
    ///
    /// Returns `true` if the source is operating normally, `false` otherwise.
    async fn health_check(&self) -> bool;
}
