#!/usr/bin/env bash
#
# Kosh: build -> ISO -> QEMU, in one command.
#
#   ./scripts/run.sh              build, make an ISO, boot it, stream serial
#   ./scripts/run.sh --debug      same, plus wait for gdb on :1234
#   ./scripts/run.sh --check      boot headless and assert the boot markers
#                                 appear on serial (this is the CI gate)
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET_JSON="x86_64-kosh.json"
TARGET_NAME="x86_64-kosh"
PROFILE="${PROFILE:-release}"
BUILD_DIR="$ROOT/build"
ISO_DIR="$BUILD_DIR/iso"
ISO="$BUILD_DIR/kosh.iso"

MODE="run"
case "${1:-}" in
    --debug) MODE="debug" ;;
    --check) MODE="check" ;;
    "")      ;;
    *)       echo "unknown option: $1" >&2; exit 2 ;;
esac

# --- toolchain sanity -------------------------------------------------------

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "error: '$1' not found. $2" >&2; exit 1; }
}
need cargo       "Install Rust: https://rustup.rs"
need grub-mkrescue "Install GRUB tools (macOS: brew install i686-elf-grub xorriso)"
need xorriso     "Install xorriso (macOS: brew install xorriso)"
need qemu-system-x86_64 "Install QEMU (macOS: brew install qemu)"

# --- build ------------------------------------------------------------------

echo "==> building kernel ($PROFILE)"
CARGO_FLAGS=(--package kosh-kernel --target "$TARGET_JSON" -Z build-std=core,alloc)
[ "$PROFILE" = "release" ] && CARGO_FLAGS+=(--release)

# Nightlies from mid-2025 onwards gate .json target specs behind an unstable
# flag. Older ones reject the flag itself, so probe once and remember.
if cargo build "${CARGO_FLAGS[@]}" -Z json-target-spec --quiet 2>/dev/null; then
    :
else
    cargo build "${CARGO_FLAGS[@]}" -Z json-target-spec 2>&1 | tail -40
    # If the failure was purely "unknown -Z flag", retry without it.
    if cargo build "${CARGO_FLAGS[@]}" -Z json-target-spec 2>&1 | grep -q "unknown.*json-target-spec"; then
        cargo build "${CARGO_FLAGS[@]}"
    fi
fi

KERNEL="$ROOT/target/$TARGET_NAME/$PROFILE/kosh-kernel"
[ -f "$KERNEL" ] || { echo "error: kernel binary not found at $KERNEL" >&2; exit 1; }

# --- validate the multiboot2 header before we waste a boot ------------------

if grub-file --is-x86-multiboot2 "$KERNEL"; then
    echo "==> multiboot2 header OK"
else
    echo "error: kernel is not multiboot2-compliant" >&2
    exit 1
fi

ENTRY=$(readelf -h "$KERNEL" 2>/dev/null | awk '/Entry point/ {print $4}')
echo "==> entry point: $ENTRY"

# --- ISO --------------------------------------------------------------------

echo "==> building ISO"
rm -rf "$ISO_DIR"
mkdir -p "$ISO_DIR/boot/grub"
cp "$KERNEL" "$ISO_DIR/boot/kosh-kernel"

cat > "$ISO_DIR/boot/grub/grub.cfg" <<'EOF'
set timeout=0
set default=0

menuentry "Kosh" {
    multiboot2 /boot/kosh-kernel
    boot
}
EOF

grub-mkrescue -o "$ISO" "$ISO_DIR" >/dev/null 2>&1
echo "==> $ISO ($(du -h "$ISO" | cut -f1))"

# --- run --------------------------------------------------------------------

QEMU_ARGS=(
    -cdrom "$ISO"
    -m 512M
    -no-reboot
    -no-shutdown
    -device isa-debug-exit,iobase=0xf4,iosize=0x04
    -d int,cpu_reset
    -D "$BUILD_DIR/qemu.log"
)

case "$MODE" in
run)
    echo "==> booting (ctrl-a x to quit)"
    exec qemu-system-x86_64 "${QEMU_ARGS[@]}" -serial mon:stdio -display none
    ;;

debug)
    echo "==> booting, gdb stub on localhost:1234 (target remote :1234)"
    exec qemu-system-x86_64 "${QEMU_ARGS[@]}" -serial mon:stdio -display none -s -S
    ;;

check)
    SERIAL="$BUILD_DIR/serial.txt"
    rm -f "$SERIAL"
    echo "==> boot check"
    timeout 30 qemu-system-x86_64 "${QEMU_ARGS[@]}" \
        -serial "file:$SERIAL" -display none >/dev/null 2>&1 || true

    echo "--- serial output ---"
    cat "$SERIAL" || true
    echo "---------------------"

    fail=0
    for marker in \
        "32-bit protected mode entry OK" \
        "long mode OK" \
        "Kosh Kernel Starting" \
        "Kernel initialization complete"
    do
        if grep -qF "$marker" "$SERIAL" 2>/dev/null; then
            echo "  PASS  $marker"
        else
            echo "  FAIL  $marker"
            fail=1
        fi
    done

    if grep -q "Triple fault" "$BUILD_DIR/qemu.log" 2>/dev/null; then
        echo "  FAIL  triple fault in qemu.log"
        fail=1
    fi

    exit $fail
    ;;
esac
