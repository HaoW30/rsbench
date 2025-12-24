# Test Scenarios

This directory contains test scenario definitions for RSBench.

## Purpose

Test scenarios define **what to test**:
- Workload type and parameters
- Load pattern (rate, duration, ramping)
- Test-specific settings

They are separate from infrastructure configuration (database connection, runtime settings), which is defined in `config/`.

## Usage

### Run with Default Infrastructure Config

```bash
rsbench --scenario scenarios/oltp_read_write.yaml
```

This uses `config/rsbench.config.yaml` by default.

### Run with Specific Infrastructure Config

```bash
rsbench --config config/rsbench.config.staging.yaml --scenario scenarios/oltp_read_write.yaml
```

### Override Config in Scenario File

You can specify a config file directly in the scenario:

```yaml
# scenarios/my_test.yaml
config: config/rsbench.config.staging.yaml

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
  workload:
    type: builtin
    name: oltp_read_write
```

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
