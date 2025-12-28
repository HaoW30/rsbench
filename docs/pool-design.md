# Connection Pool Module Design Document

**Module:** Connection Pool
**Version:** 1.0
**Status:** Design + Implementation Planning
**Last Updated:** 2025-12-27

---

## Table of Contents

1. [Overview](#overview)
2. [Current State Analysis](#current-state-analysis)
3. [Design Goals](#design-goals)
4. [Architecture](#architecture)
5. [Detailed Design](#detailed-design)
6. [Implementation Plan](#implementation-plan)
7. [Testing Strategy](#testing-strategy)
8. [Performance Targets](#performance-targets)
9. [Future Enhancements](#future-enhancements)

---

## Overview

### Purpose

The Connection Pool module manages database connections for RSBench, providing:

1. **Efficient connection reuse** - Avoid overhead of creating new connections
2. **Concurrency control** - Limit concurrent database connections
3. **Health monitoring** - Detect and replace unhealthy connections
4. **Backpressure visibility** - Track pool utilization for runtime monitoring
5. **Resource cleanup** - Graceful connection lifecycle management

### Design Philosophy

```
┌─────────────────────────────────────────────────────────┐
│  "The pool is a shared resource - its health MUST be   │
│   visible to the runtime for backpressure detection"    │
└─────────────────────────────────────────────────────────┘
```

**Core Principles:**
1. **Observable Health** - Pool stats expose utilization for backpressure monitoring
2. **Fail Fast** - Connection failures surface immediately, not hidden
3. **Simple for M0** - Start without pooling, add complexity when needed
4. **Production-Ready for M1** - Use battle-tested deadpool library

---

## Current State Analysis

### What We Have (M0)

**File Structure:**
```
src/pool/
└── mod.rs              # ConnectionPool + PooledConnection
```

**Key Components:**

1. **ConnectionPool** (`mod.rs:11-55`)
```rust
pub struct ConnectionPool {
    driver: Arc<dyn DatabaseDriver>,
    config: PoolConfig,
    connection_string: String,
}

impl ConnectionPool {
    pub async fn get(&self) -> Result<PooledConnection> {
        // M0: Creates NEW connection every time
        let conn = self.driver.connect(&conn_config).await?;
        Ok(PooledConnection { inner: conn })
    }

    pub fn stats(&self) -> PoolStats {
        // M0: Returns fake stats
        PoolStats {
            total_connections: self.config.max_size,
            active_connections: 0,  // Always 0!
            idle_connections: self.config.max_size,
            pending_requests: 0,
        }
    }
}
```

2. **PooledConnection** (`mod.rs:58-76`)
```rust
pub struct PooledConnection {
    inner: Box<dyn Connection>,
}
```

3. **PoolStats** (`mod.rs:79-85`)
```rust
pub struct PoolStats {
    pub total_connections: usize,
    pub active_connections: usize,
    pub idle_connections: usize,
    pub pending_requests: usize,
}
```

### What Works Well ✅

1. **Clean interface** - Simple API (`get()` and `stats()`)
2. **Driver agnostic** - Works with any DatabaseDriver implementation
3. **Async-first** - Non-blocking connection acquisition
4. **Stats structure** - Correct shape for backpressure monitoring

### Critical Problems 🔧

| Issue | Impact | Priority |
|-------|--------|----------|
| **No actual pooling** | Creates new connection every time → HIGH latency | CRITICAL |
| **No connection reuse** | Wastes resources, slow performance | CRITICAL |
| **Fake stats** | Runtime can't detect pool saturation | HIGH |
| **No health checking** | Broken connections remain in pool | MEDIUM |
| **No connection limits** | Can exhaust database connections | HIGH |
| **No idle timeout** | Connections never cleaned up | LOW |
| **No prepared statement cache** | Repeated parsing overhead | LOW (M1) |

### Performance Impact (M0 vs Target)

**Current M0 behavior:**
- **Connection creation**: ~10-50ms (MySQL TCP handshake + auth)
- **Total latency per operation**: `connection_time + query_time`
- **Example**: 20ms connection + 5ms query = **25ms total** ⚠️

**Target M1 behavior (with pooling):**
- **Connection checkout**: ~1-10μs (from pool)
- **Total latency**: `checkout_time + query_time`
- **Example**: 5μs checkout + 5ms query = **5.005ms total** ✅

**Improvement**: **5x faster** for fast queries

---

## Design Goals

### Milestone 0 (Current) - ACCEPTABLE FOR DEVELOPMENT

**Goal:** Simple pass-through, good enough for initial testing

**Acceptance Criteria:**
- [x] Compiles and works with DatabaseDriver
- [x] Provides stats interface (even if fake)
- [x] Non-blocking async API
- [ ] Basic unit tests

**Known Limitations:**
- No actual pooling (acceptable for M0 development)
- Fake stats (acceptable - runtime still being developed)
- Poor performance (acceptable for initial testing)

**When M0 is no longer acceptable:**
- When running real benchmarks (need realistic performance)
- When testing backpressure (need real pool utilization)
- When measuring database performance (connection overhead distorts results)

### Milestone 1 (Next) - PRODUCTION-READY

**Goal:** Production-grade connection pooling with health monitoring

**Success Criteria:**
- [ ] Real connection pooling (reuse connections)
- [ ] Accurate stats (track actual utilization)
- [ ] Connection health checking
- [ ] Idle connection cleanup
- [ ] Connection limits enforced
- [ ] Checkout timeout support
- [ ] Graceful shutdown
- [ ] Comprehensive tests (80%+ coverage)
- [ ] Performance benchmarks

**Out of Scope (M1):**
- Prepared statement caching (M2)
- Connection load balancing (M2)
- Multi-endpoint pooling (M2)
- Circuit breakers (M2)

### Milestone 2 (Future)

**Enhanced Features:**
- Prepared statement cache
- Multi-endpoint support (read replicas)
- Connection affinity
- Advanced health checks

---

## Architecture

### High-Level Design

```
┌─────────────────────────────────────────────────────────┐
│                  AsyncRuntime                           │
│                                                          │
│  Needs connection → pool.get().await                    │
└─────────────────────┬───────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────┐
│              ConnectionPool (M1)                        │
│                                                          │
│  ┌───────────────────────────────────────────┐         │
│  │  deadpool::managed::Pool<DriverManager>   │         │
│  │                                            │         │
│  │  ┌──────────┐  ┌──────────┐  ┌─────────┐ │         │
│  │  │ Conn #1  │  │ Conn #2  │  │ Conn #N │ │         │
│  │  │ (idle)   │  │ (active) │  │ (idle)  │ │         │
│  │  └──────────┘  └──────────┘  └─────────┘ │         │
│  └───────────────────────────────────────────┘         │
│                                                          │
│  Stats:                                                 │
│  - total_connections: N                                 │
│  - active_connections: 1                                │
│  - idle_connections: N-1                                │
│  - pending_requests: 0                                  │
└──────────────────┬──────────────────────────────────────┘
                   │
                   ▼
         ┌─────────────────┐
         │ DatabaseDriver  │
         │  (MySQL/PG)     │
         └─────────────────┘
```

### Component Interactions

```
┌──────────────┐      get()       ┌─────────────────┐
│   Runtime    │ ───────────────> │ ConnectionPool  │
└──────────────┘                   │                 │
                                   │ (deadpool)      │
                                   └────────┬────────┘
                                            │
                    ┌───────────────────────┼───────────────────┐
                    │                       │                   │
                    ▼                       ▼                   ▼
            ┌───────────────┐     ┌───────────────┐   ┌───────────────┐
            │  Connection   │     │  Connection   │   │  Connection   │
            │  (pooled)     │     │  (pooled)     │   │  (pooled)     │
            └───────┬───────┘     └───────┬───────┘   └───────┬───────┘
                    │                     │                   │
                    └─────────────────────┼───────────────────┘
                                          ▼
                                 ┌─────────────────┐
                                 │ DatabaseDriver  │
                                 └─────────────────┘
```

### Deadpool Integration Strategy

**Why deadpool?**
- Battle-tested (used in production across Rust ecosystem)
- Async-first design (perfect for tokio)
- Generic over connection type (works with our DatabaseDriver)
- Health checking built-in
- Timeout support
- Connection lifecycle management

**Integration approach:**
```rust
// M1: Use deadpool's managed pool
type Pool = deadpool::managed::Pool<DriverManager>;

// Custom manager implementing deadpool::managed::Manager
struct DriverManager {
    driver: Arc<dyn DatabaseDriver>,
    connection_string: String,
}

#[async_trait]
impl deadpool::managed::Manager for DriverManager {
    type Type = Box<dyn Connection>;
    type Error = crate::Error;

    async fn create(&self) -> Result<Box<dyn Connection>> {
        // Delegate to our DatabaseDriver
        self.driver.connect(&config).await
    }

    async fn recycle(&self, conn: &mut Box<dyn Connection>) -> RecycleResult<Error> {
        // Health check: ping the connection
        match conn.ping().await {
            Ok(_) => Ok(()),
            Err(e) => Err(RecycleError::Backend(e)),
        }
    }
}
```

---

## Detailed Design

### Component 1: ConnectionPool (M1 Implementation)

**Purpose:** Wrapper around deadpool providing our specific API

**Structure:**
```rust
pub struct ConnectionPool {
    // M1: Real pool using deadpool
    inner: deadpool::managed::Pool<DriverManager>,

    // Config for reference
    config: PoolConfig,
}

impl ConnectionPool {
    pub fn new(
        driver: Arc<dyn DatabaseDriver>,
        connection_string: String,
        config: PoolConfig,
    ) -> Result<Self> {
        // Create deadpool manager
        let manager = DriverManager {
            driver,
            connection_string: connection_string.clone(),
            timeout: config.connection_timeout,
        };

        // Build deadpool config
        let pool_config = deadpool::managed::PoolConfig::new(config.max_size);

        // Create pool
        let inner = pool_config.create_pool(Some(Runtime::Tokio1), manager)?;

        Ok(Self { inner, config })
    }

    pub async fn get(&self) -> Result<PooledConnection> {
        // M1: Get from pool with timeout
        let conn = self.inner
            .timeout_get(&self.config.connection_timeout)
            .await?;

        Ok(PooledConnection {
            inner: conn,
        })
    }

    pub fn stats(&self) -> PoolStats {
        // M1: Real stats from deadpool
        let status = self.inner.status();

        PoolStats {
            total_connections: status.size,
            active_connections: status.size - status.available,
            idle_connections: status.available,
            pending_requests: status.waiting,
        }
    }
}
```

### Component 2: DriverManager

**Purpose:** Adapter between deadpool and our DatabaseDriver trait

**Implementation:**
```rust
struct DriverManager {
    driver: Arc<dyn DatabaseDriver>,
    connection_string: String,
    timeout: Duration,
}

#[async_trait]
impl deadpool::managed::Manager for DriverManager {
    type Type = Box<dyn Connection>;
    type Error = crate::Error;

    async fn create(&self) -> Result<Box<dyn Connection>> {
        let config = ConnectionConfig {
            connection_string: self.connection_string.clone(),
            timeout: self.timeout,
        };

        self.driver.connect(&config).await
    }

    async fn recycle(
        &self,
        conn: &mut Box<dyn Connection>,
        _metrics: &Metrics,
    ) -> RecycleResult<Error> {
        // Health check before reuse
        match conn.ping().await {
            Ok(_) => Ok(()),
            Err(e) => Err(RecycleError::Backend(e)),
        }
    }
}
```

### Component 3: PooledConnection

**No changes needed** - Already correct for M1:

```rust
pub struct PooledConnection {
    // M1: This wraps deadpool::managed::Object<DriverManager>
    inner: deadpool::managed::Object<DriverManager>,
}

impl PooledConnection {
    pub async fn execute(&mut self, sql: &str, params: &[Value])
        -> Result<QueryResult>
    {
        self.inner.execute(sql, params).await
    }

    pub async fn ping(&mut self) -> Result<()> {
        self.inner.ping().await
    }
}

// Drop returns connection to pool automatically
```

### Component 4: PoolStats

**No changes needed** - Already correct:

```rust
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_connections: usize,    // Pool capacity
    pub active_connections: usize,    // Currently in use
    pub idle_connections: usize,      // Available in pool
    pub pending_requests: usize,      // Waiting for connection
}
```

**Key metric:**
```rust
pool_utilization = active_connections / total_connections
```

Used by RuntimeEngine for backpressure detection.

---

## Implementation Plan

### Phase 0: Analysis & Documentation ✅ CURRENT

**Goal:** Understand current state, design M1 architecture

**Tasks:**
- [x] Analyze current M0 implementation
- [x] Identify gaps and problems
- [x] Design deadpool integration strategy
- [ ] Create this design document
- [ ] Review with stakeholders

**Deliverables:**
- This document

---

### Phase 1: Deadpool Integration (M1 Core)

**Goal:** Replace fake pooling with real deadpool-based implementation

**Tasks:**

1. **Add deadpool dependency** (`Cargo.toml`)
   ```toml
   [dependencies]
   deadpool = { version = "0.12", features = ["managed"] }
   ```

2. **Create DriverManager** (`src/pool/manager.rs`)
   - Implement `deadpool::managed::Manager` trait
   - Delegate `create()` to DatabaseDriver
   - Implement `recycle()` with health check (ping)
   - Add error conversion

3. **Update ConnectionPool** (`src/pool/mod.rs`)
   - Replace fake pool with `deadpool::managed::Pool<DriverManager>`
   - Update `new()` to build deadpool config
   - Update `get()` to use `pool.timeout_get()`
   - Update `stats()` to use real pool status

4. **Update PooledConnection** (`src/pool/mod.rs`)
   - Wrap `deadpool::managed::Object<DriverManager>`
   - Keep existing execute/ping methods
   - Ensure Drop returns to pool

5. **Update tests** (`src/pool/mod.rs`)
   - Test real connection reuse
   - Test pool exhaustion behavior
   - Test health checking
   - Test stats accuracy

**Success Criteria:**
- [ ] Pool reuses connections (verified in tests)
- [ ] Stats reflect actual utilization
- [ ] Health checks work (broken connections replaced)
- [ ] All existing tests still pass
- [ ] New pool-specific tests pass

**Testing Strategy:**
```rust
#[tokio::test]
async fn test_connection_reuse() {
    // Get connection, use it, return it
    // Get another connection
    // Verify same underlying connection (via mock tracking)
}

#[tokio::test]
async fn test_pool_exhaustion() {
    // Create pool with max_size=2
    // Checkout 2 connections
    // 3rd checkout should wait or timeout
}

#[tokio::test]
async fn test_health_check() {
    // Create pool with broken connection
    // Next checkout should detect failure and recreate
}
```

---

### Phase 2: Configuration & Limits (M1)

**Goal:** Proper configuration validation and enforcement

**Tasks:**

1. **Validate PoolConfig** (`src/config/mod.rs`)
   - Ensure `max_size >= min_size`
   - Ensure timeouts are reasonable
   - Add validation in Config::validate()

2. **Add connection limits**
   - Enforce max_size strictly
   - Implement min_size (pre-warm pool)
   - Add idle_timeout cleanup

3. **Timeout handling**
   - Connection acquisition timeout
   - Connection creation timeout
   - Idle connection timeout

**Success Criteria:**
- [ ] Invalid configs rejected
- [ ] Pool respects max_size limit
- [ ] Pool maintains min_size connections
- [ ] Idle connections cleaned up after timeout

---

### Phase 3: Health Monitoring & Observability (M1)

**Goal:** Proper health checking and metrics

**Tasks:**

1. **Enhanced health checking**
   - Ping before reuse (already in recycle())
   - Periodic health checks for idle connections
   - Connection age tracking

2. **Better stats**
   - Add `max_lifetime` to PoolStats
   - Add `connection_errors` counter
   - Add `total_checkouts` counter

3. **Logging**
   - Log connection creation
   - Log health check failures
   - Log pool saturation events

**Success Criteria:**
- [ ] Broken connections detected and replaced
- [ ] Stats include health metrics
- [ ] Pool events logged with tracing

---

### Phase 4: Performance Optimization (M1)

**Goal:** Ensure pool meets performance targets

**Tasks:**

1. **Implement benchmarks** (`benches/connection_pool_bench.rs`)
   - Connection checkout latency
   - Concurrent access throughput
   - Pool saturation behavior

2. **Optimize hot paths**
   - Minimize allocations in get()
   - Reduce lock contention
   - Fast path for available connections

3. **Load testing**
   - Sustained high throughput
   - Memory leak detection
   - Connection lifecycle validation

**Success Criteria:**
- [ ] Checkout latency <10μs (p50)
- [ ] Checkout latency <100μs (p99)
- [ ] Support 100K+ checkouts/sec
- [ ] No memory leaks under sustained load

---

### Phase 5: Integration Testing (M1)

**Goal:** End-to-end validation with real databases

**Tasks:**

1. **MySQL integration tests** (`tests/integration/pool_mysql_test.rs`)
   - Real MySQL connection pooling
   - Concurrent access patterns
   - Connection reuse verification

2. **PostgreSQL integration tests** (when Postgres driver ready)
   - Same as MySQL

3. **Runtime integration**
   - Verify runtime sees accurate pool stats
   - Verify backpressure detection works
   - Test pool exhaustion triggers backpressure

**Success Criteria:**
- [ ] Real database tests pass
- [ ] Pool works with all supported drivers
- [ ] Runtime backpressure detection verified

---

### Phase 6: Documentation (M1)

**Goal:** Complete documentation for connection pool

**Tasks:**

1. **Rustdoc comments**
   - Document all public types
   - Document all public methods
   - Add examples

2. **Module-level docs**
   - Explain pooling architecture
   - Document deadpool integration
   - Provide usage examples

3. **Update CLAUDE.md**
   - Document pool design decisions
   - Add configuration guidelines
   - Document health checking strategy

**Success Criteria:**
- [ ] All public items have rustdoc
- [ ] `cargo doc` builds without warnings
- [ ] CLAUDE.md updated with pool section

---

## Testing Strategy

### Unit Tests (Target: 80%+ coverage)

**What to Test:**

1. **Pool creation**
   - Valid config
   - Invalid config (rejected)
   - Min/max size validation

2. **Connection lifecycle**
   - Create new connection
   - Checkout from pool
   - Return to pool
   - Connection reuse

3. **Health checking**
   - Ping healthy connection → reused
   - Ping broken connection → replaced
   - Health check on checkout

4. **Stats accuracy**
   - Stats reflect actual state
   - Active/idle/pending counts correct
   - Utilization calculation correct

5. **Limits enforcement**
   - Max size enforced
   - Min size maintained
   - Timeout behavior

### Integration Tests

**What to Test:**

1. **Real database connection**
   - MySQL pooling works
   - PostgreSQL pooling works (M1)

2. **Concurrent access**
   - Multiple tasks checkout simultaneously
   - Pool handles contention correctly
   - No deadlocks

3. **Pool saturation**
   - Behavior when all connections in use
   - Waiting for available connection
   - Timeout when pool exhausted

4. **Long-running stability**
   - No connection leaks
   - Idle cleanup works
   - Health checks maintain pool health

### Property Tests (Optional)

**What to Test:**

1. **Pool invariants**
   - `active + idle + pending = max_size` (or less)
   - Utilization always between 0.0 and 1.0

2. **Connection lifecycle**
   - Every checkout has matching return
   - No use-after-return

---

## Performance Targets

### Latency

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Checkout (idle pool)** | <10μs (p50) | Benchmark: pool.get() when connections available |
| **Checkout (idle pool)** | <50μs (p99) | Benchmark: pool.get() with occasional contention |
| **Checkout (saturated)** | timeout | Benchmark: pool.get() when max_size reached |
| **Health check (ping)** | <1ms | Benchmark: Connection::ping() |
| **Stats calculation** | <1μs | Benchmark: pool.stats() |

### Throughput

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Single-threaded** | 100K+ checkouts/sec | Benchmark: loop of get() -> return |
| **Concurrent (10 tasks)** | 500K+ checkouts/sec | Benchmark: 10 tasks doing get() -> return |
| **Sustained load** | 50K ops/sec for 60s | Integration test with real database |

### Resource Usage

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Memory per connection** | <100KB overhead | Pool overhead (not including connection itself) |
| **Pool overhead** | <1MB | Total pool memory (excluding connections) |
| **Connection leaks** | 0 | Long-running test, monitor connection count |

### Correctness

| Metric | Target | How to Measure |
|--------|--------|----------------|
| **Stats accuracy** | 100% | Compare stats to actual state |
| **Connection reuse** | >90% | Track connection IDs, verify reuse |
| **Health check success** | >99% | Track recycle() results |

---

## Future Enhancements

### Milestone 2

**1. Prepared Statement Cache**

```rust
pub struct ConnectionPool {
    inner: deadpool::managed::Pool<DriverManager>,
    // NEW: Per-connection statement cache
    statement_cache: DashMap<ConnectionId, StatementCache>,
}

impl PooledConnection {
    pub async fn execute_prepared(&mut self, sql: &str, params: &[Value])
        -> Result<QueryResult>
    {
        // Check cache for prepared statement
        // If not found, prepare and cache
        // Execute with cached statement
    }
}
```

**Benefits:**
- Avoid repeated SQL parsing
- Faster query execution
- Reduced database load

**2. Multi-Endpoint Support (Read Replicas)**

```rust
pub struct ConnectionPool {
    // Multiple pools for different endpoints
    primary: Pool<DriverManager>,
    replicas: Vec<Pool<DriverManager>>,
}

impl ConnectionPool {
    pub async fn get_for_write(&self) -> Result<PooledConnection> {
        self.primary.get().await
    }

    pub async fn get_for_read(&self) -> Result<PooledConnection> {
        // Load balance across replicas
        let pool = self.select_replica();
        pool.get().await
    }
}
```

**3. Connection Affinity**

```rust
// Ensure same connection for entire transaction
pub struct AffinityToken {
    connection_id: ConnectionId,
}

impl ConnectionPool {
    pub async fn get_with_affinity(&self, token: AffinityToken)
        -> Result<PooledConnection>
    {
        // Return same connection for token
    }
}
```

---

## Appendices

### Appendix A: Deadpool vs Alternatives

**Why deadpool over r2d2?**

| Feature | deadpool | r2d2 |
|---------|----------|------|
| **Async support** | ✅ Native | ❌ Sync only |
| **Generic** | ✅ Over Manager | ✅ Over ConnectionManager |
| **Health checking** | ✅ Built-in | ✅ Built-in |
| **Timeout support** | ✅ async timeout | ❌ blocking |
| **Tokio integration** | ✅ Perfect fit | ❌ Requires spawn_blocking |

**Verdict:** deadpool is the clear choice for async Rust.

### Appendix B: Configuration Guidelines

**Choosing max_size:**

```
max_size = min(
    database_max_connections / num_benchmark_clients,
    available_memory / connection_memory,
    desired_concurrency
)
```

**Example:**
- Database: MySQL with max_connections=1000
- Benchmark clients: 1 (single RSBench instance)
- Memory: 16GB available, ~10MB per connection
- Desired concurrency: 100

→ `max_size = min(1000/1, 16000/10, 100) = min(1000, 1600, 100) = 100`

**Choosing min_size:**

```
min_size = max_size * 0.1  (10% of max)
```

Ensures some connections always ready, but not too many idle.

### Appendix C: Error Handling

**Pool exhaustion:**
```rust
match pool.get().await {
    Ok(conn) => { /* use connection */ },
    Err(e) if e.is_timeout() => {
        // Pool saturated - trigger backpressure
        return Err(RuntimeError::PoolExhausted);
    },
    Err(e) => {
        // Other error
        return Err(e.into());
    }
}
```

**Connection creation failure:**
```rust
// DriverManager::create() fails
// deadpool will retry based on config
// If max retries exceeded, returns error
```

**Health check failure:**
```rust
// DriverManager::recycle() returns RecycleError::Backend
// deadpool discards connection and creates new one
```

---

**End of Connection Pool Design Document**
