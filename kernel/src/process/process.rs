use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use crate::memory::vmm::VirtualAddressSpace;
use crate::process::context::CpuContext;
use crate::{serial_println, println};

/// Process identifier type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u32);

impl ProcessId {
    /// Create a new process ID
    pub fn new(id: u32) -> Self {
        ProcessId(id)
    }
    
    /// Get the raw ID value
    pub fn as_u32(&self) -> u32 {
        self.0
    }
    
    /// Special PID for the kernel process
    pub const KERNEL: ProcessId = ProcessId(0);
    
    /// Special PID for the init process
    pub const INIT: ProcessId = ProcessId(1);
}

/// Process state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Process is currently running on CPU
    Running,
    /// Process is ready to run but waiting for CPU time
    Ready,
    /// Process is blocked waiting for an event
    Blocked(BlockReason),
    /// Process has terminated but hasn't been cleaned up yet
    Zombie,
    /// Process is being created
    Creating,
}

/// Reasons why a process might be blocked
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// Waiting for I/O operation to complete
    WaitingForIo,
    /// Waiting for a message from another process
    WaitingForMessage,
    /// Waiting for a child process to terminate
    WaitingForChild,
    /// Waiting for memory allocation
    WaitingForMemory,
    /// Waiting for a system resource
    WaitingForResource,
}

/// Process priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcessPriority {
    /// Highest priority for system-critical processes
    System = 0,
    /// High priority for interactive processes
    Interactive = 1,
    /// Normal priority for regular processes
    Normal = 2,
    /// Low priority for background processes
    Background = 3,
}

impl Default for ProcessPriority {
    fn default() -> Self {
        ProcessPriority::Normal
    }
}

impl ProcessPriority {
    /// Boost priority by the specified number of levels
    pub fn boost(self, levels: u8) -> Self {
        let current_level = self as u8;
        let new_level = current_level.saturating_sub(levels);
        match new_level {
            0 => ProcessPriority::System,
            1 => ProcessPriority::Interactive,
            2 => ProcessPriority::Normal,
            _ => ProcessPriority::Background,
        }
    }

    /// Reduce priority by the specified number of levels
    pub fn reduce(self, levels: u8) -> Self {
        let current_level = self as u8;
        let new_level = (current_level + levels).min(3);
        match new_level {
            0 => ProcessPriority::System,
            1 => ProcessPriority::Interactive,
            2 => ProcessPriority::Normal,
            _ => ProcessPriority::Background,
        }
    }
}

/// Process control block containing all process information
#[derive(Debug)]
pub struct Process {
    /// Unique process identifier
    pub pid: ProcessId,
    /// Parent process identifier (None for kernel process)
    pub parent_pid: Option<ProcessId>,
    /// Current process state
    pub state: ProcessState,
    /// Process priority
    pub priority: ProcessPriority,
    /// Process name for debugging
    pub name: String,
    /// Virtual address space (None for kernel threads)
    pub address_space: Option<VirtualAddressSpace>,
    /// CPU context for context switching
    pub cpu_context: CpuContext,
    /// CPU time used by this process (in milliseconds)
    pub cpu_time_ms: u64,
    /// Time when process was created (in milliseconds since boot)
    pub creation_time_ms: u64,
    /// Time when process was last scheduled (in milliseconds since boot)
    pub last_scheduled_ms: u64,
    /// Exit code (valid only when state is Zombie)
    pub exit_code: Option<i32>,
    /// Child process IDs
    pub children: Vec<ProcessId>,
}

impl Process {
    /// Create a new process
    pub fn new(
        pid: ProcessId,
        parent_pid: Option<ProcessId>,
        name: String,
        priority: ProcessPriority,
    ) -> Self {
        let current_time = get_current_time_ms();
        
        Self {
            pid,
            parent_pid,
            state: ProcessState::Creating,
            priority,
            name,
            address_space: None,
            cpu_context: CpuContext::new(),
            cpu_time_ms: 0,
            creation_time_ms: current_time,
            last_scheduled_ms: current_time,
            exit_code: None,
            children: Vec::new(),
        }
    }
    
    /// Set the process state
    pub fn set_state(&mut self, new_state: ProcessState) {
        serial_println!("Process {} ({}) state change: {:?} -> {:?}", 
                       self.pid.0, self.name, self.state, new_state);
        self.state = new_state;
    }
    
    /// Check if the process is runnable (Ready or Running)
    pub fn is_runnable(&self) -> bool {
        matches!(self.state, ProcessState::Ready | ProcessState::Running)
    }
    
