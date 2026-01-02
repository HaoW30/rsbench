//! Database driver module
//!
//! Provides a unified interface for database connectivity with runtime driver selection.
//!
//! # Overview
//!
//! The driver module abstracts database-specific protocol details behind a common trait-based
//! interface, allowing RSBench to work with multiple database backends (MySQL, PostgreSQL)
//! from a single binary.
//!
//! # Architecture
//!
//! - **DatabaseDriver trait**: Defines how to create connections and query capabilities
//! - **Connection trait**: Defines how to execute queries and manage transactions
//! - **DriverRegistry**: Provides runtime driver selection by name
//!
//! # Key Design Principles
//!
//! 1. **No Double-Pooling**: Drivers create direct connections; pooling is handled by the
//!    connection pool module
//! 2. **Runtime Selection**: Both drivers compiled into single binary, selected at runtime
//! 3. **Async-First**: All I/O operations are non-blocking
//!
//! # Example
//!
//! ```rust,no_run
//! use rsbench::driver::{DriverRegistry, ConnectionConfig};
//! use rsbench::Value;
//! use std::time::Duration;
//!
//! # async fn example() -> rsbench::Result<()> {
//! // Create registry (drivers auto-registered)
//! let registry = DriverRegistry::new();
//!
//! // Select driver at runtime
//! let driver = registry.get("mysql")?;
//!
//! // Create connection
//! let config = ConnectionConfig {
//!     connection_string: "mysql://root@localhost:3306/testdb".into(),
//!     timeout: Duration::from_secs(5),
//! };
//!
//! let mut conn = driver.connect(&config).await?;
//!
//! // Execute query
//! let result = conn.execute(
//!     "INSERT INTO users (id, name) VALUES (?, ?)",
//!     &[Value::Int(1), Value::String("Alice".into())]
//! ).await?;
//!
//! println!("Rows affected: {}", result.rows_affected);
//! # Ok(())
//! # }
//! ```
//!
//! # Supported Databases
//!
//! - **MySQL** (feature: `mysql`) - Uses `mysql_async` crate
//! - **PostgreSQL** (feature: `postgres`) - Uses `tokio-postgres` crate
//!
//! Both features are enabled by default.

#[cfg(feature = "mysql")]
mod mysql;

#[cfg(feature = "mysql")]
pub use mysql::MySqlDriver;

#[cfg(feature = "postgres")]
mod postgres;

#[cfg(feature = "postgres")]
pub use postgres::PostgresDriver;

use crate::{DatabaseError, Result, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Database driver trait defining how to create connections and query capabilities.
///
///# Implementation Note
///
/// Drivers should create **direct connections**, not internal connection pools.
/// Connection pooling is handled by the [`ConnectionPool`](crate::pool::ConnectionPool) module
/// to avoid double-pooling.
///
/// # Example
///
/// ```rust,no_run
/// use rsbench::driver::{DatabaseDriver, ConnectionConfig, DriverCapabilities};
/// use std::time::Duration;
///
/// # async fn example() -> rsbench::Result<()> {
/// let driver = rsbench::driver::MySqlDriver::new();
///
/// assert_eq!(driver.name(), "mysql");
///
/// let caps = driver.capabilities();
/// assert!(caps.supports_transactions);
///
/// let config = ConnectionConfig {
///     connection_string: "mysql://root@localhost/test".into(),
///     timeout: Duration::from_secs(5),
/// };
///
/// let mut conn = driver.connect(&config).await?;
/// # Ok(())
/// # }
/// ```
#[async_trait::async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// Returns the driver's unique identifier (e.g., "mysql", "postgres").
    ///
    /// Used by [`DriverRegistry`] for runtime driver selection.
    fn name(&self) -> &str;

    /// Creates a new database connection.
    ///
    /// This method should create a **direct connection** to the database, not a connection pool.
    /// Connections will be managed by the connection pool module.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseError::Connection`] if the connection fails (invalid credentials,
    /// network error, timeout, etc.).
    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection + Send>>;

    /// Returns the driver's capabilities (transaction support, prepared statements, etc.).
    fn capabilities(&self) -> DriverCapabilities;
}

