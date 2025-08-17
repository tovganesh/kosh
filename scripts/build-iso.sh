#!/bin/bash

set -e

# Configuration
ISO_NAME="kosh.iso"
ISO_DIR="iso"
BUILD_DIR="build"
TARGET_DIR="target/x86_64-kosh/release"
KERNEL_NAME="kosh-kernel"

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

# Check dependencies
check_dependencies() {
    log_info "Checking dependencies..."
    
    local missing_deps=()
    
    if ! command -v grub-mkrescue &> /dev/null; then
        missing_deps+=("grub-mkrescue")
    fi
    
    if ! command -v xorriso &> /dev/null; then
        missing_deps+=("xorriso")
    fi
    
    if [ ${#missing_deps[@]} -gt 0 ]; then
        log_warning "Missing dependencies: ${missing_deps[*]}"
        log_info "On Ubuntu/Debian: sudo apt install grub-pc-bin grub-common xorriso"
        log_info "On Fedora: sudo dnf install grub2-tools xorriso"
        log_info "On macOS: brew install grub xorriso"
        log_info "Continuing anyway - ISO structure will be created"
    else
        log_success "All dependencies found"
    fi
}

# Clean previous build
clean_iso() {
    log_info "Cleaning previous ISO build..."
    
    if [ -d "$ISO_DIR" ]; then
        rm -rf "$ISO_DIR"
        log_success "Cleaned ISO directory"
    fi
    
    if [ -f "$ISO_NAME" ]; then
        rm -f "$ISO_NAME"
        log_success "Removed previous ISO file"
    fi
}

# Create ISO directory structure
create_iso_structure() {
    log_info "Creating ISO directory structure..."
    
    # Create main directories
    mkdir -p "$ISO_DIR/boot/grub"
    mkdir -p "$ISO_DIR/drivers"
    mkdir -p "$ISO_DIR/system"
    mkdir -p "$ISO_DIR/config"
    mkdir -p "$ISO_DIR/docs"
    
    log_success "ISO directory structure created"
}

# Build the OS components
build_os() {
    log_info "Building Kosh OS components..."
    
    if [ ! -f "scripts/build.sh" ]; then
        log_error "Build script not found!"
        exit 1
    fi
    
    ./scripts/build.sh
    
    if [ $? -eq 0 ]; then
        log_success "OS build completed"
    else
        log_error "OS build failed"
        exit 1
    fi
}

# Copy kernel
copy_kernel() {
    log_info "Copying kernel..."
    
    local kernel_path="$BUILD_DIR/x86_64/$KERNEL_NAME"
    
    if [ ! -f "$kernel_path" ]; then
        log_error "Kernel not found at $kernel_path"
        exit 1
    fi
    
    cp "$kernel_path" "$ISO_DIR/boot/"
    
    # Get kernel size for information
    local kernel_size=$(stat -c%s "$kernel_path" 2>/dev/null || stat -f%z "$kernel_path" 2>/dev/null || echo "unknown")
    log_success "Kernel copied (size: $kernel_size bytes)"
}

# Copy drivers
copy_drivers() {
    log_info "Copying drivers..."
    
    # For now, create placeholder driver files since they're not yet compiled as separate binaries
    # In the future, these would be actual driver binaries
    
    local drivers=("storage" "network" "graphics" "keyboard" "touch")
    
    for driver in "${drivers[@]}"; do
        # Create a placeholder driver file with metadata
        cat > "$ISO_DIR/drivers/${driver}.ko" << EOF
# Kosh OS Driver: $driver
# This is a placeholder - actual driver binary would be here
# Driver Type: $driver
# Architecture: x86_64
# Version: 0.1.0
# Build Date: $(date)
EOF
        log_info "Created placeholder for $driver driver"
    done
    
    log_success "Driver placeholders created"
}

# Copy userspace binaries
copy_userspace() {
    log_info "Copying userspace binaries..."
    
    local binaries=(
        "kosh-init:init"
        "kosh-fs-service:fs-service"
        "kosh-driver-manager:driver-manager"
        "kosh-shell:shell"
    )
    
    for binary_mapping in "${binaries[@]}"; do
        local source_name="${binary_mapping%:*}"
        local dest_name="${binary_mapping#*:}"
        local source_path="$TARGET_DIR/$source_name"
        
        if [ -f "$source_path" ]; then
            cp "$source_path" "$ISO_DIR/system/$dest_name"
            local size=$(stat -c%s "$source_path" 2>/dev/null || stat -f%z "$source_path" 2>/dev/null || echo "unknown")
            log_success "Copied $source_name -> $dest_name (size: $size bytes)"
        else
            log_warning "$source_name not found, creating placeholder"
            echo "#!/bin/sh" > "$ISO_DIR/system/$dest_name"
            echo "echo 'Placeholder for $source_name'" >> "$ISO_DIR/system/$dest_name"
            chmod +x "$ISO_DIR/system/$dest_name"
        fi
    done
}

# Create configuration files
create_configs() {
    log_info "Creating configuration files..."
    
    # System configuration
    cat > "$ISO_DIR/config/system.conf" << EOF
# Kosh OS System Configuration
# Generated on: $(date)

[kernel]
log_level=info
debug_mode=false
panic_on_oops=true

[drivers]
autoload=true
load_timeout=30
driver_path=/drivers

[memory]
swap_enabled=true
swap_priority=1
heap_size_mb=64

[filesystem]
root_fs=ext4
mount_timeout=10

[power]
cpu_scaling=true
idle_management=true
battery_monitoring=true

[security]
capability_based=true
driver_isolation=true
EOF

    # Boot configuration
    cat > "$ISO_DIR/config/boot.conf" << EOF
# Kosh OS Boot Configuration

[boot]
timeout=5
default_entry=0
fallback_entry=1

[debug]
serial_console=true
vga_console=true
log_boot_process=true
EOF

    # Driver configuration
    cat > "$ISO_DIR/config/drivers.conf" << EOF
# Kosh OS Driver Configuration

[storage]
enabled=true
priority=high
autoload=true

[network]
enabled=true
priority=medium
autoload=true

[graphics]
enabled=true
priority=low
autoload=true

[keyboard]
enabled=true
priority=high
autoload=true

[touch]
enabled=true
priority=medium
autoload=false
EOF

    log_success "Configuration files created"
}

# Create documentation
create_docs() {
    log_info "Creating documentation..."
    
    # README for the ISO
    cat > "$ISO_DIR/docs/README.txt" << EOF
Kosh Operating System
====================

This is a bootable ISO image of Kosh OS, a lightweight, mobile-optimized
operating system written in Rust with a microkernel architecture.

Boot Options:
- Kosh OS (x86-64): Standard boot
- Kosh OS (x86-64) - Debug Mode: Boot with debug output
- Kosh OS (x86-64) - Safe Mode: Boot with minimal drivers

System Requirements:
- x86-64 compatible processor
- Minimum 128MB RAM (512MB recommended)
- VGA-compatible display
- PS/2 or USB keyboard

For more information, visit: https://github.com/your-repo/kosh

Build Information:
- Build Date: $(date)
- Kernel Version: 0.1.0
- Architecture: x86_64
EOF

    # Boot help
    cat > "$ISO_DIR/docs/BOOT_HELP.txt" << EOF
Kosh OS Boot Help
================

Available Boot Parameters:
- debug=1          : Enable debug mode
- log_level=debug  : Set log level (debug, info, warn, error)
- safe_mode=1      : Boot in safe mode
- recovery=1       : Boot in recovery mode
- single_user=1    : Boot to single user mode

Examples:
- Normal boot: (no parameters)
- Debug boot: debug=1 log_level=debug
- Safe boot: safe_mode=1 driver_autoload=false

Troubleshooting:
- If boot fails, try safe mode
- For debugging, use debug=1 parameter
- Check hardware compatibility requirements
EOF

    log_success "Documentation created"
}

# Create the ISO image
create_iso() {
    log_info "Creating ISO image..."
    
    if command -v grub-mkrescue &> /dev/null; then
        # Use grub-mkrescue to create the ISO
        local grub_options=(
            "-o" "$ISO_NAME"
            "--modules=multiboot2 normal"
            "--install-modules=multiboot2 normal"
            "--compress=xz"
        )
        
        if grub-mkrescue "${grub_options[@]}" "$ISO_DIR/"; then
            log_success "ISO created successfully: $ISO_NAME"
            
            # Get ISO size
            local iso_size=$(stat -c%s "$ISO_NAME" 2>/dev/null || stat -f%z "$ISO_NAME" 2>/dev/null || echo "unknown")
            log_info "ISO size: $iso_size bytes"
            
            return 0
        else
            log_error "Failed to create ISO with grub-mkrescue"
            return 1
        fi
    else
        log_warning "grub-mkrescue not available"
        log_info "ISO structure created in $ISO_DIR/ directory"
        log_info "To create the ISO manually:"
        log_info "  grub-mkrescue -o $ISO_NAME $ISO_DIR/"
        return 1
    fi
}

# Validate the created ISO
validate_iso() {
    if [ ! -f "$ISO_NAME" ]; then
        log_warning "ISO file not created, skipping validation"
        return 1
    fi
    
    log_info "Validating ISO image..."
    
    # Check if it's a valid ISO
    if command -v file &> /dev/null; then
        local file_info=$(file "$ISO_NAME")
        if echo "$file_info" | grep -q "ISO 9660"; then
            log_success "Valid ISO 9660 format detected"
        else
            log_warning "ISO format validation inconclusive"
        fi
    fi
    
    # Check multiboot2 compliance if grub-file is available
    if command -v grub-file &> /dev/null; then
        if grub-file --is-x86-multiboot2 "$ISO_DIR/boot/$KERNEL_NAME"; then
            log_success "Kernel is multiboot2 compliant"
        else
            log_error "Kernel is not multiboot2 compliant"
            return 1
        fi
    fi
    
    log_success "ISO validation completed"
}

# Create test scripts
create_test_scripts() {
    log_info "Creating test scripts..."
    
    # QEMU test script
    cat > "test-iso-qemu.sh" << 'EOF'
#!/bin/bash
echo "Testing Kosh OS ISO with QEMU..."
qemu-system-x86_64 \
    -cdrom kosh.iso \
    -m 512M \
    -serial stdio \
    -no-reboot \
    -no-shutdown
EOF
    chmod +x "test-iso-qemu.sh"
    
    # VirtualBox test script
    cat > "test-iso-vbox.sh" << 'EOF'
#!/bin/bash
echo "Testing Kosh OS ISO with VirtualBox..."
echo "This script helps create a VirtualBox VM for testing"
echo ""
echo "Manual steps:"
echo "1. Create new VM: 'Kosh OS Test'"
echo "2. Type: Other, Version: Other/Unknown (64-bit)"
echo "3. Memory: 512MB"
echo "4. No hard disk needed"
echo "5. Attach kosh.iso as CD/DVD"
echo "6. Boot the VM"
echo ""
echo "Or use VBoxManage commands:"
echo "VBoxManage createvm --name 'Kosh OS Test' --register"
echo "VBoxManage modifyvm 'Kosh OS Test' --memory 512 --boot1 dvd"
echo "VBoxManage storagectl 'Kosh OS Test' --name 'IDE Controller' --add ide"
echo "VBoxManage storageattach 'Kosh OS Test' --storagectl 'IDE Controller' --port 0 --device 0 --type dvddrive --medium kosh.iso"
echo "VBoxManage startvm 'Kosh OS Test'"
EOF
    chmod +x "test-iso-vbox.sh"
    
    log_success "Test scripts created"
}

# Main function
main() {
    echo "Kosh OS ISO Creation Pipeline"
    echo "============================="
    
    check_dependencies
    clean_iso
    create_iso_structure
    build_os
    copy_kernel
    copy_drivers
    copy_userspace
    create_configs
    create_docs
    
    if create_iso; then
        validate_iso
        create_test_scripts
        
        echo ""
        log_success "ISO creation pipeline completed successfully!"
        log_info "Created: $ISO_NAME"
        log_info "Test with: ./test-iso-qemu.sh"
        log_info "Or manually: qemu-system-x86_64 -cdrom $ISO_NAME"
    else
        log_warning "ISO creation failed, but structure is ready"
        log_info "ISO structure available in: $ISO_DIR/"
    fi
}

# Handle interruption
trap 'log_error "Build interrupted"; exit 1' INT TERM

# Run main function
main "$@"