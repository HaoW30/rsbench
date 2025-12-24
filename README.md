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

<!-- Quick Start and detailed examples coming soon -->

## Architecture

RSBench is built around several key components:

- **Scenario Orchestrator**: Time-driven execution with rate control
- **Dual-Mode Runtime**: Async (primary) and blocking (sysbench compatibility)
- **Connection Pool**: Backpressure-aware with multi-endpoint routing
- **Workload Engine**: Declarative YAML or programmable Lua
- **Metrics Engine**: HDR histograms with multiple output formats
- **Event Integration**: K8s events, webhooks, lifecycle testing

See the [Architecture Documentation](docs/architecture.md) for details.

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
| Load Generation | Thread-based (closed model) | Rate-based (open model) |
| I/O Model | Blocking | Async + Blocking |
| Coordinated Omission | Yes (latency skew) | No (accurate) |
| Backpressure | Hidden | Visible |
| Determinism | Limited | Full (seeded RNG) |
| Multi-Region | No | Yes |
| Test-as-Code | No | Yes (YAML/TOML) |
| Lifecycle Testing | No | Yes (K8s events) |
| Sysbench Compatibility | N/A | Yes (Lua scripts) |

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
