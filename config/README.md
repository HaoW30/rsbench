# Infrastructure Configuration

This directory contains infrastructure configurations for RSBench.

## Purpose

Infrastructure configs define **how to connect and run**:
- Database connection settings (host, port, credentials)
- Connection pool configuration (min/max size, timeouts)
- Runtime settings (workers, max_connections, backpressure)
- Output preferences (format, file)

They are **separate** from test scenarios (what to test), which are defined in `scenarios/`.

## Architecture

```
Scenario → references → Config (this directory)
```

Example:
```yaml
# scenarios/my_test.yaml
config: ../config/local.yaml  # References infrastructure config

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
  workload:
    file: ../workloads/oltp.yaml
```

## Available Configs

### Default / Development Configs

- **`rsbench.config.yaml`** - Default config (localhost:3306)
  - Use case: Local MySQL testing, development
  - Connection: `localhost:3306`

- **`local.yaml`** - Simple local config (port 4000)
  - Use case: Quick local testing with TiDB/MySQL on port 4000
  - Connection: `127.0.0.1:4000`

### High-Throughput Configs

- **`mysql_port4000.yaml`** - High connection pool (800 max)
  - Use case: 100K QPS testing
  - Connection: `localhost:4000`, max_connections: 1000

- **`mysql_port4000_20conn.yaml`** - Limited connections (20 max)
  - Use case: Backpressure testing with minimal connections
  - Connection: `localhost:4000`, max_connections: 20

- **`mysql_50conn.yaml`** - 50 connections (pre-warmed)
  - Use case: Moderate to high load testing (10K QPS)
  - Connection: `127.0.0.1:4000`, max_connections: 50

- **`tidb_100k.yaml`** - Extreme throughput (200 connections)
  - Use case: TiDB 100K QPS testing
  - Connection: `localhost:4000`, max_connections: 200

### Environment Configs

- **`rsbench.config.staging.yaml`** - Staging environment
  - Use case: Pre-production testing, QA validation
  - Connection: `staging-db.example.com:3306`

- **`rsbench.config.prod.yaml`** - Production environment (read replica)
  - Use case: Production validation, capacity verification
  - Connection: `prod-replica.example.com:3306`
  - ⚠️ **Caution**: Use conservative settings

## Usage Patterns

### Pattern 1: Scenario References Config (Recommended)

Scenario file specifies which config to use:

```yaml
# scenarios/my_test.yaml
config: ../config/local.yaml
```

```bash
rsbench --scenario scenarios/my_test.yaml run
```

✅ **Self-contained**: Config is explicit in scenario
✅ **Version control**: Easy to track which config was used

### Pattern 2: Override Config via CLI

Override scenario's config for different environments:

```bash
# Development (uses scenario's config)
rsbench --scenario scenarios/smoke_test.yaml run

# Staging (override with staging config)
rsbench --config config/staging.yaml --scenario scenarios/smoke_test.yaml run

# Production (override with prod config)
rsbench --config config/prod.yaml --scenario scenarios/smoke_test.yaml run
```

✅ **Flexible**: Test same scenario across environments

### Pattern 3: Default Config

If scenario doesn't specify a config:

```bash
rsbench --scenario scenarios/my_test.yaml run
```

Uses `config/rsbench.config.yaml` by default.

### Config in Scenario File

Scenarios can reference a specific config:

```yaml
# scenarios/staging_test.yaml
config: config/rsbench.config.staging.yaml

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s
  # ...
```

## Configuration Structure

```yaml
database:
  driver: mysql
  connection_string: "mysql://user:pass@host:port/db"
  pool:
    min_size: 10
    max_size: 100
    connection_timeout: 10s
    idle_timeout: 300s

runtime:
  type: async
  workers: 4
  max_connections: 100
  backpressure_threshold: 0.8

output:
  format: json
  file: null
```

## Creating Environment-Specific Configs

1. Copy an existing config as a template
2. Update connection string and credentials
3. Adjust pool and runtime settings for the environment
4. Save as `rsbench.config.<environment>.yaml`

**Example for QA environment:**

```yaml
# config/rsbench.config.qa.yaml
database:
  driver: mysql
  connection_string: "mysql://qauser:qapass@qa-db.internal:3306/sbtest"
  pool:
    min_size: 10
    max_size: 100
    connection_timeout: 10s
    idle_timeout: 300s

runtime:
  type: async
  workers: 8
  max_connections: 100
  backpressure_threshold: 0.8

output:
  format: json
  file: results/qa_{timestamp}.json
```

## Best Practices

### Security

- **Never commit production credentials** - Use environment variables or secrets management
- **Use read replicas** - For production testing, always use read replicas
- **Least privilege** - Use database users with minimal required permissions

### Connection Pooling

- **min_size**: Keep connections warm, but not too high
- **max_size**: Should not exceed database `max_connections`
- **Staging/Prod**: Use higher limits than local
- **Formula**: `max_size <= (db_max_connections * 0.7)`

### Runtime Settings

- **workers**: Match CPU cores for async mode
- **max_connections**: Should align with pool max_size
- **backpressure_threshold**: 0.8 is a good default (80% utilization)

### Environment Strategy

```bash
# Local (default)
rsbench --scenario scenarios/smoke_test.yaml

# CI/CD (staging)
rsbench --config config/rsbench.config.staging.yaml --scenario scenarios/smoke_test.yaml

# Capacity planning (staging with high load)
rsbench --config config/rsbench.config.staging.yaml --scenario scenarios/capacity_test.yaml

# Production validation (conservative, read replica)
rsbench --config config/rsbench.config.prod.yaml --scenario scenarios/smoke_test.yaml
```

## Credentials Management

### Environment Variables (Recommended)

```yaml
# config/rsbench.config.staging.yaml
database:
  connection_string: "${DB_CONNECTION_STRING}"
```

```bash
export DB_CONNECTION_STRING="mysql://user:pass@host:port/db"
rsbench --config config/rsbench.config.staging.yaml --scenario scenarios/oltp_read_write.yaml
```

### Secrets Files (Not Committed)

```yaml
# config/rsbench.config.prod.yaml (checked in)
database:
  connection_string: "see config/secrets.prod.yaml"

# config/secrets.prod.yaml (gitignored)
connection_string: "mysql://produser:secret@prod:3306/db"
```

Add to `.gitignore`:
```
config/secrets.*.yaml
config/*.secret.yaml
```
