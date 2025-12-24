#!/bin/bash
# Run all sample benchmarks
#
# Usage: ./run_all_samples.sh [database_connection_string]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SAMPLE_DIR="$(dirname "$SCRIPT_DIR")"
CONFIGS_DIR="$SAMPLE_DIR/configs"
RESULTS_DIR="$SAMPLE_DIR/results"

# Database connection string (can be overridden)
DB_CONN="${1:-mysql://user:password@localhost:3306/testdb}"

# Create results directory
mkdir -p "$RESULTS_DIR"

echo "================================"
echo "Running RSBench Sample Benchmarks"
echo "================================"
echo "Database: $DB_CONN"
echo "Results: $RESULTS_DIR"
echo ""

# Function to run a single benchmark
run_benchmark() {
    local config_file="$1"
    local config_name=$(basename "$config_file" .yaml)

    echo "Running: $config_name..."

    # TODO: Update connection string in config
    # For now, assume configs use environment variable or default

    # Run benchmark
    if rsbench --config "$config_file"; then
        echo "✓ $config_name completed"
    else
        echo "✗ $config_name failed"
        return 1
    fi

    echo ""
}

# Run all sample configs
for config in "$CONFIGS_DIR"/*.yaml; do
    run_benchmark "$config"
done

echo "================================"
echo "All benchmarks completed!"
echo "Results saved to: $RESULTS_DIR"
echo "================================"
