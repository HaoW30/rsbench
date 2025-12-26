# RSBench vs Sysbench: Performance Analysis

**Date**: 2024-12-26
**Status**: Pre-implementation Analysis
**Confidence Level**: Medium-High (theoretical, pending benchmarks)

---

## Executive Summary

**Claim**: RSBench can drive **2-10x higher QPS** than sysbench with the same client resources.

**Confidence**: **70-80%** for the 2-5x range, **50-60%** for 5-10x range

**Caveat**: These are **theoretical estimates** based on architectural analysis. **Actual benchmarks required** to validate.

---

## Theoretical Performance Advantages

### 1. Async I/O vs Blocking I/O

**Sysbench (Blocking)**:
```
Thread 1: ─[execute SQL]─────────────[wait 10ms]────────────────►
Thread 2: ─[execute SQL]─────────────[wait 10ms]────────────────►
Thread 3: ─[execute SQL]─────────────[wait 10ms]────────────────►
...
Thread N: ─[execute SQL]─────────────[wait 10ms]────────────────►

N threads × 8MB stack = N × 8MB memory
Each thread blocks during I/O (wasted CPU time)
```

**RSBench (Async)**:
```
Task 1: ─[submit]─┐                           ┌─[complete]─►
Task 2: ─[submit]─┤                           ├─[complete]─►
Task 3: ─[submit]─┼─► [Tokio multiplexes] ───┼─[complete]─►
...                │    on 8 OS threads       │
Task N: ─[submit]─┘                           └─[complete]─►

N tasks × 2KB stack = N × 2KB memory
OS threads never block (context switch between tasks)
```

**Impact**:
- **Memory**: 4000x less per concurrent operation (8MB → 2KB)
- **CPU**: No thread blocking → better CPU utilization

**Confidence**: **95%** - This is well-established async I/O advantage

---

### 2. Connection Pool Efficiency

**Sysbench**:
- Each thread needs its own connection (1 thread = 1 connection minimum)
- 100 threads = 100 database connections
- Context switching overhead between 100 OS threads

**RSBench**:
- M connections shared by N async tasks (M < N is possible)
- 100 workers can share 50 connections efficiently
- Async multiplexing avoids context switch overhead

**Impact**:
- **Connection efficiency**: 2x fewer connections needed
- **Database load**: Lower connection overhead on database

**Confidence**: **80%** - Depends on workload pattern (read-heavy benefits more)

---

### 3. Rate Limiter Overhead

**Sysbench**:
- No built-in rate limiting (threads run as fast as they can)
- To limit rate: sleep() in Lua script (imprecise, affects all threads)

**RSBench**:
- Token bucket rate limiter with high precision
- Tokio sleep is cheap (task yield, not thread block)

**Impact**:
- **Rate accuracy**: RSBench maintains target rate ±1%
- **Overhead**: Minimal (token bucket is O(1))

**Confidence**: **90%** - For rate-limited tests, RSBench has clear advantage

---

### 4. Metrics Collection

**Sysbench**:
- Metrics collected per-thread, aggregated at end
- Potential lock contention during collection

**RSBench**:
- Lock-free metrics (DashMap + atomics)
- HDR histograms for accurate percentiles
- No contention in hot path

**Impact**:
- **Overhead**: ~1-2% vs ~3-5% for sysbench
- **Accuracy**: HDR histograms provide better percentile accuracy

**Confidence**: **85%** - Lock-free data structures are well-tested

---

## Realistic Performance Estimates

### Scenario 1: High Concurrency (1000 workers)

**Sysbench**:
```
Configuration: --threads=1000
Memory: ~8 GB (1000 threads × 8 MB)
CPU: High context switch overhead
Practical limit: ~500 threads before OS struggles
Estimated QPS: 5,000-10,000 ops/sec (limited by thread overhead)
```

**RSBench**:
```
Configuration: workers: 1000
Memory: ~50 MB (1000 tasks × 50 KB including runtime)
CPU: 8-16 OS threads (low context switch)
Practical limit: 10,000+ workers
Estimated QPS: 20,000-50,000 ops/sec (limited by CPU/network)
```

