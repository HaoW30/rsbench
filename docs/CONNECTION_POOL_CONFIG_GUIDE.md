# Connection Pool Configuration Guide

Quick reference for configuring connection pools correctly based on workload characteristics.

---

## Key Metrics to Monitor

```
[Scenario] Operation tracking:
  Generated (submitted): X      ← How many operations submitted
  Completed (from metrics): Y   ← How many actually finished
  In-flight (difference): X-Y   ← Queued operations
  Backpressure events: Z        ← Client saturation indicator

Throughput: A ops/sec           ← Actual completed rate
Latency p99: B ms              ← Database response time
```

**Health Indicators**:
- ✅ **In-flight = 0**: All operations completed (good drain timeout)
- ✅ **Backpressure < 5%**: Client has capacity
- ✅ **Completed ≈ Generated**: No dropped operations
- ⚠️ **Backpressure 5-20%**: Consider adding connections
- ❌ **Backpressure > 20%**: Client saturated, results invalid

---

## Configuration Patterns

### Pattern 1: Development / Testing
**Use case**: Local testing, debugging, quick iterations

```yaml
database:
  pool:
    min_size: 10
    max_size: 20
    connection_timeout: 5s
    idle_timeout: 300s

runtime:
  max_connections: 20

scenario:
  executor:
    rate: 1000          # Low rate
    duration: 10s       # Short test
```

**Why**:
- Small pool (20) = fast startup
- Low rate (1K) = easy to debug
- Short duration (10s) = quick feedback

---

### Pattern 2: Capacity Testing
**Use case**: Find database limits, stress testing

```yaml
database:
  pool:
    min_size: 100       # Pre-warm all
    max_size: 100       # Fixed capacity
    connection_timeout: 5s
    idle_timeout: 600s

runtime:
  max_connections: 100  # Match pool

scenario:
  executor:
    rate: 20000         # High rate
    duration: 60s       # Longer test
```

**Why**:
- min_size = max_size: Predictable performance, no connection creation during test
- Large pool (100): Maximize throughput
- High rate (20K): Push database to limits

**How to tune**:
1. Start with 50 connections
2. If backpressure > 20%, double connections
3. If latency degrades, reduce rate
4. Find sweet spot where backpressure < 5% and latency acceptable

---

### Pattern 3: Production Simulation
**Use case**: Validate production capacity, SLA testing

```yaml
database:
  pool:
    min_size: 50        # Match expected baseline load
    max_size: 100       # Headroom for bursts
    connection_timeout: 10s
    idle_timeout: 600s

runtime:
  max_connections: 100

scenario:
  executor:
    type: ramping-rate
    stages:
      - rate: 1000      # Ramp up gradually
        duration: 30s
      - rate: 5000
        duration: 60s
      - rate: 10000
        duration: 120s
      - rate: 5000      # Ramp down
        duration: 30s
```

**Why**:
- min_size < max_size: Allow pool to scale with load
- Ramping: Realistic traffic patterns
- Long duration: Validate sustained performance

---

### Pattern 4: Read-Heavy Workload
**Use case**: Analytics, reporting, read replicas

```yaml
database:
  pool:
    min_size: 200       # Many small queries
    max_size: 200
    connection_timeout: 3s
    idle_timeout: 300s

runtime:
  max_connections: 200

scenario:
  workload:
    type: declarative
    file: workloads/oltp_point_select.yaml  # 100% reads
```

**Why**:
- Large pool (200): Read queries are fast, need many connections
- Short timeout (3s): Fast-fail if pool exhausted
- Point selects: Minimal database overhead

**Tuning**:
- If p50 latency < 5ms: Can increase connections further
- If backpressure high: Database likely saturated, not client

---

### Pattern 5: Write-Heavy Workload
**Use case**: Data ingestion, updates, transactions

```yaml
database:
  pool:
    min_size: 50        # Fewer connections for heavy writes
    max_size: 50
    connection_timeout: 10s
    idle_timeout: 600s

runtime:
  max_connections: 50

scenario:
  workload:
    type: declarative
    file: workloads/oltp_write_only.yaml
```

**Why**:
- Smaller pool (50): Writes are slower, contention higher
- Longer timeout (10s): Writes take time
- Fixed size: Predictable transaction throughput

**Tuning**:
- Monitor database lock contention
- If many deadlocks: Reduce connections
- If backpressure low but throughput low: Optimize queries

---

## Troubleshooting Guide

### Problem: Backpressure > 50%

**Symptoms**:
```
Backpressure events: 850,000 (85% of operations)
In-flight: 400,000
```

**Diagnosis**: Client saturated

**Solutions**:
1. Increase max_connections by 50%
2. Check if database can handle more load (monitor DB CPU/IO)
3. If database saturated, reduce rate instead

---

### Problem: High In-Flight Count

**Symptoms**:
```
Generated: 1,200,000
Completed: 800,000
In-flight: 400,000
```

**Diagnosis**: Drain timeout too short (default 100ms)

**Solutions**:
1. Increase connections (process queue faster)
2. Increase drain timeout in code
3. Or accept that late operations won't complete

**Code fix**:
```rust
// In src/scenario.rs, increase from 100ms to 1s
tokio::time::sleep(Duration::from_millis(1000)).await;
```

---

### Problem: Connection Creation Errors

**Symptoms**:
```
[DriverManager] create() called (count: 151)  # > max_size!
Can't assign requested address (errno 49)
```

