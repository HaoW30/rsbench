//! Configuration module
//!
//! Handles loading, parsing, and validating configuration from various sources.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::{Error, Result};

/// Configuration loader - entry point for loading config
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from various sources (legacy: combined config)
    pub fn load(source: ConfigSource) -> Result<ToolConfig> {
        match source {
            ConfigSource::File(path) => Self::load_from_file(&path),
            ConfigSource::Yaml(content) => Self::load_from_yaml(&content),
            ConfigSource::CliArgs(args) => Self::load_from_cli(args),
        }
    }

    /// Load infrastructure configuration (database, runtime, output)
    pub fn load_infrastructure(source: ConfigSource) -> Result<InfrastructureConfig> {
        match source {
            ConfigSource::File(path) => {
                let content = std::fs::read_to_string(path)?;
                Self::load_infrastructure_from_yaml(&content)
            }
            ConfigSource::Yaml(content) => Self::load_infrastructure_from_yaml(&content),
            ConfigSource::CliArgs(_) => {
                Err(Error::Config("Infrastructure config from CLI not supported".into()))
            }
        }
    }

    /// Load scenario file (may reference infrastructure config)
    pub fn load_scenario_file(source: ConfigSource) -> Result<ScenarioFile> {
        match source {
            ConfigSource::File(path) => {
                let content = std::fs::read_to_string(path)?;
                Self::load_scenario_from_yaml(&content)
            }
            ConfigSource::Yaml(content) => Self::load_scenario_from_yaml(&content),
            ConfigSource::CliArgs(_) => {
                Err(Error::Config("Scenario from CLI not supported".into()))
            }
        }
    }

    /// Merge infrastructure and scenario into complete config
    pub fn merge_infrastructure_and_scenario(
        infra: InfrastructureConfig,
        scenario_file: ScenarioFile,
    ) -> ToolConfig {
        ToolConfig {
            database: infra.database,
            runtime: infra.runtime,
            scenario: scenario_file.scenario,
            determinism: scenario_file.determinism.unwrap_or_default(),
            output: scenario_file.output.unwrap_or(infra.output),
        }
    }

    /// Validate configuration consistency
    pub fn validate(config: &ToolConfig) -> Result<()> {
        // Validate pool config constraints
        if config.database.pool.min_size > config.database.pool.max_size {
            return Err(Error::Config(format!(
                "Pool min_size ({}) cannot be greater than max_size ({})",
                config.database.pool.min_size, config.database.pool.max_size
            )));
        }

        if config.database.pool.max_size == 0 {
            return Err(Error::Config("Pool max_size must be greater than 0".into()));
        }

        // Validate connection string is not empty
        if config.database.connection_string.trim().is_empty() {
            return Err(Error::Config("Database connection string cannot be empty".into()));
        }

        // Basic connection string format validation
        let conn_str = &config.database.connection_string;
        if !conn_str.contains("://") {
            return Err(Error::Config(format!(
                "Invalid connection string format: '{}'. Expected format: driver://host/db",
                conn_str
            )));
        }

        // Validate runtime config
        match &config.runtime.mode {
            RuntimeMode::Async {
                workers,
                max_connections,
                backpressure_threshold,
            } => {
                if *workers == 0 {
                    return Err(Error::Config("Async runtime workers must be > 0".into()));
                }
                if *max_connections == 0 {
                    return Err(Error::Config(
                        "Async runtime max_connections must be > 0".into(),
                    ));
                }
                if !(*backpressure_threshold >= 0.0 && *backpressure_threshold <= 1.0) {
                    return Err(Error::Config(format!(
                        "Backpressure threshold must be between 0.0 and 1.0, got {}",
                        backpressure_threshold
                    )));
                }
                // Check that max_connections doesn't exceed pool max_size
                if *max_connections > config.database.pool.max_size {
                    return Err(Error::Config(format!(
                        "Runtime max_connections ({}) cannot exceed pool max_size ({})",
                        max_connections, config.database.pool.max_size
                    )));
                }
            }
            RuntimeMode::Blocking { threads } => {
                if *threads == 0 {
                    return Err(Error::Config("Blocking runtime threads must be > 0".into()));
                }
            }
        }

        // Validate executor config
        match &config.scenario.executor {
            ExecutorConfig::ConstantRate {
                rate,
                duration,
                max_connections,
            } => {
                if *rate == 0 {
                    return Err(Error::Config("Executor rate must be > 0".into()));
                }
                if duration.as_secs() == 0 && duration.subsec_nanos() == 0 {
                    return Err(Error::Config("Executor duration must be > 0".into()));
                }
                if *max_connections == 0 {
                    return Err(Error::Config("Executor max_connections must be > 0".into()));
                }
            }
            ExecutorConfig::RampingRate {
                stages,
                max_connections,
                ..
            } => {
                if stages.is_empty() {
                    return Err(Error::Config("Ramping executor must have at least one stage".into()));
                }
                for (i, stage) in stages.iter().enumerate() {
                    if stage.target_rate == 0 {
                        return Err(Error::Config(format!(
                            "Stage {} target_rate must be > 0",
                            i
                        )));
                    }
                    if stage.duration.as_secs() == 0 && stage.duration.subsec_nanos() == 0 {
                        return Err(Error::Config(format!("Stage {} duration must be > 0", i)));
                    }
                }
                if *max_connections == 0 {
                    return Err(Error::Config("Executor max_connections must be > 0".into()));
                }
            }
        }

        // Validate workload config
        match &config.scenario.workload {
            WorkloadConfig::Declarative {
                file,
                definition,
                overrides: _,
            } => {
                // Must have either file or inline definition
                if file.is_none() && definition.is_none() {
                    return Err(Error::Config(
                        "Declarative workload must specify either 'file' or 'definition'".into(),
                    ));
                }
                // If file is specified, ensure it's not empty
                if let Some(f) = file {
                    if f.as_os_str().is_empty() {
                        return Err(Error::Config(
                            "Declarative workload file path cannot be empty".into(),
                        ));
                    }
                }
            }
            WorkloadConfig::Lua { script } => {
                if script.as_os_str().is_empty() {
                    return Err(Error::Config("Lua script path cannot be empty".into()));
                }
            }
            #[allow(deprecated)]
            WorkloadConfig::Builtin {
                name,
                table_count,
                table_size,
            } => {
                eprintln!(
                    "Warning: Builtin workloads are deprecated. Use declarative workloads instead."
                );
                if name.is_empty() {
                    return Err(Error::Config("Workload name cannot be empty".into()));
                }
                if *table_count == 0 {
                    return Err(Error::Config("Workload table_count must be > 0".into()));
                }
                if *table_size == 0 {
                    return Err(Error::Config("Workload table_size must be > 0".into()));
                }
            }
        }

        Ok(())
    }

    /// Merge CLI args with file config (precedence: CLI > file > defaults)
    pub fn merge(mut file_config: ToolConfig, cli_args: CliArgs) -> ToolConfig {
        // Override database connection string if provided
        if let Some(db_url) = cli_args.database_url {
            file_config.database.connection_string = db_url;
        }

        // Override executor rate if provided (only for ConstantRate)
        if let Some(rate) = cli_args.rate {
            if let ExecutorConfig::ConstantRate {
                rate: ref mut config_rate,
                ..
            } = file_config.scenario.executor
            {
                *config_rate = rate;
            }
        }

        // Override executor duration if provided
        if let Some(duration) = cli_args.duration {
            match &mut file_config.scenario.executor {
                ExecutorConfig::ConstantRate {
                    duration: ref mut config_duration,
                    ..
                } => {
                    *config_duration = duration;
                }
                ExecutorConfig::RampingRate { .. } => {
                    // For ramping rate, we can't easily override duration
                    // Could log a warning here in the future
                }
            }
        }

        // Override runtime threads if provided (only for Blocking mode)
        if let Some(threads) = cli_args.threads {
            if let RuntimeMode::Blocking {
                threads: ref mut config_threads,
            } = file_config.runtime.mode
            {
                *config_threads = threads;
            }
        }

        // Override output format if provided
        if let Some(format) = cli_args.output_format {
            file_config.output.format = format;
        }

        file_config
    }

    /// Apply default values to incomplete config
    /// Note: Most defaults are already handled by serde defaults
    /// This method is available for any additional normalization
    pub fn with_defaults(config: ToolConfig) -> ToolConfig {
        // Serde already applies defaults via #[serde(default)] attributes
        // This method can be used for additional runtime defaults or normalization
        // For now, it's a passthrough as serde handles everything
        config
    }

    fn load_from_file(path: &PathBuf) -> Result<ToolConfig> {
        let content = std::fs::read_to_string(path)?;
        Self::load_from_yaml(&content)
    }

    fn load_from_yaml(content: &str) -> Result<ToolConfig> {
        serde_yaml::from_str(content)
            .map_err(|e| Error::Config(format!("YAML parse error: {}", e)))
    }

    fn load_infrastructure_from_yaml(content: &str) -> Result<InfrastructureConfig> {
        serde_yaml::from_str(content)
            .map_err(|e| Error::Config(format!("Infrastructure config parse error: {}", e)))
    }

    fn load_scenario_from_yaml(content: &str) -> Result<ScenarioFile> {
        serde_yaml::from_str(content)
            .map_err(|e| Error::Config(format!("Scenario file parse error: {}", e)))
    }

    fn load_from_cli(_args: CliArgs) -> Result<ToolConfig> {
        // TODO: Build config from CLI args
        Err(Error::Config("CLI-only config not yet implemented".into()))
    }
}

