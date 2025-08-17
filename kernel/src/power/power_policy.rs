//! Power Management Policy
//! 
//! Implements power-aware scheduling policies and coordinates power management components

use super::{
    PowerState, PowerError, ProcessActivity, CpuGovernor,
    battery_monitor::{self, BatteryEvent},
    cpu_scaling, idle_management,
};
use crate::process::{ProcessId, ProcessPriority};
use alloc::collections::BTreeMap;
use spin::Mutex;

/// Power-aware scheduling policy
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SchedulingPolicy {
    /// Performance-oriented scheduling
    Performance,
    /// Balanced performance and power
    Balanced,
    /// Power-saving scheduling
    PowerSaver,
    /// Interactive-optimized scheduling
    Interactive,
    /// Critical battery mode
    Critical,
}

/// Process power classification
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessPowerClass {
    /// Critical system processes
    Critical,
    /// Interactive user-facing processes
    Interactive,
    /// Background services
    Background,
    /// Batch/compute processes
    Batch,
}

/// Power management policy engine
pub struct PowerPolicyManager {
    current_state: PowerState,
    current_policy: SchedulingPolicy,
    process_classifications: BTreeMap<ProcessId, ProcessPowerClass>,
    interactive_boost_active: bool,
    boost_end_time: u64,
    last_policy_update: u64,
    policy_update_interval: u64,
    thermal_throttling_active: bool,
}

impl PowerPolicyManager {
    /// Create new power policy manager
    pub fn new() -> Self {
        Self {
            current_state: PowerState::Balanced,
            current_policy: SchedulingPolicy::Balanced,
            process_classifications: BTreeMap::new(),
            interactive_boost_active: false,
            boost_end_time: 0,
            last_policy_update: 0,
            policy_update_interval: 1000, // Update every second
            thermal_throttling_active: false,
        }
    }

    /// Initialize power policy management
    pub fn init(&mut self) -> Result<(), PowerError> {
        // Register for battery events
        battery_monitor::register_callback(Self::handle_battery_event);
        
        // Set initial power state based on battery
        let recommended_state = battery_monitor::get_recommended_power_state();
        self.set_power_state(recommended_state)?;
        
        Ok(())
    }

    /// Set power state and update policies
    pub fn set_power_state(&mut self, state: PowerState) -> Result<(), PowerError> {
        if state == self.current_state {
            return Ok(());
        }

        let old_state = self.current_state;
        self.current_state = state;

        // Update scheduling policy based on power state
        self.current_policy = match state {
            PowerState::Performance => SchedulingPolicy::Performance,
            PowerState::Balanced => SchedulingPolicy::Balanced,
            PowerState::PowerSaver => SchedulingPolicy::PowerSaver,
            PowerState::Critical => SchedulingPolicy::Critical,
        };

        // Update CPU governor
        let governor = match state {
            PowerState::Performance => CpuGovernor::Performance,
            PowerState::Balanced => CpuGovernor::Interactive,
            PowerState::PowerSaver => CpuGovernor::PowerSave,
            PowerState::Critical => CpuGovernor::PowerSave,
        };
        cpu_scaling::set_governor(governor)?;

        // Adjust process priorities based on new policy
        self.apply_power_aware_scheduling()?;

        Ok(())
    }

    /// Get current power state
    pub fn get_power_state(&self) -> PowerState {
        self.current_state
    }

    /// Classify a process for power management
    pub fn classify_process(&mut self, pid: ProcessId, class: ProcessPowerClass) {
        self.process_classifications.insert(pid, class);
    }

    /// Remove process classification
    pub fn remove_process(&mut self, pid: ProcessId) {
        self.process_classifications.remove(&pid);
        cpu_scaling::remove_process(pid);
        idle_management::remove_process(pid);
    }

    /// Notify of process activity for power-aware decisions
    pub fn notify_process_activity(&mut self, pid: ProcessId, activity: ProcessActivity, current_time: u64) {
        // Update CPU scaling
        cpu_scaling::notify_process_activity(pid, activity);
        
        // Update idle management
        idle_management::notify_process_activity(pid, activity, current_time);

        // Handle interactive boost
        if matches!(activity, ProcessActivity::Interactive) {
            self.activate_interactive_boost(current_time);
        }
    }

