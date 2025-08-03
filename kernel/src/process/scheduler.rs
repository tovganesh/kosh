use alloc::vec::Vec;
use spin::Mutex;
use crate::process::{ProcessId, ProcessPriority, get_runnable_processes, get_process, set_current_process, get_current_process};
use crate::process::context::{CpuContext, ContextSwitcher};
use crate::{serial_println, println};

/// Scheduler errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerError {
    /// No processes available to schedule
    NoProcessesAvailable,
    /// Scheduler not initialized
    NotInitialized,
    /// Invalid process for scheduling
    InvalidProcess,
}

/// Scheduling algorithm types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingAlgorithm {
    /// Round-robin scheduling
    RoundRobin,
    /// Priority-based scheduling
    Priority,
    /// Completely Fair Scheduler (CFS)
    CompletelyFair,
}

/// Scheduler statistics
#[derive(Debug, Clone)]
pub struct SchedulerStatistics {
    /// Total number of context switches
    pub context_switches: u64,
    /// Total scheduling decisions made
    pub scheduling_decisions: u64,
    /// Time spent in scheduler (microseconds)
    pub scheduler_time_us: u64,
    /// Current scheduling algorithm
    pub algorithm: SchedulingAlgorithm,
    /// Time slice duration in milliseconds
    pub time_slice_ms: u64,
}

/// Round-robin scheduler implementation
pub struct Scheduler {
    /// Current scheduling algorithm
    algorithm: SchedulingAlgorithm,
    /// Time slice for round-robin scheduling (in milliseconds)
    time_slice_ms: u64,
    /// Last scheduled process index for round-robin
    last_scheduled_index: usize,
    /// Scheduler statistics
    stats: SchedulerStatistics,
    /// Priority queues for priority-based scheduling
    priority_queues: [Vec<ProcessId>; 4], // One queue per priority level
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new(algorithm: SchedulingAlgorithm, time_slice_ms: u64) -> Self {
        Self {
            algorithm,
            time_slice_ms,
            last_scheduled_index: 0,
            stats: SchedulerStatistics {
                context_switches: 0,
                scheduling_decisions: 0,
                scheduler_time_us: 0,
                algorithm,
                time_slice_ms,
            },
            priority_queues: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        }
    }
    
    /// Schedule the next process to run
    pub fn schedule(&mut self) -> Result<Option<ProcessId>, SchedulerError> {
        let start_time = get_scheduler_time_us();
        self.stats.scheduling_decisions += 1;
        
        let next_process = match self.algorithm {
            SchedulingAlgorithm::RoundRobin => self.schedule_round_robin()?,
            SchedulingAlgorithm::Priority => self.schedule_priority()?,
            SchedulingAlgorithm::CompletelyFair => self.schedule_cfs()?,
        };
        
        // Update current process if we found one to schedule
        if let Some(pid) = next_process {
            let current = get_current_process();
            if current != Some(pid) {
                set_current_process(Some(pid))
                    .map_err(|_| SchedulerError::InvalidProcess)?;
                self.stats.context_switches += 1;
                
                serial_println!("Scheduled process {} (algorithm: {:?})", pid.0, self.algorithm);
            }
        } else {
            // No process to schedule, clear current process
            set_current_process(None)
                .map_err(|_| SchedulerError::InvalidProcess)?;
        }
        
        let end_time = get_scheduler_time_us();
        self.stats.scheduler_time_us += end_time - start_time;
        
        Ok(next_process)
    }
    
    /// Round-robin scheduling implementation
    fn schedule_round_robin(&mut self) -> Result<Option<ProcessId>, SchedulerError> {
        let runnable_processes = get_runnable_processes();
        
        if runnable_processes.is_empty() {
            return Ok(None);
        }
        
        // Find the next process to schedule in round-robin fashion
        let start_index = self.last_scheduled_index;
        let mut current_index = start_index;
        
        loop {
            if current_index >= runnable_processes.len() {
                current_index = 0;
            }
            
            let pid = runnable_processes[current_index];
            
            // Check if this process is still valid and runnable
            if let Some(process) = get_process(pid) {
                if process.is_runnable() {
                    self.last_scheduled_index = (current_index + 1) % runnable_processes.len();
                    return Ok(Some(pid));
                }
            }
            
            current_index += 1;
            if current_index == start_index {
                // We've checked all processes
                break;
            }
        }
        
        Ok(None)
    }
    