/// Configuration source
pub enum ConfigSource {
    /// Load from YAML file
    File(PathBuf),
    /// Parse from YAML string
    Yaml(String),
    /// From CLI arguments only
    CliArgs(CliArgs),
}

/// Infrastructure configuration (database, runtime, output)
/// This is configured once per environment and reused across tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfrastructureConfig {
    pub database: DatabaseConfig,
    pub runtime: RuntimeConfig,
    pub output: OutputConfig,
}

/// Scenario file structure (can optionally reference infrastructure config)
/// This defines the test workload and parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioFile {
    /// Optional reference to infrastructure config file
    /// If not specified, user must provide via CLI --config flag
    #[serde(default)]
    pub config: Option<PathBuf>,

    /// Test scenario definition
    pub scenario: ScenarioConfig,

    /// Determinism settings (optional, uses defaults if not specified)
    #[serde(default)]
    pub determinism: Option<DeterminismConfig>,

    /// Output settings (optional, uses infrastructure config if not specified)
    pub output: Option<OutputConfig>,
}

/// Root configuration structure (M0 simplified)
/// This is the complete merged config used at runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub database: DatabaseConfig,
    pub runtime: RuntimeConfig,
    pub scenario: ScenarioConfig,
    #[serde(default)]
    pub determinism: DeterminismConfig,
    pub output: OutputConfig,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Driver name ("mysql")
    pub driver: String,

    /// Connection string (e.g., "mysql://user:pass@host:port/db")
    pub connection_string: String,

    /// Connection pool configuration
    #[serde(default)]
    pub pool: PoolConfig,
}

