#!/bin/bash

# Master Test Runner for Kosh Operating System
# Coordinates unit tests, integration tests, and report generation

set -e

SCRIPT_DIR="$(dirname "$0")"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEST_RESULTS_DIR="$PROJECT_ROOT/test-results"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
NC='\033[0m' # No Color

echo -e "${BLUE}🚀 Kosh Operating System - Comprehensive Test Suite${NC}"
echo -e "${BLUE}====================================================${NC}"

# Create test results directory
mkdir -p "$TEST_RESULTS_DIR"

# Initialize test tracking
total_test_suites=0
passed_test_suites=0
start_time=$(date +%s)

# Function to run a test suite
run_test_suite() {
    local suite_name="$1"
    local script_path="$2"
    local description="$3"
    
    total_test_suites=$((total_test_suites + 1))
    
    echo ""
    echo -e "${PURPLE}[$total_test_suites] Running $suite_name${NC}"
    echo -e "${PURPLE}Description: $description${NC}"
    echo -e "${PURPLE}$(printf '=%.0s' {1..60})${NC}"
    
    if [ -f "$script_path" ] && [ -x "$script_path" ]; then
        if "$script_path"; then
            echo -e "${GREEN}✅ $suite_name: PASSED${NC}"
            passed_test_suites=$((passed_test_suites + 1))
            return 0
        else
            echo -e "${RED}❌ $suite_name: FAILED${NC}"
            return 1
        fi
    else
        echo -e "${YELLOW}⚠️  $suite_name: SKIPPED (script not found or not executable)${NC}"
        return 0
    fi
}

# Test Suite 1: Validation Tests
run_test_suite \
    "Test Framework Validation" \
    "$SCRIPT_DIR/validate-tests.sh" \
    "Validates that the test framework compiles and is properly configured"

# Test Suite 2: Unit Tests
echo ""
echo -e "${PURPLE}Running Unit Tests...${NC}"
if [ -f "$SCRIPT_DIR/run-kernel-tests.sh" ]; then
    run_test_suite \
        "Kernel Unit Tests" \
        "$SCRIPT_DIR/run-kernel-tests.sh" \
        "Comprehensive unit tests for kernel components"
else
    echo -e "${YELLOW}⚠️  Kernel unit tests skipped (test runner not available)${NC}"
fi

# Test Suite 3: Driver Tests
run_test_suite \
    "Driver Integration Tests" \
    "$SCRIPT_DIR/test-drivers.sh" \
    "Tests driver compilation, interface compliance, and integration"

# Test Suite 4: Build System Tests
echo ""
echo -e "${PURPLE}Testing Build System...${NC}"
if ./scripts/build.sh > "$TEST_RESULTS_DIR/build-system-test.log" 2>&1; then
    echo -e "${GREEN}✅ Build System: PASSED${NC}"
    passed_test_suites=$((passed_test_suites + 1))
else
    echo -e "${RED}❌ Build System: FAILED${NC}"
fi
total_test_suites=$((total_test_suites + 1))

# Test Suite 5: Integration Tests
run_test_suite \
    "Integration Tests" \
    "$SCRIPT_DIR/integration-tests.sh" \
    "Full system integration tests including QEMU and VirtualBox"

# Test Suite 6: Multiboot2 Validation
if [ -f "$SCRIPT_DIR/validate-multiboot2.sh" ]; then
    run_test_suite \
        "Multiboot2 Validation" \
        "$SCRIPT_DIR/validate-multiboot2.sh" \
        "Validates multiboot2 compliance of the kernel"
fi

# Calculate test duration
end_time=$(date +%s)
duration=$((end_time - start_time))
minutes=$((duration / 60))
seconds=$((duration % 60))

# Generate comprehensive test report
echo ""
echo -e "${PURPLE}Generating Test Reports...${NC}"
if [ -f "$SCRIPT_DIR/generate-test-report.sh" ]; then
    "$SCRIPT_DIR/generate-test-report.sh"
else
    echo -e "${YELLOW}⚠️  Report generation skipped (generator not available)${NC}"
fi

# Final summary
echo ""
echo -e "${BLUE}================================================================${NC}"
echo -e "${BLUE}                    FINAL TEST SUMMARY${NC}"
echo -e "${BLUE}================================================================${NC}"
echo ""
echo -e "📊 ${BLUE}Test Statistics:${NC}"
echo -e "   Total Test Suites: $total_test_suites"
echo -e "   Passed: ${GREEN}$passed_test_suites${NC}"
echo -e "   Failed: ${RED}$((total_test_suites - passed_test_suites))${NC}"
echo ""
echo -e "⏱️  ${BLUE}Test Duration:${NC} ${minutes}m ${seconds}s"
echo ""

# Calculate success rate
if [ $total_test_suites -gt 0 ]; then
    success_rate=$(( (passed_test_suites * 100) / total_test_suites ))
    echo -e "📈 ${BLUE}Success Rate:${NC} ${success_rate}%"
else
    success_rate=0
fi

echo ""

# Final result
if [ $passed_test_suites -eq $total_test_suites ]; then
    echo -e "${GREEN}🎉 ALL TESTS PASSED! 🎉${NC}"
    echo -e "${GREEN}The Kosh operating system is ready for deployment!${NC}"
    exit_code=0
else
    failed_suites=$((total_test_suites - passed_test_suites))
    echo -e "${RED}⚠️  $failed_suites TEST SUITE(S) FAILED${NC}"
    echo -e "${RED}Please review the test results and fix any issues.${NC}"
    exit_code=1
fi

echo ""
echo -e "${BLUE}Test results and reports are available in:${NC} $TEST_RESULTS_DIR"
echo ""

# Additional information
echo -e "${BLUE}Next Steps:${NC}"
if [ $exit_code -eq 0 ]; then
    echo -e "  • Review test reports for detailed results"
    echo -e "  • Consider running performance benchmarks"
    echo -e "  • Deploy to target hardware for final validation"
else
    echo -e "  • Check failed test logs in $TEST_RESULTS_DIR"
    echo -e "  • Fix any compilation or runtime issues"
    echo -e "  • Re-run tests after fixes"
fi

echo ""
echo -e "${BLUE}================================================================${NC}"

exit $exit_code