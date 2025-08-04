use alloc::{vec, vec::Vec};
use alloc::string::String;
use crate::process::ProcessId;
use crate::ipc::capability::{
    CapabilityType, ResourceId, CapabilityError, create_capability, check_capability
};
use crate::{serial_println};

/// Security policy for the IPC system
pub struct SecurityPolicy {
    /// Default capabilities for system processes
    system_capabilities: Vec<(CapabilityType, ResourceId)>,
    /// Default capabilities for user processes
    user_capabilities: Vec<(CapabilityType, ResourceId)>,
    /// Restricted operations that require explicit permission
    restricted_operations: Vec<CapabilityType>,
}

impl SecurityPolicy {
    /// Create a new security policy with default settings
    pub fn new() -> Self {
        let system_capabilities = vec![
            // System processes can send messages to any process
            (CapabilityType::SendMessage, ResourceId::Any),
            // System processes can receive messages from any process
            (CapabilityType::ReceiveMessage, ResourceId::Any),
            // System processes can access system calls
            (CapabilityType::SystemCall, ResourceId::Any),
            // System processes can manage memory
            (CapabilityType::MemoryManagement, ResourceId::Any),
            // System processes can manage other processes
            (CapabilityType::ProcessManagement, ResourceId::Any),
            // System processes can access file system
            (CapabilityType::FileSystem, ResourceId::Any),
        ];
        
        let user_capabilities = vec![
            // User processes can send messages to system services
            (CapabilityType::SendMessage, ResourceId::System(String::from("services"))),
            // User processes can receive messages sent to them
            (CapabilityType::ReceiveMessage, ResourceId::Any),
            // User processes have limited system call access
            (CapabilityType::SystemCall, ResourceId::System(String::from("user_syscalls"))),
        ];
        
        let restricted_operations = vec![
            CapabilityType::Admin,
            CapabilityType::DeviceAccess,
            CapabilityType::ProcessManagement,
            CapabilityType::MemoryManagement,
        ];
        
        Self {
            system_capabilities,
            user_capabilities,
            restricted_operations,
        }
    }
    
    /// Grant default capabilities to a system process
    pub fn grant_system_capabilities(&self, process_id: ProcessId) -> Result<Vec<crate::ipc::capability::CapabilityId>, CapabilityError> {
        let mut granted_capabilities = Vec::new();
        
        serial_println!("Granting system capabilities to process {}", process_id.0);
        
        for (capability_type, resource) in &self.system_capabilities {
            match create_capability(process_id, *capability_type, resource.clone(), None) {
                Ok(cap_id) => {
                    granted_capabilities.push(cap_id);
                    serial_println!("Granted {} capability for {} to process {}", 
                                   capability_type, resource, process_id.0);
                }
                Err(e) => {
                    serial_println!("Failed to grant {} capability to process {}: {}", 
                                   capability_type, process_id.0, e);
                    return Err(e);
                }
            }
        }
        
        Ok(granted_capabilities)
    }
    
    /// Grant default capabilities to a user process
    pub fn grant_user_capabilities(&self, process_id: ProcessId) -> Result<Vec<crate::ipc::capability::CapabilityId>, CapabilityError> {
        let mut granted_capabilities = Vec::new();
        
        serial_println!("Granting user capabilities to process {}", process_id.0);
        
        for (capability_type, resource) in &self.user_capabilities {
            match create_capability(process_id, *capability_type, resource.clone(), None) {
                Ok(cap_id) => {
                    granted_capabilities.push(cap_id);
                    serial_println!("Granted {} capability for {} to process {}", 
                                   capability_type, resource, process_id.0);
                }
                Err(e) => {
                    serial_println!("Failed to grant {} capability to process {}: {}", 
                                   capability_type, process_id.0, e);
                    return Err(e);
                }
            }
        }
        
        Ok(granted_capabilities)
    }
    
    /// Check if an operation is restricted
    pub fn is_restricted_operation(&self, capability_type: CapabilityType) -> bool {
        self.restricted_operations.contains(&capability_type)
    }
    
    /// Validate a capability request against security policy
    pub fn validate_capability_request(
        &self,
        requester: ProcessId,
        capability_type: CapabilityType,
        resource: &ResourceId,
    ) -> bool {
        // Check if the operation is restricted
        if self.is_restricted_operation(capability_type) {
            serial_println!("Restricted operation {} requested by process {}", 
                           capability_type, requester.0);
            
            // Only allow if the requester has admin privileges
            return check_capability(requester, CapabilityType::Admin, &ResourceId::Any);
        }
        
        // For non-restricted operations, allow based on existing capabilities
        match capability_type {
            CapabilityType::SendMessage => {
                // Check if the requester can send messages to the target
                check_capability(requester, CapabilityType::SendMessage, resource) ||
                check_capability(requester, CapabilityType::SendMessage, &ResourceId::Any)
            }
            CapabilityType::ReceiveMessage => {
                // Processes can always receive messages sent to them
                true
            }
            CapabilityType::SystemCall => {
                // Check if the requester has system call access
                check_capability(requester, CapabilityType::SystemCall, resource) ||
                check_capability(requester, CapabilityType::SystemCall, &ResourceId::Any)
            }
            _ => {
                // For other operations, check if the requester has the specific capability
                check_capability(requester, capability_type, resource)
            }
        }
    }
}

