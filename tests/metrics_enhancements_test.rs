// Test for M0 Phase 2 metrics enhancements

use rsbench::driver::QueryResult;
use rsbench::metrics::{MetricsCollector};
use rsbench::Result;
use std::time::Duration;

#[test]
fn test_error_rate_percentage_in_metrics() {
    let collector = MetricsCollector::new();

    // Record 100 operations with 5 errors
    let success = Ok(QueryResult {
        rows_affected: 1,
        last_insert_id: None,
    });
    let error: Result<QueryResult> = Err(rsbench::Error::Database(
        rsbench::DatabaseError::Query("test error".to_string())
    ));

    for _ in 0..95 {
        collector.record_operation("test_op", Duration::from_millis(10), &success);
    }
    for _ in 0..5 {
        collector.record_operation("test_op", Duration::from_millis(10), &error);
    }

    let snapshot = collector.snapshot();
    let metrics = snapshot.operation_metrics.get("test_op").unwrap();

    // Verify error rate calculation
    assert_eq!(metrics.count, 100);
    assert_eq!(metrics.errors, 5);
    assert!((metrics.error_rate() - 0.05).abs() < f64::EPSILON);
    assert!((metrics.success_rate() - 0.95).abs() < f64::EPSILON);
}

#[test]
fn test_client_metrics_tracking() {
    let collector = MetricsCollector::new();

    // Record various client saturation events
    collector.record_backpressure_event();
    collector.record_backpressure_event();
    collector.record_backpressure_event();

    collector.record_pool_saturation();
    collector.record_pool_saturation();

    collector.record_runtime_saturation();

    let snapshot = collector.snapshot();

    // Verify client metrics are tracked
    assert_eq!(snapshot.backpressure_events, 3);
    assert_eq!(snapshot.pool_saturation_events, 2);
    assert_eq!(snapshot.runtime_saturation_events, 1);
}

#[test]
fn test_enhanced_latency_metrics_in_snapshot() {
    let collector = MetricsCollector::new();
    let result = Ok(QueryResult {
        rows_affected: 1,
        last_insert_id: None,
    });

    // Record operations with varying latencies
    collector.record_operation("test_op", Duration::from_micros(1000), &result);  // 1ms
    collector.record_operation("test_op", Duration::from_micros(2000), &result);  // 2ms
    collector.record_operation("test_op", Duration::from_micros(5000), &result);  // 5ms
    collector.record_operation("test_op", Duration::from_micros(10000), &result); // 10ms

    let snapshot = collector.snapshot();
    let metrics = snapshot.operation_metrics.get("test_op").unwrap();

    // Verify histogram has min, max, mean
    assert!(metrics.latency_histogram.min() > 0);
    assert!(metrics.latency_histogram.max() > 0);
    assert!(metrics.latency_histogram.mean() > 0.0);

    // Verify min <= mean <= max
    assert!(metrics.latency_histogram.min() as f64 <= metrics.latency_histogram.mean());
    assert!(metrics.latency_histogram.mean() <= metrics.latency_histogram.max() as f64);
}

#[test]
fn test_error_rate_with_zero_operations() {
    let collector = MetricsCollector::new();
    let snapshot = collector.snapshot();

    // No operations recorded, snapshot should be empty
    assert_eq!(snapshot.operation_metrics.len(), 0);
    assert_eq!(snapshot.backpressure_events, 0);
    assert_eq!(snapshot.pool_saturation_events, 0);
    assert_eq!(snapshot.runtime_saturation_events, 0);
}

#[test]
fn test_error_rate_with_all_errors() {
    let collector = MetricsCollector::new();
    let error: Result<QueryResult> = Err(rsbench::Error::Database(
        rsbench::DatabaseError::Query("test error".to_string())
    ));

    for _ in 0..10 {
        collector.record_operation("test_op", Duration::from_millis(5), &error);
    }

    let snapshot = collector.snapshot();
    let metrics = snapshot.operation_metrics.get("test_op").unwrap();

    // 100% error rate
    assert_eq!(metrics.count, 10);
    assert_eq!(metrics.errors, 10);
    assert!((metrics.error_rate() - 1.0).abs() < f64::EPSILON);
    assert!((metrics.success_rate() - 0.0).abs() < f64::EPSILON);
}

#[test]
fn test_error_rate_with_no_errors() {
    let collector = MetricsCollector::new();
    let success = Ok(QueryResult {
        rows_affected: 1,
        last_insert_id: None,
    });

    for _ in 0..10 {
        collector.record_operation("test_op", Duration::from_millis(5), &success);
    }

    let snapshot = collector.snapshot();
    let metrics = snapshot.operation_metrics.get("test_op").unwrap();

    // 0% error rate
    assert_eq!(metrics.count, 10);
    assert_eq!(metrics.errors, 0);
    assert!((metrics.error_rate() - 0.0).abs() < f64::EPSILON);
    assert!((metrics.success_rate() - 1.0).abs() < f64::EPSILON);
}

#[test]
fn test_multiple_operations_separate_tracking() {
    let collector = MetricsCollector::new();
    let success = Ok(QueryResult {
        rows_affected: 1,
        last_insert_id: None,
    });
    let error: Result<QueryResult> = Err(rsbench::Error::Database(
        rsbench::DatabaseError::Query("test error".to_string())
    ));

    // Record different error rates for different operations
    for _ in 0..90 {
        collector.record_operation("point_select", Duration::from_millis(5), &success);
    }
    for _ in 0..10 {
        collector.record_operation("point_select", Duration::from_millis(5), &error);
    }

    for _ in 0..50 {
        collector.record_operation("update_index", Duration::from_millis(10), &success);
    }
    for _ in 0..50 {
        collector.record_operation("update_index", Duration::from_millis(10), &error);
    }

    let snapshot = collector.snapshot();

    // Verify point_select: 10% error rate
    let ps_metrics = snapshot.operation_metrics.get("point_select").unwrap();
    assert_eq!(ps_metrics.count, 100);
    assert_eq!(ps_metrics.errors, 10);
    assert!((ps_metrics.error_rate() - 0.1).abs() < f64::EPSILON);

    // Verify update_index: 50% error rate
    let ui_metrics = snapshot.operation_metrics.get("update_index").unwrap();
    assert_eq!(ui_metrics.count, 100);
    assert_eq!(ui_metrics.errors, 50);
    assert!((ui_metrics.error_rate() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn test_client_metrics_independence() {
    let collector = MetricsCollector::new();

    // Record only backpressure
    collector.record_backpressure_event();
    collector.record_backpressure_event();

    let snapshot1 = collector.snapshot();
    assert_eq!(snapshot1.backpressure_events, 2);
    assert_eq!(snapshot1.pool_saturation_events, 0);
    assert_eq!(snapshot1.runtime_saturation_events, 0);

    // Add pool saturation
    collector.record_pool_saturation();

    let snapshot2 = collector.snapshot();
    assert_eq!(snapshot2.backpressure_events, 2);
    assert_eq!(snapshot2.pool_saturation_events, 1);
    assert_eq!(snapshot2.runtime_saturation_events, 0);

    // Add runtime saturation
    collector.record_runtime_saturation();
    collector.record_runtime_saturation();

    let snapshot3 = collector.snapshot();
    assert_eq!(snapshot3.backpressure_events, 2);
    assert_eq!(snapshot3.pool_saturation_events, 1);
    assert_eq!(snapshot3.runtime_saturation_events, 2);
}
