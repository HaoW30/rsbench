# RSBench

> A modern, time-driven database testing tool - the next generation of sysbench

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

## Overview

RSBench is a next-generation database benchmarking and testing tool built in Rust. 

### Why RSBench?

Traditional database testing tools like sysbench have served the community well, but fall short in the cloud-native era. As databases evolved to become distributed, cloud-native systems with multi-region capabilities and new distributed SQL architectures, testing tools remained anchored in single-node assumptions.

RSBench bridges the following sysbench gaps with **distributed-aware** testing for modern databases:

- **Thread-based load generation** conflates concurrency with load
- **Blocking I/O** hides client-side bottlenecks
- **Coordinated omission** in latency measurements
- **Implicit phases** instead of explicit, deterministic behavior
- **No distributed database support** - lacks multi-region testing and distributed SQL awareness

RSBench is purpose-built for distributed SQL databases and cloud-native workloads while maintaining backward compatibility with sysbench workflows.

## Core Principles

1. **Time Is the Primary Control Plane** - Load is defined by time and rate, not by threads
2. **Backpressure Is a Signal, Not a Failure** - Client-side pressure is observable
3. **The Client Must Never Lie** - Client limits are explicitly tracked
4. **Errors Are Metrics, Not Failures** - Database errors are measured and reported, never hidden (see [Error Handling Philosophy](docs/error-handling-philosophy.md))
5. **Database Workloads Are Stateful** - Model sessions, transactions, prepared statements
6. **Test-as-Code Is the Default** - Workloads are version-controlled artifacts
7. **Explicit Phases Instead of Implicit Behavior** - Phase boundaries are clear and deterministic

## Key Features

### For Database Users

- **Accurate Benchmarking**: Rate-based execution prevents coordinated omission
- **Multi-Region Testing**: Test distributed databases with region-aware routing
- **Lifecycle Testing**: Integrate with K8s events, test during upgrades and failovers
- **Reproducible Tests**: Deterministic workload generation with seeded RNG
- **Rich Metrics**: HDR histograms for accurate percentile measurements

### For Database Teams

- **No Client Bottlenecks**: Async runtime supports 10k+ concurrent connections
- **Backpressure Visibility**: Distinguish client limits from database performance
- **Test-as-Code**: YAML/TOML workload definitions with Git versioning
- **CI/CD Integration**: Thresholds and checks for automated regression detection
- **Flexible Workloads**: Declarative YAML or programmable Lua scripts

## Design Philosophy: Errors as Observability Data

**RSBench treats database errors as metrics to measure, not failures to halt on.**

This is a fundamental design difference from sysbench:

```
Sysbench: Stop on error by default → Add --ignore-errors to continue
RSBench:  Count all errors by default → Add error_threshold to stop (optional)
```

### Why This Matters

**Capacity Testing**: Errors reveal system limits
```
Rate: 5K QPS   → Errors: 0%    ✅ Within capacity
Rate: 10K QPS  → Errors: 0.5%  ⚠️  Approaching limit
Rate: 20K QPS  → Errors: 8%    ❌ Over capacity
```
If we stopped at the first error, we'd never find the capacity curve.

**Failure Testing**: Modern systems test under failure, not just success
```
Time 0-60s:   Errors: 0%    (Normal operation)
Time 60-90s:  Errors: 85%   (Database failover)  ← Measure this!
Time 90-120s: Errors: 8%    (Recovery)           ← And this!
```
Stopping hides the failover duration and recovery behavior.

**Real-World Behavior**: Production systems measure errors, they don't halt
- Deadlocks at high concurrency? Measure the rate to set limits
- Duplicate keys in race conditions? Expected - track frequency
- Connection timeouts during upgrades? See impact and recovery time

### What You Get

RSBench provides **enhanced error visibility** instead of stopping:

```
Operation: point_select
  Count: 10000
  Errors: 150 (1.5%)           ← Error rate
  Success Rate: 98.5%          ← Success rate

⚠️  Warning: Error rate (1.5%) exceeds recommended threshold (1.0%)
```

