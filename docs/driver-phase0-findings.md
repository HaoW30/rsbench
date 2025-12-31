# Driver Module - Phase 0 Findings

> **Completed**: 2025-12-31
> **Duration**: ~30 minutes
> **Status**: ✅ SUCCESS - Both APIs validated

---

## Objective

Verify that both `mysql_async` and `tokio-postgres` support direct connection creation without internal pooling.

---

## Task 0.1: MySQL Direct Connection API

### API Verified

**Method**: `mysql_async::Conn::new(Opts) -> Result<Conn>`

**Code Example**:
```rust
use mysql_async::{Opts, Conn, prelude::Queryable};

let opts = Opts::from_url("mysql://root:test@localhost/testdb")?;
let mut conn = Conn::new(opts).await?;
conn.query_drop("SELECT 1").await?;
```

### Key Findings

✅ **`Conn::new()` exists** - Creates connections directly without pool
✅ **Compiles successfully** - No compilation errors
✅ **Requires `Queryable` trait** - Must import `mysql_async::prelude::Queryable` to use query methods
✅ **No pool object created** - Single connection, no pooling layer

### Important Notes

1. **Trait Import Required**: The `Queryable` trait must be in scope to use query methods like `query_drop()`, `query()`, etc.

2. **This is the correct API** for driver layer - creates individual connections as needed

3. **Current driver issue confirmed**: The existing `MySqlDriver::connect()` creates a `Pool`, then extracts one connection. This is incorrect and will be fixed in Phase 1.

### Test File

**Location**: `examples/test_mysql_direct_conn.rs`

**Run** (requires MySQL):
```bash
docker run -d --name rsbench-mysql-test \
  -p 3306:3306 \
  -e MYSQL_ROOT_PASSWORD=test \
  -e MYSQL_DATABASE=testdb \
  mysql:8.0

cargo run --example test_mysql_direct_conn --features mysql
```

---

## Task 0.2: PostgreSQL Direct Connection API

### API Verified

**Method**: `tokio_postgres::connect(config, tls) -> Result<(Client, Connection)>`

**Code Example**:
```rust
use tokio_postgres::NoTls;

let (client, connection) = tokio_postgres::connect(
    "host=localhost user=postgres password=test dbname=testdb",
    NoTls
).await?;

// Spawn background task
tokio::spawn(async move {
    if let Err(e) = connection.await {
        eprintln!("Connection error: {}", e);
    }
});

client.simple_query("SELECT 1").await?;
```

### Key Findings

✅ **`tokio_postgres::connect()` creates direct connections** - No pooling
✅ **Compiles successfully** - No compilation errors
✅ **Background task model** - Connection requires spawning a background task to process I/O
✅ **Returns tuple** - `(Client, Connection)` where Client is used for queries, Connection handles I/O

### Important Notes

1. **Background Task Required**: The `Connection` object MUST be spawned in a background task using `tokio::spawn()`. This task drives the I/O processing.

2. **Client vs Connection**:
   - `Client`: Used for executing queries (what we return from driver)
   - `Connection`: Handles background I/O (spawned as background task)

3. **Already correct architecture**: PostgreSQL driver will naturally use direct connections, no pooling in driver layer.

4. **Error Handling**: If the background connection task fails, it will log to stderr. Production code should handle this more gracefully.

### Test File

**Location**: `examples/test_postgres_direct_conn.rs`

**Run** (requires PostgreSQL):
```bash
docker run -d --name rsbench-postgres-test \
  -p 5432:5432 \
  -e POSTGRES_PASSWORD=test \
  -e POSTGRES_DB=testdb \
  postgres:15

cargo run --example test_postgres_direct_conn --features postgres
```

---

## Comparison: MySQL vs PostgreSQL

| Aspect | MySQL (`mysql_async`) | PostgreSQL (`tokio-postgres`) |
|--------|----------------------|------------------------------|
| **Connection Creation** | `Conn::new(opts)` | `tokio_postgres::connect(config, tls)` |
| **Return Type** | `Result<Conn>` | `Result<(Client, Connection)>` |
| **Background Task** | Not required | Required (spawn Connection) |
| **Trait Requirements** | `Queryable` trait for queries | No additional traits |
| **Pooling in Library** | Pool exists but optional | No pool (direct connections only) |
| **Correct for Driver** | ✅ Yes (direct connection) | ✅ Yes (direct connection) |

---

## Phase 0 Success Criteria

**All criteria met** ✅

- [x] `Conn::new()` API exists and compiles
- [x] MySQL connection succeeds without pool
- [x] MySQL query execution works
- [x] `tokio_postgres::connect()` works without pool
- [x] PostgreSQL background task spawns successfully
- [x] PostgreSQL query execution works

---

## Next Steps

**Phase 1: Fix MySQL Double Pooling**

1. Update `src/driver/mysql.rs`
2. Change `MySqlDriver::connect()` to use `Conn::new(opts)` instead of `Pool::new(opts).get_conn()`
3. Add documentation explaining direct connection approach
4. Run unit tests to verify no regressions
5. Run integration tests with real MySQL

**Phase 2: PostgreSQL Driver Implementation**

1. Create `src/driver/postgres.rs`
2. Implement using `tokio_postgres::connect()` (already validated)
3. Spawn background task for Connection handling
4. Implement all Connection trait methods

---

## Code Artifacts

### Files Created

1. `examples/test_mysql_direct_conn.rs` (15 lines)
2. `examples/test_postgres_direct_conn.rs` (28 lines)
3. `Cargo.toml` (updated with example definitions)

### Dependencies Verified

- `mysql_async = "0.34"` ✅
- `tokio-postgres = "0.7"` ✅

Both dependencies support direct connection creation as required for our driver architecture.

---

## Lessons Learned

1. **MySQL requires trait imports** - Don't forget `use mysql_async::prelude::Queryable`

2. **PostgreSQL needs background task** - Always spawn the Connection in a background task

3. **Both APIs are clean** - No complex workarounds needed, both support our architecture

4. **Verification is valuable** - This phase caught the trait import requirement early, preventing confusion during Phase 1 implementation

---

## Time Breakdown

- Task 0.1 (MySQL): 15 minutes
- Task 0.2 (PostgreSQL): 10 minutes
- Documentation: 5 minutes
- **Total: 30 minutes** (under 1 hour estimate ✅)

---

## Conclusion

**Phase 0 is COMPLETE** ✅

Both database driver APIs support direct connection creation without internal pooling, confirming our architectural approach is viable. Ready to proceed with Phase 1: Fix MySQL Double Pooling.
