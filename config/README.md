# Infrastructure Configuration

This directory contains infrastructure configurations for RSBench.

## Purpose

Infrastructure configs define **how to connect and run**:
- Database connection settings
- Connection pool configuration
- Runtime settings (async/blocking, workers, etc.)
- Output preferences

They are separate from test scenarios (what to test), which are defined in `scenarios/`.

## Available Configs

### `rsbench.config.yaml` (Default)
Local development / default configuration.

**Connection**: `localhost:3306`
**Use case**: Local testing, development

### `rsbench.config.staging.yaml`
Staging environment configuration.

**Connection**: Staging database cluster
**Use case**: Pre-production testing, QA validation

### `rsbench.config.prod.yaml`
Production environment configuration (read replica).

**Connection**: Production read replica
**Use case**: Production validation, capacity verification
**⚠️ Caution**: Use conservative settings to avoid impacting production

## Usage

### Default Config

If no `--config` is specified, `config/rsbench.config.yaml` is used:

```bash
rsbench --scenario scenarios/oltp_read_write.yaml
```

### Explicit Config

```bash
rsbench --config config/rsbench.config.staging.yaml --scenario scenarios/oltp_read_write.yaml
```

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
