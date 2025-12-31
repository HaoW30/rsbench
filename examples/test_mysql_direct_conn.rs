use mysql_async::{Opts, Conn, prelude::Queryable};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opts = Opts::from_url("mysql://root:test@localhost/testdb")?;

    // Test: Create connection directly without pool
    println!("Creating MySQL connection directly (no pool)...");
    let mut conn = Conn::new(opts).await?;

    // Verify connection works
    println!("Executing test query...");
    conn.query_drop("SELECT 1").await?;

    println!("✅ MySQL direct connection works!");
    println!("✅ Conn::new() API exists and creates connections without pooling");

    Ok(())
}
