use tokio_postgres::NoTls;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Test: Create connection directly (no pool)
    println!("Creating PostgreSQL connection directly (no pool)...");
    let (client, connection) = tokio_postgres::connect(
        "host=localhost user=postgres password=test dbname=testdb",
        NoTls
    ).await?;

    // Spawn background task to process connection
    println!("Spawning background connection task...");
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("Connection error: {}", e);
        }
    });

    // Verify connection works
    println!("Executing test query...");
    client.simple_query("SELECT 1").await?;

    println!("✅ PostgreSQL direct connection works!");
    println!("✅ tokio_postgres::connect() API creates connections without pooling");
    println!("✅ Background task model works correctly");

    Ok(())
}
