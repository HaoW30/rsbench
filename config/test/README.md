# Local Test Configurations

This directory contains infrastructure configurations used for local testing and development.

**These are NOT user-facing examples** - they contain specific connection strings, ports, and settings used during RSBench development and testing.

## Files

- `local.yaml` - Local development config for quick tests
- `mysql_*.yaml` - MySQL-specific test configurations with various connection pool sizes
- `tidb_*.yaml` - TiDB-specific test configurations
- `*_port4000.yaml` - Configurations targeting TiDB default port (4000)

## For Users

If you're looking for example configurations to use as templates, see the parent directory:
- `../rsbench.config.yaml` - Well-documented example configuration
- `../rsbench.config.staging.yaml` - Staging environment template
- `../rsbench.config.prod.yaml` - Production environment template
