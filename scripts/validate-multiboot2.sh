#!/bin/bash

set -e

echo "Validating Kosh kernel multiboot2 compliance..."

KERNEL_PATH="build/x86_64/kosh-kernel"

# Check if kernel exists
if [ ! -f "$KERNEL_PATH" ]; then
    echo "Error: Kernel not found at $KERNEL_PATH"
    echo "Please run ./scripts/build.sh first"
    exit 1
fi

# Check for multiboot2 magic number
echo "Checking multiboot2 magic number..."
if command -v hexdump &> /dev/null; then
    # Look for multiboot2 magic number (0xE85250D6) in little-endian format
    if hexdump -C "$KERNEL_PATH" | grep -q "d6 50 52 e8"; then
        echo "✓ Multiboot2 magic number found"
    else
        echo "✗ Multiboot2 magic number not found"
        exit 1
    fi
else
    echo "Warning: hexdump not available, skipping magic number check"
fi

# Check multiboot2 header structure using objdump if available
if command -v objdump &> /dev/null; then
    echo "Analyzing multiboot2 header structure..."
    
    # Check if .multiboot2 section exists
    if objdump -h "$KERNEL_PATH" | grep -q ".multiboot2"; then
        echo "✓ .multiboot2 section found"
        
        # Display section details
        echo "Multiboot2 section details:"
        objdump -h "$KERNEL_PATH" | grep -A1 -B1 ".multiboot2"
    else
        echo "✗ .multiboot2 section not found"
        exit 1
    fi
    
    # Check entry point
    echo "Checking entry point..."
    ENTRY_POINT=$(objdump -f "$KERNEL_PATH" | grep "start address" | awk '{print $3}')
    echo "Entry point: $ENTRY_POINT"
    
    if [ "$ENTRY_POINT" = "0x00100000" ]; then
        echo "✓ Entry point is at expected address (1MB)"
    else
        echo "Warning: Entry point is not at 1MB boundary"
    fi
else
    echo "Warning: objdump not available, skipping header structure analysis"
fi

# Check kernel size
KERNEL_SIZE=$(stat -c%s "$KERNEL_PATH" 2>/dev/null || stat -f%z "$KERNEL_PATH" 2>/dev/null || echo "unknown")
echo "Kernel size: $KERNEL_SIZE bytes"

if [ "$KERNEL_SIZE" != "unknown" ] && [ "$KERNEL_SIZE" -gt 1048576 ]; then
    echo "✓ Kernel size is reasonable (> 1MB)"
else
    echo "Warning: Kernel size seems small"
fi

# Validate ELF format
if command -v file &> /dev/null; then
    echo "Checking ELF format..."
    FILE_INFO=$(file "$KERNEL_PATH")
    echo "File info: $FILE_INFO"
    
    if echo "$FILE_INFO" | grep -q "ELF.*x86-64"; then
        echo "✓ Valid x86-64 ELF format"
    else
        echo "✗ Invalid or unexpected file format"
        exit 1
    fi
fi

# Check for required symbols
if command -v nm &> /dev/null; then
    echo "Checking for required symbols..."
    
    if nm "$KERNEL_PATH" | grep -q "_start"; then
        echo "✓ _start symbol found"
    else
        echo "✗ _start symbol not found"
        exit 1
    fi
    
    if nm "$KERNEL_PATH" | grep -q "MULTIBOOT2_HEADER"; then
        echo "✓ MULTIBOOT2_HEADER symbol found"
    else
        echo "Warning: MULTIBOOT2_HEADER symbol not found (may be optimized out)"
    fi
fi

# Test with GRUB if available
if command -v grub-file &> /dev/null; then
    echo "Testing multiboot2 compliance with GRUB..."
    
    if grub-file --is-x86-multiboot2 "$KERNEL_PATH"; then
        echo "✓ GRUB confirms multiboot2 compliance"
    else
        echo "✗ GRUB reports multiboot2 non-compliance"
        exit 1
    fi
else
    echo "Warning: grub-file not available, skipping GRUB validation"
fi

echo ""
echo "Multiboot2 validation completed successfully!"
echo "The kernel appears to be properly configured for multiboot2 booting."

# Display recommendations
echo ""
echo "Recommendations:"
echo "- Test boot with: qemu-system-x86_64 -kernel $KERNEL_PATH"
echo "- Create ISO with: ./scripts/build-iso.sh"
echo "- Test ISO with: qemu-system-x86_64 -cdrom kosh.iso"