    /// Update power management policies (called periodically)
    pub fn update(&mut self, current_time: u64) -> Result<(), PowerError> {
        if current_time.saturating_sub(self.last_policy_update) >= self.policy_update_interval {
            // Update battery monitoring
            battery_monitor::update(current_time)?;
            
            // Check if power state should change based on battery
            let recommended_state = battery_monitor::get_recommended_power_state();
            if recommended_state != self.current_state {
                self.set_power_state(recommended_state)?;
            }

            // Update CPU frequency scaling
            cpu_scaling::tick(current_time)?;

            // Check interactive boost timeout
            if self.interactive_boost_active && current_time >= self.boost_end_time {
                self.interactive_boost_active = false;
                self.apply_power_aware_scheduling()?;
            }

            self.last_policy_update = current_time;
        }

        Ok(())
    }

    /// Get power-aware priority for a process
    pub fn get_power_aware_priority(&self, pid: ProcessId, base_priority: ProcessPriority) -> ProcessPriority {
        let class = self.process_classifications.get(&pid)
            .copied()
            .unwrap_or(ProcessPowerClass::Background);

        match self.current_policy {
            SchedulingPolicy::Performance => {
                // In performance mode, don't modify priorities much
                base_priority
            }
            SchedulingPolicy::Interactive => {
                // Boost interactive processes
                match class {
                    ProcessPowerClass::Interactive => base_priority.boost(2),
                    ProcessPowerClass::Critical => base_priority,
                    _ => base_priority.reduce(1),
                }
            }
            SchedulingPolicy::Balanced => {
                // Moderate adjustments
                match class {
                    ProcessPowerClass::Critical => base_priority,
                    ProcessPowerClass::Interactive if self.interactive_boost_active => base_priority.boost(1),
                    ProcessPowerClass::Interactive => base_priority,
                    ProcessPowerClass::Background => base_priority.reduce(1),
                    ProcessPowerClass::Batch => base_priority.reduce(2),
                }
            }
            SchedulingPolicy::PowerSaver | SchedulingPolicy::Critical => {
                // Aggressive power saving
                match class {
                    ProcessPowerClass::Critical => base_priority,
                    ProcessPowerClass::Interactive => base_priority.reduce(1),
                    ProcessPowerClass::Background => base_priority.reduce(2),
                    ProcessPowerClass::Batch => base_priority.reduce(3),
                }
            }
        }
    }

    /// Get time slice adjustment for power management
    pub fn get_time_slice_multiplier(&self, pid: ProcessId) -> f32 {
        let class = self.process_classifications.get(&pid)
            .copied()
            .unwrap_or(ProcessPowerClass::Background);

        match self.current_policy {
            SchedulingPolicy::Performance => 1.0,
            SchedulingPolicy::Interactive => {
                match class {
                    ProcessPowerClass::Interactive => 1.5,
                    ProcessPowerClass::Critical => 1.0,
                    _ => 0.8,
                }
            }
            SchedulingPolicy::Balanced => {
                match class {
                    ProcessPowerClass::Critical => 1.0,
                    ProcessPowerClass::Interactive if self.interactive_boost_active => 1.2,
                    ProcessPowerClass::Interactive => 1.0,
                    ProcessPowerClass::Background => 0.8,
                    ProcessPowerClass::Batch => 0.6,
                }
            }
            SchedulingPolicy::PowerSaver | SchedulingPolicy::Critical => {
                match class {
                    ProcessPowerClass::Critical => 1.0,
                    ProcessPowerClass::Interactive => 0.8,
                    ProcessPowerClass::Background => 0.5,
                    ProcessPowerClass::Batch => 0.3,
                }
            }
        }
    }

    /// Check if background task throttling should be applied
    pub fn should_throttle_background(&self, pid: ProcessId) -> bool {
        let class = self.process_classifications.get(&pid)
            .copied()
            .unwrap_or(ProcessPowerClass::Background);

        match self.current_policy {
            SchedulingPolicy::Performance => false,
            SchedulingPolicy::Interactive => {
                matches!(class, ProcessPowerClass::Batch)
            }
            SchedulingPolicy::Balanced => {
                matches!(class, ProcessPowerClass::Batch) && !self.interactive_boost_active
            }
            SchedulingPolicy::PowerSaver | SchedulingPolicy::Critical => {
                matches!(class, ProcessPowerClass::Background | ProcessPowerClass::Batch)
            }
        }
    }

    /// Enable thermal throttling
    pub fn enable_thermal_throttling(&mut self) -> Result<(), PowerError> {
        self.thermal_throttling_active = true;
        
        // Force power saver mode during thermal throttling
        if self.current_state != PowerState::Critical {
            self.set_power_state(PowerState::PowerSaver)?;
        }
        
        Ok(())
    }

