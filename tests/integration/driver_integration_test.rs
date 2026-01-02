//! Driver integration tests
//!
//! Tests database driver functionality (requires database instance).
//!
//! Run with: cargo test --test driver_integration_test -- --ignored

use rsbench::driver::{ConnectionConfig, DatabaseDriver, MySqlDriver};

#[cfg(feature = "postgres")]
use rsbench::driver::PostgresDriver;

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

// ============================================================================
// PostgreSQL Integration Tests
// ============================================================================

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore] // Requires PostgreSQL instance
async fn test_postgres_driver_connection() {
    let driver = PostgresDriver::new();

    // Use port 5432 for local PostgreSQL instance
    let config = ConnectionConfig {
        connection_string: "postgresql://postgres@localhost:5432/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    // Test connection
    let mut conn = driver.connect(&config).await.unwrap();

    // Test ping
    conn.ping().await.unwrap();

    println!("✅ PostgreSQL driver connection successful");
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore] // Requires PostgreSQL instance
async fn test_postgres_query_execution() {
    let driver = PostgresDriver::new();
    let config = ConnectionConfig {
        connection_string: "postgresql://postgres@localhost:5432/testdb".into(),
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

    // Test INSERT (PostgreSQL uses $1, $2 instead of ?)
    let result = conn
        .execute(
            "INSERT INTO test_query (id, name) VALUES ($1, $2)",
            &[Value::Int(1), Value::String("test".into())],
        )
        .await
        .unwrap();
    assert_eq!(result.rows_affected, 1);
    println!("✅ INSERT: 1 row affected");

    // Test UPDATE
    let result = conn
        .execute(
            "UPDATE test_query SET name = $1 WHERE id = $2",
            &[Value::String("updated".into()), Value::Int(1)],
        )
        .await
        .unwrap();
    assert_eq!(result.rows_affected, 1);
    println!("✅ UPDATE: 1 row affected");

    // Test DELETE
    let result = conn
        .execute(
            "DELETE FROM test_query WHERE id = $1",
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

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore] // Requires PostgreSQL instance
async fn test_postgres_transaction_handling() {
    let driver = PostgresDriver::new();
    let config = ConnectionConfig {
        connection_string: "postgresql://postgres@localhost:5432/testdb".into(),
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
        "INSERT INTO test_txn (id) VALUES ($1)",
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
        "INSERT INTO test_txn (id) VALUES ($1)",
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

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore] // Requires PostgreSQL instance
async fn test_postgres_error_handling() {
    use rsbench::DatabaseError;

    let driver = PostgresDriver::new();

    // Test invalid connection string
    let bad_config = ConnectionConfig {
        connection_string: "postgresql://baduser:badpass@localhost:5432/baddb".into(),
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
        connection_string: "postgresql://postgres@localhost:5432/testdb".into(),
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
// ============================================================================
// Cross-Driver Tests
// ============================================================================

#[cfg(all(feature = "mysql", feature = "postgres"))]
#[tokio::test]
#[ignore] // Requires both MySQL and PostgreSQL instances
async fn test_cross_driver_runtime_switching() {
    use rsbench::driver::DriverRegistry;

    let registry = DriverRegistry::new();

    // Test 1: Verify both drivers are available
    let mysql_driver = registry.get("mysql").expect("MySQL driver should be available");
    let postgres_driver = registry.get("postgres").expect("PostgreSQL driver should be available");

    assert_eq!(mysql_driver.name(), "mysql");
    assert_eq!(postgres_driver.name(), "postgres");

    println!("✅ Both drivers registered successfully");

    // Test 2: Create connections to both databases
    let mysql_config = ConnectionConfig {
        connection_string: "mysql://root@localhost:4000/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    let postgres_config = ConnectionConfig {
        connection_string: "postgresql://postgres@localhost:5432/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    let mut mysql_conn = mysql_driver.connect(&mysql_config).await
        .expect("MySQL connection should succeed");
    let mut postgres_conn = postgres_driver.connect(&postgres_config).await
        .expect("PostgreSQL connection should succeed");

    println!("✅ Both database connections established");

    // Test 3: Execute queries on both databases
    mysql_conn.ping().await.expect("MySQL ping should succeed");
    postgres_conn.ping().await.expect("PostgreSQL ping should succeed");

    println!("✅ Both databases responding to ping");

    // Test 4: Create test tables on both databases
    mysql_conn.execute("DROP TABLE IF EXISTS cross_driver_test", &[])
        .await.expect("MySQL DROP should succeed");
    mysql_conn.execute(
        "CREATE TABLE cross_driver_test (id INT PRIMARY KEY, name VARCHAR(100))",
        &[]
    ).await.expect("MySQL CREATE should succeed");

    postgres_conn.execute("DROP TABLE IF EXISTS cross_driver_test", &[])
        .await.expect("PostgreSQL DROP should succeed");
    postgres_conn.execute(
        "CREATE TABLE cross_driver_test (id INT PRIMARY KEY, name VARCHAR(100))",
        &[]
    ).await.expect("PostgreSQL CREATE should succeed");

    println!("✅ Test tables created on both databases");

    // Test 5: Insert data using different parameter styles
    let mysql_result = mysql_conn.execute(
        "INSERT INTO cross_driver_test (id, name) VALUES (?, ?)",
        &[Value::Int(1), Value::String("mysql_test".into())]
    ).await.expect("MySQL INSERT should succeed");
    assert_eq!(mysql_result.rows_affected, 1);

    let postgres_result = postgres_conn.execute(
        "INSERT INTO cross_driver_test (id, name) VALUES ($1, $2)",
        &[Value::Int(1), Value::String("postgres_test".into())]
    ).await.expect("PostgreSQL INSERT should succeed");
    assert_eq!(postgres_result.rows_affected, 1);

    println!("✅ Data inserted into both databases");

    // Test 6: Cleanup
    mysql_conn.execute("DROP TABLE cross_driver_test", &[])
        .await.expect("MySQL cleanup should succeed");
    postgres_conn.execute("DROP TABLE cross_driver_test", &[])
        .await.expect("PostgreSQL cleanup should succeed");

    println!("✅ Cross-driver runtime switching test passed");
}

#[cfg(all(feature = "mysql", feature = "postgres"))]
#[tokio::test]
#[ignore] // Requires both MySQL and PostgreSQL instances
async fn test_cross_driver_transaction_compatibility() {
    use rsbench::driver::DriverRegistry;

    let registry = DriverRegistry::new();

    let mysql_driver = registry.get("mysql").unwrap();
    let postgres_driver = registry.get("postgres").unwrap();

    let mysql_config = ConnectionConfig {
        connection_string: "mysql://root@localhost:4000/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    let postgres_config = ConnectionConfig {
        connection_string: "postgresql://postgres@localhost:5432/testdb".into(),
        timeout: Duration::from_secs(5),
    };

    let mut mysql_conn = mysql_driver.connect(&mysql_config).await.unwrap();
    let mut postgres_conn = postgres_driver.connect(&postgres_config).await.unwrap();

    // Setup test tables
    mysql_conn.execute("DROP TABLE IF EXISTS txn_test", &[]).await.unwrap();
    mysql_conn.execute("CREATE TABLE txn_test (id INT PRIMARY KEY)", &[]).await.unwrap();

    postgres_conn.execute("DROP TABLE IF EXISTS txn_test", &[]).await.unwrap();
    postgres_conn.execute("CREATE TABLE txn_test (id INT PRIMARY KEY)", &[]).await.unwrap();

    // Test transactions on both databases
    mysql_conn.begin().await.unwrap();
    postgres_conn.begin().await.unwrap();

    mysql_conn.execute("INSERT INTO txn_test (id) VALUES (?)", &[Value::Int(1)])
        .await.unwrap();
    postgres_conn.execute("INSERT INTO txn_test (id) VALUES ($1)", &[Value::Int(1)])
        .await.unwrap();

    mysql_conn.commit().await.unwrap();
    postgres_conn.commit().await.unwrap();

    println!("✅ Transactions work on both drivers");

    // Cleanup
    mysql_conn.execute("DROP TABLE txn_test", &[]).await.unwrap();
    postgres_conn.execute("DROP TABLE txn_test", &[]).await.unwrap();

    println!("✅ Cross-driver transaction compatibility test passed");
}
