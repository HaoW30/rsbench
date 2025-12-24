//! RSBench - Modern Database Testing Tool
//!
//! Main entry point for the CLI application.

use clap::Parser;
use rsbench::config::{ConfigLoader, ConfigSource};
use rsbench::Result;

mod cli_impl;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Parse CLI arguments
    let cli = rsbench::cli::Cli::parse();

    // Load configuration
    let config = if let Some(config_path) = cli.config {
        ConfigLoader::load(ConfigSource::File(config_path))?
    } else {
        // TODO: Build config from CLI args
        return Err(rsbench::Error::Config(
            "Config file required for M0".into(),
        ));
    };

    // Validate config
    ConfigLoader::validate(&config)?;

    // Execute command
    match cli.command {
        rsbench::cli::Commands::Run { .. } => cli_impl::run_scenario(config).await?,
        rsbench::cli::Commands::Prepare { .. } => {
            println!("Prepare command not yet implemented");
        }
        rsbench::cli::Commands::Cleanup { .. } => {
            println!("Cleanup command not yet implemented");
        }
    }

    Ok(())
}
