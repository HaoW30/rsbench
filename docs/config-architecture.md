# Configuration Architecture

## Overview

RSBench uses a three-tier configuration system:

```
Scenario YAML (test file)
  ├─> references Config YAML (infrastructure)
  └─> references Workload YAML (operations)
```

**Scenario = Test Definition** (what test to run)
**Config = Infrastructure** (how to connect and run)
**Workload = Operations** (what SQL operations to execute)

## Design Philosophy

### Separation of Concerns

1. **Infrastructure Config** (`config/*.yaml`)
   - Database connection (host, port, credentials)
   - Connection pool settings (min/max connections, timeouts)
   - Runtime settings (workers, backpressure)
   - Output preferences

2. **Workload** (`workloads/*.yaml`)
   - Schema definition (tables, columns, indexes)
   - Operations (SQL queries, parameters)
   - Data distributions (uniform, zipfian, etc.)

3. **Scenario** (`scenarios/*.yaml`)
   - References config + workload
   - Executor type (constant-rate, ramping-rate)
   - Load parameters (rate, duration)
   - Test-specific settings (seed, output)

### Benefits

- ✅ **Reusability**: Same workload → different rates/configs
- ✅ **Environment Isolation**: dev/staging/prod configs separate
- ✅ **Version Control**: Config diffs show what changed
- ✅ **Clarity**: One scenario file = complete test definition

## Usage Patterns

### Pattern 1: Scenario References Config (Recommended)

**Scenario file** (`scenarios/my_test.yaml`):
```yaml
# Reference infrastructure config
config: ../config/local.yaml

scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  workload:
    type: declarative
    file: ../workloads/oltp_read_write.yaml
```

**Run:**
```bash
rsbench --scenario scenarios/my_test.yaml run
```

✅ **Self-contained**: Scenario file specifies everything
✅ **Portable**: Easy to share and version control
✅ **Clear**: Config path is explicit in the scenario

---

### Pattern 2: CLI Override Config

If scenario doesn't reference a config, or you want to override:

```bash
rsbench --config config/staging.yaml --scenario scenarios/my_test.yaml run
```

✅ **Flexible**: Test same scenario against different environments
✅ **Quick**: Override for one-off tests

---

### Pattern 3: Default Config

If scenario doesn't reference a config and none is specified via CLI:

```bash
rsbench --scenario scenarios/my_test.yaml run
```

Uses `config/rsbench.config.yaml` by default.

✅ **Convenient**: No flags needed for local dev

---

## Config Resolution Priority

When loading configuration, RSBench uses this priority:

1. **Scenario's `config` field** (if specified in scenario YAML)
2. **CLI `--config` flag** (if specified on command line)
3. **Default config** (`config/rsbench.config.yaml`)

Example:
```yaml
# scenarios/my_test.yaml
config: ../config/staging.yaml  # Priority 1

scenario:
  ...
```

```bash
# Priority 2 (overrides scenario's config)
rsbench --config config/prod.yaml --scenario scenarios/my_test.yaml run

# Priority 1 (uses scenario's config)
rsbench --scenario scenarios/my_test.yaml run

# Priority 3 (uses default)
rsbench --scenario scenarios/basic.yaml run  # (basic.yaml has no config field)
```

## File Organization

```
your-project/
├── config/                      # Infrastructure configs
│   ├── rsbench.config.yaml     # Default (local dev)
│   ├── staging.yaml            # Staging environment
│   ├── prod.yaml               # Production environment
│   └── local.yaml              # Quick local testing
│
├── workloads/                   # Workload definitions
│   ├── oltp_read_write.yaml    # Balanced OLTP
│   ├── oltp_read_only.yaml     # Read-heavy
│   └── custom_app.yaml         # Application-specific
│
└── scenarios/                   # Test scenarios
    ├── smoke_test.yaml         # Quick validation
    ├── capacity_test.yaml      # Load testing
    └── regression_test.yaml    # Regression suite
```

## Example: Complete Test Setup

### 1. Create Infrastructure Config

`config/local.yaml`:
```yaml
database:
  driver: mysql
  connection_string: "mysql://root@localhost:3306/test"
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
```

### 2. Create or Use Existing Workload

`workloads/oltp_read_write.yaml`:
```yaml
workload:
  name: oltp_read_write
  schema:
    tables:
      - name: sbtest
        count: 10
        row_count: 10000
        ...

  operations:
    - name: point_select
      weight: 60
      sql: "SELECT c FROM sbtest{table_id} WHERE id = ?"
      ...
```

### 3. Create Scenario

`scenarios/my_load_test.yaml`:
```yaml
# Reference infrastructure
config: ../config/local.yaml

scenario:
  executor:
    type: constant-rate
    rate: 5000
    duration: 120s

  # Reference workload
  workload:
    type: declarative
    file: ../workloads/oltp_read_write.yaml

determinism:
  seed: 42

output:
  format: json
```

### 4. Run Test

```bash
rsbench --scenario scenarios/my_load_test.yaml run
```

## Migration from Old Pattern

### Old (Combined Config)

```yaml
# old_combined.yaml - Everything in one file
database:
  ...
runtime:
  ...
scenario:
  executor:
    ...
  workload:
    ...
```

```bash
rsbench --config old_combined.yaml run
```

❌ **Problems:**
- Mixed concerns (infra + test definition)
- Can't reuse infrastructure across tests
- Large, monolithic files

### New (Separated Config)

Split into three files as shown above, then:

```bash
rsbench --scenario scenarios/my_test.yaml run
```

✅ **Benefits:**
- Clear separation
- Reusable components
- Smaller, focused files

## Advanced: Relative Path Resolution

Paths in scenario files are resolved relative to the **scenario file's directory**:

```yaml
# scenarios/tests/my_test.yaml
config: ../../config/local.yaml       # Goes up to project root
workload:
  file: ../../workloads/custom.yaml   # Goes up to project root
```

Absolute paths are used as-is:
```yaml
config: /absolute/path/to/config.yaml
```

## CLI Overrides

Even with scenario-referenced config, you can override settings via CLI:

```bash
rsbench --scenario scenarios/test.yaml \
  --db-url "mysql://other-host/db" \
  --rate 10000 \
  --duration 30s \
  --output json \
  run
```

Priority: **CLI args > Scenario > Config > Defaults**

## Best Practices

1. **Version control all three** (config, workload, scenario)
2. **Use relative paths** in scenarios for portability
3. **One scenario per test case** (smoke, load, capacity, etc.)
4. **Reuse configs** across multiple scenarios (staging.yaml for all staging tests)
5. **Document** config purpose in comments (staging, prod, local, etc.)
6. **Gitignore credentials** - use env vars or separate credential files

## Summary

**Before:**
```bash
rsbench --config monolithic.yaml run  # Mixed concerns
```

**After:**
```bash
rsbench --scenario scenarios/my_test.yaml run  # Clear separation
```

The scenario is now the **primary entry point** - it references config and workload, creating a complete, self-contained test definition.
