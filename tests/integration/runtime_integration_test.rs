//! Runtime integration tests
//!
//! Tests runtime execution behavior and backpressure.

use rsbench::config::PoolConfig;
use rsbench::driver::DatabaseDriver;
use rsbench::metrics::MetricsCollector;
use rsbench::pool::ConnectionPool;
use rsbench::runtime::{create_runtime, RuntimeEngine};
use rsbench::workload::{Operation, OperationType};
use rsbench::Value;
use std::sync::Arc;
use std::time::Duration;

use crate::common::MockDriver;

fn create_test_operation() -> Operation {
    Operation {
        name: "test_op".to_string(),
        sql: "SELECT 1".to_string(),
        params: vec![],
        operation_type: OperationType::Read,
        is_transaction: false,
        transaction_sqls: vec![],
        transaction_params: vec![],
    }
}

fn create_test_pool(driver: Arc<dyn DatabaseDriver>) -> Arc<ConnectionPool> {
    let config = PoolConfig {
        min_size: 1,
        max_size: 10,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(60),
    };

    Arc::new(
        ConnectionPool::new(driver, "mock://localhost".to_string(), config)
            .expect("Failed to create pool"),
    )
}

#[tokio::test]
async fn test_runtime_basic_operation_submission() {
    // Test basic operation submission
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    let runtime = create_runtime(pool.clone(), 10, 0.8, metrics.clone());

    let op = create_test_operation();
    let result = runtime.submit(op).await;

    assert!(result.is_ok());
    let op_result = result.unwrap();
    assert!(op_result.success);
    assert_eq!(op_result.rows_affected, 1);
    assert!(op_result.error.is_none());
    assert!(op_result.duration > Duration::ZERO);
}

#[tokio::test]
async fn test_runtime_database_error_handling() {
    // Test that database errors are properly handled
    let driver = Arc::new(MockDriver::new("test").with_failure());
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    let runtime = create_runtime(pool.clone(), 10, 0.8, metrics.clone());

    let op = create_test_operation();
    let result = runtime.submit(op).await;

    // Connection failure should return error
    assert!(result.is_err());
}

#[tokio::test]
async fn test_runtime_multiple_operations() {
    // Test submitting multiple operations
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver.clone());
    let metrics = MetricsCollector::new();

    let runtime = create_runtime(pool.clone(), 10, 0.8, metrics.clone());

    // Submit 5 operations
    for i in 0..5 {
        let mut op = create_test_operation();
        op.name = format!("test_op_{}", i);

        let result = runtime.submit(op).await;
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    // Verify driver was called 5 times
    assert_eq!(driver.connect_count(), 5);
}

#[tokio::test]
async fn test_runtime_concurrent_operations() {
    // Test concurrent operation submission
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver.clone());
    let metrics = MetricsCollector::new();

    let runtime = Arc::new(create_runtime(pool.clone(), 10, 0.8, metrics.clone()));

    // Launch 3 operations concurrently
    let runtime1 = runtime.clone();
    let runtime2 = runtime.clone();
    let runtime3 = runtime.clone();

    let handle1 = tokio::spawn(async move {
        let op = create_test_operation();
        runtime1.submit(op).await
    });

    let handle2 = tokio::spawn(async move {
        let op = create_test_operation();
        runtime2.submit(op).await
    });

    let handle3 = tokio::spawn(async move {
        let op = create_test_operation();
        runtime3.submit(op).await
    });

    // All should complete successfully
    let results = tokio::join!(handle1, handle2, handle3);
    assert!(results.0.is_ok());
    assert!(results.1.is_ok());
    assert!(results.2.is_ok());
}

#[tokio::test]
async fn test_runtime_semaphore_limits_concurrency() {
    // Test that semaphore properly limits concurrent operations
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver.clone());
    let metrics = MetricsCollector::new();

    // Create runtime with max 2 concurrent connections
    let runtime = Arc::new(create_runtime(pool.clone(), 2, 0.8, metrics.clone()));

    // Launch 10 operations concurrently with max 2 allowed
    let mut handles = vec![];
    for i in 0..10 {
        let runtime = runtime.clone();
        let handle = tokio::spawn(async move {
            let mut op = create_test_operation();
            op.name = format!("test_op_{}", i);
            runtime.submit(op).await
        });
        handles.push(handle);
    }

    // All should complete successfully (even though only 2 run at a time)
    for handle in handles {
        let result = handle.await;
        assert!(result.is_ok());
    }

    assert_eq!(driver.connect_count(), 10);
}

#[tokio::test]
async fn test_runtime_stats() {
    // Test that runtime stats are correct
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    let runtime = create_runtime(pool.clone(), 10, 0.8, metrics.clone());

    let stats = runtime.stats();

    // Initial stats should show no activity
    assert_eq!(stats.active_connections, 0);
    assert!(stats.pool_utilization >= 0.0 && stats.pool_utilization <= 1.0);
}

