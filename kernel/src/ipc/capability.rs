use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use alloc::string::String;
use spin::Mutex;
use core::fmt;
use crate::process::ProcessId;
use crate::{serial_println};

/// Capability identifier type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityId(pub u64);

impl CapabilityId {
    /// Create a new capability ID
    pub fn new(id: u64) -> Self {
        CapabilityId(id)
    }
    
    /// Get the raw ID value
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Types of capabilities in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityType {
    /// Read access to a resource
    Read,
    /// Write access to a resource
    Write,
    /// Execute access to a resource
    Execute,
    /// Create new resources
    Create,
    /// Delete resources
    Delete,
    /// Send messages to a process
    SendMessage,
    /// Receive messages from a process
    ReceiveMessage,
    /// Access to system calls
    SystemCall,
    /// Access to hardware devices
    DeviceAccess,
    /// Memory management operations
    MemoryManagement,
    /// Process management operations
    ProcessManagement,
    /// File system operations
    FileSystem,
    /// Network operations
    Network,
    /// Administrative privileges
    Admin,
}

impl fmt::Display for CapabilityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityType::Read => write!(f, "Read"),
            CapabilityType::Write => write!(f, "Write"),
            CapabilityType::Execute => write!(f, "Execute"),
            CapabilityType::Create => write!(f, "Create"),
            CapabilityType::Delete => write!(f, "Delete"),
            CapabilityType::SendMessage => write!(f, "SendMessage"),
            CapabilityType::ReceiveMessage => write!(f, "ReceiveMessage"),
            CapabilityType::SystemCall => write!(f, "SystemCall"),
            CapabilityType::DeviceAccess => write!(f, "DeviceAccess"),
            CapabilityType::MemoryManagement => write!(f, "MemoryManagement"),
            CapabilityType::ProcessManagement => write!(f, "ProcessManagement"),
            CapabilityType::FileSystem => write!(f, "FileSystem"),
            CapabilityType::Network => write!(f, "Network"),
            CapabilityType::Admin => write!(f, "Admin"),
        }
    }
}

/// Resource identifier for capability scoping
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ResourceId {
    /// Any resource (wildcard)
    Any,
    /// Specific process
    Process(ProcessId),
    /// Specific device
    Device(String),
    /// Specific file or directory
    File(String),
    /// Specific network endpoint
    Network(String),
    /// System resource
    System(String),
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceId::Any => write!(f, "*"),
            ResourceId::Process(pid) => write!(f, "process:{}", pid.0),
            ResourceId::Device(name) => write!(f, "device:{}", name),
            ResourceId::File(path) => write!(f, "file:{}", path),
            ResourceId::Network(endpoint) => write!(f, "network:{}", endpoint),
            ResourceId::System(name) => write!(f, "system:{}", name),
        }
    }
}

/// A capability grants specific permissions to a resource
#[derive(Debug, Clone)]
pub struct Capability {
    /// Unique capability identifier
    pub id: CapabilityId,
    /// Type of capability
    pub capability_type: CapabilityType,
    /// Resource this capability applies to
    pub resource: ResourceId,
    /// Process that owns this capability
    pub owner: ProcessId,
    /// Process that granted this capability (None for system-granted)
    pub granter: Option<ProcessId>,
    /// Whether this capability can be delegated to other processes
    pub delegatable: bool,
    /// Expiration time (None for permanent)
    pub expires_at: Option<u64>,
    /// Creation timestamp
    pub created_at: u64,
}

impl Capability {
    /// Create a new capability
    pub fn new(
        capability_type: CapabilityType,
        resource: ResourceId,
        owner: ProcessId,
        granter: Option<ProcessId>,
    ) -> Self {
        let id = generate_capability_id();
        let created_at = get_current_time_ms();
        
        Self {
            id,
            capability_type,
            resource,
            owner,
            granter,
            delegatable: false,
            expires_at: None,
            created_at,
        }
    }
    
