# Helper Scripts

Utility scripts to simplify common RSBench workflows.

## Available Scripts

### `run_examples.sh`

Run multiple example scenarios sequentially. Useful for:
- Validating setup after installation
- Regression testing after changes
- Generating baseline results

**Usage:**
```bash
# Use default config
./scripts/run_examples.sh

# Specify custom config
./scripts/run_examples.sh config/rsbench.config.staging.yaml
```

**What it runs:**
1. `quickstart.yaml` - 10 second warmup
2. `smoke_test.yaml` - Quick validation
3. `oltp_read_write.yaml` - Standard benchmark

**Output:**
- Results saved to `results/` directory
- Each run timestamped: `results/quickstart_20250127_220530.json`

---

### `compare_results.sh`

Compare two benchmark results to detect regressions.

**Usage:**
```bash
./scripts/compare_results.sh baseline.json current.json
./scripts/compare_results.sh results/before.json results/after.json
```

**What it compares:**
- Throughput (ops/sec)
- Latency percentiles (p50, p95, p99)
- Error rate
- Backpressure percentage

**Example output:**
```
=== RSBench Results Comparison ===

Baseline: results/baseline.json
Current:  results/current.json

Throughput
  Baseline:              1000.00 ops/sec
  Current:               1050.00 ops/sec
  Change:                +5.00%

Latency (lower is better)
  p50:                     10.50 ms →     9.80 ms  -6.67%
  p95:                     45.20 ms →    43.10 ms  -4.65%
  p99:                     89.30 ms →    91.20 ms  +2.13%

Error Rate (lower is better)
  Baseline:             0.0500% (5 errors)
  Current:              0.0300% (3 errors)
  Change:               -40.00%

Backpressure (lower is better)
  Baseline:                 2.30%
  Current:                  1.80%
  Change:                  -21.74%

=== Assessment ===
✓ No significant regressions detected
```

**Requirements:**
- `jq` installed (`brew install jq` or `apt-get install jq`)

**Exit codes:**
- `0` - Comparison successful
- `1` - Error (missing files, invalid JSON)

**Regression detection:**
- ⚠️ Throughput >5% slower
- ⚠️ p99 latency >10% slower
- ⚠️ Error rate increased

---

## Common Workflows

### Baseline + Regression Testing

```bash
# 1. Create baseline
./scripts/run_examples.sh
cp results/oltp_read_write_*.json results/baseline.json

# 2. Make changes to code/config
# ...

# 3. Run again
./scripts/run_examples.sh
cp results/oltp_read_write_*.json results/current.json

# 4. Compare
./scripts/compare_results.sh results/baseline.json results/current.json
```

### CI/CD Integration

```bash
# In CI pipeline:
./scripts/run_examples.sh config/ci.yaml
./scripts/compare_results.sh baseline.json results/latest.json

# Exit code determines if CI passes/fails
```

### Environment Comparison

```bash
# Compare staging vs production
rsbench --config config/staging.yaml --scenario scenarios/benchmark.yaml > staging.json
rsbench --config config/prod.yaml --scenario scenarios/benchmark.yaml > prod.json
./scripts/compare_results.sh staging.json prod.json
```

---

## Adding New Scripts

When adding new helper scripts:

1. **Make them executable:**
   ```bash
   chmod +x scripts/new_script.sh
   ```

2. **Add usage documentation:**
   ```bash
   # At the top of the script
   # Usage: ./scripts/new_script.sh <args>
   # Description of what it does
   ```

3. **Update this README** with description and examples

4. **Follow conventions:**
   - Use `set -euo pipefail` for safety
   - Check dependencies (jq, bc, etc.)
   - Provide helpful error messages
   - Exit codes: 0 = success, 1 = error
