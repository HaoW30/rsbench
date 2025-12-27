//! Scenario executor module
//!
//! Orchestrates workload execution with precise rate control and event handling.
//!
//! This module provides the core execution engine for database benchmarks, supporting
//! three execution modes: constant-rate, ramping-rate, and closed-loop. It coordinates
//! workload generation, runtime execution, metrics collection, and dynamic control via events.
//!
//! # Execution Modes
//!
//! ## Constant Rate
//! Maintains a fixed operation rate (ops/sec) for a specified duration.
//! Operations are submitted in a fire-and-forget manner, with precise rate limiting.
//!
//! ```rust,no_run
//! use rsbench::config::{ExecutorConfig, ScenarioConfig};
//! use rsbench::scenario::ScenarioExecutor;
//! use std::time::Duration;
//! # use rsbench::Result;
//! # async fn example() -> Result<()> {
//! # let workload = todo!();
//! # let runtime = todo!();
//! # let metrics = todo!();
//!
//! let config = ScenarioConfig {
//!     executor: ExecutorConfig::ConstantRate {
//!         rate: 1000,  // 1000 ops/sec
//!         duration: Duration::from_secs(60),
//!         max_connections: 100,
//!     },
//!     workload: todo!(),
//! };
//!
//! let mut executor = ScenarioExecutor::new(config, workload, runtime, metrics);
//! let result = executor.execute().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Ramping Rate
//! Gradually increases or decreases operation rate through multiple stages.
//! Useful for warmup, ramp-up testing, and capacity planning.
//!
//! ```rust,no_run
//! use rsbench::config::{ExecutorConfig, RateStage, ScenarioConfig};
//! use std::time::Duration;
//! # use rsbench::Result;
//! # async fn example() -> Result<()> {
//! # let workload = todo!();
//! # let runtime = todo!();
//! # let metrics = todo!();
//!
//! let config = ScenarioConfig {
//!     executor: ExecutorConfig::RampingRate {
//!         stages: vec![
//!             RateStage { target_rate: 100, duration: Duration::from_secs(30) },
//!             RateStage { target_rate: 500, duration: Duration::from_secs(30) },
//!             RateStage { target_rate: 1000, duration: Duration::from_secs(60) },
//!         ],
//!         prealloc_connections: 10,
//!         max_connections: 100,
//!     },
//!     workload: todo!(),
//! };
//!
//! let mut executor = ScenarioExecutor::new(config, workload, runtime, metrics);
//! let result = executor.execute().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Closed Loop
//! Runs N concurrent workers, each executing operations sequentially.
//! Similar to sysbench's `--threads` mode. Operations are synchronous per worker.
//!
//! ```rust,no_run
//! use rsbench::config::{ExecutorConfig, ScenarioConfig};
//! use std::time::Duration;
//! # use rsbench::Result;
//! # async fn example() -> Result<()> {
//! # let workload = todo!();
//! # let runtime = todo!();
//! # let metrics = todo!();
//!
//! let config = ScenarioConfig {
//!     executor: ExecutorConfig::ClosedLoop {
//!         workers: 16,  // 16 concurrent workers
//!         duration: Duration::from_secs(60),
//!         think_time: Some(Duration::from_millis(10)),
//!         max_connections: 20,
//!     },
//!     workload: todo!(),
//! };
//!
//! let mut executor = ScenarioExecutor::new(config, workload, runtime, metrics);
//! let result = executor.execute().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Event-Driven Control
//!
//! Scenarios can be controlled dynamically via events for pause/resume,
//! rate changes, graceful shutdown, and metrics snapshots.
//!
//! ```rust,no_run
//! use rsbench::scenario::{Event, Phase, ScenarioExecutor};
//! use tokio::sync::mpsc;
//! # use rsbench::Result;
//! # async fn example() -> Result<()> {
//! # let config = todo!();
//! # let workload = todo!();
//! # let runtime = todo!();
//! # let metrics = todo!();
//!
//! let mut executor = ScenarioExecutor::new(config, workload, runtime, metrics);
//!
//! // Attach event stream for dynamic control
//! let (tx, rx) = mpsc::channel(100);
//! executor.attach_event_stream(rx);
//!
//! // Send events from another task
//! tokio::spawn(async move {
//!     // Pause execution
//!     tx.send(Event::PhaseTransition(Phase::Pause)).await.unwrap();
//!
//!     // Resume after 5 seconds
//!     tokio::time::sleep(std::time::Duration::from_secs(5)).await;
//!     tx.send(Event::PhaseTransition(Phase::Resume)).await.unwrap();
//! });
//!
//! let result = executor.execute().await?;
//! # Ok(())
//! # }
//! ```

use crate::config::{ExecutorConfig, RateStage, ScenarioConfig};
use crate::metrics::{MetricsCollector, MetricsSnapshot};
use crate::rate_limiter::RateLimiter;
use crate::runtime::RuntimeEngine;
use crate::workload::{ExecutionContext, Workload};
use crate::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Events for dynamic scenario control
///
/// Events allow external systems to control scenario execution in real-time,
/// enabling pause/resume, rate adjustments, graceful shutdown, and metrics snapshots.
///
/// # Examples
///
/// ```rust
/// use rsbench::scenario::{Event, Phase};
///
/// // Rate change event
/// let event = Event::RateChange(2000); // Change to 2000 ops/sec
///
/// // Pause execution
/// let event = Event::PhaseTransition(Phase::Pause);
///
/// // Take metrics snapshot
/// let event = Event::MetricsSnapshot;
/// ```
#[derive(Debug, Clone)]
pub enum Event {
    /// Change operation rate dynamically
    ///
    /// Updates the rate limiter to the new target rate. Only affects
    /// constant-rate and ramping-rate executors. Ignored in closed-loop mode.
    ///
    /// # Example
    /// ```rust
    /// use rsbench::scenario::Event;
    /// let event = Event::RateChange(5000); // 5000 ops/sec
    /// ```
    RateChange(u64),

    /// Transition to a new execution phase
    ///
    /// Controls execution flow: pause operation submission, resume execution,
    /// or request graceful shutdown.
    ///
    /// # Example
    /// ```rust
    /// use rsbench::scenario::{Event, Phase};
    /// let event = Event::PhaseTransition(Phase::Shutdown);
    /// ```
    PhaseTransition(Phase),