    /// Priority-based scheduling implementation
    fn schedule_priority(&mut self) -> Result<Option<ProcessId>, SchedulerError> {
        // Update priority queues
        self.update_priority_queues();
        
        // Schedule from highest priority queue first
        for priority_level in 0..4 {
            if !self.priority_queues[priority_level].is_empty() {
                // Use round-robin within the same priority level
                let queue = &mut self.priority_queues[priority_level];
                
                // Find a runnable process in this priority queue
                for i in 0..queue.len() {
                    let pid = queue[i];
                    if let Some(process) = get_process(pid) {
                        if process.is_runnable() {
                            // Move this process to the end of the queue for fairness
                            queue.remove(i);
                            queue.push(pid);
                            return Ok(Some(pid));
                        }
                    }
                }
            }
        }
        
        Ok(None)
    }
    
    /// Completely Fair Scheduler (CFS) implementation (simplified)
    fn schedule_cfs(&mut self) -> Result<Option<ProcessId>, SchedulerError> {
        let runnable_processes = get_runnable_processes();
        
        if runnable_processes.is_empty() {
            return Ok(None);
        }
        
        // For now, implement a simplified CFS that selects the process with least CPU time
        let mut best_process: Option<ProcessId> = None;
        let mut least_cpu_time = u64::MAX;
        
        for pid in runnable_processes {
            if let Some(process) = get_process(pid) {
                if process.is_runnable() && process.cpu_time_ms < least_cpu_time {
                    least_cpu_time = process.cpu_time_ms;
                    best_process = Some(pid);
                }
            }
        }
        
        Ok(best_process)
    }
    
    /// Update priority queues for priority-based scheduling
    fn update_priority_queues(&mut self) {
        // Clear existing queues
        for queue in &mut self.priority_queues {
            queue.clear();
        }
        
        // Populate queues with current runnable processes
        let runnable_processes = get_runnable_processes();
        
        for pid in runnable_processes {
            if let Some(process) = get_process(pid) {
                let priority_index = match process.priority {
                    ProcessPriority::System => 0,
                    ProcessPriority::Interactive => 1,
                    ProcessPriority::Normal => 2,
                    ProcessPriority::Background => 3,
                };
                
                self.priority_queues[priority_index].push(pid);
            }
        }
    }
    
    /// Get scheduler statistics
    pub fn get_statistics(&self) -> SchedulerStatistics {
        self.stats.clone()
    }
    
    /// Set scheduling algorithm
    pub fn set_algorithm(&mut self, algorithm: SchedulingAlgorithm) {
        serial_println!("Changing scheduling algorithm from {:?} to {:?}", 
                       self.algorithm, algorithm);
        self.algorithm = algorithm;
        self.stats.algorithm = algorithm;
        
        // Reset algorithm-specific state
        self.last_scheduled_index = 0;
        for queue in &mut self.priority_queues {
            queue.clear();
        }
    }
    
    /// Set time slice duration
    pub fn set_time_slice(&mut self, time_slice_ms: u64) {
        serial_println!("Changing time slice from {} ms to {} ms", 
                       self.time_slice_ms, time_slice_ms);
        self.time_slice_ms = time_slice_ms;
        self.stats.time_slice_ms = time_slice_ms;
    }
    
    /// Get current time slice
    pub fn get_time_slice(&self) -> u64 {
        self.time_slice_ms
    }
    
    /// Handle timer tick for preemptive scheduling
    pub fn timer_tick(&mut self) -> Result<bool, SchedulerError> {
        // For now, always trigger rescheduling on timer tick
        // In a real implementation, this would check if the current process
        // has exceeded its time slice
        
        let current_process = get_current_process();
        if current_process.is_some() {
            // Trigger rescheduling
            self.schedule()?;
            Ok(true) // Rescheduling occurred
        } else {
            Ok(false) // No rescheduling needed
        }
    }
    
    /// Print scheduler information
    pub fn print_info(&self) {
        serial_println!("Scheduler Information:");
        serial_println!("  Algorithm: {:?}", self.algorithm);
        serial_println!("  Time slice: {} ms", self.time_slice_ms);
        serial_println!("  Context switches: {}", self.stats.context_switches);
        serial_println!("  Scheduling decisions: {}", self.stats.scheduling_decisions);
        serial_println!("  Scheduler overhead: {} μs", self.stats.scheduler_time_us);
        
        if self.algorithm == SchedulingAlgorithm::Priority {
            serial_println!("  Priority queue sizes:");
            serial_println!("    System: {}", self.priority_queues[0].len());
            serial_println!("    Interactive: {}", self.priority_queues[1].len());
            serial_println!("    Normal: {}", self.priority_queues[2].len());
            serial_println!("    Background: {}", self.priority_queues[3].len());
        }
        
        println!("Scheduler: {:?} algorithm, {} context switches", 
                self.algorithm, self.stats.context_switches);
    }
}

