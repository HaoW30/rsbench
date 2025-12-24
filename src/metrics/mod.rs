//! Metrics module
//!
//! Lock-free metrics collection with HDR histograms.

mod output;

pub use output::{JsonOutput, MetricsOutput, TextOutput};

use crate::driver::QueryResult;
use crate::Result;
use dashmap::DashMap;
use hdrhistogram::Histogram;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Metrics collector (lock-free)
pub struct MetricsCollector {
    operation_metrics: DashMap<String, OperationMetrics>,
    backpressure_events: AtomicU64,
    start_time: Instant,
}

impl MetricsCollector {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            operation_metrics: DashMap::new(),
            backpressure_events: AtomicU64::new(0),
            start_time: Instant::now(),
        })
    }

    /// Record operation execution
    pub fn record_operation(
        &self,
        operation_name: &str,
        duration: Duration,
        result: &Result<QueryResult>,
    ) {
        let entry = self
            .operation_metrics
            .entry(operation_name.to_string())
            .or_insert_with(OperationMetrics::new);

        entry.count.fetch_add(1, Ordering::Relaxed);

        if result.is_err() {
            entry.errors.fetch_add(1, Ordering::Relaxed);
        }

        // Record latency in histogram
        let _ = entry
            .histogram
            .lock()
            .unwrap()
            .record(duration.as_micros() as u64);
    }

    /// Record backpressure event
    pub fn record_backpressure_event(&self) {
        self.backpressure_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot of all metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        let mut operation_metrics = HashMap::new();

        for entry in self.operation_metrics.iter() {
            let key = entry.key().clone();
            let metrics = entry.value();

            operation_metrics.insert(
                key,
                OperationMetricsSnapshot {
                    count: metrics.count.load(Ordering::Relaxed),
                    errors: metrics.errors.load(Ordering::Relaxed),
                    latency_histogram: metrics.histogram.lock().unwrap().clone(),
                },
            );
        }

        MetricsSnapshot {
            operation_metrics,
            backpressure_events: self.backpressure_events.load(Ordering::Relaxed),
            duration: self.start_time.elapsed(),
            timestamp: SystemTime::now(),
        }
    }
}


/// Per-operation metrics (lock-free counters + mutex histogram)
struct OperationMetrics {
    count: AtomicU64,
    errors: AtomicU64,
    histogram: std::sync::Mutex<Histogram<u64>>,
}

impl OperationMetrics {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            histogram: std::sync::Mutex::new(Histogram::new(3).unwrap()), // 3 significant digits
        }
    }
}

/// Immutable metrics snapshot
pub struct MetricsSnapshot {
    pub operation_metrics: HashMap<String, OperationMetricsSnapshot>,
    pub backpressure_events: u64,
    pub duration: Duration,
    pub timestamp: SystemTime,
}

#[derive(Clone)]
pub struct OperationMetricsSnapshot {
    pub count: u64,
    pub errors: u64,
    pub latency_histogram: Histogram<u64>,
}

impl OperationMetricsSnapshot {
    pub fn success_rate(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            (self.count - self.errors) as f64 / self.count as f64
        }
    }

    pub fn throughput(&self, duration: Duration) -> f64 {
        if duration.as_secs_f64() == 0.0 {
            0.0
        } else {
            self.count as f64 / duration.as_secs_f64()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::QueryResult;

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        let snapshot = collector.snapshot();

        assert_eq!(snapshot.operation_metrics.len(), 0);
        assert_eq!(snapshot.backpressure_events, 0);
    }

    #[test]
    fn test_record_successful_operation() {
        let collector = MetricsCollector::new();
        let result = Ok(QueryResult {
            rows_affected: 1,
            last_insert_id: None,
        });

        collector.record_operation("test_op", Duration::from_millis(10), &result);

        let snapshot = collector.snapshot();
        let metrics = snapshot.operation_metrics.get("test_op").unwrap();

        assert_eq!(metrics.count, 1);
        assert_eq!(metrics.errors, 0);
    }

    #[test]
    fn test_record_failed_operation() {
        let collector = MetricsCollector::new();
        let result: Result<QueryResult> =
            Err(crate::Error::Database(crate::DatabaseError::Query(
                "test error".to_string(),
            )));

        collector.record_operation("test_op", Duration::from_millis(10), &result);

        let snapshot = collector.snapshot();
        let metrics = snapshot.operation_metrics.get("test_op").unwrap();

        assert_eq!(metrics.count, 1);
        assert_eq!(metrics.errors, 1);
    }

    #[test]
    fn test_record_multiple_operations() {
        let collector = MetricsCollector::new();
        let success_result = Ok(QueryResult {
            rows_affected: 1,
            last_insert_id: None,
        });

        for _ in 0..10 {
            collector.record_operation("test_op", Duration::from_millis(5), &success_result);
        }

        let snapshot = collector.snapshot();
        let metrics = snapshot.operation_metrics.get("test_op").unwrap();

        assert_eq!(metrics.count, 10);
        assert_eq!(metrics.errors, 0);
    }

    #[test]
    fn test_record_backpressure_events() {
        let collector = MetricsCollector::new();

        collector.record_backpressure_event();
        collector.record_backpressure_event();
        collector.record_backpressure_event();

        let snapshot = collector.snapshot();
        assert_eq!(snapshot.backpressure_events, 3);
    }

    #[test]
    fn test_success_rate_calculation() {
        let metrics = OperationMetricsSnapshot {
            count: 100,
            errors: 5,
            latency_histogram: Histogram::new(3).unwrap(),
        };

        let success_rate = metrics.success_rate();
        assert!((success_rate - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_zero_count() {
        let metrics = OperationMetricsSnapshot {
            count: 0,
            errors: 0,
            latency_histogram: Histogram::new(3).unwrap(),
        };

        assert_eq!(metrics.success_rate(), 0.0);
    }

    #[test]
    fn test_throughput_calculation() {
        let metrics = OperationMetricsSnapshot {
            count: 1000,
            errors: 0,
            latency_histogram: Histogram::new(3).unwrap(),
        };

        let throughput = metrics.throughput(Duration::from_secs(10));
        assert!((throughput - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_throughput_zero_duration() {
        let metrics = OperationMetricsSnapshot {
            count: 100,
            errors: 0,
            latency_histogram: Histogram::new(3).unwrap(),
        };

        assert_eq!(metrics.throughput(Duration::from_secs(0)), 0.0);
    }

    #[test]
    fn test_latency_histogram_recording() {
        let collector = MetricsCollector::new();
        let result = Ok(QueryResult {
            rows_affected: 1,
            last_insert_id: None,
        });

        // Record operations with different latencies
        collector.record_operation("test_op", Duration::from_millis(1), &result);
        collector.record_operation("test_op", Duration::from_millis(2), &result);
        collector.record_operation("test_op", Duration::from_millis(3), &result);

        let snapshot = collector.snapshot();
        let metrics = snapshot.operation_metrics.get("test_op").unwrap();

        // Verify histogram has data
        assert!(metrics.latency_histogram.len() > 0);
        assert_eq!(metrics.count, 3);
    }
}
