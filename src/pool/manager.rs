//! Connection pool manager for deadpool integration
//!
//! This module provides the adapter between deadpool's generic pooling
//! and our DatabaseDriver trait.

use crate::driver::{Connection, ConnectionConfig, DatabaseDriver};
use crate::{Error, Result};
use deadpool::managed::{Manager, RecycleError, RecycleResult};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Deadpool manager that adapts our DatabaseDriver trait
///
/// This manager implements deadpool's `Manager` trait, allowing us to use
/// deadpool's connection pooling with any of our database drivers (MySQL, PostgreSQL, etc.).
///
/// # Responsibilities
///
/// - **Create connections** - Delegates to `DatabaseDriver::connect()`
/// - **Health check** - Pings connections before reuse to ensure they're healthy
/// - **Lifecycle** - Manages connection creation and validation
///
/// # Example
///
/// ```no_run
/// use rsbench::pool::manager::DriverManager;
/// # use std::sync::Arc;
/// # use std::time::Duration;
///
/// # async fn example() -> rsbench::Result<()> {
/// # let driver = todo!();
/// let manager = DriverManager::new(
///     driver,
///     "mysql://localhost/test".to_string(),
///     Duration::from_secs(5),
/// );
///
/// // Used internally by deadpool to create connections
/// let conn = manager.create().await?;
/// # Ok(())
/// # }
/// ```
pub struct DriverManager {
    /// Database driver for creating connections
    driver: Arc<dyn DatabaseDriver>,

    /// Connection string for the database
    connection_string: String,

    /// Timeout for connection creation
    timeout: Duration,
}

impl DriverManager {
    /// Create a new DriverManager
    ///
    /// # Arguments
    ///
    /// * `driver` - Database driver to use for creating connections
    /// * `connection_string` - Database connection string (e.g., "mysql://localhost/db")
    /// * `timeout` - Maximum time to wait for connection creation
    ///
    /// # Returns
    ///
    /// A new `DriverManager` ready to be used with deadpool
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rsbench::pool::manager::DriverManager;
    /// use rsbench::driver::get_driver;
    /// use std::sync::Arc;
    /// use std::time::Duration;
    ///
    /// # fn example() -> rsbench::Result<()> {
    /// let driver = Arc::new(get_driver("mysql")?);
    /// let manager = DriverManager::new(
    ///     driver,
    ///     "mysql://root@localhost/benchdb".to_string(),
    ///     Duration::from_secs(5),
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(
        driver: Arc<dyn DatabaseDriver>,
        connection_string: String,
        timeout: Duration,
    ) -> Self {
        Self {
            driver,
            connection_string,
            timeout,
        }
    }
}

impl Manager for DriverManager {
    /// The type of connections this manager creates
    ///
    /// We use `Box<dyn Connection + Send>` to allow any database driver's connection type
    type Type = Box<dyn Connection + Send>;

    /// The error type for connection operations
    type Error = Error;

    /// Create a new database connection
    ///
    /// This is called by deadpool when:
    /// - The pool needs to create a new connection (pool growing)
    /// - An unhealthy connection needs to be replaced
    ///
    /// # Returns
    ///
    /// - `Ok(conn)` - A new database connection ready to use
    /// - `Err(e)` - Connection creation failed (network, auth, etc.)
    ///
    /// # Performance
    ///
    /// This is the expensive operation (~10-50ms for MySQL/PostgreSQL).
    /// The pool amortizes this cost by reusing connections.
    async fn create(&self) -> Result<Self::Type> {
        let config = ConnectionConfig {
            connection_string: self.connection_string.clone(),
            timeout: self.timeout,
        };

        self.driver.connect(&config).await
    }

