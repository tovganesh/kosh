//! Idle State Management
//! 
//! Manages CPU idle states for power saving when the system is not busy

use super::{PowerError, ProcessActivity};
use crate::process::ProcessId;
use alloc::collections::BTreeMap;
use spin::Mutex;

/// CPU idle states (C-states)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IdleState {
    /// CPU running normally
    C0,
    /// CPU halted, caches and TLBs maintained
    C1,
    /// CPU powered down, caches flushed
    C2,
    /// CPU and caches powered down
    C3,
    /// Deepest sleep state
    C4,
}

/// Idle state information
#[derive(Debug, Clone, Copy)]
pub struct IdleStateInfo {
    pub state: IdleState,
    pub entry_latency_us: u32,
    pub exit_latency_us: u32,
    pub power_consumption_mw: u32,
    pub available: bool,
}

/// Idle management statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct IdleStats {
    pub time_in_c0: u64,
    pub time_in_c1: u64,
    pub time_in_c2: u64,
    pub time_in_c3: u64,
    pub time_in_c4: u64,
    pub total_idle_entries: u64,
    pub total_idle_time: u64,
}

/// Idle state manager
pub struct IdleManager {
    current_state: IdleState,
    available_states: [IdleStateInfo; 5],
    stats: IdleStats,
    last_activity_time: u64,
    idle_threshold_ms: u64,
    deep_idle_threshold_ms: u64,
    active_processes: BTreeMap<ProcessId, ProcessActivity>,
    prevent_deep_idle: bool,
}

impl IdleManager {
    /// Create new idle manager
    pub fn new() -> Self {
        Self {
            current_state: IdleState::C0,
            available_states: [
                IdleStateInfo {
                    state: IdleState::C0,
                    entry_latency_us: 0,
                    exit_latency_us: 0,
                    power_consumption_mw: 1000,
                    available: true,
                },
                IdleStateInfo {
                    state: IdleState::C1,
                    entry_latency_us: 1,
                    exit_latency_us: 1,
                    power_consumption_mw: 500,
                    available: true,
                },
                IdleStateInfo {
                    state: IdleState::C2,
                    entry_latency_us: 10,
                    exit_latency_us: 20,
                    power_consumption_mw: 200,
                    available: true,
                },
                IdleStateInfo {
                    state: IdleState::C3,
                    entry_latency_us: 50,
                    exit_latency_us: 100,
                    power_consumption_mw: 50,
                    available: true,
                },
                IdleStateInfo {
                    state: IdleState::C4,
                    entry_latency_us: 200,
                    exit_latency_us: 500,
                    power_consumption_mw: 10,
                    available: false, // Disabled by default
                },
            ],
            stats: IdleStats::default(),
            last_activity_time: 0,
            idle_threshold_ms: 10,      // Enter idle after 10ms of inactivity
            deep_idle_threshold_ms: 100, // Enter deep idle after 100ms
            active_processes: BTreeMap::new(),
            prevent_deep_idle: false,
        }
    }

    /// Initialize idle state management
    pub fn init(&mut self) -> Result<(), PowerError> {
        // In a real implementation, this would:
        // 1. Detect available CPU idle states from ACPI
        // 2. Configure idle state parameters
        // 3. Set up idle state transition mechanisms
        
        // Enable C4 state if supported by hardware
        self.available_states[4].available = self.detect_deep_idle_support();
        
        Ok(())
    }

    /// Enter appropriate idle state based on system conditions
    pub fn enter_idle(&mut self, current_time: u64) -> Result<IdleState, PowerError> {
        let idle_duration = current_time.saturating_sub(self.last_activity_time);
        
        // Determine appropriate idle state
        let target_state = self.select_idle_state(idle_duration)?;
        
        if target_state != self.current_state {
            self.transition_to_state(target_state, current_time)?;
        }
        
        Ok(target_state)
    }

