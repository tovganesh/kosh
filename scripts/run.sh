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
DISK="$BUILD_DIR/disk.img"

MODE="run"
case "${1:-}" in
    --debug)     MODE="debug" ;;
    --check)     MODE="check" ;;
    --check-cli) MODE="check-cli" ;;
    "")          ;;
    *)           echo "unknown option: $1" >&2; exit 2 ;;
esac

# --- toolchain sanity -------------------------------------------------------

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "error: '$1' not found. $2" >&2; exit 1; }
}
need cargo       "Install Rust: https://rustup.rs"
need grub-mkrescue "Install GRUB tools (macOS: brew install i686-elf-grub xorriso)"
need xorriso     "Install xorriso (macOS: brew install xorriso)"
need qemu-system-x86_64 "Install QEMU (macOS: brew install qemu)"
need mkfs.vfat  "Install dosfstools (macOS: brew install dosfstools)"
need mcopy      "Install mtools (macOS: brew install mtools)"

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

# Userspace programs shipped as GRUB modules. These are ordinary static ELF
# binaries; the kernel parses and maps them at run time.
echo "==> building userspace programs"
# compiler-builtins-mem gives these binaries memcpy/memset/memcmp. There is no
# libc, and anything that moves a String around needs them.
USER_STD=(-Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem)

for pkg in kosh-hello kosh-hello2 kosh-ata-driver kosh-shell; do
    USER_FLAGS=(--package "$pkg" --target "$TARGET_JSON" "${USER_STD[@]}")
    [ "$PROFILE" = "release" ] && USER_FLAGS+=(--release)
    cargo build "${USER_FLAGS[@]}" -Z json-target-spec 2>/dev/null \
        || cargo build "${USER_FLAGS[@]}"
done

HELLO="$ROOT/target/$TARGET_NAME/$PROFILE/kosh-hello"
HELLO2="$ROOT/target/$TARGET_NAME/$PROFILE/kosh-hello2"
ATADRV="$ROOT/target/$TARGET_NAME/$PROFILE/kosh-ata-driver"
KSH="$ROOT/target/$TARGET_NAME/$PROFILE/ksh"
for bin in "$HELLO" "$HELLO2" "$ATADRV" "$KSH"; do
    [ -f "$bin" ] || { echo "error: userspace binary not found at $bin" >&2; exit 1; }
    echo "==> $(basename "$bin"): entry $(readelf -h "$bin" | awk '/Entry point/ {print $4}')"
done

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
cp "$HELLO" "$ISO_DIR/boot/hello"
cp "$HELLO2" "$ISO_DIR/boot/hello2"
cp "$ATADRV" "$ISO_DIR/boot/ata-driver"
cp "$KSH"   "$ISO_DIR/boot/ksh"

cat > "$ISO_DIR/boot/grub/grub.cfg" <<'EOF'
# Put GRUB's own output on COM1 as well as the screen. A kernel that GRUB
# refuses to load produces no serial output at all, which looks exactly like a
# kernel that hung on its first instruction — and the message that tells the two
# apart ("error: entry point isn't in a segment") only ever went to the VGA
# console, which --check does not capture.
serial --unit=0 --speed=38400 --word=8 --parity=no --stop=1
terminal_output serial console
terminal_input serial console

set timeout=0
set default=0

menuentry "Kosh" {
    multiboot2 /boot/kosh-kernel
    module2 /boot/hello hello
    module2 /boot/hello2 hello2
    module2 /boot/ata-driver ata-driver
    module2 /boot/ksh ksh
    boot
}
EOF

grub-mkrescue -o "$ISO" "$ISO_DIR" >/dev/null 2>&1
echo "==> $ISO ($(du -h "$ISO" | cut -f1))"

# --- test disk ---------------------------------------------------------------
#
# A FAT32 image with known contents, attached as a legacy IDE hard disk. FAT32
# rather than ext4 on purpose: it is a few hundred lines to read, and the image
# can be mounted on the host to check what the kernel says against what is
# actually there.

