#!/bin/bash

set -e

echo "Creating bootable ISO image..."

# Build the OS first
./scripts/build.sh

# Create ISO directory structure
mkdir -p iso/boot/grub
mkdir -p iso/drivers
mkdir -p iso/system
mkdir -p iso/config

# Copy kernel
cp build/x86_64/kosh-kernel iso/boot/

# Copy drivers (when they become actual binaries)
# cp target/x86_64-kosh/release/deps/libkosh_storage_driver-*.rlib iso/drivers/storage.ko
# cp target/x86_64-kosh/release/deps/libkosh_network_driver-*.rlib iso/drivers/network.ko
# cp target/x86_64-kosh/release/deps/libkosh_graphics_driver-*.rlib iso/drivers/graphics.ko

# Copy userspace binaries
cp target/x86_64-kosh/release/kosh-init iso/system/init
cp target/x86_64-kosh/release/kosh-fs-service iso/system/fs-service

# Create GRUB configuration
cat > iso/boot/grub/grub.cfg << EOF
set timeout=0
set default=0

menuentry "Kosh OS" {
    multiboot2 /boot/kosh-kernel
    boot
}
EOF

# Create system configuration
cat > iso/config/system.conf << EOF
# Kosh OS System Configuration
kernel_log_level=info
driver_autoload=true
swap_enabled=true
EOF

# Create ISO using grub-mkrescue (if available)
if command -v grub-mkrescue &> /dev/null; then
    grub-mkrescue -o kosh.iso iso/
    echo "ISO created: kosh.iso"
else
    echo "Warning: grub-mkrescue not found. ISO structure created in iso/ directory"
    echo "To create the ISO manually, run: grub-mkrescue -o kosh.iso iso/"
fi

echo "ISO creation completed!"