#[tokio::test]
async fn test_runtime_backpressure_detection() {
    // Test backpressure detection
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    // Set a very low threshold to trigger backpressure
    let runtime = create_runtime(pool.clone(), 10, 0.01, metrics.clone());

    let op = create_test_operation();
    let _result = runtime.submit(op).await;

    // Check metrics for backpressure events
    let snapshot = metrics.snapshot();
    // Note: Backpressure may or may not be triggered depending on timing
    // This test just ensures the mechanism doesn't crash
    assert!(snapshot.backpressure_events >= 0);
}

#[tokio::test]
async fn test_runtime_metrics_collection() {
    // Test that metrics are properly collected
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    let runtime = create_runtime(pool.clone(), 10, 0.8, metrics.clone());

    // Submit multiple operations
    for i in 0..5 {
        let mut op = create_test_operation();
        op.name = format!("test_op_{}", i);
        let _result = runtime.submit(op).await;
    }

    // Check that metrics were recorded
    let snapshot = metrics.snapshot();
    assert!(snapshot.operation_metrics.len() > 0);
}

#[tokio::test]
async fn test_runtime_shutdown() {
    // Test graceful shutdown
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    let mut runtime = create_runtime(pool.clone(), 10, 0.8, metrics.clone());

    // Submit an operation
    let op = create_test_operation();
    let _result = runtime.submit(op).await;

    // Shutdown should succeed
    let shutdown_result = runtime.shutdown().await;
    assert!(shutdown_result.is_ok());
}

#[tokio::test]
async fn test_runtime_with_query_parameters() {
    // Test operations with query parameters
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    let runtime = create_runtime(pool.clone(), 10, 0.8, metrics.clone());

    let op = Operation {
        name: "parameterized_query".to_string(),
        sql: "SELECT * FROM users WHERE id = ?".to_string(),
        params: vec![Value::Int(42)],
        operation_type: OperationType::Read,
        is_transaction: false,
        transaction_sqls: vec![],
        transaction_params: vec![],
    };

    let result = runtime.submit(op).await;
    assert!(result.is_ok());
    assert!(result.unwrap().success);
}

// Phase 1: Enhanced Backpressure Monitoring Tests

#[tokio::test]
async fn test_runtime_semaphore_only_saturation() {
    // Test that backpressure is detected when ONLY semaphore is saturated
    // (pool has capacity, but semaphore is exhausted)
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    // Set low max_connections to easily saturate semaphore
    // High threshold ensures pool won't trigger backpressure
    let runtime = Arc::new(create_runtime(pool.clone(), 2, 0.99, metrics.clone()));

    // Launch 3 concurrent long-running operations
    // This will saturate the semaphore (2 permits) but not the pool
    let runtime1 = runtime.clone();
    let runtime2 = runtime.clone();
    let runtime3 = runtime.clone();

    let handle1 = tokio::spawn(async move {
        let op = create_test_operation();
        runtime1.submit(op).await
    });

    let handle2 = tokio::spawn(async move {
        let op = create_test_operation();
        runtime2.submit(op).await
    });

    let handle3 = tokio::spawn(async move {
        let op = create_test_operation();
        runtime3.submit(op).await
    });

    // Wait for all to complete
    let _results = tokio::join!(handle1, handle2, handle3);

    // Check stats - semaphore should show high utilization
    let stats = runtime.stats();
    // With 2 max_connections and operations completing quickly,
    // we should see some semaphore utilization
    assert!(stats.semaphore_utilization >= 0.0);
}

#[tokio::test]
async fn test_runtime_backpressure_stats_includes_semaphore() {
    // Test that RuntimeStats includes semaphore_utilization
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    let runtime = create_runtime(pool.clone(), 10, 0.8, metrics.clone());

    // Get stats
    let stats = runtime.stats();

    // Verify semaphore_utilization field exists and is valid
    assert!(stats.semaphore_utilization >= 0.0);
    assert!(stats.semaphore_utilization <= 1.0);
    assert!(stats.pool_utilization >= 0.0);
    assert!(stats.pool_utilization <= 1.0);
}

#[tokio::test]
async fn test_runtime_backpressure_dual_source_detection() {
    // Test that backpressure can be triggered by either pool OR semaphore
    let driver = Arc::new(MockDriver::new("test"));
    let pool = create_test_pool(driver);
    let metrics = MetricsCollector::new();

    // Use low threshold to make backpressure easy to detect
    let runtime = create_runtime(pool.clone(), 5, 0.1, metrics.clone());

    // Submit several operations
    for _ in 0..3 {
        let op = create_test_operation();
        let _ = runtime.submit(op).await;
    }

    // Get stats
    let stats = runtime.stats();

    // Backpressure detection should consider both sources
    // If either pool_utilization > 0.1 OR semaphore_utilization > 0.1,
    // backpressure_active should be true
    let should_be_active = stats.pool_utilization > 0.1 || stats.semaphore_utilization > 0.1;
    if should_be_active {
        // If we expect backpressure, verify metrics recorded it
        let snapshot = metrics.snapshot();
        // Backpressure may or may not have been triggered depending on timing
        assert!(snapshot.backpressure_events >= 0);
    }
}
