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
