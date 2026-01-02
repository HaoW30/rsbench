# Error Handling Philosophy

## TL;DR

**RSBench treats errors as metrics to measure, not failures to halt on.**

This is a fundamental design difference from sysbench and aligns with modern observability and chaos engineering practices.

## Sysbench vs RSBench

### Sysbench Approach

**Stops by default** on database errors, requiring explicit ignore list:

```bash
sysbench --ignore-errors=1062,1213 oltp_read_write run
# 1062 = Duplicate key
# 1213 = Deadlock
```

**Philosophy**: Database operations should succeed; errors indicate test failure.

**Problem**: You must know error codes in advance to test under realistic conditions.

---

### RSBench Approach

**Never stops** on database errors - they're counted and reported:

```
Operation: point_select
  Count: 10000
  Errors: 150 (1.5%)
  Success Rate: 98.5%
  Throughput: 1000 ops/sec
```

**Philosophy**: Errors are data points that reveal system behavior under load.

**Benefit**: Discover error rates without knowing error codes in advance.

---

## Why RSBench's Approach is Better

### 1. Errors Reveal Capacity Limits

**Goal**: Find maximum sustainable throughput

Ramping test example:
```
Rate: 5K QPS   → Errors: 0 (0%)       ✅ Within capacity
Rate: 10K QPS  → Errors: 50 (0.5%)    ⚠️ Approaching limit
Rate: 15K QPS  → Errors: 800 (5.3%)   ❌ Over capacity
Rate: 20K QPS  → Errors: 2400 (12%)   ❌ Far over capacity
```

**If we stopped at first error**: Test would halt at 10K QPS, never finding that 15K+ is unusable.

**With continuous execution**: We see the degradation curve and know 5-10K is safe, 10-15K is risky, 15K+ is broken.

---

### 2. Real-World Behavior Simulation

Production systems don't halt on errors - they measure and adapt:

| Scenario | Error Type | Why We Measure It |
|----------|------------|-------------------|
| **High Concurrency** | Deadlocks (1213) | Normal at high load - measure rate to set concurrency limits |
| **Race Conditions** | Duplicate key (1062) | Expected in distributed systems - measure frequency |
| **Failover Testing** | Connection timeouts | Want to see failover duration and recovery time |
| **Network Issues** | I/O errors | Measure impact and recovery patterns |
| **Query Timeouts** | Lock wait timeout (1205) | Indicates slow queries - measure to optimize |

**Example - Failover Test**:
```
Time: 0-60s   → Errors: 0%      (Normal operation)
Time: 60-90s  → Errors: 85%     (Failover in progress) ← Measure this!
Time: 90-120s → Errors: 12%     (Recovery)             ← And this!
Time: 120-180s → Errors: 1%     (Stabilized)           ← And this!
```

Stopping at the first error would hide the entire failover/recovery behavior.

---

### 3. Chaos Engineering & Failure Testing

Modern systems are tested **under failure**, not just success:

```yaml
# Test database during pod restart
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 300s  # 5 minutes
```

**During test**:
- Kill database pod at 60s
- Measure error spike duration
- Measure recovery time
- Measure error rate after recovery

**Valuable metrics**:
- Failover duration: 12 seconds (85% error rate)
- Recovery time: 30 seconds (errors drop to <5%)
- Residual error rate: 1% (lingering connections)

**If we stopped**: We'd only know "an error happened at 60s" - no duration, no recovery data.

---

### 4. Time-Driven vs Thread-Driven Architecture

This difference is **architectural**, not just philosophical:

**Sysbench (Thread-Driven)**:
```
Thread 1: SELECT → Error → Thread stops
Thread 2: SELECT → Error → Thread stops
Thread 3: SELECT → Error → Thread stops
...
All threads stopped → Test halts
```

**Need `--ignore-errors`** to keep threads alive.

**RSBench (Time-Driven)**:
```
t=0ms:    Submit operation → Error → Count error, submit next
t=1ms:    Submit operation → Error → Count error, submit next
t=2ms:    Submit operation → Success → Count success, submit next
...
Rate maintained regardless of errors
```

**Don't need `--ignore-errors`** - errors don't block operation scheduling.

---

## What RSBench Provides Instead

### 1. Error Rate Metrics (Current - M0)

