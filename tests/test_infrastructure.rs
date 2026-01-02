//! Verification test for test infrastructure
//!
//! This test ensures that all test utilities in tests/common work correctly

mod common;

use common::*;
use rsbench::config::*;
use rsbench::driver::*;
use rsbench::workload::*;
use std::time::Duration;

#[tokio::test]
async fn test_mock_driver_works() {
    let driver = MockDriver::new("test");
    let conn_result = driver
        .connect(&ConnectionConfig {
            connection_string: "mock://test".to_string(),
            timeout: Duration::from_secs(5),
        })
        .await;

    assert!(conn_result.is_ok());
    assert_eq!(driver.connect_count(), 1);
}

#[tokio::test]
async fn test_mock_workload_works() {
    let mut workload = MockWorkload::new("test");
    let ctx = ExecutionContext {
        worker_id: 0,
        iteration: 0,
        elapsed: Duration::from_secs(0),
    };

    let op = workload.next_operation(&ctx).unwrap();
    assert_eq!(op.name, "default_op");
}

#[test]
fn test_config_builder_works() {
    let config = TestConfigBuilder::new()
        .with_driver("mysql")
        .with_constant_rate(1000, Duration::from_secs(60))
        .build();

    assert_eq!(config.database.driver, "mysql");
}

#[test]
fn test_assertions_work() {
    use rsbench::metrics::{MetricsSnapshot, OperationMetricsSnapshot};
    use std::collections::HashMap;

    let mut operation_metrics = HashMap::new();
    let mut histogram = hdrhistogram::Histogram::new(3).unwrap();
    histogram.record(1000).unwrap();

    operation_metrics.insert(
        "test_op".to_string(),
        OperationMetricsSnapshot {
            count: 100,
            errors: 5,
            latency_histogram: histogram,
        },
    );

    let snapshot = MetricsSnapshot {
        operation_metrics,
        backpressure_events: 0,
        pool_saturation_events: 0,
        runtime_saturation_events: 0,
        duration: Duration::from_secs(10),
        timestamp: std::time::SystemTime::now(),
    };

    // These should all pass
    assert_metric_count_in_range(&snapshot, "test_op", 90, 110);
    assert_error_rate_below(&snapshot, "test_op", 0.10);
    assert_success_rate_above(&snapshot, "test_op", 0.90);
    assert_backpressure_events(&snapshot, false);
}
