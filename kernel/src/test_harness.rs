//! Comprehensive test harness for Kosh kernel
//! 
//! This module provides a testing framework for kernel components including
//! memory management, scheduler, IPC, and driver framework tests.

// Mock serial_println for library testing
#[cfg(not(feature = "console"))]
macro_rules! serial_println {
    ($($arg:tt)*) => {
        // No-op for library testing
    };
}

#[cfg(feature = "console")]
use crate::serial_println;
use alloc::vec::Vec;
use alloc::string::String;
use core::fmt;

/// Test result enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestResult {
    Pass,
    Fail,
    Skip,
}

impl fmt::Display for TestResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestResult::Pass => write!(f, "PASS"),
            TestResult::Fail => write!(f, "FAIL"),
            TestResult::Skip => write!(f, "SKIP"),
        }
    }
}

/// Test case structure
pub struct TestCase {
    pub name: &'static str,
    pub test_fn: fn() -> TestResult,
    pub category: TestCategory,
}

/// Test categories for organizing tests
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestCategory {
    Memory,
    Scheduler,
    Ipc,
    DriverFramework,
    SystemCall,
    Process,
    Core,
}

impl fmt::Display for TestCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestCategory::Memory => write!(f, "Memory"),
            TestCategory::Scheduler => write!(f, "Scheduler"),
            TestCategory::Ipc => write!(f, "IPC"),
            TestCategory::DriverFramework => write!(f, "Driver"),
            TestCategory::SystemCall => write!(f, "SysCall"),
            TestCategory::Process => write!(f, "Process"),
            TestCategory::Core => write!(f, "Core"),
        }
    }
}

/// Test statistics
#[derive(Default)]
pub struct TestStats {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl TestStats {
    pub fn success_rate(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f32 / self.total as f32) * 100.0
        }
    }
}

/// Main test runner for kernel tests
pub struct KernelTestRunner {
    tests: Vec<TestCase>,
    stats: TestStats,
}

impl KernelTestRunner {
    pub fn new() -> Self {
        Self {
            tests: Vec::new(),
            stats: TestStats::default(),
        }
    }

    /// Register a test case
    pub fn register_test(&mut self, test: TestCase) {
        self.tests.push(test);
    }

    /// Run all registered tests
    pub fn run_all_tests(&mut self) {
        serial_println!("\n=== Kosh Kernel Test Suite ===");
        serial_println!("Running {} tests...\n", self.tests.len());

        self.stats = TestStats::default();
        self.stats.total = self.tests.len();

        // Group tests by category
        let mut categories = Vec::new();
        for test in &self.tests {
            if !categories.contains(&test.category) {
                categories.push(test.category);
            }
        }

        // Run tests by category
        for category in categories {
            self.run_category_tests(category);
        }

        self.print_summary();
    }

    /// Run tests for a specific category
    fn run_category_tests(&mut self, category: TestCategory) {
        serial_println!("--- {} Tests ---", category);
        
        let category_tests: Vec<&TestCase> = self.tests.iter()
            .filter(|test| test.category == category)
            .collect();

        for test in category_tests {
            let result = (test.test_fn)();
            
            match result {
                TestResult::Pass => {
                    serial_println!("  [{}] {}", result, test.name);
                    self.stats.passed += 1;
                }
                TestResult::Fail => {
                    serial_println!("  [{}] {} ❌", result, test.name);
                    self.stats.failed += 1;
                }
                TestResult::Skip => {
                    serial_println!("  [{}] {} ⚠️", result, test.name);
                    self.stats.skipped += 1;
                }
            }
        }
        
        serial_println!("");
    }

    /// Print test summary
    fn print_summary(&self) {
        serial_println!("=== Test Summary ===");
        serial_println!("Total:   {}", self.stats.total);
        serial_println!("Passed:  {} ✅", self.stats.passed);
        serial_println!("Failed:  {} ❌", self.stats.failed);
        serial_println!("Skipped: {} ⚠️", self.stats.skipped);
        serial_println!("Success Rate: {:.1}%", self.stats.success_rate());
        
        if self.stats.failed > 0 {
            serial_println!("\n⚠️  Some tests failed! Check output above for details.");
        } else if self.stats.skipped > 0 {
            serial_println!("\n✅ All tests passed (some skipped)");
        } else {
            serial_println!("\n🎉 All tests passed!");
        }
    }
}

/// Macro for creating test cases
#[macro_export]
macro_rules! kernel_test {
    ($name:expr, $category:expr, $test_fn:expr) => {
        TestCase {
            name: $name,
            test_fn: $test_fn,
            category: $category,
        }
    };
}

/// Assertion macros for kernel tests
#[macro_export]
macro_rules! assert_kernel_eq {
    ($left:expr, $right:expr) => {
        if $left != $right {
            serial_println!("    Assertion failed: {} != {}", $left, $right);
            return TestResult::Fail;
        }
    };
    ($left:expr, $right:expr, $msg:expr) => {
        if $left != $right {
            serial_println!("    Assertion failed: {}", $msg);
            serial_println!("      Expected: {}", $right);
            serial_println!("      Got:      {}", $left);
            return TestResult::Fail;
        }
    };
}

#[macro_export]
macro_rules! assert_kernel_ne {
    ($left:expr, $right:expr) => {
        if $left == $right {
            serial_println!("    Assertion failed: {} == {}", $left, $right);
            return TestResult::Fail;
        }
    };
}

#[macro_export]
macro_rules! assert_kernel_true {
    ($expr:expr) => {
        if !$expr {
            serial_println!("    Assertion failed: expression was false");
            return TestResult::Fail;
        }
    };
    ($expr:expr, $msg:expr) => {
        if !$expr {
            serial_println!("    Assertion failed: {}", $msg);
            return TestResult::Fail;
        }
    };
}

#[macro_export]
macro_rules! assert_kernel_false {
    ($expr:expr) => {
        if $expr {
            serial_println!("    Assertion failed: expression was true");
            return TestResult::Fail;
        }
    };
}

/// Test helper functions
pub mod test_helpers {
    use super::*;
    use crate::memory::{PAGE_SIZE, align_up, align_down, is_aligned};
    
    /// Create test data of specified size
    pub fn create_test_data(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 256) as u8).collect()
    }
    
    /// Verify memory alignment
    pub fn verify_alignment(addr: usize) -> bool {
        is_aligned(addr)
    }
    
    /// Create aligned test address
    pub fn create_aligned_address(base: usize) -> usize {
        align_up(base)
    }
    
    /// Simulate memory pressure for testing
    pub fn simulate_memory_pressure() -> TestResult {
        // This would simulate low memory conditions
        // For now, just return success
        TestResult::Pass
    }
}

// Export commonly used items
pub use test_helpers::*;