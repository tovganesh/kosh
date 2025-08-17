#!/bin/bash

set -e

# Configuration
PACKAGE_DIR="packages"
TARGET_DIR="target/x86_64-kosh/release"
DRIVERS_DIR="drivers"
USERSPACE_DIR="userspace"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Create package directory structure
setup_packaging() {
    log_info "Setting up packaging environment..."
    
    rm -rf "$PACKAGE_DIR"
    mkdir -p "$PACKAGE_DIR"/{drivers,userspace,kernel,docs}
    
    log_success "Package directory structure created"
}

# Package kernel
package_kernel() {
    log_info "Packaging kernel..."
    
    local kernel_path="build/x86_64/kosh-kernel"
    
    if [ -f "$kernel_path" ]; then
        cp "$kernel_path" "$PACKAGE_DIR/kernel/"
        
        # Create kernel metadata
        cat > "$PACKAGE_DIR/kernel/kernel.meta" << EOF
[kernel]
name=kosh-kernel
version=0.1.0
architecture=x86_64
build_date=$(date -Iseconds)
size=$(stat -c%s "$kernel_path" 2>/dev/null || stat -f%z "$kernel_path" 2>/dev/null)
multiboot2=true
debug_symbols=false

[features]
memory_management=true
process_management=true
ipc=true
swap_support=true
power_management=true
driver_framework=true

[requirements]
min_memory_mb=128
recommended_memory_mb=512
cpu_arch=x86_64
multiboot2_bootloader=required
EOF
        
        log_success "Kernel packaged with metadata"
    else
        log_error "Kernel not found at $kernel_path"
        return 1
    fi
}

# Package drivers
package_drivers() {
    log_info "Packaging drivers..."
    
    local driver_components=(
        "graphics:Graphics Driver:VGA text mode display driver"
        "keyboard:Keyboard Driver:PS/2 keyboard input driver"
        "storage:Storage Driver:Basic storage device driver"
        "network:Network Driver:Network interface driver"
        "touch:Touch Driver:Touch input driver for mobile devices"
    )
    
    for component in "${driver_components[@]}"; do
        local name="${component%%:*}"
        local display_name="${component#*:}"
        local display_name="${display_name%%:*}"
        local description="${component##*:}"
        
        log_info "Packaging $display_name..."
        
        local driver_dir="$PACKAGE_DIR/drivers/$name"
        mkdir -p "$driver_dir"
        
        # Check if driver library exists
        local driver_lib="$TARGET_DIR/libkosh_${name}_driver.rlib"
        if [ -f "$driver_lib" ]; then
            cp "$driver_lib" "$driver_dir/${name}.ko"
            log_success "Copied driver binary: $name"
        else
            # Create placeholder driver
            cat > "$driver_dir/${name}.ko" << EOF
#!/bin/sh
# Kosh OS Driver Placeholder: $display_name
# This is a placeholder for the actual driver binary
echo "Loading $display_name..."
echo "Driver: $name"
echo "Status: Placeholder - not yet implemented as separate binary"
EOF
            chmod +x "$driver_dir/${name}.ko"
            log_warning "Created placeholder for $name driver"
        fi
        
        # Create driver metadata
        cat > "$driver_dir/driver.meta" << EOF
[driver]
name=$name
display_name=$display_name
description=$description
version=0.1.0
architecture=x86_64
build_date=$(date -Iseconds)
type=kernel_module
isolation=driver_space

[capabilities]
hardware_access=true
memory_allocation=true
interrupt_handling=true
dma_access=false

[dependencies]
kernel_version=0.1.0
required_services=[]
optional_services=[]

[configuration]
autoload=true
priority=medium
load_timeout=30
EOF
        
        # Create driver configuration template
        cat > "$driver_dir/${name}.conf" << EOF
# $display_name Configuration
# This file contains driver-specific configuration options

[${name}]
enabled=true
debug=false
log_level=info

# Driver-specific options would go here
# For example:
# polling_interval=100
# buffer_size=4096
# max_devices=8
EOF
        
        log_success "$display_name packaged"
    done
}

