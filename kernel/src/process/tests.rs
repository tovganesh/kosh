//! Comprehensive process and scheduler tests
//! 
//! Tests for process management, scheduling algorithms, and context switching.

use super::*;
use crate::test_harness::{TestResult, TestCategory};
use crate::{kernel_test, assert_kernel_eq, assert_kernel_true, assert_kernel_false};
use alloc::vec::Vec;
use alloc::string::ToString;

/// Test process creation and management
pub fn test_process_creation() -> TestResult {
    let pid = ProcessId::new(42);
    assert_kernel_eq!(pid.as_u32(), 42, "Process ID should match");
    
    // Test process creation
    let process = Process::new(
        pid,
        "test_process".to_string(),
        ProcessType::User,
        Priority::Normal,
    );
    
    assert_kernel_eq!(process.pid(), pid, "Process PID should match");
    assert_kernel_eq!(process.name(), "test_process", "Process name should match");
    assert_kernel_eq!(process.process_type(), ProcessType::User, "Process type should be User");
    assert_kernel_eq!(process.priority(), Priority::Normal, "Priority should be Normal");
    assert_kernel_eq!(process.state(), ProcessState::Ready, "Initial state should be Ready");
    
    TestResult::Pass
}

/// Test process state transitions
pub fn test_process_state_transitions() -> TestResult {
    let mut process = Process::new(
        ProcessId::new(1),
        "test".to_string(),
        ProcessType::User,
        Priority::Normal,
    );
    
    // Test state transitions
    assert_kernel_eq!(process.state(), ProcessState::Ready, "Initial state should be Ready");
    
    process.set_state(ProcessState::Running);
    assert_kernel_eq!(process.state(), ProcessState::Running, "State should be Running");
    
    process.set_state(ProcessState::Blocked(BlockReason::WaitingForIo));
    if let ProcessState::Blocked(reason) = process.state() {
        assert_kernel_eq!(reason, BlockReason::WaitingForIo, "Block reason should match");
    } else {
        return TestResult::Fail;
    }
    
    process.set_state(ProcessState::Zombie);
    assert_kernel_eq!(process.state(), ProcessState::Zombie, "State should be Zombie");
    
    TestResult::Pass
}

/// Test process table management
pub fn test_process_table() -> TestResult {
    let mut table = ProcessTable::new(10);
    
    // Test initial state
    assert_kernel_eq!(table.count(), 0, "Initial process count should be 0");
    assert_kernel_eq!(table.capacity(), 10, "Capacity should be 10");
    
    // Add processes
    let pid1 = ProcessId::new(1);
    let process1 = Process::new(
        pid1,
        "process1".to_string(),
        ProcessType::User,
        Priority::Normal,
    );
    
    assert_kernel_true!(table.add_process(process1).is_ok(), "Should add process successfully");
    assert_kernel_eq!(table.count(), 1, "Process count should be 1");
    
    // Test process lookup
    assert_kernel_true!(table.get_process(pid1).is_some(), "Should find process by PID");
    assert_kernel_true!(table.get_process(ProcessId::new(999)).is_none(), "Should not find non-existent process");
    
    TestResult::Pass
}

/// Test scheduler creation and configuration
pub fn test_scheduler_creation() -> TestResult {
    let scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
    
    assert_kernel_eq!(scheduler.algorithm(), SchedulingAlgorithm::RoundRobin, "Algorithm should be RoundRobin");
    assert_kernel_eq!(scheduler.time_slice_ms(), 10, "Time slice should be 10ms");
    assert_kernel_eq!(scheduler.ready_queue_size(), 0, "Ready queue should be empty");
    
    TestResult::Pass
}

/// Test scheduler algorithm changes
pub fn test_scheduler_algorithms() -> TestResult {
    let mut scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
    
    // Test algorithm change
    scheduler.set_algorithm(SchedulingAlgorithm::PriorityBased);
    assert_kernel_eq!(scheduler.algorithm(), SchedulingAlgorithm::PriorityBased, "Algorithm should change to PriorityBased");
    
    scheduler.set_algorithm(SchedulingAlgorithm::Cfs);
    assert_kernel_eq!(scheduler.algorithm(), SchedulingAlgorithm::Cfs, "Algorithm should change to CFS");
    
    TestResult::Pass
}

/// Test scheduler time slice management
pub fn test_scheduler_time_slice() -> TestResult {
    let mut scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
    
    // Test time slice change
    scheduler.set_time_slice_ms(20);
    assert_kernel_eq!(scheduler.time_slice_ms(), 20, "Time slice should change to 20ms");
    
    // Test adaptive time slice
    scheduler.set_adaptive_time_slice(true);
    assert_kernel_true!(scheduler.is_adaptive_time_slice(), "Adaptive time slice should be enabled");
    
    TestResult::Pass
}

/// Test process priority management
pub fn test_process_priorities() -> TestResult {
    let mut high_priority = Process::new(
        ProcessId::new(1),
        "high".to_string(),
        ProcessType::User,
        Priority::High,
    );
    
    let mut low_priority = Process::new(
        ProcessId::new(2),
        "low".to_string(),
        ProcessType::User,
        Priority::Low,
    );
    
    assert_kernel_eq!(high_priority.priority(), Priority::High, "Priority should be High");
    assert_kernel_eq!(low_priority.priority(), Priority::Low, "Priority should be Low");
    
    // Test priority changes
    high_priority.set_priority(Priority::Normal);
    assert_kernel_eq!(high_priority.priority(), Priority::Normal, "Priority should change to Normal");
    
    low_priority.set_priority(Priority::High);
    assert_kernel_eq!(low_priority.priority(), Priority::High, "Priority should change to High");
    
    TestResult::Pass
}

