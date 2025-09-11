#!/bin/bash

set -e

# Configuration
ISO_NAME="kosh.iso"
ISO_DIR="iso"
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
    
    log_success "ISO directory structure created"
}

# Build the kernel only
build_kernel() {
    log_info "Building Kosh OS kernel..."
    
    # Build the kernel
    cargo build --package kosh-kernel --target x86_64-kosh.json --release -Z build-std=core,alloc
    
    if [ $? -eq 0 ]; then
        log_success "Kernel build completed"
    else
        log_error "Kernel build failed"
        exit 1
    fi
}

# Copy kernel
copy_kernel() {
    log_info "Copying kernel..."
    
    local kernel_path="target/x86_64-kosh/release/$KERNEL_NAME"
    
    if [ ! -f "$kernel_path" ]; then
        log_error "Kernel not found at $kernel_path"
        exit 1
    fi
    
    cp "$kernel_path" "$ISO_DIR/boot/"
    
    # Get kernel size for information
    local kernel_size=$(stat -c%s "$kernel_path" 2>/dev/null || stat -f%z "$kernel_path" 2>/dev/null || echo "unknown")
    log_success "Kernel copied (size: $kernel_size bytes)"
}

# Create GRUB configuration
create_grub_config() {
    log_info "Creating GRUB configuration..."
    
    cat > "$ISO_DIR/boot/grub/grub.cfg" << EOF
set timeout=5
set default=0

menuentry "Kosh Operating System" {
    multiboot2 /boot/$KERNEL_NAME
    boot
}

menuentry "Kosh OS - Debug Mode" {
    multiboot2 /boot/$KERNEL_NAME debug=1
    boot
}

menuentry "Kosh OS - Safe Mode" {
    multiboot2 /boot/$KERNEL_NAME safe_mode=1
    boot
}
EOF

    log_success "GRUB configuration created"
}

# Create documentation
create_docs() {
    log_info "Creating documentation..."
    
    # README for the ISO
    cat > "$ISO_DIR/README.txt" << EOF
Kosh Operating System - Minimal Bootable ISO
============================================

This is a minimal bootable ISO image of Kosh OS, containing just the kernel
for testing the platform abstraction layer and basic OS functionality.

Boot Options:
- Kosh Operating System: Standard boot
- Kosh OS - Debug Mode: Boot with debug output
- Kosh OS - Safe Mode: Boot with minimal features

System Requirements:
- x86-64 compatible processor
- Minimum 64MB RAM (128MB recommended)
- VGA-compatible display

What to Expect:
- Platform abstraction layer initialization
- CPU detection and information display
- Memory management setup
- Basic kernel services initialization

Build Information:
- Build Date: $(date)
- Kernel Version: 0.1.0
- Architecture: x86_64
- Platform Abstraction: Enabled
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
    cat > "test-kosh-qemu.sh" << 'EOF'
#!/bin/bash
echo "Testing Kosh OS ISO with QEMU..."
echo "Expected output: Platform abstraction layer initialization and kernel boot messages"
echo ""
qemu-system-x86_64 \
    -cdrom kosh.iso \
    -m 128M \
    -serial stdio \
    -no-reboot \
    -no-shutdown \
    -display curses
EOF
    chmod +x "test-kosh-qemu.sh"
    
    # VirtualBox test script
    cat > "test-kosh-vbox.sh" << 'EOF'
#!/bin/bash
echo "Testing Kosh OS ISO with VirtualBox..."
echo ""
echo "Manual VirtualBox setup:"
echo "1. Create new VM: 'Kosh OS Test'"
echo "2. Type: Other, Version: Other/Unknown (64-bit)"
echo "3. Memory: 128MB (minimum)"
echo "4. No hard disk needed"
echo "5. Attach kosh.iso as CD/DVD"
echo "6. Boot the VM"
echo ""
echo "Expected output:"
echo "- GRUB menu with Kosh OS options"
echo "- Platform abstraction layer initialization"
echo "- CPU detection and platform information"
echo "- Memory management setup"
echo "- Kernel initialization messages"
EOF
    chmod +x "test-kosh-vbox.sh"
    
    log_success "Test scripts created"
}

# Main function
main() {
    echo "Kosh OS Minimal ISO Creation"
    echo "============================"
    
    check_dependencies
    clean_iso
    create_iso_structure
    build_kernel
    copy_kernel
    create_grub_config
    create_docs
    
    if create_iso; then
        validate_iso
        create_test_scripts
        
        echo ""
        log_success "Minimal ISO creation completed successfully!"
        log_info "Created: $ISO_NAME"
        log_info "Test with QEMU: ./test-kosh-qemu.sh"
        log_info "Test with VirtualBox: ./test-kosh-vbox.sh"
        log_info "Or manually: qemu-system-x86_64 -cdrom $ISO_NAME -m 128M"
        echo ""
        log_info "What to expect when booting:"
        log_info "- GRUB bootloader menu"
        log_info "- Platform abstraction layer initialization"
        log_info "- CPU detection and platform information display"
        log_info "- Memory management setup"
        log_info "- Basic kernel services initialization"
    else
        log_warning "ISO creation failed, but structure is ready"
        log_info "ISO structure available in: $ISO_DIR/"
        log_info "Kernel ready at: $ISO_DIR/boot/$KERNEL_NAME"
    fi
}

# Handle interruption
trap 'log_error "Build interrupted"; exit 1' INT TERM

# Run main function
main "$@"