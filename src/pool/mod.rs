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
use deadpool::Runtime;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn, error};

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
    config: PoolConfig,

    /// Health metrics - total successful checkouts
    total_checkouts: Arc<AtomicUsize>,

    /// Health metrics - total connection errors
    connection_errors: Arc<AtomicUsize>,
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
        // Timeouts are now enabled with proper runtime specification
        let pool_config = DeadpoolConfig {
            max_size: config.max_size,
            timeouts: deadpool::managed::Timeouts {
                wait: Some(config.connection_timeout),
                create: Some(config.connection_timeout),
                recycle: Some(config.connection_timeout),
            },
            ..Default::default()
        };

        // Build the pool using deadpool 0.12 API
        // Key: Must specify runtime for timeout support (see https://github.com/deadpool-rs/deadpool/issues/195)
        let inner = Pool::builder(manager)
            .runtime(Runtime::Tokio1)  // Enable Tokio runtime for timeouts
            .config(pool_config)
            .build()
            .map_err(|e| {
                crate::PoolError::Configuration(format!(
                    "Failed to build pool: {}",
                    e
                ))
            })?;

        let pool = Self {
            inner,
            config,
            total_checkouts: Arc::new(AtomicUsize::new(0)),
            connection_errors: Arc::new(AtomicUsize::new(0)),
        };

        info!(
            max_size = pool.config.max_size,
            min_size = pool.config.min_size,
            connection_timeout_ms = pool.config.connection_timeout.as_millis(),
            "Connection pool created"
        );

        Ok(pool)
    }

    /// Pre-warm the pool by creating min_size connections
    ///
    /// This method creates the minimum number of connections specified in the pool config,
    /// ensuring they're ready for immediate use. This is useful to avoid cold-start latency
    /// on first requests.
    ///
    /// # Returns
    ///
    /// - `Ok(())` - Successfully created min_size connections
    /// - `Err(e)` - Failed to create connections
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use rsbench::pool::ConnectionPool;
    /// # async fn example(pool: ConnectionPool) -> rsbench::Result<()> {
    /// // Pre-warm the pool on startup
    /// pool.warm_up().await?;
    ///
    /// // Now connections are ready for use
    /// let conn = pool.get().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn warm_up(&self) -> Result<()> {
        // Pre-create min_size connections by checking them out and immediately returning them
        let mut connections = Vec::new();

        for _ in 0..self.config.min_size {
            match self.get().await {
                Ok(conn) => connections.push(conn),
                Err(e) => {
                    // Failed to create a connection during warm-up
                    // Drop any connections we did create and return the error
                    return Err(e);
                }
            }
        }

        // Connections are automatically returned to the pool when dropped
        drop(connections);

        Ok(())
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
        let result = self
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
            });

        // Track metrics and log events
        match &result {
            Ok(_) => {
                // Successful checkout
                let checkout_count = self.total_checkouts.fetch_add(1, Ordering::Relaxed) + 1;
                debug!(
                    checkout_count,
                    active = self.inner.status().size - self.inner.status().available,
                    "Connection checked out from pool"
                );
            }
            Err(e) => {
                // Connection error
                let error_count = self.connection_errors.fetch_add(1, Ordering::Relaxed) + 1;
                error!(
                    error_count,
                    error = %e,
                    "Failed to get connection from pool"
                );

                // Warn if pool is saturated
                let status = self.inner.status();
                if status.waiting > 0 {
                    warn!(
                        pending_requests = status.waiting,
                        total_connections = status.size,
                        available = status.available,
                        "Pool saturation detected - requests waiting for connections"
                    );
                }
            }
        }

        result.map(|conn| PooledConnection { inner: conn })
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
            // Pool metrics
            total_connections: status.size,
            active_connections: status.size - status.available,
            idle_connections: status.available,
            pending_requests: status.waiting,

            // Health metrics
            total_checkouts: self.total_checkouts.load(Ordering::Relaxed),
            connection_errors: self.connection_errors.load(Ordering::Relaxed),
            max_lifetime: Some(self.config.idle_timeout),
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