    /// Take an intermediate metrics snapshot
    ///
    /// Captures current metrics and logs them. Useful for monitoring progress
    /// during long-running benchmarks.
    ///
    /// # Example
    /// ```rust
    /// use rsbench::scenario::Event;
    /// let event = Event::MetricsSnapshot;
    /// ```
    MetricsSnapshot,

    /// Custom application-defined event
    ///
    /// Reserved for future use or application-specific event handling.
    /// Currently logged at debug level.
    ///
    /// # Example
    /// ```rust
    /// use rsbench::scenario::Event;
    /// use serde_json::json;
    /// let event = Event::Custom(json!({"action": "checkpoint"}));
    /// ```
    Custom(serde_json::Value),
}

/// Execution phase control
///
/// Defines the execution state of a scenario, allowing dynamic control
/// over operation submission.
#[derive(Debug, Clone)]
pub enum Phase {
    /// Pause operation submission
    ///
    /// Stops submitting new operations while keeping the executor running.
    /// In-flight operations continue to completion. The executor sleeps
    /// briefly between pause checks to avoid busy-waiting.
    Pause,

    /// Resume operation submission
    ///
    /// Resumes normal operation submission after a pause. The executor
    /// continues from where it left off in the duration/iteration count.
    Resume,

    /// Request graceful shutdown
    ///
    /// Signals the executor to stop as soon as possible. The current iteration
    /// completes, in-flight operations finish, but no new operations are submitted.
    /// The scenario returns earlier than the configured duration.
    Shutdown,
}

