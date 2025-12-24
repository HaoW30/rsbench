//! Mock database driver for testing

use rsbench::driver::{Connection, ConnectionConfig, DatabaseDriver, DriverCapabilities, QueryResult};
use rsbench::{Result, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Mock database driver that tracks calls and returns configurable results
pub struct MockDriver {
    name: String,
    connect_count: Arc<AtomicUsize>,
    connections: Arc<Mutex<Vec<MockConnection>>>,
    should_fail: bool,
}

impl MockDriver {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            connect_count: Arc::new(AtomicUsize::new(0)),
            connections: Arc::new(Mutex::new(Vec::new())),
            should_fail: false,
        }
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }

    pub fn connect_count(&self) -> usize {
        self.connect_count.load(Ordering::SeqCst)
    }

    pub fn connections(&self) -> Vec<MockConnection> {
        self.connections.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for MockDriver {
    fn name(&self) -> &str {
        &self.name
    }

    async fn connect(&self, _config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        self.connect_count.fetch_add(1, Ordering::SeqCst);

        if self.should_fail {
            return Err(rsbench::Error::Database(
                rsbench::DatabaseError::Connection("Mock connection failure".into()),
            ));
        }

        let conn = MockConnection::new(self.connect_count());
        self.connections.lock().unwrap().push(conn.clone());
        Ok(Box::new(conn))
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            supports_transactions: true,
            supports_prepared_statements: true,
        }
    }
}

/// Mock database connection that tracks operations
#[derive(Clone)]
pub struct MockConnection {
    id: usize,
    execute_count: Arc<AtomicUsize>,
    queries: Arc<Mutex<Vec<String>>>,
    should_fail: bool,
    rows_affected: u64,
}

impl MockConnection {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            execute_count: Arc::new(AtomicUsize::new(0)),
            queries: Arc::new(Mutex::new(Vec::new())),
            should_fail: false,
            rows_affected: 1,
        }
    }

    pub fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }

    pub fn with_rows_affected(mut self, rows: u64) -> Self {
        self.rows_affected = rows;
        self
    }

    pub fn execute_count(&self) -> usize {
        self.execute_count.load(Ordering::SeqCst)
    }

    pub fn queries(&self) -> Vec<String> {
        self.queries.lock().unwrap().clone()
    }

    pub fn id(&self) -> usize {
        self.id
    }
}

#[async_trait::async_trait]
impl Connection for MockConnection {
    async fn execute(&mut self, sql: &str, _params: &[Value]) -> Result<QueryResult> {
        self.execute_count.fetch_add(1, Ordering::SeqCst);
        self.queries.lock().unwrap().push(sql.to_string());

        if self.should_fail {
            return Err(rsbench::Error::Database(
                rsbench::DatabaseError::Query("Mock query failure".into()),
            ));
        }

        Ok(QueryResult {
            rows_affected: self.rows_affected,
            last_insert_id: Some(self.id as u64),
        })
    }

    async fn begin(&mut self) -> Result<()> {
        self.queries
            .lock()
            .unwrap()
            .push("BEGIN TRANSACTION".to_string());
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.queries.lock().unwrap().push("COMMIT".to_string());
        Ok(())
    }

    async fn rollback(&mut self) -> Result<()> {
        self.queries.lock().unwrap().push("ROLLBACK".to_string());
        Ok(())
    }

    async fn ping(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_driver_connect() {
        let driver = MockDriver::new("mock");
        assert_eq!(driver.connect_count(), 0);

        let _conn = driver
            .connect(&ConnectionConfig {
                connection_string: "mock://test".to_string(),
                timeout: std::time::Duration::from_secs(5),
            })
            .await
            .unwrap();

        assert_eq!(driver.connect_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_connection_execute() {
        let driver = MockDriver::new("mock");
        let mut conn = driver
            .connect(&ConnectionConfig {
                connection_string: "mock://test".to_string(),
                timeout: std::time::Duration::from_secs(5),
            })
            .await
            .unwrap();

        let result = conn.execute("SELECT 1", &[]).await.unwrap();
        assert_eq!(result.rows_affected, 1);
    }

    #[tokio::test]
    async fn test_mock_driver_failure() {
        let driver = MockDriver::new("mock").with_failure();
        let result = driver
            .connect(&ConnectionConfig {
                connection_string: "mock://test".to_string(),
                timeout: std::time::Duration::from_secs(5),
            })
            .await;

        assert!(result.is_err());
    }
}
