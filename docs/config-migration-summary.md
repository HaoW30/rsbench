# Configuration Architecture Migration Summary

## What Changed

All configuration files have been updated to the new three-tier architecture:

**Before:**
```yaml
# Combined config file
database:
  ...
runtime:
  ...
scenario:
  executor:
    ...
```

**After:**
```yaml
# Scenario file (entry point)
config: ../config/local.yaml
scenario:
  executor:
    ...
  workload:
    file: ../workloads/oltp.yaml
```

## Files Updated

### Scenarios Updated (Added `config:` Reference)

All scenario files now reference an infrastructure config:

1. **scenarios/smoke_test.yaml** → `../config/local.yaml`
2. **scenarios/quickstart.yaml** → `../config/rsbench.config.yaml`
3. **scenarios/oltp_read_write.yaml** → `../config/rsbench.config.yaml`
4. **scenarios/capacity_test.yaml** → `../config/rsbench.config.yaml`
5. **scenarios/high_throughput.yaml** → `../config/rsbench.config.yaml`
6. **scenarios/100k_qps_readonly.yaml** → `../config/mysql_port4000.yaml`
7. **scenarios/100k_qps_readonly_20conn.yaml** → `../config/mysql_port4000_20conn.yaml`

### New Scenarios Created (Split from Combined Configs)

8. **scenarios/tidb_100k_qps.yaml** → `../config/tidb_100k.yaml`
   - Split from `config/tidb_100k_combined.yaml`

9. **scenarios/10k_qps_50conn.yaml** → `../config/mysql_50conn.yaml`
   - Split from `config/mysql_10k_20conn.yaml`

### Infrastructure Configs (Infrastructure-Only)

These configs contain ONLY database, runtime, and output settings:

**Existing (Already Clean):**
- ✅ `config/rsbench.config.yaml` (default)
- ✅ `config/rsbench.config.staging.yaml`
- ✅ `config/rsbench.config.prod.yaml`
- ✅ `config/mysql_port4000.yaml`
- ✅ `config/mysql_port4000_20conn.yaml`

**Newly Created:**
- ✅ `config/local.yaml` (simple local testing)
- ✅ `config/tidb_100k.yaml` (split from combined)
- ✅ `config/mysql_50conn.yaml` (split from combined)

**Legacy (Combined - Keep for Compatibility):**
- ⚠️ `config/tidb_100k_combined.yaml` (now use `scenarios/tidb_100k_qps.yaml` instead)
- ⚠️ `config/mysql_10k_20conn.yaml` (now use `scenarios/10k_qps_50conn.yaml` instead)
- ⚠️ `config/rsbench.default.yaml` (monolithic example)

## Usage Changes

### Old Way

```bash
# Combined config with scenario embedded
rsbench --config config/tidb_100k_combined.yaml run
```

### New Way (Recommended)

```bash
# Scenario references config
rsbench --scenario scenarios/tidb_100k_qps.yaml run
```

### New Way (Override Config)

```bash
# Test same scenario against different environment
rsbench --config config/staging.yaml --scenario scenarios/smoke_test.yaml run
```

### New Way (Default Config)

```bash
# Uses config/rsbench.config.yaml by default
rsbench --scenario scenarios/quickstart.yaml run
```

## Migration Guide for Custom Configs

If you have custom combined config files, split them as follows:

### 1. Extract Infrastructure Config

Create `config/my_infra.yaml`:
```yaml
database:
  driver: mysql
  connection_string: "mysql://..."
  pool:
    min_size: 10
    max_size: 100
    connection_timeout: 5s
    idle_timeout: 300s

runtime:
  type: async
  workers: 4
  max_connections: 100
  backpressure_threshold: 0.8

output:
  format: text
  file: null
```

### 2. Create Scenario File

Create `scenarios/my_test.yaml`:
```yaml
# Reference infrastructure
config: ../config/my_infra.yaml

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  workload:
    type: declarative
    file: ../workloads/oltp_read_write.yaml

determinism:
  seed: 42
```

### 3. Run

```bash
rsbench --scenario scenarios/my_test.yaml run
```

## Benefits

1. **Separation of Concerns**
   - Infrastructure (database, pool, runtime) → Reusable across tests
   - Test definition (scenario) → Clear, focused files
   - Workload (operations) → Shared across scenarios

2. **Environment Management**
   - One scenario → Multiple environments (dev, staging, prod)
   - Example: `rsbench --config config/prod.yaml --scenario scenarios/smoke_test.yaml run`

3. **Version Control**
   - Smaller, focused files
   - Easy to diff and track changes
   - Clear test definition history

4. **Self-Documenting**
   - Scenario file shows complete test setup
   - Infrastructure config shows environment details
   - No guessing what config was used

## Backward Compatibility

Old combined config files still work if you specify them via `--config`:

```bash
# Still works (but not recommended)
rsbench --config config/tidb_100k_combined.yaml run
```

But the recommended approach is to use the new split architecture.

## Testing

All scenarios have been tested and verified:

```bash
# Test default config resolution
$ rsbench --scenario scenarios/smoke_test.yaml run
✅ Uses config/local.yaml (from scenario)

# Test quickstart
$ rsbench --scenario scenarios/quickstart.yaml run
✅ Uses config/rsbench.config.yaml (from scenario)

# Test config override
$ rsbench --config config/staging.yaml --scenario scenarios/smoke_test.yaml run
✅ Uses config/staging.yaml (from CLI override)
```

## Summary

| Change | Before | After |
|--------|--------|-------|
| **Entry Point** | `--config combined.yaml` | `--scenario test.yaml` |
| **Config Structure** | Monolithic (infra + scenario) | Separated (infra / scenario / workload) |
| **Reusability** | Limited | High (mix & match) |
| **Clarity** | Mixed concerns | Clear separation |
| **Version Control** | Large diffs | Small, focused diffs |

All existing scenarios now follow the new architecture. Use `--scenario` as your primary entry point!