    /// Exit idle state due to activity
    pub fn exit_idle(&mut self, current_time: u64) -> Result<(), PowerError> {
        if self.current_state != IdleState::C0 {
            let idle_time = current_time.saturating_sub(self.last_activity_time);
            self.update_idle_stats(idle_time);
            
            self.transition_to_state(IdleState::C0, current_time)?;
        }
        
        self.last_activity_time = current_time;
        Ok(())
    }

    /// Notify of process activity
    pub fn notify_process_activity(&mut self, pid: ProcessId, activity: ProcessActivity, current_time: u64) {
        self.active_processes.insert(pid, activity);
        self.last_activity_time = current_time;
        
        // Prevent deep idle for interactive processes
        self.prevent_deep_idle = self.active_processes
            .values()
            .any(|&act| matches!(act, ProcessActivity::Interactive));
    }

    /// Remove process from activity tracking
    pub fn remove_process(&mut self, pid: ProcessId) {
        self.active_processes.remove(&pid);
        
        // Update deep idle prevention
        self.prevent_deep_idle = self.active_processes
            .values()
            .any(|&act| matches!(act, ProcessActivity::Interactive));
    }

    /// Get current idle state
    pub fn get_current_state(&self) -> IdleState {
        self.current_state
    }

    /// Get idle statistics
    pub fn get_stats(&self) -> IdleStats {
        self.stats
    }

    /// Get available idle states
    pub fn get_available_states(&self) -> &[IdleStateInfo; 5] {
        &self.available_states
    }

    /// Set idle thresholds
    pub fn set_thresholds(&mut self, idle_threshold_ms: u64, deep_idle_threshold_ms: u64) {
        self.idle_threshold_ms = idle_threshold_ms;
        self.deep_idle_threshold_ms = deep_idle_threshold_ms;
    }

    // Private methods

    fn detect_deep_idle_support(&self) -> bool {
        // In a real implementation, this would check ACPI tables
        // For now, assume deep idle is supported
        true
    }

    fn select_idle_state(&self, idle_duration_ms: u64) -> Result<IdleState, PowerError> {
        // Don't enter idle if there's recent activity
        if idle_duration_ms < self.idle_threshold_ms {
            return Ok(IdleState::C0);
        }

        // Check for interactive processes that need responsiveness
        let has_interactive = self.active_processes
            .values()
            .any(|&activity| matches!(activity, ProcessActivity::Interactive));

        let target_state = if has_interactive {
            // For interactive workloads, use shallow idle states
            if idle_duration_ms > 50 {
                IdleState::C2
            } else if idle_duration_ms > 20 {
                IdleState::C1
            } else {
                IdleState::C0
            }
        } else if self.prevent_deep_idle {
            // Prevent deep idle but allow moderate power saving
            if idle_duration_ms > self.deep_idle_threshold_ms {
                IdleState::C2
            } else if idle_duration_ms > 20 {
                IdleState::C1
            } else {
                IdleState::C0
            }
        } else {
            // Full idle state selection for background workloads
            if idle_duration_ms > 500 && self.available_states[4].available {
                IdleState::C4
            } else if idle_duration_ms > self.deep_idle_threshold_ms {
                IdleState::C3
            } else if idle_duration_ms > 50 {
                IdleState::C2
            } else if idle_duration_ms > 20 {
                IdleState::C1
            } else {
                IdleState::C0
            }
        };

        // Ensure the target state is available
        let state_index = target_state as usize;
        if self.available_states[state_index].available {
            Ok(target_state)
        } else {
            // Fall back to a shallower available state
            self.find_fallback_state(target_state)
        }
    }

    fn find_fallback_state(&self, requested_state: IdleState) -> Result<IdleState, PowerError> {
        let mut state_index = requested_state as usize;
        
        while state_index > 0 {
            state_index -= 1;
            if self.available_states[state_index].available {
                return Ok(match state_index {
                    0 => IdleState::C0,
                    1 => IdleState::C1,
                    2 => IdleState::C2,
                    3 => IdleState::C3,
                    4 => IdleState::C4,
                    _ => IdleState::C0,
                });
            }
        }
        
        Ok(IdleState::C0)
    }

