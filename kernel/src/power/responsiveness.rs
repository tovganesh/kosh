//! Responsiveness Optimizations
//! 
//! Implements optimizations for interactive responsiveness including
//! touch input latency reduction and adaptive scheduling

use super::{ProcessActivity, PowerError};
use crate::process::{ProcessId, ProcessPriority};
use alloc::collections::BTreeMap;
use spin::Mutex;

/// Touch input event types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TouchEvent {
    /// Touch down event
    TouchDown { x: u16, y: u16 },
    /// Touch move event
    TouchMove { x: u16, y: u16 },
    /// Touch up event
    TouchUp { x: u16, y: u16 },
    /// Multi-touch gesture
    Gesture { gesture_type: GestureType },
}

/// Gesture types for touch input
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GestureType {
    /// Pinch to zoom
    Pinch,
    /// Swipe gesture
    Swipe { direction: SwipeDirection },
    /// Tap gesture
    Tap,
    /// Long press
    LongPress,
}

/// Swipe directions
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Interactive boost configuration
#[derive(Debug, Clone, Copy)]
pub struct InteractiveBoostConfig {
    /// Duration of interactive boost in milliseconds
    pub boost_duration_ms: u64,
    /// CPU frequency boost percentage (0-100)
    pub cpu_boost_percent: u8,
    /// Priority boost levels
    pub priority_boost_levels: u8,
    /// Time slice multiplier during boost
    pub time_slice_multiplier: f32,
}

impl Default for InteractiveBoostConfig {
    fn default() -> Self {
        Self {
            boost_duration_ms: 100,
            cpu_boost_percent: 20,
            priority_boost_levels: 1,
            time_slice_multiplier: 1.5,
        }
    }
}

/// Touch input latency optimization settings
#[derive(Debug, Clone, Copy)]
pub struct TouchLatencyConfig {
    /// Maximum acceptable touch latency in microseconds
    pub max_latency_us: u32,
    /// Touch input polling rate in Hz
    pub polling_rate_hz: u32,
    /// Enable touch prediction
    pub enable_prediction: bool,
    /// Touch prediction lookahead in milliseconds
    pub prediction_lookahead_ms: u8,
}

impl Default for TouchLatencyConfig {
    fn default() -> Self {
        Self {
            max_latency_us: 16000, // 16ms for 60fps
            polling_rate_hz: 120,   // 120Hz polling
            enable_prediction: true,
            prediction_lookahead_ms: 8,
        }
    }
}

/// Adaptive scheduling configuration
#[derive(Debug, Clone, Copy)]
pub struct AdaptiveSchedulingConfig {
    /// Base time slice in milliseconds
    pub base_time_slice_ms: u64,
    /// Minimum time slice in milliseconds
    pub min_time_slice_ms: u64,
    /// Maximum time slice in milliseconds
    pub max_time_slice_ms: u64,
    /// Load threshold for time slice adjustment
    pub load_threshold_percent: u8,
    /// Interactive process detection window in milliseconds
    pub interactive_window_ms: u64,
}

impl Default for AdaptiveSchedulingConfig {
    fn default() -> Self {
        Self {
            base_time_slice_ms: 10,
            min_time_slice_ms: 1,
            max_time_slice_ms: 50,
            load_threshold_percent: 70,
            interactive_window_ms: 500,
        }
    }
}

/// Background task throttling configuration
#[derive(Debug, Clone, Copy)]
pub struct ThrottlingConfig {
    /// Enable background task throttling
    pub enable_throttling: bool,
    /// CPU usage threshold to start throttling (0-100)
    pub cpu_threshold_percent: u8,
    /// Memory usage threshold to start throttling (0-100)
    pub memory_threshold_percent: u8,
    /// Throttling factor (0.0-1.0, lower means more aggressive)
    pub throttling_factor: f32,
    /// Minimum time slice for throttled processes
    pub min_throttled_time_slice_ms: u64,
}

impl Default for ThrottlingConfig {
    fn default() -> Self {
        Self {
            enable_throttling: true,
            cpu_threshold_percent: 80,
            memory_threshold_percent: 85,
            throttling_factor: 0.3,
            min_throttled_time_slice_ms: 1,
        }
    }
}

