# Workload Module Design

## Overview

The workload module supports two types of workload definitions:

1. **Declarative YAML** - For most use cases, including all sysbench built-in tests
2. **Lua Scripts** - For complex custom workloads requiring programmatic logic

## Design Principle

**Workloads should be declarative by default, programmable when needed.**

The built-in OLTP tests (oltp_read_write, oltp_read_only, etc.) are NOT hardcoded Rust structs. Instead, they are **declarative YAML files** that are interpreted by a generic workload engine.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Workload Factory                          │
│                                                              │
│  ┌────────────────────┐         ┌─────────────────────┐    │
│  │ Declarative YAML   │         │   Lua Script        │    │
│  │  (Primary)         │         │   (Advanced)        │    │
│  └──────────┬─────────┘         └──────────┬──────────┘    │
│             │                              │                │
│             ▼                              ▼                │
│  ┌────────────────────┐         ┌─────────────────────┐    │
│  │ DeclarativeWorkload│         │   LuaWorkload       │    │
│  │  - Parses YAML     │         │   - Executes Lua    │    │
│  │  - Generates ops   │         │   - Custom logic    │    │
│  └────────────────────┘         └─────────────────────┘    │
│             │                              │                │
│             └──────────────┬───────────────┘                │
│                            ▼                                │
│                   ┌─────────────────┐                       │
│                   │ Workload Trait  │                       │
│                   │  - prepare()    │                       │
│                   │  - next_op()    │                       │
│                   │  - cleanup()    │                       │
│                   └─────────────────┘                       │
└─────────────────────────────────────────────────────────────┘
```

## Declarative Workload Format

### Top-Level Structure

```yaml
# workloads/oltp_read_write.yaml
workload:
  name: oltp_read_write
  description: "Balanced read/write OLTP workload (sysbench compatible)"

  # Schema definition
  schema:
    tables:
      - name: sbtest
        count: 10                    # Creates sbtest1..sbtest10
        row_count: 10000             # Rows per table
        columns:
          - name: id
            type: INT
            primary_key: true
            auto_increment: false
          - name: k
            type: INT
            default: 0
            index: k_idx
          - name: c
            type: CHAR(120)
            default: ""
          - name: pad
            type: CHAR(60)
            default: ""

  # Data generation (for prepare phase)
  data_generation:
    strategy: uniform              # uniform, zipfian, gaussian
    seed: null                     # Use scenario seed if not specified

  # Operations definition
  operations:
    # Point select (60% probability)
    - name: point_select
      weight: 60
      type: read
      sql: "SELECT c FROM sbtest{table_id} WHERE id = ?"
      parameters:
        - name: table_id
          distribution:
            type: round_robin       # Round robin across tables
            range: [1, ${table_count}]
        - name: id
          distribution:
            type: uniform           # Uniform within table
            range: [1, ${row_count}]

    # Update non-index (40% probability)
    - name: update_non_index
      weight: 40
      type: write
      sql: "UPDATE sbtest{table_id} SET c = ? WHERE id = ?"
      parameters:
        - name: table_id
          distribution:
            type: round_robin
            range: [1, ${table_count}]
        - name: c
          generator:
            type: string
            template: "{iteration:0>120}"  # Pad iteration to 120 chars
        - name: id
          distribution:
            type: uniform
            range: [1, ${row_count}]
```

### Sysbench Built-in Tests as Declarative Workloads

All sysbench built-in tests are defined as YAML files:

```
workloads/
├── oltp_read_write.yaml       # Balanced 60/40 read/write
├── oltp_read_only.yaml        # 100% reads (point selects, ranges)
├── oltp_write_only.yaml       # 100% writes (updates, deletes, inserts)
├── oltp_insert.yaml           # Insert-only workload
├── oltp_update_index.yaml     # Update indexed column
├── oltp_update_non_index.yaml # Update non-indexed column
├── oltp_delete.yaml           # Delete workload
├── oltp_point_select.yaml     # Point select only
├── select_random_points.yaml  # Random point selects
└── select_random_ranges.yaml  # Range queries
```

## Configuration in Scenario Files

### Option 1: Reference Pre-defined Workload

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

    # Override specific parameters
    overrides:
      schema:
        tables:
          - row_count: 50000        # Override row count
      operations:
        - name: point_select
          weight: 80                # Override weight (80% reads)
        - name: update_non_index
          weight: 20                # 20% writes
```

