//! Custom assertions for testing metrics and results

use rsbench::metrics::MetricsSnapshot;
use std::time::Duration;

/// Assert that metric count is within expected range
pub fn assert_metric_count_in_range(
    snapshot: &MetricsSnapshot,
    operation_name: &str,
    min: u64,
    max: u64,
) {
    let metrics = snapshot
        .operation_metrics
        .get(operation_name)
        .expect(&format!("Operation '{}' not found in metrics", operation_name));

    assert!(
        metrics.count >= min && metrics.count <= max,
        "Operation '{}' count {} not in range [{}, {}]",
        operation_name,
        metrics.count,
        min,
        max
    );
}

/// Assert that operation throughput is within expected range
pub fn assert_throughput_in_range(
    snapshot: &MetricsSnapshot,
    operation_name: &str,
    min_ops_per_sec: f64,
    max_ops_per_sec: f64,
) {
    let metrics = snapshot
        .operation_metrics
        .get(operation_name)
        .expect(&format!("Operation '{}' not found in metrics", operation_name));

    let throughput = metrics.throughput(snapshot.duration);

    assert!(
        throughput >= min_ops_per_sec && throughput <= max_ops_per_sec,
        "Operation '{}' throughput {:.2} not in range [{:.2}, {:.2}]",
        operation_name,
        throughput,
        min_ops_per_sec,
        max_ops_per_sec
    );
}

/// Assert that error rate is below threshold
pub fn assert_error_rate_below(
    snapshot: &MetricsSnapshot,
    operation_name: &str,
    max_error_rate: f64,
) {
    let metrics = snapshot
        .operation_metrics
        .get(operation_name)
        .expect(&format!("Operation '{}' not found in metrics", operation_name));

    let error_rate = if metrics.count > 0 {
        metrics.errors as f64 / metrics.count as f64
    } else {
        0.0
    };

    assert!(
        error_rate <= max_error_rate,
        "Operation '{}' error rate {:.2}% exceeds threshold {:.2}%",
        operation_name,
        error_rate * 100.0,
        max_error_rate * 100.0
    );
}

/// Assert that latency percentile is within expected range
pub fn assert_latency_percentile(
    snapshot: &MetricsSnapshot,
    operation_name: &str,
    percentile: f64,
    max_micros: u64,
) {
    let metrics = snapshot
        .operation_metrics
        .get(operation_name)
        .expect(&format!("Operation '{}' not found in metrics", operation_name));

    let actual = metrics.latency_histogram.value_at_quantile(percentile);

    assert!(
        actual <= max_micros,
        "Operation '{}' p{} latency {} μs exceeds max {} μs",
        operation_name,
        (percentile * 100.0) as u8,
        actual,
        max_micros
    );
}

/// Assert that test duration is within expected range
pub fn assert_duration_in_range(
    actual: Duration,
    expected: Duration,
    tolerance_percent: f64,
) {
    let actual_secs = actual.as_secs_f64();
    let expected_secs = expected.as_secs_f64();
    let tolerance = expected_secs * tolerance_percent;

    assert!(
        (actual_secs - expected_secs).abs() <= tolerance,
        "Duration {:.2}s not within {:.0}% of expected {:.2}s",
        actual_secs,
        tolerance_percent * 100.0,
        expected_secs
    );
}

/// Assert that backpressure events occurred (or didn't)
pub fn assert_backpressure_events(snapshot: &MetricsSnapshot, should_occur: bool) {
    if should_occur {
        assert!(
            snapshot.backpressure_events > 0,
            "Expected backpressure events but found none"
        );
    } else {
        assert!(
            snapshot.backpressure_events == 0,
            "Expected no backpressure events but found {}",
            snapshot.backpressure_events
        );
    }
}

/// Assert that success rate is above threshold
pub fn assert_success_rate_above(
    snapshot: &MetricsSnapshot,
    operation_name: &str,
    min_success_rate: f64,
) {
    let metrics = snapshot
        .operation_metrics
        .get(operation_name)
        .expect(&format!("Operation '{}' not found in metrics", operation_name));

    let success_rate = metrics.success_rate();

    assert!(
        success_rate >= min_success_rate,
        "Operation '{}' success rate {:.2}% below threshold {:.2}%",
        operation_name,
        success_rate * 100.0,
        min_success_rate * 100.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsbench::metrics::OperationMetricsSnapshot;
    use std::collections::HashMap;

    fn create_test_snapshot() -> MetricsSnapshot {
        let mut operation_metrics = HashMap::new();
        let mut histogram = hdrhistogram::Histogram::new(3).unwrap();
        histogram.record(1000).unwrap(); // 1ms
        histogram.record(2000).unwrap(); // 2ms
        histogram.record(3000).unwrap(); // 3ms

        operation_metrics.insert(
            "test_op".to_string(),
            OperationMetricsSnapshot {
                count: 100,
                errors: 5,
                latency_histogram: histogram,
            },
        );

        MetricsSnapshot {
            operation_metrics,
            backpressure_events: 0,
            duration: Duration::from_secs(10),
            timestamp: std::time::SystemTime::now(),
        }
    }

    #[test]
    fn test_assert_metric_count_in_range() {
        let snapshot = create_test_snapshot();
        assert_metric_count_in_range(&snapshot, "test_op", 90, 110);
    }

    #[test]
    #[should_panic]
    fn test_assert_metric_count_out_of_range() {
        let snapshot = create_test_snapshot();
        assert_metric_count_in_range(&snapshot, "test_op", 200, 300);
    }

    #[test]
    fn test_assert_error_rate_below() {
        let snapshot = create_test_snapshot();
        assert_error_rate_below(&snapshot, "test_op", 0.10); // 10% max
    }

    #[test]
    fn test_assert_success_rate_above() {
        let snapshot = create_test_snapshot();
        assert_success_rate_above(&snapshot, "test_op", 0.90); // 90% min
    }
}