/// Scenario executor - orchestrates workload execution
///
/// The core execution engine that coordinates workload generation, runtime execution,
/// metrics collection, and rate limiting. Supports three execution modes:
/// - **Constant Rate**: Fixed ops/sec for specified duration
/// - **Ramping Rate**: Dynamic rate changes through multiple stages
/// - **Closed Loop**: N workers executing operations sequentially
///
/// # Lifecycle
///
/// 1. Create executor with [`new()`](ScenarioExecutor::new)
/// 2. Optionally attach event stream with [`attach_event_stream()`](ScenarioExecutor::attach_event_stream)
/// 3. Execute scenario with [`execute()`](ScenarioExecutor::execute)
/// 4. Analyze results from [`ScenarioResult`]
///
/// # Thread Safety
///
/// ScenarioExecutor is not `Send` or `Sync` due to internal state management.
/// It must be created and executed on the same tokio task.
///
/// # Examples
///
/// See module-level documentation for detailed examples of each execution mode.
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
    /// Creates a new scenario executor
    ///
    /// Initializes the executor with configuration, workload, runtime, and metrics.
    /// The rate limiter is automatically configured based on the executor type.
    ///
    /// # Arguments
    ///
    /// * `config` - Scenario configuration including executor type and parameters
    /// * `workload` - Workload implementation for generating operations
    /// * `runtime` - Runtime engine for executing operations
    /// * `metrics` - Metrics collector for tracking execution statistics
    ///
    /// # Rate Limiter Initialization
    ///
    /// - **ConstantRate**: Set to configured rate
    /// - **RampingRate**: Set to first stage's rate (or 100 if no stages)
    /// - **ClosedLoop**: Set to u64::MAX (unlimited, workers control rate)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsbench::config::{ExecutorConfig, ScenarioConfig};
    /// use rsbench::scenario::ScenarioExecutor;
    /// use std::time::Duration;
    /// # use rsbench::Result;
    /// # fn example() -> Result<()> {
    /// # let workload = todo!();
    /// # let runtime = todo!();
    /// # let metrics = todo!();
    ///
    /// let config = ScenarioConfig {
    ///     executor: ExecutorConfig::ConstantRate {
    ///         rate: 1000,
    ///         duration: Duration::from_secs(60),
    ///         max_connections: 100,
    ///     },
    ///     workload: todo!(),
    /// };
    ///
    /// let executor = ScenarioExecutor::new(config, workload, runtime, metrics);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(
        config: ScenarioConfig,
        workload: Box<dyn Workload>,
        runtime: Arc<dyn RuntimeEngine>,
        metrics: Arc<MetricsCollector>,
    ) -> Self {
        let rate_limiter = match &config.executor {
            ExecutorConfig::ConstantRate { rate, .. } => RateLimiter::new(*rate),
            ExecutorConfig::RampingRate { stages, .. } => {
                let initial_rate = stages.first().map(|s| s.target_rate).unwrap_or(100);
                RateLimiter::new(initial_rate)
            }
            // ClosedLoop doesn't use rate limiting (workers drive the rate)
            ExecutorConfig::ClosedLoop { .. } => RateLimiter::new(u64::MAX),
        };

        Self {
            config,
            workload,
            runtime,
            metrics,
            rate_limiter,
            event_rx: None,
            paused: false,
            shutdown_requested: false,
        }
    }

    /// Attaches an event stream for dynamic runtime control
    ///
    /// Enables external control of the scenario via events. Events are processed
    /// non-blockingly during execution, allowing pause/resume, rate changes,
    /// graceful shutdown, and metrics snapshots.
    ///
    /// # Arguments
    ///
    /// * `rx` - Receiver end of an mpsc channel for receiving events
    ///
    /// # Event Processing
    ///
    /// Events are processed via `try_recv()` to avoid blocking execution.
    /// The executor checks for events at the start of each iteration in
    /// constant-rate and ramping-rate modes. Closed-loop mode does not
    /// currently support events (M1 feature).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsbench::scenario::{Event, Phase, ScenarioExecutor};
    /// use tokio::sync::mpsc;
    /// # use rsbench::Result;
    /// # async fn example() -> Result<()> {
    /// # let config = todo!();
    /// # let workload = todo!();
    /// # let runtime = todo!();
    /// # let metrics = todo!();
    ///
    /// let mut executor = ScenarioExecutor::new(config, workload, runtime, metrics);
    ///
    /// // Create event channel
    /// let (tx, rx) = mpsc::channel(100);
    /// executor.attach_event_stream(rx);
    ///
    /// // Send events from another task
    /// tokio::spawn(async move {
    ///     tx.send(Event::PhaseTransition(Phase::Pause)).await.unwrap();
    ///     tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    ///     tx.send(Event::PhaseTransition(Phase::Resume)).await.unwrap();
    /// });
    ///
    /// let result = executor.execute().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn attach_event_stream(&mut self, rx: mpsc::Receiver<Event>) {
        self.event_rx = Some(rx);
    }

    /// Handle incoming events (non-blocking)
    async fn handle_events(&mut self) -> Result<()> {
        if let Some(ref mut rx) = self.event_rx {
            // Try to receive events without blocking
            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::RateChange(new_rate) => {
                        self.rate_limiter.set_rate(new_rate);
                        tracing::info!("Rate changed to {} ops/sec", new_rate);
                    }
                    Event::PhaseTransition(phase) => match phase {
                        Phase::Pause => {
                            self.paused = true;
                            tracing::info!("Scenario paused");
                        }
                        Phase::Resume => {
                            self.paused = false;
                            tracing::info!("Scenario resumed");
                        }
                        Phase::Shutdown => {
                            self.shutdown_requested = true;
                            tracing::info!("Graceful shutdown requested");
                        }
                    },
                    Event::MetricsSnapshot => {
                        // Take intermediate snapshot and log summary
                        let snapshot = self.metrics.snapshot();
                        let total_ops: u64 = snapshot
                            .operation_metrics
                            .values()
                            .map(|m| m.count)
                            .sum();
                        let total_errors: u64 = snapshot
                            .operation_metrics
                            .values()
                            .map(|m| m.errors)
                            .sum();
                        tracing::info!(
                            "Intermediate metrics snapshot: {} operations, {} errors, {} backpressure events, elapsed={:?}",
                            total_ops,
                            total_errors,
                            snapshot.backpressure_events,
                            snapshot.duration
                        );
                    }
                    Event::Custom(data) => {
                        tracing::debug!("Custom event received: {:?}", data);
                        // TODO: M1 - Handle custom events based on application logic
                    }
                }
            }
        }
        Ok(())
    }

    /// Executes the scenario and returns results
    ///
    /// This is the main entry point for scenario execution. It dispatches to the
    /// appropriate executor based on configuration, runs the workload, collects metrics,
    /// and returns comprehensive results.
    ///
    /// # Returns
    ///
    /// Returns [`ScenarioResult`] containing:
    /// - Execution duration
    /// - Operations completed/failed counts
    /// - Success rate and throughput
    /// - Full metrics snapshot
    /// - Backpressure events
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Workload generation fails (e.g., invalid operation templates)
    /// - Rate limiter encounters issues
    /// - Fatal runtime errors occur (non-fatal errors are logged)
    ///
    /// # Execution Flow
    ///
    /// 1. Log scenario start
    /// 2. Dispatch to executor based on config type
    /// 3. Run workload with rate control
    /// 4. Collect final metrics
    /// 5. Log completion summary
    /// 6. Return results
    ///
    /// # M0 Limitations
    ///
    /// - `workload.prepare()` is not called (tables must be created manually)
    /// - Closed-loop mode does not support event handling
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsbench::scenario::ScenarioExecutor;
    /// # use rsbench::Result;
    /// # async fn example() -> Result<()> {
    /// # let config = todo!();
    /// # let workload = todo!();
    /// # let runtime = todo!();
    /// # let metrics = todo!();
    ///
    /// let mut executor = ScenarioExecutor::new(config, workload, runtime, metrics);
    /// let result = executor.execute().await?;
    ///
    /// println!("Completed {} operations", result.operations_completed);
    /// println!("Success rate: {:.2}%", result.success_rate() * 100.0);
    /// println!("Throughput: {:.2} ops/sec", result.throughput());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute(&mut self) -> Result<ScenarioResult> {
        tracing::info!("Starting scenario execution");

        // TODO M1: Call workload.prepare() to create tables and load data
        // This requires adding ConnectionPool to ScenarioExecutor constructor
        // and implementing PrepareDatabase adapter. For M0, tables must be
        // created manually before running benchmarks.
        // Reference: src/workload/mod.rs PrepareContext

        // Execute based on executor type
        let executor = self.config.executor.clone();
        let result = match executor {
            ExecutorConfig::ConstantRate {
                rate, duration, ..
            } => {
                tracing::info!("Executing constant-rate scenario: rate={}/s, duration={:?}", rate, duration);
                self.execute_constant_rate(rate, duration).await?
            }
            ExecutorConfig::RampingRate { stages, .. } => {
                let total_duration: Duration = stages.iter().map(|s| s.duration).sum();
                tracing::info!("Executing ramping-rate scenario: {} stages, total duration={:?}", stages.len(), total_duration);
                self.execute_ramping_rate(&stages).await?
            }
            ExecutorConfig::ClosedLoop {
                workers, duration, ..
            } => {
                tracing::info!("Executing closed-loop scenario: {} workers, duration={:?}", workers, duration);
                self.execute_closed_loop(workers, duration).await?
            }
        };

        tracing::info!(
            "Scenario execution completed: {} operations in {:?}, success_rate={:.2}%",
            result.operations_completed,
            result.duration,
            result.success_rate() * 100.0
        );

        Ok(result)
    }

    async fn execute_constant_rate(&mut self, rate: u64, duration: Duration) -> Result<ScenarioResult> {
        let start = Instant::now();
        let end_time = start + duration;
        let mut iteration = 0u64;

        // Set initial rate
        self.rate_limiter.set_rate(rate);

        while Instant::now() < end_time {
            // Check for events (pause, resume, shutdown, rate change)
            self.handle_events().await?;

            // Handle shutdown request
            if self.shutdown_requested {
                break;
            }

            // Handle pause state
            if self.paused {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            // Rate limiting (time-driven)
            let _permit = self.rate_limiter.acquire().await;

            // Generate operation
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration,
                elapsed: start.elapsed(),
            };

            let op = self.workload.next_operation(&ctx)?;

            // Submit to runtime (fire-and-forget)
            let runtime = self.runtime.clone();
            tokio::spawn(async move {
                let _ = runtime.submit(op).await;
                // Errors are recorded in metrics collector
            });

            iteration += 1;
        }

        // Wait for in-flight operations to complete
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Collect results
        let actual_end = Instant::now();
        let metrics_snapshot = self.metrics.snapshot();
        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: iteration,
            operations_failed: 0, // M0: simplified, actual errors in metrics
            metrics: metrics_snapshot.clone(),
            start_time: start,
            end_time: actual_end,
            backpressure_events: metrics_snapshot.backpressure_events,
        })
    }

    async fn execute_ramping_rate(&mut self, stages: &[RateStage]) -> Result<ScenarioResult> {
        let start = Instant::now();
        let mut iteration = 0u64;
        let mut current_stage_index = 0;
        let mut last_stage_rate = 0u64;

        loop {
            // Check for events (pause, resume, shutdown, rate change)
            self.handle_events().await?;

            // Handle shutdown request
            if self.shutdown_requested {
                break;
            }

            // Handle pause state
            if self.paused {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            // Determine current stage based on elapsed time
            let elapsed = start.elapsed();
            let current_stage = self.get_current_stage(stages, elapsed, &mut current_stage_index);

            // If all stages complete, break
            if current_stage.is_none() {
                break;
            }

            let stage = current_stage.unwrap();

            // Update rate limiter if stage changed
            if stage.target_rate != last_stage_rate {
                self.rate_limiter.set_rate(stage.target_rate);
                last_stage_rate = stage.target_rate;
            }

            // Rate limiting (time-driven)
            let _permit = self.rate_limiter.acquire().await;

            // Generate operation
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration,
                elapsed,
            };

            let op = self.workload.next_operation(&ctx)?;

            // Submit to runtime (fire-and-forget)
            let runtime = self.runtime.clone();
            tokio::spawn(async move {
                let _ = runtime.submit(op).await;
                // Errors are recorded in metrics collector
            });

            iteration += 1;
        }

        // Wait for in-flight operations to complete
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Collect results
        let actual_end = Instant::now();
        let metrics_snapshot = self.metrics.snapshot();
        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: iteration,
            operations_failed: 0, // M0: simplified, actual errors in metrics
            metrics: metrics_snapshot.clone(),
            start_time: start,
            end_time: actual_end,
            backpressure_events: metrics_snapshot.backpressure_events,
        })
    }

    /// Get the current stage based on elapsed time
    fn get_current_stage<'a>(
        &self,
        stages: &'a [RateStage],
        elapsed: Duration,
        current_index: &mut usize,
    ) -> Option<&'a RateStage> {
        let mut cumulative_duration = Duration::from_secs(0);

        for (index, stage) in stages.iter().enumerate() {
            cumulative_duration += stage.duration;
            if elapsed < cumulative_duration {
                *current_index = index;
                return Some(stage);
            }
        }

        None // All stages complete
    }

    async fn execute_closed_loop(&mut self, workers: usize, duration: Duration) -> Result<ScenarioResult> {
        let start = Instant::now();
        let end_time = start + duration;

        // Extract think_time from config
        let think_time = if let ExecutorConfig::ClosedLoop { think_time, .. } = &self.config.executor {
            *think_time
        } else {
            None
        };

        // TODO M1: Add event handling support for closed-loop (pause/resume/shutdown)
        // For M0, workers run for full duration without event handling

        // Spawn worker tasks
        let mut handles = vec![];

        for worker_id in 0..workers {
            // Create independent workload for this worker with unique seed
            let workload_config = self.config.workload.clone();
            let worker_workload = match crate::workload::WorkloadFactory::create(&workload_config, worker_id as u64) {
                Ok(w) => w,
                Err(e) => {
                    // If we can't create workload, log and continue with remaining workers
                    tracing::warn!("Failed to create workload for worker {}: {}", worker_id, e);
                    continue;
                }
            };

            let runtime = self.runtime.clone();

            // Spawn worker task
            let handle = tokio::spawn(async move {
                let mut iteration = 0u64;
                let task_start = Instant::now();
                let mut workload = worker_workload;

                while Instant::now() < end_time {
                    // Generate operation
                    let ctx = crate::workload::ExecutionContext {
                        worker_id,
                        iteration,
                        elapsed: task_start.elapsed(),
                    };

                    let operation = match workload.next_operation(&ctx) {
                        Ok(op) => op,
                        Err(e) => {
                            tracing::warn!("Worker {} failed to generate operation: {}", worker_id, e);
                            break;
                        }
                    };

                    // Submit operation and WAIT for completion (closed-loop)
                    // Note: Runtime records metrics internally
                    let result = runtime.submit(operation).await;

                    // Log errors (metrics are already recorded by runtime)
                    if let Err(e) = result {
                        tracing::warn!("Worker {} operation failed: {}", worker_id, e);
                    }

                    // Optional think time (like sysbench --think-time)
                    if let Some(delay) = think_time {
                        tokio::time::sleep(delay).await;
                    }

                    iteration += 1;
                }

                Ok::<u64, crate::Error>(iteration)
            });

            handles.push(handle);
        }

        // Wait for all workers to complete
        let mut total_iterations = 0u64;
        for (idx, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(Ok(iterations)) => {
                    total_iterations += iterations;
                }
                Ok(Err(e)) => {
                    tracing::warn!("Worker {} failed: {}", idx, e);
                }
                Err(e) => {
                    tracing::warn!("Worker {} panicked: {}", idx, e);
                }
            }
        }

        // Brief cooldown for any remaining metrics
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Collect results
        let actual_end = Instant::now();
        let metrics_snapshot = self.metrics.snapshot();

        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: total_iterations,
            operations_failed: 0, // M0: simplified, actual errors in metrics
            metrics: metrics_snapshot.clone(),
            start_time: start,
            end_time: actual_end,
            backpressure_events: metrics_snapshot.backpressure_events,
        })
    }
}

