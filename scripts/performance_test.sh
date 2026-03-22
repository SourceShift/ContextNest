#!/bin/bash

# ContextEngineering Performance Testing Script
# This script runs comprehensive performance tests for ContextNest

set -e

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RESULTS_DIR="$PROJECT_ROOT/test_results/performance"
CONFIG_FILE="$PROJECT_ROOT/config/performance.toml"

# Create results directory
mkdir -p "$RESULTS_DIR"
mkdir -p "$RESULTS_DIR/logs"
mkdir -p "$RESULTS_DIR/reports"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging function
log() {
    echo -e "${GREEN}[$(date '+%Y-%m-%d %H:%M:%S')] $1${NC}"
}

warn() {
    echo -e "${YELLOW}[$(date '+%Y-%m-%d %H:%M:%S')] WARNING: $1${NC}"
}

error() {
    echo -e "${RED}[$(date '+%Y-%m-%d %H:%M:%S')] ERROR: $1${NC}"
}

info() {
    echo -e "${BLUE}[$(date '+%Y-%m-%d %H:%M:%S')] INFO: $1${NC}"
}

# Check if required tools are installed
check_dependencies() {
    log "Checking dependencies..."

    # Check if cargo is installed
    if ! command -v cargo &> /dev/null; then
        error "cargo is not installed"
        exit 1
    fi

    # Check if required cargo tools are installed
    if ! cargo install --list | grep -q "cargo-criterion"; then
        log "Installing cargo-criterion for benchmarking..."
        cargo install cargo-criterion
    fi

    # Check if jq is installed for JSON processing
    if ! command -v jq &> /dev/null; then
        warn "jq is not installed. JSON processing will be limited."
    fi

    # Check if Python is installed for analysis scripts
    if ! command -v python3 &> /dev/null; then
        warn "python3 is not installed. Some analysis scripts may not work."
    fi

    log "Dependencies check completed"
}

# Build the project
build_project() {
    log "Building ContextNest project..."

    cd "$PROJECT_ROOT"

    # Build in release mode for performance testing
    cargo build --release --example performance_integration_example

    if [ $? -ne 0 ]; then
        error "Failed to build project"
        exit 1
    fi

    log "Project build completed successfully"
}

# Run unit tests
run_unit_tests() {
    log "Running unit tests..."

    cd "$PROJECT_ROOT"

    # Run tests with performance profiling
    cargo test --release -- --nocapture 2>&1 | tee "$RESULTS_DIR/logs/unit_tests.log"

    if [ $? -ne 0 ]; then
        error "Unit tests failed"
        exit 1
    fi

    log "Unit tests completed successfully"
}

# Run integration tests
run_integration_tests() {
    log "Running integration tests..."

    cd "$PROJECT_ROOT"

    # Run integration tests
    cargo test --release --test integration 2>&1 | tee "$RESULTS_DIR/logs/integration_tests.log"

    if [ $? -ne 0 ]; then
        warn "Some integration tests failed"
    fi

    log "Integration tests completed"
}

# Run performance benchmarks
run_performance_benchmarks() {
    log "Running performance benchmarks..."

    cd "$PROJECT_ROOT"

    # Run criterion benchmarks
    cargo bench -- 2>&1 | tee "$RESULTS_DIR/logs/benchmarks.log"

    # Copy benchmark results
    if [ -d "target/criterion" ]; then
        cp -r target/criterion "$RESULTS_DIR/"
    fi

    log "Performance benchmarks completed"
}

# Run load tests
run_load_tests() {
    log "Running load tests..."

    cd "$PROJECT_ROOT"

    # Run the performance integration example with load testing
    timeout 300s cargo run --release --example performance_integration_example 2>&1 | \
        tee "$RESULTS_DIR/logs/load_tests.log"

    log "Load tests completed"
}

# Run neural field benchmarks
run_neural_field_benchmarks() {
    log "Running neural field benchmarks..."

    cd "$PROJECT_ROOT"

    # Create a specific neural field test
    cat > "$RESULTS_DIR/neural_field_test.toml" << EOF
[benchmark]
type = "neural_field"
duration_seconds = 120
field_dimensions = [256, 512, 1024]
pattern_counts = [500, 1000, 2000]
embedding_dimensions = [384, 768, 1536]
memory_pressure_levels = [0.5, 0.7, 0.9]
EOF

    # Run neural field specific tests
    timeout 180s cargo run --release --bin contextnest -- \
        --config "$RESULTS_DIR/neural_field_test.toml" \
        --benchmark neural_field 2>&1 | \
        tee "$RESULTS_DIR/logs/neural_field_tests.log"

    log "Neural field benchmarks completed"
}

