# Declarative Workload Definitions

This directory contains declarative YAML workload definitions that can be used with RSBench.

## Overview

Workloads are defined in YAML format and describe:
- **Schema**: Table structure, columns, indexes
- **Data Generation**: How to populate tables during prepare phase
- **Operations**: SQL queries/statements to execute during the test

## Available Workloads

### Sysbench-Compatible OLTP Workloads

These workloads are equivalent to sysbench's built-in tests:

| File | Description | Sysbench Equivalent |
|------|-------------|-------------------|
| `oltp_read_write.yaml` | Balanced 60% reads, 40% writes | `oltp_read_write.lua` |
| `oltp_read_only.yaml` | Various SELECT patterns | `oltp_read_only.lua` |
| `oltp_write_only.yaml` | Updates, deletes, inserts | `oltp_write_only.lua` |
| `oltp_point_select.yaml` | Point select only (100% reads) | `oltp_point_select.lua` |
| `oltp_insert.yaml` | Insert-only workload | `oltp_insert.lua` |
| `oltp_update_index.yaml` | Update indexed column | `oltp_update_index.lua` |
| `oltp_update_non_index.yaml` | Update non-indexed column | `oltp_update_non_index.lua` |
| `oltp_delete.yaml` | Delete workload | `oltp_delete.lua` |
| `select_random_points.yaml` | Random point selects | `select_random_points.lua` |
| `select_random_ranges.yaml` | Random range queries | `select_random_ranges.lua` |

## Usage

### Using in Scenario Files

```yaml
# scenarios/my_test.yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml
```

### Overriding Parameters

You can override specific workload parameters:

```yaml
scenario:
  workload:
    type: declarative
    file: workloads/oltp_read_write.yaml
    overrides:
      schema:
        tables:
          - row_count: 50000        # Override table size
      operations:
        - name: point_select
          weight: 80                # Override to 80% reads
        - name: update_non_index
          weight: 20                # 20% writes
```

### Command-Line Overrides

```bash
# Override table size
rsbench --scenario scenarios/oltp_test.yaml \
        --workload-override schema.tables.row_count=50000

# Override operation weights
rsbench --scenario scenarios/oltp_test.yaml \
        --workload-override operations.point_select.weight=80 \
        --workload-override operations.update_non_index.weight=20
```

## Creating Custom Workloads

You can create your own workload definitions:

```yaml
# workloads/my_custom_workload.yaml
workload:
  name: my_custom_workload
  description: "Custom workload for my application"

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
            index: email_idx
          - name: balance
            type: DECIMAL(10,2)

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

    - name: update_balance
      weight: 30
      type: write
      sql: "UPDATE users SET balance = balance + ? WHERE user_id = ?"
      parameters:
        - name: delta
          generator:
            type: decimal
            range: [-100.00, 100.00]
        - name: user_id
          distribution:
            type: uniform
            range: [1, 100000]
```

## Workload Format Reference

See [workload-design.md](../docs/workload-design.md) for complete format specification.

### Top-Level Fields

- `workload.name`: Workload identifier
- `workload.description`: Human-readable description
- `workload.schema`: Table and column definitions
- `workload.data_generation`: Data population strategy
- `workload.operations`: List of operations to execute

### Schema Definition

```yaml
schema:
  tables:
    - name: table_name
      count: 10                   # Number of tables (e.g., sbtest1..sbtest10)
      row_count: 10000           # Rows per table
      columns:
        - name: column_name
          type: INT | CHAR(n) | VARCHAR(n) | DECIMAL(p,s)
          primary_key: true | false
          auto_increment: true | false
          index: index_name
          default: value
```

### Operation Definition

```yaml
operations:
  - name: operation_name
    weight: 50                    # Percentage (60 = 60% probability)
    type: read | write
    sql: "SQL statement with {placeholders} and ?"
    parameters:
      - name: param_name
        distribution:             # For selecting values
          type: uniform | round_robin | zipfian | gaussian
          range: [min, max]
        generator:                # For generating values
          type: integer | string | decimal
          template: "{iteration:0>120}"
```

## Distribution Strategies

- **uniform**: Equal probability across range
- **round_robin**: Cycle through values sequentially
- **zipfian**: 80/20 distribution (hot rows)
- **gaussian**: Normal distribution around center

## Generator Types

- **integer**: Generate random integers
- **string**: Generate strings from template
- **decimal**: Generate decimal numbers
- **choice**: Choose from predefined values

## Sysbench Parameter Mapping

| Sysbench | RSBench Declarative |
|----------|-------------------|
| `--tables` | `schema.tables[].count` |
| `--table-size` | `schema.tables[].row_count` |
| `--range-size` | Operation parameter expression |
| `--point-selects` | Operation weight |
| `--index-updates` | Operation weight |
| `--non-index-updates` | Operation weight |
| `--delete-inserts` | Combined delete+insert weights |

## Migration from Lua

If you have existing sysbench Lua scripts, you can convert them to declarative YAML:

1. Extract schema from `prepare()` function
2. Map operations from `event()` function
3. Convert parameter generation to distributions/generators
4. Set weights based on operation frequency

For complex logic that cannot be expressed declaratively, continue using Lua scripts.