/// Connection pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Minimum pool size
    #[serde(default = "default_min_size")]
    pub min_size: usize,

    /// Maximum pool size
    #[serde(default = "default_max_size")]
    pub max_size: usize,

    /// Connection timeout
    #[serde(default = "default_connection_timeout", with = "humantime_serde")]
    pub connection_timeout: Duration,

    /// Idle connection timeout
    #[serde(default = "default_idle_timeout", with = "humantime_serde")]
    pub idle_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_size: default_min_size(),
            max_size: default_max_size(),
            connection_timeout: default_connection_timeout(),
            idle_timeout: default_idle_timeout(),
        }
    }
}

fn default_min_size() -> usize {
    10
}
fn default_max_size() -> usize {
    100
}
fn default_connection_timeout() -> Duration {
    Duration::from_secs(10)
}
fn default_idle_timeout() -> Duration {
    Duration::from_secs(300)
}

/// Runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(flatten)]
    pub mode: RuntimeMode,
}

/// Runtime execution mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuntimeMode {
    /// Blocking mode (sysbench compatibility)
    Blocking {
        #[serde(default = "default_threads")]
        threads: usize,
    },
    /// Async mode (primary, backpressure-aware)
    Async {
        #[serde(default = "default_workers")]
        workers: usize,

        #[serde(default = "default_max_connections")]
        max_connections: usize,

        #[serde(default = "default_backpressure_threshold")]
        backpressure_threshold: f64,
    },
}

fn default_threads() -> usize {
    16
}
fn default_workers() -> usize {
    num_cpus::get()
}
fn default_max_connections() -> usize {
    100
}
fn default_backpressure_threshold() -> f64 {
    0.8
}