/// Scenario execution results
///
/// Contains comprehensive statistics and metrics from a scenario execution,
/// including operation counts, timing information, and detailed metrics.
///
/// # Examples
///
/// ```rust,no_run
/// use rsbench::scenario::ScenarioExecutor;
/// # use rsbench::Result;
/// # async fn example() -> Result<()> {
/// # let config = todo!();
/// # let workload = todo!();
/// # let runtime = todo!();
/// # let metrics = todo!();
///
/// let mut executor = ScenarioExecutor::new(config, workload, runtime, metrics);
/// let result = executor.execute().await?;
///
/// println!("Duration: {:?}", result.duration);
/// println!("Operations: {}", result.operations_completed);
/// println!("Success rate: {:.2}%", result.success_rate() * 100.0);
/// println!("Throughput: {:.2} ops/sec", result.throughput());
///
/// if result.had_backpressure() {
///     println!("Backpressure: {:.2}%", result.backpressure_percentage());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Total duration of the scenario execution
    ///
    /// Measured from start to completion, including any pause time if paused.
    pub duration: Duration,

    /// Number of operations that completed
    ///
    /// In open-loop mode, this is the number of operations submitted.
    /// In closed-loop mode, this is the sum of all worker iterations.
    pub operations_completed: u64,

    /// Number of operations that failed
    ///
    /// M0 Note: Currently always 0. Detailed error counts are available
    /// in the metrics snapshot.
    pub operations_failed: u64,

    /// Complete metrics snapshot at end of execution
    ///
    /// Contains per-operation statistics, latency histograms, and error counts.
    pub metrics: MetricsSnapshot,

    /// Execution start time
    ///
    /// Instant when scenario execution began (before any operations).
    pub start_time: Instant,

    /// Execution end time
    ///
    /// Instant when scenario execution completed (after final operation).
    pub end_time: Instant,

    /// Number of backpressure events detected
    ///
    /// Indicates how many times the runtime detected backpressure during execution.
    /// Non-zero values suggest the workload exceeded database capacity.
    pub backpressure_events: u64,
}

