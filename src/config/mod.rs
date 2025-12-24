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
    /// Load configuration from various sources
    pub fn load(source: ConfigSource) -> Result<ToolConfig> {
        match source {
            ConfigSource::File(path) => Self::load_from_file(&path),
            ConfigSource::Yaml(content) => Self::load_from_yaml(&content),
            ConfigSource::CliArgs(args) => Self::load_from_cli(args),
        }
    }

    /// Validate configuration consistency
    pub fn validate(_config: &ToolConfig) -> Result<()> {
        // TODO: Implement validation logic
        // - Check pool config constraints
        // - Validate connection string format
        // - Verify rate > 0
        // - Check duration > 0
        Ok(())
    }

    /// Merge CLI args with file config (precedence: CLI > file > defaults)
    pub fn merge(file_config: ToolConfig, _cli_args: CliArgs) -> ToolConfig {
        // TODO: Implement merge logic
        file_config
    }

    /// Apply default values to incomplete config
    pub fn with_defaults(config: ToolConfig) -> ToolConfig {
        // TODO: Apply defaults
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

/// Root configuration structure (M0 simplified)
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

/// Workload configuration (M0: builtin or Lua)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WorkloadConfig {
    /// Built-in OLTP workload
    Builtin {
        name: String, // "oltp_read_write"
        #[serde(default = "default_table_count")]
        table_count: usize,
        #[serde(default = "default_table_size")]
        table_size: usize,
    },
    /// Lua script (sysbench compatible)
    Lua { script: PathBuf },
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
}
