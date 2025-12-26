# Documentation Updates Summary

**Date**: 2024-12-26
**Topic**: Executor Pattern Decision, Sysbench Compatibility, Backpressure Awareness, M:N Distributed Architecture

---

## Overview

Comprehensive documentation updates to reflect the architectural decision to replace BlockingRuntime with Closed-Loop Executor pattern and clarify worker architecture.

---

## Files Updated

### 1. ✅ scenario-design.md

**Location**: `/docs/scenario-design.md`

**Changes**:

1. **New Section 2.4: Worker Architecture and Scalability**
   - Line ~184-298
   - Explains workers are Tokio async tasks, not OS threads
   - Tokio M:N threading model diagram
   - Scalability analysis table
   - Memory footprint comparison
   - Sysbench vs RSBench worker comparison

2. **Updated Section 5.3.1: Blocking Runtime Compatibility**
   - Line ~569-642
   - Documents current BlockingRuntime limitations
   - Explains it doesn't provide true sysbench semantics
   - Points to closed-loop executor as replacement
   - Configuration examples for both modes

3. **New Section 11.1: Closed-Loop Executor (BlockingRuntime Replacement)**
   - Line ~1178-1347
   - Complete implementation design
   - Benefits over BlockingRuntime
   - Open-loop vs closed-loop comparison table
   - Migration path
   - Testing strategy

4. **New Appendix C: Sysbench Migration Guide**
   - Line ~1699-1910
   - Complete command mapping (sysbench → RSBench)
   - Workload translation table
   - Parameter mapping table
   - Execution phase mapping
   - Key differences from sysbench
   - Complete migration workflow example
   - Sysbench-compatible CLI wrapper plan

5. **Updated Glossary (Appendix B)**
   - Added: Worker, Closed-Loop, Open-Loop definitions

6. **New Section 5.3.2: Backpressure Awareness Deep Dive**
   - Line ~758-906
   - Complete explanation of what backpressure is (client saturated vs database slow)
   - How RSBench detects backpressure (pool utilization + semaphore saturation)
   - Backpressure metrics table (backpressure_events, backpressure_rate, backpressure_percentage)
   - Example metrics output showing invalid results
   - Interpretation guide (0-1% = valid, >5% = invalid)
   - Configuration examples with backpressure_threshold
   - Best practices for using backpressure as tuning signal
   - Comparison with sysbench (hidden vs explicit)
   - Example: detecting and fixing backpressure
   - Advanced: backpressure event correlation with external events

7. **New Section 11.8: Distributed M:N Architecture (M1+)**
   - Line ~1777-2244
   - Complete M:N distributed testing design
   - M clients (1 leader + M-1 workers) → N database endpoints
   - Architecture diagram showing TiDB 3-region test
   - Configuration schema (infrastructure + scenario configs)
   - Routing strategies (region-affinity, cross-region, read-write-split)
   - Leader responsibilities (worker discovery, routing assignment, phase orchestration, metrics aggregation)
   - Worker implementation with endpoint assignments
   - Multi-endpoint connection pool design
   - Metrics breakdown (per-endpoint, per-worker, global aggregated)
   - Scenario module integration (DistributedMode enum)
   - Complete TiDB 3-region test example with execution flow
   - Event integration (K8s watch, failover testing)
   - Benefits of M:N architecture (8 major points)

8. **New Section 5.6: Event Module Integration** (Line ~953-1489)
   - **~540 lines** of comprehensive parallel module design
   - Module position in architecture (parallel to Scenario, not nested)
   - Communication channel (tokio::mpsc)
   - Integration pattern (main.rs orchestration)
   - Why parallel, not nested (4 key reasons)
   - How Scenario reacts to events (event-driven loop with try_recv)
   - Event types (RateChange, PhaseTransition, MetricsSnapshot, RoutingChange, K8sEvent)
   - Event sources (3 types):
     - K8s Event Watcher (M1+) with configuration examples
     - Webhook Listener (M1+) with Chaos Mesh integration
     - Timer Events (M0) with scheduled phase transitions
   - Use cases (4 detailed scenarios):
     - Failover testing (K8s pod deletion correlation)
     - Rolling upgrade testing (load reduction)
     - Chaos engineering integration (Chaos Mesh/Litmus)
     - Time-based load patterns (daily traffic simulation)
   - Benefits of parallel design (5 points)
   - Distributed mode compatibility (leader receives events, broadcasts to workers)
   - Implementation status (M0, M1, M2+)

