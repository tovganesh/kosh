pub mod message;
pub mod queue;
pub mod capability;
pub mod security;

#[cfg(test)]
pub mod capability_test;

#[cfg(test)]
pub mod tests;

pub use message::{
    Message, MessageId, MessageType, MessageData, MessageHeader, MessageError,
    create_message, send_message, receive_message, reply_message
};
pub use queue::{
    MessageQueue, MessageQueueError, create_message_queue, get_message_queue
};
pub use capability::{
    Capability, CapabilityType, CapabilitySet, CapabilityError,
    create_capability, check_capability, delegate_capability
};
pub use security::{
    init_security_policy, grant_system_process_capabilities, grant_user_process_capabilities,
    validate_capability_request, is_restricted_operation, create_secure_ipc_channel,
    revoke_process_capabilities
};

use crate::process::ProcessId;
use crate::{serial_println, println};

/// IPC system initialization
pub fn init_ipc_system() -> Result<(), &'static str> {
    serial_println!("Initializing IPC system...");
    
    // Initialize message queue system
    queue::init_message_queues()?;
    
    // Initialize capability system
    capability::init_capability_system()?;
    
    // Initialize security policy
    security::init_security_policy()?;
    
    serial_println!("IPC system initialized successfully");
    Ok(())
}

/// IPC system statistics
#[derive(Debug, Clone)]
pub struct IpcStatistics {
    pub total_messages_sent: u64,
    pub total_messages_received: u64,
    pub active_message_queues: usize,
    pub total_capabilities: usize,
    pub capability_checks_performed: u64,
    pub capability_checks_failed: u64,
}

/// Get IPC system statistics
pub fn get_ipc_statistics() -> IpcStatistics {
    let queue_stats = queue::get_queue_statistics();
    let capability_stats = capability::get_capability_statistics();
    
    IpcStatistics {
        total_messages_sent: queue_stats.total_messages_sent,
        total_messages_received: queue_stats.total_messages_received,
        active_message_queues: queue_stats.active_queues,
        total_capabilities: capability_stats.total_capabilities,
        capability_checks_performed: capability_stats.checks_performed,
        capability_checks_failed: capability_stats.checks_failed,
    }
}

/// Print IPC system information
pub fn print_ipc_info() {
    let stats = get_ipc_statistics();
    
    serial_println!("IPC System Statistics:");
    serial_println!("  Messages sent: {}", stats.total_messages_sent);
    serial_println!("  Messages received: {}", stats.total_messages_received);
    serial_println!("  Active message queues: {}", stats.active_message_queues);
    serial_println!("  Total capabilities: {}", stats.total_capabilities);
    serial_println!("  Capability checks: {} (failed: {})", 
                   stats.capability_checks_performed, stats.capability_checks_failed);
    
    println!("IPC: {} queues, {} messages sent", 
             stats.active_message_queues, stats.total_messages_sent);
}

/// Run capability-based security tests
#[cfg(test)]
pub fn run_capability_tests() -> Result<(), &'static str> {
    serial_println!("Running capability-based security tests...");
    
    capability_test::test_capability_security()?;
    capability_test::test_ipc_message_security()?;
    
    serial_println!("All capability tests passed successfully!");
    Ok(())
}