//! Scenario executor module
//!
//! Orchestrates workload execution with rate control.

use crate::config::{ExecutorConfig, RateStage, ScenarioConfig};
use crate::metrics::{MetricsCollector, MetricsSnapshot};
use crate::rate_limiter::RateLimiter;
use crate::runtime::RuntimeEngine;
use crate::workload::{ExecutionContext, Workload};
use crate::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Event types for external event integration (M1)
#[derive(Debug, Clone)]
pub enum Event {
    /// Change rate dynamically
    RateChange(u64),
    /// Phase transition (pause, resume, shutdown)
    PhaseTransition(Phase),
    /// Take intermediate metrics snapshot
    MetricsSnapshot,
    /// Custom user-defined event
    Custom(serde_json::Value),
}

/// Phase control for event-driven execution
#[derive(Debug, Clone)]
pub enum Phase {
    /// Stop submitting operations
    Pause,
    /// Continue submitting operations
    Resume,
    /// Graceful shutdown
    Shutdown,
}

/// Scenario executor - orchestrates workload execution
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
    ) -> Self {
        let rate_limiter = match &config.executor {
            ExecutorConfig::ConstantRate { rate, .. } => RateLimiter::new(*rate),
            ExecutorConfig::RampingRate { stages, .. } => RateLimiter::new(stages[0].target_rate),
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

    /// Attach external event stream for runtime control (M1)
    pub fn attach_event_stream(&mut self, rx: mpsc::Receiver<Event>) {
        self.event_rx = Some(rx);
    }

    /// Execute scenario
    pub async fn execute(&mut self) -> Result<ScenarioResult> {
        // TODO: Prepare workload
        // let prepare_ctx = PrepareContext { ... };
        // self.workload.prepare(&prepare_ctx)?;

        // Execute based on executor type
        let executor = self.config.executor.clone();
        match executor {
            ExecutorConfig::ConstantRate {
                rate, duration, ..
            } => self.execute_constant_rate(rate, duration).await,
            ExecutorConfig::RampingRate { stages, .. } => self.execute_ramping_rate(&stages).await,
            ExecutorConfig::ClosedLoop {
                workers, duration, ..
            } => self.execute_closed_loop(workers, duration).await,
        }
    }

    async fn execute_constant_rate(&mut self, _rate: u64, duration: Duration) -> Result<ScenarioResult> {
        let start = Instant::now();
        let end_time = start + duration;
        let mut iteration = 0u64;

        while Instant::now() < end_time {
            // Rate limiting (time-driven)
            self.rate_limiter.acquire().await?;

            // Generate operation
            let ctx = ExecutionContext {
                worker_id: 0,
                iteration,
                elapsed: start.elapsed(),
            };

            let op = self.workload.next_operation(&ctx)?;

            // Submit to runtime (non-blocking)
            let runtime = self.runtime.clone();
            tokio::spawn(async move {
                let _ = runtime.submit(op).await;
            });

            iteration += 1;
        }

        // Wait a bit for in-flight operations to complete
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Collect results
        let end_time = Instant::now();
        let metrics_snapshot = self.metrics.snapshot();
        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: iteration,
            operations_failed: 0, // M0: simplified
            metrics: metrics_snapshot.clone(),
            start_time: start,
            end_time,
            backpressure_events: metrics_snapshot.backpressure_events,
        })
    }

    async fn execute_ramping_rate(&mut self, stages: &[RateStage]) -> Result<ScenarioResult> {
        let start = Instant::now();
        let mut iteration = 0u64;

        for stage in stages {
            self.rate_limiter.set_rate(stage.target_rate);
            let stage_end = Instant::now() + stage.duration;

            while Instant::now() < stage_end {
                self.rate_limiter.acquire().await?;

                let ctx = ExecutionContext {
                    worker_id: 0,
                    iteration,
                    elapsed: start.elapsed(),
                };

                let op = self.workload.next_operation(&ctx)?;

                let runtime = self.runtime.clone();
                tokio::spawn(async move {
                    let _ = runtime.submit(op).await;
                });

                iteration += 1;
            }
        }

        // Wait for in-flight operations
        tokio::time::sleep(Duration::from_secs(1)).await;

        let end_time = Instant::now();
        let metrics_snapshot = self.metrics.snapshot();
        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: iteration,
            operations_failed: 0,
            metrics: metrics_snapshot.clone(),
            start_time: start,
            end_time,
            backpressure_events: metrics_snapshot.backpressure_events,
        })
    }

    async fn execute_closed_loop(&mut self, _workers: usize, _duration: Duration) -> Result<ScenarioResult> {
        // TODO: Implement in Phase 3 - Closed-Loop Executor
        // This will spawn N worker tasks that continuously execute operations
        // until duration expires or shutdown is requested
        todo!("Closed-loop executor implementation (Phase 3)")
    }
}

/// Scenario execution result
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Total duration of the test
    pub duration: Duration,

    /// Number of operations completed successfully
    pub operations_completed: u64,

    /// Number of operations that failed
    pub operations_failed: u64,

    /// Metrics snapshot at end of test
    pub metrics: MetricsSnapshot,

    /// Start time of the test
    pub start_time: Instant,

    /// End time of the test
    pub end_time: Instant,

    /// Number of backpressure events detected
    pub backpressure_events: u64,
}

impl ScenarioResult {
    /// Calculate success rate (0.0 to 1.0)
    pub fn success_rate(&self) -> f64 {
        let total = self.operations_completed + self.operations_failed;
        if total == 0 {
            return 0.0;
        }
        self.operations_completed as f64 / total as f64
    }

    /// Calculate throughput (operations per second)
    pub fn throughput(&self) -> f64 {
        let duration_secs = self.duration.as_secs_f64();
        if duration_secs == 0.0 {
            return 0.0;
        }
        self.operations_completed as f64 / duration_secs
    }

    /// Check if backpressure was detected during test
    pub fn had_backpressure(&self) -> bool {
        self.backpressure_events > 0
    }

    /// Calculate backpressure percentage
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
}
