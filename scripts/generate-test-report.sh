#!/bin/bash

# Test Report Generator
# Generates comprehensive test reports from test results

set -e

SCRIPT_DIR="$(dirname "$0")"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEST_RESULTS_DIR="$PROJECT_ROOT/test-results"
REPORT_DIR="$TEST_RESULTS_DIR/reports"
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")

# Create report directory
mkdir -p "$REPORT_DIR"

echo "=== Generating Kosh Test Reports ==="

# Function to generate HTML report
generate_html_report() {
    local html_file="$REPORT_DIR/test-report-$TIMESTAMP.html"
    
    cat > "$html_file" << 'EOF'
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Kosh Test Report</title>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; background-color: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .header { text-align: center; color: #333; border-bottom: 2px solid #007acc; padding-bottom: 20px; margin-bottom: 30px; }
        .summary { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin-bottom: 30px; }
        .summary-card { background: #f8f9fa; padding: 20px; border-radius: 6px; text-align: center; border-left: 4px solid #007acc; }
        .summary-card h3 { margin: 0 0 10px 0; color: #333; }
        .summary-card .number { font-size: 2em; font-weight: bold; color: #007acc; }
        .test-section { margin-bottom: 30px; }
        .test-section h2 { color: #333; border-bottom: 1px solid #ddd; padding-bottom: 10px; }
        .test-result { padding: 10px; margin: 5px 0; border-radius: 4px; }
        .test-pass { background-color: #d4edda; border-left: 4px solid #28a745; }
        .test-fail { background-color: #f8d7da; border-left: 4px solid #dc3545; }
        .test-skip { background-color: #fff3cd; border-left: 4px solid #ffc107; }
        .timestamp { color: #666; font-size: 0.9em; }
        .details { margin-top: 10px; font-size: 0.9em; color: #666; }
        .footer { text-align: center; margin-top: 40px; padding-top: 20px; border-top: 1px solid #ddd; color: #666; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🚀 Kosh Operating System</h1>
            <h2>Test Report</h2>
            <p class="timestamp">Generated on: $(date)</p>
        </div>
        
        <div class="summary">
            <div class="summary-card">
                <h3>Total Tests</h3>
                <div class="number" id="total-tests">0</div>
            </div>
            <div class="summary-card">
                <h3>Passed</h3>
                <div class="number" id="passed-tests" style="color: #28a745;">0</div>
            </div>
            <div class="summary-card">
                <h3>Failed</h3>
                <div class="number" id="failed-tests" style="color: #dc3545;">0</div>
            </div>
            <div class="summary-card">
                <h3>Skipped</h3>
                <div class="number" id="skipped-tests" style="color: #ffc107;">0</div>
            </div>
            <div class="summary-card">
                <h3>Success Rate</h3>
                <div class="number" id="success-rate" style="color: #007acc;">0%</div>
            </div>
        </div>
        
        <div class="test-section">
            <h2>📊 Test Results</h2>
            <div id="test-results">
EOF

    # Parse test results and add to HTML
    if [ -f "$TEST_RESULTS_DIR/integration-test-results.log" ]; then
        local total=0 passed=0 failed=0 skipped=0
        
        while IFS= read -r line; do
            if [[ $line =~ (.+):\ (.+)\ -\ (PASS|FAIL|SKIP)\ -\ (.+) ]]; then
                local timestamp="${BASH_REMATCH[1]}"
                local test_name="${BASH_REMATCH[2]}"
                local result="${BASH_REMATCH[3]}"
                local details="${BASH_REMATCH[4]}"
                
                total=$((total + 1))
                
                case $result in
                    PASS)
                        passed=$((passed + 1))
                        echo "                <div class=\"test-result test-pass\">" >> "$html_file"
                        echo "                    <strong>✅ $test_name</strong>" >> "$html_file"
                        ;;
                    FAIL)
                        failed=$((failed + 1))
                        echo "                <div class=\"test-result test-fail\">" >> "$html_file"
                        echo "                    <strong>❌ $test_name</strong>" >> "$html_file"
                        ;;
                    SKIP)
                        skipped=$((skipped + 1))
                        echo "                <div class=\"test-result test-skip\">" >> "$html_file"
                        echo "                    <strong>⚠️ $test_name</strong>" >> "$html_file"
                        ;;
                esac
                
                echo "                    <div class=\"details\">$details</div>" >> "$html_file"
                echo "                    <div class=\"timestamp\">$timestamp</div>" >> "$html_file"
                echo "                </div>" >> "$html_file"
            fi
        done < "$TEST_RESULTS_DIR/integration-test-results.log"
        
        # Calculate success rate
        local success_rate=0
        if [ $total -gt 0 ]; then
            success_rate=$(( (passed * 100) / total ))
        fi
        
        # Update summary with JavaScript
        cat >> "$html_file" << EOF
            </div>
        </div>
        
        <div class="footer">
            <p>Generated by Kosh Test Suite | $(date)</p>
        </div>
    </div>
    
    <script>
        document.getElementById('total-tests').textContent = '$total';
        document.getElementById('passed-tests').textContent = '$passed';
        document.getElementById('failed-tests').textContent = '$failed';
        document.getElementById('skipped-tests').textContent = '$skipped';
        document.getElementById('success-rate').textContent = '$success_rate%';
    </script>
</body>
</html>
EOF
    else
        echo "                <p>No test results found.</p>" >> "$html_file"
        echo "            </div>" >> "$html_file"
        echo "        </div>" >> "$html_file"
        echo "    </div>" >> "$html_file"
        echo "</body>" >> "$html_file"
        echo "</html>" >> "$html_file"
    fi
    
    echo "HTML report generated: $html_file"
}

# Function to generate JUnit XML report
generate_junit_xml() {
    local xml_file="$REPORT_DIR/junit-report-$TIMESTAMP.xml"
    
    cat > "$xml_file" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="Kosh Test Suite">
    <testsuite name="Integration Tests" tests="0" failures="0" errors="0" skipped="0" time="0">
EOF

    if [ -f "$TEST_RESULTS_DIR/integration-test-results.log" ]; then
        local total=0 failures=0 skipped=0
        
        while IFS= read -r line; do
            if [[ $line =~ (.+):\ (.+)\ -\ (PASS|FAIL|SKIP)\ -\ (.+) ]]; then
                local test_name="${BASH_REMATCH[2]}"
                local result="${BASH_REMATCH[3]}"
                local details="${BASH_REMATCH[4]}"
                
                total=$((total + 1))
                
                echo "        <testcase name=\"$test_name\" classname=\"IntegrationTest\">" >> "$xml_file"
                
                case $result in
                    FAIL)
                        failures=$((failures + 1))
                        echo "            <failure message=\"Test failed\">$details</failure>" >> "$xml_file"
                        ;;
                    SKIP)
                        skipped=$((skipped + 1))
                        echo "            <skipped message=\"Test skipped\">$details</skipped>" >> "$xml_file"
                        ;;
                esac
                
                echo "        </testcase>" >> "$xml_file"
            fi
        done < "$TEST_RESULTS_DIR/integration-test-results.log"
        
        # Update testsuite attributes
        sed -i "s/tests=\"0\"/tests=\"$total\"/" "$xml_file"
        sed -i "s/failures=\"0\"/failures=\"$failures\"/" "$xml_file"
        sed -i "s/skipped=\"0\"/skipped=\"$skipped\"/" "$xml_file"
    fi
    
    echo "    </testsuite>" >> "$xml_file"
    echo "</testsuites>" >> "$xml_file"
    
    echo "JUnit XML report generated: $xml_file"
}

# Function to generate summary report
generate_summary() {
    local summary_file="$REPORT_DIR/summary-$TIMESTAMP.txt"
    
    cat > "$summary_file" << EOF
Kosh Operating System - Test Summary Report
Generated on: $(date)
==========================================

EOF

    if [ -f "$TEST_RESULTS_DIR/integration-test-results.log" ]; then
        local total=0 passed=0 failed=0 skipped=0
        
        while IFS= read -r line; do
            if [[ $line =~ (.+):\ (.+)\ -\ (PASS|FAIL|SKIP)\ -\ (.+) ]]; then
                local result="${BASH_REMATCH[3]}"
                total=$((total + 1))
                
                case $result in
                    PASS) passed=$((passed + 1)) ;;
                    FAIL) failed=$((failed + 1)) ;;
                    SKIP) skipped=$((skipped + 1)) ;;
                esac
            fi
        done < "$TEST_RESULTS_DIR/integration-test-results.log"
        
        local success_rate=0
        if [ $total -gt 0 ]; then
            success_rate=$(( (passed * 100) / total ))
        fi
        
        cat >> "$summary_file" << EOF
Test Statistics:
- Total Tests: $total
- Passed: $passed
- Failed: $failed
- Skipped: $skipped
- Success Rate: $success_rate%

EOF

        if [ $failed -eq 0 ]; then
            echo "✅ All tests passed successfully!" >> "$summary_file"
        else
            echo "❌ Some tests failed. Review the detailed reports for more information." >> "$summary_file"
        fi
    else
        echo "No test results found." >> "$summary_file"
    fi
    
    echo "Summary report generated: $summary_file"
}

# Generate all reports
echo "Generating HTML report..."
generate_html_report

echo "Generating JUnit XML report..."
generate_junit_xml

echo "Generating summary report..."
generate_summary

echo ""
echo "✅ All reports generated successfully!"
echo "Reports location: $REPORT_DIR"
echo ""
echo "Available reports:"
ls -la "$REPORT_DIR"/*$TIMESTAMP*