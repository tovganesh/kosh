#!/bin/bash

set -e

echo "Testing Kosh OS boot process..."

# Configuration
KERNEL_PATH="build/x86_64/kosh-kernel"
ISO_PATH="kosh.iso"
QEMU_TIMEOUT=30
QEMU_MEMORY="512M"

# Test functions
test_direct_kernel_boot() {
    echo "Testing direct kernel boot with QEMU..."
    
    if [ ! -f "$KERNEL_PATH" ]; then
        echo "Error: Kernel not found. Building first..."
        ./scripts/build.sh
    fi
    
    echo "Starting QEMU with direct kernel boot (timeout: ${QEMU_TIMEOUT}s)..."
    
    # Test basic boot
    timeout $QEMU_TIMEOUT qemu-system-x86_64 \
        -kernel "$KERNEL_PATH" \
        -m "$QEMU_MEMORY" \
        -serial stdio \
        -display none \
        -no-reboot \
        -no-shutdown &
    
    QEMU_PID=$!
    
    # Wait for boot or timeout
    sleep 5
    
    if kill -0 $QEMU_PID 2>/dev/null; then
        echo "✓ Kernel boot successful (QEMU running)"
        kill $QEMU_PID 2>/dev/null || true
        wait $QEMU_PID 2>/dev/null || true
    else
        echo "✗ Kernel boot failed or crashed"
        return 1
    fi
}

test_kernel_boot_with_params() {
    echo "Testing kernel boot with parameters..."
    
    local test_params=(
        "debug=1"
        "log_level=debug"
        "safe_mode=1"
        "debug=1 log_level=info safe_mode=1"
    )
    
    for params in "${test_params[@]}"; do
        echo "Testing with parameters: $params"
        
        timeout 10 qemu-system-x86_64 \
            -kernel "$KERNEL_PATH" \
            -append "$params" \
            -m "$QEMU_MEMORY" \
            -serial stdio \
            -display none \
            -no-reboot \
            -no-shutdown &
        
        QEMU_PID=$!
        sleep 3
        
        if kill -0 $QEMU_PID 2>/dev/null; then
            echo "✓ Boot with '$params' successful"
            kill $QEMU_PID 2>/dev/null || true
            wait $QEMU_PID 2>/dev/null || true
        else
            echo "✗ Boot with '$params' failed"
        fi
    done
}

test_iso_boot() {
    echo "Testing ISO boot..."
    
    if [ ! -f "$ISO_PATH" ]; then
        echo "ISO not found. Building..."
        ./scripts/build-iso.sh
    fi
    
    if [ ! -f "$ISO_PATH" ]; then
        echo "✗ Failed to create ISO"
        return 1
    fi
    
    echo "Testing ISO boot with QEMU..."
    
    timeout $QEMU_TIMEOUT qemu-system-x86_64 \
        -cdrom "$ISO_PATH" \
        -m "$QEMU_MEMORY" \
        -serial stdio \
        -display none \
        -no-reboot \
        -no-shutdown &
    
    QEMU_PID=$!
    sleep 5
    
    if kill -0 $QEMU_PID 2>/dev/null; then
        echo "✓ ISO boot successful"
        kill $QEMU_PID 2>/dev/null || true
        wait $QEMU_PID 2>/dev/null || true
    else
        echo "✗ ISO boot failed"
        return 1
    fi
}

test_grub_menu() {
    echo "Testing GRUB menu functionality..."
    
    if [ ! -f "$ISO_PATH" ]; then
        echo "ISO not found. Building..."
        ./scripts/build-iso.sh
    fi
    
    echo "Starting QEMU with GRUB menu (will timeout after ${QEMU_TIMEOUT}s)..."
    echo "You should see the GRUB menu with multiple boot options"
    
    # Start QEMU with display to show GRUB menu
    timeout $QEMU_TIMEOUT qemu-system-x86_64 \
        -cdrom "$ISO_PATH" \
        -m "$QEMU_MEMORY" \
        -serial stdio \
        -no-reboot \
        -no-shutdown || true
    
    echo "GRUB menu test completed"
}

test_memory_configurations() {
    echo "Testing different memory configurations..."
    
    local memory_configs=("128M" "256M" "512M" "1G")
    
    for mem in "${memory_configs[@]}"; do
        echo "Testing with $mem memory..."
        
        timeout 10 qemu-system-x86_64 \
            -kernel "$KERNEL_PATH" \
            -m "$mem" \
            -serial stdio \
            -display none \
            -no-reboot \
            -no-shutdown &
        
        QEMU_PID=$!
        sleep 3
        
        if kill -0 $QEMU_PID 2>/dev/null; then
            echo "✓ Boot with $mem memory successful"
            kill $QEMU_PID 2>/dev/null || true
            wait $QEMU_PID 2>/dev/null || true
        else
            echo "✗ Boot with $mem memory failed"
        fi
    done
}

# Check dependencies
check_dependencies() {
    echo "Checking dependencies..."
    
    if ! command -v qemu-system-x86_64 &> /dev/null; then
        echo "Error: qemu-system-x86_64 not found"
        echo "Please install QEMU to run boot tests"
        exit 1
    fi
    
    echo "✓ QEMU found"
}

# Main test execution
main() {
    echo "Kosh OS Boot Test Suite"
    echo "======================"
    
    check_dependencies
    
    # Run tests based on arguments
    if [ $# -eq 0 ]; then
        echo "Running all boot tests..."
        test_direct_kernel_boot
        test_kernel_boot_with_params
        test_iso_boot
        test_memory_configurations
    else
        case "$1" in
            "kernel")
                test_direct_kernel_boot
                ;;
            "params")
                test_kernel_boot_with_params
                ;;
            "iso")
                test_iso_boot
                ;;
            "grub")
                test_grub_menu
                ;;
            "memory")
                test_memory_configurations
                ;;
            *)
                echo "Usage: $0 [kernel|params|iso|grub|memory]"
                echo "  kernel  - Test direct kernel boot"
                echo "  params  - Test kernel boot with parameters"
                echo "  iso     - Test ISO boot"
                echo "  grub    - Test GRUB menu (interactive)"
                echo "  memory  - Test different memory configurations"
                echo "  (no args) - Run all tests"
                exit 1
                ;;
        esac
    fi
    
    echo ""
    echo "Boot testing completed!"
}

# Handle script interruption
trap 'echo "Test interrupted"; kill $(jobs -p) 2>/dev/null || true; exit 1' INT TERM

main "$@"