/// Process interaction tracking
#[derive(Debug, Clone)]
struct ProcessInteraction {
    last_interactive_time: u64,
    interaction_count: u32,
    touch_events_handled: u32,
    average_response_time_us: u32,
}

/// Responsiveness optimizer
pub struct ResponsivenessOptimizer {
    interactive_boost_config: InteractiveBoostConfig,
    touch_latency_config: TouchLatencyConfig,
    adaptive_scheduling_config: AdaptiveSchedulingConfig,
    throttling_config: ThrottlingConfig,
    process_interactions: BTreeMap<ProcessId, ProcessInteraction>,
    current_interactive_processes: BTreeMap<ProcessId, u64>, // PID -> boost end time
    touch_input_queue: alloc::vec::Vec<(TouchEvent, u64)>, // Event and timestamp
    system_load_percent: u8,
    memory_usage_percent: u8,
    last_update_time: u64,
}

impl ResponsivenessOptimizer {
    /// Create new responsiveness optimizer
    pub fn new() -> Self {
        Self {
            interactive_boost_config: InteractiveBoostConfig::default(),
            touch_latency_config: TouchLatencyConfig::default(),
            adaptive_scheduling_config: AdaptiveSchedulingConfig::default(),
            throttling_config: ThrottlingConfig::default(),
            process_interactions: BTreeMap::new(),
            current_interactive_processes: BTreeMap::new(),
            touch_input_queue: alloc::vec::Vec::new(),
            system_load_percent: 0,
            memory_usage_percent: 0,
            last_update_time: 0,
        }
    }

    /// Initialize responsiveness optimizations
    pub fn init(&mut self) -> Result<(), PowerError> {
        // In a real implementation, this would:
        // 1. Configure hardware touch controllers
        // 2. Set up high-priority interrupt handlers
        // 3. Initialize touch prediction algorithms
        // 4. Configure adaptive scheduling parameters
        
        Ok(())
    }

    /// Handle touch input event with latency optimization
    pub fn handle_touch_event(&mut self, event: TouchEvent, timestamp: u64) -> Result<(), PowerError> {
        // Add to touch input queue for processing
        self.touch_input_queue.push((event, timestamp));
        
        // Limit queue size to prevent memory issues
        if self.touch_input_queue.len() > 100 {
            self.touch_input_queue.remove(0);
        }
        
        // Trigger interactive boost for touch events
        self.trigger_interactive_boost(timestamp);
        
        // Process touch event immediately for low latency
        self.process_touch_event_immediate(event, timestamp)?;
        
        Ok(())
    }

    /// Process touch event with minimal latency
    fn process_touch_event_immediate(&mut self, event: TouchEvent, timestamp: u64) -> Result<(), PowerError> {
        // Find processes that should handle this touch event
        let interactive_processes = self.find_touch_handlers();
        
        for pid in interactive_processes {
            // Boost process priority for immediate response
            self.boost_process_for_touch(pid, timestamp);
            
            // Update interaction tracking
            self.update_process_interaction(pid, timestamp);
        }
        
        // If touch prediction is enabled, predict next touch position
        if self.touch_latency_config.enable_prediction {
            self.predict_next_touch(event, timestamp);
        }
        
        Ok(())
    }

    /// Trigger interactive boost for responsive UI
    fn trigger_interactive_boost(&mut self, current_time: u64) {
        let boost_end_time = current_time + self.interactive_boost_config.boost_duration_ms;
        
        // Find all interactive processes and boost them
        for (pid, interaction) in &self.process_interactions {
            if current_time.saturating_sub(interaction.last_interactive_time) < 
               self.interactive_boost_config.boost_duration_ms {
                self.current_interactive_processes.insert(*pid, boost_end_time);
            }
        }
    }

