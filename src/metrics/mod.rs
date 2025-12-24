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