    /// Check if this capability is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            get_current_time_ms() > expires_at
        } else {
            false
        }
    }
    
    /// Check if this capability matches the given type and resource
    pub fn matches(&self, capability_type: CapabilityType, resource: &ResourceId) -> bool {
        if self.is_expired() {
            return false;
        }
        
        // Check capability type
        if self.capability_type != capability_type {
            return false;
        }
        
        // Check resource match
        match (&self.resource, resource) {
            (ResourceId::Any, _) => true, // Wildcard matches everything
            (a, b) if a == b => true,     // Exact match
            _ => false,
        }
    }
    
    /// Set expiration time for this capability
    pub fn set_expiration(&mut self, expires_at: u64) {
        self.expires_at = Some(expires_at);
    }
    
    /// Make this capability delegatable
    pub fn make_delegatable(&mut self) {
        self.delegatable = true;
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cap[{}] {} on {} (owner: {})", 
               self.id.0, self.capability_type, self.resource, self.owner.0)
    }
}

/// Set of capabilities owned by a process
#[derive(Debug, Clone)]
pub struct CapabilitySet {
    /// List of capabilities
    capabilities: Vec<Capability>,
}

impl CapabilitySet {
    /// Create a new empty capability set
    pub fn new() -> Self {
        Self {
            capabilities: Vec::new(),
        }
    }
    
    /// Add a capability to the set
    pub fn add(&mut self, capability: Capability) {
        self.capabilities.push(capability);
    }
    
    /// Remove a capability by ID
    pub fn remove(&mut self, capability_id: CapabilityId) -> Option<Capability> {
        if let Some(pos) = self.capabilities.iter().position(|c| c.id == capability_id) {
            Some(self.capabilities.remove(pos))
        } else {
            None
        }
    }
    
    /// Check if the set contains a capability for the given type and resource
    pub fn has_capability(&self, capability_type: CapabilityType, resource: &ResourceId) -> bool {
        self.capabilities.iter()
            .any(|cap| cap.matches(capability_type, resource))
    }
    
    /// Get all capabilities of a specific type
    pub fn get_capabilities_by_type(&self, capability_type: CapabilityType) -> Vec<&Capability> {
        self.capabilities.iter()
            .filter(|cap| cap.capability_type == capability_type && !cap.is_expired())
            .collect()
    }
    
    /// Get all capabilities for a specific resource
    pub fn get_capabilities_by_resource(&self, resource: &ResourceId) -> Vec<&Capability> {
        self.capabilities.iter()
            .filter(|cap| cap.matches(cap.capability_type, resource))
            .collect()
    }
    
    /// Remove expired capabilities
    pub fn cleanup_expired(&mut self) -> usize {
        let initial_count = self.capabilities.len();
        self.capabilities.retain(|cap| !cap.is_expired());
        initial_count - self.capabilities.len()
    }
    
    /// Get the number of capabilities in the set
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }
    
    /// Check if the set is empty
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }
    
    /// Get the total size of the capability set in bytes (approximate)
    pub fn size(&self) -> usize {
        self.capabilities.len() * core::mem::size_of::<Capability>()
    }
    
    /// Get all capabilities (for debugging)
    pub fn get_all(&self) -> &[Capability] {
        &self.capabilities
    }
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self::new()
    }
}

/// Capability management errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityError {
    /// Capability not found
    CapabilityNotFound,
    /// Permission denied for this operation
    PermissionDenied,
    /// Capability has expired
    CapabilityExpired,
    /// Cannot delegate this capability
    NotDelegatable,
    /// Invalid capability parameters
    InvalidCapability,
    /// System resource exhausted
    ResourceExhausted,
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityError::CapabilityNotFound => write!(f, "Capability not found"),
            CapabilityError::PermissionDenied => write!(f, "Permission denied"),
            CapabilityError::CapabilityExpired => write!(f, "Capability has expired"),
            CapabilityError::NotDelegatable => write!(f, "Capability cannot be delegated"),
            CapabilityError::InvalidCapability => write!(f, "Invalid capability"),
            CapabilityError::ResourceExhausted => write!(f, "System resource exhausted"),
        }
    }
}

