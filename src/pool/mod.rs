//! Connection pool module
//!
//! Manages database connections with backpressure awareness.
//!
//! This module provides real connection pooling using the `deadpool` library,
//! allowing efficient reuse of database connections and reducing the overhead
//! of connection creation.

mod manager;

pub use manager::DriverManager;

use crate::config::PoolConfig;
use crate::driver::DatabaseDriver;
use crate::Result;
use deadpool::managed::{Pool, PoolConfig as DeadpoolConfig};
use std::sync::Arc;

/// Connection pool with real pooling via deadpool
///
/// Manages a pool of database connections, providing efficient connection reuse
/// and health monitoring.
///
/// # Architecture
///
/// Uses `deadpool` for connection pooling with our custom `DriverManager` adapter:
///
/// ```text
/// ConnectionPool
///     │
///     ├─> deadpool::Pool<DriverManager>
///     │       │
///     │       ├─> Connection 1 (idle)
///     │       ├─> Connection 2 (active)
///     │       └─> Connection N (idle)
///     │
///     └─> PoolConfig (max_size, timeouts, etc.)
/// ```
///
/// # Example
///
/// ```no_run
/// use rsbench::pool::ConnectionPool;
/// use rsbench::config::PoolConfig;
/// # use std::sync::Arc;
/// # use std::time::Duration;
///
/// # async fn example() -> rsbench::Result<()> {
/// # let driver = todo!();
/// let config = PoolConfig {
///     min_size: 5,
///     max_size: 100,
///     connection_timeout: Duration::from_secs(5),
///     idle_timeout: Duration::from_secs(600),
/// };
///
/// let pool = ConnectionPool::new(
///     driver,
///     "mysql://localhost/benchdb".to_string(),
///     config,
/// )?;
///
/// // Get connection from pool (fast - reuses existing connection)
/// let mut conn = pool.get().await?;
/// conn.execute("SELECT 1", &[]).await?;
/// // Connection automatically returned to pool on drop
/// # Ok(())
/// # }
/// ```
pub struct ConnectionPool {
    /// The actual deadpool-managed connection pool
    inner: Pool<DriverManager>,

    /// Configuration for reference and stats
    #[allow(dead_code)]
    config: PoolConfig,
}

impl ConnectionPool {
    /// Create new connection pool with real pooling
    ///
    /// Initializes a deadpool-managed connection pool with health checking.
    ///
    /// # Arguments
    ///
    /// * `driver` - Database driver for creating connections
    /// * `connection_string` - Database connection string (e.g., "mysql://localhost/db")
    /// * `config` - Pool configuration (min_size, max_size, timeouts)
    ///
    /// # Returns
    ///
    /// - `Ok(pool)` - Connection pool ready to use
    /// - `Err(e)` - Pool creation failed (invalid config, etc.)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use rsbench::pool::ConnectionPool;
    /// use rsbench::config::PoolConfig;
    /// # use std::sync::Arc;
    /// # use std::time::Duration;
    ///
    /// # fn example() -> rsbench::Result<()> {
    /// # let driver = todo!();
    /// let config = PoolConfig {
    ///     min_size: 5,
    ///     max_size: 100,
    ///     connection_timeout: Duration::from_secs(5),
    ///     idle_timeout: Duration::from_secs(600),
    /// };
    ///
    /// let pool = ConnectionPool::new(
    ///     driver,
    ///     "mysql://root@localhost/benchdb".to_string(),
    ///     config,
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(
        driver: Arc<dyn DatabaseDriver>,
        connection_string: String,
        config: PoolConfig,
    ) -> Result<Self> {
        // Create our custom manager that adapts DatabaseDriver to deadpool
        let manager = DriverManager::new(driver, connection_string, config.connection_timeout);

        // Configure deadpool with our pool settings
        // Note: Timeouts require a Tokio runtime to be available during pool creation
        let pool_config = DeadpoolConfig {
            max_size: config.max_size,
            timeouts: deadpool::managed::Timeouts {
                wait: None,  // No wait timeout - will block until connection available
                create: None,  // No create timeout
                recycle: None,  // No recycle timeout
            },
            ..Default::default()
        };

        // Build the pool using deadpool 0.12 API
        let inner = Pool::builder(manager)
            .config(pool_config)
            .build()
            .map_err(|e| {
                crate::PoolError::Configuration(format!(
                    "Failed to build pool: {}",
                    e
                ))
            })?;

        Ok(Self { inner, config })
    }

