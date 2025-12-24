//! Test configuration builders and utilities

use rsbench::config::*;
use std::time::Duration;

/// Builder for creating test configurations easily
pub struct TestConfigBuilder {
    config: ToolConfig,
}

impl TestConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: Self::default_config(),
        }
    }

    fn default_config() -> ToolConfig {
        ToolConfig {
            database: DatabaseConfig {
                driver: "mock".to_string(),
                connection_string: "mock://localhost/testdb".to_string(),
                pool: PoolConfig {
                    min_size: 1,
                    max_size: 10,
                    connection_timeout: Duration::from_secs(5),
                    idle_timeout: Duration::from_secs(60),
                },
            },
            runtime: RuntimeConfig {
                mode: RuntimeMode::Async {
                    workers: 1,
                    max_connections: 10,
                    backpressure_threshold: 0.8,
                },
            },
            scenario: ScenarioConfig {
                executor: ExecutorConfig::ConstantRate {
                    rate: 100,
                    duration: Duration::from_secs(1),
                    max_connections: 10,
                },
                workload: WorkloadConfig::Builtin {
                    name: "oltp_read_write".to_string(),
                    table_count: 1,
                    table_size: 100,
                },
            },
            determinism: DeterminismConfig {
                seed: 42,
                strict_mode: false,
            },
            output: OutputConfig {
                format: OutputFormat::Json,
                file: None,
            },
        }
    }

    pub fn with_driver(mut self, driver: &str) -> Self {
        self.config.database.driver = driver.to_string();
        self
    }

    pub fn with_connection_string(mut self, conn_str: &str) -> Self {
        self.config.database.connection_string = conn_str.to_string();
        self
    }

    pub fn with_pool_size(mut self, min: usize, max: usize) -> Self {
        self.config.database.pool.min_size = min;
        self.config.database.pool.max_size = max;
        self
    }

    pub fn with_async_runtime(mut self, workers: usize, max_connections: usize) -> Self {
        self.config.runtime.mode = RuntimeMode::Async {
            workers,
            max_connections,
            backpressure_threshold: 0.8,
        };
        self
    }

    pub fn with_blocking_runtime(mut self, threads: usize) -> Self {
        self.config.runtime.mode = RuntimeMode::Blocking { threads };
        self
    }

    pub fn with_constant_rate(mut self, rate: u64, duration: Duration) -> Self {
        if let ScenarioConfig { ref mut executor, .. } = self.config.scenario {
            *executor = ExecutorConfig::ConstantRate {
                rate,
                duration,
                max_connections: 10,
            };
        }
        self
    }

    pub fn with_ramping_rate(mut self, stages: Vec<RateStage>) -> Self {
        if let ScenarioConfig { ref mut executor, .. } = self.config.scenario {
            *executor = ExecutorConfig::RampingRate {
                stages,
                prealloc_connections: 10,
                max_connections: 50,
            };
        }
        self
    }

    pub fn with_workload(mut self, workload: WorkloadConfig) -> Self {
        self.config.scenario.workload = workload;
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.config.determinism.seed = seed;
        self
    }

    pub fn with_strict_determinism(mut self) -> Self {
        self.config.determinism.strict_mode = true;
        self
    }

    pub fn with_output_format(mut self, format: OutputFormat) -> Self {
        self.config.output.format = format;
        self
    }

    pub fn build(self) -> ToolConfig {
        self.config
    }
}

impl Default for TestConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TestConfigBuilder::new().build();
        assert_eq!(config.database.driver, "mock");
        assert_eq!(config.determinism.seed, 42);
    }

    #[test]
    fn test_custom_config() {
        let config = TestConfigBuilder::new()
            .with_driver("mysql")
            .with_seed(12345)
            .with_constant_rate(1000, Duration::from_secs(60))
            .build();

        assert_eq!(config.database.driver, "mysql");
        assert_eq!(config.determinism.seed, 12345);

        if let ExecutorConfig::ConstantRate { rate, duration, .. } = config.scenario.executor {
            assert_eq!(rate, 1000);
            assert_eq!(duration, Duration::from_secs(60));
        } else {
            panic!("Expected ConstantRate executor");
        }
    }

    #[test]
    fn test_ramping_rate_config() {
        let stages = vec![
            RateStage {
                duration: Duration::from_secs(30),
                target_rate: 500,
            },
            RateStage {
                duration: Duration::from_secs(60),
                target_rate: 1000,
            },
        ];

        let config = TestConfigBuilder::new()
            .with_ramping_rate(stages.clone())
            .build();

        if let ExecutorConfig::RampingRate { stages: s, .. } = config.scenario.executor {
            assert_eq!(s.len(), 2);
            assert_eq!(s[0].target_rate, 500);
            assert_eq!(s[1].target_rate, 1000);
        } else {
            panic!("Expected RampingRate executor");
        }
    }
}