    fn transition_to_state(&mut self, target_state: IdleState, current_time: u64) -> Result<(), PowerError> {
        let old_state = self.current_state;
        
        // In a real implementation, this would:
        // 1. Execute platform-specific idle instructions (HLT, MWAIT, etc.)
        // 2. Configure hardware power management
        // 3. Handle cache and TLB management
        
        self.current_state = target_state;
        self.stats.total_idle_entries += 1;
        
        // Simulate hardware idle state transition
        match target_state {
            IdleState::C0 => {
                // CPU active, no special handling needed
            }
            IdleState::C1 => {
                // Execute HLT instruction equivalent
                self.execute_halt();
            }
            IdleState::C2 => {
                // Deeper idle with cache management
                self.execute_deep_halt();
            }
            IdleState::C3 | IdleState::C4 => {
                // Very deep idle states
                self.execute_very_deep_halt();
            }
        }
        
        Ok(())
    }

    fn update_idle_stats(&mut self, idle_time: u64) {
        self.stats.total_idle_time += idle_time;
        
        match self.current_state {
            IdleState::C0 => self.stats.time_in_c0 += idle_time,
            IdleState::C1 => self.stats.time_in_c1 += idle_time,
            IdleState::C2 => self.stats.time_in_c2 += idle_time,
            IdleState::C3 => self.stats.time_in_c3 += idle_time,
            IdleState::C4 => self.stats.time_in_c4 += idle_time,
        }
    }

    fn execute_halt(&self) {
        // In a real implementation, this would execute the HLT instruction
        // For now, this is a placeholder
    }

    fn execute_deep_halt(&self) {
        // In a real implementation, this would execute deeper idle instructions
        // and manage cache coherency
    }

    fn execute_very_deep_halt(&self) {
        // In a real implementation, this would execute the deepest idle states
        // with full power management
    }
}

/// Global idle manager instance
static IDLE_MANAGER: Mutex<Option<IdleManager>> = Mutex::new(None);

/// Initialize idle state management
pub fn init() -> Result<(), PowerError> {
    let mut manager = IdleManager::new();
    manager.init()?;
    *IDLE_MANAGER.lock() = Some(manager);
    Ok(())
}

/// Enter idle state
pub fn enter_idle(current_time: u64) -> Result<IdleState, PowerError> {
    if let Some(ref mut manager) = IDLE_MANAGER.lock().as_mut() {
        manager.enter_idle(current_time)
    } else {
        Err(PowerError::NotSupported)
    }
}

/// Exit idle state
pub fn exit_idle(current_time: u64) -> Result<(), PowerError> {
    if let Some(ref mut manager) = IDLE_MANAGER.lock().as_mut() {
        manager.exit_idle(current_time)
    } else {
        Err(PowerError::NotSupported)
    }
}

/// Notify of process activity
pub fn notify_process_activity(pid: ProcessId, activity: ProcessActivity, current_time: u64) {
    if let Some(ref mut manager) = IDLE_MANAGER.lock().as_mut() {
        manager.notify_process_activity(pid, activity, current_time);
    }
}

/// Remove process from tracking
pub fn remove_process(pid: ProcessId) {
    if let Some(ref mut manager) = IDLE_MANAGER.lock().as_mut() {
        manager.remove_process(pid);
    }
}

/// Get current idle state
pub fn get_current_state() -> Result<IdleState, PowerError> {
    if let Some(ref manager) = IDLE_MANAGER.lock().as_ref() {
        Ok(manager.get_current_state())
    } else {
        Err(PowerError::NotSupported)
    }
}

/// Get idle statistics
pub fn get_stats() -> Result<IdleStats, PowerError> {
    if let Some(ref manager) = IDLE_MANAGER.lock().as_ref() {
        Ok(manager.get_stats())
    } else {
        Err(PowerError::NotSupported)
    }
}