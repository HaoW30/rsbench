#!/bin/bash
#
# Compare two RSBench benchmark results
#
# Usage:
#   ./scripts/compare_results.sh baseline.json current.json
#   ./scripts/compare_results.sh results/before.json results/after.json
#
# Compares key metrics between two benchmark runs:
#   - Throughput (ops/sec)
#   - Latency percentiles (p50, p95, p99)
#   - Error rate
#   - Backpressure percentage
#
# Exit codes:
#   0 - Results compared successfully
#   1 - Error (missing files, invalid JSON, etc.)

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check arguments
if [ $# -ne 2 ]; then
    echo "Usage: $0 <baseline.json> <current.json>"
    echo ""
    echo "Example:"
    echo "  $0 results/baseline.json results/current.json"
    exit 1
fi

BASELINE="$1"
CURRENT="$2"

# Check files exist
if [ ! -f "$BASELINE" ]; then
    echo -e "${RED}Error: Baseline file not found: $BASELINE${NC}"
    exit 1
fi

if [ ! -f "$CURRENT" ]; then
    echo -e "${RED}Error: Current file not found: $CURRENT${NC}"
    exit 1
fi

# Check jq is installed
if ! command -v jq &> /dev/null; then
    echo -e "${RED}Error: jq is not installed${NC}"
    echo "Install with: brew install jq (macOS) or apt-get install jq (Linux)"
    exit 1
fi

# Function to extract metric from JSON
extract_metric() {
    local file=$1
    local path=$2
    jq -r "$path // 0" "$file" 2>/dev/null || echo "0"
}

# Function to calculate percentage change
calc_change() {
    local baseline=$1
    local current=$2

    if [ "$baseline" = "0" ] || [ "$baseline" = "0.0" ]; then
        echo "N/A"
        return
    fi

    echo "scale=2; (($current - $baseline) / $baseline) * 100" | bc -l
}

# Function to format change with color
format_change() {
    local change=$1
    local better_direction=${2:-"higher"}  # "higher" or "lower"

    if [ "$change" = "N/A" ]; then
        echo -e "${YELLOW}N/A${NC}"
        return
    fi

    local is_positive=$(echo "$change > 0" | bc -l)
    local abs_change=$(echo "$change" | tr -d '-')

    if [ "$is_positive" = "1" ]; then
        if [ "$better_direction" = "higher" ]; then
            echo -e "${GREEN}+${abs_change}%${NC}"
        else
            echo -e "${RED}+${abs_change}%${NC}"
        fi
    else
        if [ "$better_direction" = "higher" ]; then
            echo -e "${RED}${change}%${NC}"
        else
            echo -e "${GREEN}${change}%${NC}"
        fi
    fi
}

echo -e "${BLUE}=== RSBench Results Comparison ===${NC}"
echo ""
echo -e "Baseline: ${BASELINE}"
echo -e "Current:  ${CURRENT}"
echo ""

# Extract metrics
echo -e "${BLUE}Throughput${NC}"
baseline_ops=$(extract_metric "$BASELINE" '.summary.total_operations')
current_ops=$(extract_metric "$CURRENT" '.summary.total_operations')
baseline_duration=$(extract_metric "$BASELINE" '.summary.total_duration_secs')
current_duration=$(extract_metric "$CURRENT" '.summary.total_duration_secs')

baseline_throughput=$(echo "scale=2; $baseline_ops / $baseline_duration" | bc -l)
current_throughput=$(echo "scale=2; $current_ops / $current_duration" | bc -l)
throughput_change=$(calc_change "$baseline_throughput" "$current_throughput")

printf "  %-20s %12.2f ops/sec\n" "Baseline:" "$baseline_throughput"
printf "  %-20s %12.2f ops/sec\n" "Current:" "$current_throughput"
printf "  %-20s %s\n" "Change:" "$(format_change "$throughput_change" "higher")"
echo ""

# Latency percentiles
echo -e "${BLUE}Latency (lower is better)${NC}"

for percentile in p50 p95 p99; do
    baseline_latency=$(extract_metric "$BASELINE" ".latency.${percentile}_ms")
    current_latency=$(extract_metric "$CURRENT" ".latency.${percentile}_ms")
    latency_change=$(calc_change "$baseline_latency" "$current_latency")

    printf "  %-20s %12.2f ms → %12.2f ms  %s\n" \
        "${percentile}:" \
        "$baseline_latency" \
        "$current_latency" \
        "$(format_change "$latency_change" "lower")"
done
echo ""

# Error rate
echo -e "${BLUE}Error Rate (lower is better)${NC}"
baseline_errors=$(extract_metric "$BASELINE" '.summary.failed_operations')
current_errors=$(extract_metric "$CURRENT" '.summary.failed_operations')

baseline_error_rate=$(echo "scale=4; ($baseline_errors / $baseline_ops) * 100" | bc -l)
current_error_rate=$(echo "scale=4; ($current_errors / $current_ops) * 100" | bc -l)
error_change=$(calc_change "$baseline_error_rate" "$current_error_rate")

printf "  %-20s %12.4f%% (%d errors)\n" "Baseline:" "$baseline_error_rate" "$baseline_errors"
printf "  %-20s %12.4f%% (%d errors)\n" "Current:" "$current_error_rate" "$current_errors"
printf "  %-20s %s\n" "Change:" "$(format_change "$error_change" "lower")"
echo ""

# Backpressure
echo -e "${BLUE}Backpressure (lower is better)${NC}"
baseline_bp=$(extract_metric "$BASELINE" '.client_metrics.backpressure_percentage')
current_bp=$(extract_metric "$CURRENT" '.client_metrics.backpressure_percentage')
bp_change=$(calc_change "$baseline_bp" "$current_bp")

printf "  %-20s %12.2f%%\n" "Baseline:" "$baseline_bp"
printf "  %-20s %12.2f%%\n" "Current:" "$current_bp"
printf "  %-20s %s\n" "Change:" "$(format_change "$bp_change" "lower")"
echo ""

# Overall assessment
echo -e "${BLUE}=== Assessment ===${NC}"

# Check for regressions
has_regression=0

# Throughput regression (>5% slower)
if [ "$throughput_change" != "N/A" ]; then
    is_regression=$(echo "$throughput_change < -5" | bc -l)
    if [ "$is_regression" = "1" ]; then
        echo -e "${RED}⚠ Throughput regression detected (>5% slower)${NC}"
        has_regression=1
    fi
fi

# Latency regression (>10% slower for p99)
if [ "$latency_change" != "N/A" ]; then
    is_regression=$(echo "$latency_change > 10" | bc -l)
    if [ "$is_regression" = "1" ]; then
        echo -e "${RED}⚠ Latency regression detected (p99 >10% slower)${NC}"
        has_regression=1
    fi
fi

# Error rate increase
if [ "$error_change" != "N/A" ]; then
    is_regression=$(echo "$error_change > 0" | bc -l)
    if [ "$is_regression" = "1" ]; then
        echo -e "${RED}⚠ Error rate increased${NC}"
        has_regression=1
    fi
fi

if [ $has_regression -eq 0 ]; then
    echo -e "${GREEN}✓ No significant regressions detected${NC}"
fi

echo ""
echo -e "${BLUE}Done${NC}"

exit 0
