//! MySQL driver implementation

use super::{Connection, ConnectionConfig, DatabaseDriver, DriverCapabilities, QueryResult};
use crate::{DatabaseError, Result, Value};
use mysql_async::{prelude::*, Conn, Params, Pool};

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

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let opts = mysql_async::Opts::from_url(&config.connection_string)
            .map_err(|e| DatabaseError::Connection(e.to_string()))?;

        let pool = Pool::new(opts);
        let conn = pool
            .get_conn()
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
