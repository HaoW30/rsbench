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
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;

        // ✅ Create connection directly (no pool)
        // This is the correct approach - pooling happens in ConnectionPool module
        let conn = Conn::new(opts)
            .await
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;

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

        self.conn
            .exec_drop(sql, mysql_params)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

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