# Package userspace components
package_userspace() {
    log_info "Packaging userspace components..."
    
    local userspace_components=(
        "kosh-init:init:System initialization process"
        "kosh-fs-service:fs-service:File system service"
        "kosh-driver-manager:driver-manager:Driver management service"
        "kosh-shell:shell:Interactive shell"
    )
    
    for component in "${userspace_components[@]}"; do
        local binary_name="${component%%:*}"
        local service_name="${component#*:}"
        local service_name="${service_name%%:*}"
        local description="${component##*:}"
        
        log_info "Packaging $service_name..."
        
        local service_dir="$PACKAGE_DIR/userspace/$service_name"
        mkdir -p "$service_dir"
        
        # Check if binary exists
        local binary_path="$TARGET_DIR/$binary_name"
        if [ -f "$binary_path" ]; then
            cp "$binary_path" "$service_dir/$service_name"
            chmod +x "$service_dir/$service_name"
            log_success "Copied binary: $service_name"
        else
            # Create placeholder service
            cat > "$service_dir/$service_name" << EOF
#!/bin/sh
# Kosh OS Service: $service_name
# Description: $description
echo "Starting $service_name..."
echo "Service: $service_name"
echo "Status: Placeholder - not yet fully implemented"
sleep 1
echo "$service_name started (placeholder mode)"
EOF
            chmod +x "$service_dir/$service_name"
            log_warning "Created placeholder for $service_name"
        fi
        
        # Create service metadata
        cat > "$service_dir/service.meta" << EOF
[service]
name=$service_name
binary_name=$binary_name
description=$description
version=0.1.0
architecture=x86_64
build_date=$(date -Iseconds)
type=userspace_service

[runtime]
user_space=true
privileges=normal
memory_limit_mb=64
cpu_priority=normal

[dependencies]
required_services=[]
optional_services=[]
required_drivers=[]

[startup]
autostart=true
start_order=10
restart_on_failure=true
max_restarts=3
EOF
        
        # Create service configuration
        cat > "$service_dir/${service_name}.conf" << EOF
# $service_name Configuration

[${service_name}]
enabled=true
debug=false
log_level=info
log_file=/var/log/${service_name}.log

# Service-specific configuration
# Add service-specific options here
EOF
        
        log_success "$service_name packaged"
    done
}