    /// Get adaptive time slice for a process
    pub fn get_adaptive_time_slice(&self, pid: ProcessId, base_time_slice: u64, current_time: u64) -> u64 {
        // Check if process is currently boosted
        if let Some(&boost_end_time) = self.current_interactive_processes.get(&pid) {
            if current_time < boost_end_time {
                // Apply interactive boost multiplier
                return ((base_time_slice as f32) * self.interactive_boost_config.time_slice_multiplier) as u64;
            }
        }
        
        // Check if process should be throttled
        if self.should_throttle_process(pid) {
            let throttled_slice = ((base_time_slice as f32) * self.throttling_config.throttling_factor) as u64;
            return throttled_slice.max(self.throttling_config.min_throttled_time_slice_ms);
        }
        
        // Adaptive time slice based on system load
        let load_factor = if self.system_load_percent > self.adaptive_scheduling_config.load_threshold_percent {
            // High load: reduce time slices for better responsiveness
            0.7
        } else {
            // Normal load: use standard time slices
            1.0
        };
        
        let adaptive_slice = ((base_time_slice as f32) * load_factor) as u64;
        adaptive_slice.clamp(
            self.adaptive_scheduling_config.min_time_slice_ms,
            self.adaptive_scheduling_config.max_time_slice_ms
        )
    }

    /// Check if a process should be throttled
    pub fn should_throttle_process(&self, pid: ProcessId) -> bool {
        if !self.throttling_config.enable_throttling {
            return false;
        }
        
        // Don't throttle interactive processes
        if self.current_interactive_processes.contains_key(&pid) {
            return false;
        }
        
        // Don't throttle if system load is low
        if self.system_load_percent < self.throttling_config.cpu_threshold_percent &&
           self.memory_usage_percent < self.throttling_config.memory_threshold_percent {
            return false;
        }
        
        // Check if process is classified as background
        if let Some(interaction) = self.process_interactions.get(&pid) {
            // Throttle processes with low interaction
            interaction.interaction_count < 5
        } else {
            // Throttle unknown processes when system is under pressure
            true
        }
    }

    /// Update system load and memory usage
    pub fn update_system_metrics(&mut self, cpu_load_percent: u8, memory_usage_percent: u8, current_time: u64) {
        self.system_load_percent = cpu_load_percent;
        self.memory_usage_percent = memory_usage_percent;
        self.last_update_time = current_time;
        
        // Clean up expired interactive boosts
        self.current_interactive_processes.retain(|_, &mut boost_end_time| {
            current_time < boost_end_time
        });
    }

    /// Notify of process activity for responsiveness tracking
    pub fn notify_process_activity(&mut self, pid: ProcessId, activity: ProcessActivity, current_time: u64) {
        match activity {
            ProcessActivity::Interactive => {
                self.update_process_interaction(pid, current_time);
                
                // Extend interactive boost if already active
                if let Some(boost_end_time) = self.current_interactive_processes.get_mut(&pid) {
                    *boost_end_time = current_time + self.interactive_boost_config.boost_duration_ms;
                } else {
                    // Start new interactive boost
                    self.current_interactive_processes.insert(
                        pid, 
                        current_time + self.interactive_boost_config.boost_duration_ms
                    );
                }
            }
            _ => {
                // Update interaction tracking for non-interactive activities
                if let Some(interaction) = self.process_interactions.get_mut(&pid) {
                    // Decay interaction count for non-interactive activities
                    interaction.interaction_count = interaction.interaction_count.saturating_sub(1);
                }
            }
        }
    }

    /// Get responsiveness statistics
    pub fn get_statistics(&self) -> ResponsivenessStats {
        let total_interactive_processes = self.current_interactive_processes.len();
        let total_tracked_processes = self.process_interactions.len();
        let average_response_time = self.calculate_average_response_time();
        
        ResponsivenessStats {
            interactive_processes_count: total_interactive_processes,
            tracked_processes_count: total_tracked_processes,
            average_response_time_us: average_response_time,
            system_load_percent: self.system_load_percent,
            memory_usage_percent: self.memory_usage_percent,
            touch_events_queued: self.touch_input_queue.len(),
            throttled_processes_count: self.count_throttled_processes(),
        }
    }

    // Private helper methods

    fn find_touch_handlers(&self) -> alloc::vec::Vec<ProcessId> {
        // In a real implementation, this would identify processes that handle touch input
        // For now, return interactive processes
        self.current_interactive_processes.keys().copied().collect()
    }

    fn boost_process_for_touch(&mut self, pid: ProcessId, timestamp: u64) {
        // Boost process for immediate touch response
        let boost_end_time = timestamp + self.interactive_boost_config.boost_duration_ms;
        self.current_interactive_processes.insert(pid, boost_end_time);
    }

