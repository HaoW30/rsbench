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
    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Database connection string
    #[arg(long)]
    pub db_url: Option<String>,

    /// Target rate (ops/sec)
    #[arg(long)]
    pub rate: Option<u64>,

    /// Test duration
    #[arg(long, value_parser = parse_duration)]
    pub duration: Option<Duration>,

    /// Number of threads (blocking mode)
    #[arg(long)]
    pub threads: Option<usize>,

    /// Output format (text|json)
    #[arg(long)]
    pub output: Option<String>,

    /// Determinism seed
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