# Create package documentation
create_package_docs() {
    log_info "Creating package documentation..."
    
    # Main package README
    cat > "$PACKAGE_DIR/README.md" << EOF
# Kosh OS Component Packages

This directory contains packaged components for Kosh OS.

## Structure

- \`kernel/\` - Kernel binary and metadata
- \`drivers/\` - Device drivers with metadata and configuration
- \`userspace/\` - Userspace services and applications
- \`docs/\` - Documentation and installation guides

## Installation

These packages are designed to be included in the Kosh OS ISO image.
The build system automatically integrates these components.

## Component Types

### Kernel
- Core operating system kernel
- Multiboot2 compliant
- Supports x86-64 architecture

### Drivers
- Hardware abstraction layer
- Run in isolated driver space
- Capability-based security model

### Userspace Services
- System services and applications
- Run in user space with restricted privileges
- IPC-based communication

## Build Information

- Build Date: $(date)
- Target Architecture: x86_64
- Kernel Version: 0.1.0
- Package Format: Kosh OS Native

## Usage

These packages are automatically integrated during ISO creation.
Manual installation is not typically required.
EOF

    # Installation guide
    cat > "$PACKAGE_DIR/docs/INSTALLATION.md" << EOF
# Kosh OS Component Installation Guide

## Overview

This guide describes how Kosh OS components are packaged and installed.

## Package Structure

Each component type has a specific structure:

### Kernel Package
\`\`\`
kernel/
├── kosh-kernel          # Kernel binary
└── kernel.meta          # Kernel metadata
\`\`\`

### Driver Package
\`\`\`
drivers/<driver_name>/
├── <driver_name>.ko     # Driver binary
├── driver.meta          # Driver metadata
└── <driver_name>.conf   # Driver configuration
\`\`\`

### Userspace Package
\`\`\`
userspace/<service_name>/
├── <service_name>       # Service binary
├── service.meta         # Service metadata
└── <service_name>.conf  # Service configuration
\`\`\`

## Installation Process

1. **Build Phase**: Components are built using Cargo
2. **Package Phase**: Components are packaged with metadata
3. **Integration Phase**: Packages are integrated into ISO
4. **Boot Phase**: Components are loaded during system boot

## Metadata Format

All components include metadata files with:
- Version information
- Dependencies
- Configuration options
- Runtime requirements

## Configuration

Each component includes a configuration file template.
These can be customized for specific deployments.
EOF

    # Component list
    cat > "$PACKAGE_DIR/docs/COMPONENTS.md" << EOF
# Kosh OS Components

## Kernel Components

### kosh-kernel
- **Type**: Microkernel
- **Architecture**: x86_64
- **Features**: Memory management, process scheduling, IPC
- **Size**: $(stat -c%s "build/x86_64/kosh-kernel" 2>/dev/null || echo "unknown") bytes

## Driver Components

### Graphics Driver
- **Type**: Display driver
- **Hardware**: VGA-compatible displays
- **Features**: Text mode output, basic graphics

### Keyboard Driver
- **Type**: Input driver
- **Hardware**: PS/2 keyboards
- **Features**: Scancode translation, input events

### Storage Driver
- **Type**: Storage driver
- **Hardware**: Basic storage devices
- **Features**: Block device access

### Network Driver
- **Type**: Network driver
- **Hardware**: Network interfaces
- **Features**: Packet transmission/reception

### Touch Driver
- **Type**: Input driver
- **Hardware**: Touch screens
- **Features**: Touch event processing, gesture recognition

## Userspace Components

### init
- **Type**: System service
- **Purpose**: System initialization
- **Features**: Process spawning, service management

### fs-service
- **Type**: File system service
- **Purpose**: File system operations
- **Features**: VFS layer, ext4 support

### driver-manager
- **Type**: System service
- **Purpose**: Driver management
- **Features**: Driver loading, dependency resolution

### shell
- **Type**: User application
- **Purpose**: Command line interface
- **Features**: Command execution, system interaction

## Build Date
$(date)
EOF

    log_success "Package documentation created"
}

# Create package manifest
create_manifest() {
    log_info "Creating package manifest..."
    
    cat > "$PACKAGE_DIR/MANIFEST.json" << EOF
{
  "manifest_version": "1.0",
  "package_name": "kosh-os-components",
  "package_version": "0.1.0",
  "build_date": "$(date -Iseconds)",
  "target_architecture": "x86_64",
  "components": {
    "kernel": {
      "name": "kosh-kernel",
      "version": "0.1.0",
      "type": "kernel",
      "path": "kernel/kosh-kernel",
      "metadata": "kernel/kernel.meta"
    },
    "drivers": [
      {
        "name": "graphics",
        "version": "0.1.0",
        "type": "driver",
        "path": "drivers/graphics/graphics.ko",
        "metadata": "drivers/graphics/driver.meta",
        "config": "drivers/graphics/graphics.conf"
      },
      {
        "name": "keyboard",
        "version": "0.1.0",
        "type": "driver",
        "path": "drivers/keyboard/keyboard.ko",
        "metadata": "drivers/keyboard/driver.meta",
        "config": "drivers/keyboard/keyboard.conf"
      },
      {
        "name": "storage",
        "version": "0.1.0",
        "type": "driver",
        "path": "drivers/storage/storage.ko",
        "metadata": "drivers/storage/driver.meta",
        "config": "drivers/storage/storage.conf"
      },
      {
        "name": "network",
        "version": "0.1.0",
        "type": "driver",
        "path": "drivers/network/network.ko",
        "metadata": "drivers/network/driver.meta",
        "config": "drivers/network/network.conf"
      },
      {
        "name": "touch",
        "version": "0.1.0",
        "type": "driver",
        "path": "drivers/touch/touch.ko",
        "metadata": "drivers/touch/driver.meta",
        "config": "drivers/touch/touch.conf"
      }
    ],
    "userspace": [
      {
        "name": "init",
        "version": "0.1.0",
        "type": "service",
        "path": "userspace/init/init",
        "metadata": "userspace/init/service.meta",
        "config": "userspace/init/init.conf"
      },
      {
        "name": "fs-service",
        "version": "0.1.0",
        "type": "service",
        "path": "userspace/fs-service/fs-service",
        "metadata": "userspace/fs-service/service.meta",
        "config": "userspace/fs-service/fs-service.conf"
      },
      {
        "name": "driver-manager",
        "version": "0.1.0",
        "type": "service",
        "path": "userspace/driver-manager/driver-manager",
        "metadata": "userspace/driver-manager/service.meta",
        "config": "userspace/driver-manager/driver-manager.conf"
      },
      {
        "name": "shell",
        "version": "0.1.0",
        "type": "application",
        "path": "userspace/shell/shell",
        "metadata": "userspace/shell/service.meta",
        "config": "userspace/shell/shell.conf"
      }
    ]
  },
  "checksums": {
    "algorithm": "sha256",
    "files": {}
  }
}
EOF

    log_success "Package manifest created"
}

# Calculate checksums
calculate_checksums() {
    log_info "Calculating checksums..."
    
    if command -v sha256sum &> /dev/null; then
        local checksum_file="$PACKAGE_DIR/CHECKSUMS.sha256"
        
        # Calculate checksums for all files
        find "$PACKAGE_DIR" -type f -not -name "CHECKSUMS.sha256" -not -name "MANIFEST.json" | \
        while read -r file; do
            local relative_path="${file#$PACKAGE_DIR/}"
            sha256sum "$file" | sed "s|$PACKAGE_DIR/||" >> "$checksum_file"
        done
        
        log_success "Checksums calculated and saved to CHECKSUMS.sha256"
    else
        log_warning "sha256sum not available, skipping checksum calculation"
    fi
}

# Main packaging function
main() {
    echo "Kosh OS Component Packaging System"
    echo "=================================="
    
    setup_packaging
    package_kernel
    package_drivers
    package_userspace
    create_package_docs
    create_manifest
    calculate_checksums
    
    # Display summary
    echo ""
    log_success "Component packaging completed!"
    log_info "Package directory: $PACKAGE_DIR/"
    
    # Show package structure
    if command -v tree &> /dev/null; then
        echo ""
        log_info "Package structure:"
        tree "$PACKAGE_DIR"
    else
        echo ""
        log_info "Package contents:"
        find "$PACKAGE_DIR" -type f | sort
    fi
    
    # Show package size
    local package_size=$(du -sh "$PACKAGE_DIR" 2>/dev/null | cut -f1 || echo "unknown")
    log_info "Total package size: $package_size"
}

# Handle interruption
trap 'log_error "Packaging interrupted"; exit 1' INT TERM

# Run main function
main "$@"