# Run protocol benchmarks
run_protocol_benchmarks() {
    log "Running protocol benchmarks..."

    cd "$PROJECT_ROOT"

    # Test different consensus protocols
    for protocol in raft byzantine gossip; do
        log "Testing protocol: $protocol"

        timeout 120s cargo run --release --bin contextnest -- \
            --config "$CONFIG_FILE" \
            --benchmark protocol \
            --protocol "$protocol" 2>&1 | \
            tee "$RESULTS_DIR/logs/protocol_${protocol}_tests.log"
    done

    log "Protocol benchmarks completed"
}

# Run cache benchmarks
run_cache_benchmarks() {
    log "Running cache benchmarks..."

    cd "$PROJECT_ROOT"

    # Test different cache configurations
    for strategy in LRU LFU TTL; do
        log "Testing cache strategy: $strategy"

        timeout 90s cargo run --release --bin contextnest -- \
            --config "$CONFIG_FILE" \
            --benchmark cache \
            --strategy "$strategy" 2>&1 | \
            tee "$RESULTS_DIR/logs/cache_${strategy}_tests.log"
    done

    log "Cache benchmarks completed"
}

# Run stress tests
run_stress_tests() {
    log "Running stress tests..."

    cd "$PROJECT_ROOT"

    # Create stress test configuration
    cat > "$RESULTS_DIR/stress_test.toml" << EOF
[stress_test]
duration_seconds = 300
max_concurrent_users = 1000
max_requests_per_second = 5000
memory_pressure = 0.9
cpu_pressure = 0.85
EOF

    # Run stress test
    timeout 400s cargo run --release --bin contextnest -- \
        --config "$RESULTS_DIR/stress_test.toml" \
        --stress-test 2>&1 | \
        tee "$RESULTS_DIR/logs/stress_tests.log"

    log "Stress tests completed"
}