    /// Check if the process is terminated
    pub fn is_terminated(&self) -> bool {
        matches!(self.state, ProcessState::Zombie)
    }
    
    /// Add CPU time to this process
    pub fn add_cpu_time(&mut self, time_ms: u64) {
        self.cpu_time_ms += time_ms;
        self.last_scheduled_ms = get_current_time_ms();
    }
    
    /// Add a child process
    pub fn add_child(&mut self, child_pid: ProcessId) {
        if !self.children.contains(&child_pid) {
            self.children.push(child_pid);
        }
    }
    
    /// Remove a child process
    pub fn remove_child(&mut self, child_pid: ProcessId) {
        self.children.retain(|&pid| pid != child_pid);
    }
    
    /// Terminate the process with an exit code
    pub fn terminate(&mut self, exit_code: i32) {
        self.set_state(ProcessState::Zombie);
        self.exit_code = Some(exit_code);
        serial_println!("Process {} ({}) terminated with exit code {}", 
                       self.pid.0, self.name, exit_code);
    }
}

/// Process management errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    /// Process with given PID not found
    ProcessNotFound,
    /// Process table is full
    ProcessTableFull,
    /// Invalid process state transition
    InvalidStateTransition,
    /// Cannot perform operation on terminated process
    ProcessTerminated,
    /// Memory allocation failed
    OutOfMemory,
    /// Invalid process ID
    InvalidPid,
}

/// Process table for managing all processes in the system
pub struct ProcessTable {
    /// Vector of all processes (index is not necessarily PID)
    processes: Vec<Option<Process>>,
    /// Next available PID
    next_pid: u32,
    /// Currently running process PID
    current_pid: Option<ProcessId>,
    /// Maximum number of processes
    max_processes: usize,
}

impl ProcessTable {
    /// Create a new process table
    pub fn new(max_processes: usize) -> Self {
        Self {
            processes: Vec::with_capacity(max_processes),
            next_pid: 1, // PID 0 is reserved for kernel
            current_pid: None,
            max_processes,
        }
    }
    
    /// Create a new process and add it to the table
    pub fn create_process(
        &mut self,
        parent_pid: Option<ProcessId>,
        name: String,
        priority: ProcessPriority,
    ) -> Result<ProcessId, ProcessError> {
        // Check if we have space for a new process
        if self.processes.len() >= self.max_processes {
            return Err(ProcessError::ProcessTableFull);
        }
        
        // Allocate a new PID
        let pid = ProcessId::new(self.next_pid);
        self.next_pid += 1;
        
        // Create the new process
        let mut process = Process::new(pid, parent_pid, name, priority);
        process.set_state(ProcessState::Ready);
        
        // Add to parent's children list if parent exists
        if let Some(parent_pid) = parent_pid {
            if let Some(parent) = self.get_process_mut(parent_pid) {
                parent.add_child(pid);
            }
        }
        
        // Add to process table
        self.processes.push(Some(process));
        
        serial_println!("Created process {} with PID {}", 
                       self.processes.last().as_ref().unwrap().as_ref().unwrap().name, 
                       pid.0);
        
        Ok(pid)
    }
    
    /// Get a process by PID (immutable reference)
    pub fn get_process(&self, pid: ProcessId) -> Option<&Process> {
        self.processes.iter()
            .find_map(|p| p.as_ref().filter(|proc| proc.pid == pid))
    }
    
    /// Get a process by PID (mutable reference)
    pub fn get_process_mut(&mut self, pid: ProcessId) -> Option<&mut Process> {
        self.processes.iter_mut()
            .find_map(|p| p.as_mut().filter(|proc| proc.pid == pid))
    }
    
    /// Remove a process from the table
    pub fn remove_process(&mut self, pid: ProcessId) -> Result<Process, ProcessError> {
        // Find the process index
        let index = self.processes.iter()
            .position(|p| p.as_ref().map_or(false, |proc| proc.pid == pid))
            .ok_or(ProcessError::ProcessNotFound)?;
        
        // Remove the process
        let process = self.processes[index].take()
            .ok_or(ProcessError::ProcessNotFound)?;
        
        // Remove from parent's children list
        if let Some(parent_pid) = process.parent_pid {
            if let Some(parent) = self.get_process_mut(parent_pid) {
                parent.remove_child(pid);
            }
        }
        
        // Terminate all child processes
        let children: Vec<ProcessId> = process.children.clone();
        for child_pid in children {
            if let Some(child) = self.get_process_mut(child_pid) {
                child.terminate(-1); // Terminate with error code
            }
        }
        
        serial_println!("Removed process {} ({})", pid.0, process.name);
        Ok(process)
    }
    