/// Global capability manager
struct CapabilityManager {
    /// Map of process ID to capability set
    process_capabilities: BTreeMap<ProcessId, CapabilitySet>,
    /// Total number of capabilities created
    total_capabilities_created: u64,
    /// Total number of capability checks performed
    checks_performed: u64,
    /// Total number of failed capability checks
    checks_failed: u64,
}

impl CapabilityManager {
    /// Create a new capability manager
    fn new() -> Self {
        Self {
            process_capabilities: BTreeMap::new(),
            total_capabilities_created: 0,
            checks_performed: 0,
            checks_failed: 0,
        }
    }
    
    /// Grant a capability to a process
    fn grant_capability(
        &mut self,
        process_id: ProcessId,
        capability_type: CapabilityType,
        resource: ResourceId,
        granter: Option<ProcessId>,
    ) -> Result<CapabilityId, CapabilityError> {
        let capability = Capability::new(capability_type, resource, process_id, granter);
        let capability_id = capability.id;
        
        let capability_set = self.process_capabilities
            .entry(process_id)
            .or_insert_with(CapabilitySet::new);
        
        capability_set.add(capability);
        self.total_capabilities_created += 1;
        
        serial_println!("Granted {} capability to process {} for resource {}", 
                       capability_type, process_id.0, 
                       capability_set.capabilities.last().unwrap().resource);
        
        Ok(capability_id)
    }
    
    /// Check if a process has a specific capability
    fn check_capability(
        &mut self,
        process_id: ProcessId,
        capability_type: CapabilityType,
        resource: &ResourceId,
    ) -> bool {
        self.checks_performed += 1;
        
        if let Some(capability_set) = self.process_capabilities.get(&process_id) {
            let has_capability = capability_set.has_capability(capability_type, resource);
            if !has_capability {
                self.checks_failed += 1;
                serial_println!("Capability check failed: process {} lacks {} for {}", 
                               process_id.0, capability_type, resource);
            }
            has_capability
        } else {
            self.checks_failed += 1;
            serial_println!("Capability check failed: no capabilities for process {}", 
                           process_id.0);
            false
        }
    }
    
    /// Delegate a capability from one process to another
    fn delegate_capability(
        &mut self,
        from_process: ProcessId,
        to_process: ProcessId,
        capability_id: CapabilityId,
    ) -> Result<CapabilityId, CapabilityError> {
        // Find the capability in the source process
        let source_capability = {
            let source_set = self.process_capabilities.get(&from_process)
                .ok_or(CapabilityError::CapabilityNotFound)?;
            
            let capability = source_set.capabilities.iter()
                .find(|c| c.id == capability_id)
                .ok_or(CapabilityError::CapabilityNotFound)?;
            
            if !capability.delegatable {
                return Err(CapabilityError::NotDelegatable);
            }
            
            if capability.is_expired() {
                return Err(CapabilityError::CapabilityExpired);
            }
            
            capability.clone()
        };
        
        // Create a new capability for the target process
        let mut new_capability = Capability::new(
            source_capability.capability_type,
            source_capability.resource,
            to_process,
            Some(from_process),
        );
        
        // Inherit expiration and delegation properties
        new_capability.expires_at = source_capability.expires_at;
        new_capability.delegatable = source_capability.delegatable;
        
        let new_capability_id = new_capability.id;
        
        // Add to target process
        let target_set = self.process_capabilities
            .entry(to_process)
            .or_insert_with(CapabilitySet::new);
        
        target_set.add(new_capability);
        self.total_capabilities_created += 1;
        
        serial_println!("Delegated capability {} from process {} to process {}", 
                       capability_id.0, from_process.0, to_process.0);
        
        Ok(new_capability_id)
    }
    
    /// Revoke a capability from a process
    fn revoke_capability(
        &mut self,
        process_id: ProcessId,
        capability_id: CapabilityId,
    ) -> Result<(), CapabilityError> {
        let capability_set = self.process_capabilities.get_mut(&process_id)
            .ok_or(CapabilityError::CapabilityNotFound)?;
        
        capability_set.remove(capability_id)
            .ok_or(CapabilityError::CapabilityNotFound)?;
        
        serial_println!("Revoked capability {} from process {}", 
                       capability_id.0, process_id.0);
        
        Ok(())
    }
    