impl ScenarioResult {
    /// Calculates the success rate as a fraction (0.0 to 1.0)
    ///
    /// Returns the ratio of completed operations to total operations.
    /// A value of 1.0 means all operations succeeded, 0.0 means all failed.
    ///
    /// # Returns
    ///
    /// Success rate between 0.0 and 1.0, or 0.0 if no operations were executed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use rsbench::scenario::ScenarioResult;
    /// # use std::time::{Duration, Instant};
    /// # let result = ScenarioResult {
    /// #     duration: Duration::from_secs(60),
    /// #     operations_completed: 950,
    /// #     operations_failed: 50,
    /// #     metrics: todo!(),
    /// #     start_time: Instant::now(),
    /// #     end_time: Instant::now(),
    /// #     backpressure_events: 0,
    /// # };
    ///
    /// let success_rate = result.success_rate();
    /// println!("Success rate: {:.2}%", success_rate * 100.0);
    /// // Output: Success rate: 95.00%
    /// ```
    pub fn success_rate(&self) -> f64 {
        let total = self.operations_completed + self.operations_failed;
        if total == 0 {
            return 0.0;
        }
        self.operations_completed as f64 / total as f64
    }

    /// Calculates throughput in operations per second
    ///
    /// Divides total operations by execution duration to compute average throughput.
    ///
    /// # Returns
    ///
    /// Throughput in ops/sec, or 0.0 if duration was zero.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use rsbench::scenario::ScenarioResult;
    /// # use std::time::{Duration, Instant};
    /// # let result = ScenarioResult {
    /// #     duration: Duration::from_secs(60),
    /// #     operations_completed: 6000,
    /// #     operations_failed: 0,
    /// #     metrics: todo!(),
    /// #     start_time: Instant::now(),
    /// #     end_time: Instant::now(),
    /// #     backpressure_events: 0,
    /// # };
    ///
    /// let throughput = result.throughput();
    /// println!("Throughput: {:.2} ops/sec", throughput);
    /// // Output: Throughput: 100.00 ops/sec
    /// ```
    pub fn throughput(&self) -> f64 {
        let duration_secs = self.duration.as_secs_f64();
        if duration_secs == 0.0 {
            return 0.0;
        }
        self.operations_completed as f64 / duration_secs
    }

    /// Checks if any backpressure was detected during execution
    ///
    /// Returns true if the runtime detected backpressure at any point,
    /// indicating the workload exceeded database capacity.
    ///
    /// # Returns
    ///
    /// `true` if backpressure was detected, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use rsbench::scenario::ScenarioResult;
    /// # use std::time::{Duration, Instant};
    /// # let result = ScenarioResult {
    /// #     duration: Duration::from_secs(60),
    /// #     operations_completed: 1000,
    /// #     operations_failed: 0,
    /// #     metrics: todo!(),
    /// #     start_time: Instant::now(),
    /// #     end_time: Instant::now(),
    /// #     backpressure_events: 5,
    /// # };
    ///
    /// if result.had_backpressure() {
    ///     println!("Warning: Backpressure detected!");
    ///     println!("Backpressure: {:.2}%", result.backpressure_percentage());
    /// }
    /// ```
    pub fn had_backpressure(&self) -> bool {
        self.backpressure_events > 0
    }