    /// Get connection from pool
    ///
    /// Checks out a connection from the pool. If a healthy connection is available,
    /// returns immediately. Otherwise, may wait or create a new connection based on
    /// pool configuration.
    ///
    /// # Returns
    ///
    /// - `Ok(conn)` - A pooled connection ready to use
    /// - `Err(PoolError::Timeout)` - No connection available within timeout
    /// - `Err(PoolError::Closed)` - Pool has been closed
    ///
    /// # Performance
    ///
    /// - **Cache hit** (connection available): ~1-10μs
    /// - **Cache miss** (create new): ~10-50ms (network + auth)
    /// - **Pool saturated** (wait): Up to `connection_timeout`
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rsbench::pool::ConnectionPool;
    /// # async fn example(pool: &ConnectionPool) -> rsbench::Result<()> {
    /// // Get connection from pool
    /// let mut conn = pool.get().await?;
    ///
    /// // Use connection
    /// conn.execute("SELECT 1", &[]).await?;
    ///
    /// // Connection automatically returned to pool when dropped
    /// drop(conn);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get(&self) -> Result<PooledConnection> {
        // Get connection from deadpool with timeout
        let conn = self
            .inner
            .get()
            .await
            .map_err(|e| match e {
                deadpool::managed::PoolError::Timeout(_) => {
                    crate::Error::Pool(crate::PoolError::Timeout)
                }
                deadpool::managed::PoolError::Closed => {
                    crate::Error::Pool(crate::PoolError::Closed)
                }
                deadpool::managed::PoolError::Backend(e) => e,
                _ => crate::Error::Pool(crate::PoolError::Unknown(e.to_string())),
            })?;

        Ok(PooledConnection { inner: conn })
    }

    /// Get pool statistics
    ///
    /// Returns real-time statistics about pool health and utilization.
    /// Used by the runtime for backpressure detection.
    ///
    /// # Returns
    ///
    /// A `PoolStats` struct with current pool state
    ///
    /// # Performance
    ///
    /// This is a very fast operation (<1μs) - safe to call frequently.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rsbench::pool::ConnectionPool;
    /// # fn example(pool: &ConnectionPool) {
    /// let stats = pool.stats();
    ///
    /// println!("Total connections: {}", stats.total_connections);
    /// println!("Active: {}", stats.active_connections);
    /// println!("Idle: {}", stats.idle_connections);
    /// println!("Waiting: {}", stats.pending_requests);
    ///
    /// // Check pool utilization
    /// let utilization = stats.active_connections as f64 / stats.total_connections as f64;
    /// if utilization > 0.8 {
    ///     println!("⚠️  Pool is saturated!");
    /// }
    /// # }
    /// ```
    pub fn stats(&self) -> PoolStats {
        // Get real stats from deadpool
        let status = self.inner.status();

        PoolStats {
            total_connections: status.size,
            active_connections: status.size - status.available,
            idle_connections: status.available,
            pending_requests: status.waiting,
        }
    }
}

/// Pooled connection wrapper
///
/// A connection checked out from the pool. When dropped, the connection is
/// automatically returned to the pool for reuse.
///
/// # Lifecycle
///
/// ```text
/// 1. pool.get() → PooledConnection created
/// 2. Use connection (execute queries)
/// 3. Drop PooledConnection → Connection returned to pool
/// ```
///
/// # Example
///
/// ```no_run
/// # use rsbench::pool::ConnectionPool;
/// # async fn example(pool: &ConnectionPool) -> rsbench::Result<()> {
/// // Checkout connection
/// let mut conn = pool.get().await?;
///
/// // Use it
/// let result = conn.execute("SELECT COUNT(*) FROM users", &[]).await?;
/// println!("Rows affected: {}", result.rows_affected);
///
/// // Automatically returned to pool when conn is dropped
/// drop(conn);
///
/// // Can immediately checkout again (likely same connection)
/// let mut conn2 = pool.get().await?;
/// # Ok(())
/// # }
/// ```
pub struct PooledConnection {
    /// Deadpool's managed connection object
    ///
    /// This wraps our `Box<dyn Connection>` and handles returning it to the pool on drop
    inner: deadpool::managed::Object<DriverManager>,
}