    /// Check if a connection is healthy and can be reused
    ///
    /// Called by deadpool before returning a connection from the pool.
    /// This ensures we never hand out a broken connection.
    ///
    /// # Arguments
    ///
    /// * `conn` - The connection to check
    ///
    /// # Returns
    ///
    /// - `Ok(())` - Connection is healthy, safe to reuse
    /// - `Err(RecycleError::Backend(e))` - Connection is broken, will be discarded
    ///
    /// # Health Check Strategy
    ///
    /// We use `Connection::ping()` which:
    /// - Sends a lightweight query to the database
    /// - Verifies the connection is still alive
    /// - Typically takes <1ms
    ///
    /// # Performance
    ///
    /// This adds ~1ms overhead per checkout, but prevents:
    /// - Returning broken connections to callers
    /// - Query failures due to stale connections
    /// - Network errors mid-transaction
    ///
    /// The tradeoff is worth it for reliability.
    async fn recycle(
        &self,
        conn: &mut Self::Type,
        _metrics: &deadpool::managed::Metrics,
    ) -> RecycleResult<Self::Error> {
        // Health check: verify connection is still alive
        let ping_result = conn.ping().await;

        match ping_result {
            Ok(_) => {
                debug!("Connection health check passed, returning to pool");
                Ok(())
            }
            Err(e) => {
                // Health check failed - connection is broken, will be discarded
                static PING_FAIL_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                let fail_num = PING_FAIL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;

                eprintln!("[Pool] ⚠️  Health check FAILED #{}: {}", fail_num, e);

                warn!(
                    error = %e,
                    failure_count = fail_num,
                    "Connection health check failed, discarding connection"
                );
                Err(RecycleError::Backend(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::driver::{DriverCapabilities, QueryResult};

    // Mock connection for testing
    struct MockConnection {
        healthy: bool,
    }

    #[async_trait]
    impl Connection for MockConnection {
        async fn execute(
            &mut self,
            _sql: &str,
            _params: &[crate::Value],
        ) -> Result<QueryResult> {
            Ok(QueryResult {
                rows_affected: 1,
                last_insert_id: None,
            })
        }

        async fn begin(&mut self) -> Result<()> {
            Ok(())
        }

        async fn commit(&mut self) -> Result<()> {
            Ok(())
        }

        async fn rollback(&mut self) -> Result<()> {
            Ok(())
        }

        async fn ping(&mut self) -> Result<()> {
            if self.healthy {
                Ok(())
            } else {
                Err(Error::Database(crate::DatabaseError::Connection(
                    "Connection lost".to_string(),
                )))
            }
        }
    }

    // Mock driver for testing
    struct MockDriver {
        should_fail: bool,
    }

    #[async_trait]
    impl DatabaseDriver for MockDriver {
        fn name(&self) -> &str {
            "mock"
        }

        async fn connect(&self, _config: &ConnectionConfig) -> Result<Box<dyn Connection + Send>> {
            if self.should_fail {
                Err(Error::Database(crate::DatabaseError::Connection(
                    "Connection failed".to_string(),
                )))
            } else {
                Ok(Box::new(MockConnection { healthy: true }))
            }
        }

        fn capabilities(&self) -> DriverCapabilities {
            DriverCapabilities {
                supports_transactions: true,
                supports_prepared_statements: true,
            }
        }
    }

    #[tokio::test]
    async fn test_manager_create_success() {
        let driver = Arc::new(MockDriver { should_fail: false });
        let manager = DriverManager::new(
            driver,
            "mock://localhost".to_string(),
            Duration::from_secs(5),
        );

        let conn = manager.create().await;
        assert!(conn.is_ok());
    }

    #[tokio::test]
    async fn test_manager_create_failure() {
        let driver = Arc::new(MockDriver { should_fail: true });
        let manager = DriverManager::new(
            driver,
            "mock://localhost".to_string(),
            Duration::from_secs(5),
        );

        let conn = manager.create().await;
        assert!(conn.is_err());
    }

    #[tokio::test]
    async fn test_manager_recycle_healthy() {
        let driver = Arc::new(MockDriver { should_fail: false });
        let manager = DriverManager::new(
            driver,
            "mock://localhost".to_string(),
            Duration::from_secs(5),
        );

        let mut conn = Box::new(MockConnection { healthy: true }) as Box<dyn Connection + Send>;
        let metrics = deadpool::managed::Metrics::default();

        let result = manager.recycle(&mut conn, &metrics).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_manager_recycle_unhealthy() {
        let driver = Arc::new(MockDriver { should_fail: false });
        let manager = DriverManager::new(
            driver,
            "mock://localhost".to_string(),
            Duration::from_secs(5),
        );

        let mut conn = Box::new(MockConnection { healthy: false }) as Box<dyn Connection + Send>;
        let metrics = deadpool::managed::Metrics::default();

        let result = manager.recycle(&mut conn, &metrics).await;
        assert!(result.is_err());

        // Verify it's a Backend error (connection will be discarded)
        match result {
            Err(RecycleError::Backend(_)) => {}
            _ => panic!("Expected RecycleError::Backend"),
        }
    }
}