    /// Disable thermal throttling
    pub fn disable_thermal_throttling(&mut self) -> Result<(), PowerError> {
        self.thermal_throttling_active = false;
        
        // Restore normal power state based on battery
        let recommended_state = battery_monitor::get_recommended_power_state();
        self.set_power_state(recommended_state)?;
        
        Ok(())
    }

    // Private methods

    fn activate_interactive_boost(&mut self, current_time: u64) {
        self.interactive_boost_active = true;
        self.boost_end_time = current_time + 500; // 500ms boost
        
        // Apply immediate scheduling changes
        let _ = self.apply_power_aware_scheduling();
    }

    fn apply_power_aware_scheduling(&self) -> Result<(), PowerError> {
        // In a real implementation, this would update the scheduler
        // with new priorities and time slice adjustments
        Ok(())
    }

    fn handle_battery_event(event: BatteryEvent) {
        // This would be called by the battery monitor
        // In a real implementation, this would update power policies
        // based on battery events
        match event {
            BatteryEvent::CriticalLevel => {
                // Force critical power state
                if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
                    let _ = manager.set_power_state(PowerState::Critical);
                }
            }
            BatteryEvent::LowLevel => {
                // Switch to power saver mode
                if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
                    let _ = manager.set_power_state(PowerState::PowerSaver);
                }
            }
            BatteryEvent::ChargingStateChanged(charging) => {
                if charging {
                    // Can be more aggressive with performance when charging
                    if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
                        let _ = manager.set_power_state(PowerState::Performance);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Global power policy manager
static POWER_POLICY: Mutex<Option<PowerPolicyManager>> = Mutex::new(None);

/// Initialize power policy management
pub fn init() -> Result<(), PowerError> {
    let mut manager = PowerPolicyManager::new();
    manager.init()?;
    *POWER_POLICY.lock() = Some(manager);
    Ok(())
}

/// Set power state
pub fn set_power_state(state: PowerState) -> Result<(), PowerError> {
    if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
        manager.set_power_state(state)
    } else {
        Err(PowerError::NotSupported)
    }
}

/// Get current power state
pub fn get_power_state() -> PowerState {
    if let Some(ref manager) = POWER_POLICY.lock().as_ref() {
        manager.get_power_state()
    } else {
        PowerState::Balanced
    }
}

/// Classify process for power management
pub fn classify_process(pid: ProcessId, class: ProcessPowerClass) {
    if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
        manager.classify_process(pid, class);
    }
}

/// Remove process from power management
pub fn remove_process(pid: ProcessId) {
    if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
        manager.remove_process(pid);
    }
}

/// Notify of process activity
pub fn notify_process_activity(pid: ProcessId, activity: ProcessActivity, current_time: u64) {
    if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
        manager.notify_process_activity(pid, activity, current_time);
    }
}

/// Update power management (called periodically)
pub fn update(current_time: u64) -> Result<(), PowerError> {
    if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
        manager.update(current_time)
    } else {
        Ok(())
    }
}

/// Get power-aware priority for process
pub fn get_power_aware_priority(pid: ProcessId, base_priority: ProcessPriority) -> ProcessPriority {
    if let Some(ref manager) = POWER_POLICY.lock().as_ref() {
        manager.get_power_aware_priority(pid, base_priority)
    } else {
        base_priority
    }
}

/// Get time slice multiplier for process
pub fn get_time_slice_multiplier(pid: ProcessId) -> f32 {
    if let Some(ref manager) = POWER_POLICY.lock().as_ref() {
        manager.get_time_slice_multiplier(pid)
    } else {
        1.0
    }
}

/// Check if background task should be throttled
pub fn should_throttle_background(pid: ProcessId) -> bool {
    if let Some(ref manager) = POWER_POLICY.lock().as_ref() {
        manager.should_throttle_background(pid)
    } else {
        false
    }
}

/// Enable thermal throttling
pub fn enable_thermal_throttling() -> Result<(), PowerError> {
    if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
        manager.enable_thermal_throttling()
    } else {
        Err(PowerError::NotSupported)
    }
}

/// Disable thermal throttling
pub fn disable_thermal_throttling() -> Result<(), PowerError> {
    if let Some(ref mut manager) = POWER_POLICY.lock().as_mut() {
        manager.disable_thermal_throttling()
    } else {
        Err(PowerError::NotSupported)
    }
}