**Advantage**: **2-5x higher QPS**

**Confidence**: **75%** - Async I/O advantage is significant at high concurrency

---

### Scenario 2: Medium Concurrency (100 workers)

**Sysbench**:
```
Configuration: --threads=100
Memory: ~800 MB
CPU: Moderate context switch overhead
Estimated QPS: 10,000-15,000 ops/sec
```

**RSBench**:
```
Configuration: workers: 100
Memory: ~20 MB
CPU: Low overhead
Estimated QPS: 15,000-30,000 ops/sec
```

**Advantage**: **1.5-2x higher QPS**

**Confidence**: **70%** - Advantage is smaller at moderate concurrency

---

### Scenario 3: Low Concurrency (10 workers)

**Sysbench**:
```
Configuration: --threads=10
Memory: ~80 MB
Estimated QPS: 5,000-8,000 ops/sec (depends on query latency)
```

**RSBench**:
```
Configuration: workers: 10
Memory: ~10 MB
Estimated QPS: 6,000-10,000 ops/sec
```

**Advantage**: **1.2-1.5x higher QPS**

**Confidence**: **60%** - At low concurrency, async advantage is minimal

---

## Bottlenecks and Limitations

### RSBench Bottlenecks

1. **CPU-bound operations** (Hot Path):
   - Rate limiter token acquisition: ~10μs per operation
   - Workload operation generation: ~5μs per operation
   - Tokio task spawn: ~2μs per operation
   - **Total**: ~17μs per operation → max ~58K ops/sec per thread

2. **Network bandwidth**:
   - 1 Gbps link: ~125 MB/sec
   - Typical query: 1 KB request + 1 KB response = 2 KB
   - **Theoretical max**: ~62K queries/sec per client

3. **Database connection limits**:
   - Most databases: 1000-5000 max connections
   - RSBench needs fewer connections than sysbench, but still limited

### Sysbench Bottlenecks

1. **Thread context switching**:
   - 1000 threads: ~100μs context switch time
   - Significant CPU waste

2. **Memory pressure**:
   - 1000 threads × 8 MB = 8 GB
   - Can trigger swapping, killing performance

3. **OS limits**:
   - Linux default: ulimit -u 1024 (max user processes)
   - Must increase limits for high thread counts

---

## When RSBench Is Faster (High Confidence)

### ✅ Scenarios Where RSBench Wins by 2-10x:

1. **High concurrency tests** (500+ workers)
   - Async I/O shines with many concurrent operations
   - Memory footprint doesn't explode
   - **Confidence**: **85%**

2. **Rate-limited tests** (target QPS specified)
   - Precise rate limiting without thread sleep
   - Better CPU utilization
   - **Confidence**: **90%**

3. **Long-running tests** (hours to days)
   - Lower memory footprint reduces OS pressure
   - No thread context switch overhead
   - **Confidence**: **80%**

4. **Resource-constrained clients** (limited CPU/memory)
   - Can drive more load with less resources
   - **Confidence**: **85%**

---

## When RSBench Is Similar (Lower Confidence)

### ≈ Scenarios Where RSBench Is Only 1-2x Faster:

1. **Low concurrency tests** (< 50 workers)
   - Async advantage is minimal
   - Thread overhead is manageable
   - **Confidence**: **60%**

2. **CPU-intensive workloads** (complex Lua scripts)
   - Both tools bottlenecked by CPU, not I/O
   - **Confidence**: **70%**

3. **Very low latency databases** (< 1ms response time)
   - I/O multiplexing advantage reduced
   - **Confidence**: **65%**

---

## Recommended Benchmarks to Validate Claims

### Critical Benchmarks (Must Run):

1. **Throughput Comparison**:
   ```bash
   # Sysbench
   sysbench oltp_read_write --threads=100 --time=60 run

   # RSBench
   rsbench --scenario oltp_read_write.yaml --workers=100 --duration=60s

   # Measure: QPS, p99 latency, client CPU/memory
   ```