    /// Calculates the percentage of operations affected by backpressure
    ///
    /// Returns the ratio of backpressure events to total operations as a percentage.
    /// High values indicate the database was frequently overloaded.
    ///
    /// # Returns
    ///
    /// Percentage (0.0 to 100.0), or 0.0 if no operations were executed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use rsbench::scenario::ScenarioResult;
    /// # use std::time::{Duration, Instant};
    /// # let result = ScenarioResult {
    /// #     duration: Duration::from_secs(60),
    /// #     operations_completed: 1000,
    /// #     operations_failed: 0,
    /// #     metrics: todo!(),
    /// #     start_time: Instant::now(),
    /// #     end_time: Instant::now(),
    /// #     backpressure_events: 50,
    /// # };
    ///
    /// let bp_pct = result.backpressure_percentage();
    /// println!("Backpressure: {:.2}%", bp_pct);
    /// // Output: Backpressure: 5.00%
    /// ```
    pub fn backpressure_percentage(&self) -> f64 {
        let total_ops = self.operations_completed + self.operations_failed;
        if total_ops == 0 {
            return 0.0;
        }
        (self.backpressure_events as f64 / total_ops as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_scenario_result_creation() {
        use crate::metrics::MetricsSnapshot;

        let start = Instant::now();
        let end = start + Duration::from_secs(60);

        let metrics_snapshot = MetricsSnapshot {
            operation_metrics: HashMap::new(),
            backpressure_events: 0,
            duration: Duration::from_secs(10),
            timestamp: std::time::SystemTime::now(),
        };

        let result = ScenarioResult {
            duration: Duration::from_secs(60),
            operations_completed: 1000,
            operations_failed: 5,
            metrics: metrics_snapshot,
            start_time: start,
            end_time: end,
            backpressure_events: 0,
        };

        assert_eq!(result.duration, Duration::from_secs(60));
        assert_eq!(result.operations_completed, 1000);
        assert_eq!(result.operations_failed, 5);
        assert_eq!(result.backpressure_events, 0);
    }

    #[test]
    fn test_scenario_result_success_rate() {
        use crate::metrics::{MetricsSnapshot, OperationMetricsSnapshot};
        use hdrhistogram::Histogram;

        let start = Instant::now();
        let end = start + Duration::from_secs(60);

        let mut operation_metrics = HashMap::new();
        operation_metrics.insert(
            "test_op".to_string(),
            OperationMetricsSnapshot {
                count: 1000,
                errors: 50,
                latency_histogram: Histogram::new(3).unwrap(),
            },
        );

        let metrics_snapshot = MetricsSnapshot {
            operation_metrics,
            backpressure_events: 0,
            duration: Duration::from_secs(10),
            timestamp: std::time::SystemTime::now(),
        };

        let result = ScenarioResult {
            duration: Duration::from_secs(60),
            operations_completed: 1000,
            operations_failed: 50,
            metrics: metrics_snapshot,
            start_time: start,
            end_time: end,
            backpressure_events: 0,
        };

        let op_metrics = result.metrics.operation_metrics.get("test_op").unwrap();
        let success_rate = op_metrics.success_rate();

        assert!((success_rate - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_scenario_result_helper_methods() {
        use crate::metrics::MetricsSnapshot;

        let start = Instant::now();
        let end = start + Duration::from_secs(60);

        let metrics_snapshot = MetricsSnapshot {
            operation_metrics: HashMap::new(),
            backpressure_events: 10,
            duration: Duration::from_secs(60),
            timestamp: std::time::SystemTime::now(),
        };

        let result = ScenarioResult {
            duration: Duration::from_secs(60),
            operations_completed: 1000,
            operations_failed: 50,
            metrics: metrics_snapshot,
            start_time: start,
            end_time: end,
            backpressure_events: 10,
        };

        // Test success_rate()
        let success_rate = result.success_rate();
        assert!((success_rate - 0.952380952).abs() < 0.0001); // 1000 / 1050

        // Test throughput()
        let throughput = result.throughput();
        assert!((throughput - 16.666666).abs() < 0.001); // 1000 / 60

        // Test had_backpressure()
        assert!(result.had_backpressure());

        // Test backpressure_percentage()
        let bp_pct = result.backpressure_percentage();
        assert!((bp_pct - 0.952380952).abs() < 0.001); // 10 / 1050 * 100
    }

    #[test]
    fn test_scenario_result_zero_operations() {
        use crate::metrics::MetricsSnapshot;

        let start = Instant::now();
        let end = start + Duration::from_secs(1);

        let metrics_snapshot = MetricsSnapshot {
            operation_metrics: HashMap::new(),
            backpressure_events: 0,
            duration: Duration::from_secs(1),
            timestamp: std::time::SystemTime::now(),
        };

        let result = ScenarioResult {
            duration: Duration::from_secs(1),
            operations_completed: 0,
            operations_failed: 0,
            metrics: metrics_snapshot,
            start_time: start,
            end_time: end,
            backpressure_events: 0,
        };

        // All calculations should handle zero operations gracefully
        assert_eq!(result.success_rate(), 0.0);
        assert_eq!(result.throughput(), 0.0);
        assert!(!result.had_backpressure());
        assert_eq!(result.backpressure_percentage(), 0.0);
    }

    #[test]
    fn test_scenario_result_no_backpressure() {
        use crate::metrics::MetricsSnapshot;

        let start = Instant::now();
        let end = start + Duration::from_secs(30);

        let metrics_snapshot = MetricsSnapshot {
            operation_metrics: HashMap::new(),
            backpressure_events: 0,
            duration: Duration::from_secs(30),
            timestamp: std::time::SystemTime::now(),
        };

        let result = ScenarioResult {
            duration: Duration::from_secs(30),
            operations_completed: 500,
            operations_failed: 0,
            metrics: metrics_snapshot,
            start_time: start,
            end_time: end,
            backpressure_events: 0,
        };

        assert!(!result.had_backpressure());
        assert_eq!(result.backpressure_percentage(), 0.0);
        assert_eq!(result.success_rate(), 1.0);
    }

    // Mock workload for testing
    struct MockWorkload;

    impl crate::workload::Workload for MockWorkload {
        fn prepare(&mut self, _ctx: &mut crate::workload::PrepareContext) -> Result<()> {
            Ok(())
        }

        fn next_operation(&mut self, _ctx: &crate::workload::ExecutionContext) -> Result<crate::workload::Operation> {
            Ok(crate::workload::Operation {
                name: "test".to_string(),
                sql: "SELECT 1".to_string(),
                params: vec![],
                operation_type: crate::workload::OperationType::Read,
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

    // Mock runtime for testing
    // Enhanced MockRuntime with configurable behavior
    struct MockRuntime {
        backpressure_enabled: bool,
        operation_delay: Duration,
    }

    impl MockRuntime {
        fn new() -> Self {
            Self {
                backpressure_enabled: false,
                operation_delay: Duration::from_millis(10),
            }
        }

        fn with_backpressure() -> Self {
            Self {
                backpressure_enabled: true,
                operation_delay: Duration::from_millis(10),
            }
        }

        fn with_delay(delay: Duration) -> Self {
            Self {
                backpressure_enabled: false,
                operation_delay: delay,
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::runtime::RuntimeEngine for MockRuntime {
        async fn submit(&self, _op: crate::workload::Operation) -> Result<crate::runtime::OperationResult> {
            // Simulate operation delay
            if self.operation_delay.as_millis() > 0 {
                tokio::time::sleep(self.operation_delay).await;
            }

            Ok(crate::runtime::OperationResult {
                success: true,
                duration: self.operation_delay,
                rows_affected: 1,
                error: None,
            })
        }

        fn stats(&self) -> crate::runtime::RuntimeStats {
            crate::runtime::RuntimeStats {
                active_connections: 0,
                queued_operations: 0,
                pool_utilization: 0.0,
                backpressure_active: self.backpressure_enabled,
            }
        }

        async fn shutdown(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_get_current_stage() {
        use crate::config::RateStage;
        use crate::metrics::MetricsCollector;

        // Create a minimal executor for testing
        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::RampingRate {
                stages: vec![],
                prealloc_connections: 50,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Test stages
        let stages = vec![
            RateStage {
                target_rate: 100,
                duration: Duration::from_secs(10),
            },
            RateStage {
                target_rate: 200,
                duration: Duration::from_secs(10),
            },
            RateStage {
                target_rate: 300,
                duration: Duration::from_secs(10),
            },
        ];

        let mut current_index = 0;

        // Test stage 0 (0-10 seconds)
        let stage = executor.get_current_stage(&stages, Duration::from_secs(5), &mut current_index);
        assert!(stage.is_some());
        assert_eq!(stage.unwrap().target_rate, 100);
        assert_eq!(current_index, 0);

        // Test stage 1 (10-20 seconds)
        let stage = executor.get_current_stage(&stages, Duration::from_secs(15), &mut current_index);
        assert!(stage.is_some());
        assert_eq!(stage.unwrap().target_rate, 200);
        assert_eq!(current_index, 1);

        // Test stage 2 (20-30 seconds)
        let stage = executor.get_current_stage(&stages, Duration::from_secs(25), &mut current_index);
        assert!(stage.is_some());
        assert_eq!(stage.unwrap().target_rate, 300);
        assert_eq!(current_index, 2);

        // Test after all stages (30+ seconds)
        let stage = executor.get_current_stage(&stages, Duration::from_secs(35), &mut current_index);
        assert!(stage.is_none());
    }

    #[test]
    fn test_get_current_stage_edge_cases() {
        use crate::config::RateStage;
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::RampingRate {
                stages: vec![],
                prealloc_connections: 50,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Empty stages
        let stages: Vec<RateStage> = vec![];
        let mut current_index = 0;
        let stage = executor.get_current_stage(&stages, Duration::from_secs(0), &mut current_index);
        assert!(stage.is_none());

        // Single stage at exact boundary
        let stages = vec![RateStage {
            target_rate: 100,
            duration: Duration::from_secs(10),
        }];
        let stage = executor.get_current_stage(&stages, Duration::from_secs(10), &mut current_index);
        assert!(stage.is_none()); // Exactly at boundary = complete

        // At time zero
        let stage = executor.get_current_stage(&stages, Duration::from_secs(0), &mut current_index);
        assert!(stage.is_some());
        assert_eq!(stage.unwrap().target_rate, 100);
    }

    #[tokio::test]
    async fn test_handle_events_pause_resume() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 100,
                duration: Duration::from_secs(10),
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Create event channel
        let (tx, rx) = mpsc::channel(10);
        executor.attach_event_stream(rx);

        // Initially not paused
        assert!(!executor.paused);
        assert!(!executor.shutdown_requested);

        // Send pause event
        tx.send(Event::PhaseTransition(Phase::Pause))
            .await
            .unwrap();
        executor.handle_events().await.unwrap();
        assert!(executor.paused);

        // Send resume event
        tx.send(Event::PhaseTransition(Phase::Resume))
            .await
            .unwrap();
        executor.handle_events().await.unwrap();
        assert!(!executor.paused);

        // Send shutdown event
        tx.send(Event::PhaseTransition(Phase::Shutdown))
            .await
            .unwrap();
        executor.handle_events().await.unwrap();
        assert!(executor.shutdown_requested);
    }

    #[tokio::test]
    async fn test_handle_events_rate_change() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 100,
                duration: Duration::from_secs(10),
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Create event channel
        let (tx, rx) = mpsc::channel(10);
        executor.attach_event_stream(rx);

        // Send rate change event
        tx.send(Event::RateChange(500)).await.unwrap();
        executor.handle_events().await.unwrap();

        // Note: We can't easily verify the rate limiter state without exposing it,
        // but we can at least verify the event was processed without errors
    }

    #[tokio::test]
    async fn test_handle_events_metrics_snapshot() {
        use crate::metrics::MetricsCollector;
        use crate::driver::QueryResult;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 100,
                duration: Duration::from_secs(10),
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        // Record some operations to have data in the snapshot
        metrics.record_operation(
            "test_op",
            Duration::from_millis(10),
            &Ok(QueryResult {
                rows_affected: 1,
                last_insert_id: None,
            }),
        );
        metrics.record_backpressure_event();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Create event channel
        let (tx, rx) = mpsc::channel(10);
        executor.attach_event_stream(rx);

        // Send metrics snapshot event
        tx.send(Event::MetricsSnapshot).await.unwrap();
        executor.handle_events().await.unwrap();

        // Event should be processed without errors
        // The snapshot is logged (verified by tracing in handle_events)
    }

    #[tokio::test]
    async fn test_handle_events_custom_event() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 100,
                duration: Duration::from_secs(10),
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Create event channel
        let (tx, rx) = mpsc::channel(10);
        executor.attach_event_stream(rx);

        // Send custom event
        let custom_data = serde_json::json!({"action": "test", "value": 42});
        tx.send(Event::Custom(custom_data)).await.unwrap();
        executor.handle_events().await.unwrap();

        // Event should be processed without errors (logged at debug level)
    }

    #[tokio::test]
    async fn test_closed_loop_basic() {
        use crate::metrics::MetricsCollector;

        // Create a minimal inline workload definition
        let workload_def = serde_yaml::from_str(r#"
name: test_workload
schema:
  tables:
    - name: test_table
      count: 1
      columns:
        - name: id
          type: integer
          primary_key: true
operations:
  - name: select
    sql: "SELECT * FROM test_table WHERE id = ?"
    params:
      - type: integer
        distribution: uniform
        min: 1
        max: 100
    weight: 100
"#).unwrap();

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ClosedLoop {
                workers: 2,
                duration: Duration::from_millis(100), // Short duration for test
                think_time: None,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: Some(workload_def),
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Execute closed-loop
        let result = executor.execute_closed_loop(2, Duration::from_millis(100)).await;

        // Should complete without errors (even if no operations due to workload creation issues)
        assert!(result.is_ok());
        let result = result.unwrap();

        // Duration should be approximately 100ms
        assert!(result.duration.as_millis() >= 90 && result.duration.as_millis() <= 200);
    }

    #[tokio::test]
    async fn test_closed_loop_with_think_time() {
        use crate::metrics::MetricsCollector;

        // Create a minimal inline workload definition
        let workload_def = serde_yaml::from_str(r#"
name: test_workload
schema:
  tables:
    - name: test_table
      count: 1
      columns:
        - name: id
          type: integer
          primary_key: true
operations:
  - name: select
    sql: "SELECT * FROM test_table WHERE id = ?"
    params:
      - type: integer
        distribution: uniform
        min: 1
        max: 100
    weight: 100
"#).unwrap();

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ClosedLoop {
                workers: 1,
                duration: Duration::from_millis(100),
                think_time: Some(Duration::from_millis(10)), // 10ms think time
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: Some(workload_def),
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Execute closed-loop with think time
        let result = executor.execute_closed_loop(1, Duration::from_millis(100)).await;

        // Should complete without errors
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_constant_rate_dispatcher() {
        // Test that execute() correctly dispatches to execute_constant_rate()
        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 100,
                duration: Duration::from_millis(50),
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Execute via main entry point
        let result = executor.execute().await;

        assert!(result.is_ok());
        let result = result.unwrap();

        // Should have completed some operations
        assert!(result.duration.as_millis() >= 40);
    }

    #[tokio::test]
    async fn test_execute_ramping_rate_dispatcher() {
        // Test that execute() correctly dispatches to execute_ramping_rate()
        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::RampingRate {
                stages: vec![
                    RateStage {
                        target_rate: 50,
                        duration: Duration::from_millis(30),
                    },
                    RateStage {
                        target_rate: 100,
                        duration: Duration::from_millis(30),
                    },
                ],
                prealloc_connections: 5,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Execute via main entry point
        let result = executor.execute().await;

        assert!(result.is_ok());
        let result = result.unwrap();

        // Should have run for approximately 60ms total
        assert!(result.duration.as_millis() >= 50);
    }

    #[tokio::test]
    async fn test_execute_closed_loop_dispatcher() {
        // Test that execute() correctly dispatches to execute_closed_loop()
        let workload_def = serde_yaml::from_str(
            r#"
name: test_workload
schema:
  tables:
    - name: test_table
      count: 1
      columns:
        - name: id
          type: integer
          primary_key: true
operations:
  - name: select
    sql: "SELECT * FROM test_table WHERE id = ?"
    params:
      - type: integer
        distribution: uniform
        min: 1
        max: 100
    weight: 100
"#,
        )
        .unwrap();

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ClosedLoop {
                workers: 2,
                duration: Duration::from_millis(50),
                think_time: None,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: Some(workload_def),
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Execute via main entry point
        let result = executor.execute().await;

        assert!(result.is_ok());
        let result = result.unwrap();

        // Should have completed in approximately the specified duration
        assert!(result.duration.as_millis() >= 40);
    }

    // ===== Error Propagation Tests =====

    #[tokio::test]
    async fn test_error_propagation_from_workload() {
        use crate::metrics::MetricsCollector;

        // Create a workload that fails
        struct FailingWorkload;
        impl crate::workload::Workload for FailingWorkload {
            fn prepare(&mut self, _ctx: &mut crate::workload::PrepareContext) -> Result<()> {
                Ok(())
            }

            fn next_operation(
                &mut self,
                _ctx: &crate::workload::ExecutionContext,
            ) -> Result<crate::workload::Operation> {
                Err(crate::Error::Workload("Simulated workload error".to_string()))
            }

            fn cleanup(&mut self) -> Result<()> {
                Ok(())
            }

            fn name(&self) -> &str {
                "failing"
            }
        }

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 100,
                duration: Duration::from_millis(50),
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(FailingWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Execute - workload errors propagate to the caller
        let result = executor.execute().await;

        // Error should propagate up from workload
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Simulated workload error"));
    }

    #[tokio::test]
    async fn test_error_propagation_from_runtime() {
        use crate::metrics::MetricsCollector;

        // Create a runtime that fails
        struct FailingRuntime;

        #[async_trait::async_trait]
        impl crate::runtime::RuntimeEngine for FailingRuntime {
            async fn submit(
                &self,
                _op: crate::workload::Operation,
            ) -> Result<crate::runtime::OperationResult> {
                Err(crate::Error::Database(crate::DatabaseError::Query(
                    "Simulated runtime error".to_string(),
                )))
            }

            fn stats(&self) -> crate::runtime::RuntimeStats {
                crate::runtime::RuntimeStats {
                    active_connections: 0,
                    queued_operations: 0,
                    pool_utilization: 0.0,
                    backpressure_active: false,
                }
            }

            async fn shutdown(&mut self) -> Result<()> {
                Ok(())
            }
        }

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 100,
                duration: Duration::from_millis(50),
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(FailingRuntime);
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Execute - runtime errors are logged but don't stop execution
        let result = executor.execute().await;

        // Should complete even with runtime errors (fire-and-forget in open-loop)
        assert!(result.is_ok());
    }

    // ===== Boundary Condition Tests =====

    #[tokio::test]
    async fn test_zero_duration_constant_rate() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 100,
                duration: Duration::from_millis(0), // Zero duration
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        let result = executor.execute().await;
        assert!(result.is_ok());

        let result = result.unwrap();
        // Should complete immediately with zero or very few operations
        assert!(result.operations_completed <= 1);
    }

    #[tokio::test]
    async fn test_very_short_duration() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 1000,
                duration: Duration::from_millis(1), // 1ms duration
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        let result = executor.execute().await;
        assert!(result.is_ok());

        // Should handle very short durations gracefully
        let result = result.unwrap();
        // Account for 100ms sleep at end of executor
        assert!(result.duration.as_millis() <= 150);
    }

    #[tokio::test]
    async fn test_very_high_rate() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ConstantRate {
                rate: 1_000_000, // 1M ops/sec (very high)
                duration: Duration::from_millis(10),
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        let result = executor.execute().await;
        assert!(result.is_ok());

        // Should handle high rates without panicking
        // (actual rate limited by system capabilities)
    }

    #[test]
    fn test_empty_ramping_stages() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::RampingRate {
                stages: vec![], // Empty stages
                prealloc_connections: 5,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        // Constructor should handle empty stages gracefully
        let executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Should succeed (uses default rate of 100)
        assert_eq!(executor.rate_limiter.current_rate(), 100);
    }

    #[tokio::test]
    async fn test_single_stage_ramping() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::RampingRate {
                stages: vec![RateStage {
                    target_rate: 200,
                    duration: Duration::from_millis(50),
                }],
                prealloc_connections: 5,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        let result = executor.execute().await;
        assert!(result.is_ok());

        // Should handle single stage correctly
        let result = result.unwrap();
        assert!(result.duration.as_millis() >= 40);
    }

    #[tokio::test]
    async fn test_closed_loop_zero_workers() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ClosedLoop {
                workers: 0, // Zero workers
                duration: Duration::from_millis(50),
                think_time: None,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        let result = executor.execute().await;
        assert!(result.is_ok());

        let result = result.unwrap();
        // Should complete with zero operations
        assert_eq!(result.operations_completed, 0);
    }

    #[tokio::test]
    async fn test_closed_loop_single_worker() {
        use crate::metrics::MetricsCollector;

        let scenario_config = ScenarioConfig {
            executor: ExecutorConfig::ClosedLoop {
                workers: 1, // Single worker
                duration: Duration::from_millis(50),
                think_time: None,
                max_connections: 10,
            },
            workload: crate::config::WorkloadConfig::Declarative {
                file: None,
                definition: None,
                overrides: None,
            },
        };

        let workload = Box::new(MockWorkload);
        let runtime = Arc::new(MockRuntime::new());
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        let result = executor.execute().await;
        assert!(result.is_ok());

        // Should work with single worker
        let result = result.unwrap();
        assert!(result.duration.as_millis() >= 40);
    }
}