### Option 2: Inline Workload Definition

```yaml
# scenarios/custom_test.yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  workload:
    type: declarative
    definition:
      name: custom_workload
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

### Option 3: Lua Script (Complex Workloads)

```yaml
# scenarios/complex_test.yaml
scenario:
  executor:
    type: constant-rate
    rate: 1000
    duration: 60s

  workload:
    type: lua
    script: workloads/custom_transaction.lua
```

## Declarative Workload Features

### 1. Schema Definition

Supports all sysbench table features:
- Multiple tables
- Auto-increment columns
- Indexes
- Column types (INT, CHAR, VARCHAR, DECIMAL, etc.)
- Default values

### 2. Data Generation

Supports multiple distribution strategies:
- **uniform**: Equal probability
- **zipfian**: 80/20 distribution (hot rows)
- **gaussian**: Normal distribution
- **sequential**: Sequential access

### 3. Operations

Each operation can specify:
- **SQL template**: With variable substitution
- **Weight**: Probability (e.g., 60 for 60%)
- **Type**: Read or Write
- **Parameters**: With generators/distributions
- **Transaction**: Optional transaction boundaries

### 4. Advanced Features

**Transactions:**
```yaml
operations:
  - name: transfer
    weight: 30
    type: write
    transaction: true
    statements:
      - sql: "UPDATE accounts SET balance = balance - ? WHERE id = ?"
        parameters: [amount, from_id]
      - sql: "UPDATE accounts SET balance = balance + ? WHERE id = ?"
        parameters: [amount, to_id]
```

**Conditional Logic:**
```yaml
operations:
  - name: conditional_update
    weight: 50
    type: write
    sql: "UPDATE users SET status = ? WHERE id = ? AND status = 'active'"
    parameters:
      - name: status
        generator:
          type: choice
          values: ['pending', 'active', 'inactive']
      - name: id
        distribution:
          type: uniform
          range: [1, 10000]
```

**Range Queries:**
```yaml
operations:
  - name: range_select
    weight: 20
    type: read
    sql: "SELECT * FROM sbtest{table_id} WHERE k BETWEEN ? AND ?"
    parameters:
      - name: table_id
        distribution:
          type: round_robin
          range: [1, ${table_count}]
      - name: start_k
        distribution:
          type: uniform
          range: [1, 100000]
      - name: end_k
        expression: "${start_k} + ${range_size}"
        where:
          range_size: 100
```

## Sysbench Compatibility

### All Sysbench OLTP Parameters Supported

The declarative format supports all sysbench parameters:

| Sysbench Parameter | Declarative Equivalent |
|-------------------|----------------------|
| `--tables` | `schema.tables[].count` |
| `--table-size` | `schema.tables[].row_count` |
| `--range-size` | Operation parameter `range_size` |
| `--point-selects` | Operation weight for point_select |
| `--simple-ranges` | Operation weight for simple_range |
| `--sum-ranges` | Operation weight for sum_range |
| `--order-ranges` | Operation weight for order_range |
| `--distinct-ranges` | Operation weight for distinct_range |
| `--index-updates` | Operation weight for update_index |
| `--non-index-updates` | Operation weight for update_non_index |
| `--delete-inserts` | Operation weight for delete + insert |
| `--skip-trx` | `operations[].transaction: false` |
| `--auto-inc` | `columns[].auto_increment: true` |

### Sysbench Built-in Test Mappings

Each sysbench test is a declarative YAML file:

```bash
# Sysbench command
sysbench oltp_read_write --tables=10 --table-size=10000 run

# RSBench equivalent
rsbench --scenario scenarios/oltp_read_write.yaml \
        --workload-override tables.row_count=10000