/// Database connection trait defining query execution and transaction management.
///
/// Connections are **mutable** because they maintain internal state (active transaction,
/// cursor position, etc.). Use one connection per task/worker.
///
/// # Example
///
/// ```rust,no_run
/// use rsbench::driver::{Connection, DriverRegistry};
/// use rsbench::Value;
///
/// # async fn example() -> rsbench::Result<()> {
/// let registry = DriverRegistry::new();
/// let driver = registry.get("mysql")?;
/// let mut conn = driver.connect(&config).await?;
///
/// // Execute queries
/// let result = conn.execute(
///     "INSERT INTO users VALUES (?, ?)",
///     &[Value::Int(1), Value::String("Alice".into())]
/// ).await?;
///
/// // Transactions
/// conn.begin().await?;
/// conn.execute("UPDATE accounts SET balance = balance - 100 WHERE id = 1", &[]).await?;
/// conn.execute("UPDATE accounts SET balance = balance + 100 WHERE id = 2", &[]).await?;
/// conn.commit().await?;
/// # Ok(())
/// # }
/// ```
#[async_trait::async_trait]
pub trait Connection: Send + Sync {
    /// Executes a SQL query with parameter binding.
    ///
    /// Parameters are passed using database-specific placeholders:
    /// - MySQL: `?` (e.g., `"SELECT * FROM users WHERE id = ?"`)
    /// - PostgreSQL: `$1, $2, $3` (e.g., `"SELECT * FROM users WHERE id = $1"`)
    ///
    /// # Parameters
    ///
    /// - `sql`: SQL query string with placeholders
    /// - `params`: Parameter values in order of appearance
    ///
    /// # Returns
    ///
    /// [`QueryResult`] containing rows affected and last insert ID (MySQL only).
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseError::Query`] for syntax errors, constraint violations, etc.
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult>;

    /// Begins a new transaction.
    ///
    /// Subsequent queries will be part of the transaction until [`commit`](Self::commit)
    /// or [`rollback`](Self::rollback) is called.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseError::Transaction`] if a transaction is already active or
    /// database error occurs.
    async fn begin(&mut self) -> Result<()>;

    /// Commits the active transaction.
    ///
    /// All changes made since [`begin`](Self::begin) become permanent.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseError::Transaction`] if no transaction is active or commit fails.
    async fn commit(&mut self) -> Result<()>;

    /// Rolls back the active transaction.
    ///
    /// All changes made since [`begin`](Self::begin) are discarded.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseError::Transaction`] if no transaction is active or rollback fails.
    async fn rollback(&mut self) -> Result<()>;

    /// Health check - verifies the connection is still alive.
    ///
    /// Useful for connection pool health monitoring.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseError::Connection`] if the connection is broken.
    async fn ping(&mut self) -> Result<()>;
}

/// Result of a query execution.
///
/// Contains metadata about the query execution (rows affected, insert ID).
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Number of rows affected by the query (INSERT, UPDATE, DELETE).
    pub rows_affected: u64,

    /// Last auto-increment ID generated by INSERT (MySQL only).
    ///
    /// PostgreSQL returns `None` - use `RETURNING` clause instead.
    pub last_insert_id: Option<u64>,
}

/// Driver capability flags.
///
/// Indicates which features the driver supports (transactions, prepared statements, etc.).
#[derive(Debug, Clone)]
pub struct DriverCapabilities {
    /// Whether the driver supports transactions (BEGIN, COMMIT, ROLLBACK).
    ///
    /// Both MySQL and PostgreSQL support transactions.
    pub supports_transactions: bool,

    /// Whether the driver supports prepared statements.
    ///
    /// Both MySQL and PostgreSQL support prepared statements.
    pub supports_prepared_statements: bool,
}

/// Configuration for creating a database connection.
///
/// # Connection String Formats
///
/// **MySQL**:
/// ```text
/// mysql://user:password@host:port/database
/// mysql://user:password@host:port/database?ssl-mode=required
/// ```
///
/// **PostgreSQL**:
/// ```text
/// postgresql://user:password@host:port/database
/// postgres://user:password@host:port/database?sslmode=require
/// ```
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Database connection string (database-specific format).
    pub connection_string: String,

    /// Connection timeout (how long to wait for connection establishment).
    pub timeout: Duration,
}

/// Registry for runtime driver selection.
///
/// The registry maintains a mapping of driver names to driver implementations,
/// allowing drivers to be selected at runtime by name.
///
/// # Example
///
/// ```rust,no_run
/// use rsbench::driver::DriverRegistry;
///
/// # fn example() -> rsbench::Result<()> {
/// // Create registry (drivers auto-registered)
/// let registry = DriverRegistry::new();
///
/// // Get driver by name
/// let mysql_driver = registry.get("mysql")?;
/// let postgres_driver = registry.get("postgres")?;
///
/// assert_eq!(mysql_driver.name(), "mysql");
/// assert_eq!(postgres_driver.name(), "postgres");
/// # Ok(())
/// # }
/// ```
///
/// # Built-in Drivers
///
/// The following drivers are automatically registered if their features are enabled:
/// - `"mysql"` - MySQL driver (feature: `mysql`)
/// - `"postgres"` - PostgreSQL driver (feature: `postgres`)
pub struct DriverRegistry {
    drivers: HashMap<String, Arc<dyn DatabaseDriver>>,
}

