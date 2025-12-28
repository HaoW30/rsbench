#!/bin/bash
#
# Run common example scenarios
#
# Usage:
#   ./scripts/run_examples.sh [config_file]
#
# Examples:
#   ./scripts/run_examples.sh                              # Use default config
#   ./scripts/run_examples.sh config/rsbench.config.yaml   # Specify config
#
# What this does:
#   - Runs multiple example scenarios sequentially
#   - Saves results to results/ directory
#   - Useful for validating setup or regression testing

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="$PROJECT_ROOT/results"

# Default config
CONFIG="${1:-$PROJECT_ROOT/config/rsbench.config.yaml}"

# Create results directory
mkdir -p "$RESULTS_DIR"

# Check if rsbench is built
RSBENCH="$PROJECT_ROOT/target/release/rsbench"
if [ ! -f "$RSBENCH" ]; then
    RSBENCH="$PROJECT_ROOT/target/debug/rsbench"
    if [ ! -f "$RSBENCH" ]; then
        echo "Error: rsbench not found. Build it first:"
        echo "  cargo build --release"
        exit 1
    fi
fi

echo "================================"
echo "Running RSBench Example Scenarios"
echo "================================"
echo "Using config: $CONFIG"
echo "Results dir:  $RESULTS_DIR"
echo "RSBench bin:  $RSBENCH"
echo ""

# Scenarios to run (in order)
SCENARIOS=(
    "quickstart.yaml:Quickstart (10s warmup)"
    "smoke_test.yaml:Smoke Test (quick validation)"
    "oltp_read_write.yaml:OLTP Read/Write (standard benchmark)"
)

# Function to run a single scenario
run_scenario() {
    local scenario_file="$1"
    local description="$2"
    local scenario_name=$(basename "$scenario_file" .yaml)
    local result_file="$RESULTS_DIR/${scenario_name}_$(date +%Y%m%d_%H%M%S).json"

    echo "─────────────────────────────────"
    echo "Running: $description"
    echo "Scenario: scenarios/$scenario_file"
    echo "─────────────────────────────────"

    # Run benchmark
    if "$RSBENCH" \
        --config "$CONFIG" \
        --scenario "$PROJECT_ROOT/scenarios/$scenario_file" \
        --output-format json > "$result_file" 2>&1; then
        echo "✓ $description completed"
        echo "  Results: $result_file"
    else
        echo "✗ $description failed"
        echo "  Check logs: $result_file"
        return 1
    fi

    echo ""
}

# Track failures
failed_scenarios=()

# Run all scenarios
for scenario_entry in "${SCENARIOS[@]}"; do
    IFS=':' read -r scenario_file description <<< "$scenario_entry"

    if ! run_scenario "$scenario_file" "$description"; then
        failed_scenarios+=("$description")
    fi
done

# Summary
echo "================================"
echo "Summary"
echo "================================"
total=${#SCENARIOS[@]}
failed=${#failed_scenarios[@]}
passed=$((total - failed))

echo "Total:  $total scenarios"
echo "Passed: $passed"
echo "Failed: $failed"

if [ ${#failed_scenarios[@]} -gt 0 ]; then
    echo ""
    echo "Failed scenarios:"
    for scenario in "${failed_scenarios[@]}"; do
        echo "  - $scenario"
    done
    exit 1
fi

echo ""
echo "✓ All scenarios passed!"
echo "Results saved to: $RESULTS_DIR"
echo ""
echo "Compare results:"
echo "  ./scripts/compare_results.sh results/baseline.json results/latest.json"
echo "================================"

exit 0