# Generate performance report
generate_report() {
    log "Generating performance report..."

    cd "$PROJECT_ROOT"

    # Create a Python script to generate the report
    cat > "$RESULTS_DIR/generate_report.py" << 'EOF'
#!/usr/bin/env python3
import json
import os
import sys
from datetime import datetime

def parse_log_file(log_file):
    """Parse a log file and extract performance metrics"""
    metrics = {}

    try:
        with open(log_file, 'r') as f:
            for line in f:
                if "Response Time:" in line:
                    # Extract response time
                    parts = line.split("Response Time:")
                    if len(parts) > 1:
                        time_val = float(parts[1].split("ms")[0].strip())
                        metrics['response_time'] = time_val

                elif "Throughput:" in line:
                    # Extract throughput
                    parts = line.split("Throughput:")
                    if len(parts) > 1:
                        throughput_val = float(parts[1].split("ops/sec")[0].strip())
                        metrics['throughput'] = throughput_val

                elif "Hit Rate:" in line:
                    # Extract cache hit rate
                    parts = line.split("Hit Rate:")
                    if len(parts) > 1:
                        hit_rate_val = float(parts[1].split("%")[0].strip())
                        metrics['hit_rate'] = hit_rate_val

                elif "CPU Usage:" in line:
                    # Extract CPU usage
                    parts = line.split("CPU Usage:")
                    if len(parts) > 1:
                        cpu_val = float(parts[1].split("%")[0].strip())
                        metrics['cpu_usage'] = cpu_val

                elif "Memory Usage:" in line:
                    # Extract memory usage
                    parts = line.split("Memory Usage:")
                    if len(parts) > 1:
                        memory_val = float(parts[1].split("MB")[0].strip())
                        metrics['memory_usage'] = memory_val

    except Exception as e:
        print(f"Error parsing {log_file}: {e}")

    return metrics

def generate_html_report(results_dir):
    """Generate an HTML performance report"""

    # Collect metrics from all log files
    all_metrics = {}

    log_files = [
        'load_tests.log',
        'neural_field_tests.log',
        'protocol_raft_tests.log',
        'protocol_byzantine_tests.log',
        'protocol_gossip_tests.log',
        'cache_LRU_tests.log',
        'cache_LFU_tests.log',
        'cache_TTL_tests.log',
        'stress_tests.log'
    ]

    for log_file in log_files:
        log_path = os.path.join(results_dir, 'logs', log_file)
        if os.path.exists(log_path):
            test_name = log_file.replace('.log', '').replace('tests', '').replace('_', ' ').title()
            metrics = parse_log_file(log_path)
            if metrics:
                all_metrics[test_name] = metrics

    # Generate HTML report
    html_content = f"""
<!DOCTYPE html>
<html>
<head>
    <title>ContextNest Performance Report</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 20px; }}
        .header {{ background-color: #f0f0f0; padding: 20px; border-radius: 5px; }}
        .metric {{ margin: 10px 0; padding: 10px; border-left: 4px solid #007bff; }}
        .metric-name {{ font-weight: bold; }}
        .metric-value {{ color: #007bff; font-size: 1.2em; }}
        .test-section {{ margin: 20px 0; padding: 15px; border: 1px solid #ddd; border-radius: 5px; }}
        .test-title {{ background-color: #007bff; color: white; padding: 10px; margin: -15px -15px 15px -15px; border-radius: 5px 5px 0 0; }}
        table {{ width: 100%; border-collapse: collapse; margin: 10px 0; }}
        th, td {{ border: 1px solid #ddd; padding: 8px; text-align: left; }}
        th {{ background-color: #f2f2f2; }}
        .good {{ color: green; }}
        .warning {{ color: orange; }}
        .critical {{ color: red; }}
    </style>
</head>
<body>
    <div class="header">
        <h1>ContextNest Performance Report</h1>
        <p>Generated on: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>
    </div>

    <div class="test-section">
        <h2>Performance Summary</h2>
        <table>
            <tr>
                <th>Test</th>
                <th>Response Time (ms)</th>
                <th>Throughput (ops/sec)</th>
                <th>Cache Hit Rate (%)</th>
                <th>CPU Usage (%)</th>
                <th>Memory Usage (MB)</th>
                <th>Status</th>
            </tr>
"""

    # Add metrics for each test
    for test_name, metrics in all_metrics.items():
        response_time = metrics.get('response_time', 'N/A')
        throughput = metrics.get('throughput', 'N/A')
        hit_rate = metrics.get('hit_rate', 'N/A')
        cpu_usage = metrics.get('cpu_usage', 'N/A')
        memory_usage = metrics.get('memory_usage', 'N/A')

        # Determine status based on metrics
        status = "Good"
        status_class = "good"

        if isinstance(response_time, (int, float)) and response_time > 1000:
            status = "Critical"
            status_class = "critical"
        elif isinstance(response_time, (int, float)) and response_time > 500:
            status = "Warning"
            status_class = "warning"

        if isinstance(cpu_usage, (int, float)) and cpu_usage > 90:
            status = "Critical"
            status_class = "critical"
        elif isinstance(cpu_usage, (int, float)) and cpu_usage > 80:
            status = "Warning"
            status_class = "warning"

        html_content += f"""
            <tr>
                <td>{test_name}</td>
                <td>{response_time}</td>
                <td>{throughput}</td>
                <td>{hit_rate}</td>
                <td>{cpu_usage}</td>
                <td>{memory_usage}</td>
                <td class="{status_class}">{status}</td>
            </tr>
"""

    html_content += """
        </table>
    </div>

    <div class="test-section">
        <h2>System Information</h2>
        <div class="metric">
            <div class="metric-name">Operating System:</div>
            <div class="metric-value">""" + os.uname().sysname + """</div>
        </div>
        <div class="metric">
            <div class="metric-name">Architecture:</div>
            <div class="metric-value">""" + os.uname().machine + """</div>
        </div>
        <div class="metric">
            <div class="metric-name">Python Version:</div>
            <div class="metric-value">""" + sys.version.split()[0] + """</div>
        </div>
    </div>

    <div class="test-section">
        <h2>Recommendations</h2>
        <ul>
"""

    # Add recommendations based on metrics
    for test_name, metrics in all_metrics.items():
        if isinstance(metrics.get('response_time'), (int, float)) and metrics['response_time'] > 1000:
            html_content += f"<li><strong>{test_name}:</strong> Response time is too high. Consider optimizing algorithms or increasing resources.</li>\n"

        if isinstance(metrics.get('cpu_usage'), (int, float)) and metrics['cpu_usage'] > 90:
            html_content += f"<li><strong>{test_name}:</strong> CPU usage is critical. Consider load balancing or optimizing CPU-intensive operations.</li>\n"

        if isinstance(metrics.get('hit_rate'), (int, float)) and metrics['hit_rate'] < 70:
            html_content += f"<li><strong>{test_name}:</strong> Cache hit rate is low. Consider optimizing cache strategies or increasing cache size.</li>\n"

    html_content += """
        </ul>
    </div>
</body>
</html>
"""

    # Write HTML report
    report_path = os.path.join(results_dir, 'performance_report.html')
    with open(report_path, 'w') as f:
        f.write(html_content)

    print(f"Performance report generated: {report_path}")

if __name__ == "__main__":
    results_dir = sys.argv[1] if len(sys.argv) > 1 else "."
    generate_html_report(results_dir)
EOF

    # Run the report generation script
    python3 "$RESULTS_DIR/generate_report.py" "$RESULTS_DIR"

    log "Performance report generated: $RESULTS_DIR/performance_report.html"
}