/// Test CPU context management
pub fn test_cpu_context() -> TestResult {
    use crate::process::context::CpuContext;
    
    let context = CpuContext::new();
    
    // Test initial context state
    assert_kernel_eq!(context.rax(), 0, "RAX should be 0");
    assert_kernel_eq!(context.rbx(), 0, "RBX should be 0");
    assert_kernel_eq!(context.rsp(), 0, "RSP should be 0");
    
    TestResult::Pass
}

/// Test context switching simulation
pub fn test_context_switching() -> TestResult {
    use crate::process::context::CpuContext;
    
    let mut context1 = CpuContext::new();
    let mut context2 = CpuContext::new();
    
    // Set different values for each context
    context1.set_rax(0x1111);
    context1.set_rbx(0x2222);
    
    context2.set_rax(0x3333);
    context2.set_rbx(0x4444);
    
    // Verify contexts are different
    assert_kernel_eq!(context1.rax(), 0x1111, "Context1 RAX should be 0x1111");
    assert_kernel_eq!(context2.rax(), 0x3333, "Context2 RAX should be 0x3333");
    
    TestResult::Pass
}

/// Test scheduler statistics
pub fn test_scheduler_statistics() -> TestResult {
    let scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
    let stats = scheduler.get_statistics();
    
    // Test initial statistics
    assert_kernel_eq!(stats.total_context_switches, 0, "Initial context switches should be 0");
    assert_kernel_eq!(stats.total_processes_scheduled, 0, "Initial processes scheduled should be 0");
    assert_kernel_true!(stats.average_wait_time_ms >= 0.0, "Average wait time should be non-negative");
    
    TestResult::Pass
}

/// Test process parent-child relationships
pub fn test_parent_child_relationships() -> TestResult {
    let mut table = ProcessTable::new(10);
    
    // Create parent process
    let parent_pid = ProcessId::new(1);
    let parent = Process::new(
        parent_pid,
        "parent".to_string(),
        ProcessType::User,
        Priority::Normal,
    );
    
    // Create child process
    let child_pid = ProcessId::new(2);
    let mut child = Process::new(
        child_pid,
        "child".to_string(),
        ProcessType::User,
        Priority::Normal,
    );
    
    child.set_parent(Some(parent_pid));
    
    // Add to table
    table.add_process(parent).unwrap();
    table.add_process(child).unwrap();
    
    // Test relationship
    if let Some(child_process) = table.get_process(child_pid) {
        assert_kernel_eq!(child_process.parent(), Some(parent_pid), "Child should have correct parent");
    } else {
        return TestResult::Fail;
    }
    
    TestResult::Pass
}

/// Test mobile-specific scheduling optimizations
pub fn test_mobile_scheduling() -> TestResult {
    let mut scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
    
    // Test interactive task prioritization
    scheduler.enable_interactive_boost(true);
    assert_kernel_true!(scheduler.is_interactive_boost_enabled(), "Interactive boost should be enabled");
    
    // Test background task throttling
    scheduler.enable_background_throttling(true);
    assert_kernel_true!(scheduler.is_background_throttling_enabled(), "Background throttling should be enabled");
    
    TestResult::Pass
}

/// Register all process and scheduler tests
pub fn register_process_tests(runner: &mut crate::test_harness::KernelTestRunner) {
    runner.register_test(kernel_test!(
        "Process Creation",
        TestCategory::Process,
        test_process_creation
    ));
    
    runner.register_test(kernel_test!(
        "Process State Transitions",
        TestCategory::Process,
        test_process_state_transitions
    ));
    
    runner.register_test(kernel_test!(
        "Process Table Management",
        TestCategory::Process,
        test_process_table
    ));
    
    runner.register_test(kernel_test!(
        "Scheduler Creation",
        TestCategory::Scheduler,
        test_scheduler_creation
    ));
    
    runner.register_test(kernel_test!(
        "Scheduler Algorithms",
        TestCategory::Scheduler,
        test_scheduler_algorithms
    ));
    
    runner.register_test(kernel_test!(
        "Scheduler Time Slice",
        TestCategory::Scheduler,
        test_scheduler_time_slice
    ));
    
    runner.register_test(kernel_test!(
        "Process Priorities",
        TestCategory::Process,
        test_process_priorities
    ));
    
    runner.register_test(kernel_test!(
        "CPU Context Management",
        TestCategory::Process,
        test_cpu_context
    ));
    
    runner.register_test(kernel_test!(
        "Context Switching",
        TestCategory::Process,
        test_context_switching
    ));
    
    runner.register_test(kernel_test!(
        "Scheduler Statistics",
        TestCategory::Scheduler,
        test_scheduler_statistics
    ));
    
    runner.register_test(kernel_test!(
        "Parent-Child Relationships",
        TestCategory::Process,
        test_parent_child_relationships
    ));
    
    runner.register_test(kernel_test!(
        "Mobile Scheduling Optimizations",
        TestCategory::Scheduler,
        test_mobile_scheduling
    ));
}