/// Pool statistics with health monitoring
#[derive(Debug, Clone)]
pub struct PoolStats {
    // Connection pool metrics
    pub total_connections: usize,
    pub active_connections: usize,
    pub idle_connections: usize,
    pub pending_requests: usize,

    // Health monitoring metrics
    pub total_checkouts: usize,
    pub connection_errors: usize,
    pub max_lifetime: Option<Duration>,
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
            total_checkouts: 100,
            connection_errors: 2,
            max_lifetime: Some(Duration::from_secs(600)),
        };

        assert_eq!(stats.total_connections, 10);
        assert_eq!(stats.active_connections, 5);
        assert_eq!(stats.idle_connections, 5);
        assert_eq!(stats.pending_requests, 0);
        assert_eq!(stats.total_checkouts, 100);
        assert_eq!(stats.connection_errors, 2);
        assert_eq!(stats.max_lifetime, Some(Duration::from_secs(600)));
    }

    #[test]
    fn test_pool_stats_busy() {
        let stats = PoolStats {
            total_connections: 10,
            active_connections: 10,
            idle_connections: 0,
            pending_requests: 5,
            total_checkouts: 50,
            connection_errors: 10,
            max_lifetime: Some(Duration::from_secs(300)),
        };

        assert_eq!(stats.active_connections, stats.total_connections);
        assert_eq!(stats.idle_connections, 0);
        assert!(stats.pending_requests > 0);
        assert_eq!(stats.total_checkouts, 50);
        assert_eq!(stats.connection_errors, 10);
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

    #[tokio::test(flavor = "multi_thread")]
    async fn test_pool_warm_up() {
        use crate::driver::{ConnectionConfig, DatabaseDriver, DriverCapabilities, QueryResult};
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Track how many connections were created
        let create_count = Arc::new(AtomicUsize::new(0));
        let create_count_clone = create_count.clone();

        struct TestDriver {
            create_count: Arc<AtomicUsize>,
        }

        #[async_trait::async_trait]
        impl DatabaseDriver for TestDriver {
            fn name(&self) -> &str {
                "test"
            }

            async fn connect(
                &self,
                _config: &ConnectionConfig,
            ) -> Result<Box<dyn Connection + Send>> {
                // Increment connection counter
                self.create_count.fetch_add(1, Ordering::SeqCst);

                // Create a test connection
                Ok(Box::new(TestConnection))
            }

            fn capabilities(&self) -> DriverCapabilities {
                DriverCapabilities {
                    supports_transactions: true,
                    supports_prepared_statements: true,
                }
            }
        }

        struct TestConnection;

        #[async_trait::async_trait]
        impl Connection for TestConnection {
            async fn execute(
                &mut self,
                _sql: &str,
                _params: &[crate::Value],
            ) -> Result<QueryResult> {
                Ok(QueryResult {
                    rows_affected: 0,
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
                Ok(())
            }
        }

        let driver = Arc::new(TestDriver {
            create_count: create_count_clone,
        });

        let config = PoolConfig {
            min_size: 5,
            max_size: 10,
            connection_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(60),
        };

        let pool = ConnectionPool::new(driver, "test://localhost".to_string(), config).unwrap();

        // Initially no connections
        assert_eq!(pool.stats().total_connections, 0);
        assert_eq!(create_count.load(Ordering::SeqCst), 0);

        // Warm up the pool
        pool.warm_up().await.unwrap();

        // Should have created min_size connections
        assert_eq!(create_count.load(Ordering::SeqCst), 5);

        // After warm-up, pool should have connections
        let stats = pool.stats();
        assert_eq!(stats.total_connections, 5);
        assert_eq!(stats.idle_connections, 5);
        assert_eq!(stats.active_connections, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_pool_timeout_enforcement() {
        use crate::driver::{ConnectionConfig, DatabaseDriver, DriverCapabilities};
        use tokio::time::{sleep, Duration};

        // Driver that takes a long time to connect
        struct SlowDriver;

        #[async_trait::async_trait]
        impl DatabaseDriver for SlowDriver {
            fn name(&self) -> &str {
                "slow"
            }

            async fn connect(
                &self,
                _config: &ConnectionConfig,
            ) -> Result<Box<dyn Connection + Send>> {
                // Simulate slow connection (10 seconds)
                sleep(Duration::from_secs(10)).await;
                Err(crate::Error::Database(crate::DatabaseError::Connection(
                    "Should timeout before this".to_string(),
                )))
            }

            fn capabilities(&self) -> DriverCapabilities {
                DriverCapabilities {
                    supports_transactions: true,
                    supports_prepared_statements: true,
                }
            }
        }

        let driver = Arc::new(SlowDriver);
        let config = PoolConfig {
            min_size: 1,
            max_size: 10,
            connection_timeout: Duration::from_millis(100), // Short timeout
            idle_timeout: Duration::from_secs(60),
        };

        let pool = ConnectionPool::new(driver, "slow://localhost".to_string(), config).unwrap();

        // Try to get a connection - should timeout
        let start = tokio::time::Instant::now();
        let result = pool.get().await;
        let elapsed = start.elapsed();

        // Should fail with timeout
        assert!(result.is_err());

        // Should timeout quickly (within 1 second), not wait for full 10 seconds
        assert!(elapsed < Duration::from_secs(1), "Timeout took too long: {:?}", elapsed);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_health_metrics_tracking() {
        use crate::driver::{ConnectionConfig, DatabaseDriver, DriverCapabilities, QueryResult};

        // Track successful and failed connections
        struct MetricsTestDriver {
            fail_count: Arc<AtomicUsize>,
        }

        #[async_trait::async_trait]
        impl DatabaseDriver for MetricsTestDriver {
            fn name(&self) -> &str {
                "metrics-test"
            }

            async fn connect(
                &self,
                _config: &ConnectionConfig,
            ) -> Result<Box<dyn Connection + Send>> {
                // Fail first 2 attempts, then succeed
                let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    Err(crate::Error::Database(crate::DatabaseError::Connection(
                        format!("Simulated failure #{}", count + 1),
                    )))
                } else {
                    Ok(Box::new(MetricsTestConnection))
                }
            }

            fn capabilities(&self) -> DriverCapabilities {
                DriverCapabilities {
                    supports_transactions: true,
                    supports_prepared_statements: true,
                }
            }
        }

        struct MetricsTestConnection;

        #[async_trait::async_trait]
        impl Connection for MetricsTestConnection {
            async fn execute(
                &mut self,
                _sql: &str,
                _params: &[crate::Value],
            ) -> Result<QueryResult> {
                Ok(QueryResult {
                    rows_affected: 0,
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
                Ok(())
            }
        }

        let fail_count = Arc::new(AtomicUsize::new(0));
        let driver = Arc::new(MetricsTestDriver {
            fail_count: fail_count.clone(),
        });

        let config = PoolConfig {
            min_size: 1,
            max_size: 10,
            connection_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(60),
        };

        let pool = ConnectionPool::new(driver, "metrics://localhost".to_string(), config).unwrap();

        // Initial stats - no activity
        let stats = pool.stats();
        assert_eq!(stats.total_checkouts, 0);
        assert_eq!(stats.connection_errors, 0);

        // Try to get connections - first 2 will fail
        let _ = pool.get().await; // Fail #1
        let _ = pool.get().await; // Fail #2
        let conn3 = pool.get().await; // Success #1

        // Check metrics after failures and success
        let stats = pool.stats();
        assert_eq!(stats.connection_errors, 2, "Should have 2 connection errors");
        assert_eq!(stats.total_checkouts, 1, "Should have 1 successful checkout");

        // Get more successful connections
        drop(conn3);
        let _conn4 = pool.get().await.unwrap(); // Success #2
        let _conn5 = pool.get().await.unwrap(); // Success #3

        // Final metrics check
        let stats = pool.stats();
        assert_eq!(stats.connection_errors, 2, "Still 2 errors");
        assert_eq!(stats.total_checkouts, 3, "Now 3 successful checkouts");
        assert_eq!(stats.max_lifetime, Some(Duration::from_secs(60)));
    }
}