```

## Implementation Overview

### Module Structure

```
src/workload/
├── mod.rs                    # Workload trait, factory
├── declarative/
│   ├── mod.rs               # DeclarativeWorkload implementation
│   ├── parser.rs            # YAML parsing
│   ├── schema.rs            # Schema definition & table creation
│   ├── generator.rs         # Data generation (uniform, zipfian, etc.)
│   ├── operations.rs        # Operation generation
│   ├── parameters.rs        # Parameter value generation
│   └── distributions.rs     # Distribution strategies
├── lua/
│   └── mod.rs               # LuaWorkload (existing)
└── legacy/
    └── oltp.rs              # Deprecated: Remove after migration
```

### Workload Factory Logic

```rust
impl WorkloadFactory {
    pub fn create(config: &WorkloadConfig, seed: u64) -> Result<Box<dyn Workload>> {
        match config {
            // Declarative workload (YAML file or inline)
            WorkloadConfig::Declarative { file, definition, overrides } => {
                DeclarativeWorkload::new(file, definition, overrides, seed)
            }

            // Lua script
            WorkloadConfig::Lua { script } => {
                LuaWorkload::new(script, seed)
            }

            // Legacy builtin (deprecated, warn user)
            WorkloadConfig::Builtin { name, .. } => {
                eprintln!("Warning: Builtin workloads are deprecated. Use declarative workloads instead.");
                DeclarativeWorkload::load_builtin(name, seed)
            }
        }
    }
}
```

## Migration Path

### Phase 1: Add Declarative Support (Current)
- Define declarative workload format
- Update config module to support new format
- Create YAML files for all sysbench tests
- Update documentation

### Phase 2: Implement Declarative Engine
- Implement DeclarativeWorkload
- Parser for YAML format
- Schema creation
- Operation generation with distributions
- Parameter value generation

### Phase 3: Deprecate Hardcoded Tests
- Mark `OltpReadWrite` as deprecated
- Migrate all usage to declarative YAML
- Remove hardcoded implementations

### Phase 4: Advanced Features
- Transaction support
- Conditional operations
- Complex parameter relationships
- Custom generators

## Benefits

1. **User Flexibility**: Users can create custom workloads without writing code
2. **Transparency**: Workload logic is visible and modifiable in YAML
3. **Reusability**: Share workload definitions across teams
4. **Version Control**: Workloads are text files, easy to diff and review
5. **Sysbench Compatibility**: Full feature parity with sysbench
6. **Extensibility**: Easy to add new operation types or distributions

## Example Use Cases

### Custom E-commerce Workload
```yaml
workload:
  name: ecommerce
  schema:
    tables:
      - name: products
        row_count: 100000
      - name: orders
        row_count: 1000000
      - name: customers
        row_count: 50000

  operations:
    - name: browse_products
      weight: 50
      sql: "SELECT * FROM products WHERE category = ? LIMIT 20"

    - name: place_order
      weight: 30
      transaction: true
      statements:
        - sql: "INSERT INTO orders (...) VALUES (?...)"
        - sql: "UPDATE products SET stock = stock - ? WHERE id = ?"

    - name: customer_lookup
      weight: 20
      sql: "SELECT * FROM customers WHERE email = ?"
```

### Analytics Workload
```yaml
workload:
  name: analytics
  operations:
    - name: daily_report
      weight: 10
      sql: "SELECT DATE(created_at), COUNT(*) FROM events WHERE created_at >= ? GROUP BY 1"

    - name: user_activity
      weight: 40
      sql: "SELECT user_id, COUNT(*) FROM events WHERE timestamp >= ? GROUP BY 1 ORDER BY 2 DESC LIMIT 100"

    - name: aggregate_stats
      weight: 50
      sql: "SELECT COUNT(*), AVG(value), MAX(value) FROM metrics WHERE timestamp >= ?"
```

## Conclusion

This design makes workloads **first-class, declarative, user-configurable entities** rather than hardcoded black boxes. Users can:

1. Use pre-defined sysbench-compatible tests
2. Customize existing tests via overrides
3. Create entirely custom workloads in YAML
4. Fall back to Lua for complex logic

The declarative approach provides the **flexibility of configuration with the power of code when needed**.
