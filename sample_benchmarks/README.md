# Sample Benchmarks

This directory contains example benchmarks that demonstrate how to use RSBench for database testing.

## Purpose

These samples serve as:
- **User examples**: Ready-to-run benchmarks for common scenarios
- **Documentation**: Reference for creating custom workloads
- **Templates**: Starting points for your own benchmarks

## Directory Structure

```
sample_benchmarks/
├── configs/           # Sample configuration files
├── workloads/         # Sample Lua workloads (when lua feature enabled)
└── scripts/           # Helper scripts for running and analyzing benchmarks
```

## Quick Start

1. **Choose a sample config:**
   ```bash
   cd sample_benchmarks
   ls configs/
   ```

2. **Run a benchmark:**
   ```bash
   rsbench --config configs/oltp_read_write.yaml
   ```

3. **Compare results:**
   ```bash
   ./scripts/compare_results.py results1.json results2.json
   ```

## Available Samples

### Configs

- `oltp_read_write.yaml` - Balanced read/write workload (60% read, 40% write)
- `read_heavy.yaml` - Read-intensive workload (90% reads)
- `write_heavy.yaml` - Write-intensive workload (90% writes)
- `ramping_load.yaml` - Gradual load increase to find limits
- `multi_table.yaml` - Multi-table operations

### Workloads (Lua)

- `simple_point_select.lua` - Basic point select queries
- `insert_heavy.lua` - Bulk insert simulation
- `transaction_mix.lua` - Mixed transactional workload

## Customization

To create your own benchmark:

1. Copy a sample config as a template
2. Modify parameters (rate, duration, table size, etc.)
3. For Lua workloads: copy and modify a sample script
4. Run and iterate

## Tips

- Start with low rates to verify setup
- Use ramping_load.yaml to find capacity limits
- Compare multiple runs for consistency
- Monitor database metrics alongside RSBench output

## Next Steps

See the main README for full documentation on configuration options and workload development.
