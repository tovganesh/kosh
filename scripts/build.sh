#!/bin/bash

set -e

echo "Building Kosh Operating System..."

# Build for x86_64
echo "Building for x86_64..."
cargo build --package kosh-kernel --target x86_64-kosh.json --release -Z build-std=core,alloc

# Create output directory
mkdir -p build/x86_64

# Copy kernel binary
cp target/x86_64-kosh/release/kosh-kernel build/x86_64/

# Build drivers
echo "Building drivers..."
cargo build --package kosh-storage-driver --release
cargo build --package kosh-network-driver --release  
cargo build --package kosh-graphics-driver --release

# Build userspace
echo "Building userspace..."
cargo build --package kosh-init --target x86_64-kosh.json --release -Z build-std=core,alloc
cargo build --package kosh-fs-service --target x86_64-kosh.json --release -Z build-std=core,alloc
cargo build --package kosh-driver-manager --target x86_64-kosh.json --release -Z build-std=core,alloc

echo "Build completed successfully!"