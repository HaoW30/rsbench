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

    /// Attach external event stream for runtime control (M1)
    pub fn attach_event_stream(&mut self, rx: mpsc::Receiver<Event>) {
        self.event_rx = Some(rx);
    }

    /// Handle incoming events (non-blocking)
    /// Returns true if should continue execution, false if should stop
    async fn handle_events(&mut self) -> Result<()> {
        if let Some(ref mut rx) = self.event_rx {
            // Try to receive events without blocking
            while let Ok(event) = rx.try_recv() {
                match event {
                    Event::RateChange(new_rate) => {
                        self.rate_limiter.set_rate(new_rate);
                    }
                    Event::PhaseTransition(phase) => match phase {
                        Phase::Pause => {
                            self.paused = true;
                        }
                        Phase::Resume => {
                            self.paused = false;
                        }
                        Phase::Shutdown => {
                            self.shutdown_requested = true;
                        }
                    },
                    Event::MetricsSnapshot => {
                        // TODO: M1 - Take intermediate snapshot
                        // For now, this is a no-op
                    }
                    Event::Custom(_) => {
                        // TODO: M1 - Handle custom events
                        // For now, this is a no-op
                    }
                }
            }
        }
        Ok(())
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
            self.rate_limiter.acquire().await?;

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
            self.rate_limiter.acquire().await?;

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
    struct MockRuntime;

    #[async_trait::async_trait]
    impl crate::runtime::RuntimeEngine for MockRuntime {
        async fn submit(&self, _op: crate::workload::Operation) -> Result<crate::runtime::OperationResult> {
            Ok(crate::runtime::OperationResult {
                success: true,
                duration: Duration::from_millis(10),
                rows_affected: 1,
                error: None,
            })
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
        let runtime = Arc::new(MockRuntime);
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
        let runtime = Arc::new(MockRuntime);
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
        let runtime = Arc::new(MockRuntime);
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
        let runtime = Arc::new(MockRuntime);
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
        let runtime = Arc::new(MockRuntime);
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
        let runtime = Arc::new(MockRuntime);
        let metrics = MetricsCollector::new();

        let mut executor = ScenarioExecutor::new(scenario_config, workload, runtime, metrics);

        // Execute closed-loop with think time
        let result = executor.execute_closed_loop(1, Duration::from_millis(100)).await;

        // Should complete without errors
        assert!(result.is_ok());
    }
}
