//! CLI implementation logic

use rsbench::config::{OutputFormat, ToolConfig};
use rsbench::metrics::{JsonOutput, MetricsOutput, TextOutput};
use rsbench::pool::ConnectionPool;
use rsbench::workload::PrepareContext;
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

    // 2.1 Pre-warm the pool to avoid concurrent connection creation burst
    pool.warm_up().await?;

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

pub async fn prepare_workload(config: ToolConfig, workload_path: &str) -> Result<()> {
    println!("Preparing workload from: {}", workload_path);

    // 1. Create driver registry and get driver
    let registry = DriverRegistry::new();
    let driver = registry.get(&config.database.driver)?;

    // 2. Create a single connection for prepare operations
    println!("Connecting to database: {}", config.database.connection_string);
    let conn_config = rsbench::driver::ConnectionConfig {
        connection_string: config.database.connection_string.clone(),
        timeout: config.database.pool.connection_timeout,
    };
    let mut conn = driver.connect(&conn_config).await?;

    // 3. Create workload from file
    println!("Loading workload definition...");
    let workload_config = rsbench::config::WorkloadConfig::Declarative {
        file: Some(std::path::PathBuf::from(workload_path)),
        definition: None,
        overrides: None,
    };
    let mut workload = WorkloadFactory::create(&workload_config, config.determinism.seed)?;

    // 4. Create PrepareContext with the connection
    let mut ctx = PrepareContext {
        connection: conn.as_mut(),
        seed: config.determinism.seed,
        worker_count: 1, // Not used during prepare
    };

    // 5. Call workload.prepare() to create tables and load data
    println!("Creating tables and loading data...");
    workload.prepare(&mut ctx).await?;

    println!("✓ Workload preparation completed successfully!");

    Ok(())
}
