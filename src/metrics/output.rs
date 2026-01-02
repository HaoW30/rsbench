//! Metrics output formats

use super::MetricsSnapshot;
use crate::Result;
use std::io::Write;

/// Metrics output trait
pub trait MetricsOutput: Send + Sync {
    fn export(&mut self, snapshot: &MetricsSnapshot) -> Result<()>;
}

/// Text output (sysbench-compatible)
pub struct TextOutput {
    writer: Box<dyn Write + Send + Sync>,
}

impl TextOutput {
    pub fn new(writer: Box<dyn Write + Send + Sync>) -> Self {
        Self { writer }
    }
}

impl MetricsOutput for TextOutput {
    fn export(&mut self, snapshot: &MetricsSnapshot) -> Result<()> {
        writeln!(self.writer, "RSBench Results:")?;
        writeln!(self.writer, "Total duration: {:?}", snapshot.duration)?;
        writeln!(self.writer)?;

        for (op_name, metrics) in &snapshot.operation_metrics {
            let error_rate = metrics.error_rate() * 100.0;
            let success_rate = metrics.success_rate() * 100.0;

            writeln!(self.writer, "Operation: {}", op_name)?;
            writeln!(self.writer, "  Count: {}", metrics.count)?;
            writeln!(
                self.writer,
                "  Errors: {} ({:.2}%)",
                metrics.errors, error_rate
            )?;
            writeln!(self.writer, "  Success Rate: {:.2}%", success_rate)?;
            writeln!(
                self.writer,
                "  Throughput: {:.2} ops/sec",
                metrics.throughput(snapshot.duration)
            )?;
            writeln!(self.writer, "  Latency:")?;
            writeln!(
                self.writer,
                "    min: {} μs",
                metrics.latency_histogram.min()
            )?;
            writeln!(
                self.writer,
                "    max: {} μs",
                metrics.latency_histogram.max()
            )?;
            writeln!(
                self.writer,
                "    mean: {:.2} μs",
                metrics.latency_histogram.mean()
            )?;
            writeln!(
                self.writer,
                "    p50: {} μs",
                metrics.latency_histogram.value_at_quantile(0.50)
            )?;
            writeln!(
                self.writer,
                "    p95: {} μs",
                metrics.latency_histogram.value_at_quantile(0.95)
            )?;
            writeln!(
                self.writer,
                "    p99: {} μs",
                metrics.latency_histogram.value_at_quantile(0.99)
            )?;
            writeln!(
                self.writer,
                "    p999: {} μs",
                metrics.latency_histogram.value_at_quantile(0.999)
            )?;

            // Warning for high error rate
            if error_rate > 1.0 {
                writeln!(self.writer)?;
                writeln!(
                    self.writer,
                    "  ⚠️  Warning: Error rate ({:.2}%) exceeds recommended threshold (1.0%)",
                    error_rate
                )?;
            }

            writeln!(self.writer)?;
        }

        writeln!(self.writer, "Client Metrics:")?;
        writeln!(
            self.writer,
            "  Backpressure events: {}",
            snapshot.backpressure_events
        )?;
        writeln!(
            self.writer,
            "  Pool saturation events: {}",
            snapshot.pool_saturation_events
        )?;
        writeln!(
            self.writer,
            "  Runtime saturation events: {}",
            snapshot.runtime_saturation_events
        )?;

        // Warning for high backpressure
        let total_ops: u64 = snapshot.operation_metrics.values().map(|m| m.count).sum();
        if total_ops > 0 {
            let backpressure_rate = snapshot.backpressure_events as f64 / total_ops as f64 * 100.0;
            if backpressure_rate > 5.0 {
                writeln!(self.writer)?;
                writeln!(
                    self.writer,
                    "⚠️  Warning: High backpressure rate ({:.2}%) - client may be saturated",
                    backpressure_rate
                )?;
                writeln!(
                    self.writer,
                    "    Consider increasing max_connections or reducing target rate"
                )?;
            }
        }

        Ok(())
    }
}

/// JSON output
pub struct JsonOutput {
    writer: Box<dyn Write + Send + Sync>,
}