    fn update_process_interaction(&mut self, pid: ProcessId, timestamp: u64) {
        let interaction = self.process_interactions.entry(pid).or_insert(ProcessInteraction {
            last_interactive_time: timestamp,
            interaction_count: 0,
            touch_events_handled: 0,
            average_response_time_us: 0,
        });
        
        interaction.last_interactive_time = timestamp;
        interaction.interaction_count += 1;
        interaction.touch_events_handled += 1;
    }

    fn predict_next_touch(&mut self, _event: TouchEvent, _timestamp: u64) {
        // In a real implementation, this would use machine learning or
        // simple prediction algorithms to anticipate next touch position
        // For now, this is a placeholder
    }

    fn calculate_average_response_time(&self) -> u32 {
        if self.process_interactions.is_empty() {
            return 0;
        }
        
        let total_response_time: u32 = self.process_interactions
            .values()
            .map(|interaction| interaction.average_response_time_us)
            .sum();
        
        total_response_time / self.process_interactions.len() as u32
    }

    fn count_throttled_processes(&self) -> usize {
        self.process_interactions
            .keys()
            .filter(|&&pid| self.should_throttle_process(pid))
            .count()
    }
}

/// Responsiveness statistics
#[derive(Debug, Clone)]
pub struct ResponsivenessStats {
    pub interactive_processes_count: usize,
    pub tracked_processes_count: usize,
    pub average_response_time_us: u32,
    pub system_load_percent: u8,
    pub memory_usage_percent: u8,
    pub touch_events_queued: usize,
    pub throttled_processes_count: usize,
}

/// Global responsiveness optimizer
static RESPONSIVENESS_OPTIMIZER: Mutex<Option<ResponsivenessOptimizer>> = Mutex::new(None);

/// Initialize responsiveness optimizations
pub fn init() -> Result<(), PowerError> {
    let mut optimizer = ResponsivenessOptimizer::new();
    optimizer.init()?;
    *RESPONSIVENESS_OPTIMIZER.lock() = Some(optimizer);
    Ok(())
}

/// Handle touch input event
pub fn handle_touch_event(event: TouchEvent, timestamp: u64) -> Result<(), PowerError> {
    if let Some(ref mut optimizer) = RESPONSIVENESS_OPTIMIZER.lock().as_mut() {
        optimizer.handle_touch_event(event, timestamp)
    } else {
        Err(PowerError::NotSupported)
    }
}

/// Get adaptive time slice for process
pub fn get_adaptive_time_slice(pid: ProcessId, base_time_slice: u64, current_time: u64) -> u64 {
    if let Some(ref optimizer) = RESPONSIVENESS_OPTIMIZER.lock().as_ref() {
        optimizer.get_adaptive_time_slice(pid, base_time_slice, current_time)
    } else {
        base_time_slice
    }
}

/// Check if process should be throttled
pub fn should_throttle_process(pid: ProcessId) -> bool {
    if let Some(ref optimizer) = RESPONSIVENESS_OPTIMIZER.lock().as_ref() {
        optimizer.should_throttle_process(pid)
    } else {
        false
    }
}

/// Update system metrics
pub fn update_system_metrics(cpu_load_percent: u8, memory_usage_percent: u8, current_time: u64) {
    if let Some(ref mut optimizer) = RESPONSIVENESS_OPTIMIZER.lock().as_mut() {
        optimizer.update_system_metrics(cpu_load_percent, memory_usage_percent, current_time);
    }
}

/// Notify of process activity
pub fn notify_process_activity(pid: ProcessId, activity: ProcessActivity, current_time: u64) {
    if let Some(ref mut optimizer) = RESPONSIVENESS_OPTIMIZER.lock().as_mut() {
        optimizer.notify_process_activity(pid, activity, current_time);
    }
}

/// Get responsiveness statistics
pub fn get_statistics() -> Option<ResponsivenessStats> {
    if let Some(ref optimizer) = RESPONSIVENESS_OPTIMIZER.lock().as_ref() {
        Some(optimizer.get_statistics())
    } else {
        None
    }
}