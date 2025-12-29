//! MySQL Connection Pool Integration Tests
//!
//! Tests real connection pooling behavior with MySQL database.
//! These tests require a running MySQL instance.

use rsbench::config::PoolConfig;
use rsbench::driver::MySqlDriver;
use rsbench::pool::ConnectionPool;
use rsbench::Value;
use std::sync::Arc;
use std::time::Duration;

/// Get MySQL connection string from environment or use default
fn get_mysql_url() -> String {
    std::env::var("MYSQL_URL").unwrap_or_else(|_| "mysql://root@localhost:3306/test".to_string())
}

/// Skip test if MySQL is not available
fn mysql_available() -> bool {
    std::env::var("SKIP_MYSQL_TESTS").is_err()
}

#[tokio::test]
#[ignore] // Run with --ignored flag when MySQL is available
async fn test_mysql_real_connection_pooling() {
    if !mysql_available() {
        println!("Skipping MySQL test - set MYSQL_URL to enable");
        return;
    }

    let driver = Arc::new(MySqlDriver::new());
    let config = PoolConfig {
        max_size: 5,
        min_size: 2,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(600),
    };

    let pool = ConnectionPool::new(driver, get_mysql_url(), config)
        .expect("Failed to create pool");

    // Pre-warm the pool
    pool.warm_up().await.expect("Failed to warm up pool");

    // Verify pool stats show warmed connections
    let stats = pool.stats();
    assert!(stats.total_connections >= 2, "Pool should have at least min_size connections");

    // Test connection checkout and query execution
    let mut conn = pool.get().await.expect("Failed to get connection");
    let result = conn.execute("SELECT 1", &[]).await.expect("Query failed");

    assert_eq!(result.rows_affected, 0); // SELECT doesn't affect rows
    drop(conn);

    // Connection should be returned to pool
    let stats_after = pool.stats();
    assert_eq!(stats_after.active_connections, 0, "Connection should be returned to pool");
}

#[tokio::test]
#[ignore]
async fn test_mysql_connection_reuse() {
    if !mysql_available() {
        return;
    }

    let driver = Arc::new(MySqlDriver::new());
    let config = PoolConfig {
        max_size: 3,
        min_size: 1,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(600),
    };

    let pool = ConnectionPool::new(driver, get_mysql_url(), config)
        .expect("Failed to create pool");

    pool.warm_up().await.expect("Failed to warm up");

    // Get and release a connection multiple times
    for i in 0..10 {
        let mut conn = pool.get().await.expect("Failed to get connection");
        let query = format!("SELECT {}", i);
        conn.execute(&query, &[]).await.expect("Query failed");
        drop(conn);
    }

    // Pool should have reused connections (total shouldn't grow much)
    let stats = pool.stats();
    assert!(
        stats.total_connections <= 3,
        "Pool should reuse connections, not create new ones. Total: {}",
        stats.total_connections
    );
}