if [ ! -f "$DISK" ] || [ "${REBUILD_DISK:-0}" = "1" ]; then
    echo "==> building FAT32 test disk"
    STAGE="$(mktemp -d)"

    cat > "$STAGE/README.TXT" <<'TXT'
Kosh test disk.

This file lives in a FAT32 filesystem on a virtual IDE disk. If you are reading
it from the kosh> prompt, then the ATA driver, the block layer and the FAT32
implementation all work.
TXT

    printf 'hello from the filesystem
' > "$STAGE/HELLO.TXT"
    printf 'line one
line two
line three
' > "$STAGE/LINES.TXT"
    # A file that spans more than one cluster, to exercise the FAT chain walk.
    for i in $(seq 1 400); do
        printf 'This is line %03d of a deliberately multi-cluster file.
' "$i"
    done > "$STAGE/BIG.TXT"

    dd if=/dev/zero of="$DISK" bs=1M count=64 status=none
    mkfs.vfat -F 32 -n KOSHDISK "$DISK" >/dev/null

    mmd   -i "$DISK" ::/docs
    mcopy -i "$DISK" "$STAGE/README.TXT" ::/README.TXT
    mcopy -i "$DISK" "$STAGE/HELLO.TXT"  ::/HELLO.TXT
    mcopy -i "$DISK" "$STAGE/LINES.TXT"  ::/LINES.TXT
    mcopy -i "$DISK" "$STAGE/BIG.TXT"    ::/BIG.TXT
    mcopy -i "$DISK" "$STAGE/HELLO.TXT"  ::/docs/NOTES.TXT
    # A name that does not fit 8.3, so FAT has to store it as long-filename
    # entries. Without this the LFN assembly code is never exercised — every
    # other name here is a valid short name.
    mcopy -i "$DISK" "$STAGE/HELLO.TXT"  "::/A Long File Name.txt"

    rm -rf "$STAGE"
    echo "==> $DISK ($(du -h "$DISK" | cut -f1)), label KOSHDISK"
else
    echo "==> reusing $DISK (REBUILD_DISK=1 to regenerate)"
fi

# --- run --------------------------------------------------------------------

