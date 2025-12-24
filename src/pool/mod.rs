//! Connection pool module
//!
//! Manages database connections with backpressure awareness.

use crate::config::PoolConfig;
use crate::driver::{Connection, DatabaseDriver};
use crate::Result;
use std::sync::Arc;

/// Connection pool (M0: single endpoint only)
pub struct ConnectionPool {
    driver: Arc<dyn DatabaseDriver>,
    config: PoolConfig,
    connection_string: String,
}

impl ConnectionPool {
    /// Create new connection pool
    pub fn new(
        driver: Arc<dyn DatabaseDriver>,
        connection_string: String,
        config: PoolConfig,
    ) -> Result<Self> {
        Ok(Self {
            driver,
            config,
            connection_string,
        })
    }

    /// Get connection from pool
    pub async fn get(&self) -> Result<PooledConnection> {
        // M0: Simplified - create new connection each time
        // TODO: Implement actual pooling with deadpool in later iteration
        let conn_config = crate::driver::ConnectionConfig {
            connection_string: self.connection_string.clone(),
            timeout: self.config.connection_timeout,
        };

        let conn = self.driver.connect(&conn_config).await?;

        Ok(PooledConnection { inner: conn })
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        // M0: Simplified - return basic stats
        PoolStats {
            total_connections: self.config.max_size,
            active_connections: 0, // TODO: Track actual connections
            idle_connections: self.config.max_size,
            pending_requests: 0,
        }
    }
}

/// Pooled connection wrapper
pub struct PooledConnection {
    inner: Box<dyn Connection>,
}

impl PooledConnection {
    /// Execute query
    pub async fn execute(
        &mut self,
        sql: &str,
        params: &[crate::Value],
    ) -> Result<crate::driver::QueryResult> {
        self.inner.execute(sql, params).await
    }

    /// Health check
    pub async fn ping(&mut self) -> Result<()> {
        self.inner.ping().await
    }
}

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_connections: usize,
    pub active_connections: usize,
    pub idle_connections: usize,
    pub pending_requests: usize,
}
