//! Driver integration tests
//!
//! Tests database driver functionality (requires database instance).
//!
//! Run with: cargo test --test driver_integration_test -- --ignored

use rsbench::driver::{ConnectionConfig, DatabaseDriver, MySqlDriver};
use rsbench::Value;
use std::time::Duration;

#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore] // Requires MySQL instance
async fn test_mysql_driver_connection() {
    let driver = MySqlDriver::new();

    // Use port 4000 for local MySQL instance (empty password)
    let config = ConnectionConfig {
        connection_string: "mysql://root@localhost:4000/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    // Test connection
    let mut conn = driver.connect(&config).await.unwrap();

    // Test ping
    conn.ping().await.unwrap();

    println!("✅ MySQL driver connection successful");
}

#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore] // Requires MySQL instance
async fn test_mysql_query_execution() {
    let driver = MySqlDriver::new();
    let config = ConnectionConfig {
        connection_string: "mysql://root@localhost:4000/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    let mut conn = driver.connect(&config).await.unwrap();

    // Create test table
    conn.execute("DROP TABLE IF EXISTS test_query", &[])
        .await
        .unwrap();

    conn.execute(
        "CREATE TABLE test_query (id INT PRIMARY KEY, name VARCHAR(100))",
        &[],
    )
    .await
    .unwrap();

    // Test INSERT
    let result = conn
        .execute(
            "INSERT INTO test_query (id, name) VALUES (?, ?)",
            &[Value::Int(1), Value::String("test".into())],
        )
        .await
        .unwrap();
    assert_eq!(result.rows_affected, 1);
    println!("✅ INSERT: 1 row affected");

    // Test UPDATE
    let result = conn
        .execute(
            "UPDATE test_query SET name = ? WHERE id = ?",
            &[Value::String("updated".into()), Value::Int(1)],
        )
        .await
        .unwrap();
    assert_eq!(result.rows_affected, 1);
    println!("✅ UPDATE: 1 row affected");

    // Test DELETE
    let result = conn
        .execute(
            "DELETE FROM test_query WHERE id = ?",
            &[Value::Int(1)],
        )
        .await
        .unwrap();
    assert_eq!(result.rows_affected, 1);
    println!("✅ DELETE: 1 row affected");

    // Cleanup
    conn.execute("DROP TABLE test_query", &[]).await.unwrap();

    println!("✅ All query execution tests passed");
}

#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore] // Requires MySQL instance
async fn test_mysql_transaction_handling() {
    let driver = MySqlDriver::new();
    let config = ConnectionConfig {
        connection_string: "mysql://root@localhost:4000/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    let mut conn = driver.connect(&config).await.unwrap();

    // Setup test table
    conn.execute("DROP TABLE IF EXISTS test_txn", &[])
        .await
        .unwrap();

    conn.execute(
        "CREATE TABLE test_txn (id INT PRIMARY KEY)",
        &[],
    )
    .await
    .unwrap();

    // Test transaction rollback
    conn.begin().await.unwrap();
    println!("✅ BEGIN transaction");

    conn.execute(
        "INSERT INTO test_txn (id) VALUES (?)",
        &[Value::Int(1)],
    )
    .await
    .unwrap();
    println!("✅ INSERT in transaction");

    conn.rollback().await.unwrap();
    println!("✅ ROLLBACK transaction");

    // Test transaction commit
    conn.begin().await.unwrap();
    println!("✅ BEGIN transaction");

    conn.execute(
        "INSERT INTO test_txn (id) VALUES (?)",
        &[Value::Int(2)],
    )
    .await
    .unwrap();
    println!("✅ INSERT in transaction");

    conn.commit().await.unwrap();
    println!("✅ COMMIT transaction");

    // Cleanup
    conn.execute("DROP TABLE test_txn", &[]).await.unwrap();

    println!("✅ All transaction tests passed");
}

#[cfg(feature = "mysql")]
#[tokio::test]
#[ignore] // Requires MySQL instance
async fn test_mysql_error_handling() {
    use rsbench::DatabaseError;

    let driver = MySqlDriver::new();

    // Test invalid connection string
    let bad_config = ConnectionConfig {
        connection_string: "mysql://baduser:badpass@localhost:4000/baddb".into(),
        timeout: Duration::from_secs(5),
    };

    let result = driver.connect(&bad_config).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, rsbench::Error::Database(DatabaseError::Connection(_))));
    }
    println!("✅ Invalid connection properly rejected");

    // Test invalid query
    let good_config = ConnectionConfig {
        connection_string: "mysql://root@localhost:4000/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    let mut conn = driver.connect(&good_config).await.unwrap();

    let result = conn.execute("INVALID SQL SYNTAX", &[]).await;
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(matches!(e, rsbench::Error::Database(DatabaseError::Query(_))));
    }
    println!("✅ Invalid query properly rejected");

    println!("✅ All error handling tests passed");
}
