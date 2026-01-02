# Local Test Scenarios

This directory contains test scenarios used for local testing and development.

**These are NOT user-facing examples** - they contain specific throughput targets, connection counts, and configurations used during RSBench development and performance testing.

## Files

- `*_qps_*.yaml` - Specific throughput test scenarios (10k, 100k QPS)
- `tidb_100k_qps.yaml` - TiDB-specific extreme throughput test
- `*_*conn.yaml` - Connection pool sizing tests

## For Users

If you're looking for example scenarios to use as templates, see the parent directory:
- `../quickstart.yaml` - Simple getting started example
- `../smoke_test.yaml` - Quick validation test
- `../oltp_read_write.yaml` - Standard OLTP benchmark
- `../high_throughput.yaml` - Load testing scenario
- `../capacity_test.yaml` - Capacity planning with ramping load
