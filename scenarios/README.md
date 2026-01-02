# Test Scenarios

This directory contains test scenario definitions for RSBench.

## Quick Start

**Run a scenario** (recommended way):
```bash
rsbench --scenario scenarios/smoke_test.yaml run
```

The scenario file is the **entry point** - it references infrastructure config and workload.

## Purpose

Scenarios define **what test to run**:
- Which infrastructure config to use (database, pool, runtime)
- Which workload to execute (operations, SQL queries)
- Load pattern (rate, duration, ramping)
- Test-specific settings (seed, output)

## Architecture

```
Scenario File (Entry Point)
  ├─> references Config (infrastructure)
  └─> references Workload (operations)
```

Example scenario structure:
```yaml
# scenarios/my_test.yaml
config: ../config/local.yaml        # Infrastructure (database, pool, runtime)

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  workload:
    type: declarative
    file: ../workloads/oltp.yaml    # Operations (SQL, parameters)

determinism:
  seed: 42
```

## Usage Patterns

### Pattern 1: Scenario References Config (Recommended)

```bash
rsbench --scenario scenarios/smoke_test.yaml run
```

✅ **Self-contained**: Scenario specifies everything
✅ **Portable**: Easy to share and version control
✅ **Clear**: Config path is explicit

### Pattern 2: Override Config via CLI

Test same scenario against different environments:

```bash
# Development
rsbench --scenario scenarios/smoke_test.yaml run
# (uses config from scenario file)

# Staging
rsbench --config config/staging.yaml --scenario scenarios/smoke_test.yaml run

# Production
rsbench --config config/prod.yaml --scenario scenarios/smoke_test.yaml run
```

✅ **Flexible**: Same test, different environments

### Pattern 3: Default Config

If scenario doesn't specify a config:

```bash
rsbench --scenario scenarios/my_test.yaml run
```

Uses `config/rsbench.config.yaml` by default.

## Config Resolution Priority

1. **CLI `--config` flag** (highest priority)
2. **Scenario's `config` field**
3. **Default `config/rsbench.config.yaml`** (lowest priority)

## Available Scenarios

### `smoke_test.yaml`
Quick sanity check with low load and short duration. Use this to verify database connectivity.

**Use case**: CI/CD pre-deployment checks, quick validation

```bash
rsbench --scenario scenarios/smoke_test.yaml
```

### `oltp_read_write.yaml`
Balanced read/write workload (60% reads, 40% writes). Standard OLTP application pattern.

**Use case**: General performance testing, baseline benchmarks

```bash
rsbench --scenario scenarios/oltp_read_write.yaml
```

### `high_throughput.yaml`
High request rate (10K ops/sec) for sustained duration. Tests maximum sustainable throughput.

**Use case**: Performance limits, capacity planning

```bash
rsbench --scenario scenarios/high_throughput.yaml
```

### `capacity_test.yaml`
Ramping load from 500 to 15K ops/sec. Finds where performance degrades and backpressure begins.

**Use case**: Finding capacity limits, stress testing

```bash
rsbench --scenario scenarios/capacity_test.yaml
```

## Creating Custom Scenarios

1. Copy an existing scenario as a template
2. Modify the workload and executor settings
3. Save with a descriptive name

**Minimal scenario:**

```yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
  workload:
    type: builtin
    name: oltp_read_write

determinism:
  seed: 42
```

## Best Practices

- **One scenario per test case** - Keep scenarios focused
- **Descriptive names** - Use names that indicate what you're testing
- **Version control** - Commit scenarios to Git with your code
- **CI/CD integration** - Run smoke tests in pipelines
- **Document intent** - Add comments explaining why values were chosen
- **Separate environments** - Use different infrastructure configs, not different scenarios

## Environment-Specific Testing

```bash
# Development
rsbench --scenario scenarios/oltp_read_write.yaml

# Staging
rsbench --config config/rsbench.config.staging.yaml --scenario scenarios/oltp_read_write.yaml

# Production (read replica)
rsbench --config config/rsbench.config.prod.yaml --scenario scenarios/smoke_test.yaml
```

Same test scenario, different infrastructure!