/// Scenario configuration (M0: single scenario only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioConfig {
    /// Executor configuration
    pub executor: ExecutorConfig,

    /// Workload definition
    pub workload: WorkloadConfig,
}

/// Executor configuration (M0: constant-rate and ramping-rate)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ExecutorConfig {
    ConstantRate {
        /// Operations per second
        rate: u64,

        /// Test duration
        #[serde(with = "humantime_serde")]
        duration: Duration,

        /// Maximum concurrent connections
        #[serde(default = "default_max_connections")]
        max_connections: usize,
    },
    RampingRate {
        /// Rate stages
        stages: Vec<RateStage>,

        /// Preallocate connections
        #[serde(default = "default_prealloc")]
        prealloc_connections: usize,

        /// Maximum concurrent connections
        #[serde(default = "default_max_connections")]
        max_connections: usize,
    },
}

fn default_prealloc() -> usize {
    50
}

/// Rate stage for ramping executor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateStage {
    #[serde(with = "humantime_serde")]
    pub duration: Duration,
    pub target_rate: u64,
}

/// Workload configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WorkloadConfig {
    /// Declarative workload (YAML-based, recommended)
    /// This is the primary workload format supporting all sysbench features
    Declarative {
        /// Path to declarative workload YAML file
        /// Example: workloads/oltp_read_write.yaml
        #[serde(default)]
        file: Option<PathBuf>,

        /// Inline workload definition (alternative to file)
        #[serde(default)]
        definition: Option<serde_yaml::Value>,

        /// Overrides for specific workload parameters
        /// Allows customizing pre-defined workloads without editing the file
        #[serde(default)]
        overrides: Option<serde_yaml::Value>,
    },

    /// Lua script (for complex custom workloads)
    Lua {
        /// Path to Lua script file
        script: PathBuf,
    },

    /// Built-in OLTP workload (DEPRECATED - use declarative instead)
    /// This will be removed in a future version
    /// Migrate to: workloads/oltp_read_write.yaml
    #[deprecated(
        since = "0.2.0",
        note = "Use declarative workloads instead. See workloads/oltp_*.yaml"
    )]
    Builtin {
        name: String, // "oltp_read_write"
        #[serde(default = "default_table_count")]
        table_count: usize,
        #[serde(default = "default_table_size")]
        table_size: usize,
    },
}

fn default_table_count() -> usize {
    10
}
fn default_table_size() -> usize {
    10000
}

/// Determinism configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterminismConfig {
    /// RNG seed
    #[serde(default = "default_seed")]
    pub seed: u64,

    /// Strict mode (fail on non-deterministic operations)
    #[serde(default)]
    pub strict_mode: bool,
}

fn default_seed() -> u64 {
    42
}

impl Default for DeterminismConfig {
    fn default() -> Self {
        Self {
            seed: default_seed(),
            strict_mode: false,
        }
    }
}

/// Output configuration (M0: text or JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output format
    pub format: OutputFormat,

    /// Output file (None = stdout)
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Text,
    Json,
}

