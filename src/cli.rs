//! CLI module
//!
//! Command-line interface definition.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "rsbench")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Modern database testing tool", long_about = None)]
pub struct Cli {
    /// Infrastructure configuration file (database, runtime, pool settings)
    /// Default: config/rsbench.config.yaml
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Scenario file (workload, executor, test parameters)
    /// Defines what test to run
    #[arg(short, long)]
    pub scenario: Option<PathBuf>,

    /// Database connection string (overrides config file)
    #[arg(long)]
    pub db_url: Option<String>,

    /// Target rate (ops/sec) (overrides scenario file)
    #[arg(long)]
    pub rate: Option<u64>,

    /// Test duration (overrides scenario file)
    #[arg(long, value_parser = parse_duration)]
    pub duration: Option<Duration>,

    /// Number of threads (blocking mode) (overrides config file)
    #[arg(long)]
    pub threads: Option<usize>,

    /// Output format (text|json) (overrides config/scenario file)
    #[arg(long)]
    pub output: Option<String>,

    /// Determinism seed (overrides scenario file)
    #[arg(long)]
    pub seed: Option<u64>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run a workload
    Run {
        /// Workload name or Lua script
        workload: Option<String>,
    },

    /// Prepare database (create tables, load data)
    Prepare { workload: String },

    /// Cleanup database
    Cleanup { workload: String },
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}