    /// Get capability statistics
    fn get_statistics(&self) -> CapabilityStatistics {
        let mut total_capabilities = 0;
        let mut expired_capabilities = 0;
        
        for capability_set in self.process_capabilities.values() {
            total_capabilities += capability_set.len();
            for capability in &capability_set.capabilities {
                if capability.is_expired() {
                    expired_capabilities += 1;
                }
            }
        }
        
        CapabilityStatistics {
            total_capabilities,
            expired_capabilities,
            processes_with_capabilities: self.process_capabilities.len(),
            total_capabilities_created: self.total_capabilities_created,
            checks_performed: self.checks_performed,
            checks_failed: self.checks_failed,
        }
    }
    
    /// Clean up expired capabilities for all processes
    fn cleanup_expired_capabilities(&mut self) -> usize {
        let mut total_cleaned = 0;
        
        for capability_set in self.process_capabilities.values_mut() {
            total_cleaned += capability_set.cleanup_expired();
        }
        
        if total_cleaned > 0 {
            serial_println!("Cleaned up {} expired capabilities", total_cleaned);
        }
        
        total_cleaned
    }
}

/// Capability system statistics
#[derive(Debug, Clone)]
pub struct CapabilityStatistics {
    pub total_capabilities: usize,
    pub expired_capabilities: usize,
    pub processes_with_capabilities: usize,
    pub total_capabilities_created: u64,
    pub checks_performed: u64,
    pub checks_failed: u64,
}

/// Global capability manager instance
static CAPABILITY_MANAGER: Mutex<Option<CapabilityManager>> = Mutex::new(None);

/// Global capability ID counter
static mut NEXT_CAPABILITY_ID: u64 = 1;

/// Generate a unique capability ID
fn generate_capability_id() -> CapabilityId {
    unsafe {
        let id = NEXT_CAPABILITY_ID;
        NEXT_CAPABILITY_ID += 1;
        CapabilityId::new(id)
    }
}

/// Initialize the capability system
pub fn init_capability_system() -> Result<(), &'static str> {
    serial_println!("Initializing capability system...");
    
    let manager = CapabilityManager::new();
    *CAPABILITY_MANAGER.lock() = Some(manager);
    
    serial_println!("Capability system initialized");
    Ok(())
}

/// Create and grant a capability to a process
pub fn create_capability(
    process_id: ProcessId,
    capability_type: CapabilityType,
    resource: ResourceId,
    granter: Option<ProcessId>,
) -> Result<CapabilityId, CapabilityError> {
    let mut manager = CAPABILITY_MANAGER.lock();
    let manager = manager.as_mut().ok_or(CapabilityError::ResourceExhausted)?;
    manager.grant_capability(process_id, capability_type, resource, granter)
}

/// Check if a process has a specific capability
pub fn check_capability(
    process_id: ProcessId,
    capability_type: CapabilityType,
    resource: &ResourceId,
) -> bool {
    let mut manager = CAPABILITY_MANAGER.lock();
    if let Some(manager) = manager.as_mut() {
        manager.check_capability(process_id, capability_type, resource)
    } else {
        false
    }
}

/// Delegate a capability from one process to another
pub fn delegate_capability(
    from_process: ProcessId,
    to_process: ProcessId,
    capability_id: CapabilityId,
) -> Result<CapabilityId, CapabilityError> {
    let mut manager = CAPABILITY_MANAGER.lock();
    let manager = manager.as_mut().ok_or(CapabilityError::ResourceExhausted)?;
    manager.delegate_capability(from_process, to_process, capability_id)
}

