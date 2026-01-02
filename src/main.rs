//! RSBench - Modern Database Testing Tool
//!
//! Main entry point for the CLI application.

use clap::Parser;
use rsbench::Result;

mod cli_impl;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Parse CLI arguments
    let cli = rsbench::cli::Cli::parse();

    // Load configuration (scenario → config → merge)
    let config = cli.load_config()?;

    // Execute command
    match cli.command {
        rsbench::cli::Commands::Run { .. } => cli_impl::run_scenario(config).await?,
        rsbench::cli::Commands::Prepare { workload } => {
            cli_impl::prepare_workload(config, &workload).await?;
        }
        rsbench::cli::Commands::Cleanup { .. } => {
            println!("Cleanup command not yet implemented");
        }
    }

    Ok(())
}