/// CLI arguments structure
pub struct CliArgs {
    pub config_file: Option<PathBuf>,
    pub database_url: Option<String>,
    pub rate: Option<u64>,
    pub duration: Option<Duration>,
    pub threads: Option<usize>,
    pub output_format: Option<OutputFormat>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config_from_yaml() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"
  pool:
    min_size: 1
    max_size: 10
    connection_timeout: 5s
    idle_timeout: 60s

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write
    table_count: 10
    table_size: 10000

determinism:
  seed: 42
  strict_mode: false

output:
  format: json
  file: null
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string()));
        if let Err(e) = &config {
            panic!("Config loading failed: {:?}", e);
        }

        let config = config.unwrap();
        assert_eq!(config.database.driver, "mysql");
        assert_eq!(config.determinism.seed, 42);
    }

    #[test]
    fn test_invalid_yaml_returns_error() {
        let invalid_yaml = "invalid: yaml: content: [";
        let result = ConfigLoader::load(ConfigSource::Yaml(invalid_yaml.to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validation() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_determinism_config_default() {
        let config = DeterminismConfig::default();
        assert_eq!(config.seed, 42);
        assert!(!config.strict_mode);
    }

    #[test]
    fn test_output_format_variants() {
        let text = OutputFormat::Text;
        let json = OutputFormat::Json;

        // Just verify they exist and can be pattern matched
        match text {
            OutputFormat::Text => {},
            _ => panic!("Expected Text variant"),
        }

        match json {
            OutputFormat::Json => {},
            _ => panic!("Expected Json variant"),
        }
    }

    #[test]
    fn test_pool_config_default() {
        let pool = PoolConfig::default();
        assert_eq!(pool.min_size, 10);
        assert_eq!(pool.max_size, 100);
        assert_eq!(pool.connection_timeout, Duration::from_secs(10));
        assert_eq!(pool.idle_timeout, Duration::from_secs(300));
    }

    #[test]
    fn test_runtime_mode_async() {
        let mode = RuntimeMode::Async {
            workers: 4,
            max_connections: 10,
            backpressure_threshold: 0.8,
        };

        match mode {
            RuntimeMode::Async { workers, max_connections, backpressure_threshold } => {
                assert_eq!(workers, 4);
                assert_eq!(max_connections, 10);
                assert!((backpressure_threshold - 0.8).abs() < f64::EPSILON);
            }
            _ => panic!("Expected Async mode"),
        }
    }

    #[test]
    fn test_runtime_mode_blocking() {
        let mode = RuntimeMode::Blocking { threads: 8 };

        match mode {
            RuntimeMode::Blocking { threads } => {
                assert_eq!(threads, 8);
            }
            _ => panic!("Expected Blocking mode"),
        }
    }

    #[test]
    fn test_executor_config_constant_rate() {
        let executor = ExecutorConfig::ConstantRate {
            rate: 1000,
            duration: Duration::from_secs(60),
            max_connections: 50,
        };

        match executor {
            ExecutorConfig::ConstantRate { rate, duration, max_connections } => {
                assert_eq!(rate, 1000);
                assert_eq!(duration, Duration::from_secs(60));
                assert_eq!(max_connections, 50);
            }
            _ => panic!("Expected ConstantRate"),
        }
    }

    #[test]
    fn test_executor_config_ramping_rate() {
        let stages = vec![
            RateStage {
                duration: Duration::from_secs(30),
                target_rate: 500,
            },
            RateStage {
                duration: Duration::from_secs(30),
                target_rate: 1000,
            },
        ];

        let executor = ExecutorConfig::RampingRate {
            stages: stages.clone(),
            prealloc_connections: 10,
            max_connections: 50,
        };

        match executor {
            ExecutorConfig::RampingRate { stages: s, .. } => {
                assert_eq!(s.len(), 2);
                assert_eq!(s[0].target_rate, 500);
                assert_eq!(s[1].target_rate, 1000);
            }
            _ => panic!("Expected RampingRate"),
        }
    }

    #[test]
    fn test_load_default_config_file() {
        // Test that the default config file in the repo can be loaded
        let yaml = std::fs::read_to_string("rsbench.default.yaml");
        if yaml.is_err() {
            // Skip test if file doesn't exist (e.g., in some test environments)
            return;
        }

        let result = ConfigLoader::load(ConfigSource::Yaml(yaml.unwrap()));
        assert!(result.is_ok(), "Default config file should parse correctly");

        let config = result.unwrap();
        assert_eq!(config.database.driver, "mysql");
        assert_eq!(config.determinism.seed, 42);
    }

    #[test]
    fn test_load_infrastructure_config() {
        // Test loading infrastructure config from config/ directory
        let yaml = std::fs::read_to_string("config/rsbench.config.yaml");
        if yaml.is_err() {
            // Skip test if file doesn't exist
            return;
        }

        let result = ConfigLoader::load_infrastructure(ConfigSource::Yaml(yaml.unwrap()));
        assert!(result.is_ok(), "Infrastructure config should parse correctly");

        let config = result.unwrap();
        assert_eq!(config.database.driver, "mysql");
        assert!(matches!(config.runtime.mode, RuntimeMode::Async { .. }));
    }

    #[test]
    fn test_load_scenario_file() {
        // Test loading scenario file from scenarios/ directory
        let yaml = std::fs::read_to_string("scenarios/smoke_test.yaml");
        if yaml.is_err() {
            // Skip test if file doesn't exist
            return;
        }

        let result = ConfigLoader::load_scenario_file(ConfigSource::Yaml(yaml.unwrap()));
        assert!(result.is_ok(), "Scenario file should parse correctly");

        let scenario_file = result.unwrap();
        assert!(matches!(
            scenario_file.scenario.executor,
            ExecutorConfig::ConstantRate { .. }
        ));
        // Smoke test now uses declarative workload format
        assert!(matches!(
            scenario_file.scenario.workload,
            WorkloadConfig::Declarative { .. }
        ));
    }

    #[test]
    fn test_merge_infrastructure_and_scenario() {
        // Test merging infrastructure and scenario into complete config
        let infra_yaml = std::fs::read_to_string("config/rsbench.config.yaml");
        let scenario_yaml = std::fs::read_to_string("scenarios/smoke_test.yaml");

        if infra_yaml.is_err() || scenario_yaml.is_err() {
            // Skip test if files don't exist
            return;
        }

        let infra = ConfigLoader::load_infrastructure(
            ConfigSource::Yaml(infra_yaml.unwrap())
        ).unwrap();

        let scenario_file = ConfigLoader::load_scenario_file(
            ConfigSource::Yaml(scenario_yaml.unwrap())
        ).unwrap();

        let merged = ConfigLoader::merge_infrastructure_and_scenario(infra, scenario_file);

        assert_eq!(merged.database.driver, "mysql");
        assert!(matches!(merged.runtime.mode, RuntimeMode::Async { .. }));
        assert!(matches!(
            merged.scenario.executor,
            ExecutorConfig::ConstantRate { .. }
        ));
        assert_eq!(merged.determinism.seed, 42);
    }

    #[test]
    fn test_all_scenario_files_parse() {
        // Test that all scenario files in scenarios/ directory parse correctly
        let scenario_files = [
            "scenarios/smoke_test.yaml",
            "scenarios/oltp_read_write.yaml",
            "scenarios/high_throughput.yaml",
            "scenarios/capacity_test.yaml",
        ];

        for file in &scenario_files {
            let yaml = std::fs::read_to_string(file);
            if yaml.is_err() {
                continue; // Skip if file doesn't exist
            }

            let result = ConfigLoader::load_scenario_file(ConfigSource::Yaml(yaml.unwrap()));
            assert!(
                result.is_ok(),
                "Scenario file {} should parse correctly: {:?}",
                file,
                result.err()
            );
        }
    }

    #[test]
    fn test_all_infrastructure_configs_parse() {
        // Test that all infrastructure configs in config/ directory parse correctly
        let config_files = [
            "config/rsbench.config.yaml",
            "config/rsbench.config.staging.yaml",
            "config/rsbench.config.prod.yaml",
        ];

        for file in &config_files {
            let yaml = std::fs::read_to_string(file);
            if yaml.is_err() {
                continue; // Skip if file doesn't exist
            }

            let result = ConfigLoader::load_infrastructure(ConfigSource::Yaml(yaml.unwrap()));
            assert!(
                result.is_ok(),
                "Infrastructure config {} should parse correctly: {:?}",
                file,
                result.err()
            );
        }
    }

    // ========================================================================
    // Validation Tests
    // ========================================================================

    #[test]
    fn test_validate_pool_min_greater_than_max() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"
  pool:
    min_size: 100
    max_size: 10

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("min_size"));
    }

    #[test]
    fn test_validate_pool_max_size_zero() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"
  pool:
    min_size: 0
    max_size: 0

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("max_size must be greater than 0"));
    }

    #[test]
    fn test_validate_empty_connection_string() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: ""

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("connection string cannot be empty"));
    }

    #[test]
    fn test_validate_invalid_connection_string_format() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "localhost"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid connection string format"));
    }

    #[test]
    fn test_validate_async_workers_zero() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 0
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("workers must be > 0"));
    }

    #[test]
    fn test_validate_backpressure_threshold_out_of_range() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 1.5

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("between 0.0 and 1.0"));
    }

    #[test]
    fn test_validate_runtime_max_connections_exceeds_pool() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"
  pool:
    max_size: 10