    /// Get all processes in a specific state
    pub fn get_processes_by_state(&self, state: ProcessState) -> Vec<ProcessId> {
        self.processes.iter()
            .filter_map(|p| p.as_ref())
            .filter(|proc| proc.state == state)
            .map(|proc| proc.pid)
            .collect()
    }
    
    /// Get all runnable processes (Ready or Running)
    pub fn get_runnable_processes(&self) -> Vec<ProcessId> {
        self.processes.iter()
            .filter_map(|p| p.as_ref())
            .filter(|proc| proc.is_runnable())
            .map(|proc| proc.pid)
            .collect()
    }
    
    /// Get processes by priority
    pub fn get_processes_by_priority(&self, priority: ProcessPriority) -> Vec<ProcessId> {
        self.processes.iter()
            .filter_map(|p| p.as_ref())
            .filter(|proc| proc.priority == priority)
            .map(|proc| proc.pid)
            .collect()
    }
    
    /// Set the currently running process
    pub fn set_current_process(&mut self, pid: Option<ProcessId>) -> Result<(), ProcessError> {
        // Update previous current process state
        if let Some(current_pid) = self.current_pid {
            if let Some(current_proc) = self.get_process_mut(current_pid) {
                if current_proc.state == ProcessState::Running {
                    current_proc.set_state(ProcessState::Ready);
                }
            }
        }
        
        // Update new current process state
        if let Some(new_pid) = pid {
            if let Some(new_proc) = self.get_process_mut(new_pid) {
                new_proc.set_state(ProcessState::Running);
            } else {
                return Err(ProcessError::ProcessNotFound);
            }
        }
        
        self.current_pid = pid;
        Ok(())
    }
    
    /// Get the currently running process PID
    pub fn get_current_process(&self) -> Option<ProcessId> {
        self.current_pid
    }
    
    /// Get process count
    pub fn process_count(&self) -> usize {
        self.processes.iter().filter(|p| p.is_some()).count()
    }
    
    /// Get process statistics
    pub fn get_statistics(&self) -> ProcessTableStatistics {
        let total_processes = self.process_count();
        let mut state_counts = [0; 5]; // Creating, Ready, Running, Blocked, Zombie
        let mut priority_counts = [0; 4]; // System, Interactive, Normal, Background
        
        for process_opt in &self.processes {
            if let Some(process) = process_opt {
                // Count by state
                let state_index = match process.state {
                    ProcessState::Creating => 0,
                    ProcessState::Ready => 1,
                    ProcessState::Running => 2,
                    ProcessState::Blocked(_) => 3,
                    ProcessState::Zombie => 4,
                };
                state_counts[state_index] += 1;
                
                // Count by priority
                let priority_index = match process.priority {
                    ProcessPriority::System => 0,
                    ProcessPriority::Interactive => 1,
                    ProcessPriority::Normal => 2,
                    ProcessPriority::Background => 3,
                };
                priority_counts[priority_index] += 1;
            }
        }
        
        ProcessTableStatistics {
            total_processes,
            creating_processes: state_counts[0],
            ready_processes: state_counts[1],
            running_processes: state_counts[2],
            blocked_processes: state_counts[3],
            zombie_processes: state_counts[4],
            system_priority_processes: priority_counts[0],
            interactive_priority_processes: priority_counts[1],
            normal_priority_processes: priority_counts[2],
            background_priority_processes: priority_counts[3],
            current_pid: self.current_pid,
        }
    }
    
    /// Clean up zombie processes
    pub fn cleanup_zombies(&mut self) -> usize {
        let mut cleaned_count = 0;
        
        // Find zombie processes
        let zombie_pids: Vec<ProcessId> = self.get_processes_by_state(ProcessState::Zombie);
        
        for pid in zombie_pids {
            if self.remove_process(pid).is_ok() {
                cleaned_count += 1;
            }
        }
        
        if cleaned_count > 0 {
            serial_println!("Cleaned up {} zombie processes", cleaned_count);
        }
        
        cleaned_count
    }
}

/// Process table statistics
#[derive(Debug, Clone)]
pub struct ProcessTableStatistics {
    pub total_processes: usize,
    pub creating_processes: usize,
    pub ready_processes: usize,
    pub running_processes: usize,
    pub blocked_processes: usize,
    pub zombie_processes: usize,
    pub system_priority_processes: usize,
    pub interactive_priority_processes: usize,
    pub normal_priority_processes: usize,
    pub background_priority_processes: usize,
    pub current_pid: Option<ProcessId>,
}

