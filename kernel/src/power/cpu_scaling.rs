//! CPU Frequency Scaling
//! 
//! Provides dynamic CPU frequency scaling for power management

use super::{CpuFrequency, CpuGovernor, PowerError, ProcessActivity};
use crate::process::ProcessId;
use alloc::collections::BTreeMap;
use spin::Mutex;

/// CPU frequency scaling manager
pub struct CpuScalingManager {
    current_governor: CpuGovernor,
    current_frequency: u32,
    min_frequency: u32,
    max_frequency: u32,
    load_history: [u8; 10], // Last 10 load measurements (0-100%)
    load_index: usize,
    process_activities: BTreeMap<ProcessId, ProcessActivity>,
    interactive_boost_active: bool,
    boost_end_time: u64, // Timestamp when boost should end
}

impl CpuScalingManager {
    /// Create new CPU scaling manager
    pub fn new() -> Self {
        Self {
            current_governor: CpuGovernor::OnDemand,
            current_frequency: 1000, // Default 1GHz
            min_frequency: 800,      // 800MHz min
            max_frequency: 2400,     // 2.4GHz max
            load_history: [0; 10],
            load_index: 0,
            process_activities: BTreeMap::new(),
            interactive_boost_active: false,
            boost_end_time: 0,
        }
    }

    /// Initialize CPU frequency scaling
    pub fn init(&mut self) -> Result<(), PowerError> {
        // In a real implementation, this would:
        // 1. Detect CPU frequency scaling capabilities
        // 2. Read available frequencies from hardware
        // 3. Set initial governor and frequency
        
        // For now, simulate successful initialization
        self.detect_frequency_range()?;
        self.set_frequency(self.current_frequency)?;
        Ok(())
    }

    /// Set CPU governor
    pub fn set_governor(&mut self, governor: CpuGovernor) -> Result<(), PowerError> {
        self.current_governor = governor;
        
        // Apply governor-specific settings
        match governor {
            CpuGovernor::Performance => {
                self.set_frequency(self.max_frequency)?;
            }
            CpuGovernor::PowerSave => {
                self.set_frequency(self.min_frequency)?;
            }
            CpuGovernor::OnDemand | CpuGovernor::Conservative | CpuGovernor::Interactive => {
                // These governors will adjust frequency dynamically
                // Initial frequency will be set based on current load when update_load is called
            }
        }
        
        Ok(())
    }

    /// Get current CPU frequency information
    pub fn get_frequency_info(&self) -> CpuFrequency {
        CpuFrequency {
            current_mhz: self.current_frequency,
            min_mhz: self.min_frequency,
            max_mhz: self.max_frequency,
        }
    }

    /// Update CPU load and adjust frequency if needed
    pub fn update_load(&mut self, load_percent: u8) -> Result<(), PowerError> {
        // Store load in circular buffer
        self.load_history[self.load_index] = load_percent;
        self.load_index = (self.load_index + 1) % self.load_history.len();

        // Update frequency based on current governor
        match self.current_governor {
            CpuGovernor::OnDemand => self.on_demand_scaling(load_percent)?,
            CpuGovernor::Conservative => self.conservative_scaling(load_percent)?,
            CpuGovernor::Interactive => self.interactive_scaling(load_percent)?,
            _ => {} // Performance and PowerSave don't adjust dynamically
        }

        Ok(())
    }

    /// Notify of process activity for interactive boost
    pub fn notify_process_activity(&mut self, pid: ProcessId, activity: ProcessActivity) {
        self.process_activities.insert(pid, activity);

        // Activate interactive boost for user-facing activities
        if matches!(activity, ProcessActivity::Interactive) {
            self.activate_interactive_boost();
        }
    }

    /// Remove process from activity tracking
    pub fn remove_process(&mut self, pid: ProcessId) {
        self.process_activities.remove(&pid);
    }

    /// Update frequency scaling (called periodically)
    pub fn tick(&mut self, current_time: u64) -> Result<(), PowerError> {
        // Check if interactive boost should end
        if self.interactive_boost_active && current_time >= self.boost_end_time {
            self.interactive_boost_active = false;
        }

        // Update frequency based on current conditions
        if self.current_governor == CpuGovernor::Interactive {
            let avg_load = self.get_average_load();
            self.interactive_scaling(avg_load)?;
        }

        Ok(())
    }

    // Private methods

    fn detect_frequency_range(&mut self) -> Result<(), PowerError> {
        // In a real implementation, this would read from hardware
        // For now, use default values
        Ok(())
    }

