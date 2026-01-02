# Error Recovery Strategies - Proposal

## Current Situation

**Problem**: We classify errors but don't take action based on classification

```rust
if is_recoverable_error(&e) {
    // No action - just log
} else {
    eprintln!("[MySQL] Non-recoverable error detected");
    // Still no action!
}
```

---

## Proposed Solutions

### Option 1: Explicit Connection Reset for Recoverable Errors

**Idea**: After recoverable errors, explicitly reset connection state

```rust
// In src/driver/mysql.rs
async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult> {
    let result = self.conn.exec_drop(sql, mysql_params).await;

    match &result {
        Err(e) if is_recoverable_error(e) => {
            // Error happened but connection should be OK
            // Explicitly verify connection is still usable
            if let Err(ping_err) = self.conn.ping().await {
                eprintln!("[MySQL] Connection broken after recoverable error!");
                eprintln!("  Original error: {}", e);
                eprintln!("  Ping failed: {}", ping_err);

                // Return I/O error so health check will discard this connection
                return Err(DatabaseError::Connection(format!(
                    "Connection broken after error: {}", ping_err
                )));
            }
            // Ping succeeded, connection is fine, just return the original error
        }
        Err(e) => {
            // Non-recoverable error, connection is likely broken
            // Health check will catch this on recycle
        }
        Ok(_) => {
            // Success, no action needed
        }
    }

    // ... rest of function
}
```

**Pros**:
- Proactively detects if "recoverable" errors actually broke the connection
- Prevents returning broken connections to pool
- Adds only ~1ms overhead per error (ping is fast)

**Cons**:
- Adds latency to error path (but errors are already slow)
- May be unnecessary if mysql_async handles this correctly

---

### Option 2: Track Error Rate per Connection

**Idea**: If a connection has too many errors, discard it even if errors are "recoverable"

```rust
pub struct MySqlConnection {
    conn: Conn,
    error_count: AtomicUsize,  // NEW
    error_threshold: usize,     // NEW: e.g., 10 errors = discard
}

async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult> {
    let result = self.conn.exec_drop(sql, mysql_params).await;

    if result.is_err() {
        let errors = self.error_count.fetch_add(1, Ordering::Relaxed) + 1;

        if errors >= self.error_threshold {
            eprintln!("[MySQL] Connection error threshold exceeded ({} errors)", errors);
            eprintln!("  Marking connection as broken for safety");

            // Force connection to fail health check
            return Err(DatabaseError::Connection(format!(
                "Connection has {} errors, exceeds threshold of {}",
                errors, self.error_threshold
            )));
        }
    } else {
        // Success - reset error counter
        self.error_count.store(0, Ordering::Relaxed);
    }

    // ... continue
}
```

**Pros**:
- Prevents "death by a thousand cuts" - one bad connection with many errors
- Configurable threshold
- Automatic recovery when connection succeeds

**Cons**:
- May discard connections unnecessarily if errors are transient
- Adds memory overhead (atomic counter per connection)

---

### Option 3: Fail-Fast on Critical Errors

**Idea**: For certain error types, immediately mark connection as broken

```rust
fn is_connection_fatal(e: &mysql_async::Error) -> bool {
    use mysql_async::Error;

    match e {
        // These definitely mean connection is dead
        Error::Io(_) => true,

        // Server gone away
        Error::Server(ref err) if err.code == 2006 => true,

        // Connection lost during query
        Error::Server(ref err) if err.code == 2013 => true,

        // All other errors: connection probably OK
        _ => false,
    }
}

async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult> {
    let result = self.conn.exec_drop(sql, mysql_params).await;

    if let Err(ref e) = result {
        if is_connection_fatal(e) {
            // Explicitly close connection so health check will fail
            drop(std::mem::replace(&mut self.conn, /* create dummy conn */));

            return Err(DatabaseError::Connection(format!(
                "Fatal connection error: {}", e
            )));
        }
    }

    // ... continue
}
```

**Pros**:
- Fast detection of truly broken connections
- No overhead for normal operations
- Based on MySQL error codes (well-defined)

**Cons**:
- Need to handle all MySQL error codes correctly
- May miss edge cases

---

## Recommendation

**Implement Option 1 (Connection Reset) + Option 3 (Fatal Error Detection)**

Combined approach:

```rust
async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult> {
    let result = self.conn.exec_drop(sql, mysql_params).await;

    match &result {
        Err(e) => {
            // Check if this is a fatal connection error
            if is_connection_fatal(e) {
                eprintln!("[MySQL] FATAL connection error: {}", e);
                return Err(DatabaseError::Connection(format!(
                    "Fatal error, connection broken: {}", e
                )));
            }

            // For recoverable errors, verify connection still works
            if is_recoverable_error(e) {
                // Ping to ensure connection is still alive
                if let Err(ping_err) = self.conn.ping().await {
                    eprintln!("[MySQL] Connection broken after recoverable error");
                    eprintln!("  Query error: {}", e);
                    eprintln!("  Ping failed: {}", ping_err);

                    return Err(DatabaseError::Connection(format!(
                        "Connection broken: {}", ping_err
                    )));
                }
                // Ping succeeded, connection is fine
            }
        }
        Ok(_) => {
            // Success
        }
    }

    result.map_err(|e| DatabaseError::Query(e.to_string()))?;

    Ok(QueryResult {
        rows_affected: self.conn.affected_rows(),
        last_insert_id: self.conn.last_insert_id(),
    })
}
```

**Why this combination**:
1. Fatal errors → immediate detection, no wasted retries
2. Recoverable errors → verify connection still works
3. Minimal overhead (ping only on errors, not on success path)
4. Defensive: catches edge cases where "recoverable" errors actually break connection

---

## Testing Strategy

1. **Test Driver errors** (parameter mismatch):
   - Should be caught by ping, connection stays alive
   - Or prevented entirely by workload parameter fix

2. **Test Server errors** (SQL syntax, constraints):
   - Should not break connection
   - Verify with manual testing

3. **Test I/O errors** (network disconnect):
   - Should be immediately detected as fatal
   - Connection discarded immediately

4. **Test high error rate** (1000s of errors/sec):
   - Verify connections don't accumulate errors
   - Health checks eventually catch broken connections

---

## Implementation Priority

**Must Have**:
- ✅ Workload parameter fix (already done - prevents Driver errors)
- ✅ Error classification (already done - identifies error types)
- ✅ Health checks enabled (already done - catches broken connections)

**Should Have** (implement next):
- ⬜ Fatal error detection (Option 3) - fast path for known-broken connections
- ⬜ Post-error ping verification (Option 1) - defensive check

**Nice to Have** (future):
- ⬜ Per-connection error tracking (Option 2) - advanced monitoring
- ⬜ Configurable error thresholds
- ⬜ Per-error-type metrics

---

## Conclusion

**Current state**:
- Parameter fix prevents most errors
- Health checks catch broken connections (with 1 operation delay)
- Error classification provides visibility

**Recommended improvements**:
- Add fatal error detection for immediate action
- Add post-error ping for defensive verification
- Keep as simple as possible - don't over-engineer

The key insight: **Prevention (parameter fix) is better than recovery (error handling)**. We should keep error handling simple and focus on preventing errors in the first place.
