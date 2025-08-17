#!/bin/bash

set -e

# Default target is x86_64, can be overridden with TARGET environment variable
TARGET=${TARGET:-x86_64}

echo "Building Kosh Operating System for $TARGET..."

# Validate target
case $TARGET in
    x86_64)
        TARGET_JSON="x86_64-kosh.json"
        ;;
    aarch64)
        TARGET_JSON="aarch64-kosh.json"
        ;;
    *)
        echo "Error: Unsupported target '$TARGET'. Supported targets: x86_64, aarch64"
        exit 1
        ;;
esac

# Build kernel for specified target
echo "Building kernel for $TARGET..."
cargo build --package kosh-kernel --target $TARGET_JSON --release -Z build-std=core,alloc

# Create output directory
mkdir -p build/$TARGET

# Copy kernel binary
cp target/${TARGET_JSON%.*}/release/kosh-kernel build/$TARGET/

# Build drivers (currently only for x86_64)
if [ "$TARGET" = "x86_64" ]; then
    echo "Building drivers for $TARGET..."
    cargo build --package kosh-storage-driver --release
    cargo build --package kosh-network-driver --release  
    cargo build --package kosh-graphics-driver --release
    
    # Copy driver binaries
    mkdir -p build/$TARGET/drivers
    cp target/release/libkosh_storage_driver.rlib build/$TARGET/drivers/ 2>/dev/null || true
    cp target/release/libkosh_network_driver.rlib build/$TARGET/drivers/ 2>/dev/null || true
    cp target/release/libkosh_graphics_driver.rlib build/$TARGET/drivers/ 2>/dev/null || true
else
    echo "Driver support for $TARGET not yet implemented"
fi

# Build userspace
echo "Building userspace for $TARGET..."
cargo build --package kosh-init --target $TARGET_JSON --release -Z build-std=core,alloc
cargo build --package kosh-fs-service --target $TARGET_JSON --release -Z build-std=core,alloc
cargo build --package kosh-driver-manager --target $TARGET_JSON --release -Z build-std=core,alloc

# Copy userspace binaries
mkdir -p build/$TARGET/userspace
cp target/${TARGET_JSON%.*}/release/kosh-init build/$TARGET/userspace/
cp target/${TARGET_JSON%.*}/release/kosh-fs-service build/$TARGET/userspace/
cp target/${TARGET_JSON%.*}/release/kosh-driver-manager build/$TARGET/userspace/

echo "Build completed successfully for $TARGET!"
echo "Output directory: build/$TARGET/"