runtime:
  type: async
  workers: 4
  max_connections: 100
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot exceed pool max_size"));
    }

    #[test]
    fn test_validate_executor_rate_zero() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 0
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("rate must be > 0"));
    }

    #[test]
    fn test_validate_executor_duration_zero() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 0s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("duration must be > 0"));
    }

    #[test]
    fn test_validate_ramping_empty_stages() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: ramping-rate
    stages: []
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("at least one stage"));
    }

    #[test]
    fn test_validate_workload_empty_name() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: ""

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("name cannot be empty"));
    }

    #[test]
    fn test_validate_valid_config_passes() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let result = ConfigLoader::validate(&config);
        assert!(result.is_ok());
    }

    // ========================================================================
    // Merge Tests
    // ========================================================================

    #[test]
    fn test_merge_cli_overrides_database_url() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let cli_args = CliArgs {
            config_file: None,
            database_url: Some("mysql://newhost/newdb".to_string()),
            rate: None,
            duration: None,
            threads: None,
            output_format: None,
        };

        let merged = ConfigLoader::merge(config, cli_args);
        assert_eq!(merged.database.connection_string, "mysql://newhost/newdb");
    }

    #[test]
    fn test_merge_cli_overrides_rate() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let cli_args = CliArgs {
            config_file: None,
            database_url: None,
            rate: Some(5000),
            duration: None,
            threads: None,
            output_format: None,
        };

        let merged = ConfigLoader::merge(config, cli_args);
        match merged.scenario.executor {
            ExecutorConfig::ConstantRate { rate, .. } => {
                assert_eq!(rate, 5000);
            }
            _ => panic!("Expected ConstantRate executor"),
        }
    }

    #[test]
    fn test_merge_cli_overrides_duration() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let cli_args = CliArgs {
            config_file: None,
            database_url: None,
            rate: None,
            duration: Some(Duration::from_secs(120)),
            threads: None,
            output_format: None,
        };

        let merged = ConfigLoader::merge(config, cli_args);
        match merged.scenario.executor {
            ExecutorConfig::ConstantRate { duration, .. } => {
                assert_eq!(duration, Duration::from_secs(120));
            }
            _ => panic!("Expected ConstantRate executor"),
        }
    }

    #[test]
    fn test_merge_cli_overrides_threads() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: blocking
  threads: 8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let cli_args = CliArgs {
            config_file: None,
            database_url: None,
            rate: None,
            duration: None,
            threads: Some(16),
            output_format: None,
        };

        let merged = ConfigLoader::merge(config, cli_args);
        match merged.runtime.mode {
            RuntimeMode::Blocking { threads } => {
                assert_eq!(threads, 16);
            }
            _ => panic!("Expected Blocking runtime"),
        }
    }

    #[test]
    fn test_merge_cli_overrides_output_format() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let cli_args = CliArgs {
            config_file: None,
            database_url: None,
            rate: None,
            duration: None,
            threads: None,
            output_format: Some(OutputFormat::Json),
        };

        let merged = ConfigLoader::merge(config, cli_args);
        assert!(matches!(merged.output.format, OutputFormat::Json));
    }

    #[test]
    fn test_merge_multiple_cli_overrides() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let cli_args = CliArgs {
            config_file: None,
            database_url: Some("mysql://cli/db".to_string()),
            rate: Some(2000),
            duration: Some(Duration::from_secs(30)),
            threads: None,
            output_format: Some(OutputFormat::Json),
        };

        let merged = ConfigLoader::merge(config, cli_args);
        assert_eq!(merged.database.connection_string, "mysql://cli/db");
        assert!(matches!(merged.output.format, OutputFormat::Json));
        match merged.scenario.executor {
            ExecutorConfig::ConstantRate { rate, duration, .. } => {
                assert_eq!(rate, 2000);
                assert_eq!(duration, Duration::from_secs(30));
            }
            _ => panic!("Expected ConstantRate executor"),
        }
    }

    #[test]
    fn test_merge_no_cli_overrides_preserves_config() {
        let yaml = r#"
database:
  driver: mysql
  connection_string: "mysql://localhost/test"

runtime:
  type: async
  workers: 4
  max_connections: 10
  backpressure_threshold: 0.8

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
    max_connections: 10
  workload:
    type: builtin
    name: oltp_read_write

output:
  format: text
"#;

        let config = ConfigLoader::load(ConfigSource::Yaml(yaml.to_string())).unwrap();
        let original_conn_str = config.database.connection_string.clone();

        let cli_args = CliArgs {
            config_file: None,
            database_url: None,
            rate: None,
            duration: None,
            threads: None,
            output_format: None,
        };

        let merged = ConfigLoader::merge(config, cli_args);
        assert_eq!(merged.database.connection_string, original_conn_str);
        assert!(matches!(merged.output.format, OutputFormat::Text));
    }
}
