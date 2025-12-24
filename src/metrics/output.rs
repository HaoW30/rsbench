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
            writeln!(self.writer, "Operation: {}", op_name)?;
            writeln!(self.writer, "  Count: {}", metrics.count)?;
            writeln!(self.writer, "  Errors: {}", metrics.errors)?;
            writeln!(
                self.writer,
                "  Throughput: {:.2} ops/sec",
                metrics.throughput(snapshot.duration)
            )?;
            writeln!(self.writer, "  Latency:")?;
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
            writeln!(self.writer)?;
        }

        writeln!(
            self.writer,
            "Backpressure events: {}",
            snapshot.backpressure_events
        )?;

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
            duration: Duration::from_secs(1),
            timestamp: SystemTime::now(),
        };

        let buffer: Vec<u8> = Vec::new();
        let mut output = JsonOutput::new(Box::new(buffer));

        let result = output.export(&snapshot);
        assert!(result.is_ok());
    }
}