2. **Scalability Test**:
   ```bash
   # Test with: 10, 50, 100, 500, 1000 workers
   # Measure: QPS vs worker count
   # Expected: RSBench scales better at high worker counts
   ```

3. **Resource Efficiency**:
   ```bash
   # Same QPS target (e.g., 10K ops/sec)
   # Measure: Client CPU, memory, connections needed
   # Expected: RSBench uses 2-4x less memory
   ```

4. **Rate Accuracy**:
   ```bash
   # Target: 1000 ops/sec for 300 seconds
   # Measure: Actual ops/sec (average, stddev)
   # Expected: RSBench maintains rate ±1%, sysbench ±5-10%
   ```

### Nice-to-Have Benchmarks:

5. **Long-running stability** (24 hours)
6. **Backpressure behavior** (database slows down)
7. **Distributed mode overhead** (M clients vs 1 client)

---

## Honest Assessment

### What I'm Confident About (80-95%):

1. ✅ RSBench uses **40x less memory** than sysbench for same concurrency
2. ✅ RSBench can drive **2-5x higher QPS** with same client resources at high concurrency
3. ✅ RSBench has **better rate limiting accuracy** (±1% vs ±5-10%)
4. ✅ RSBench can **scale to 10K+ workers** vs sysbench limit of ~500-1000 threads

### What Needs Validation (50-70%):

1. ⚠️ **5-10x QPS advantage** - Possible but needs benchmarks to confirm
2. ⚠️ **CPU efficiency** - Theoretical advantage, but actual overhead unknown
3. ⚠️ **Network saturation** - May hit network limits before async advantage shows
4. ⚠️ **Real-world workloads** - Synthetic benchmarks may not reflect production

### What Could Go Wrong (Risks):

1. ❌ **Tokio overhead** - Task spawn/scheduling may be higher than expected
2. ❌ **Lock-free metrics** - DashMap contention could be significant
3. ❌ **Rate limiter** - Token bucket may be CPU bottleneck at extreme rates
4. ❌ **Workload generation** - RNG + parameter generation could be slow

---

## Conservative Claims for Documentation

**Recommended Messaging**:

Instead of saying:
> ❌ "RSBench is 10x faster than sysbench"

Say:
> ✅ "RSBench can drive 2-5x higher QPS with the same client resources due to async I/O and lower memory footprint. Async architecture enables 10K+ concurrent workers vs sysbench's ~500-1000 thread limit."

Instead of:
> ❌ "No client bottlenecks"

Say:
> ✅ "Lower client-side overhead due to async I/O. Backpressure detection ensures you know when client is saturated."

Instead of:
> ❌ "Unlimited scalability"

Say:
> ✅ "Scales to 10K+ workers on a single client pod (vs ~500-1000 threads for sysbench) with memory footprint of ~50-100 MB (vs ~4-8 GB for sysbench)."

---

## Action Items Before Release

1. **[ ] Run benchmarks** on representative hardware:
   - 4-core, 8GB RAM client
   - MySQL 8.0 database
   - OLTP read/write workload
   - Test: 10, 50, 100, 500, 1000 workers

2. **[ ] Measure actual overhead**:
   - Profile hot path (rate_limiter, workload generation, tokio spawn)
   - Confirm <17μs per operation

3. **[ ] Validate memory claims**:
   - Measure actual memory with 1000 workers
   - Confirm <100 MB (vs 8 GB sysbench)

4. **[ ] Update documentation** based on actual benchmarks

---

## Conclusion

**Current Confidence Level**: **70-75%** for 2-5x QPS advantage

**Risk Assessment**: **Medium** - Claims are well-reasoned but unproven

**Recommendation**:
- Use **conservative claims** (2-5x) in public documentation
- Run **benchmarks ASAP** to validate theoretical advantages
- Be transparent about "theoretical" vs "measured" performance
- Update claims based on actual benchmark results

**Bottom Line**: The architectural advantages are sound (async I/O, lower memory), but actual performance depends on implementation quality and real-world bottlenecks. **Ship with conservative claims, update with measured data.**

---

**Last Updated**: 2024-12-26
**Next Review**: After M0 benchmarks complete
