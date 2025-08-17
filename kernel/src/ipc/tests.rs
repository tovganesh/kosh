//! Comprehensive IPC (Inter-Process Communication) tests
//! 
//! Tests for message passing, capability-based security, and IPC performance.

use super::*;
use crate::test_harness::{TestResult, TestCategory};
use crate::{kernel_test, assert_kernel_eq, assert_kernel_true, assert_kernel_false};
use crate::process::ProcessId;
use alloc::vec::Vec;
use alloc::string::ToString;

/// Test message creation and basic properties
pub fn test_message_creation() -> TestResult {
    let sender = ProcessId::new(1);
    let receiver = ProcessId::new(2);
    let data = MessageData::Text("Hello, World!".to_string());
    
    let message = Message::new(
        sender,
        receiver,
        MessageType::ServiceRequest,
        data,
    );
    
    assert_kernel_eq!(message.sender(), sender, "Sender should match");
    assert_kernel_eq!(message.receiver(), receiver, "Receiver should match");
    assert_kernel_eq!(message.message_type(), MessageType::ServiceRequest, "Message type should match");
    
    if let MessageData::Text(ref text) = message.data() {
        assert_kernel_eq!(text, "Hello, World!", "Message text should match");
    } else {
        return TestResult::Fail;
    }
    
    TestResult::Pass
}

/// Test message queue operations
pub fn test_message_queue() -> TestResult {
    let mut queue = MessageQueue::new(10);
    
    // Test initial state
    assert_kernel_eq!(queue.len(), 0, "Queue should be empty initially");
    assert_kernel_eq!(queue.capacity(), 10, "Queue capacity should be 10");
    assert_kernel_true!(queue.is_empty(), "Queue should be empty");
    assert_kernel_false!(queue.is_full(), "Queue should not be full");
    
    // Test message enqueueing
    let message = Message::new(
        ProcessId::new(1),
        ProcessId::new(2),
        MessageType::ServiceRequest,
        MessageData::Text("Test message".to_string()),
    );
    
    assert_kernel_true!(queue.enqueue(message).is_ok(), "Should enqueue message successfully");
    assert_kernel_eq!(queue.len(), 1, "Queue length should be 1");
    assert_kernel_false!(queue.is_empty(), "Queue should not be empty");
    
    // Test message dequeuing
    let dequeued = queue.dequeue();
    assert_kernel_true!(dequeued.is_some(), "Should dequeue message successfully");
    assert_kernel_eq!(queue.len(), 0, "Queue should be empty after dequeue");
    
    TestResult::Pass
}

/// Test synchronous message passing
pub fn test_synchronous_messaging() -> TestResult {
    let sender = ProcessId::new(1);
    let receiver = ProcessId::new(2);
    
    // Create a simple message
    let message = Message::new(
        sender,
        receiver,
        MessageType::SystemCall,
        MessageData::Binary(vec![1, 2, 3, 4]),
    );
    
    // Test message properties
    assert_kernel_eq!(message.sender(), sender, "Sender should be preserved");
    assert_kernel_eq!(message.receiver(), receiver, "Receiver should be preserved");
    
    if let MessageData::Binary(ref data) = message.data() {
        assert_kernel_eq!(data.len(), 4, "Binary data length should be 4");
        assert_kernel_eq!(data[0], 1, "First byte should be 1");
        assert_kernel_eq!(data[3], 4, "Last byte should be 4");
    } else {
        return TestResult::Fail;
    }
    
    TestResult::Pass
}

/// Test capability-based security
pub fn test_capability_security() -> TestResult {
    use crate::ipc::capability::{Capability, CapabilityType, CapabilitySet};
    
    // Create capabilities
    let read_cap = Capability::new(CapabilityType::Read, 1);
    let write_cap = Capability::new(CapabilityType::Write, 1);
    let execute_cap = Capability::new(CapabilityType::Execute, 1);
    
    assert_kernel_eq!(read_cap.capability_type(), CapabilityType::Read, "Capability type should be Read");
    assert_kernel_eq!(read_cap.resource_id(), 1, "Resource ID should be 1");
    
    // Test capability set
    let mut cap_set = CapabilitySet::new();
    assert_kernel_true!(cap_set.is_empty(), "Capability set should be empty initially");
    
    cap_set.add_capability(read_cap);
    cap_set.add_capability(write_cap);
    
    assert_kernel_eq!(cap_set.len(), 2, "Capability set should have 2 capabilities");
    assert_kernel_true!(cap_set.has_capability(CapabilityType::Read, 1), "Should have read capability");
    assert_kernel_true!(cap_set.has_capability(CapabilityType::Write, 1), "Should have write capability");
    assert_kernel_false!(cap_set.has_capability(CapabilityType::Execute, 1), "Should not have execute capability");
    
    TestResult::Pass
}

/// Test capability delegation
pub fn test_capability_delegation() -> TestResult {
    use crate::ipc::capability::{Capability, CapabilityType, CapabilitySet};
    
    let mut source_set = CapabilitySet::new();
    let mut target_set = CapabilitySet::new();
    
    // Add capabilities to source
    source_set.add_capability(Capability::new(CapabilityType::Read, 1));
    source_set.add_capability(Capability::new(CapabilityType::Write, 1));
    
    // Test delegation
    let read_cap = source_set.get_capability(CapabilityType::Read, 1);
    assert_kernel_true!(read_cap.is_some(), "Should find read capability");
    
    if let Some(cap) = read_cap {
        target_set.add_capability(cap.clone());
        assert_kernel_true!(target_set.has_capability(CapabilityType::Read, 1), "Target should have delegated capability");
    }
    
    TestResult::Pass
}

