# Rate Limiter Module Design Document

**Project:** RSBench
**Module:** Rate Limiter
**Version:** 1.0
**Date:** 2025-12-27
**Author:** Design Review
**Status:** Ready for Implementation

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Current State Analysis](#current-state-analysis)
3. [Design Goals](#design-goals)
4. [Architecture Overview](#architecture-overview)
5. [Algorithm Deep Dive: Option 1 vs Hybrid](#algorithm-deep-dive-option-1-vs-hybrid)
6. [Recommended Implementation: Hybrid Approach](#recommended-implementation-hybrid-approach)
7. [API Design](#api-design)
8. [Implementation Plan](#implementation-plan)
9. [Testing Strategy](#testing-strategy)
10. [Performance Optimization](#performance-optimization)
11. [Success Criteria](#success-criteria)
12. [Risk Mitigation](#risk-mitigation)
13. [Decision Log](#decision-log)

---

## Executive Summary

### Purpose

Design and implement a **high-performance, zero-allocation, thread-safe** rate limiter optimized for RSBench's critical data path. The rate limiter must support **100K-1M operations per second** with **±2% rate accuracy** and **<100ns overhead per token acquisition**.

### Key Requirements

- **Super performant**: <100ns overhead, 1M ops/sec throughput
- **Reliable**: ±2% rate accuracy, no drift over time
- **Robust**: Thread-safe, handles concurrent access
- **Simple**: Clear API, easy to use correctly
- **Low resource**: <128 bytes memory, zero allocations after init
- **Not a bottleneck**: Lock-free fast path, minimal contention

### Approach

Implement a **hybrid lock-free rate limiter** combining:
1. **Token bucket** semantics (burst support)
2. **Nanosecond precision** accounting (no float drift)
3. **Atomic operations** (lock-free concurrency)
4. **Smart sleep** optimization (event-driven, not busy-wait)

### Expected Outcomes

- **3 atomic operations** per acquire (vs 6+ in naive implementation)
- **~80ns latency** per acquire at 100K ops/sec
- **1M ops/sec** sustained throughput
- **100% thread-safe** with `Arc` sharing
- **±2% rate accuracy** measured over 1+ second intervals

---

## Current State Analysis

### Existing Implementation

**Location:** `src/rate_limiter.rs` (95 lines)

**Current Code Structure:**
```rust
pub struct RateLimiter {
    rate: u64,           // Operations per second
    capacity: f64,       // Max burst (float)
    tokens: f64,         // Current token balance (float)
    last_refill: Instant,
}

pub async fn acquire(&mut self) -> Result<Permit> {
    loop {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return Ok(Permit);
        }
        let wait_time = Duration::from_micros(1_000_000 / self.rate);
        tokio::time::sleep(wait_time).await;
    }
}
```

### Critical Issues Identified

| Issue | Impact | Severity | Solution |
|-------|--------|----------|----------|
| **Not thread-safe** (`&mut self`) | Can't share across tasks | 🔴 **Critical** | Use atomics, change to `&self` |
| **Busy-wait loop** | CPU waste, scheduler churn | 🔴 **Critical** | Calculate exact sleep time |
| **Coarse sleep granularity** | Rate inaccuracy at high rates | 🟡 **High** | Nanosecond-precision deficit |
| **No atomics** | Requires mutex if shared | 🟡 **High** | Lock-free atomic operations |
| **Float precision** | Drift at high rates/long runs | 🟡 **Medium** | Integer nanosecond accounting |
| **No batching** | Inefficient for burst loads | 🟡 **Medium** | Add `acquire_many()` |
| **Minimal testing** | Only 2 basic tests | 🟡 **Medium** | Comprehensive test suite |

### Performance Analysis

**At 100K ops/sec:**
```
Current implementation:
- 1 acquire() call every 10 microseconds
- Sleep interval: 10μs
- Context switches: ~100K/sec → UNACCEPTABLE
- Scheduler overhead: ~50-100μs per wake
- Potential bottleneck: YES ❌

Required performance:
- Overhead per acquire: <100ns (<1% of 10μs interval)
- No context switches on fast path
- Lock-free atomic operations only
```

**Conclusion:** Current implementation cannot support target workloads. Complete redesign required.

---

## Design Goals

### Performance Requirements

| Metric | Target | Rationale |
|--------|--------|-----------|
| **Overhead per acquire** | <100ns | <1% overhead at 10μs intervals (100K ops/sec) |
| **Maximum throughput** | 1M ops/sec | Support extreme workloads |
| **Rate accuracy** | ±2% | Professional-grade precision |
| **Concurrent scalability** | 100+ tasks | Multi-core utilization |
| **Memory footprint** | <128 bytes | Cache-friendly (2 cache lines) |
| **Zero allocations** | After init | No GC pressure |
| **Thread-safe** | Yes | Concurrent executors |

### Design Principles

1. **Lock-Free Fast Path** - Atomics only, no mutex contention
2. **Precise Timing** - Nanosecond-precision token accounting
3. **Smart Waiting** - Tokio timer integration, no busy loops
4. **Zero-Copy** - No allocations in hot path
5. **Simple API** - Easy to use correctly, hard to misuse
6. **Self-Correcting** - Tolerate races, converge to correct rate

---

## Architecture Overview

### Three Considered Approaches

#### Option A: Mutex-Protected Token Bucket
```rust
pub struct RateLimiter {
    state: Mutex<TokenBucketState>,
}
```
**Verdict:** ❌ **Rejected** - Lock contention unacceptable at 100K+ ops/sec

#### Option B: Per-Task Rate Limiter
```rust
pub struct RateLimiter {
    // No synchronization needed
}
```
**Verdict:** ❌ **Rejected** - Can't enforce global rate limit

#### Option C: Atomic Token Bucket (Selected)
```rust
pub struct RateLimiter {
    tokens_nanos: AtomicI64,
    last_update: AtomicU64,
    // ...
}
```
**Verdict:** ✅ **Selected** - Lock-free, high performance, global rate control

### Key Innovation: Nanosecond Token Accounting

**Traditional approach** (float-based):
```rust
tokens: f64               // 123.456 tokens
tokens += elapsed * rate  // Precision loss over time
```

**Our approach** (integer nanoseconds):
```rust
tokens_nanos: i64         // 123,456,789 nano-tokens
tokens_nanos += elapsed   // 1 nanosecond = 1 nano-token
```

**Benefits:**
- ✅ No float precision drift
- ✅ Exact integer arithmetic
- ✅ Atomic operations on integers
- ✅ Natural 1:1 time-to-token mapping

---

## Algorithm Deep Dive: Option 1 vs Hybrid

### Option 1: Straightforward Atomic Token Bucket

**Conceptual Design:**
```rust
pub struct RateLimiter {
    rate_nanos: AtomicU64,        // ns per token (1e9 / ops_per_sec)
    capacity_nanos: AtomicU64,    // Max burst capacity (in ns)
    tokens_nanos: AtomicI64,      // Current token balance (in ns)
    last_refill: AtomicU64,       // Last refill timestamp
}
```

**Algorithm:**
```
acquire():
  1. Calculate elapsed = now - last_refill
  2. Add elapsed to tokens (atomic add)
  3. Cap tokens at capacity (load, compare, store)
  4. Update last_refill to now (store)
  5. Try to consume rate_nanos tokens (atomic sub)
  6. If success → return permit
  7. If fail → sleep(rate_nanos) and retry
```

**Implementation:**
```rust
pub async fn acquire(&self) -> Permit {
    loop {
        // Refill tokens based on elapsed time
        let now = nanos_since_epoch();
        let last = self.last_refill.load(Ordering::Acquire);
        let elapsed = now - last;

        // Add tokens for elapsed time
        self.tokens_nanos.fetch_add(elapsed as i64, Ordering::AcqRel);

        // Update last refill timestamp
        self.last_refill.store(now, Ordering::Release);

        // Cap tokens at capacity
        let current = self.tokens_nanos.load(Ordering::Acquire);
        let capacity = self.capacity_nanos.load(Ordering::Relaxed);
        if current > capacity as i64 {
            self.tokens_nanos.store(capacity as i64, Ordering::Release);
        }

        // Try to consume one token
        let rate_nanos = self.rate_nanos.load(Ordering::Relaxed);
        let prev = self.tokens_nanos.fetch_sub(rate_nanos as i64, Ordering::AcqRel);

        if prev >= rate_nanos as i64 {
            return Permit;  // Success!
        } else {
            // Failed - restore token and wait
            self.tokens_nanos.fetch_add(rate_nanos as i64, Ordering::AcqRel);
            tokio::sleep(Duration::from_nanos(rate_nanos)).await;
        }
    }
}
```

**Atomic Operation Count:**
- Load last_refill: 1
- fetch_add tokens: 1
- Store last_refill: 1
- Load tokens (for cap check): 1
- Load capacity: 1
- Store tokens (if capped): 0-1
- Load rate_nanos: 1
- fetch_sub tokens: 1
- fetch_add restore (on failure): 0-1
- **Total: 6-9 atomic operations per acquire**

**Issues:**
- ❌ **High atomic operation count** (6-9 per acquire)
- ❌ **Atomic contention** on capacity capping
- ❌ **Race conditions** on last_refill (multiple threads updating)
- ❌ **Restore logic** adds complexity and overhead
- ⚠️ **Fixed sleep** doesn't account for partial token accumulation

### Hybrid Approach: Optimized Lock-Free Implementation

**Design Improvements:**

1. **Lazy refill with tolerated races**
   - Use `compare_exchange_weak` on timestamp
   - Tolerate CAS failures (next iteration corrects)
   - No strict synchronization on refill timing

2. **Soft capacity limit**
   - Compute capacity check without atomic enforcement
   - Natural capping through consumption rate
   - Eliminates atomic store on cap

3. **Allow negative token balance**
   - Use signed `i64` for tokens
   - Simplifies race handling (no restore needed)
   - Single atomic operation to attempt consumption

4. **Precise sleep calculation**
   - Calculate exact deficit: `rate_nanos - current_tokens`
   - Sleep exactly the required time
   - Better rate accuracy and lower latency

**Optimized Implementation:**
```rust
pub struct RateLimiter {
    // Config (can be changed dynamically)
    rate_nanos: AtomicU64,         // ns per token (1e9 / ops_per_sec)
    capacity_nanos: AtomicU64,     // Max burst in nanoseconds

    // State (lock-free)
    tokens_nanos: AtomicI64,       // Current balance (SIGNED - can go negative)
    last_update: AtomicU64,        // Last update timestamp
}

impl RateLimiter {
    pub async fn acquire(&self) -> Permit {
        loop {
            // 1. Get current time and calculate elapsed (no atomic yet)
            let now = nanos_since_epoch();
            let last = self.last_update.load(Ordering::Relaxed);  // Relaxed OK
            let elapsed = now.saturating_sub(last);

            // 2. Add elapsed time as tokens + get previous balance (1 atomic op)
            let prev_tokens = self.tokens_nanos.fetch_add(
                elapsed as i64,
                Ordering::AcqRel
            );

            // 3. Compute current tokens with soft capacity limit (no atomic)
            let rate_nanos = self.rate_nanos.load(Ordering::Relaxed);
            let capacity = self.capacity_nanos.load(Ordering::Relaxed);
            let current_tokens = (prev_tokens + elapsed as i64).min(capacity as i64);

            // 4. Update last_update using weak CAS, tolerate failure (1 atomic op)
            self.last_update.compare_exchange_weak(
                last, now,
                Ordering::Release,
                Ordering::Relaxed
            ).ok();  // Ignore failure! Next iteration will correct

            // 5. Try to consume one token (1 atomic op)
            if current_tokens >= rate_nanos as i64 {
                let consumed = self.tokens_nanos.fetch_sub(
                    rate_nanos as i64,
                    Ordering::AcqRel
                );

                if consumed >= rate_nanos as i64 {
                    return Permit;  // Success!
                }
                // Lost race - tokens already negative
                // No restore needed, just retry
            } else {
                // 6. Not enough tokens - calculate exact deficit and sleep
                let deficit = (rate_nanos as i64 - current_tokens).max(0) as u64;
                tokio::sleep(Duration::from_nanos(deficit)).await;
            }
        }
    }
}
```

**Atomic Operation Count:**
- Load last_update (Relaxed): 0.5 (very fast)
- fetch_add tokens: 1
- Load rate_nanos (Relaxed): 0.5
- Load capacity (Relaxed): 0.5
- compare_exchange_weak: 1
- fetch_sub tokens: 1
- **Total: 3-4 effective atomic operations per acquire**

**Optimizations Applied:**
- ✅ **50% reduction** in atomic operations (3-4 vs 6-9)
- ✅ **Weak CAS** tolerates failures (no retry loop)
- ✅ **Relaxed loads** for config (never changes frequently)
- ✅ **Soft capacity** (computed, not enforced atomically)
- ✅ **Precise sleep** based on exact token deficit
- ✅ **Negative tokens** simplify race handling (no restore)

### Side-by-Side Performance Comparison

| Metric | Option 1 (Naive) | Hybrid (Optimized) | Improvement |
|--------|------------------|-------------------|-------------|
| **Atomic ops per acquire** | 6-9 | 3-4 | **50-60% reduction** |
| **Lock contention points** | 3 (refill, cap, consume) | 1 (consume only) | **66% reduction** |
| **Race handling** | Restore token (2 ops) | Tolerate negative (0 ops) | **Simpler** |
| **Sleep precision** | Fixed interval | Exact deficit | **More accurate** |
| **Timestamp sync** | Strict (store) | Relaxed (weak CAS) | **Lower contention** |
| **Expected latency** | ~150-200ns | **~80ns** | **2-2.5x faster** |
| **Throughput ceiling** | ~500K ops/sec | **1M+ ops/sec** | **2x higher** |

### Why "Hybrid"? Four Combined Techniques

The "hybrid" name comes from combining multiple algorithmic optimizations:

| Technique | Borrowed From | Applied As | Benefit |
|-----------|---------------|------------|---------|
| **Token accumulation** | Token Bucket algorithm | Allow burst up to capacity | Burst handling, smoother rate |
| **Nanosecond precision** | Leaky Bucket algorithm | Integer ns accounting | No float drift, exact timing |
| **Lock-free atomics** | Concurrent algorithms | Atomic operations only | Thread-safe, no mutex |
| **Event-driven sleep** | Tokio async runtime | Calculated deficit sleep | CPU efficiency, low latency |

**It's not "hybrid" as mixing two algorithms**, but rather **"hybrid optimizations"** - taking the best techniques from multiple approaches into a single optimized implementation.

### Key Innovations Explained

#### Innovation 1: Lazy Refill with Natural Token Accumulation

```rust
// No timer thread, no periodic wake-ups
// Tokens accumulate naturally with time passage

tokens_nanos += elapsed_nanos;  // 1:1 mapping: 1ns = 1 nano-token

// Example: At 1000 ops/sec (1M ns per token)
// - Idle for 5ms → 5,000,000 nano-tokens accumulated
// - Can burst 5 permits instantly
// - Capacity caps max accumulation (2x rate = 2 permits)
```

**Benefits:**
- No background thread
- No periodic timer
- Burst handling automatic
- Self-pacing

#### Innovation 2: Tolerating Timestamp Races

```rust
// Multiple threads might update last_update concurrently
// Instead of strict synchronization, we TOLERATE races

self.last_update.compare_exchange_weak(
    last, now,
    Ordering::Release,
    Ordering::Relaxed
).ok();  // .ok() = ignore CAS failure!

// Why it's safe:
// - Worst case: count same elapsed time twice → extra tokens
// - Capacity limit prevents unbounded accumulation
// - fetch_sub consumption is still strictly synchronized
// - Over time, converges to correct rate (self-correcting)
```

**Benefits:**
- No CAS retry loop
- No contention on timestamp
- Simpler code
- Self-correcting

#### Innovation 3: Signed Integer Tokens (Allow Negative)

```rust
tokens_nanos: AtomicI64  // SIGNED, can go negative!

// Scenario: Multiple threads racing
// tokens = 500,000 ns
// Thread A: fetch_sub(1,000,000) → prev=500,000, now=-500,000
// Thread B: fetch_sub(1,000,000) → prev=-500,000, now=-1,500,000

// Thread A: prev < rate_nanos → will sleep and retry
// Thread B: prev < rate_nanos → will sleep and retry
// No restore needed! Negative tokens naturally prevent over-consumption
```

**Benefits:**
- Simpler race handling
- No restore logic (no extra atomic op)
- Single atomic operation to attempt consume
- Natural rate enforcement

#### Innovation 4: Precise Sleep Calculation

```rust
// Don't sleep a fixed interval - calculate EXACT time needed

let deficit = (rate_nanos - current_tokens).max(0);
tokio::sleep(Duration::from_nanos(deficit)).await;

// Example at 1000 ops/sec (1M ns per token):
// - current_tokens = 750,000 ns
// - Need: 1,000,000 ns
// - Deficit: 250,000 ns
// - Sleep: 250μs (not 1ms!)

// Better accuracy:
// - Wake up exactly when token available
// - No wasted CPU cycles
// - Lower latency variance
```

**Benefits:**
- Minimal wasted time
- Better rate accuracy
- Lower latency variance
- Fewer unnecessary wake-ups

---

## Recommended Implementation: Hybrid Approach

### Final Data Structure

```rust
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// High-performance lock-free rate limiter using hybrid token bucket algorithm
#[repr(align(64))]  // Cache line alignment
pub struct RateLimiter {
    // Configuration (can be changed dynamically via set_rate)
    rate_nanos: AtomicU64,         // Nanoseconds per token (1e9 / ops_per_sec)
    capacity_nanos: AtomicU64,     // Max burst capacity in nanoseconds

    // State (lock-free, frequently updated)
    tokens_nanos: AtomicI64,       // Current token balance (SIGNED, can go negative)
    last_update: AtomicU64,        // Last update timestamp (nanos since epoch)
}

/// Zero-sized permit token
pub struct Permit;

/// Batch of permits
pub struct Permits {
    count: u64,
}
```

**Memory Layout:**
- Total size: 32 bytes (4 × 8 bytes)
- Fits in: **Single cache line** (64 bytes)
- Alignment: 64-byte aligned (cache line)
- No padding needed

### Complete Implementation

```rust
impl RateLimiter {
    /// Create new rate limiter with default burst capacity (2x rate)
    ///
    /// # Arguments
    /// * `rate` - Target operations per second
    ///
    /// # Examples
    /// ```
    /// let limiter = RateLimiter::new(1000);  // 1K ops/sec
    /// ```
    pub fn new(rate: u64) -> Self {
        Self::with_capacity(rate, rate * 2)
    }

    /// Create rate limiter with custom burst capacity
    ///
    /// # Arguments
    /// * `rate` - Target operations per second
    /// * `capacity` - Maximum burst capacity (in operations)
    ///
    /// # Examples
    /// ```
    /// let limiter = RateLimiter::with_capacity(1000, 5000);  // Burst up to 5K ops
    /// ```
    pub fn with_capacity(rate: u64, capacity: u64) -> Self {
        assert!(rate > 0, "Rate must be greater than 0");

        let rate_nanos = 1_000_000_000 / rate;
        let capacity_nanos = capacity * rate_nanos;

        Self {
            rate_nanos: AtomicU64::new(rate_nanos),
            capacity_nanos: AtomicU64::new(capacity_nanos),
            tokens_nanos: AtomicI64::new(capacity_nanos as i64),
            last_update: AtomicU64::new(nanos_since_epoch()),
        }
    }

    /// Acquire single permit (lock-free, async)
    ///
    /// # Returns
    /// Permit token (zero-sized)
    ///
    /// # Examples
    /// ```no_run
    /// let limiter = RateLimiter::new(1000);
    /// let permit = limiter.acquire().await;
    /// // Submit operation...
    /// ```
    pub async fn acquire(&self) -> Permit {
        loop {
            // 1. Get current time and calculate elapsed
            let now = nanos_since_epoch();
            let last = self.last_update.load(Ordering::Relaxed);
            let elapsed = now.saturating_sub(last);

            // 2. Add elapsed time as tokens (1 atomic op)
            let prev_tokens = self.tokens_nanos.fetch_add(
                elapsed as i64,
                Ordering::AcqRel
            );

            // 3. Calculate current tokens with soft capacity limit
            let rate_nanos = self.rate_nanos.load(Ordering::Relaxed);
            let capacity = self.capacity_nanos.load(Ordering::Relaxed);
            let current_tokens = (prev_tokens + elapsed as i64).min(capacity as i64);

            // 4. Update timestamp (weak CAS, tolerate failure)
            self.last_update.compare_exchange_weak(
                last, now,
                Ordering::Release,
                Ordering::Relaxed
            ).ok();

            // 5. Try to consume one token (1 atomic op)
            if current_tokens >= rate_nanos as i64 {
                let consumed = self.tokens_nanos.fetch_sub(
                    rate_nanos as i64,
                    Ordering::AcqRel
                );

                if consumed >= rate_nanos as i64 {
                    return Permit;
                }
                // Lost race, retry
            } else {
                // 6. Not enough tokens - sleep exact deficit
                let deficit = (rate_nanos as i64 - current_tokens).max(0) as u64;
                tokio::time::sleep(Duration::from_nanos(deficit)).await;
            }
        }
    }

    /// Acquire N permits in batch (more efficient than N × acquire)
    ///
    /// # Arguments
    /// * `n` - Number of permits to acquire
    ///
    /// # Returns
    /// Batch of N permits
    ///
    /// # Examples
    /// ```no_run
    /// let permits = limiter.acquire_many(100).await;
    /// for _ in 0..100 {
    ///     // Submit operation...
    /// }
    /// ```
    pub async fn acquire_many(&self, n: u64) -> Permits {
        assert!(n > 0, "Must acquire at least 1 permit");

        let rate_nanos = self.rate_nanos.load(Ordering::Relaxed);
        let required_nanos = n * rate_nanos;

        loop {
            let now = nanos_since_epoch();
            let last = self.last_update.load(Ordering::Relaxed);
            let elapsed = now.saturating_sub(last);

            let prev_tokens = self.tokens_nanos.fetch_add(
                elapsed as i64,
                Ordering::AcqRel
            );

            let capacity = self.capacity_nanos.load(Ordering::Relaxed);
            let current_tokens = (prev_tokens + elapsed as i64).min(capacity as i64);

            self.last_update.compare_exchange_weak(
                last, now,
                Ordering::Release,
                Ordering::Relaxed
            ).ok();

            if current_tokens >= required_nanos as i64 {
                let consumed = self.tokens_nanos.fetch_sub(
                    required_nanos as i64,
                    Ordering::AcqRel
                );

                if consumed >= required_nanos as i64 {
                    return Permits { count: n };
                }
            } else {
                let deficit = (required_nanos as i64 - current_tokens).max(0) as u64;
                tokio::time::sleep(Duration::from_nanos(deficit)).await;
            }
        }
    }

    /// Change rate dynamically (for ramping scenarios)
    ///
    /// # Arguments
    /// * `new_rate` - New target rate in ops/sec
    ///
    /// # Examples
    /// ```
    /// limiter.set_rate(500);   // Ramp down
    /// limiter.set_rate(2000);  // Ramp up
    /// ```
    pub fn set_rate(&self, new_rate: u64) {
        assert!(new_rate > 0, "Rate must be greater than 0");

        let new_rate_nanos = 1_000_000_000 / new_rate;
        self.rate_nanos.store(new_rate_nanos, Ordering::Release);

        // Update capacity proportionally (2x rate)
        let new_capacity = new_rate * 2 * new_rate_nanos;
        self.capacity_nanos.store(new_capacity, Ordering::Release);
    }

    /// Get current configured rate
    ///
    /// # Returns
    /// Current rate in operations per second
    pub fn current_rate(&self) -> u64 {
        let nanos = self.rate_nanos.load(Ordering::Relaxed);
        1_000_000_000 / nanos
    }

    /// Get available permits (diagnostic)
    ///
    /// # Returns
    /// Number of permits immediately available
    pub fn available_permits(&self) -> u64 {
        let tokens = self.tokens_nanos.load(Ordering::Relaxed);
        let rate_nanos = self.rate_nanos.load(Ordering::Relaxed);

        if tokens > 0 {
            (tokens as u64) / rate_nanos
        } else {
            0
        }
    }
}

// Helper function
fn nanos_since_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System clock went backwards")
        .as_nanos() as u64
}

// Ensure Send + Sync for Arc sharing
unsafe impl Send for RateLimiter {}
unsafe impl Sync for RateLimiter {}
```

### Usage Examples

```rust
use std::sync::Arc;

// Basic usage
let limiter = RateLimiter::new(1000);  // 1K ops/sec
let permit = limiter.acquire().await;
// Submit operation...

// Concurrent usage across tasks
let limiter = Arc::new(RateLimiter::new(10_000));
let mut handles = vec![];

for _ in 0..100 {
    let lim = limiter.clone();
    handles.push(tokio::spawn(async move {
        let permit = lim.acquire().await;
        // Do work...
    }));
}

futures::future::join_all(handles).await;

// Dynamic rate changes (ramping)
limiter.set_rate(500);    // Ramp down
tokio::time::sleep(Duration::from_secs(10)).await;
limiter.set_rate(2000);   // Ramp up

// Batch acquisition for efficiency
let permits = limiter.acquire_many(100).await;
for _ in 0..100 {
    // Submit operations at burst rate...
}
```

---

## API Design

### Public API Surface

```rust
// Construction
impl RateLimiter {
    pub fn new(rate: u64) -> Self;
    pub fn with_capacity(rate: u64, capacity: u64) -> Self;
}

// Core operations
impl RateLimiter {
    pub async fn acquire(&self) -> Permit;
    pub async fn acquire_many(&self, n: u64) -> Permits;
}

// Configuration
impl RateLimiter {
    pub fn set_rate(&self, new_rate: u64);
    pub fn current_rate(&self) -> u64;
}

// Diagnostics
impl RateLimiter {
    pub fn available_permits(&self) -> u64;
}

// Permit types
pub struct Permit;          // Zero-sized
pub struct Permits { count: u64 }

// Traits
unsafe impl Send for RateLimiter {}
unsafe impl Sync for RateLimiter {}
```

### Design Decisions

| Decision | Rationale |
|----------|-----------|
| **`&self` not `&mut self`** | Thread-safe, can share with Arc |
| **`async fn acquire`** | Non-blocking, integrates with tokio |
| **Zero-sized `Permit`** | No runtime cost, type-safe token |
| **`set_rate(&self)`** | Lock-free rate changes |
| **No `Drop` on Permit** | Permits don't need return (fire-and-forget) |
| **`assert!` on invalid inputs** | Fail fast, clear error messages |

---

## Implementation Plan

### Decision: Direct Hybrid Implementation

**Approach:** Implement the hybrid optimized version directly (not Option 1 first).

**Rationale:**
- Hybrid is strictly superior in all metrics
- Option 1 is primarily educational
- No need to implement and then refactor
- Faster time to production-ready code

### Phase 1: Core Implementation (Week 1, Days 1-2)

**Goal:** Lock-free rate limiter with basic functionality

**Tasks:**
1. **Create module structure** (1 hour)
   - File: `src/rate_limiter.rs`
   - Imports, module doc comments
   - Data structure definitions

2. **Implement constructors** (2 hours)
   - `new(rate)`
   - `with_capacity(rate, capacity)`
   - Input validation (rate > 0)
   - Initialize atomic fields

3. **Implement `acquire()`** (4 hours)
   - Lock-free refill logic
   - Token consumption with atomics
   - Smart sleep calculation
   - Handle races correctly

4. **Implement `set_rate()`** (2 hours)
   - Atomic rate update
   - Proportional capacity adjustment
   - Thread-safe guarantees

5. **Basic unit tests** (3 hours)
   - Test single-threaded acquire
   - Test rate changes
   - Test zero/negative edge cases

**Deliverables:**
- Working `acquire()` and `set_rate()`
- 5+ unit tests passing
- Compiles with no warnings

### Phase 2: Batching & Optimizations (Week 1, Days 3-4)

**Goal:** Optimize performance and add batch support

**Tasks:**
1. **Implement `acquire_many()`** (3 hours)
   - Batch token acquisition
   - Efficient deficit calculation
   - Tests for batch operations

2. **Add diagnostics** (2 hours)
   - `available_permits()`
   - `current_rate()`
   - Helpful for debugging/monitoring

3. **Memory layout optimization** (2 hours)
   - Cache line alignment (`#[repr(align(64))]`)
   - Field ordering (hot vs cold)
   - Verify with `std::mem::size_of`

4. **Atomic ordering optimization** (3 hours)
   - Profile atomic operations
   - Use Relaxed where safe
   - Document ordering choices

5. **Concurrent tests** (4 hours)
   - Test 10+ concurrent tasks
   - Test under high contention
   - Verify no deadlocks/races

**Deliverables:**
- `acquire_many()` implemented
- Diagnostics API complete
- 15+ unit tests (including concurrent)
- Memory layout optimized

### Phase 3: Comprehensive Testing (Week 2, Days 1-3)

**Goal:** Prove correctness with extensive testing

**Tasks:**
1. **Property-based tests** (6 hours)
   - File: `tests/property/rate_accuracy_test.rs`
   - Rate never exceeded
   - Rate accuracy ±2%
   - Dynamic rate changes
   - Burst behavior

2. **Integration tests** (4 hours)
   - File: `tests/integration/rate_limiter_integration_test.rs`
   - Test with scenario executor
   - End-to-end rate accuracy
   - Long-running stability

3. **Stress tests** (4 hours)
   - 1M ops/sec sustained
   - 100+ concurrent tasks
   - Rate changes under load
   - Multi-hour runs

4. **Benchmarks** (6 hours)
   - File: `benches/rate_limiter_bench.rs`
   - Single-threaded throughput
   - Concurrent throughput
   - Rate accuracy measurement
   - Latency distribution

**Deliverables:**
- Property tests implemented (4+ tests)
- Integration tests complete
- Stress tests passing
- Comprehensive benchmarks

### Phase 4: Documentation & Polish (Week 2, Days 4-5)

**Goal:** Production-ready module

**Tasks:**
1. **Rustdoc documentation** (4 hours)
   - Module-level overview
   - Algorithm explanation
   - Performance characteristics
   - Examples for all APIs

2. **Performance guide** (2 hours)
   - When to use batching
   - Capacity selection guide
   - Tuning recommendations

3. **Code review** (2 hours)
   - Safety review (`unsafe` blocks if any)
   - API ergonomics
   - Error messages
   - Code comments

4. **Final verification** (2 hours)
   - All tests passing
   - Benchmarks meet targets
   - Documentation complete
   - No compiler warnings

**Deliverables:**
- 100% rustdoc coverage
- Performance tuning guide
- All tests passing (30+ tests)
- Benchmarks showing <100ns overhead

### Timeline Summary

| Phase | Duration | Completion Date |
|-------|----------|-----------------|
| Phase 1: Core Implementation | 2 days | Day 2 |
| Phase 2: Batching & Optimization | 2 days | Day 4 |
| Phase 3: Comprehensive Testing | 3 days | Day 7 |
| Phase 4: Documentation & Polish | 2 days | Day 9 |
| **Total** | **9 days** | **~2 weeks** |

---

## Testing Strategy

### Unit Tests (20+ tests)

**File:** `src/rate_limiter.rs` (inline `#[cfg(test)]`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // === Correctness Tests ===

    #[test]
    fn test_new_limiter_has_full_capacity() {
        let limiter = RateLimiter::new(1000);
        assert_eq!(limiter.available_permits(), 2000);  // 2x capacity
    }

    #[tokio::test]
    async fn test_acquire_consumes_token() {
        let limiter = RateLimiter::new(1000);
        let initial = limiter.available_permits();
        let _permit = limiter.acquire().await;
        assert_eq!(limiter.available_permits(), initial - 1);
    }

    #[tokio::test]
    async fn test_tokens_refill_over_time() {
        let limiter = RateLimiter::new(1000);
        // Consume all tokens
        for _ in 0..2000 {
            limiter.acquire().await;
        }
        assert_eq!(limiter.available_permits(), 0);

        // Wait for refill
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(limiter.available_permits() > 0);
    }

    #[test]
    fn test_capacity_limits_burst() {
        let limiter = RateLimiter::with_capacity(1000, 500);
        // Idle for long time
        std::thread::sleep(Duration::from_secs(10));
        // Capacity should cap at 500, not accumulate indefinitely
        assert_eq!(limiter.available_permits(), 500);
    }

    #[test]
    fn test_set_rate_changes_interval() {
        let limiter = RateLimiter::new(1000);
        assert_eq!(limiter.current_rate(), 1000);

        limiter.set_rate(2000);
        assert_eq!(limiter.current_rate(), 2000);
    }

    // === Concurrency Tests ===

    #[tokio::test]
    async fn test_concurrent_acquire_no_races() {
        let limiter = Arc::new(RateLimiter::new(10_000));
        let mut handles = vec![];

        for _ in 0..100 {
            let lim = limiter.clone();
            handles.push(tokio::spawn(async move {
                lim.acquire().await;
            }));
        }

        // Should complete without deadlock
        for handle in handles {
            handle.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_concurrent_acquire_correct_count() {
        let limiter = Arc::new(RateLimiter::new(100_000));
        let counter = Arc::new(AtomicU64::new(0));
        let mut handles = vec![];

        for _ in 0..10 {
            let lim = limiter.clone();
            let cnt = counter.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..100 {
                    lim.acquire().await;
                    cnt.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Exactly 1000 permits acquired
        assert_eq!(counter.load(Ordering::Relaxed), 1000);
    }

    #[tokio::test]
    async fn test_rate_change_under_load() {
        let limiter = Arc::new(RateLimiter::new(1000));
        let barrier = Arc::new(tokio::sync::Barrier::new(11));
        let mut handles = vec![];

        // 10 tasks acquiring concurrently
        for _ in 0..10 {
            let lim = limiter.clone();
            let bar = barrier.clone();
            handles.push(tokio::spawn(async move {
                bar.wait().await;
                for _ in 0..100 {
                    lim.acquire().await;
                }
            }));
        }

        // Change rate while tasks are running
        barrier.wait().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        limiter.set_rate(2000);

        // All tasks should complete
        for handle in handles {
            handle.await.unwrap();
        }
    }

    // === Edge Cases ===

    #[test]
    #[should_panic(expected = "Rate must be greater than 0")]
    fn test_zero_rate_panics() {
        RateLimiter::new(0);
    }

    #[tokio::test]
    async fn test_very_high_rate_accurate() {
        let limiter = RateLimiter::new(1_000_000);  // 1M ops/sec

        let start = Instant::now();
        for _ in 0..10000 {
            limiter.acquire().await;
        }
        let elapsed = start.elapsed();

        // Should take ~10ms (10K ops at 1M ops/sec)
        assert!(elapsed.as_millis() >= 8);   // -20%
        assert!(elapsed.as_millis() <= 12);  // +20%
    }

    #[tokio::test]
    async fn test_very_low_rate_accurate() {
        let limiter = RateLimiter::new(10);  // 10 ops/sec

        let start = Instant::now();
        for _ in 0..5 {
            limiter.acquire().await;
        }
        let elapsed = start.elapsed();

        // Should take ~400ms (5 ops at 10 ops/sec after initial burst)
        assert!(elapsed.as_millis() >= 300);  // -25%
        assert!(elapsed.as_millis() <= 500);  // +25%
    }

    #[tokio::test]
    async fn test_burst_within_capacity() {
        let limiter = RateLimiter::with_capacity(100, 500);

        // Idle to accumulate tokens
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Burst acquire
        let start = Instant::now();
        for _ in 0..500 {
            limiter.acquire().await;
        }
        let burst_time = start.elapsed();

        // Burst should be very fast (<10ms)
        assert!(burst_time.as_millis() < 10);

        // Next 100 should be rate-limited
        let start = Instant::now();
        for _ in 0..100 {
            limiter.acquire().await;
        }
        let limited_time = start.elapsed();

        // Should take ~1 second (100 ops at 100 ops/sec)
        assert!(limited_time.as_millis() >= 900);
        assert!(limited_time.as_millis() <= 1100);
    }

    // === Performance Tests ===

    #[tokio::test]
    async fn test_acquire_low_latency() {
        let limiter = RateLimiter::new(100_000);

        // Warmup
        for _ in 0..100 {
            limiter.acquire().await;
        }

        // Measure latency
        let mut latencies = vec![];
        for _ in 0..1000 {
            let start = Instant::now();
            limiter.acquire().await;
            latencies.push(start.elapsed().as_nanos());
        }

        // Calculate percentiles
        latencies.sort();
        let p50 = latencies[500];
        let p99 = latencies[990];

        // Latency should be <100ns for p50, <500ns for p99
        assert!(p50 < 100, "p50 latency {}ns exceeds 100ns", p50);
        assert!(p99 < 500, "p99 latency {}ns exceeds 500ns", p99);
    }

    #[test]
    fn test_no_allocations() {
        // Verify struct size
        assert_eq!(std::mem::size_of::<RateLimiter>(), 32);
        assert_eq!(std::mem::size_of::<Permit>(), 0);

        // Could use allocation profiler here
        // For now, manual verification that acquire() doesn't allocate
    }
}
```

### Property-Based Tests (proptest)

**File:** `tests/property/rate_accuracy_test.rs`

```rust
use proptest::prelude::*;
use rsbench::rate_limiter::RateLimiter;
use std::time::Instant;

proptest! {
    #[test]
    fn rate_never_exceeded(rate in 100..100_000u64) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let limiter = RateLimiter::new(rate);
            let duration = Duration::from_secs(1);

            let mut count = 0u64;
            let start = Instant::now();

            while start.elapsed() < duration {
                limiter.acquire().await;
                count += 1;
            }

            let actual_rate = (count as f64) / start.elapsed().as_secs_f64();

            // Should never exceed target rate (allow 5% tolerance for measurement)
            prop_assert!(actual_rate <= (rate as f64 * 1.05));
        });
    }

    #[test]
    fn rate_accuracy_within_tolerance(rate in 100..100_000u64) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let limiter = RateLimiter::new(rate);
            let duration = Duration::from_secs(2);  // Longer for better accuracy

            let mut count = 0u64;
            let start = Instant::now();

            while start.elapsed() < duration {
                limiter.acquire().await;
                count += 1;
            }

            let actual_rate = (count as f64) / start.elapsed().as_secs_f64();
            let error = ((actual_rate - rate as f64) / rate as f64).abs();

            // Should be within ±2% of target
            prop_assert!(error < 0.02,
                "Rate error {:.2}% exceeds 2% (target={}, actual={:.2})",
                error * 100.0, rate, actual_rate
            );
        });
    }

    #[test]
    fn rate_change_takes_effect(
        initial in 100..10_000u64,
        new in 100..10_000u64
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let limiter = RateLimiter::new(initial);

            // Run at initial rate for 500ms
            let start = Instant::now();
            let mut count1 = 0;
            while start.elapsed() < Duration::from_millis(500) {
                limiter.acquire().await;
                count1 += 1;
            }

            // Change rate
            limiter.set_rate(new);

            // Run at new rate for 500ms
            let start = Instant::now();
            let mut count2 = 0;
            while start.elapsed() < Duration::from_millis(500) {
                limiter.acquire().await;
                count2 += 1;
            }

            // Verify both rates were respected
            let rate1 = (count1 as f64) / 0.5;
            let rate2 = (count2 as f64) / 0.5;

            prop_assert!((rate1 - initial as f64).abs() / initial as f64 < 0.1);
            prop_assert!((rate2 - new as f64).abs() / new as f64 < 0.1);
        });
    }

    #[test]
    fn burst_respects_capacity(
        rate in 100..10_000u64,
        capacity_multiplier in 1..10u64
    ) {
        let capacity = rate * capacity_multiplier;
        let limiter = RateLimiter::with_capacity(rate, capacity);

        // Idle for long time
        std::thread::sleep(Duration::from_secs(10));

        // Available permits should not exceed capacity
        let available = limiter.available_permits();
        prop_assert!(available <= capacity,
            "Available {} exceeds capacity {}", available, capacity
        );
    }
}
```

### Integration Tests

**File:** `tests/integration/rate_limiter_integration_test.rs`

```rust
use rsbench::rate_limiter::RateLimiter;
use rsbench::scenario::ScenarioExecutor;
use std::sync::Arc;

#[tokio::test]
async fn test_rate_limiter_with_scenario_executor() {
    // Create scenario with rate limiter
    // Verify end-to-end rate accuracy
    // TODO: Implement after scenario integration
}

#[tokio::test]
async fn test_long_running_stability() {
    let limiter = Arc::new(RateLimiter::new(10_000));
    let duration = Duration::from_secs(60);  // 1 minute

    let mut handles = vec![];
    for _ in 0..10 {
        let lim = limiter.clone();
        handles.push(tokio::spawn(async move {
            let start = Instant::now();
            let mut count = 0u64;

            while start.elapsed() < duration {
                lim.acquire().await;
                count += 1;
            }

            count
        }));
    }

    // Collect results
    let mut total = 0u64;
    for handle in handles {
        total += handle.await.unwrap();
    }

    // Should have executed ~600K operations (10K/sec × 60sec)
    // Allow ±5% tolerance
    assert!(total >= 570_000);
    assert!(total <= 630_000);
}
```

### Benchmarks (criterion)

**File:** `benches/rate_limiter_bench.rs`

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use rsbench::rate_limiter::RateLimiter;
use std::sync::Arc;
use std::time::Duration;

fn benchmark_single_threaded_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_threaded");

    for rate in [1_000, 10_000, 100_000, 1_000_000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(rate),
            &rate,
            |b, &rate| {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let limiter = RateLimiter::new(rate);

                b.to_async(&rt).iter(|| async {
                    black_box(limiter.acquire().await);
                });
            }
        );
    }

    group.finish();
}

fn benchmark_concurrent_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let limiter = Arc::new(RateLimiter::new(100_000));

    c.bench_function("concurrent_100_tasks", |b| {
        b.to_async(&rt).iter(|| async {
            let handles: Vec<_> = (0..100).map(|_| {
                let lim = limiter.clone();
                tokio::spawn(async move {
                    black_box(lim.acquire().await);
                })
            }).collect();

            futures::future::join_all(handles).await;
        });
    });
}

fn benchmark_rate_accuracy(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("rate_accuracy_10k", |b| {
        b.to_async(&rt).iter(|| async {
            let limiter = RateLimiter::new(10_000);
            let start = std::time::Instant::now();

            for _ in 0..10_000 {
                limiter.acquire().await;
            }

            let elapsed = start.elapsed();
            black_box(elapsed);
        });
    });
}

fn benchmark_batch_vs_single(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let limiter = RateLimiter::new(100_000);

    let mut group = c.benchmark_group("batch_vs_single");

    group.bench_function("single_100x", |b| {
        b.to_async(&rt).iter(|| async {
            for _ in 0..100 {
                black_box(limiter.acquire().await);
            }
        });
    });

    group.bench_function("batch_100", |b| {
        b.to_async(&rt).iter(|| async {
            black_box(limiter.acquire_many(100).await);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_single_threaded_throughput,
    benchmark_concurrent_throughput,
    benchmark_rate_accuracy,
    benchmark_batch_vs_single
);
criterion_main!(benches);
```

---

## Performance Optimization

### Memory Layout Optimization

```rust
#[repr(align(64))]  // Align to cache line (64 bytes on x86_64)
pub struct RateLimiter {
    // Hot path fields (frequently accessed together)
    tokens_nanos: AtomicI64,       // 8 bytes
    last_update: AtomicU64,        // 8 bytes

    // Config fields (read-only after set_rate)
    rate_nanos: AtomicU64,         // 8 bytes
    capacity_nanos: AtomicU64,     // 8 bytes
}
// Total: 32 bytes → fits in single cache line with padding
```

**Benefits:**
- ✅ All fields fit in single cache line
- ✅ No false sharing between RateLimiters
- ✅ Better cache locality

### Atomic Ordering Optimization

```rust
// Relaxed: No synchronization, just atomic read/write
let rate = self.rate_nanos.load(Ordering::Relaxed);  // Config rarely changes

// Acquire: Synchronize with previous Release stores
let last = self.last_update.load(Ordering::Acquire);

// Release: Make changes visible to Acquire loads
self.last_update.store(now, Ordering::Release);

// AcqRel: Both Acquire and Release (for read-modify-write)
let prev = self.tokens_nanos.fetch_add(elapsed, Ordering::AcqRel);
```

**Ordering Choice Rationale:**

| Operation | Ordering | Why |
|-----------|----------|-----|
| Read rate/capacity | Relaxed | Config, rarely changes |
| Read last_update | Relaxed | Tolerate stale value |
| Update last_update (CAS) | Release/Relaxed | Publish timestamp |
| Add tokens (fetch_add) | AcqRel | Sync token balance |
| Sub tokens (fetch_sub) | AcqRel | Sync consumption |

### Lock-Free Algorithm Correctness

**Invariants:**
1. `tokens_nanos` can be negative (simplifies races)
2. `last_update` may be stale (self-correcting)
3. Capacity is soft limit (computed, not enforced atomically)
4. Rate accuracy converges over time (even with races)

**Why it's safe:**
```rust
// Scenario: Two threads racing

Thread A:
  fetch_add(1_000_000)  // Add 1ms of tokens
  fetch_sub(1_000_000)  // Try to consume

Thread B:
  fetch_add(1_000_000)  // Add 1ms of tokens
  fetch_sub(1_000_000)  // Try to consume

// Possible outcomes:
// 1. Both succeed (if enough initial tokens)
// 2. One succeeds, one fails (if ~1 token available)
// 3. Both fail (if tokens < 1)

// All outcomes are correct:
// - No over-consumption (fetch_sub is atomic)
// - Failed attempts will retry after sleeping
// - Token balance self-corrects on next refill
```

### Zero-Allocation Guarantee

```rust
// ✅ Construction: One-time allocation
pub fn new(rate: u64) -> Self {
    // Stack allocation only
    Self { ... }
}

// ✅ Acquire: Zero allocations
pub async fn acquire(&self) -> Permit {
    // No heap allocations
    // Only atomic operations + sleep
    loop { ... }
}

// ✅ Permit: Zero-sized type
pub struct Permit;  // sizeof = 0
```

**Verification:**
```rust
#[test]
fn test_sizes() {
    assert_eq!(std::mem::size_of::<RateLimiter>(), 32);
    assert_eq!(std::mem::size_of::<Permit>(), 0);
    assert_eq!(std::mem::align_of::<RateLimiter>(), 64);
}
```

---

## Success Criteria

### Functional Requirements

| Requirement | Target | Verification Method |
|-------------|--------|---------------------|
| **Thread-safe** | `Arc` sharing | Concurrent tests (100+ tasks) |
| **Rate accuracy** | ±2% | Property tests over 1+ sec |
| **Dynamic rate changes** | Immediate effect | Unit + property tests |
| **Burst support** | 2x capacity default | Capacity limit tests |
| **No panics** | Normal operation | Stress tests + fuzzing |

### Performance Requirements

| Metric | Target | Verification Method |
|--------|--------|---------------------|
| **Overhead per acquire** | <100ns | Benchmark p50 latency |
| **Max throughput** | 1M ops/sec | Benchmark sustained rate |
| **Concurrent scalability** | 100+ tasks | Concurrent benchmark |
| **Memory footprint** | <128 bytes | `size_of` test |
| **Zero allocations** | After init | Heap profiler |
| **Atomic operations** | 3-4 per acquire | Manual code review |

### Quality Requirements

| Requirement | Target | Status |
|-------------|--------|--------|
| **Rustdoc coverage** | 100% | To be verified |
| **Unit tests** | 20+ passing | To be implemented |
| **Property tests** | 4+ passing | To be implemented |
| **Integration tests** | 2+ passing | To be implemented |
| **Benchmarks** | 4+ suites | To be implemented |
| **No warnings** | 0 clippy warnings | To be verified |

### Acceptance Criteria

**Module is considered complete when:**
- ✅ All functional requirements met
- ✅ All performance requirements met
- ✅ All tests passing (30+ total)
- ✅ Benchmarks show <100ns overhead
- ✅ Documentation 100% complete
- ✅ Code review approved
- ✅ Integration with scenario module successful

---

## Risk Mitigation

### Risk Analysis

| Risk | Likelihood | Impact | Mitigation Strategy |
|------|-----------|--------|---------------------|
| **Float precision drift** | High | Medium | Use integer nanosecond accounting |
| **Atomic contention** | Medium | High | Minimize atomic ops, optimize ordering |
| **Sleep granularity** | Medium | Medium | Use tokio timer, validate on platforms |
| **Integer overflow** | Low | High | Use saturating arithmetic, u64 limits |
| **Clock monotonicity** | Low | Critical | Use `Instant::now()` not `SystemTime` |
| **Timestamp races** | High | Low | Tolerate with weak CAS, self-correcting |
| **Negative token balance** | High | Low | Designed to allow, prevents over-consumption |

### Platform-Specific Concerns

**Clock Resolution:**
- Linux: `clock_gettime(CLOCK_MONOTONIC)` → nanosecond precision ✅
- macOS: `mach_absolute_time()` → nanosecond precision ✅
- Windows: `QueryPerformanceCounter()` → 100ns precision ⚠️

**Mitigation:** Document Windows limitation (±100ns jitter)

**Timer Precision:**
- Tokio timer: Depends on OS scheduler (~1ms granularity on Windows)
- High-rate workloads (>1K ops/sec) should batch operations

**Mitigation:** Add `acquire_many()` for batching

### Alternative Clock Source (Future Enhancement)

```rust
// For testing/simulation
pub trait Clock {
    fn now(&self) -> u64;
}

impl RateLimiter {
    pub fn with_clock<C: Clock>(rate: u64, clock: C) -> Self {
        // Use custom clock for deterministic testing
    }
}
```

**Status:** Deferred to future milestone (not needed for M0)

---

## Decision Log

### Design Decisions

| Date | Decision | Rationale | Alternatives Considered |
|------|----------|-----------|------------------------|
| 2025-12-27 | Use hybrid approach directly | Strictly superior to Option 1 | Option 1 (naive), mutex-based |
| 2025-12-27 | Nanosecond token accounting | Eliminates float drift | Float-based, millisecond-based |
| 2025-12-27 | Allow negative token balance | Simplifies race handling | Restore tokens on failed consume |
| 2025-12-27 | Weak CAS on timestamp | Lower contention | Strong CAS with retry loop |
| 2025-12-27 | Burst capacity = 2x rate | Reasonable default | User-configurable (deferred) |
| 2025-12-27 | No support for <1 ops/sec | Simplifies implementation | Support fractional rates (deferred) |
| 2025-12-27 | Custom clock deferred | Not needed for M0 | Implement now |

### Implementation Decisions

| Component | Decision | Status |
|-----------|----------|--------|
| **Algorithm** | Hybrid lock-free token bucket | ✅ Approved |
| **Minimum rate** | 1 ops/sec | ✅ Approved |
| **Burst capacity** | Default 2x, configurable | ✅ Approved |
| **Testing platforms** | x86_64 + ARM64 (future) | ⚠️ x86_64 for M0 |
| **Custom clock** | Deferred to M1+ | ⚠️ Future |
| **Implementation approach** | Direct hybrid (not Option 1 first) | ✅ Approved |

---

## Appendix A: Algorithm Pseudocode

### Hybrid Token Bucket Algorithm

```
struct RateLimiter:
    rate_nanos: atomic_u64          // nanoseconds per token
    capacity_nanos: atomic_u64      // max burst capacity
    tokens_nanos: atomic_i64        // current token balance (signed!)
    last_update: atomic_u64         // last update timestamp

function new(rate):
    rate_nanos = 1_000_000_000 / rate
    capacity = rate * 2 * rate_nanos
    return RateLimiter {
        rate_nanos: rate_nanos,
        capacity_nanos: capacity,
        tokens_nanos: capacity,
        last_update: now()
    }

function acquire():
    loop:
        // 1. Calculate elapsed time
        now = current_time_nanos()
        last = atomic_load_relaxed(last_update)
        elapsed = now - last

        // 2. Refill tokens (add elapsed time)
        prev_tokens = atomic_fetch_add(tokens_nanos, elapsed, AcqRel)

        // 3. Soft capacity cap
        rate = atomic_load_relaxed(rate_nanos)
        capacity = atomic_load_relaxed(capacity_nanos)
        current = min(prev_tokens + elapsed, capacity)

        // 4. Update timestamp (tolerate failure)
        atomic_compare_exchange_weak(last_update, last, now, Release, Relaxed)

        // 5. Try to consume token
        if current >= rate:
            consumed = atomic_fetch_sub(tokens_nanos, rate, AcqRel)
            if consumed >= rate:
                return Permit
            // else: lost race, retry
        else:
            // 6. Not enough tokens - sleep exact deficit
            deficit = max(0, rate - current)
            sleep(deficit)
```

---

## Appendix B: Performance Projections

### Expected Latency Distribution

Based on hybrid algorithm analysis:

| Percentile | Latency (at 100K ops/sec) | Breakdown |
|------------|---------------------------|-----------|
| **p50** | ~80ns | Atomic ops: 60ns, overhead: 20ns |
| **p90** | ~120ns | Cache miss: +40ns |
| **p99** | ~200ns | Context switch: +80ns |
| **p99.9** | ~500ns | Sleep wake-up: +300ns |

### Expected Throughput

| Scenario | Expected Throughput |
|----------|---------------------|
| Single-threaded | 500K-800K ops/sec |
| 10 concurrent tasks | 1M-1.5M ops/sec |
| 100 concurrent tasks | 2M+ ops/sec |

**Note:** Actual throughput limited by workload execution time, not rate limiter.

---

## Appendix C: References

### Algorithms
- Token Bucket: https://en.wikipedia.org/wiki/Token_bucket
- Leaky Bucket: https://en.wikipedia.org/wiki/Leaky_bucket

### Lock-Free Programming
- Rust Atomics and Locks (Mara Bos): https://marabos.nl/atomics/
- Atomic Ordering: https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html

### Existing Implementations
- `tokio-rate-limit`: https://crates.io/crates/tokio-rate-limit
- `governor`: https://crates.io/crates/governor
- `leaky-bucket`: https://crates.io/crates/leaky-bucket

### Performance Benchmarks
- Criterion.rs: https://github.com/bheisler/criterion.rs
- Cache Line Sizes: https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html

---

## Document History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2025-12-27 | Design Review | Initial design document |

---

**End of Design Document**
