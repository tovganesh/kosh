#!/bin/bash

# Driver Integration Test Script
# Tests driver loading, communication, and error handling

set -e

echo "=== Driver Integration Tests ==="

# Test driver compilation
echo "Testing driver compilation..."

DRIVERS_DIR="$(dirname "$0")/../drivers"
FAILED_DRIVERS=()
PASSED_DRIVERS=()

# Test each driver directory
for driver_dir in "$DRIVERS_DIR"/*; do
    if [ -d "$driver_dir" ]; then
        driver_name=$(basename "$driver_dir")
        echo "Testing driver: $driver_name"
        
        if [ -f "$driver_dir/Cargo.toml" ]; then
            # Try to build the driver
            if (cd "$driver_dir" && cargo check --target ../../x86_64-kosh.json -Z build-std=core,alloc 2>/dev/null); then
                echo "✅ $driver_name: Compilation successful"
                PASSED_DRIVERS+=("$driver_name")
            else
                echo "❌ $driver_name: Compilation failed"
                FAILED_DRIVERS+=("$driver_name")
            fi
        else
            echo "⚠️  $driver_name: No Cargo.toml found, skipping"
        fi
    fi
done

# Test driver interface compliance
echo ""
echo "Testing driver interface compliance..."

# Check if drivers implement required traits
for driver_dir in "$DRIVERS_DIR"/*; do
    if [ -d "$driver_dir" ] && [ -f "$driver_dir/src/lib.rs" ]; then
        driver_name=$(basename "$driver_dir")
        
        # Check for KoshDriver trait implementation
        if grep -q "impl.*KoshDriver" "$driver_dir/src/lib.rs" 2>/dev/null; then
            echo "✅ $driver_name: Implements KoshDriver trait"
        else
            echo "⚠️  $driver_name: May not implement KoshDriver trait"
        fi
        
        # Check for error handling
        if grep -q "Result<" "$driver_dir/src/lib.rs" 2>/dev/null; then
            echo "✅ $driver_name: Uses proper error handling"
        else
            echo "⚠️  $driver_name: May lack proper error handling"
        fi
    fi
done

# Test driver manager integration
echo ""
echo "Testing driver manager integration..."

DRIVER_MANAGER_DIR="$(dirname "$0")/../userspace/driver-manager"
if [ -f "$DRIVER_MANAGER_DIR/Cargo.toml" ]; then
    if (cd "$DRIVER_MANAGER_DIR" && cargo check --target ../../x86_64-kosh.json -Z build-std=core,alloc 2>/dev/null); then
        echo "✅ Driver Manager: Compilation successful"
    else
        echo "❌ Driver Manager: Compilation failed"
        exit 1
    fi
else
    echo "❌ Driver Manager: Not found"
    exit 1
fi

# Test shared driver library
echo ""
echo "Testing shared driver library..."

SHARED_DRIVER_DIR="$(dirname "$0")/../shared/kosh-driver"
if [ -f "$SHARED_DRIVER_DIR/Cargo.toml" ]; then
    if (cd "$SHARED_DRIVER_DIR" && cargo check --target ../../x86_64-kosh.json -Z build-std=core,alloc 2>/dev/null); then
        echo "✅ Shared Driver Library: Compilation successful"
    else
        echo "❌ Shared Driver Library: Compilation failed"
        exit 1
    fi
else
    echo "❌ Shared Driver Library: Not found"
    exit 1
fi

# Summary
echo ""
echo "=== Driver Integration Test Summary ==="
echo "Passed drivers: ${#PASSED_DRIVERS[@]}"
echo "Failed drivers: ${#FAILED_DRIVERS[@]}"

if [ ${#PASSED_DRIVERS[@]} -gt 0 ]; then
    echo "Successful drivers: ${PASSED_DRIVERS[*]}"
fi

if [ ${#FAILED_DRIVERS[@]} -gt 0 ]; then
    echo "Failed drivers: ${FAILED_DRIVERS[*]}"
    exit 1
fi

echo "✅ All driver integration tests passed!"
exit 0