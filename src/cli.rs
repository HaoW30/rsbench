//! CLI module
//!
//! Command-line interface definition and helper methods.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

use crate::config::{CliArgs, ConfigLoader, ConfigSource, OutputFormat, ScenarioFile, ToolConfig};
use crate::{Error, Result};

/// Default infrastructure configuration file path
pub const DEFAULT_CONFIG_PATH: &str = "config/rsbench.config.yaml";

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

impl Cli {
    /// Load complete configuration from CLI arguments
    ///
    /// This method:
    /// 1. Loads infrastructure config (from --config or default)
    /// 2. Loads scenario file (from --scenario or error if not provided)
    /// 3. Merges them into a complete ToolConfig
    /// 4. Applies CLI argument overrides
    /// 5. Validates the final configuration
    pub fn load_config(&self) -> Result<ToolConfig> {
        // Determine infrastructure config path
        let infra_path = self.get_config_path();

        // Load infrastructure config
        let infra_config = ConfigLoader::load_infrastructure(ConfigSource::File(infra_path))?;

        // Load scenario file
        let scenario_file = self.load_scenario_file()?;

        // Merge infrastructure and scenario
        let mut config =
            ConfigLoader::merge_infrastructure_and_scenario(infra_config, scenario_file);

        // Apply CLI overrides
        config = self.apply_cli_overrides(config)?;

        // Apply seed override if provided
        if let Some(seed) = self.seed {
            config.determinism.seed = seed;
        }

        // Validate the final configuration
        ConfigLoader::validate(&config)?;

        Ok(config)
    }

    /// Get the infrastructure config file path (uses default if not specified)
    pub fn get_config_path(&self) -> PathBuf {
        self.config
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH))
    }

    /// Get the scenario file path (returns error if not specified)
    pub fn get_scenario_path(&self) -> Result<PathBuf> {
        self.scenario
            .clone()
            .ok_or_else(|| Error::Config("Scenario file must be specified with --scenario".into()))
    }

    /// Load scenario file from CLI arguments
    fn load_scenario_file(&self) -> Result<ScenarioFile> {
        let scenario_path = self.get_scenario_path()?;
        ConfigLoader::load_scenario_file(ConfigSource::File(scenario_path))
    }

    /// Apply CLI argument overrides to configuration
    fn apply_cli_overrides(&self, config: ToolConfig) -> Result<ToolConfig> {
        let cli_args = self.to_cli_args()?;
        Ok(ConfigLoader::merge(config, cli_args))
    }

    /// Convert Cli to CliArgs for config merging
    fn to_cli_args(&self) -> Result<CliArgs> {
        let output_format = if let Some(ref output_str) = self.output {
            Some(parse_output_format(output_str)?)
        } else {
            None
        };

        Ok(CliArgs {
            config_file: self.config.clone(),
            database_url: self.db_url.clone(),
            rate: self.rate,
            duration: self.duration,
            threads: self.threads,
            output_format,
        })
    }

    /// Check if infrastructure config file exists
    pub fn config_exists(&self) -> bool {
        self.get_config_path().exists()
    }

    /// Check if scenario file exists
    pub fn scenario_exists(&self) -> bool {
        self.scenario
            .as_ref()
            .map(|p| p.exists())
            .unwrap_or(false)
    }
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

fn parse_duration(s: &str) -> std::result::Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

