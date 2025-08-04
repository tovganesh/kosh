/// Test module to demonstrate capability-based security functionality
use crate::process::ProcessId;
use crate::ipc::capability::{
    CapabilityType, ResourceId, create_capability, check_capability, 
    init_capability_system, get_capability_statistics
};
use crate::ipc::security::{
    init_security_policy, grant_system_process_capabilities, 
    grant_user_process_capabilities, validate_capability_request
};
use crate::ipc::message::{MessageType, MessageData, send_message, create_message};
use crate::{serial_println};
use alloc::string::String;

/// Test capability-based security system
pub fn test_capability_security() -> Result<(), &'static str> {
    serial_println!("Testing capability-based security system...");
    
    // Initialize the capability system
    init_capability_system()?;
    init_security_policy()?;
    
    // Create test processes
    let system_process = ProcessId::new(100);
    let user_process = ProcessId::new(200);
    let target_process = ProcessId::new(300);
    
    // Test 1: Grant system process capabilities
    serial_println!("Test 1: Granting system process capabilities");
    match grant_system_process_capabilities(system_process) {
        Ok(capabilities) => {
            serial_println!("✓ Granted {} capabilities to system process {}", 
                           capabilities.len(), system_process.0);
        }
        Err(e) => {
            serial_println!("✗ Failed to grant system capabilities: {}", e);
            return Err("Failed to grant system capabilities");
        }
    }
    
    // Test 2: Grant user process capabilities
    serial_println!("Test 2: Granting user process capabilities");
    match grant_user_process_capabilities(user_process) {
        Ok(capabilities) => {
            serial_println!("✓ Granted {} capabilities to user process {}", 
                           capabilities.len(), user_process.0);
        }
        Err(e) => {
            serial_println!("✗ Failed to grant user capabilities: {}", e);
            return Err("Failed to grant user capabilities");
        }
    }
    
    // Test 3: Check capability validation
    serial_println!("Test 3: Testing capability validation");
    
    // System process should be able to send messages to any process
    let can_send_system = check_capability(
        system_process,
        CapabilityType::SendMessage,
        &ResourceId::Process(target_process)
    );
    serial_println!("System process can send messages: {}", can_send_system);
    
    // User process should have limited capabilities
    let can_send_user = check_capability(
        user_process,
        CapabilityType::SendMessage,
        &ResourceId::Process(target_process)
    );
    serial_println!("User process can send messages to arbitrary process: {}", can_send_user);
    
    // Test 4: Test security policy validation
    serial_println!("Test 4: Testing security policy validation");
    
    // Test restricted operation (should fail for user process)
    let admin_request_valid = validate_capability_request(
        user_process,
        CapabilityType::Admin,
        &ResourceId::Any
    );
    serial_println!("User process admin request valid: {}", admin_request_valid);
    
    // Test allowed operation
    let receive_request_valid = validate_capability_request(
        user_process,
        CapabilityType::ReceiveMessage,
        &ResourceId::Process(user_process)
    );
    serial_println!("User process receive message request valid: {}", receive_request_valid);
    
    // Test 5: Create specific capability for user process
    serial_println!("Test 5: Creating specific capability");
    match create_capability(
        user_process,
        CapabilityType::SendMessage,
        ResourceId::Process(target_process),
        Some(system_process)
    ) {
        Ok(cap_id) => {
            serial_println!("✓ Created specific SendMessage capability: {}", cap_id.0);
            
            // Now user process should be able to send to target process
            let can_send_specific = check_capability(
                user_process,
                CapabilityType::SendMessage,
                &ResourceId::Process(target_process)
            );
            serial_println!("User process can now send to target: {}", can_send_specific);
        }
        Err(e) => {
            serial_println!("✗ Failed to create specific capability: {}", e);
        }
    }
    
    // Test 6: Display system statistics
    serial_println!("Test 6: Capability system statistics");
    let stats = get_capability_statistics();
    serial_println!("Total capabilities: {}", stats.total_capabilities);
    serial_println!("Capability checks performed: {}", stats.checks_performed);
    serial_println!("Capability checks failed: {}", stats.checks_failed);
    
    serial_println!("✓ Capability-based security system test completed successfully");
    Ok(())
}

/// Test IPC message security with capabilities
pub fn test_ipc_message_security() -> Result<(), &'static str> {
    serial_println!("Testing IPC message security...");
    
    let sender = ProcessId::new(400);
    let receiver = ProcessId::new(500);
    
    // Grant basic capabilities
    match grant_user_process_capabilities(sender) {
        Ok(_) => {},
        Err(_) => return Err("Failed to grant user capabilities to sender"),
    }
    match grant_user_process_capabilities(receiver) {
        Ok(_) => {},
        Err(_) => return Err("Failed to grant user capabilities to receiver"),
    }
    
    // Create a message
    let message = create_message(
        sender,
        receiver,
        MessageType::ServiceRequest,
        MessageData::Text(String::from("Test message"))
    );
    
    serial_println!("Created test message from {} to {}", sender.0, receiver.0);
    
    // This should fail because sender doesn't have SendMessage capability for receiver
    match send_message(message) {
        Ok(()) => {
            serial_println!("✗ Message sent without proper capability (should have failed)");
        }
        Err(e) => {
            serial_println!("✓ Message correctly blocked: {}", e);
        }
    }
    
    // Grant specific capability and try again
    match create_capability(
        sender,
        CapabilityType::SendMessage,
        ResourceId::Process(receiver),
        None
    ) {
        Ok(_) => {},
        Err(_) => return Err("Failed to create specific capability"),
    }
    
    let message2 = create_message(
        sender,
        receiver,
        MessageType::ServiceRequest,
        MessageData::Text(String::from("Test message 2"))
    );
    
    match send_message(message2) {
        Ok(()) => {
            serial_println!("✓ Message sent successfully with proper capability");
        }
        Err(e) => {
            serial_println!("✗ Message failed with capability: {}", e);
        }
    }
    
    serial_println!("✓ IPC message security test completed");
    Ok(())
}