9. **Enhanced Section 11.8: Distributed M:N Architecture - Added Loose Coordination** (Line ~2327-2570)
   - **~245 lines** on loose coordination philosophy
   - Motivation: why loose coordination over tight sync
   - Design rationale (4 major points):
     - Simplicity over perfection (gRPC vs consensus protocols)
     - Fault tolerance (worker failure doesn't block others)
     - Performance (zero hot path overhead)
     - Realistic load generation (models real clients)
   - What IS coordinated (minimal: 3 sync points, routing, metrics)
   - What IS NOT coordinated (execution, clocks, workload state, connections)
   - Implementation examples (LeaderOrchestrator, WorkerExecutor)
   - Comparison table (tight sync vs loose coordination, 8 aspects)
   - Benefits of loose coordination (5 major points)
   - Trade-offs and mitigations (3 acceptable trade-offs)
   - Why this is the right design for database load testing

**Content Summary**:
- Added ~150 lines on backpressure awareness (Section 5.3.2)
- Added ~540 lines on Event Module Integration (Section 5.6) - **NEW**
- Added ~470 lines on M:N distributed architecture (Section 11.8)
- Added ~245 lines on loose coordination philosophy (Section 11.8 subsection) - **NEW**
- Total new content: ~1,405 lines

---

### 2. ✅ README.md

**Location**: `/README.md`

**Changes**:

1. **Updated Architecture Section** (Line ~169-216)
   - Removed "Dual-Mode Runtime"
   - Added "Async Runtime" (single implementation)
   - Added "Executor vs Runtime Separation" subsection
   - Added "Worker Model" explanation
   - Added "Executor Patterns" with open-loop and closed-loop descriptions
   - Updated documentation links

2. **Updated Comparison Table** (Line ~250-270)
   - Added "Thread Model" row (OS threads vs Async tasks)
   - Added "Memory (100 threads)" row (~800 MB vs ~20 MB)
   - Changed "Load Generation" row to show both patterns
   - Updated "I/O Model" (Blocking → Async)
   - Added "--threads=N" mapping row
   - Added "--rate=X" mapping row
   - Clarified sysbench compatibility note

---

### 3. ✅ project_structure.md

**Location**: `/docs/project_structure.md`

**Changes**:

1. **Updated Runtime Module Section** (Line ~71-76)
   - Changed from "Implementations" (plural) to "Implementation" (singular)
   - Removed BlockingRuntime mention
   - Added deprecation note

2. **Updated Scenario Module Section** (Line ~96-106)
   - Expanded description with executor types
   - Added open-loop executors (M0)
   - Added closed-loop executor (M0/M1)
   - Clarified worker model
   - Added separation of concerns note

3. **New Section: Executor Pattern (M0 Design Decision)** (Line ~147-177)
   - Explains why executor, not runtime mode
   - Benefits table
   - Previous vs current design comparison
   - Reference to detailed discussion

4. **Updated M0 Status Section** (Line ~174-192)
   - Added checkmarks for declarative workload engine
   - Added checkmarks for open-loop executors
   - **Priority TODO**: Implement closed-loop executor
   - Added: Remove BlockingRuntime (deprecated)

---

### 4. ✅ CLAUDE.md

**Location**: `/CLAUDE.md`

**Changes**:

1. **New Section: Key Architectural Innovations** (Line ~178-302)
   - **Worker Model**: Async tasks vs OS threads explanation
     - Comparison table (sysbench vs RSBench)
     - Memory footprint (800 MB vs 20 MB for 100 workers)
     - Scalability limits (500 threads vs 10K workers)
   - **Backpressure Awareness**: Client saturation visibility
     - What is backpressure (client vs database)
     - How RSBench detects it (pool + semaphore monitoring)
     - Example metrics output with warnings
     - Interpretation guide (0-1% = valid, >20% = invalid)
     - Why this matters (sysbench hides, RSBench shows)
   - **M:N Distributed Architecture**: Multi-region testing
     - Architecture diagram (M clients → N endpoints)
     - Leader vs worker responsibilities
     - Routing strategies (3 types)
     - Why this matters (5 major benefits)
   - **Event-Driven Execution**: Lifecycle testing
     - K8s integration example
     - Failover testing use case
     - Phase transition triggers

2. **Updated Section 8: Scenario Module** (Line ~625-642)
   - Expanded with executor pattern details
   - Added open-loop executors (constant-rate, ramping-rate)
   - Added closed-loop executor (sysbench equivalent)
   - Added distributed mode (M:N architecture)
   - Added critical points (time-driven, worker-driven, event integration)

3. **Updated File References** (Line ~928-940)
   - Added `docs/scenario-design.md` with section references
   - Added `docs/executor-pattern-decision.md` (ADR)
   - Added `docs/DOCUMENTATION_UPDATES.md` (this file)

4. **Updated Footer** (Line ~1021-1031)
   - Changed last updated date: 2025-12-24 → 2025-12-26
   - Added "Recent Updates" section with 6 items

5. **Enhanced Section on Key Architectural Innovations** (Line ~282-329)
   - Updated Event Module description (parallel module, not nested)
   - Added architecture diagram (Scenario ← mpsc ← Event Module)
   - Why parallel design (4 key reasons)
   - Event sources (K8s, Webhook, Timer)
   - How Scenario reacts (event loop with try_recv)
   - Use cases (4 scenarios)
   - Distributed mode note (leader receives, broadcasts)

6. **Enhanced M:N Distributed Architecture** (Line ~244-318)
   - Added "Loose Coordination, NOT Tight Sync" subtitle
   - Comparison box (loose vs tight)
   - What IS coordinated (3 items)
   - What IS NOT coordinated (4 items)
   - Why loose coordination (4 reasons)
   - Updated routing strategies section

**Content Summary**:
- Added ~130 lines on key architectural innovations
- Added ~50 lines on Event Module parallel design - **NEW**
- Added ~75 lines on loose coordination in M:N - **NEW**
- Updated scenario module description (+15 lines)
- Updated file references (+2 entries)
- Total changes: ~270 lines

---

### 5. ✅ executor-pattern-decision.md (NEW)

**Location**: `/docs/executor-pattern-decision.md`

**Purpose**: Architectural Decision Record (ADR)

**Contents**:
- Decision summary and TL;DR table
- Problem statement with BlockingRuntime issues
- Proposed solution (closed-loop executor)
- Worker architecture explanation
- Benefits (4 major points)
- Architecture diagrams (before/after)
- Sysbench compatibility story
- Implementation checklist
- Migration path (3 phases)
- Risks and mitigations
- Success metrics
- Code size comparison

---

## Key Concepts Documented

### 1. Worker Architecture

**What**: Workers are Tokio async tasks (green threads), not OS threads

**Why it matters**:
- 1000 workers = 1000 async tasks on 8 OS threads (vs 1000 OS threads in sysbench)
- Memory: ~20 MB for 100 workers vs ~800 MB for 100 OS threads
- Scalability: Can simulate 10K+ concurrent users efficiently

**Where documented**:
- scenario-design.md: Section 2.4 (detailed)
- README.md: Architecture section (summary)
- project_structure.md: Scenario module (mention)

### 2. Executor Pattern Decision

**What**: Move execution pattern control from Runtime layer to Scenario/Executor layer

**Why**:
- BlockingRuntime doesn't provide true sysbench semantics
- Cleaner separation: Executor = when/how to submit, Runtime = how to execute
- Simpler: One runtime (async) instead of two

**Where documented**:
- executor-pattern-decision.md: Complete ADR
- scenario-design.md: Section 11.1 (detailed design)
- project_structure.md: Executor Pattern section (summary)
- README.md: Architecture section (user-facing)

### 3. Sysbench Compatibility Mapping

**What**: Explicit command and parameter translation guide

**Examples**:
- `sysbench --threads=16` → `executor: { type: closed-loop, workers: 16 }`
- `sysbench --rate=1000` → `executor: { type: constant-rate, rate: 1000 }`
- `oltp_read_write` → `workloads/oltp_read_write.yaml`

**Where documented**:
- scenario-design.md: Appendix C (complete guide)
- executor-pattern-decision.md: Compatibility section
- README.md: Comparison table

### 4. Backpressure Awareness

**What**: Client saturation detection and visibility

**How**:
- Monitor pool utilization (> 80% threshold)
- Track semaphore saturation (no permits available)
- Record backpressure_events counter
- Calculate backpressure_percentage

**Interpretation**:
- 0-1%: ✅ Valid results (measuring database)
- 1-5%: ⚠️ Minor skew (consider increasing max_connections)
- 5-20%: ❌ Invalid (measuring client limits)
- >20%: ❌ Completely invalid (significantly increase capacity)

**Why it matters**:
- Sysbench hides backpressure by blocking threads
- RSBench makes it visible with explicit metrics
- Prevents coordinated omission
- Distinguishes client bottlenecks from database performance

**Where documented**:
- scenario-design.md: Section 5.3.2 (detailed)
- CLAUDE.md: Key Architectural Innovations (summary)
- README.md: Architecture section (mention)

### 5. M:N Distributed Architecture with Loose Coordination

**What**: Multi-region testing with M clients driving load to N database endpoints

**Architecture**:
- M clients: 1 leader + (M-1) workers
- N endpoints: Database nodes in different regions
- Leader: Orchestrates phases, assigns routing, aggregates metrics
- Workers: Execute workload **independently**, report to leader

**Critical Design: Loose Coordination**

**Motivation**:
- Simplicity over perfection (gRPC vs consensus protocols)
- Fault tolerance (worker failure doesn't block others)
- Performance (zero hot path overhead)
- Realism (models real clients, not artificial lockstep)

**What IS Coordinated** (minimal):
1. Phase boundaries (3 sync points: prepare, execute, collect)
2. Routing assignments (once at startup)
3. Metrics aggregation (once at end)

**What IS NOT Coordinated** (independent):
1. Operation execution (each worker at own pace)
2. Clock synchronization (1-2s drift acceptable, no NTP)
3. Workload state (independent RNG per worker)
4. Connection management (each worker own pool)

**Implementation**:
- Simple gRPC calls (fire-and-forget for phase transitions)
- Timeout-based result collection (30s)
- Continue on worker failure (partial results valid)

**Comparison**: ~200 lines vs ~1000+ for tight sync

**Trade-offs** (acceptable):
- Phase drift: 1-2s (negligible for tests > 60s)
- Rate fluctuation: ±5% (use more workers for smoothing)
- Partial results on failure (other workers still valid)

**Routing Strategies**:
- Region affinity: Workers mapped to specific regions
- Cross-region: Operations distributed across all regions
- Read-write split: Reads to all regions, writes to primary

**Use Cases**:
- Test TiDB/CockroachDB/YugabyteDB multi-region deployments
- Measure cross-region latency
- Failover testing during region failures
- Per-endpoint performance breakdown

**Where documented**:
- scenario-design.md: Section 11.8 (complete design with ~245 lines on loose coordination)
- CLAUDE.md: Key Architectural Innovations #3 (summary with loose coordination)
- executor-pattern-decision.md: Future enhancements (mention)

### 6. Event Module Integration (Parallel Module)

**What**: Parallel module for external event-driven orchestration

**Architecture**:
- Event Module runs **alongside** Scenario Module (not nested)
- Communication via tokio::mpsc channel
- Event Module produces, Scenario Module consumes

**Why Parallel**:
- Separation of concerns (event watching ≠ workload execution)
- Composability (can run Scenario without events)
- Testability (test modules independently)
- Reusability (same Event Module for all executors)

**Event Sources**:
1. **K8s Watcher** (M1+): Watch pod/deployment events
2. **Webhook Listener** (M1+): Receive from Chaos Mesh, Prometheus
3. **Timer Events** (M0): Time-based phase transitions

**How Scenario Reacts**:
- Non-blocking `try_recv()` in execution loop
- Event handling: rate change, phase transition, metrics snapshot
- Distributed mode: leader receives, broadcasts to workers

**Use Cases**:
1. Failover testing (K8s pod deletion → latency spike correlation)
2. Rolling upgrade testing (reduce load during upgrades)
3. Chaos engineering (Chaos Mesh integration)
4. Time-based patterns (simulate daily traffic)

**Where documented**:
- scenario-design.md: **Section 5.6** (comprehensive, ~540 lines)
- CLAUDE.md: Key Architectural Innovations #4 (summary)

### 7. Open-Loop vs Closed-Loop

**Open-Loop**:
- Time-driven submission (rate limiter)
- Fire-and-forget execution
- Use case: Load generation, capacity testing
- Executors: ConstantRate, RampingRate

**Closed-Loop**:
- Worker-driven submission
- Wait for completion before next operation
- Use case: User concurrency simulation, sysbench replacement
- Executor: ClosedLoop (M0)

**Where documented**:
- scenario-design.md: Section 2.4, 3.x, 11.1
- README.md: Executor Patterns subsection
- project_structure.md: Scenario module

---

## Documentation Structure

```
docs/
├── README.md                            ✅ Updated
├── scenario-design.md                   ✅ Updated (major additions)
├── project_structure.md                 ✅ Updated
├── executor-pattern-decision.md         ✅ NEW (ADR)
├── DOCUMENTATION_UPDATES.md             ✅ NEW (this file)
├── api_spec_m0.md                       ⏭️ TODO
├── workload-design.md                   ✅ No changes needed
├── declarative-workload-migration.md    ✅ No changes needed
└── m0_progress_summary.md               ⏭️ TODO (update after M0)
```

---

## Migration Impact

### For New Users
- **Impact**: None, new docs explain everything
- **Action**: Read scenario-design.md and README.md

### For Existing Users (if any)
- **Impact**: Low (BlockingRuntime still available in M0)
- **Action**:
  1. Read executor-pattern-decision.md
  2. Migrate configs from `runtime: blocking` to `executor: closed-loop`
  3. Test with new executor
  4. Remove old configs before M1

### For Contributors
- **Impact**: Medium (need to understand new architecture)
- **Action**:
  1. Read all updated docs
  2. Focus on implementing closed-loop executor
  3. Write tests for new executor pattern

---

## Next Steps

### Documentation (Remaining)

1. **api_spec_m0.md**:
   - [ ] Update Section 9 (Scenario Module) with closed-loop executor
   - [ ] Update runtime section to deprecate BlockingRuntime
   - [ ] Add executor configuration examples

2. **m0_progress_summary.md**:
   - [ ] Update after closed-loop executor implementation
   - [ ] Mark BlockingRuntime as deprecated
   - [ ] Add closed-loop to completed features

3. **Blog Post / Announcement**:
   - [ ] Write "Why RSBench Uses Async Tasks, Not OS Threads"
   - [ ] Explain the executor pattern decision
   - [ ] Show performance/memory comparisons

### Code (Implementation)

1. **Closed-Loop Executor**:
   - [ ] Implement `execute_closed_loop()` in scenario.rs
   - [ ] Add `ClosedLoop` to `ExecutorConfig`
   - [ ] Add unit tests
   - [ ] Add integration tests
   - [ ] Add benchmarks

2. **BlockingRuntime Deprecation**:
   - [ ] Add `#[deprecated]` attribute
   - [ ] Add deprecation warnings in logs
   - [ ] Update examples away from blocking mode

3. **Sysbench CLI Wrapper** (Future M1):
   - [ ] Design CLI translation layer
   - [ ] Implement argument mapping
   - [ ] Add tests for all sysbench commands

---

## Documentation Quality Checklist

- [x] Clear diagrams and tables
- [x] Code examples with syntax highlighting
- [x] Before/after comparisons
- [x] Cross-references between documents
- [x] Consistent terminology (Worker, Executor, Runtime)
- [x] User-facing explanations (README)
- [x] Technical details (scenario-design.md)
- [x] Migration guides (Appendix C)
- [x] Decision rationale (executor-pattern-decision.md)

---

## Word Count

| Document | Before (Initial) | After First Pass | After Second Pass | Total Added |
|----------|------------------|------------------|-------------------|-------------|
| scenario-design.md | ~950 lines | ~2540 lines | ~3330 lines | **+2380 lines** |
| README.md | ~300 lines | ~300 lines | ~300 lines | Restructured |
| project_structure.md | ~200 lines | ~230 lines | ~230 lines | +30 lines |
| executor-pattern-decision.md | 0 lines | ~450 lines | ~450 lines | +450 lines (NEW) |
| CLAUDE.md | ~878 lines | ~1030 lines | ~1100 lines | **+222 lines** |
| DOCUMENTATION_UPDATES.md | 0 lines | ~400 lines | ~550 lines | +550 lines (NEW, this file) |
| **Total** | ~2328 lines | ~4950 lines | ~5960 lines | **+3632 lines** |

---

## Conclusion

All documentation has been updated to reflect:

1. ✅ Worker architecture (async tasks, not OS threads)
2. ✅ Executor pattern decision (scenario layer, not runtime)
3. ✅ Sysbench compatibility mapping (explicit translation guide)
4. ✅ BlockingRuntime deprecation path
5. ✅ Closed-loop executor design (M0 implementation ready)
6. ✅ Backpressure awareness (detection, metrics, interpretation)
7. ✅ **Event Module as parallel module** (standalone section, ~540 lines)
8. ✅ **M:N distributed architecture with loose coordination philosophy** (multi-region testing, ~245 lines on coordination)

**Documentation is now comprehensive, consistent, and ready for implementation!**

**Total Documentation Additions**: ~3,632 lines across 6 files

**Major Additions in Second Pass**:
- Event Module Integration (Section 5.6): ~540 lines - **standalone comprehensive section**
- Loose Coordination Design (Section 11.8 subsection): ~245 lines - **motivation, rationale, implementation**

---

**Last Updated**: 2024-12-26
**Reviewed By**: N/A (awaiting review)
**Status**: ✅ Complete
