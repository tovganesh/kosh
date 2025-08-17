use crate::process::ProcessId;
use crate::syscall::{dispatch_syscall, SyscallError, SYS_GETPID, SYS_TIME};

/// Test basic system call functionality
pub fn test_syscall_basic() {
    let test_pid = ProcessId::new(42);
    let args = [0; 6];
    
    // Test getpid system call
    match dispatch_syscall(test_pid, SYS_GETPID, args) {
        Ok(result) => {
            assert_eq!(result, 42);
            crate::serial_println!("✓ getpid syscall test passed: returned {}", result);
        }
        Err(e) => {
            crate::serial_println!("✗ getpid syscall test failed: {:?}", e);
            panic!("getpid syscall test failed");
        }
    }
    
    // Test time system call
    match dispatch_syscall(test_pid, SYS_TIME, args) {
        Ok(result) => {
            crate::serial_println!("✓ time syscall test passed: returned {}", result);
        }
        Err(e) => {
            crate::serial_println!("✗ time syscall test failed: {:?}", e);
            panic!("time syscall test failed");
        }
    }
    
    // Test invalid system call
    match dispatch_syscall(test_pid, 999, args) {
        Ok(_) => {
            crate::serial_println!("✗ Invalid syscall test failed: should have returned error");
            panic!("Invalid syscall test failed");
        }
        Err(SyscallError::InvalidSyscall) => {
            crate::serial_println!("✓ Invalid syscall test passed: returned InvalidSyscall error");
        }
        Err(e) => {
            crate::serial_println!("✗ Invalid syscall test failed: wrong error type {:?}", e);
            panic!("Invalid syscall test failed with wrong error");
        }
    }
    
    crate::serial_println!("✓ All basic system call tests passed!");
}

/// Test system call argument validation
pub fn test_syscall_validation() {
    let test_pid = ProcessId::new(1);
    
    // Test with valid arguments
    let valid_args = [0; 6];
    match dispatch_syscall(test_pid, SYS_GETPID, valid_args) {
        Ok(_) => {
            crate::serial_println!("✓ Valid argument test passed");
        }
        Err(e) => {
            crate::serial_println!("✗ Valid argument test failed: {:?}", e);
            panic!("Valid argument test failed");
        }
    }
    
    crate::serial_println!("✓ All system call validation tests passed!");
}

/// Test system call error handling
pub fn test_syscall_error_handling() {
    let test_pid = ProcessId::new(1);
    let args = [0; 6];
    
    // Test various error conditions
    let error_syscalls = [
        (999, SyscallError::InvalidSyscall),  // Invalid syscall number
        (0, SyscallError::InvalidSyscall),    // Zero syscall number
    ];
    
    for (syscall_num, expected_error) in error_syscalls.iter() {
        match dispatch_syscall(test_pid, *syscall_num, args) {
            Ok(_) => {
                crate::serial_println!("✗ Error test failed for syscall {}: should have returned error", syscall_num);
                panic!("Error test failed");
            }
            Err(actual_error) => {
                if core::mem::discriminant(&actual_error) == core::mem::discriminant(expected_error) {
                    crate::serial_println!("✓ Error test passed for syscall {}: returned {:?}", syscall_num, actual_error);
                } else {
                    crate::serial_println!("✗ Error test failed for syscall {}: expected {:?}, got {:?}", 
                                         syscall_num, expected_error, actual_error);
                    panic!("Error test failed with wrong error type");
                }
            }
        }
    }
    
    crate::serial_println!("✓ All system call error handling tests passed!");
}

/// Run all system call tests
pub fn run_all_syscall_tests() {
    crate::serial_println!("Running system call tests...");
    
    test_syscall_basic();
    test_syscall_validation();
    test_syscall_error_handling();
    
    crate::serial_println!("✓ All system call tests completed successfully!");
}