**Diagnosis**: Semaphore not enforcing limit

**Check**:
1. Verify semaphore is enabled in pool (should be after fix)
2. Check runtime.max_connections = pool.max_size
3. Verify scenario.executor.max_connections = pool.max_size

---

### Problem: Health Check Failures

**Symptoms**:
```
[DriverManager] Health check FAILED #1: Connection error
[DriverManager] Health check FAILED #2: Connection error
```

**Diagnosis**: Query errors corrupting connections

**Solutions**:
1. Check for query errors in logs
2. Fix parameter bugs in workload
3. Ensure queries are valid SQL

**Common causes**:
- Parameter count mismatch
- Invalid table names
- SQL syntax errors
- Constraint violations

---

### Problem: Latency Degradation

**Symptoms**:
```
50 conn:  p99 = 10ms
100 conn: p99 = 17ms
150 conn: p99 = 28ms  ← Getting worse!
```

**Diagnosis**: Database saturated

**Solutions**:
1. Stop increasing connections (won't help)
2. Optimize queries (add indexes, reduce data scanned)
3. Scale database (add replicas, sharding)
4. Or accept lower throughput at acceptable latency

**Sweet spot**: Usually where latency starts to degrade sharply

---

## Quick Decision Tree

```
Start here
    |
    v
Run test with current config
    |
    v
Backpressure > 20%?
    |
    +-- YES --> Increase connections by 50%
    |           Run again
    |           Did throughput increase > 10%?
    |               |
    |               +-- YES --> Continue increasing
    |               |
    |               +-- NO --> Database saturated
    |                          Stop, this is your limit
    |
    +-- NO --> Check latency
                   |
                   v
                Latency acceptable?
                   |
                   +-- YES --> ✅ Done! This is optimal config
                   |
                   +-- NO --> Optimize queries or scale DB
```

---

## Configuration Formulas

### Estimating Required Connections

**Formula**: `connections ≈ target_qps × avg_latency_sec`

**Example**:
- Target: 20,000 QPS
- Average latency: 5ms = 0.005s
- Connections needed: 20,000 × 0.005 = **100 connections**

**Validation**:
- Run test with calculated connections
- If backpressure > 5%, increase by 20-50%
- If backpressure < 1%, can reduce connections

### Estimating Throughput Limit

**Formula**: `max_qps ≈ connections / avg_latency_sec`

**Example**:
- Connections: 50
- Latency: 3ms = 0.003s
- Max throughput: 50 / 0.003 = **16,666 QPS**

**Reality check**:
- Actual will be ~70-80% of theoretical max
- Due to semaphore overhead, connection checkout time, etc.
- So expect: ~12,000-13,000 QPS with 50 connections at 3ms latency

---

## Best Practices

### DO ✅

1. **Set min_size = max_size for predictable performance**
   - Pre-warms all connections at startup
   - No latency spikes from connection creation
   - Memory cost is negligible

2. **Match runtime.max_connections to pool.max_size**
   - Prevents runtime semaphore from bottlenecking
   - Allows full connection utilization

3. **Monitor backpressure percentage**
   - < 1%: Excellent, client has capacity
   - 1-5%: Good, normal operation
   - 5-20%: Warning, approaching limit
   - > 20%: Critical, client saturated

4. **Test incrementally**
   - Start with 50 connections
   - Double if backpressure high
   - Stop when latency degrades

5. **Keep health checks enabled**
   - Adds ~1ms overhead
   - But prevents broken connections from causing failures
   - Worth it for reliability

### DON'T ❌

1. **Don't set min_size << max_size in production**
   - Causes unpredictable performance
   - Connection creation spikes under load
   - Better to pre-warm

2. **Don't ignore backpressure events**
   - > 5% means results may be invalid
   - You're measuring client limits, not database

3. **Don't keep increasing connections if latency degrades**
   - Database is saturated, not client
   - More connections = more contention
   - Will make it worse

4. **Don't disable health checks to "improve performance"**
   - The 1ms overhead is negligible
   - Broken connections cause much worse problems
   - False economy

5. **Don't use localhost in connection strings**
   - May cause DNS resolution delays
   - Use 127.0.0.1 directly for local testing
   - Use actual IPs/hostnames for remote

---

## Example Configurations

### Small Database (Dev)
```yaml
pool: { min_size: 10, max_size: 10 }
runtime: { max_connections: 10 }
scenario: { rate: 1000, duration: 10s }
Expected: ~1K QPS, < 1% backpressure
```

### Medium Database (Staging)
```yaml
pool: { min_size: 50, max_size: 50 }
runtime: { max_connections: 50 }
scenario: { rate: 10000, duration: 60s }
Expected: ~10-12K QPS, 5-10% backpressure
```

### Large Database (Production)
```yaml
pool: { min_size: 200, max_size: 200 }
runtime: { max_connections: 200 }
scenario: { rate: 50000, duration: 300s }
Expected: ~40-50K QPS, < 5% backpressure
```

### Distributed (Multi-Region)
```yaml
# Per region
pool: { min_size: 100, max_size: 100 }
runtime: { max_connections: 100 }

# 3 regions × 100 conn = 300 total
Expected: ~80-120K QPS aggregate
```

---

**Last Updated**: 2026-01-03
**Tested With**: TiDB 7.x, RSBench M0