#[tokio::test]
#[ignore]
async fn test_mysql_concurrent_access() {
    if !mysql_available() {
        return;
    }

    let driver = Arc::new(MySqlDriver::new());
    let config = PoolConfig {
        max_size: 10,
        min_size: 3,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(600),
    };

    let pool = Arc::new(
        ConnectionPool::new(driver, get_mysql_url(), config).expect("Failed to create pool"),
    );

    pool.warm_up().await.expect("Failed to warm up");

    // Spawn multiple concurrent tasks
    let mut handles = vec![];
    for i in 0..20 {
        let pool_clone = pool.clone();
        let handle = tokio::spawn(async move {
            let mut conn = pool_clone.get().await.expect("Failed to get connection");
            let query = format!("SELECT {} + {}", i, i);
            conn.execute(&query, &[]).await.expect("Query failed");
            // Small delay to simulate work
            tokio::time::sleep(Duration::from_millis(10)).await;
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // All connections should be returned
    let stats = pool.stats();
    assert_eq!(
        stats.active_connections, 0,
        "All connections should be returned. Active: {}",
        stats.active_connections
    );
    assert!(
        stats.total_connections <= 10,
        "Pool should not exceed max_size. Total: {}",
        stats.total_connections
    );
}

#[tokio::test]
#[ignore]
async fn test_mysql_pool_exhaustion() {
    if !mysql_available() {
        return;
    }

    let driver = Arc::new(MySqlDriver::new());
    let config = PoolConfig {
        max_size: 2, // Small pool for testing exhaustion
        min_size: 1,
        connection_timeout: Duration::from_secs(1), // Short timeout
        idle_timeout: Duration::from_secs(600),
    };

    let pool = Arc::new(
        ConnectionPool::new(driver, get_mysql_url(), config).expect("Failed to create pool"),
    );

    // Checkout all connections and hold them
    let _conn1 = pool.get().await.expect("Failed to get first connection");
    let _conn2 = pool.get().await.expect("Failed to get second connection");

    // Pool should be exhausted
    let stats = pool.stats();
    assert_eq!(stats.active_connections, 2, "All connections should be active");
    assert_eq!(stats.idle_connections, 0, "No connections should be idle");

    // Try to get another connection - should timeout
    let result = tokio::time::timeout(Duration::from_millis(500), pool.get()).await;

    assert!(
        result.is_err(),
        "Getting connection from exhausted pool should timeout"
    );
}

#[tokio::test]
#[ignore]
async fn test_mysql_health_check() {
    if !mysql_available() {
        return;
    }

    let driver = Arc::new(MySqlDriver::new());
    let config = PoolConfig {
        max_size: 3,
        min_size: 1,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(600),
    };

    let pool = ConnectionPool::new(driver, get_mysql_url(), config)
        .expect("Failed to create pool");

    // Get a connection, use it, return it
    {
        let mut conn = pool.get().await.expect("Failed to get connection");
        conn.execute("SELECT 1", &[]).await.expect("Query failed");
    }

    // Get another connection - pool should health check before returning
    let mut conn2 = pool.get().await.expect("Failed to get connection");
    let result = conn2.execute("SELECT 2", &[]).await;
    assert!(result.is_ok(), "Health checked connection should work");

    // Verify pool health metrics
    let stats = pool.stats();
    assert_eq!(stats.connection_errors, 0, "Should have no connection errors");
    assert!(stats.total_checkouts >= 2, "Should have tracked checkouts");
}

#[tokio::test]
#[ignore]
async fn test_mysql_transactions() {
    if !mysql_available() {
        return;
    }

    let driver = Arc::new(MySqlDriver::new());
    let config = PoolConfig {
        max_size: 3,
        min_size: 1,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(600),
    };

    let pool = ConnectionPool::new(driver, get_mysql_url(), config)
        .expect("Failed to create pool");

    // Test transaction support
    let mut conn = pool.get().await.expect("Failed to get connection");

    // Begin transaction
    conn.begin().await.expect("Failed to begin transaction");

    // Execute query in transaction
    conn.execute("SELECT 1", &[])
        .await
        .expect("Failed to execute in transaction");

    // Commit transaction
    conn.commit().await.expect("Failed to commit transaction");

    // Rollback test
    conn.begin().await.expect("Failed to begin second transaction");
    conn.execute("SELECT 2", &[])
        .await
        .expect("Failed to execute in second transaction");
    conn.rollback()
        .await
        .expect("Failed to rollback transaction");

    drop(conn);

    // Connection should be returned healthy
    let stats = pool.stats();
    assert_eq!(stats.connection_errors, 0, "Transactions should not cause errors");
}

#[tokio::test]
#[ignore]
async fn test_mysql_prepared_statements() {
    if !mysql_available() {
        return;
    }

    let driver = Arc::new(MySqlDriver::new());
    let config = PoolConfig {
        max_size: 3,
        min_size: 1,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(600),
    };

    let pool = ConnectionPool::new(driver, get_mysql_url(), config)
        .expect("Failed to create pool");

    let mut conn = pool.get().await.expect("Failed to get connection");

    // Test parameterized query
    let params = vec![Value::Int(42)];
    let result = conn.execute("SELECT ?", &params).await;

    assert!(result.is_ok(), "Prepared statement should work");

    // Test multiple parameters
    let params2 = vec![Value::Int(1), Value::Int(2)];
    let result2 = conn.execute("SELECT ? + ?", &params2).await;

    assert!(result2.is_ok(), "Multi-param prepared statement should work");
}

#[tokio::test]
#[ignore]
async fn test_mysql_pool_stats_accuracy() {
    if !mysql_available() {
        return;
    }

    let driver = Arc::new(MySqlDriver::new());
    let config = PoolConfig {
        max_size: 5,
        min_size: 2,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(600),
    };

    let pool = ConnectionPool::new(driver, get_mysql_url(), config)
        .expect("Failed to create pool");

    pool.warm_up().await.expect("Failed to warm up");

    let initial_stats = pool.stats();
    assert!(initial_stats.total_connections >= 2, "Should have min_size connections");
    assert_eq!(initial_stats.active_connections, 0, "No connections should be active");

    // Checkout connections and verify stats
    let _conn1 = pool.get().await.expect("Failed to get connection 1");
    let stats1 = pool.stats();
    assert_eq!(stats1.active_connections, 1, "One connection should be active");

    let _conn2 = pool.get().await.expect("Failed to get connection 2");
    let stats2 = pool.stats();
    assert_eq!(stats2.active_connections, 2, "Two connections should be active");

    drop(_conn1);
    let stats3 = pool.stats();
    assert_eq!(stats3.active_connections, 1, "One connection returned");

    drop(_conn2);
    let stats4 = pool.stats();
    assert_eq!(stats4.active_connections, 0, "All connections returned");

    // Verify health metrics
    assert!(stats4.total_checkouts >= 2, "Should track total checkouts");
    assert_eq!(stats4.connection_errors, 0, "Should have no errors");
    assert!(stats4.max_lifetime.is_some(), "Should report max lifetime");
}