/// Global process table
static PROCESS_TABLE: Mutex<Option<ProcessTable>> = Mutex::new(None);

/// Maximum number of processes in the system
const MAX_PROCESSES: usize = 1024;

/// Initialize the global process table
pub fn init_process_table() -> Result<(), &'static str> {
    serial_println!("Initializing process table...");
    
    let process_table = ProcessTable::new(MAX_PROCESSES);
    *PROCESS_TABLE.lock() = Some(process_table);
    
    serial_println!("Process table initialized with capacity for {} processes", MAX_PROCESSES);
    Ok(())
}

/// Create a new process
pub fn create_process(
    parent_pid: Option<ProcessId>,
    name: String,
    priority: ProcessPriority,
) -> Result<ProcessId, ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    let table = table.as_mut().ok_or(ProcessError::ProcessNotFound)?;
    table.create_process(parent_pid, name, priority)
}

/// Get a process by PID (returns a copy of basic process info)
pub fn get_process(pid: ProcessId) -> Option<ProcessInfo> {
    let table = PROCESS_TABLE.lock();
    let table = table.as_ref()?;
    table.get_process(pid).map(|p| ProcessInfo {
        pid: p.pid,
        parent_pid: p.parent_pid,
        state: p.state,
        priority: p.priority,
        name: p.name.clone(),
        cpu_time_ms: p.cpu_time_ms,
        creation_time_ms: p.creation_time_ms,
        last_scheduled_ms: p.last_scheduled_ms,
        exit_code: p.exit_code,
        children_count: p.children.len(),
    })
}

/// Lightweight process information structure for external access
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: ProcessId,
    pub parent_pid: Option<ProcessId>,
    pub state: ProcessState,
    pub priority: ProcessPriority,
    pub name: String,
    pub cpu_time_ms: u64,
    pub creation_time_ms: u64,
    pub last_scheduled_ms: u64,
    pub exit_code: Option<i32>,
    pub children_count: usize,
}

impl ProcessInfo {
    /// Check if the process is runnable (Ready or Running)
    pub fn is_runnable(&self) -> bool {
        matches!(self.state, ProcessState::Ready | ProcessState::Running)
    }
    
    /// Check if the process is terminated
    pub fn is_terminated(&self) -> bool {
        matches!(self.state, ProcessState::Zombie)
    }
}

/// Remove a process
pub fn remove_process(pid: ProcessId) -> Result<Process, ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    let table = table.as_mut().ok_or(ProcessError::ProcessNotFound)?;
    table.remove_process(pid)
}

/// Set the currently running process
pub fn set_current_process(pid: Option<ProcessId>) -> Result<(), ProcessError> {
    let mut table = PROCESS_TABLE.lock();
    let table = table.as_mut().ok_or(ProcessError::ProcessNotFound)?;
    table.set_current_process(pid)
}

/// Get the currently running process PID
pub fn get_current_process() -> Option<ProcessId> {
    let table = PROCESS_TABLE.lock();
    table.as_ref()?.get_current_process()
}

/// Get all runnable processes
pub fn get_runnable_processes() -> Vec<ProcessId> {
    let table = PROCESS_TABLE.lock();
    if let Some(table) = table.as_ref() {
        table.get_runnable_processes()
    } else {
        Vec::new()
    }
}

/// Get process table statistics
pub fn get_process_statistics() -> Option<ProcessTableStatistics> {
    let table = PROCESS_TABLE.lock();
    table.as_ref().map(|t| t.get_statistics())
}

/// Print process table information
pub fn print_process_table() {
    let table = PROCESS_TABLE.lock();
    if let Some(table) = table.as_ref() {
        let stats = table.get_statistics();
        
        serial_println!("Process Table Statistics:");
        serial_println!("  Total processes: {}", stats.total_processes);
        serial_println!("  Creating: {}, Ready: {}, Running: {}, Blocked: {}, Zombie: {}", 
                       stats.creating_processes, stats.ready_processes, 
                       stats.running_processes, stats.blocked_processes, stats.zombie_processes);
        serial_println!("  Priority distribution - System: {}, Interactive: {}, Normal: {}, Background: {}", 
                       stats.system_priority_processes, stats.interactive_priority_processes,
                       stats.normal_priority_processes, stats.background_priority_processes);
        
        if let Some(current_pid) = stats.current_pid {
            serial_println!("  Current process: {}", current_pid.0);
        } else {
            serial_println!("  No current process");
        }
        
        println!("Process table: {} processes active", stats.total_processes);
    } else {
        serial_println!("Process table not initialized");
    }
}