impl DriverRegistry {
    /// Creates a new registry with all built-in drivers automatically registered.
    ///
    /// Built-in drivers are registered based on enabled Cargo features:
    /// - MySQL driver (`"mysql"`) if `feature = "mysql"` is enabled
    /// - PostgreSQL driver (`"postgres"`) if `feature = "postgres"` is enabled
    ///
    /// Both features are enabled by default.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rsbench::driver::DriverRegistry;
    ///
    /// let registry = DriverRegistry::new();
    ///
    /// # #[cfg(feature = "mysql")]
    /// assert!(registry.get("mysql").is_ok());
    /// # #[cfg(feature = "postgres")]
    /// assert!(registry.get("postgres").is_ok());
    /// ```
    pub fn new() -> Self {
        let mut registry = Self {
            drivers: HashMap::new(),
        };

        // Register built-in drivers
        #[cfg(feature = "mysql")]
        registry.register(Arc::new(MySqlDriver::new()));

        #[cfg(feature = "postgres")]
        registry.register(Arc::new(PostgresDriver::new()));

        registry
    }

    /// Registers a custom driver.
    ///
    /// This method allows registering additional drivers beyond the built-in ones.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use rsbench::driver::DriverRegistry;
    /// use std::sync::Arc;
    ///
    /// let mut registry = DriverRegistry::new();
    /// registry.register(Arc::new(MyCustomDriver::new()));
    ///
    /// let driver = registry.get("custom")?;
    /// ```
    pub fn register(&mut self, driver: Arc<dyn DatabaseDriver>) {
        self.drivers.insert(driver.name().to_string(), driver);
    }

    /// Retrieves a driver by name.
    ///
    /// Returns an `Arc` clone of the driver, allowing it to be shared across threads.
    ///
    /// # Parameters
    ///
    /// - `name`: Driver identifier (e.g., "mysql", "postgres")
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseError::DriverNotFound`] if no driver with the given name is registered.
    ///
    /// # Example
    ///
    /// ```rust
    /// use rsbench::driver::DriverRegistry;
    ///
    /// # fn example() -> rsbench::Result<()> {
    /// let registry = DriverRegistry::new();
    ///
    /// # #[cfg(feature = "mysql")]
    /// let mysql = registry.get("mysql")?;
    /// # #[cfg(feature = "mysql")]
    /// assert_eq!(mysql.name(), "mysql");
    ///
    /// // Unknown driver returns error
    /// assert!(registry.get("unknown").is_err());
    /// # Ok(())
    /// # }
    /// ```
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

    #[test]
    #[cfg(feature = "postgres")]
    fn test_driver_registry_postgres() {
        let registry = DriverRegistry::new();
        let driver = registry.get("postgres").unwrap();

        assert_eq!(driver.name(), "postgres");

        let caps = driver.capabilities();
        assert!(caps.supports_transactions);
        assert!(caps.supports_prepared_statements);
    }

    // ========================================================================
    // Cross-Driver Tests
    // ========================================================================

    #[test]
    #[cfg(all(feature = "mysql", feature = "postgres"))]
    fn test_driver_registry_both_drivers() {
        let registry = DriverRegistry::new();

        // Verify both drivers are registered
        let mysql = registry.get("mysql");
        let postgres = registry.get("postgres");

        assert!(mysql.is_ok(), "MySQL driver should be registered");
        assert!(postgres.is_ok(), "PostgreSQL driver should be registered");

        // Verify they have correct names
        assert_eq!(mysql.unwrap().name(), "mysql");
        assert_eq!(postgres.unwrap().name(), "postgres");
    }

    #[test]
    #[cfg(all(feature = "mysql", feature = "postgres"))]
    fn test_driver_runtime_selection() {
        let registry = DriverRegistry::new();

        // Test runtime selection by name
        let driver_names = vec!["mysql", "postgres"];

        for name in driver_names {
            let driver = registry.get(name);
            assert!(driver.is_ok(), "Driver '{}' should be available", name);

            let driver = driver.unwrap();
            assert_eq!(driver.name(), name);

            // Verify capabilities
            let caps = driver.capabilities();
            assert!(caps.supports_transactions);
            assert!(caps.supports_prepared_statements);
        }
    }

    #[test]
    #[cfg(all(feature = "mysql", feature = "postgres"))]
    fn test_driver_registry_independence() {
        let registry = DriverRegistry::new();

        // Get same driver multiple times
        let mysql1 = registry.get("mysql").unwrap();
        let mysql2 = registry.get("mysql").unwrap();
        let postgres1 = registry.get("postgres").unwrap();
        let postgres2 = registry.get("postgres").unwrap();

        // Verify they are independent Arc clones
        assert_eq!(mysql1.name(), mysql2.name());
        assert_eq!(postgres1.name(), postgres2.name());
        assert_ne!(mysql1.name(), postgres1.name());
    }

    #[test]
    fn test_driver_registry_default_is_new() {
        let registry1 = DriverRegistry::default();
        let registry2 = DriverRegistry::new();

        // Both should have the same drivers registered
        #[cfg(feature = "mysql")]
        {
            assert!(registry1.get("mysql").is_ok());
            assert!(registry2.get("mysql").is_ok());
        }

        #[cfg(feature = "postgres")]
        {
            assert!(registry1.get("postgres").is_ok());
            assert!(registry2.get("postgres").is_ok());
        }
    }
}
