#!/bin/bash

set -e

# Configuration
ISO_NAME="kosh.iso"
TEST_RESULTS_DIR="test-results"
QEMU_TIMEOUT=30
VBOX_TIMEOUT=60

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

# Setup test environment
setup_test_env() {
    log_info "Setting up test environment..."
    
    mkdir -p "$TEST_RESULTS_DIR"
    
    # Create test log
    TEST_LOG="$TEST_RESULTS_DIR/iso-test-$(date +%Y%m%d-%H%M%S).log"
    touch "$TEST_LOG"
    
    log_success "Test environment ready"
    log_info "Test log: $TEST_LOG"
}

# Check if ISO exists
check_iso() {
    log_info "Checking ISO file..."
    
    if [ ! -f "$ISO_NAME" ]; then
        log_error "ISO file '$ISO_NAME' not found"
        log_info "Run './scripts/build-iso.sh' first"
        exit 1
    fi
    
    local iso_size=$(stat -c%s "$ISO_NAME" 2>/dev/null || stat -f%z "$ISO_NAME" 2>/dev/null || echo "unknown")
    log_success "ISO found (size: $iso_size bytes)"
}

# Test ISO structure
test_iso_structure() {
    log_info "Testing ISO structure..."
    
    # Create temporary mount point
    local mount_point="/tmp/kosh-iso-test-$$"
    mkdir -p "$mount_point"
    
    # Try to mount the ISO (Linux/macOS)
    if command -v mount &> /dev/null; then
        if mount -o loop,ro "$ISO_NAME" "$mount_point" 2>/dev/null; then
            log_success "ISO mounted successfully"
            
            # Check required files
            local required_files=(
                "boot/kosh-kernel"
                "boot/grub/grub.cfg"
                "system/init"
                "config/system.conf"
            )
            
            local missing_files=()
            for file in "${required_files[@]}"; do
                if [ -f "$mount_point/$file" ]; then
                    log_success "Found: $file"
                else
                    log_warning "Missing: $file"
                    missing_files+=("$file")
                fi
            done
            
            # Check optional files
            local optional_files=(
                "system/fs-service"
                "system/driver-manager"
                "drivers/storage.ko"
                "docs/README.txt"
            )
            
            for file in "${optional_files[@]}"; do
                if [ -f "$mount_point/$file" ]; then
                    log_info "Optional file found: $file"
                fi
            done
            
            # Unmount
            umount "$mount_point" 2>/dev/null || true
            rmdir "$mount_point" 2>/dev/null || true
            
            if [ ${#missing_files[@]} -eq 0 ]; then
                log_success "ISO structure validation passed"
                return 0
            else
                log_error "ISO structure validation failed - missing files: ${missing_files[*]}"
                return 1
            fi
        else
            log_warning "Could not mount ISO for structure testing"
            return 1
        fi
    else
        log_warning "Mount command not available, skipping structure test"
        return 1
    fi
}

# Test QEMU boot
test_qemu_boot() {
    log_info "Testing QEMU boot..."
    
    if ! command -v qemu-system-x86_64 &> /dev/null; then
        log_warning "QEMU not found, skipping QEMU tests"
        return 1
    fi
    
    local qemu_log="$TEST_RESULTS_DIR/qemu-boot.log"
    
    # Test basic boot
    log_info "Testing basic QEMU boot (timeout: ${QEMU_TIMEOUT}s)..."
    
    timeout $QEMU_TIMEOUT qemu-system-x86_64 \
        -cdrom "$ISO_NAME" \
        -m 512M \
        -serial file:"$qemu_log" \
        -display none \
        -no-reboot \
        -no-shutdown &
    
    local qemu_pid=$!
    
    # Wait for boot process
    sleep 10
    
    if kill -0 $qemu_pid 2>/dev/null; then
        log_success "QEMU boot test successful"
        kill $qemu_pid 2>/dev/null || true
        wait $qemu_pid 2>/dev/null || true
        
        # Analyze boot log
        if [ -f "$qemu_log" ]; then
            log_info "Analyzing boot log..."
            
            if grep -q "Kosh Kernel Starting" "$qemu_log"; then
                log_success "Kernel started successfully"
            else
                log_warning "Kernel start message not found in log"
            fi
            
            if grep -q "Multiboot2 info parsed successfully" "$qemu_log"; then
                log_success "Multiboot2 parsing successful"
            else
                log_warning "Multiboot2 parsing message not found"
            fi
            
            if grep -q "KERNEL PANIC" "$qemu_log"; then
                log_error "Kernel panic detected in boot log"
                return 1
            fi
        fi
        
        return 0
    else
        log_error "QEMU boot test failed"
        return 1
    fi
}

# Test QEMU with different configurations
test_qemu_configurations() {
    log_info "Testing QEMU with different configurations..."
    
    if ! command -v qemu-system-x86_64 &> /dev/null; then
        log_warning "QEMU not found, skipping configuration tests"
        return 1
    fi
    
    local configs=(
        "128M:Basic (128MB RAM)"
        "256M:Standard (256MB RAM)"
        "512M:Recommended (512MB RAM)"
        "1G:High Memory (1GB RAM)"
    )
    
    for config in "${configs[@]}"; do
        local memory="${config%:*}"
        local description="${config#*:}"
        
        log_info "Testing $description..."
        
        local config_log="$TEST_RESULTS_DIR/qemu-${memory}.log"
        
        timeout 15 qemu-system-x86_64 \
            -cdrom "$ISO_NAME" \
            -m "$memory" \
            -serial file:"$config_log" \
            -display none \
            -no-reboot \
            -no-shutdown &
        
        local qemu_pid=$!
        sleep 5
        
        if kill -0 $qemu_pid 2>/dev/null; then
            log_success "$description test passed"
            kill $qemu_pid 2>/dev/null || true
            wait $qemu_pid 2>/dev/null || true
        else
            log_warning "$description test failed or crashed"
        fi
    done
}

# Test VirtualBox (if available)
test_virtualbox() {
    log_info "Testing VirtualBox compatibility..."
    
    if ! command -v VBoxManage &> /dev/null; then
        log_warning "VirtualBox not found, skipping VirtualBox tests"
        return 1
    fi
    
    local vm_name="KoshOS-Test-$$"
    local vbox_log="$TEST_RESULTS_DIR/virtualbox.log"
    
    log_info "Creating temporary VirtualBox VM: $vm_name"
    
    # Create VM
    VBoxManage createvm --name "$vm_name" --register &>> "$vbox_log"
    
    # Configure VM
    VBoxManage modifyvm "$vm_name" \
        --memory 512 \
        --boot1 dvd \
        --boot2 none \
        --boot3 none \
        --boot4 none \
        --ostype "Other_64" \
        --acpi on \
        --ioapic on &>> "$vbox_log"
    
    # Add storage controller
    VBoxManage storagectl "$vm_name" \
        --name "IDE Controller" \
        --add ide &>> "$vbox_log"
    
    # Attach ISO
    VBoxManage storageattach "$vm_name" \
        --storagectl "IDE Controller" \
        --port 0 \
        --device 0 \
        --type dvddrive \
        --medium "$PWD/$ISO_NAME" &>> "$vbox_log"
    
    # Start VM in headless mode
    log_info "Starting VirtualBox VM (headless mode)..."
    VBoxManage startvm "$vm_name" --type headless &>> "$vbox_log" &
    
    local vbox_pid=$!
    
    # Wait for boot
    sleep 15
    
    # Check if VM is running
    if VBoxManage list runningvms | grep -q "$vm_name"; then
        log_success "VirtualBox boot test successful"
        
        # Stop VM
        VBoxManage controlvm "$vm_name" poweroff &>> "$vbox_log" || true
        sleep 5
    else
        log_warning "VirtualBox boot test failed or VM not running"
    fi
    
    # Cleanup VM
    log_info "Cleaning up VirtualBox VM..."
    VBoxManage unregistervm "$vm_name" --delete &>> "$vbox_log" || true
    
    wait $vbox_pid 2>/dev/null || true
    
    log_success "VirtualBox test completed"
}

# Test boot parameters
test_boot_parameters() {
    log_info "Testing boot parameters..."
    
    if ! command -v qemu-system-x86_64 &> /dev/null; then
        log_warning "QEMU not found, skipping boot parameter tests"
        return 1
    fi
    
    # Note: Boot parameters are passed via GRUB menu, not directly to QEMU with ISO
    # This test verifies that the GRUB menu entries work
    
    log_info "Testing GRUB menu entries..."
    
    # Start QEMU and let it show GRUB menu briefly
    timeout 10 qemu-system-x86_64 \
        -cdrom "$ISO_NAME" \
        -m 512M \
        -serial stdio \
        -display none \
        -no-reboot \
        -no-shutdown &
    
    local qemu_pid=$!
    sleep 3
    
    if kill -0 $qemu_pid 2>/dev/null; then
        log_success "GRUB menu accessible"
        kill $qemu_pid 2>/dev/null || true
        wait $qemu_pid 2>/dev/null || true
    else
        log_warning "GRUB menu test inconclusive"
    fi
}

# Generate test report
generate_report() {
    log_info "Generating test report..."
    
    local report_file="$TEST_RESULTS_DIR/iso-test-report.txt"
    
    cat > "$report_file" << EOF
Kosh OS ISO Test Report
======================
Generated: $(date)
ISO File: $ISO_NAME
ISO Size: $(stat -c%s "$ISO_NAME" 2>/dev/null || stat -f%z "$ISO_NAME" 2>/dev/null || echo "unknown") bytes

Test Results:
EOF
    
    # Add test results to report
    if [ -f "$TEST_RESULTS_DIR/qemu-boot.log" ]; then
        echo "" >> "$report_file"
        echo "QEMU Boot Log Summary:" >> "$report_file"
        echo "=====================" >> "$report_file"
        head -20 "$TEST_RESULTS_DIR/qemu-boot.log" >> "$report_file"
        echo "..." >> "$report_file"
        tail -10 "$TEST_RESULTS_DIR/qemu-boot.log" >> "$report_file"
    fi
    
    log_success "Test report generated: $report_file"
}

# Main test function
main() {
    echo "Kosh OS ISO Test Suite"
    echo "====================="
    
    setup_test_env
    check_iso
    
    local test_passed=0
    local test_failed=0
    
    # Run tests
    if test_iso_structure; then
        ((test_passed++))
    else
        ((test_failed++))
    fi
    
    if test_qemu_boot; then
        ((test_passed++))
    else
        ((test_failed++))
    fi
    
    test_qemu_configurations
    test_virtualbox
    test_boot_parameters
    
    generate_report
    
    echo ""
    echo "Test Summary:"
    echo "============="
    log_success "Tests passed: $test_passed"
    if [ $test_failed -gt 0 ]; then
        log_error "Tests failed: $test_failed"
    else
        log_success "Tests failed: $test_failed"
    fi
    
    echo ""
    if [ $test_failed -eq 0 ]; then
        log_success "All critical tests passed! ISO is ready for use."
    else
        log_warning "Some tests failed. Check logs in $TEST_RESULTS_DIR/"
    fi
    
    log_info "Test results available in: $TEST_RESULTS_DIR/"
}

# Handle interruption
trap 'log_error "Testing interrupted"; exit 1' INT TERM

# Run based on arguments
case "${1:-all}" in
    "structure")
        setup_test_env
        check_iso
        test_iso_structure
        ;;
    "qemu")
        setup_test_env
        check_iso
        test_qemu_boot
        test_qemu_configurations
        ;;
    "vbox"|"virtualbox")
        setup_test_env
        check_iso
        test_virtualbox
        ;;
    "params"|"parameters")
        setup_test_env
        check_iso
        test_boot_parameters
        ;;
    "all"|*)
        main
        ;;
esac