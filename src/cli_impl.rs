//! CLI implementation logic

use rsbench::config::{OutputFormat, ToolConfig};
use rsbench::metrics::{JsonOutput, MetricsOutput, TextOutput};
use rsbench::pool::ConnectionPool;
use rsbench::{DriverRegistry, MetricsCollector, Result, create_runtime, ScenarioExecutor, WorkloadFactory};
use std::sync::Arc;

pub async fn run_scenario(config: ToolConfig) -> Result<()> {
    // 1. Create driver registry
    let registry = DriverRegistry::new();
    let driver = registry.get(&config.database.driver)?;

    // 2. Create connection pool
    let pool = Arc::new(ConnectionPool::new(
        driver,
        config.database.connection_string.clone(),
        config.database.pool.clone(),
    )?);

    // 3. Create metrics collector
    let metrics = MetricsCollector::new();

    // 4. Create runtime (async-only)
    let runtime = create_runtime(
        pool.clone(),
        config.runtime.max_connections,
        config.runtime.backpressure_threshold,
        metrics.clone(),
    );
    let runtime = Arc::from(runtime);

    // 5. Create workload
    let workload = WorkloadFactory::create(&config.scenario.workload, config.determinism.seed)?;

    // 6. Create and execute scenario
    let mut executor = ScenarioExecutor::new(
        config.scenario.clone(),
        workload,
        runtime,
        metrics.clone(),
    );

    println!("Starting scenario execution...");
    let result = executor.execute().await?;
    println!("Scenario completed!");

    // 7. Output results
    let snapshot = result.metrics;
    let mut output: Box<dyn MetricsOutput> = match config.output.format {
        OutputFormat::Text => Box::new(TextOutput::new(Box::new(std::io::stdout()))),
        OutputFormat::Json => Box::new(JsonOutput::new(Box::new(std::io::stdout()))),
    };

    output.export(&snapshot)?;

    Ok(())
}