**Optional error thresholds** (M1) for regression tests where errors are truly unexpected:
```yaml
error_handling:
  threshold: 5.0    # Stop if > 5% error rate
  action: stop      # or "warn"
```

See [Error Handling Philosophy](docs/error-handling-philosophy.md) for complete rationale and use cases.

## Getting Started

RSBench is currently in early development. Build from source to try it out:

```bash
git clone https://github.com/HaoW30/rsbench
cd rsbench
cargo build --release
```

### Quick Start (5 Minutes)

**Understanding RSBench:** A benchmark = **config** (where) + **scenario** (how/when) + **workload** (what)

- **config/** - Infrastructure setup (database connection, pool settings)
- **scenarios/** - Test execution (rate, duration, which workload to run)
- **workloads/** - What operations to perform (SQL queries, parameters, distributions)

**Step 1: Setup your database**

```bash
# Example with MySQL
mysql -u root -e "CREATE DATABASE sbtest;"
```

**Step 2: Configure database connection**

```bash
# Edit config to point to your database
vim config/rsbench.config.yaml

# Or use default (mysql://localhost/sbtest)
```

**Step 3: Prepare the database (create tables and load data)**

RSBench requires a separate **prepare step** to set up the database before running benchmarks:

```bash
# Prepare: Create tables and load initial test data
./target/release/rsbench prepare workloads/oltp_read_write.yaml

# What this does:
# 1. Reads the workload schema definition (tables, columns, indexes)
# 2. Creates tables (e.g., sbtest1, sbtest2, ..., sbtest10)
# 3. Creates indexes as specified in the workload
# 4. Loads initial test data (default: 10,000 rows per table)
#
# This is a ONE-TIME setup step. You only need to run it once
# before your first benchmark, or when you want fresh data.
```

**What happens during prepare:**

```sql
-- Example: For oltp_read_write workload, this creates:
CREATE TABLE IF NOT EXISTS sbtest1 (
  id INT AUTO_INCREMENT PRIMARY KEY,
  k INT,
  c CHAR(120),
  INDEX k_idx (k)
);

-- Loads 10,000 rows per table
INSERT INTO sbtest1 (k, c) VALUES (...), (...), ...;  -- Batched inserts
-- ... repeats for sbtest2, sbtest3, ..., sbtest10
```

**Note:** Prepare is intentionally a separate command (not automatic during `run`). This allows you to:
- Prepare once, run many benchmarks without recreating data
- Use different scenarios against the same dataset
- Inspect/modify tables manually between prepare and run if needed

**Step 4: Run your first benchmark**

```bash
# Now run the benchmark against the prepared tables
./target/release/rsbench \
  --config config/rsbench.config.yaml \
  --scenario scenarios/quickstart.yaml

# What just happened:
# - Used config: mysql://localhost/sbtest (from config file)
# - Used scenario: 100 ops/sec for 10 seconds (from quickstart.yaml)
# - Used workload: oltp_read_write (referenced in scenario)
# - Ran queries against the tables created in Step 3
```

**Step 5: Try other scenarios**

```bash
# All scenarios run against the same prepared tables
# No need to run prepare again!

# Smoke test (very quick validation)
./target/release/rsbench --scenario scenarios/smoke_test.yaml

# OLTP read/write test (standard benchmark)
./target/release/rsbench --scenario scenarios/oltp_read_write.yaml

# High throughput test (find max rate)
./target/release/rsbench --scenario scenarios/high_throughput.yaml

# Capacity test (ramping load to find limits)
./target/release/rsbench --scenario scenarios/capacity_test.yaml
```

**Step 6: Test different environments**

```bash
# Development (default config)
rsbench --scenario scenarios/quickstart.yaml

# Staging environment
rsbench --config config/rsbench.config.staging.yaml \
        --scenario scenarios/oltp_read_write.yaml

# Production (read replica, conservative rates)
rsbench --config config/rsbench.config.prod.yaml \
        --scenario scenarios/smoke_test.yaml
```

### Next Steps

- **Customize workloads**: See `workloads/README.md` for creating custom operations
- **Create scenarios**: See `scenarios/README.md` for different execution patterns
- **Configure infrastructure**: See `config/README.md` for connection settings
- **Advanced features**: See `docs/` for distributed testing, event integration, etc.

## Database Preparation

### Understanding the Prepare Step

RSBench uses a **two-phase approach** for database testing:

1. **Prepare Phase** (one-time): Create schema and load test data
2. **Run Phase** (repeatable): Execute benchmark workload

This separation provides several benefits:
- **Efficiency**: Prepare once, run many benchmarks
- **Reproducibility**: All tests use the same baseline data
- **Flexibility**: Manually inspect or modify data between phases
- **Control**: Choose when to reset data vs. accumulate changes

### Running Prepare

```bash
# Basic usage
rsbench prepare <workload_file>

# Example
rsbench prepare workloads/oltp_read_write.yaml
```

### What Prepare Does

The prepare command performs these operations in order:

**1. Load Workload Definition**
```bash
# Reads workload YAML file
# Parses schema definitions (tables, columns, indexes, row counts)
```

**2. Connect to Database**
```bash
# Uses database connection from config/rsbench.config.yaml
# Creates a single connection for DDL/DML operations
```

**3. Create Tables**
```sql
-- For each table in the workload schema:
CREATE TABLE IF NOT EXISTS sbtest1 (
  id INT AUTO_INCREMENT PRIMARY KEY,
  k INT,
  c CHAR(120),
  INDEX k_idx (k)
);

-- Creates all indexes as specified
CREATE INDEX k_idx ON sbtest1 (k);
```

**4. Load Test Data**
```bash
# Inserts initial rows based on row_count configuration
# Uses batched inserts (1000 rows per batch) for performance
# Default: 10,000 rows per table (configurable in workload YAML)
```

### Prepare Output Example

```
$ rsbench prepare workloads/oltp_read_write.yaml

Preparing workload from: workloads/oltp_read_write.yaml
Connecting to database: mysql://localhost:3306/sbtest
Loading workload definition...
Creating tables and loading data...
Loading 10000 rows into table sbtest1 using uniform strategy...
Loading 10000 rows into table sbtest2 using uniform strategy...
Loading 10000 rows into table sbtest3 using uniform strategy...
...
Loading 10000 rows into table sbtest10 using uniform strategy...
✓ Workload preparation completed successfully!
```

### Customizing Data Volume

You can control the amount of test data by modifying the workload file:

```yaml
# workloads/my_custom_workload.yaml
workload:
  name: my_test
  schema:
    tables:
      - name: sbtest
        count: 20              # Create 20 tables instead of 10
        row_count: 100000      # Load 100k rows instead of 10k
        columns:
          - name: id
            type: INT
            primary_key: true
          - name: k
            type: INT
            index: k_idx
          - name: c
            type: CHAR(120)
```

Then prepare with your custom settings:
```bash
rsbench prepare workloads/my_custom_workload.yaml
```

### When to Re-run Prepare

You need to run prepare again when:
- **First-time setup**: Initial database setup
- **Schema changes**: Modified workload table/column definitions
- **Data reset**: Want to start with fresh baseline data
- **Different workload**: Switching to a workload with different schema

You do NOT need to re-run prepare when:
- Running different scenarios against the same workload
- Testing different rates/durations
- Running on different database environments (each environment prepares independently)

### Typical Workflow

```bash
# 1. PREPARE ONCE: Set up database schema and data
rsbench prepare workloads/oltp_read_write.yaml

# 2. RUN MANY TIMES: Execute different benchmark scenarios
rsbench run scenarios/smoke_test.yaml        # Quick validation
rsbench run scenarios/oltp_read_write.yaml   # Standard test
rsbench run scenarios/high_throughput.yaml   # Find limits

# 3. OPTIONAL: Inspect data manually
mysql -u root sbtest -e "SELECT COUNT(*) FROM sbtest1;"

# 4. RESET: Re-run prepare when you want fresh data
rsbench prepare workloads/oltp_read_write.yaml  # Recreates tables
```

### PrepareContext Details

For workload developers, the `prepare()` method receives a `PrepareContext`:

```rust
pub struct PrepareContext<'a> {
    /// Database connection for DDL/DML operations
    pub connection: &'a mut dyn Connection,

    /// Determinism seed (for reproducible data generation)
    pub seed: u64,

    /// Number of workers (for data partitioning in distributed mode)
    pub worker_count: usize,
}
```

This context provides:
- **connection**: Execute CREATE TABLE, INSERT, CREATE INDEX statements
- **seed**: Generate deterministic test data (same seed = same data)
- **worker_count**: Partition data across workers in distributed scenarios (M1+)

### Cleanup (Manual)

Currently (M0), cleanup is manual:

```sql
-- To remove tables after testing
DROP TABLE IF EXISTS sbtest1, sbtest2, sbtest3, ..., sbtest10;

-- Or drop the entire database
DROP DATABASE sbtest;
CREATE DATABASE sbtest;
```

**Note:** Automatic cleanup via `rsbench cleanup` command is planned for M1.

### Customizing Workloads

RSBench uses **declarative YAML workloads** that are fully transparent and configurable:

**Use pre-defined workloads:**
```yaml
# scenarios/my_test.yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml  # Use built-in workload
```

**Customize operation distribution:**
```yaml
# Override to 90% reads, 10% writes
scenario:
  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml
    overrides:
      operations:
        - name: point_select
          weight: 90
        - name: update_non_index
          weight: 10
```

**Create custom workloads:**
```yaml
# workloads/my_app.yaml
workload:
  name: my_application

  schema:
    tables:
      - name: users
        count: 1
        row_count: 100000
        columns:
          - name: user_id
            type: INT
            primary_key: true
          - name: email
            type: VARCHAR(255)

  operations:
    - name: get_user
      weight: 70
      type: read
      sql: "SELECT * FROM users WHERE user_id = ?"
      parameters:
        - name: user_id
          distribution:
            type: uniform
            range: [1, 100000]
```

See `workloads/README.md` for the complete guide to creating custom workloads.

<!-- Detailed API documentation coming soon -->

## Architecture

RSBench is built around several key components:

- **Scenario Orchestrator**: Time-driven execution with pluggable executor patterns (open-loop, closed-loop)
- **Async Runtime**: Efficient tokio-based execution with backpressure monitoring
- **Connection Pool**: Backpressure-aware with multi-endpoint routing
- **Declarative Workload Engine**: Transparent YAML workloads with full sysbench compatibility
- **Lua Workload Engine**: Programmable workloads for complex scenarios
- **Metrics Engine**: HDR histograms with multiple output formats
- **Event Integration**: K8s events, webhooks, lifecycle testing (M1+)

### Key Architectural Decisions

**Executor vs Runtime Separation:**
- **Executor** = HOW/WHEN to submit operations (open-loop: time-driven, closed-loop: worker-driven)
- **Runtime** = HOW to execute operations (always async with tokio)
- **Workload** = WHAT operations to run (SQL generation, data distribution)

This three-layer separation provides maximum flexibility:
- Same workload can run with different execution patterns (open/closed loop)
- Same executor can work with different workloads
- Single async runtime handles all execution modes efficiently

**Worker Model - Async Tasks, Not OS Threads:**
- **Workers** are lightweight Tokio async tasks (~2KB stack each)
- 1000 workers can run on 8 OS threads via async I/O multiplexing
- Sysbench `--threads=100` ≈ RSBench `workers: 100` (semantically equivalent, 40x less memory)
- Scalable: Simulate 10K+ concurrent users without 10K OS threads

**Executor Patterns:**
1. **Open-Loop (Rate-Based)**: Time-driven fire-and-forget submission
   - Constant-rate: Fixed ops/sec (e.g., `rate: 1000`)
   - Ramping-rate: Staged rate changes (capacity testing)
   - Use case: Load generation, throughput testing

2. **Closed-Loop (Sysbench-Compatible)**: Worker-driven sequential execution
   - Fixed worker concurrency (e.g., `workers: 16` = sysbench `--threads=16`)
   - Natural backpressure (slow queries → lower throughput)
   - Use case: User concurrency simulation, sysbench replacement

**Declarative-First Design:**
- All sysbench OLTP tests are transparent YAML files in `workloads/`
- Full control over schema, operations, data distributions, and parameters
- No code changes needed to customize workloads
- Lua scripts available for complex scenarios that need programmability

See the [Scenario Design](docs/scenario-design.md), [Workload Design](docs/workload-design.md), and [API Specification](docs/api_spec_m0.md) for details.

## Roadmap

### Milestone 0: Foundation (Current)
Drop-in sysbench replacement with time-driven, rate-based execution

**What you'll get:**
- Rate-based execution that maintains target QPS regardless of query latency
- Backpressure visibility - client saturation reported in metrics
- No coordinated omission - accurate latency measurement under load
- 10x higher throughput capacity than sysbench in async mode
- Deterministic testing - same seed produces same operations
- MySQL support with async and blocking runtimes
- HDR histogram metrics with text and JSON output

### Milestone 1: Distributed Testing (Next)
Multi-region testing with essential workload flexibility

**What you'll get:**
- Multi-region distributed mode with loose coordination
- Read/write split routing and region-aware execution
- K8s event integration - events trigger phase changes
- Failover event correlation with latency spikes
- PostgreSQL driver support
- Declarative YAML workloads covering 80% of use cases
- Fast, deterministic data generation engine

### Milestone 2: Production Features
CI/CD integration, advanced workload scenarios, and transaction support

### Milestone 3+: Enterprise Ready
Monitoring integrations, web interface, and industry-standard benchmark suites

## Comparison with Sysbench

| Feature | Sysbench | RSBench |
|---------|----------|---------|
| Load Generation | Thread-based only | Open-loop (rate-based) + Closed-loop (thread-based) |
| Thread Model | OS threads | Async tasks (green threads) |
| Memory (100 threads) | ~800 MB | ~20 MB |
| I/O Model | Blocking | Async (non-blocking) |
| Coordinated Omission | Yes (latency skew) | No (accurate) |
| Backpressure | Hidden | Visible (explicit metric) |
| **Error Handling** | **Stop by default, opt-in ignore** | **Count all errors, opt-in stop** |
| Determinism | Limited | Full (seeded RNG) |
| Multi-Region | No | Yes (M1+) |
| Test-as-Code | No | Yes (YAML/TOML) |
| Lifecycle Testing | No | Yes (K8s events, M1+) |
| Workload Definition | Hardcoded Lua | Declarative YAML + Lua |
| Workload Transparency | Black box | Fully visible/editable |
| OLTP Tests | Built-in binary | YAML files (user-modifiable) |
| Custom Workloads | Write Lua | Write YAML (or Lua) |
| Sysbench Compatibility | N/A | 100% (command mapping in docs) |
| `--threads=N` | N OS threads | `workers: N` (N async tasks) |
| `--rate=X` | Not supported | `rate: X` (open-loop executor) |
| `--ignore-errors` | Required for error tolerance | Not needed (errors are metrics) |

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Setup

```bash
# Clone the repository
git clone https://github.com/yourusername/rsbench
cd rsbench

# Build with all features
cargo build --all-features

# Run tests
cargo test

# Run benchmarks
cargo bench
```

## License

RSBench is licensed under the Apache License 2.0. See [LICENSE](LICENSE) for details.


## Community & Support

- **Issues**: [GitHub Issues](https://github.com/yourusername/rsbench/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/rsbench/discussions)

## Acknowledgments

RSBench draws inspiration from:

- **sysbench**: The industry-standard database testing tool
- **k6**: Modern load testing with scenarios and executors
- **Tokio**: Rust's async runtime ecosystem

## Citation

If you use RSBench in your research or benchmarking, please cite:

```bibtex
@software{rsbench,
  title = {RSBench: A Modern Database Testing Tool},
  author = {RSBench Contributors},
  year = {2024},
  url = {https://github.com/yourusername/rsbench}
}
```

---

**Status**: 🚧 Early Development - Not yet production ready

Built with ❤️ in Rust