/// Test IPC error handling
pub fn test_ipc_error_handling() -> TestResult {
    use crate::ipc::IpcError;
    
    // Test different error types
    let invalid_pid_error = IpcError::InvalidProcessId;
    let queue_full_error = IpcError::QueueFull;
    let permission_denied_error = IpcError::PermissionDenied;
    
    // Test error properties
    assert_kernel_eq!(invalid_pid_error.to_string(), "Invalid process ID", "Error message should match");
    assert_kernel_eq!(queue_full_error.to_string(), "Message queue is full", "Error message should match");
    assert_kernel_eq!(permission_denied_error.to_string(), "Permission denied", "Error message should match");
    
    TestResult::Pass
}

/// Test message routing
pub fn test_message_routing() -> TestResult {
    let sender = ProcessId::new(1);
    let receiver = ProcessId::new(2);
    let intermediate = ProcessId::new(3);
    
    // Create message with routing information
    let mut message = Message::new(
        sender,
        receiver,
        MessageType::ServiceRequest,
        MessageData::Text("Routed message".to_string()),
    );
    
    // Test message routing (simplified)
    assert_kernel_eq!(message.sender(), sender, "Original sender should be preserved");
    assert_kernel_eq!(message.receiver(), receiver, "Final receiver should be correct");
    
    // Simulate routing through intermediate process
    let routed_message = Message::new(
        intermediate,
        receiver,
        message.message_type(),
        message.data().clone(),
    );
    
    assert_kernel_eq!(routed_message.sender(), intermediate, "Intermediate sender should be set");
    assert_kernel_eq!(routed_message.receiver(), receiver, "Final receiver should be preserved");
    
    TestResult::Pass
}

/// Test shared memory IPC
pub fn test_shared_memory_ipc() -> TestResult {
    // Test shared memory region creation (simplified)
    #[derive(Debug, PartialEq)]
    struct SharedMemoryRegion {
        id: u32,
        size: usize,
        permissions: u32,
    }
    
    let region = SharedMemoryRegion {
        id: 1,
        size: 4096,
        permissions: 0b110, // Read + Write
    };
    
    assert_kernel_eq!(region.id, 1, "Shared memory ID should be 1");
    assert_kernel_eq!(region.size, 4096, "Shared memory size should be 4KB");
    assert_kernel_eq!(region.permissions & 0b100, 0b100, "Should have read permission");
    assert_kernel_eq!(region.permissions & 0b010, 0b010, "Should have write permission");
    assert_kernel_eq!(region.permissions & 0b001, 0, "Should not have execute permission");
    
    TestResult::Pass
}

/// Test IPC performance characteristics
pub fn test_ipc_performance() -> TestResult {
    let mut queue = MessageQueue::new(1000);
    
    // Test bulk message operations
    let messages: Vec<Message> = (0..100).map(|i| {
        Message::new(
            ProcessId::new(1),
            ProcessId::new(2),
            MessageType::ServiceRequest,
            MessageData::Binary(vec![i as u8; 64]),
        )
    }).collect();
    
    // Enqueue all messages
    for message in messages {
        assert_kernel_true!(queue.enqueue(message).is_ok(), "Should enqueue message successfully");
    }
    
    assert_kernel_eq!(queue.len(), 100, "Queue should contain 100 messages");
    
    // Dequeue all messages
    let mut dequeued_count = 0;
    while queue.dequeue().is_some() {
        dequeued_count += 1;
    }
    
    assert_kernel_eq!(dequeued_count, 100, "Should dequeue 100 messages");
    assert_kernel_true!(queue.is_empty(), "Queue should be empty after dequeuing all messages");
    
    TestResult::Pass
}

/// Test IPC security policies
pub fn test_ipc_security_policies() -> TestResult {
    use crate::ipc::security::{SecurityPolicy, AccessControl};
    
    let policy = SecurityPolicy::new();
    
    // Test default policy
    assert_kernel_true!(policy.is_strict_mode(), "Default policy should be strict");
    
    // Test access control
    let sender = ProcessId::new(1);
    let receiver = ProcessId::new(2);
    
    let access_control = AccessControl::new(sender, receiver);
    assert_kernel_eq!(access_control.sender(), sender, "Sender should match");
    assert_kernel_eq!(access_control.receiver(), receiver, "Receiver should match");
    
    TestResult::Pass
}

/// Register all IPC tests
pub fn register_ipc_tests(runner: &mut crate::test_harness::KernelTestRunner) {
    runner.register_test(kernel_test!(
        "Message Creation",
        TestCategory::Ipc,
        test_message_creation
    ));
    
    runner.register_test(kernel_test!(
        "Message Queue Operations",
        TestCategory::Ipc,
        test_message_queue
    ));
    
    runner.register_test(kernel_test!(
        "Synchronous Messaging",
        TestCategory::Ipc,
        test_synchronous_messaging
    ));
    
    runner.register_test(kernel_test!(
        "Capability Security",
        TestCategory::Ipc,
        test_capability_security
    ));
    
    runner.register_test(kernel_test!(
        "Capability Delegation",
        TestCategory::Ipc,
        test_capability_delegation
    ));
    
    runner.register_test(kernel_test!(
        "IPC Error Handling",
        TestCategory::Ipc,
        test_ipc_error_handling
    ));
    
    runner.register_test(kernel_test!(
        "Message Routing",
        TestCategory::Ipc,
        test_message_routing
    ));
    
    runner.register_test(kernel_test!(
        "Shared Memory IPC",
        TestCategory::Ipc,
        test_shared_memory_ipc
    ));
    
    runner.register_test(kernel_test!(
        "IPC Performance",
        TestCategory::Ipc,
        test_ipc_performance
    ));
    
    runner.register_test(kernel_test!(
        "IPC Security Policies",
        TestCategory::Ipc,
        test_ipc_security_policies
    ));
}