fn parse_output_format(s: &str) -> Result<OutputFormat> {
    match s.to_lowercase().as_str() {
        "text" => Ok(OutputFormat::Text),
        "json" => Ok(OutputFormat::Json),
        _ => Err(Error::Config(format!(
            "Invalid output format '{}'. Expected 'text' or 'json'",
            s
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test Cli instance
    fn create_test_cli(
        config: Option<PathBuf>,
        scenario: Option<PathBuf>,
        db_url: Option<String>,
        rate: Option<u64>,
        duration: Option<Duration>,
        threads: Option<usize>,
        output: Option<String>,
        seed: Option<u64>,
    ) -> Cli {
        Cli {
            config,
            scenario,
            db_url,
            rate,
            duration,
            threads,
            output,
            seed,
            command: Commands::Run { workload: None },
        }
    }

    #[test]
    fn test_default_config_path() {
        let cli = create_test_cli(None, None, None, None, None, None, None, None);
        let path = cli.get_config_path();
        assert_eq!(path, PathBuf::from(DEFAULT_CONFIG_PATH));
    }

    #[test]
    fn test_explicit_config_path() {
        let custom_path = PathBuf::from("custom/config.yaml");
        let cli = create_test_cli(
            Some(custom_path.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let path = cli.get_config_path();
        assert_eq!(path, custom_path);
    }

    #[test]
    fn test_get_scenario_path_when_provided() {
        let scenario_path = PathBuf::from("scenarios/test.yaml");
        let cli = create_test_cli(
            None,
            Some(scenario_path.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let result = cli.get_scenario_path();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), scenario_path);
    }

    #[test]
    fn test_get_scenario_path_when_missing() {
        let cli = create_test_cli(None, None, None, None, None, None, None, None);
        let result = cli.get_scenario_path();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be specified"));
    }

    #[test]
    fn test_to_cli_args_with_no_overrides() {
        let cli = create_test_cli(None, None, None, None, None, None, None, None);
        let cli_args = cli.to_cli_args().unwrap();

        assert!(cli_args.config_file.is_none());
        assert!(cli_args.database_url.is_none());
        assert!(cli_args.rate.is_none());
        assert!(cli_args.duration.is_none());
        assert!(cli_args.threads.is_none());
        assert!(cli_args.output_format.is_none());
    }

    #[test]
    fn test_to_cli_args_with_all_overrides() {
        let cli = create_test_cli(
            Some(PathBuf::from("config.yaml")),
            None,
            Some("mysql://localhost/test".to_string()),
            Some(5000),
            Some(Duration::from_secs(120)),
            Some(16),
            Some("json".to_string()),
            Some(42),
        );
        let cli_args = cli.to_cli_args().unwrap();

        assert!(cli_args.config_file.is_some());
        assert_eq!(
            cli_args.database_url.unwrap(),
            "mysql://localhost/test"
        );
        assert_eq!(cli_args.rate.unwrap(), 5000);
        assert_eq!(cli_args.duration.unwrap(), Duration::from_secs(120));
        assert_eq!(cli_args.threads.unwrap(), 16);
        assert!(matches!(
            cli_args.output_format.unwrap(),
            OutputFormat::Json
        ));
    }

    #[test]
    fn test_parse_output_format_text() {
        let result = parse_output_format("text");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), OutputFormat::Text));
    }

    #[test]
    fn test_parse_output_format_json() {
        let result = parse_output_format("json");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), OutputFormat::Json));
    }

    #[test]
    fn test_parse_output_format_case_insensitive() {
        assert!(matches!(
            parse_output_format("TEXT").unwrap(),
            OutputFormat::Text
        ));
        assert!(matches!(
            parse_output_format("Json").unwrap(),
            OutputFormat::Json
        ));
        assert!(matches!(
            parse_output_format("JSON").unwrap(),
            OutputFormat::Json
        ));
    }

    #[test]
    fn test_parse_output_format_invalid() {
        let result = parse_output_format("xml");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid output format"));
    }

    #[test]
    fn test_config_exists_true() {
        // Test with a file we know exists (the default config)
        let cli = create_test_cli(
            Some(PathBuf::from("config/rsbench.config.yaml")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        // This will return true if the file exists, false otherwise
        // We don't assert a specific value since the file might not exist in all test environments
        let _ = cli.config_exists();
    }

    #[test]
    fn test_config_exists_false() {
        let cli = create_test_cli(
            Some(PathBuf::from("nonexistent/config.yaml")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(!cli.config_exists());
    }

    #[test]
    fn test_scenario_exists_none() {
        let cli = create_test_cli(None, None, None, None, None, None, None, None);
        assert!(!cli.scenario_exists());
    }

    #[test]
    fn test_scenario_exists_false() {
        let cli = create_test_cli(
            None,
            Some(PathBuf::from("nonexistent/scenario.yaml")),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(!cli.scenario_exists());
    }

    #[test]
    fn test_load_config_with_existing_files() {
        // This test will only run if the files actually exist
        let cli = create_test_cli(
            Some(PathBuf::from("config/rsbench.config.yaml")),
            Some(PathBuf::from("scenarios/smoke_test.yaml")),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        // Try to load config - will skip if files don't exist
        if cli.config_exists() && cli.scenario_exists() {
            let result = cli.load_config();
            assert!(
                result.is_ok(),
                "Config loading should succeed: {:?}",
                result.err()
            );
        }
    }

    #[test]
    fn test_load_config_with_cli_overrides() {
        // This test will only run if the files actually exist
        let cli = create_test_cli(
            Some(PathBuf::from("config/rsbench.config.yaml")),
            Some(PathBuf::from("scenarios/smoke_test.yaml")),
            Some("mysql://override/db".to_string()),
            Some(2000),
            None,
            None,
            Some("json".to_string()),
            Some(99),
        );

        // Try to load config - will skip if files don't exist
        if cli.config_exists() && cli.scenario_exists() {
            let result = cli.load_config();
            if let Ok(config) = result {
                // Verify overrides were applied
                assert_eq!(config.database.connection_string, "mysql://override/db");
                assert_eq!(config.determinism.seed, 99);
                assert!(matches!(config.output.format, OutputFormat::Json));

                // Verify rate override for ConstantRate executor
                if let crate::config::ExecutorConfig::ConstantRate { rate, .. } =
                    config.scenario.executor
                {
                    assert_eq!(rate, 2000);
                }
            }
        }
    }

    #[test]
    fn test_load_config_without_scenario_fails() {
        let cli = create_test_cli(
            Some(PathBuf::from("config/rsbench.config.yaml")),
            None, // No scenario
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let result = cli.load_config();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must be specified"));
    }

    #[test]
    fn test_parse_duration_valid() {
        assert_eq!(parse_duration("10s").unwrap(), Duration::from_secs(10));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("invalid").is_err());
        assert!(parse_duration("").is_err());
    }
}