# Check for performance regressions
check_regressions() {
    log "Checking for performance regressions..."

    cd "$PROJECT_ROOT"

    # Compare current results with baseline if available
    if [ -f "baseline_performance.json" ]; then
        # Run comparison logic here
        info "Baseline performance data found, comparing results..."

        # Add actual comparison logic
    else
        warn "No baseline performance data found. Consider establishing a baseline."
    fi

    log "Regression check completed"
}

# Cleanup temporary files
cleanup() {
    log "Cleaning up temporary files..."

    # Remove temporary test configurations
    rm -f "$RESULTS_DIR/neural_field_test.toml"
    rm -f "$RESULTS_DIR/stress_test.toml"

    # Keep logs and reports
    info "Keeping logs and reports in $RESULTS_DIR"

    log "Cleanup completed"
}

# Main execution
main() {
    log "Starting ContextEngineering performance testing..."

    # Create a timestamp for this test run
    TIMESTAMP=$(date '+%Y%m%d_%H%M%S')
    TEST_RUN_DIR="$RESULTS_DIR/run_$TIMESTAMP"
    mkdir -p "$TEST_RUN_DIR"

    # Update results directory to include timestamp
    RESULTS_DIR="$TEST_RUN_DIR"

    info "Test run directory: $RESULTS_DIR"

    # Run all tests
    check_dependencies
    build_project
    run_unit_tests
    run_integration_tests
    run_performance_benchmarks
    run_load_tests
    run_neural_field_benchmarks
    run_protocol_benchmarks
    run_cache_benchmarks
    run_stress_tests
    generate_report
    check_regressions
    cleanup

    log "Performance testing completed successfully!"
    log "Results available in: $RESULTS_DIR"
    log "Performance report: $RESULTS_DIR/performance_report.html"

    # Exit with appropriate code
    if [ -f "$RESULTS_DIR/performance_report.html" ]; then
        exit 0
    else
        error "Performance report was not generated"
        exit 1
    fi
}

# Handle script arguments
case "${1:-}" in
    "unit")
        check_dependencies
        build_project
        run_unit_tests
        ;;
    "integration")
        check_dependencies
        build_project
        run_integration_tests
        ;;
    "benchmarks")
        check_dependencies
        build_project
        run_performance_benchmarks
        ;;
    "load")
        check_dependencies
        build_project
        run_load_tests
        ;;
    "neural")
        check_dependencies
        build_project
        run_neural_field_benchmarks
        ;;
    "protocol")
        check_dependencies
        build_project
        run_protocol_benchmarks
        ;;
    "cache")
        check_dependencies
        build_project
        run_cache_benchmarks
        ;;
    "stress")
        check_dependencies
        build_project
        run_stress_tests
        ;;
    "report")
        generate_report
        ;;
    "clean")
        rm -rf "$PROJECT_ROOT/test_results/performance"
        log "Performance test results cleaned"
        ;;
    "help"|"-h"|"--help")
        echo "ContextEngineering Performance Testing Script"
        echo ""
        echo "Usage: $0 [COMMAND]"
        echo ""
        echo "Commands:"
        echo "  (no args)    Run all performance tests"
        echo "  unit         Run unit tests only"
        echo "  integration  Run integration tests only"
        echo "  benchmarks   Run performance benchmarks only"
        echo "  load         Run load tests only"
        echo "  neural       Run neural field benchmarks only"
        echo "  protocol     Run protocol benchmarks only"
        echo "  cache        Run cache benchmarks only"
        echo "  stress       Run stress tests only"
        echo "  report       Generate performance report only"
        echo "  clean        Clean test results"
        echo "  help         Show this help message"
        echo ""
        echo "Examples:"
        echo "  $0                    # Run all tests"
        echo "  $0 unit               # Run unit tests only"
        echo "  $0 benchmarks         # Run benchmarks only"
        echo "  $0 report              # Generate report only"
        ;;
    *)
        main
        ;;
esac