//! Mock connection pool for testing

use rsbench::pool::{PoolStats, PooledConnection};
use rsbench::driver::Connection;
use rsbench::Result;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::mock_driver::MockConnection;

/// Mock connection pool that returns configurable connections and stats
pub struct MockConnectionPool {
    connections: Arc<Mutex<Vec<MockConnection>>>,
    get_count: Arc<AtomicUsize>,
    should_fail: bool,
    stats: Arc<Mutex<PoolStats>>,
}

impl MockConnectionPool {
    /// Create a new mock pool
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(Vec::new())),
            get_count: Arc::new(AtomicUsize::new(0)),
            should_fail: false,
            stats: Arc::new(Mutex::new(PoolStats {
                total_connections: 10,
                active_connections: 0,
                idle_connections: 10,
                pending_requests: 0,
            })),
        }
    }

    /// Configure pool to fail on get()
    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }

    /// Set pool statistics
    pub fn with_stats(self, stats: PoolStats) -> Self {
        *self.stats.lock().unwrap() = stats;
        self
    }

    /// Add a connection to return from get()
    pub fn add_connection(&self, conn: MockConnection) {
        self.connections.lock().unwrap().push(conn);
    }

    /// Get number of times get() was called
    pub fn get_count(&self) -> usize {
        self.get_count.load(Ordering::SeqCst)
    }

    /// Get connection from pool (simulates ConnectionPool::get)
    pub async fn get(&self) -> Result<Box<dyn Connection>> {
        self.get_count.fetch_add(1, Ordering::SeqCst);

        if self.should_fail {
            return Err(rsbench::Error::Database(
                rsbench::DatabaseError::Connection("Mock pool exhausted".into()),
            ));
        }

        // Return a fresh mock connection
        let conn = MockConnection::new(self.get_count());
        Ok(Box::new(conn))
    }

    /// Get pool statistics (simulates ConnectionPool::stats)
    pub fn stats(&self) -> PoolStats {
        self.stats.lock().unwrap().clone()
    }

    /// Update pool statistics (for testing)
    pub fn set_stats(&self, stats: PoolStats) {
        *self.stats.lock().unwrap() = stats;
    }
}

impl Default for MockConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_pool_get() {
        let pool = MockConnectionPool::new();
        assert_eq!(pool.get_count(), 0);

        let _conn = pool.get().await.unwrap();
        assert_eq!(pool.get_count(), 1);

        let _conn2 = pool.get().await.unwrap();
        assert_eq!(pool.get_count(), 2);
    }

    #[tokio::test]
    async fn test_mock_pool_failure() {
        let pool = MockConnectionPool::new().with_failure();
        let result = pool.get().await;
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_pool_stats() {
        let pool = MockConnectionPool::new();
        let stats = pool.stats();

        assert_eq!(stats.total_connections, 10);
        assert_eq!(stats.active_connections, 0);
    }

    #[test]
    fn test_mock_pool_custom_stats() {
        let custom_stats = PoolStats {
            total_connections: 20,
            active_connections: 15,
            idle_connections: 5,
            pending_requests: 3,
        };

        let pool = MockConnectionPool::new().with_stats(custom_stats.clone());
        let stats = pool.stats();

        assert_eq!(stats.total_connections, 20);
        assert_eq!(stats.active_connections, 15);
        assert_eq!(stats.idle_connections, 5);
        assert_eq!(stats.pending_requests, 3);
    }

    #[test]
    fn test_mock_pool_set_stats() {
        let pool = MockConnectionPool::new();

        let new_stats = PoolStats {
            total_connections: 100,
            active_connections: 90,
            idle_connections: 10,
            pending_requests: 5,
        };

        pool.set_stats(new_stats.clone());
        let stats = pool.stats();

        assert_eq!(stats.total_connections, 100);
        assert_eq!(stats.active_connections, 90);
    }
}
