#!/bin/bash

# Kosh Kernel Test Runner
# This script runs the comprehensive kernel test suite

set -e

echo "=== Kosh Kernel Test Suite ==="
echo "Building and running kernel tests..."

# Change to kernel directory
cd "$(dirname "$0")/../kernel"

# Build kernel with test features
echo "Building kernel with test configuration..."
cargo build --target x86_64-kosh.json --features test

# Run tests in QEMU
echo "Running tests in QEMU..."
qemu-system-x86_64 \
    -drive format=raw,file=target/x86_64-kosh/debug/bootimage-kosh-kernel.bin \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -serial stdio \
    -display none \
    -no-reboot \
    -no-shutdown

# Check exit code
EXIT_CODE=$?
if [ $EXIT_CODE -eq 33 ]; then  # QEMU exit code for success (0x10 + 33)
    echo "✅ All tests passed!"
    exit 0
else
    echo "❌ Tests failed with exit code $EXIT_CODE"
    exit 1
fi