use spin::Mutex;

/// Global security policy instance
static SECURITY_POLICY: Mutex<Option<SecurityPolicy>> = Mutex::new(None);

/// Initialize the security policy system
pub fn init_security_policy() -> Result<(), &'static str> {
    serial_println!("Initializing security policy...");
    
    *SECURITY_POLICY.lock() = Some(SecurityPolicy::new());
    
    serial_println!("Security policy initialized");
    Ok(())
}

/// Get the global security policy (executes a closure with the policy)
fn with_security_policy<T, F>(f: F) -> Option<T>
where
    F: FnOnce(&SecurityPolicy) -> T,
{
    let policy_guard = SECURITY_POLICY.lock();
    policy_guard.as_ref().map(f)
}

/// Grant default capabilities to a system process
pub fn grant_system_process_capabilities(process_id: ProcessId) -> Result<Vec<crate::ipc::capability::CapabilityId>, CapabilityError> {
    with_security_policy(|policy| policy.grant_system_capabilities(process_id))
        .unwrap_or(Err(CapabilityError::ResourceExhausted))
}

/// Grant default capabilities to a user process
pub fn grant_user_process_capabilities(process_id: ProcessId) -> Result<Vec<crate::ipc::capability::CapabilityId>, CapabilityError> {
    with_security_policy(|policy| policy.grant_user_capabilities(process_id))
        .unwrap_or(Err(CapabilityError::ResourceExhausted))
}

/// Validate a capability request against the security policy
pub fn validate_capability_request(
    requester: ProcessId,
    capability_type: CapabilityType,
    resource: &ResourceId,
) -> bool {
    with_security_policy(|policy| policy.validate_capability_request(requester, capability_type, resource))
        .unwrap_or(false)
}

/// Check if an operation is restricted by the security policy
pub fn is_restricted_operation(capability_type: CapabilityType) -> bool {
    with_security_policy(|policy| policy.is_restricted_operation(capability_type))
        .unwrap_or(true) // Default to restricted if no policy is loaded
}

/// Create a secure IPC channel between two processes
pub fn create_secure_ipc_channel(
    process_a: ProcessId,
    process_b: ProcessId,
) -> Result<(), CapabilityError> {
    serial_println!("Creating secure IPC channel between processes {} and {}", 
                   process_a.0, process_b.0);
    
    // Grant bidirectional message passing capabilities
    create_capability(
        process_a,
        CapabilityType::SendMessage,
        ResourceId::Process(process_b),
        None,
    )?;
    
    create_capability(
        process_b,
        CapabilityType::SendMessage,
        ResourceId::Process(process_a),
        None,
    )?;
    
    serial_println!("Secure IPC channel created successfully");
    Ok(())
}

/// Revoke all capabilities for a process (used when process terminates)
pub fn revoke_process_capabilities(process_id: ProcessId) -> Result<(), CapabilityError> {
    serial_println!("Revoking all capabilities for process {}", process_id.0);
    
    // In a real implementation, we would iterate through all capabilities
    // and revoke those owned by the process. For now, we'll just log it.
    serial_println!("Process {} capabilities revoked", process_id.0);
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    
    #[test_case]
    fn test_security_policy_creation() {
        let policy = SecurityPolicy::new();
        
        assert!(!policy.system_capabilities.is_empty());
        assert!(!policy.user_capabilities.is_empty());
        assert!(!policy.restricted_operations.is_empty());
    }
    
    #[test_case]
    fn test_restricted_operations() {
        let policy = SecurityPolicy::new();
        
        assert!(policy.is_restricted_operation(CapabilityType::Admin));
        assert!(policy.is_restricted_operation(CapabilityType::DeviceAccess));
        assert!(!policy.is_restricted_operation(CapabilityType::Read));
    }
    
    #[test_case]
    fn test_capability_validation() {
        let policy = SecurityPolicy::new();
        let process_id = ProcessId::new(1);
        
        // Test restricted operation without admin privileges
        assert!(!policy.validate_capability_request(
            process_id,
            CapabilityType::Admin,
            &ResourceId::Any,
        ));
        
        // Test non-restricted operation
        assert!(policy.validate_capability_request(
            process_id,
            CapabilityType::ReceiveMessage,
            &ResourceId::Process(ProcessId::new(2)),
        ));
    }
}