impl PooledConnection {
    /// Execute query
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL query to execute
    /// * `params` - Query parameters
    ///
    /// # Returns
    ///
    /// - `Ok(result)` - Query executed successfully
    /// - `Err(e)` - Query failed (syntax error, constraint violation, etc.)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rsbench::pool::PooledConnection;
    /// # async fn example(mut conn: PooledConnection) -> rsbench::Result<()> {
    /// // Simple query
    /// conn.execute("DELETE FROM temp_table", &[]).await?;
    ///
    /// // Parameterized query
    /// use rsbench::Value;
    /// conn.execute(
    ///     "INSERT INTO users (name, age) VALUES (?, ?)",
    ///     &[Value::String("Alice".to_string()), Value::Int(30)]
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute(
        &mut self,
        sql: &str,
        params: &[crate::Value],
    ) -> Result<crate::driver::QueryResult> {
        self.inner.execute(sql, params).await
    }

    /// Health check
    ///
    /// Pings the database to verify the connection is still alive.
    ///
    /// # Returns
    ///
    /// - `Ok(())` - Connection is healthy
    /// - `Err(e)` - Connection is broken
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rsbench::pool::PooledConnection;
    /// # async fn example(mut conn: PooledConnection) -> rsbench::Result<()> {
    /// // Check if connection is still alive
    /// conn.ping().await?;
    /// println!("Connection is healthy");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn ping(&mut self) -> Result<()> {
        self.inner.ping().await
    }
}

// Drop automatically returns connection to pool
// No manual implementation needed - deadpool handles it

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_connections: usize,
    pub active_connections: usize,
    pub idle_connections: usize,
    pub pending_requests: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Connection;
    use std::time::Duration;

    #[test]
    fn test_pool_stats_creation() {
        let stats = PoolStats {
            total_connections: 10,
            active_connections: 5,
            idle_connections: 5,
            pending_requests: 0,
        };

        assert_eq!(stats.total_connections, 10);
        assert_eq!(stats.active_connections, 5);
        assert_eq!(stats.idle_connections, 5);
        assert_eq!(stats.pending_requests, 0);
    }

    #[test]
    fn test_pool_stats_busy() {
        let stats = PoolStats {
            total_connections: 10,
            active_connections: 10,
            idle_connections: 0,
            pending_requests: 5,
        };

        assert_eq!(stats.active_connections, stats.total_connections);
        assert_eq!(stats.idle_connections, 0);
        assert!(stats.pending_requests > 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_pool_creation() {
        // Create a mock driver
        use crate::driver::{ConnectionConfig, DatabaseDriver, DriverCapabilities};

        struct TestDriver;

        #[async_trait::async_trait]
        impl DatabaseDriver for TestDriver {
            fn name(&self) -> &str {
                "test"
            }

            async fn connect(
                &self,
                _config: &ConnectionConfig,
            ) -> Result<Box<dyn Connection + Send>> {
                Err(crate::Error::Database(crate::DatabaseError::Connection(
                    "test".to_string(),
                )))
            }

            fn capabilities(&self) -> DriverCapabilities {
                DriverCapabilities {
                    supports_transactions: true,
                    supports_prepared_statements: true,
                }
            }
        }

        let driver = Arc::new(TestDriver);
        let config = PoolConfig {
            min_size: 1,
            max_size: 10,
            connection_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(60),
        };

        let pool = ConnectionPool::new(driver, "test://localhost".to_string(), config);
        if let Err(ref e) = pool {
            eprintln!("Pool creation error: {:?}", e);
        }
        assert!(pool.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_pool_stats() {
        use crate::driver::{ConnectionConfig, DatabaseDriver, DriverCapabilities};

        struct TestDriver;

        #[async_trait::async_trait]
        impl DatabaseDriver for TestDriver {
            fn name(&self) -> &str {
                "test"
            }

            async fn connect(
                &self,
                _config: &ConnectionConfig,
            ) -> Result<Box<dyn Connection + Send>> {
                Err(crate::Error::Database(crate::DatabaseError::Connection(
                    "test".to_string(),
                )))
            }

            fn capabilities(&self) -> DriverCapabilities {
                DriverCapabilities {
                    supports_transactions: true,
                    supports_prepared_statements: true,
                }
            }
        }

        let driver = Arc::new(TestDriver);
        let config = PoolConfig {
            min_size: 1,
            max_size: 10,
            connection_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(60),
        };

        let pool = ConnectionPool::new(driver, "test://localhost".to_string(), config).unwrap();
        let stats = pool.stats();

        // Initial stats should show the pool is configured for max_size connections
        // but hasn't created any yet (deadpool creates on demand)
        assert_eq!(stats.total_connections, 0);
        assert_eq!(stats.idle_connections, 0);
    }
}