```
Operation: point_select
  Count: 10000
  Errors: 150
  Throughput: 1000 ops/sec
```

### 2. Enhanced Error Visibility (Planned - M0)

```
Operation: point_select
  Count: 10000
  Errors: 150 (1.5%)           ← NEW: Show percentage
  Success Rate: 98.5%          ← NEW: Explicit success rate
  Throughput: 165.3 ops/sec

⚠️  Warning: Error rate (1.5%) exceeds recommended threshold (1.0%)
```

### 3. Error Type Breakdown (Planned - M1)

```
Operation: point_select
  Count: 10000
  Errors: 150 (1.5%)

  Error Breakdown:
    - Deadlock (1213): 120 (80% of errors)
    - Duplicate key (1062): 25 (16.7%)
    - Lock timeout (1205): 5 (3.3%)
```

**Benefit**: Know **what's failing**, not just that something failed.

### 4. Optional Error Thresholds (Planned - M1)

For tests where **errors are truly unexpected**:

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  # Optional: Stop if error rate exceeds threshold
  error_handling:
    threshold: 5.0       # Stop if > 5% error rate
    window: 10s          # Must be sustained for 10s
    action: stop         # "stop" | "warn" | "ignore"
```

**Philosophy**: Opt-in stopping (vs sysbench's opt-in ignoring)

**Use case**: Regression tests where errors indicate bugs, not capacity limits.

---

## Error Classification

RSBench already classifies errors (from our connection pool work):

```rust
// src/driver/mysql.rs
fn is_recoverable_error(e: &mysql_async::Error) -> bool {
    match e {
        Error::Driver(_) => true,   // Param mismatch - connection OK
        Error::Server(_) => true,   // SQL error (deadlock, constraint) - connection OK
        Error::Io(_) => false,      // I/O error - connection broken
        _ => false,
    }
}
```

**Recoverable Errors** (Continue normally):
- Deadlock detected (1213)
- Duplicate entry (1062)
- Lock wait timeout (1205)
- Constraint violations (1451, 1452)
- Query syntax errors

**Fatal Errors** (Connection broken):
- I/O errors (connection lost)
- Server gone away (2006)
- Connection lost during query (2013)

**Current behavior**: Both types are counted, neither stops the test.

**Future enhancement**: Could show breakdown by recoverability:
```
Errors: 150 total
  - Recoverable: 145 (deadlocks, constraints)
  - Fatal: 5 (connection lost)
```

---

## Use Cases and Recommendations

### Capacity Testing (Recommended: No Threshold)

**Goal**: Find maximum throughput

```yaml
scenario:
  executor:
    type: ramping-rate
    stages:
      - duration: 60s
        target_rate: 5000
      - duration: 60s
        target_rate: 10000
      - duration: 60s
        target_rate: 20000
```

**Don't set error threshold** - you want to see degradation.

**Output**:
```
Stage 1 (5K):  Errors: 0%    ✅ Safe
Stage 2 (10K): Errors: 0.5%  ⚠️ Approaching limit
Stage 3 (20K): Errors: 8%    ❌ Over capacity
```

---

### Regression Testing (Optional: Set Threshold)

**Goal**: Ensure code changes don't break queries

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  error_handling:
    threshold: 1.0    # Stop if > 1% errors
    action: stop
```

**Set threshold** - errors indicate bugs, not expected behavior.

**Output**:
```
Operation: user_query
  Count: 5234
  Errors: 68 (1.3%)  ❌ Exceeds threshold (1.0%)

⛔ Test stopped: Error rate exceeds threshold
```

---

### Failover Testing (Recommended: No Threshold)

**Goal**: Measure failover duration and recovery

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 300s  # 5 minutes
```

**External action**: Kill database pod at 60s

**Don't set threshold** - high error rate during failover is expected.

**Output**:
```
Time 0-60s:   Errors: 0%    (Normal)
Time 60-90s:  Errors: 82%   (Failover)  ← Key metric!
Time 90-120s: Errors: 8%    (Recovery)  ← Key metric!
Time 120-300s: Errors: 1%   (Stable)
```

---

### Stress Testing (Recommended: Warn Threshold)

**Goal**: Push database beyond normal limits

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 50000   # Intentionally high
    duration: 120s

  error_handling:
    threshold: 10.0   # Warn if > 10% errors
    action: warn      # Don't stop, just warn
```

