@echo off
REM Kosh Integration Test Suite for Windows
REM This script runs comprehensive integration tests

setlocal enabledelayedexpansion

set SCRIPT_DIR=%~dp0
set PROJECT_ROOT=%SCRIPT_DIR%..
set TEST_RESULTS_DIR=%PROJECT_ROOT%\test-results
set ISO_PATH=%PROJECT_ROOT%\kosh.iso
set QEMU_LOG=%TEST_RESULTS_DIR%\qemu-test.log
set VBOX_LOG=%TEST_RESULTS_DIR%\vbox-test.log

echo === Kosh Integration Test Suite ===

REM Create test results directory
if not exist "%TEST_RESULTS_DIR%" mkdir "%TEST_RESULTS_DIR%"

set total_tests=0
set passed_tests=0

REM Function to log test results
:log_test_result
set test_name=%~1
set result=%~2
set details=%~3

echo %date% %time%: %test_name% - %result% - %details% >> "%TEST_RESULTS_DIR%\integration-test-results.log"

if "%result%"=="PASS" (
    echo ✅ %test_name%: PASSED
) else (
    echo ❌ %test_name%: FAILED
    if not "%details%"=="" echo    Details: %details%
)
goto :eof

REM Test 1: Build system test
echo Running build system tests...
call :test_build_system
if !errorlevel! equ 0 (
    set /a passed_tests+=1
)
set /a total_tests+=1

REM Test 2: QEMU boot test
echo Running QEMU boot tests...
call :test_qemu_boot
if !errorlevel! equ 0 (
    set /a passed_tests+=1
)
set /a total_tests+=1

REM Test 3: File system integrity tests
echo Running file system integrity tests...
call :test_filesystem_integrity
if !errorlevel! equ 0 (
    set /a passed_tests+=1
)
set /a total_tests+=1

REM Generate final report
echo.
echo === Integration Test Results ===
echo Total tests: !total_tests!
echo Passed: !passed_tests!
set /a failed_tests=!total_tests!-!passed_tests!
echo Failed: !failed_tests!

if !passed_tests! equ !total_tests! (
    echo 🎉 All integration tests passed!
    exit /b 0
) else (
    echo ⚠️  Some integration tests failed. Check logs in %TEST_RESULTS_DIR%
    exit /b 1
)

:test_build_system
echo Testing build system...

REM Test kernel build
cd /d "%PROJECT_ROOT%"
call scripts\build.bat > "%TEST_RESULTS_DIR%\build-test.log" 2>&1
if !errorlevel! equ 0 (
    call :log_test_result "Kernel Build" "PASS" "Kernel built successfully"
) else (
    call :log_test_result "Kernel Build" "FAIL" "Kernel build failed - see build-test.log"
    goto :eof
)

REM Test ISO generation
call scripts\build-iso.bat > "%TEST_RESULTS_DIR%\iso-build-test.log" 2>&1
if !errorlevel! equ 0 (
    call :log_test_result "ISO Generation" "PASS" "ISO generated successfully"
) else (
    call :log_test_result "ISO Generation" "FAIL" "ISO generation failed - see iso-build-test.log"
    goto :eof
)

REM Verify ISO file exists
if exist "%ISO_PATH%" (
    call :log_test_result "ISO Validation" "PASS" "ISO file exists"
) else (
    call :log_test_result "ISO Validation" "FAIL" "ISO file not found"
    exit /b 1
)

exit /b 0

:test_qemu_boot
echo Testing QEMU boot...

if not exist "%ISO_PATH%" (
    call :log_test_result "QEMU Boot Test" "FAIL" "ISO file not found"
    exit /b 1
)

REM Check if QEMU is available
qemu-system-x86_64 --version >nul 2>&1
if !errorlevel! neq 0 (
    call :log_test_result "QEMU Boot Test" "SKIP" "QEMU not available"
    exit /b 0
)

REM Run QEMU with timeout (simplified for Windows)
start /b qemu-system-x86_64 -cdrom "%ISO_PATH%" -m 256M -serial file:"%QEMU_LOG%" -display none -no-reboot -device isa-debug-exit,iobase=0xf4,iosize=0x04

REM Wait for QEMU to start and produce output
timeout /t 10 /nobreak >nul

REM Check for successful boot messages in log
if exist "%QEMU_LOG%" (
    findstr /c:"Kosh Kernel Starting" "%QEMU_LOG%" >nul
    if !errorlevel! equ 0 (
        call :log_test_result "QEMU Boot Test" "PASS" "Kernel started successfully in QEMU"
        
        REM Check for additional boot stages
        findstr /c:"initialized successfully" "%QEMU_LOG%" >nul
        if !errorlevel! equ 0 (
            call :log_test_result "QEMU Initialization" "PASS" "Kernel initialization completed"
        ) else (
            call :log_test_result "QEMU Initialization" "PARTIAL" "Kernel started but initialization incomplete"
        )
    ) else (
        call :log_test_result "QEMU Boot Test" "FAIL" "No boot messages found in QEMU log"
        exit /b 1
    )
) else (
    call :log_test_result "QEMU Boot Test" "FAIL" "QEMU log file not created"
    exit /b 1
)

REM Clean up QEMU processes
taskkill /f /im qemu-system-x86_64.exe >nul 2>&1

exit /b 0

:test_filesystem_integrity
echo Testing file system integrity...

REM Test multiboot2 compliance if validator exists
if exist "%PROJECT_ROOT%\scripts\validate-multiboot2.bat" (
    call "%PROJECT_ROOT%\scripts\validate-multiboot2.bat" > "%TEST_RESULTS_DIR%\multiboot2-test.log" 2>&1
    if !errorlevel! equ 0 (
        call :log_test_result "Multiboot2 Compliance" "PASS" "Kernel is multiboot2 compliant"
    ) else (
        call :log_test_result "Multiboot2 Compliance" "FAIL" "Multiboot2 validation failed"
        exit /b 1
    )
) else (
    call :log_test_result "Multiboot2 Compliance" "SKIP" "Multiboot2 validator not available"
)

REM Basic ISO file validation
if exist "%ISO_PATH%" (
    for %%A in ("%ISO_PATH%") do set iso_size=%%~zA
    if !iso_size! gtr 1048576 (
        call :log_test_result "ISO File System" "PASS" "ISO file size: !iso_size! bytes"
    ) else (
        call :log_test_result "ISO File System" "FAIL" "ISO file too small: !iso_size! bytes"
        exit /b 1
    )
) else (
    call :log_test_result "ISO File System" "FAIL" "ISO file not found"
    exit /b 1
)

exit /b 0