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

/// Scenario executor - orchestrates workload execution
pub struct ScenarioExecutor {
    config: ScenarioConfig,
    workload: Box<dyn Workload>,
    rate_limiter: RateLimiter,
    runtime: Arc<dyn RuntimeEngine>,
    metrics: Arc<MetricsCollector>,
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
        };

        Self {
            config,
            workload,
            rate_limiter,
            runtime,
            metrics,
        }
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
        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: iteration,
            operations_failed: 0, // M0: simplified
            metrics: self.metrics.snapshot(),
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

        Ok(ScenarioResult {
            duration: start.elapsed(),
            operations_completed: iteration,
            operations_failed: 0,
            metrics: self.metrics.snapshot(),
        })
    }
}

/// Scenario execution result
pub struct ScenarioResult {
    pub duration: Duration,
    pub operations_completed: u64,
    pub operations_failed: u64,
    pub metrics: MetricsSnapshot,
}