/// Revoke a capability from a process
pub fn revoke_capability(
    process_id: ProcessId,
    capability_id: CapabilityId,
) -> Result<(), CapabilityError> {
    let mut manager = CAPABILITY_MANAGER.lock();
    let manager = manager.as_mut().ok_or(CapabilityError::ResourceExhausted)?;
    manager.revoke_capability(process_id, capability_id)
}

/// Get capability system statistics
pub fn get_capability_statistics() -> CapabilityStatistics {
    let manager = CAPABILITY_MANAGER.lock();
    if let Some(manager) = manager.as_ref() {
        manager.get_statistics()
    } else {
        CapabilityStatistics {
            total_capabilities: 0,
            expired_capabilities: 0,
            processes_with_capabilities: 0,
            total_capabilities_created: 0,
            checks_performed: 0,
            checks_failed: 0,
        }
    }
}

/// Clean up expired capabilities
pub fn cleanup_expired_capabilities() -> usize {
    let mut manager = CAPABILITY_MANAGER.lock();
    if let Some(manager) = manager.as_mut() {
        manager.cleanup_expired_capabilities()
    } else {
        0
    }
}

/// Placeholder function for getting current time
/// TODO: Replace with actual timer implementation
fn get_current_time_ms() -> u64 {
    // For now, return 0. This will be replaced with actual timer implementation
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    
    #[test_case]
    fn test_capability_creation() {
        let process_id = ProcessId::new(1);
        let capability = Capability::new(
            CapabilityType::Read,
            ResourceId::File("test.txt".to_string()),
            process_id,
            None,
        );
        
        assert_eq!(capability.capability_type, CapabilityType::Read);
        assert_eq!(capability.owner, process_id);
        assert!(!capability.delegatable);
        assert!(!capability.is_expired());
    }
    
    #[test_case]
    fn test_capability_matching() {
        let process_id = ProcessId::new(1);
        let file_resource = ResourceId::File("test.txt".to_string());
        
        let capability = Capability::new(
            CapabilityType::Read,
            file_resource.clone(),
            process_id,
            None,
        );
        
        // Should match exact type and resource
        assert!(capability.matches(CapabilityType::Read, &file_resource));
        
        // Should not match different type
        assert!(!capability.matches(CapabilityType::Write, &file_resource));
        
        // Should not match different resource
        let other_resource = ResourceId::File("other.txt".to_string());
        assert!(!capability.matches(CapabilityType::Read, &other_resource));
    }
    
    #[test_case]
    fn test_capability_set() {
        let process_id = ProcessId::new(1);
        let mut capability_set = CapabilitySet::new();
        
        assert!(capability_set.is_empty());
        assert_eq!(capability_set.len(), 0);
        
        let capability = Capability::new(
            CapabilityType::Read,
            ResourceId::File("test.txt".to_string()),
            process_id,
            None,
        );
        
        let capability_id = capability.id;
        capability_set.add(capability);
        
        assert!(!capability_set.is_empty());
        assert_eq!(capability_set.len(), 1);
        
        // Test capability checking
        let file_resource = ResourceId::File("test.txt".to_string());
        assert!(capability_set.has_capability(CapabilityType::Read, &file_resource));
        assert!(!capability_set.has_capability(CapabilityType::Write, &file_resource));
        
        // Test removal
        let removed = capability_set.remove(capability_id);
        assert!(removed.is_some());
        assert!(capability_set.is_empty());
    }
    
    #[test_case]
    fn test_wildcard_capability() {
        let process_id = ProcessId::new(1);
        let wildcard_capability = Capability::new(
            CapabilityType::Read,
            ResourceId::Any,
            process_id,
            None,
        );
        
        // Wildcard should match any resource
        assert!(wildcard_capability.matches(CapabilityType::Read, &ResourceId::File("any.txt".to_string())));
        assert!(wildcard_capability.matches(CapabilityType::Read, &ResourceId::Device("any_device".to_string())));
        assert!(wildcard_capability.matches(CapabilityType::Read, &ResourceId::Process(ProcessId::new(42))));
        
        // But not different capability types
        assert!(!wildcard_capability.matches(CapabilityType::Write, &ResourceId::File("any.txt".to_string())));
    }
}