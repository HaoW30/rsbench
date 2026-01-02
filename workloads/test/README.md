# Local Test Workloads

This directory is reserved for workload definitions used during local testing and development.

**These are NOT user-facing examples** - they may contain experimental workload patterns, specific test scenarios, or configurations used during RSBench development.

## Purpose

Use this directory for:
- Experimental workload patterns being tested
- Temporary workload definitions for debugging
- Performance testing workloads with specific characteristics
- Custom test workloads that are not meant to be examples

## For Users

If you're looking for example workloads to use as templates, see the parent directory:
- `../oltp_read_write.yaml` - Balanced 60/40 read/write workload (sysbench default)
- `../oltp_read_only.yaml` - Read-only queries (various SELECT patterns)
- `../oltp_write_only.yaml` - Write-only operations (UPDATE, DELETE, INSERT)
- `../oltp_point_select.yaml` - Pure point select (100% simple reads)

See `../README.md` for complete documentation on creating custom workloads.
