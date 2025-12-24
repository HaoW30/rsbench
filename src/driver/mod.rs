//! Database driver module
//!
//! Abstracts database protocol details.

#[cfg(feature = "mysql")]
mod mysql;

#[cfg(feature = "mysql")]
pub use mysql::MySqlDriver;

use crate::{DatabaseError, Result, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Database driver trait
#[async_trait::async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// Driver name
    fn name(&self) -> &str;

    /// Create new connection
    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>>;

    /// Driver capabilities
    fn capabilities(&self) -> DriverCapabilities;
}

/// Connection trait
#[async_trait::async_trait]
pub trait Connection: Send + Sync {
    /// Execute query
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult>;

    /// Begin transaction (M0: basic support)
    async fn begin(&mut self) -> Result<()>;

    /// Commit transaction
    async fn commit(&mut self) -> Result<()>;

    /// Rollback transaction
    async fn rollback(&mut self) -> Result<()>;

    /// Health check
    async fn ping(&mut self) -> Result<()>;
}

/// Query execution result
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub rows_affected: u64,
    pub last_insert_id: Option<u64>,
}

/// Driver capabilities
#[derive(Debug, Clone)]
pub struct DriverCapabilities {
    pub supports_transactions: bool,
    pub supports_prepared_statements: bool,
}

/// Connection configuration
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub connection_string: String,
    pub timeout: Duration,
}

/// Driver registry
pub struct DriverRegistry {
    drivers: HashMap<String, Arc<dyn DatabaseDriver>>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            drivers: HashMap::new(),
        };

        // Register built-in drivers
        #[cfg(feature = "mysql")]
        registry.register(Arc::new(MySqlDriver::new()));

        registry
    }

    pub fn register(&mut self, driver: Arc<dyn DatabaseDriver>) {
        self.drivers.insert(driver.name().to_string(), driver);
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn DatabaseDriver>> {
        self.drivers
            .get(name)
            .cloned()
            .ok_or_else(|| crate::Error::Database(DatabaseError::DriverNotFound(name.to_string())))
    }
}

impl Default for DriverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_result_creation() {
        let result = QueryResult {
            rows_affected: 5,
            last_insert_id: Some(42),
        };

        assert_eq!(result.rows_affected, 5);
        assert_eq!(result.last_insert_id, Some(42));
    }

    #[test]
    fn test_query_result_no_insert_id() {
        let result = QueryResult {
            rows_affected: 3,
            last_insert_id: None,
        };

        assert_eq!(result.rows_affected, 3);
        assert!(result.last_insert_id.is_none());
    }

    #[test]
    fn test_driver_capabilities() {
        let caps = DriverCapabilities {
            supports_transactions: true,
            supports_prepared_statements: true,
        };

        assert!(caps.supports_transactions);
        assert!(caps.supports_prepared_statements);
    }

    #[test]
    fn test_connection_config() {
        let config = ConnectionConfig {
            connection_string: "mysql://localhost/test".to_string(),
            timeout: Duration::from_secs(5),
        };

        assert_eq!(config.connection_string, "mysql://localhost/test");
        assert_eq!(config.timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_driver_registry_default() {
        let registry = DriverRegistry::default();

        // Should have mysql driver if feature is enabled
        #[cfg(feature = "mysql")]
        {
            let driver = registry.get("mysql");
            assert!(driver.is_ok());
        }
    }

    #[test]
    fn test_driver_registry_get_missing() {
        let registry = DriverRegistry::new();
        let result = registry.get("nonexistent");

        assert!(result.is_err());
        match result {
            Err(crate::Error::Database(DatabaseError::DriverNotFound(name))) => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Expected DriverNotFound error"),
        }
    }

    #[test]
    #[cfg(feature = "mysql")]
    fn test_driver_registry_mysql() {
        let registry = DriverRegistry::new();
        let driver = registry.get("mysql").unwrap();

        assert_eq!(driver.name(), "mysql");

        let caps = driver.capabilities();
        assert!(caps.supports_transactions);
        assert!(caps.supports_prepared_statements);
    }
}
