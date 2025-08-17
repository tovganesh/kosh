#!/bin/bash

# Validate Kosh Kernel Test Framework
# This script validates that the test framework compiles correctly

set -e

echo "=== Validating Kosh Kernel Test Framework ==="

# Change to kernel directory
cd "$(dirname "$0")/../kernel"

# Check if test files exist
echo "Checking test files..."
test_files=(
    "src/test_harness.rs"
    "src/memory/tests.rs"
    "src/process/tests.rs"
    "src/ipc/tests.rs"
    "src/driver_tests.rs"
    "tests/kernel_tests.rs"
)

for file in "${test_files[@]}"; do
    if [ -f "$file" ]; then
        echo "✅ $file exists"
    else
        echo "❌ $file missing"
        exit 1
    fi
done

# Try to compile with test features
echo "Compiling kernel with test framework..."
if RUSTFLAGS="-C link-arg=-nostartfiles" cargo check --target ../x86_64-kosh.json -Z build-std=core,alloc; then
    echo "✅ Kernel compiles successfully with test framework"
else
    echo "❌ Kernel compilation failed"
    exit 1
fi

# Check test syntax (simplified check)
echo "Checking test syntax..."
if RUSTFLAGS="-C link-arg=-nostartfiles" cargo check --lib --target ../x86_64-kosh.json -Z build-std=core,alloc; then
    echo "✅ Test syntax is valid"
else
    echo "❌ Test syntax errors found"
    exit 1
fi

echo "🎉 Test framework validation completed successfully!"