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
4. **Database Workloads Are Stateful** - Model sessions, transactions, prepared statements
5. **Test-as-Code Is the Default** - Workloads are version-controlled artifacts
6. **Explicit Phases Instead of Implicit Behavior** - Phase boundaries are clear and deterministic

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

## Getting Started

RSBench is currently in early development. Build from source to try it out:

```bash
git clone https://github.com/HaoW30/rsbench
cd rsbench
cargo build --release
```

### Quick Start

RSBench separates infrastructure configuration from test scenarios:

**1. Configure your database connection once:**

```bash
# Edit the default config for your database
vim config/rsbench.config.yaml
```

**2. Run test scenarios:**

```bash
# Quick smoke test
./target/release/rsbench --scenario scenarios/smoke_test.yaml

# OLTP read/write test
./target/release/rsbench --scenario scenarios/oltp_read_write.yaml

# Capacity test (finds limits)
./target/release/rsbench --scenario scenarios/capacity_test.yaml
```

**3. Test against different environments:**

```bash
# Development (default)
rsbench --scenario scenarios/oltp_read_write.yaml

# Staging
rsbench --config config/rsbench.config.staging.yaml --scenario scenarios/oltp_read_write.yaml

# Production (read replica, conservative)
rsbench --config config/rsbench.config.prod.yaml --scenario scenarios/smoke_test.yaml
```

See `config/README.md` and `scenarios/README.md` for detailed documentation.

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
