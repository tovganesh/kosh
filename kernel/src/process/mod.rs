pub mod process;
pub mod scheduler;
pub mod context;

#[cfg(test)]
pub mod tests;

pub use process::{
    Process, ProcessId, ProcessState, ProcessTable, ProcessError, ProcessPriority, ProcessInfo,
    create_process, get_process, remove_process, set_current_process, get_current_process,
    get_runnable_processes, get_process_statistics, print_process_table, cleanup_zombie_processes,
    init_process_table
};
pub use scheduler::{
    Scheduler, SchedulerError, SchedulingAlgorithm,
    schedule_next_process, handle_timer_tick, set_scheduling_algorithm, set_time_slice,
    get_scheduler_statistics, print_scheduler_info
};
pub use context::{CpuContext, ContextSwitcher, test_context_switching};

/// Process management initialization
pub fn init_process_management() -> Result<(), &'static str> {
    crate::serial_println!("Initializing process management...");
    
    // Initialize the global process table
    process::init_process_table()?;
    
    // Initialize the scheduler
    scheduler::init_scheduler()?;
    
    crate::serial_println!("Process management initialized successfully");
    Ok(())
}