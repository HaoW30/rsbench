//! PostgreSQL driver implementation
//!
//! # Architecture Note
//!
//! This driver creates individual connections directly using `tokio_postgres::connect()`.
//! **Connection pooling is handled by the ConnectionPool module**, not by this driver.
//! This design avoids double-pooling and maintains clear separation of concerns.
//!
//! # Connection String Format
//!
//! ```text
//! postgresql://user:password@host:port/database
//! postgres://user:password@host:port/database?sslmode=require
//! ```
//!
//! # Background Task Model
//!
//! PostgreSQL connections require spawning a background task to process I/O.
//! The `Connection` object drives the protocol, while the `Client` executes queries.

use super::{Connection, ConnectionConfig, DatabaseDriver, DriverCapabilities, QueryResult};
use crate::{DatabaseError, Result, Value};
use tokio_postgres::{types::ToSql, Client, NoTls};

/// PostgreSQL driver implementation
pub struct PostgresDriver;

impl PostgresDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for PostgresDriver {
    fn name(&self) -> &str {
        "postgres"
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection + Send>> {
        // Parse connection string and connect
        let (client, connection) = tokio_postgres::connect(&config.connection_string, NoTls)
            .await
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;

        // Spawn background task to process connection I/O
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("PostgreSQL connection error: {}", e);
            }
        });

        Ok(Box::new(PostgresConnection { client }))
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            supports_transactions: true,
            supports_prepared_statements: true,
        }
    }
}

/// PostgreSQL connection
struct PostgresConnection {
    client: Client,
}

#[async_trait::async_trait]
impl Connection for PostgresConnection {
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        // Convert RSBench Value types to PostgreSQL ToSql-compatible concrete types
        // We use concrete types (i32, i64, f64, String, Vec<u8>) instead of trait objects
        // to avoid Send issues across await boundaries
        let converted_params: Vec<_> = params
            .iter()
            .map(|v| match v {
                Value::Int(i) => {
                    // PostgreSQL INT type is i32, BIGINT is i64
                    // Use i32 if value fits, otherwise use i64
                    if *i >= i32::MIN as i64 && *i <= i32::MAX as i64 {
                        ConvertedParam::Int32(*i as i32)
                    } else {
                        ConvertedParam::Int64(*i)
                    }
                }
                Value::Float(f) => ConvertedParam::Float(*f),
                Value::String(s) => ConvertedParam::String(s.clone()),
                Value::Bytes(b) => ConvertedParam::Bytes(b.clone()),
                Value::Null => ConvertedParam::Null,
            })
            .collect();

        // Create references for PostgreSQL execute call
        let param_refs: Vec<&(dyn ToSql + Sync)> = converted_params
            .iter()
            .map(|p| p.as_tosql())
            .collect();

        // Execute query
        let rows_affected = self
            .client
            .execute(sql, &param_refs[..])
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(QueryResult {
            rows_affected,
            last_insert_id: None, // PostgreSQL uses RETURNING clause for insert IDs
        })
    }

    async fn begin(&mut self) -> Result<()> {
        self.client
            .batch_execute("BEGIN")
            .await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))?;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.client
            .batch_execute("COMMIT")
            .await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))?;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<()> {
        self.client
            .batch_execute("ROLLBACK")
            .await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))?;
        Ok(())
    }

    async fn ping(&mut self) -> Result<()> {
        self.client
            .simple_query("SELECT 1")
            .await
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;
        Ok(())
    }
}

/// Helper enum to store converted parameter values
/// Uses concrete types instead of trait objects to ensure Send safety
enum ConvertedParam {
    Int32(i32),  // PostgreSQL INT is i32
    Int64(i64),  // PostgreSQL BIGINT is i64
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Null,
}

impl ConvertedParam {
    fn as_tosql(&self) -> &(dyn ToSql + Sync) {
        match self {
            ConvertedParam::Int32(i) => i,
            ConvertedParam::Int64(i) => i,
            ConvertedParam::Float(f) => f,
            ConvertedParam::String(s) => s,
            ConvertedParam::Bytes(b) => b,
            ConvertedParam::Null => &Option::<i32>::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_driver_creation() {
        let driver = PostgresDriver::new();
        assert_eq!(driver.name(), "postgres");
    }

    #[test]
    fn test_postgres_capabilities() {
        let driver = PostgresDriver::new();
        let caps = driver.capabilities();
        assert!(caps.supports_transactions);
        assert!(caps.supports_prepared_statements);
    }

    #[test]
    fn test_postgres_parameter_conversion() {
        let params = vec![
            Value::Int(42),
            Value::Int(i64::MAX), // Test large int -> BIGINT
            Value::Float(3.14),
            Value::String("test".into()),
            Value::Bytes(vec![1, 2, 3]),
            Value::Null,
        ];

        let converted: Vec<_> = params
            .iter()
            .map(|v| match v {
                Value::Int(i) => {
                    if *i >= i32::MIN as i64 && *i <= i32::MAX as i64 {
                        ConvertedParam::Int32(*i as i32)
                    } else {
                        ConvertedParam::Int64(*i)
                    }
                }
                Value::Float(f) => ConvertedParam::Float(*f),
                Value::String(s) => ConvertedParam::String(s.clone()),
                Value::Bytes(b) => ConvertedParam::Bytes(b.clone()),
                Value::Null => ConvertedParam::Null,
            })
            .collect();

        assert_eq!(converted.len(), 6);

        // Test that as_tosql() works
        for param in &converted {
            let _ = param.as_tosql();
        }

        // Verify correct type selection
        match &converted[0] {
            ConvertedParam::Int32(i) => assert_eq!(*i, 42),
            _ => panic!("Expected Int32"),
        }

        match &converted[1] {
            ConvertedParam::Int64(i) => assert_eq!(*i, i64::MAX),
            _ => panic!("Expected Int64"),
        }
    }
}