/// Clean up zombie processes
pub fn cleanup_zombie_processes() -> usize {
    let mut table = PROCESS_TABLE.lock();
    if let Some(table) = table.as_mut() {
        table.cleanup_zombies()
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
    fn test_process_id_creation() {
        let pid = ProcessId::new(42);
        assert_eq!(pid.as_u32(), 42);
        
        assert_eq!(ProcessId::KERNEL.as_u32(), 0);
        assert_eq!(ProcessId::INIT.as_u32(), 1);
    }
    
    #[test_case]
    fn test_process_creation() {
        let pid = ProcessId::new(1);
        let process = Process::new(
            pid,
            Some(ProcessId::KERNEL),
            "test_process".to_string(),
            ProcessPriority::Normal,
        );
        
        assert_eq!(process.pid, pid);
        assert_eq!(process.parent_pid, Some(ProcessId::KERNEL));
        assert_eq!(process.name, "test_process");
        assert_eq!(process.priority, ProcessPriority::Normal);
        assert_eq!(process.state, ProcessState::Creating);
        assert_eq!(process.cpu_time_ms, 0);
        assert!(process.children.is_empty());
    }
    
    #[test_case]
    fn test_process_state_transitions() {
        let mut process = Process::new(
            ProcessId::new(1),
            None,
            "test".to_string(),
            ProcessPriority::Normal,
        );
        
        assert_eq!(process.state, ProcessState::Creating);
        assert!(!process.is_runnable());
        assert!(!process.is_terminated());
        
        process.set_state(ProcessState::Ready);
        assert_eq!(process.state, ProcessState::Ready);
        assert!(process.is_runnable());
        assert!(!process.is_terminated());
        
        process.set_state(ProcessState::Running);
        assert_eq!(process.state, ProcessState::Running);
        assert!(process.is_runnable());
        assert!(!process.is_terminated());
        
        process.set_state(ProcessState::Blocked(BlockReason::WaitingForIo));
        assert_eq!(process.state, ProcessState::Blocked(BlockReason::WaitingForIo));
        assert!(!process.is_runnable());
        assert!(!process.is_terminated());
        
        process.terminate(0);
        assert_eq!(process.state, ProcessState::Zombie);
        assert_eq!(process.exit_code, Some(0));
        assert!(!process.is_runnable());
        assert!(process.is_terminated());
    }
    
    #[test_case]
    fn test_process_table_creation() {
        let mut table = ProcessTable::new(10);
        assert_eq!(table.process_count(), 0);
        assert_eq!(table.get_current_process(), None);
        
        let pid = table.create_process(
            None,
            "init".to_string(),
            ProcessPriority::System,
        ).unwrap();
        
        assert_eq!(table.process_count(), 1);
        assert_eq!(pid.as_u32(), 1);
        
        let process = table.get_process(pid).unwrap();
        assert_eq!(process.name, "init");
        assert_eq!(process.priority, ProcessPriority::System);
        assert_eq!(process.state, ProcessState::Ready);
    }
    
    #[test_case]
    fn test_process_table_parent_child_relationship() {
        let mut table = ProcessTable::new(10);
        
        let parent_pid = table.create_process(
            None,
            "parent".to_string(),
            ProcessPriority::Normal,
        ).unwrap();
        
        let child_pid = table.create_process(
            Some(parent_pid),
            "child".to_string(),
            ProcessPriority::Normal,
        ).unwrap();
        
        let parent = table.get_process(parent_pid).unwrap();
        assert!(parent.children.contains(&child_pid));
        
        let child = table.get_process(child_pid).unwrap();
        assert_eq!(child.parent_pid, Some(parent_pid));
    }
    
    #[test_case]
    fn test_process_table_statistics() {
        let mut table = ProcessTable::new(10);
        
        let _pid1 = table.create_process(None, "proc1".to_string(), ProcessPriority::System).unwrap();
        let _pid2 = table.create_process(None, "proc2".to_string(), ProcessPriority::Normal).unwrap();
        let pid3 = table.create_process(None, "proc3".to_string(), ProcessPriority::Background).unwrap();
        
        // Terminate one process
        if let Some(proc) = table.get_process_mut(pid3) {
            proc.terminate(0);
        }
        
        let stats = table.get_statistics();
        assert_eq!(stats.total_processes, 3);
        assert_eq!(stats.ready_processes, 2);
        assert_eq!(stats.zombie_processes, 1);
        assert_eq!(stats.system_priority_processes, 1);
        assert_eq!(stats.normal_priority_processes, 1);
        assert_eq!(stats.background_priority_processes, 1);
    }
}