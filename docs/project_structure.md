# RSBench Project Structure

## Overview

This document describes the module organization and file structure for RSBench Milestone 0.

## Directory Structure

```
rsbench/
├── Cargo.toml              # Project dependencies and configuration
├── README.md               # Project overview
├── LICENSE                 # Apache 2.0 license
├── docs/                   # Documentation
│   ├── api_spec_m0.md     # Detailed API specification for M0
│   └── project_structure.md # This file
├── examples/               # Example configurations
│   └── basic_config.yaml  # Basic M0 configuration example
└── src/                    # Source code
    ├── lib.rs              # Library root, core types and errors
    ├── main.rs             # Binary entry point
    ├── cli_impl.rs         # CLI implementation logic
    ├── cli.rs              # CLI argument definitions
    ├── config/             # Configuration module
    │   └── mod.rs          # Config loading, parsing, validation
    ├── workload/           # Workload module
    │   ├── mod.rs          # Workload trait and factory
    │   ├── oltp.rs         # Built-in OLTP workload
    │   └── lua.rs          # Lua script workload (optional)
    ├── rate_limiter.rs     # Token bucket rate limiter
    ├── runtime/            # Runtime execution engines
    │   ├── mod.rs          # Runtime trait and factory
    │   ├── async_runtime.rs # Async runtime (primary)
    │   └── blocking.rs     # Blocking runtime (sysbench compat)
    ├── pool/               # Connection pool
    │   └── mod.rs          # Pool management
    ├── driver/             # Database drivers
    │   ├── mod.rs          # Driver trait and registry
    │   └── mysql.rs        # MySQL driver implementation
    ├── metrics/            # Metrics collection
    │   ├── mod.rs          # Metrics collector with HDR histograms
    │   └── output.rs       # Output formats (text, JSON)
    └── scenario.rs         # Scenario executor (orchestration)
```

## Module Responsibilities

### Core (lib.rs)
- Defines main error types (`Error`, `RuntimeError`, `DatabaseError`)
- Defines common types (`Value`, `Result`)
- Re-exports public API

### Config Module
- Parses YAML/TOML configuration files
- Validates configuration constraints
- Applies defaults
- Supports CLI argument merging

### Workload Module
- **Trait**: `Workload` - abstraction for all workload types
- **Factory**: Creates workload instances from config
- **Implementations**:
  - `OltpReadWrite`: Built-in sysbench-compatible OLTP workload
  - `LuaWorkload`: Lua script execution (feature-gated)

### Rate Limiter
- Token bucket algorithm for rate control
- Time-driven operation submission
- Dynamic rate adjustment for ramping

### Runtime Module
- **Trait**: `RuntimeEngine` - abstraction for execution engines
- **Factory**: Creates runtime based on config
- **Implementations**:
  - `AsyncRuntime`: Primary mode with backpressure monitoring
  - `BlockingRuntime`: Sysbench compatibility mode

### Pool Module
- Manages database connections
- Provides backpressure signals
- M0: Simplified (creates new connections)
- Future: Full deadpool integration

### Driver Module
- **Trait**: `DatabaseDriver` - database abstraction
- **Trait**: `Connection` - connection operations
- **Registry**: Compile-time driver registration
- **MySQL Driver**: M0 implementation

### Metrics Module
- Lock-free metrics collection using `DashMap` and atomics
- HDR histograms for accurate latency percentiles
- Backpressure event tracking
- **Outputs**: Text (sysbench-compatible), JSON

### Scenario Module
- Orchestrates workload execution
- Implements executor types:
  - `ConstantRate`: Fixed ops/sec
  - `RampingRate`: Staged rate changes
- Time-driven scheduling via rate limiter

### CLI Module
- Command-line argument parsing (clap)
- Subcommands: run, prepare, cleanup
- Integration with configuration system

## Dependency Graph

```
CLI → Config → Scenario → Runtime → Pool → Driver
                    ↓          ↓
                Workload   Metrics
                    ↑
                RateLimiter
```

## Key Design Patterns

### Trait-Based Abstraction
All major components use traits for extensibility:
- `Workload`: Custom workload types
- `RuntimeEngine`: Different execution modes
- `DatabaseDriver`: Multiple database support
- `MetricsOutput`: Various output formats

### Factory Pattern
Factories create instances from configuration:
- `WorkloadFactory`
- `RuntimeFactory`
- `DriverRegistry`

### Builder Pattern
Configuration uses serde with defaults for ergonomic setup

### Arc-Based Sharing
Shared components use `Arc` for thread-safe access:
- `ConnectionPool`
- `MetricsCollector`
- `RuntimeEngine`

## Feature Flags

```toml
default = ["mysql"]
mysql = ["mysql_async"]
postgres = ["tokio-postgres"]
lua = ["mlua"]
all-drivers = ["mysql", "postgres"]
full = ["mysql", "postgres", "lua"]
```

### Building

```bash
# Default (MySQL only)
cargo build

# With Lua support (requires LuaJIT)
cargo build --features lua

# All drivers
cargo build --features all-drivers

# Everything
cargo build --features full
```

## M0 Status

✅ Complete module structure
✅ All interface definitions
✅ Basic implementations for each module
✅ Code compiles successfully
✅ Example configuration

🚧 TODO for full M0 functionality:
- Complete workload prepare() implementation
- Add data loading to OLTP workload
- Implement proper connection pooling
- Add comprehensive error handling
- Write unit tests
- Write integration tests
- Performance optimization

## Next Steps

1. Implement unit tests for each module
2. Add integration tests
3. Complete OLTP workload data loading
4. Add example Lua workloads
5. Create docker-compose for testing
6. Write user guide documentation