QEMU_ARGS=(
    -cdrom "$ISO"
    # Primary IDE master, so it answers on the legacy ports at 0x1F0. The CD is
    # on the secondary channel, which is why the driver finds the disk and not
    # the ISO.
    -drive "file=$DISK,format=raw,if=ide,index=0,media=disk"
    # Boot the CD, not the disk. mkfs.vfat writes a 0xAA55 signature at offset
    # 510, so SeaBIOS considers the test disk bootable and will happily try it
    # first — which hangs, with no output, looking exactly like a kernel that
    # failed to start.
    -boot order=d
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
    timeout 20 qemu-system-x86_64 "${QEMU_ARGS[@]}" \
        -serial "file:$SERIAL" -display none >/dev/null 2>&1 || true

    echo "--- serial output (last 30 lines) ---"
    tail -30 "$SERIAL" || true
    echo "---------------------"

    fail=0
    for marker in \
        "32-bit protected mode entry OK" \
        "long mode OK" \
        "Kosh Kernel Starting" \
        "IDT installed" \
        "returned from int3 handler" \
        "PIC remapped" \
        "PIT channel 0" \
        "Interrupts enabled" \
        "Timer: PASS" \
        "kernel page tables active" \
        "physmap aliases identity map: OK" \
        "page 0 unmapped: OK" \
        "kernel out of PML4[0]: OK" \
        "Address spaces: PASS" \
        "PASS: heap fully reclaimed" \
        "Scheduler: PASS" \
        "hello from ring 3" \
        "kernel rejected my out-of-bounds pointer" \
        "ring 3 fault — terminating the process" \
        "Ring 3: PASS" \
        "kernel survived a ring 3 fault" \
        "A: survived 12 yields inside a syscall" \
        "B: survived 12 yields inside a syscall" \
        "Concurrent ring 3: PASS" \
        "hello from a loaded ELF binary" \
        ".bss was zeroed correctly" \
        "stack is writable and readable" \
        "mmap gave me a usable megabyte" \
        "Demand paging: " \
        "munmap returned the pages" \
        "recycled pages came back zeroed" \
        "CLOCK_MONOTONIC moves forwards" \
        "debug_print echoed my message" \
        "I/O bitmap at TSS+" \
        "ata-driver: got the ata0 ports" \
        "the kernel's own ATA driver now refuses ata0" \
        "IDENTIFY succeeded from ring 3" \
        "a FAT32 boot sector, signature and all" \
        "a second sector read back different bytes" \
        "refused a read past the end of the disk" \
        "driver exited and gave ata0 back" \
        "killed for touching its ports" \
        "child: I inherited the value my parent set before forking" \
        "child: my copy of the witness is mine" \
        "parent: still writable after the child exited" \
        "message, bytes and all" \
        "sending to a pid that does not exist was refused" \
        "Copy-on-write: " \
        "hello2 here: exec replaced the whole image" \
        "hello2: my .bss is mine and it is zero" \
        "parent: the child did not touch my memory" \
        "and exited 7" \
        "capability check not exercised" \
        "seconds since the epoch" \
        "ELF loader: PASS" \
        "Storage: PASS" \
        "Filesystem: PASS" \
        "Kernel initialization complete" \
        "supervisor started on its own thread" \
        "starting the userspace shell" \
        "ksh: the Kosh shell, in ring 3"
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

check-cli)
    # Boot, then type at the console through QEMU's monitor and check what it
    # answers. Nothing else in this repo exercises the keyboard IRQ, the line
    # editor and the command table end to end — and a console that only a human
    # can test is a console that silently rots.
    SERIAL="$BUILD_DIR/serial-cli.txt"
    rm -f "$SERIAL"
    echo "==> console check"

    # QEMU's `sendkey` speaks key names, not characters.
    keyname() {
        case "$1" in
            " ") echo "spc" ;;
            ".") echo "dot" ;;
            "-") echo "minus" ;;
            "/") echo "slash" ;;
            ",") echo "comma" ;;
            # Shifted punctuation. Note there is no working name for '|' on this
            # layout: QEMU accepts `shift-backslash` but the guest sees no
            # character, which is why the pipe path is tested by `parsetest`
            # rather than by typing one.
            ">") echo "shift-dot" ;;
            "<") echo "shift-comma" ;;
            "&") echo "shift-7" ;;
            "_") echo "shift-minus" ;;
            [a-z0-9]) echo "$1" ;;
            # Anything unmapped is skipped rather than silently mistyped — but
            # that silence is itself a trap, so say so.
            *) echo "" ;;
        esac
    }

    type_line() {
        local text="$1" i c k
        for (( i=0; i<${#text}; i++ )); do
            c="${text:$i:1}"
            k="$(keyname "$c")"
            [ -n "$k" ] && echo "sendkey $k"
            # Deliberately unhurried. QEMU drops sendkeys if you push them
            # faster than the guest drains the PS/2 controller, and a flaky
            # console test is worse than a slow one.
            sleep 0.04
        done
        sleep 0.2
        echo "sendkey ret"
        sleep 0.8
    }

    {
        # Let the boot demos finish before typing.
        sleep 14

        # --- the userspace shell, in ring 3 ---
        type_line "help"
        type_line "getpid"
        type_line "date"
        type_line "pwd"
        type_line "ls"
        type_line "cat hello.txt"
        type_line "cd docs"
        type_line "pwd"
        type_line "ls"
        type_line "cd .."
        type_line "echo hello from ring 3 shell"
        type_line "stat big.txt"
        type_line "cat nope.txt"
        # An unknown command is now a spawn attempt, so this also proves ENOENT
        # from spawn still reads as "command not found" and nothing else does.
        type_line "nosuchcommand"
        # The real thing: the shell launches a separate program, blocks in the
        # kernel's wait, and gets its prompt back when the child exits. Twice,
        # because running a program a second time is what exercises the teardown
        # of the first one's mappings.
        type_line "hello"
        type_line "hello"
        # ksh inside ksh. This was *refused* until per-process address spaces:
        # both are linked at 8 MiB, so a second copy in one address space would
        # have overwritten the .text the first one was executing. Now each has
        # its own PML4 and the same address means different memory.
        type_line "ksh"
        type_line "getpid"
        type_line "exit"
        type_line "getpid"
        # `cmd &` was refused as "needs fork" for three phases; what it actually
        # needed was to start a program and not block, which spawn already did.
        type_line "hello2 &"
        # The shell must refuse redirection rather than silently dropping it.
        type_line "echo a > out.txt"
        # QEMU's sendkey cannot produce a '|' through this keyboard layout, so
        # the pipe path is checked by running the tokenizer directly instead of
        # by typing one.
        type_line "parsetest"
        type_line "history"

        # Leaving ksh must hand the console back to the kernel, which is both a
        # feature and the only way to test both shells in one session.
        type_line "exit"
        sleep 2

        # --- the in-kernel debug console ---
        type_line "uname"
        type_line "mem"
        type_line "ps"
        type_line "lsblk"
        type_line "df"
        sleep 1
        echo quit
    # 240s, not 180: the session now also spawns a disk driver twice and waits
    # for it to identify a drive and serve four requests each time.
    } | timeout 240 qemu-system-x86_64 "${QEMU_ARGS[@]}" \
            -serial "file:$SERIAL" -display none -monitor stdio >/dev/null 2>&1 || true

    echo "--- console session ---"
    # `|| true` because `head` closing the pipe sends SIGPIPE to `sed`, and
    # `set -o pipefail` turns that into a failed script — a failure mode that
    # only appears once the log grows past the line limit, which is to say on
    # the day you add output rather than the day you write the line.
    { sed -n '/ksh: the Kosh shell/,$p' "$SERIAL" 2>/dev/null | head -110; } || true
    echo "-----------------------"

    fail=0
    for marker in \
        "ksh: the Kosh shell, in ring 3" \
        "ksh:/\$" \
        "ksh - the Kosh shell, running in ring 3" \
        "hello from the filesystem" \
        "README.TXT" \
        "ksh:/docs\$" \
        "NOTES.TXT" \
        "hello from ring 3 shell" \
        "type  file" \
        " UTC" \
        "cat: /nope.txt: no such file" \
        "nosuchcommand: command not found" \
        "spawn 'hello': thread" \
        "hello from a loaded ELF binary" \
        "released 'hello'" \
        "Loading 'ksh' into a new address space" \
        "spawn 'ksh': thread" \
        "address space of thread" \
        "redirection needs a writable filesystem" \
        "pipe yes" \
        "redirect out" \
        "background" \
        "hello2 here: exec replaced the whole image" \
        "child: messaging my grandparent was refused" \
        "the ring-3 driver identified a disk of" \
        "read LBA 0 in ring 3: a FAT32 boot sector" \
        "killed for touching its ports" \
        "ksh: exiting" \
        "ksh exited with code 0, falling back to the kernel console" \
        "Kosh console" \
        "Kosh 0.1.0 x86_64" \
        "physical memory:" \
        "context switches since boot" \
        "QEMU HARDDISK" \
        "FAT32 'KOSHDISK'"
    do
        if grep -qF "$(printf '%b' "$marker")" "$SERIAL" 2>/dev/null; then
            echo "  PASS  $marker"
        else
            echo "  FAIL  $marker"
            fail=1
        fi
    done

    exit $fail
    ;;
esac