**Warn threshold** - inform but don't halt.

---

## Comparison with Industry Tools

| Tool | Default Behavior | Philosophy |
|------|------------------|------------|
| **sysbench** | Stop on errors | Errors = test failure |
| **Apache JMeter** | Continue, count errors | Errors = metrics |
| **Gatling** | Continue, count errors | Errors = metrics |
| **k6** | Continue, count errors | Errors = metrics |
| **wrk** | Continue, count errors | Errors = metrics |
| **RSBench** | Continue, count errors | Errors = metrics |

**Modern tools treat errors as observability data**, not test failures.

---

## Implementation Roadmap

### M0 (Current)
- ✅ Errors counted in metrics
- ✅ Never stop on errors
- ✅ Error classification (recoverable vs fatal)
- ⬜ Show error rate percentage in output
- ⬜ Show success rate percentage

### M1 (Planned)
- ⬜ Error type breakdown (per error code)
- ⬜ Optional error threshold config
- ⬜ Configurable threshold action (stop/warn/ignore)
- ⬜ Error rate warnings in output

### M2 (Future)
- ⬜ Per-operation error thresholds
- ⬜ Error rate over time (time-series)
- ⬜ Error correlation with latency spikes
- ⬜ Automatic "expected" vs "unexpected" classification

---

## Design Principles

1. **Errors Are Data, Not Failures**
   - Every error reveals system behavior
   - Error rates show capacity limits
   - Error patterns show failure modes

2. **Observability First**
   - Make errors visible, not hidden
   - Provide context (type, rate, timing)
   - Enable root cause analysis

3. **Opt-In Strictness**
   - Default: Continuous execution
   - Optional: Configurable thresholds
   - User decides what's "acceptable"

4. **Time-Driven Resilience**
   - Errors don't block operation scheduling
   - Target rate maintained regardless of errors
   - Natural fit for failure testing

5. **Real-World Alignment**
   - Production systems measure errors, don't halt
   - Chaos engineering requires error observation
   - Modern reliability practices embrace failure

---

## FAQ

### Q: Won't this hide critical errors?

**A**: No - errors are prominently displayed in output. We enhance visibility, not hide it.

```
⚠️  Warning: Error rate (8.5%) exceeds recommended threshold (1.0%)

  Error Breakdown:
    - Deadlock (1213): 450 errors
    - Connection timeout: 85 errors
```

---

### Q: What if I want tests to fail on errors?

**A**: Use optional error thresholds (M1):

```yaml
error_handling:
  threshold: 1.0
  action: stop
```

Or check exit code:
```bash
rsbench --scenario test.yaml run
if [ $? -ne 0 ]; then
  echo "Test failed"
fi
```

---

### Q: How is this different from `--ignore-errors`?

**A**:

| Approach | Default | Opt-In |
|----------|---------|--------|
| **sysbench** | Stop on errors | Ignore specific codes |
| **RSBench** | Count all errors | Stop if rate exceeds threshold |

Opposite philosophies:
- sysbench: "Errors are bad unless ignored"
- RSBench: "Errors are data unless critical"

---

### Q: What about SQL syntax errors?

**A**:

Syntax errors indicate **test bugs**, not capacity issues:

```
⚠️  Error: Query syntax error
  SQL: SELECT * FORM users  (typo: FORM → FROM)

  This indicates a workload bug, not database behavior.
```

**Solution**: Fix the workload YAML, not ignore the error.

---

## Summary

**RSBench's error handling philosophy**:

1. ✅ **Errors are metrics** - Count, measure, report
2. ✅ **Never stop by default** - Observe full behavior
3. ✅ **Enhance visibility** - Show rates, types, patterns
4. ✅ **Optional strictness** - Configurable thresholds for specific needs
5. ✅ **Align with modern practices** - Chaos engineering, observability, SRE

**This is not a missing feature** - it's a fundamental design choice that makes RSBench better suited for:
- Capacity testing
- Failure testing
- Chaos engineering
- Real-world simulation
- Modern reliability practices

**If you need sysbench's behavior**: Use error thresholds (M1) to opt-in to stopping.
