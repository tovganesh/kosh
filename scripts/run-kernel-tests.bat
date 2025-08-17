@echo off
REM Kosh Kernel Test Runner for Windows
REM This script runs the comprehensive kernel test suite

echo === Kosh Kernel Test Suite ===
echo Building and running kernel tests...

REM Change to kernel directory
cd /d "%~dp0\..\kernel"

REM Build kernel with test features
echo Building kernel with test configuration...
cargo build --target x86_64-kosh.json

REM Check if build succeeded
if %ERRORLEVEL% neq 0 (
    echo ❌ Build failed!
    exit /b 1
)

REM Run tests in QEMU (if available)
echo Running tests in QEMU...
qemu-system-x86_64 ^
    -drive format=raw,file=target/x86_64-kosh/debug/bootimage-kosh-kernel.bin ^
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 ^
    -serial stdio ^
    -display none ^
    -no-reboot ^
    -no-shutdown

REM Check exit code
if %ERRORLEVEL% equ 33 (
    echo ✅ All tests passed!
    exit /b 0
) else (
    echo ❌ Tests failed with exit code %ERRORLEVEL%
    exit /b 1
)