/// Global scheduler instance
static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);

/// Default time slice in milliseconds
const DEFAULT_TIME_SLICE_MS: u64 = 10;

/// Initialize the global scheduler
pub fn init_scheduler() -> Result<(), &'static str> {
    serial_println!("Initializing scheduler...");
    
    let scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, DEFAULT_TIME_SLICE_MS);
    *SCHEDULER.lock() = Some(scheduler);
    
    serial_println!("Scheduler initialized with round-robin algorithm and {} ms time slice", 
                   DEFAULT_TIME_SLICE_MS);
    Ok(())
}

/// Schedule the next process
pub fn schedule_next_process() -> Result<Option<ProcessId>, SchedulerError> {
    let mut scheduler = SCHEDULER.lock();
    let scheduler = scheduler.as_mut().ok_or(SchedulerError::NotInitialized)?;
    scheduler.schedule()
}

/// Handle timer tick
pub fn handle_timer_tick() -> Result<bool, SchedulerError> {
    let mut scheduler = SCHEDULER.lock();
    let scheduler = scheduler.as_mut().ok_or(SchedulerError::NotInitialized)?;
    scheduler.timer_tick()
}

/// Set scheduling algorithm
pub fn set_scheduling_algorithm(algorithm: SchedulingAlgorithm) -> Result<(), SchedulerError> {
    let mut scheduler = SCHEDULER.lock();
    let scheduler = scheduler.as_mut().ok_or(SchedulerError::NotInitialized)?;
    scheduler.set_algorithm(algorithm);
    Ok(())
}

/// Set time slice duration
pub fn set_time_slice(time_slice_ms: u64) -> Result<(), SchedulerError> {
    let mut scheduler = SCHEDULER.lock();
    let scheduler = scheduler.as_mut().ok_or(SchedulerError::NotInitialized)?;
    scheduler.set_time_slice(time_slice_ms);
    Ok(())
}

/// Get scheduler statistics
pub fn get_scheduler_statistics() -> Option<SchedulerStatistics> {
    let scheduler = SCHEDULER.lock();
    scheduler.as_ref().map(|s| s.get_statistics())
}

/// Print scheduler information
pub fn print_scheduler_info() {
    let scheduler = SCHEDULER.lock();
    if let Some(scheduler) = scheduler.as_ref() {
        scheduler.print_info();
    } else {
        serial_println!("Scheduler not initialized");
    }
}

/// Placeholder function for getting scheduler time in microseconds
/// TODO: Replace with actual high-resolution timer implementation
fn get_scheduler_time_us() -> u64 {
    // For now, return 0. This will be replaced with actual timer implementation
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{init_process_table, create_process, ProcessPriority};
    use alloc::string::ToString;
    
    #[test_case]
    fn test_scheduler_creation() {
        let scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
        assert_eq!(scheduler.algorithm, SchedulingAlgorithm::RoundRobin);
        assert_eq!(scheduler.time_slice_ms, 10);
        assert_eq!(scheduler.stats.context_switches, 0);
        assert_eq!(scheduler.stats.scheduling_decisions, 0);
    }
    
    #[test_case]
    fn test_scheduler_algorithm_change() {
        let mut scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
        assert_eq!(scheduler.algorithm, SchedulingAlgorithm::RoundRobin);
        
        scheduler.set_algorithm(SchedulingAlgorithm::Priority);
        assert_eq!(scheduler.algorithm, SchedulingAlgorithm::Priority);
        assert_eq!(scheduler.stats.algorithm, SchedulingAlgorithm::Priority);
    }
    
    #[test_case]
    fn test_scheduler_time_slice_change() {
        let mut scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
        assert_eq!(scheduler.time_slice_ms, 10);
        
        scheduler.set_time_slice(20);
        assert_eq!(scheduler.time_slice_ms, 20);
        assert_eq!(scheduler.stats.time_slice_ms, 20);
    }
    
    #[test_case]
    fn test_scheduler_statistics() {
        let scheduler = Scheduler::new(SchedulingAlgorithm::RoundRobin, 10);
        let stats = scheduler.get_statistics();
        
        assert_eq!(stats.algorithm, SchedulingAlgorithm::RoundRobin);
        assert_eq!(stats.time_slice_ms, 10);
        assert_eq!(stats.context_switches, 0);
        assert_eq!(stats.scheduling_decisions, 0);
        assert_eq!(stats.scheduler_time_us, 0);
    }
}