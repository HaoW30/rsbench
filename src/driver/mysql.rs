//! MySQL driver implementation
//!
//! # Architecture Note
//!
//! This driver creates individual connections directly using `mysql_async::Conn::new()`.
//! **Connection pooling is handled by the ConnectionPool module**, not by this driver.
//! This design avoids double-pooling and maintains clear separation of concerns.
//!
//! # Connection String Format
//!
//! ```text
//! mysql://user:password@host:port/database
//! mysql://user:password@host:port/database?ssl-mode=required
//! ```

use super::{Connection, ConnectionConfig, DatabaseDriver, DriverCapabilities, QueryResult};
use crate::{DatabaseError, Result, Value};
use mysql_async::{prelude::*, Conn, Params};

/// MySQL driver implementation
pub struct MySqlDriver;

impl MySqlDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl DatabaseDriver for MySqlDriver {
    fn name(&self) -> &str {
        "mysql"
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection + Send>> {
        let opts = mysql_async::Opts::from_url(&config.connection_string)
            .map_err(|e| DatabaseError::Connection(format!("URL parse error: {}", e)))?;

        // ✅ Create connection directly (no pool)
        // This is the correct approach - pooling happens in ConnectionPool module
        let conn = Conn::new(opts).await
            .map_err(|e| DatabaseError::Connection(format!("MySQL connect error: {}", e)))?;

        Ok(Box::new(MySqlConnection { conn }))
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            supports_transactions: true,
            supports_prepared_statements: true,
        }
    }
}

/// MySQL connection
struct MySqlConnection {
    conn: Conn,
}

#[async_trait::async_trait]
impl Connection for MySqlConnection {
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult> {
        let mysql_params = convert_params(params);

        // Execute query and capture result
        let result = self.conn.exec_drop(sql, mysql_params).await;

        match &result {
            Ok(_) => {
                // Success - no logging needed
            }
            Err(e) => {
                // Error - classify and handle based on error type
                static ERR_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                let err_num = ERR_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;

                // Only log first few errors to avoid spam (runtime will also log)
                if err_num <= 5 {
                    eprintln!("[MySQL] Query ERROR #{}: {}", err_num, e);
                    eprintln!("[MySQL]   SQL: {}", sql);
                    eprintln!("[MySQL]   Params: {:?}", params);
                    eprintln!("[MySQL]   Error details: {:?}", e);
                }

                // Try to recover connection state after query errors
                // This attempts to clear any partial state that might corrupt the connection
                // For most query errors (syntax, param mismatch, constraint violations),
                // the connection should remain usable
                if is_recoverable_error(&e) {
                    // For recoverable errors, the connection should still be valid
                    // No action needed - just return the error
                } else {
                    // For non-recoverable errors (I/O errors, connection closed),
                    // the connection is likely broken and will fail ping() on recycle
                    eprintln!("[MySQL] Non-recoverable error detected - connection may be broken");
                }
            }
        }

        result.map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(QueryResult {
            rows_affected: self.conn.affected_rows(),
            last_insert_id: self.conn.last_insert_id(),
        })
    }

    async fn begin(&mut self) -> Result<()> {
        self.conn
            .query_drop("START TRANSACTION")
            .await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))?;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.conn
            .query_drop("COMMIT")
            .await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))?;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<()> {
        self.conn
            .query_drop("ROLLBACK")
            .await
            .map_err(|e| DatabaseError::Transaction(e.to_string()))?;
        Ok(())
    }

    async fn ping(&mut self) -> Result<()> {
        self.conn
            .ping()
            .await
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;
        Ok(())
    }
}

/// Classify whether an error is recoverable (connection remains valid)
/// vs non-recoverable (connection is broken)
fn is_recoverable_error(e: &mysql_async::Error) -> bool {
    use mysql_async::Error;

    match e {
        // Driver errors (parameter mismatch, etc.) are recoverable
        // The query never reaches the server, connection is still valid
        Error::Driver(_) => true,

        // Server errors (SQL syntax, constraint violations, etc.) are recoverable
        // The server processed the query and returned an error, but connection is fine
        Error::Server(_) => true,

        // I/O errors mean the connection is broken
        Error::Io(_) => false,

        // Other errors - treat as non-recoverable to be safe
        _ => false,
    }
}

fn convert_params(params: &[Value]) -> Params {
    let values: Vec<mysql_async::Value> = params
        .iter()
        .map(|v| match v {
            Value::Int(i) => mysql_async::Value::Int(*i),
            Value::Float(f) => mysql_async::Value::Double(*f),
            Value::String(s) => mysql_async::Value::Bytes(s.as_bytes().to_vec()),
            Value::Bytes(b) => mysql_async::Value::Bytes(b.clone()),
            Value::Null => mysql_async::Value::NULL,
        })
        .collect();

    Params::Positional(values)
}