impl JsonOutput {
    pub fn new(writer: Box<dyn Write + Send + Sync>) -> Self {
        Self { writer }
    }
}

impl MetricsOutput for JsonOutput {
    fn export(&mut self, snapshot: &MetricsSnapshot) -> Result<()> {
        use serde_json::json;

        let mut operations = serde_json::Map::new();

        for (op_name, metrics) in &snapshot.operation_metrics {
            operations.insert(
                op_name.clone(),
                json!({
                    "count": metrics.count,
                    "errors": metrics.errors,
                    "error_rate": metrics.error_rate(),
                    "success_rate": metrics.success_rate(),
                    "throughput": metrics.throughput(snapshot.duration),
                    "latency": {
                        "min": metrics.latency_histogram.min(),
                        "max": metrics.latency_histogram.max(),
                        "mean": metrics.latency_histogram.mean(),
                        "p50": metrics.latency_histogram.value_at_quantile(0.50),
                        "p95": metrics.latency_histogram.value_at_quantile(0.95),
                        "p99": metrics.latency_histogram.value_at_quantile(0.99),
                        "p999": metrics.latency_histogram.value_at_quantile(0.999),
                    }
                }),
            );
        }

        let output = json!({
            "timestamp": snapshot.timestamp,
            "duration_secs": snapshot.duration.as_secs_f64(),
            "operations": operations,
            "client_metrics": {
                "backpressure_events": snapshot.backpressure_events,
                "pool_saturation_events": snapshot.pool_saturation_events,
                "runtime_saturation_events": snapshot.runtime_saturation_events,
            }
        });

        serde_json::to_writer_pretty(&mut self.writer, &output)?;
        writeln!(self.writer)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{MetricsSnapshot, OperationMetricsSnapshot};
    use hdrhistogram::Histogram;
    use std::collections::HashMap;
    use std::time::{Duration, SystemTime};

    fn create_test_snapshot() -> MetricsSnapshot {
        let mut operation_metrics = HashMap::new();
        let mut histogram = Histogram::new(3).unwrap();
        histogram.record(1000).unwrap(); // 1ms
        histogram.record(2000).unwrap(); // 2ms
        histogram.record(5000).unwrap(); // 5ms

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
            backpressure_events: 3,
            pool_saturation_events: 2,
            runtime_saturation_events: 1,
            duration: Duration::from_secs(10),
            timestamp: SystemTime::now(),
        }
    }

    #[test]
    fn test_text_output_export_succeeds() {
        let snapshot = create_test_snapshot();
        let buffer: Vec<u8> = Vec::new();
        let mut output = TextOutput::new(Box::new(buffer));

        let result = output.export(&snapshot);
        assert!(result.is_ok());
    }

    #[test]
    fn test_json_output_export_succeeds() {
        let snapshot = create_test_snapshot();
        let buffer: Vec<u8> = Vec::new();
        let mut output = JsonOutput::new(Box::new(buffer));

        let result = output.export(&snapshot);
        assert!(result.is_ok());
    }

    #[test]
    fn test_text_output_empty_metrics_succeeds() {
        let snapshot = MetricsSnapshot {
            operation_metrics: HashMap::new(),
            backpressure_events: 0,
            pool_saturation_events: 0,
            runtime_saturation_events: 0,
            duration: Duration::from_secs(1),
            timestamp: SystemTime::now(),
        };

        let buffer: Vec<u8> = Vec::new();
        let mut output = TextOutput::new(Box::new(buffer));

        let result = output.export(&snapshot);
        assert!(result.is_ok());
    }

    #[test]
    fn test_json_output_empty_metrics_succeeds() {
        let snapshot = MetricsSnapshot {
            operation_metrics: HashMap::new(),
            backpressure_events: 0,
            pool_saturation_events: 0,
            runtime_saturation_events: 0,
            duration: Duration::from_secs(1),
            timestamp: SystemTime::now(),
        };

        let buffer: Vec<u8> = Vec::new();
        let mut output = JsonOutput::new(Box::new(buffer));

        let result = output.export(&snapshot);
        assert!(result.is_ok());
    }
}