    fn set_frequency(&mut self, frequency_mhz: u32) -> Result<(), PowerError> {
        if frequency_mhz < self.min_frequency || frequency_mhz > self.max_frequency {
            return Err(PowerError::InvalidTransition);
        }

        // In a real implementation, this would write to hardware registers
        // or use ACPI/platform-specific interfaces
        self.current_frequency = frequency_mhz;
        Ok(())
    }

    fn on_demand_scaling(&mut self, load_percent: u8) -> Result<(), PowerError> {
        let target_frequency = if load_percent > 80 {
            self.max_frequency
        } else if load_percent > 50 {
            self.min_frequency + ((self.max_frequency - self.min_frequency) * load_percent as u32) / 100
        } else {
            self.min_frequency
        };

        self.set_frequency(target_frequency)
    }

    fn conservative_scaling(&mut self, load_percent: u8) -> Result<(), PowerError> {
        let step_size = (self.max_frequency - self.min_frequency) / 10;
        let current = self.current_frequency;

        let target_frequency = if load_percent > 75 && current < self.max_frequency {
            (current + step_size).min(self.max_frequency)
        } else if load_percent < 25 && current > self.min_frequency {
            (current - step_size).max(self.min_frequency)
        } else {
            current
        };

        self.set_frequency(target_frequency)
    }

    fn interactive_scaling(&mut self, load_percent: u8) -> Result<(), PowerError> {
        let has_interactive_processes = self.process_activities
            .values()
            .any(|&activity| matches!(activity, ProcessActivity::Interactive));

        let target_frequency = if self.interactive_boost_active || has_interactive_processes {
            // Boost frequency for interactive workloads
            if load_percent > 30 {
                self.max_frequency
            } else {
                self.min_frequency + (self.max_frequency - self.min_frequency) / 2
            }
        } else {
            // Standard scaling for background tasks
            if load_percent > 60 {
                self.max_frequency
            } else if load_percent > 30 {
                self.min_frequency + ((self.max_frequency - self.min_frequency) * load_percent as u32) / 100
            } else {
                self.min_frequency
            }
        };

        self.set_frequency(target_frequency)
    }

    fn activate_interactive_boost(&mut self) {
        self.interactive_boost_active = true;
        // Boost for 100ms (assuming tick is called every 10ms, this is 10 ticks)
        // In a real implementation, this would use actual time
        self.boost_end_time = 10; // Simplified for this implementation
    }

    fn get_average_load(&self) -> u8 {
        let sum: u32 = self.load_history.iter().map(|&x| x as u32).sum();
        (sum / self.load_history.len() as u32) as u8
    }
}

/// Global CPU scaling manager instance
static CPU_SCALING: Mutex<Option<CpuScalingManager>> = Mutex::new(None);

/// Initialize CPU frequency scaling
pub fn init() -> Result<(), PowerError> {
    let mut manager = CpuScalingManager::new();
    manager.init()?;
    *CPU_SCALING.lock() = Some(manager);
    Ok(())
}

/// Set CPU governor
pub fn set_governor(governor: CpuGovernor) -> Result<(), PowerError> {
    if let Some(ref mut manager) = CPU_SCALING.lock().as_mut() {
        manager.set_governor(governor)
    } else {
        Err(PowerError::NotSupported)
    }
}

/// Get CPU frequency information
pub fn get_frequency_info() -> Result<CpuFrequency, PowerError> {
    if let Some(ref manager) = CPU_SCALING.lock().as_ref() {
        Ok(manager.get_frequency_info())
    } else {
        Err(PowerError::FrequencyScalingUnavailable)
    }
}

/// Update CPU load
pub fn update_load(load_percent: u8) -> Result<(), PowerError> {
    if let Some(ref mut manager) = CPU_SCALING.lock().as_mut() {
        manager.update_load(load_percent)
    } else {
        Err(PowerError::NotSupported)
    }
}

/// Notify of process activity
pub fn notify_process_activity(pid: ProcessId, activity: ProcessActivity) {
    if let Some(ref mut manager) = CPU_SCALING.lock().as_mut() {
        manager.notify_process_activity(pid, activity);
    }
}

/// Remove process from tracking
pub fn remove_process(pid: ProcessId) {
    if let Some(ref mut manager) = CPU_SCALING.lock().as_mut() {
        manager.remove_process(pid);
    }
}

/// Periodic tick for frequency scaling updates
pub fn tick(current_time: u64) -> Result<(), PowerError> {
    if let Some(ref mut manager) = CPU_SCALING.lock().as_mut() {
        manager.tick(current_time)
    } else {
        Ok(())
    }
}