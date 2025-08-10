#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec, string::String, boxed::Box, collections::VecDeque};
use kosh_driver::{
    KoshDriver, DriverInfo, DriverType, HardwareId, DriverStatus, PowerEvent,
    DriverRequest, DriverResponse, DriverCapabilityType, HardwareCapability
};
use kosh_types::{DriverError, Capability};
use spin::Mutex;
// use volatile::Volatile; // Not needed for this implementation
use bitflags::bitflags;

/// PS/2 keyboard controller ports
const PS2_DATA_PORT: u16 = 0x60;
const PS2_STATUS_PORT: u16 = 0x64;
const PS2_COMMAND_PORT: u16 = 0x64;

/// PS/2 status register bits
bitflags! {
    #[derive(Debug, Clone, Copy)]
    struct PS2Status: u8 {
        const OUTPUT_BUFFER_FULL = 1 << 0;
        const INPUT_BUFFER_FULL = 1 << 1;
        const SYSTEM_FLAG = 1 << 2;
        const COMMAND_DATA = 1 << 3;
        const KEYBOARD_LOCK = 1 << 4;
        const AUXILIARY_OUTPUT_BUFFER_FULL = 1 << 5;
        const TIMEOUT_ERROR = 1 << 6;
        const PARITY_ERROR = 1 << 7;
    }
}

/// Key codes for common keys
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyCode {
    // Letters
    A = 0x1E, B = 0x30, C = 0x2E, D = 0x20, E = 0x12, F = 0x21, G = 0x22,
    H = 0x23, I = 0x17, J = 0x24, K = 0x25, L = 0x26, M = 0x32, N = 0x31,
    O = 0x18, P = 0x19, Q = 0x10, R = 0x13, S = 0x1F, T = 0x14, U = 0x16,
    V = 0x2F, W = 0x11, X = 0x2D, Y = 0x15, Z = 0x2C,
    
    // Numbers
    Key0 = 0x0B, Key1 = 0x02, Key2 = 0x03, Key3 = 0x04, Key4 = 0x05,
    Key5 = 0x06, Key6 = 0x07, Key7 = 0x08, Key8 = 0x09, Key9 = 0x0A,
    
    // Function keys
    F1 = 0x3B, F2 = 0x3C, F3 = 0x3D, F4 = 0x3E, F5 = 0x3F, F6 = 0x40,
    F7 = 0x41, F8 = 0x42, F9 = 0x43, F10 = 0x44, F11 = 0x57, F12 = 0x58,
    
    // Special keys
    Escape = 0x01,
    Backspace = 0x0E,
    Tab = 0x0F,
    Enter = 0x1C,
    Space = 0x39,
    LeftShift = 0x2A,
    RightShift = 0x36,
    LeftCtrl = 0x1D,
    LeftAlt = 0x38,
    CapsLock = 0x3A,
    
    // Arrow keys (extended scancodes)
    ArrowUp = 0x48,
    ArrowDown = 0x50,
    ArrowLeft = 0x4B,
    ArrowRight = 0x4D,
    
    // Other keys
    Delete = 0x53,
    Home = 0x47,
    End = 0x4F,
    PageUp = 0x49,
    PageDown = 0x51,
    Insert = 0x52,
    
    // Unknown key
    Unknown = 0xFF,
}

/// Key event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEventType {
    KeyPress,
    KeyRelease,
}

/// Input event structure
#[derive(Debug, Clone)]
pub struct InputEvent {
    pub event_type: KeyEventType,
    pub key_code: KeyCode,
    pub scancode: u8,
    pub modifiers: KeyModifiers,
    pub ascii_char: Option<char>,
    pub timestamp: u64, // In a real implementation, this would be a proper timestamp
}

/// Key modifier flags
bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct KeyModifiers: u8 {
        const SHIFT = 1 << 0;
        const CTRL = 1 << 1;
        const ALT = 1 << 2;
        const CAPS_LOCK = 1 << 3;
        const NUM_LOCK = 1 << 4;
        const SCROLL_LOCK = 1 << 5;
    }
}

/// PS/2 keyboard driver implementation
pub struct PS2KeyboardDriver {
    status: DriverStatus,
    event_queue: VecDeque<InputEvent>,
    modifiers: KeyModifiers,
    extended_scancode: bool,
    max_queue_size: usize,
}

impl PS2KeyboardDriver {
    /// Create a new PS/2 keyboard driver instance
    pub fn new() -> Self {
        Self {
            status: DriverStatus::Uninitialized,
            event_queue: VecDeque::new(),
            modifiers: KeyModifiers::empty(),
            extended_scancode: false,
            max_queue_size: 256,
        }
    }

    /// Read a byte from the PS/2 data port
    fn read_data(&self) -> u8 {
        // In a real implementation, this would use proper I/O port access
        // For now, we'll simulate reading from the port
        unsafe {
            // This is a placeholder - in real implementation would use:
            // x86_64::instructions::port::Port::new(PS2_DATA_PORT).read()
            0
        }
    }

    /// Read the PS/2 status register
    fn read_status(&self) -> PS2Status {
        // In a real implementation, this would use proper I/O port access
        // For now, we'll simulate reading from the port
        unsafe {
            // This is a placeholder - in real implementation would use:
            // PS2Status::from_bits_truncate(x86_64::instructions::port::Port::new(PS2_STATUS_PORT).read())
            PS2Status::empty()
        }
    }

    /// Write a command to the PS/2 command port
    fn write_command(&self, command: u8) {
        // In a real implementation, this would use proper I/O port access
        unsafe {
            // This is a placeholder - in real implementation would use:
            // x86_64::instructions::port::Port::new(PS2_COMMAND_PORT).write(command)
        }
    }

    /// Convert scancode to keycode
    fn scancode_to_keycode(&self, scancode: u8) -> KeyCode {
        match scancode {
            // Letters
            0x1E => KeyCode::A, 0x30 => KeyCode::B, 0x2E => KeyCode::C, 0x20 => KeyCode::D,
            0x12 => KeyCode::E, 0x21 => KeyCode::F, 0x22 => KeyCode::G, 0x23 => KeyCode::H,
            0x17 => KeyCode::I, 0x24 => KeyCode::J, 0x25 => KeyCode::K, 0x26 => KeyCode::L,
            0x32 => KeyCode::M, 0x31 => KeyCode::N, 0x18 => KeyCode::O, 0x19 => KeyCode::P,
            0x10 => KeyCode::Q, 0x13 => KeyCode::R, 0x1F => KeyCode::S, 0x14 => KeyCode::T,
            0x16 => KeyCode::U, 0x2F => KeyCode::V, 0x11 => KeyCode::W, 0x2D => KeyCode::X,
            0x15 => KeyCode::Y, 0x2C => KeyCode::Z,
            
            // Numbers
            0x0B => KeyCode::Key0, 0x02 => KeyCode::Key1, 0x03 => KeyCode::Key2,
            0x04 => KeyCode::Key3, 0x05 => KeyCode::Key4, 0x06 => KeyCode::Key5,
            0x07 => KeyCode::Key6, 0x08 => KeyCode::Key7, 0x09 => KeyCode::Key8,
            0x0A => KeyCode::Key9,
            
            // Function keys
            0x3B => KeyCode::F1, 0x3C => KeyCode::F2, 0x3D => KeyCode::F3, 0x3E => KeyCode::F4,
            0x3F => KeyCode::F5, 0x40 => KeyCode::F6, 0x41 => KeyCode::F7, 0x42 => KeyCode::F8,
            0x43 => KeyCode::F9, 0x44 => KeyCode::F10, 0x57 => KeyCode::F11, 0x58 => KeyCode::F12,
            
            // Special keys
            0x01 => KeyCode::Escape,
            0x0E => KeyCode::Backspace,
            0x0F => KeyCode::Tab,
            0x1C => KeyCode::Enter,
            0x39 => KeyCode::Space,
            0x2A => KeyCode::LeftShift,
            0x36 => KeyCode::RightShift,
            0x1D => KeyCode::LeftCtrl,
            0x38 => KeyCode::LeftAlt,
            0x3A => KeyCode::CapsLock,
            
            // Extended keys (when extended_scancode is true)
            0x48 if self.extended_scancode => KeyCode::ArrowUp,
            0x50 if self.extended_scancode => KeyCode::ArrowDown,
            0x4B if self.extended_scancode => KeyCode::ArrowLeft,
            0x4D if self.extended_scancode => KeyCode::ArrowRight,
            0x53 if self.extended_scancode => KeyCode::Delete,
            0x47 if self.extended_scancode => KeyCode::Home,
            0x4F if self.extended_scancode => KeyCode::End,
            0x49 if self.extended_scancode => KeyCode::PageUp,
            0x51 if self.extended_scancode => KeyCode::PageDown,
            0x52 if self.extended_scancode => KeyCode::Insert,
            
            _ => KeyCode::Unknown,
        }
    }

    /// Convert keycode to ASCII character (considering modifiers)
    fn keycode_to_ascii(&self, key_code: KeyCode) -> Option<char> {
        let shift_pressed = self.modifiers.contains(KeyModifiers::SHIFT);
        let caps_lock = self.modifiers.contains(KeyModifiers::CAPS_LOCK);
        
        match key_code {
            // Letters - handle each key individually since range patterns don't work with enums
            KeyCode::A => {
                let base_char = 'a';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::B => {
                let base_char = 'b';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::C => {
                let base_char = 'c';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::D => {
                let base_char = 'd';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::E => {
                let base_char = 'e';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::F => {
                let base_char = 'f';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::G => {
                let base_char = 'g';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::H => {
                let base_char = 'h';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::I => {
                let base_char = 'i';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::J => {
                let base_char = 'j';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::K => {
                let base_char = 'k';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::L => {
                let base_char = 'l';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::M => {
                let base_char = 'm';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::N => {
                let base_char = 'n';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::O => {
                let base_char = 'o';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::P => {
                let base_char = 'p';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::Q => {
                let base_char = 'q';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::R => {
                let base_char = 'r';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::S => {
                let base_char = 's';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::T => {
                let base_char = 't';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::U => {
                let base_char = 'u';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::V => {
                let base_char = 'v';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::W => {
                let base_char = 'w';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::X => {
                let base_char = 'x';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::Y => {
                let base_char = 'y';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            KeyCode::Z => {
                let base_char = 'z';
                if shift_pressed ^ caps_lock {
                    Some(base_char.to_ascii_uppercase())
                } else {
                    Some(base_char)
                }
            }
            
            // Numbers
            KeyCode::Key0 => Some(if shift_pressed { ')' } else { '0' }),
            KeyCode::Key1 => Some(if shift_pressed { '!' } else { '1' }),
            KeyCode::Key2 => Some(if shift_pressed { '@' } else { '2' }),
            KeyCode::Key3 => Some(if shift_pressed { '#' } else { '3' }),
            KeyCode::Key4 => Some(if shift_pressed { '$' } else { '4' }),
            KeyCode::Key5 => Some(if shift_pressed { '%' } else { '5' }),
            KeyCode::Key6 => Some(if shift_pressed { '^' } else { '6' }),
            KeyCode::Key7 => Some(if shift_pressed { '&' } else { '7' }),
            KeyCode::Key8 => Some(if shift_pressed { '*' } else { '8' }),
            KeyCode::Key9 => Some(if shift_pressed { '(' } else { '9' }),
            
            // Special characters
            KeyCode::Space => Some(' '),
            KeyCode::Tab => Some('\t'),
            KeyCode::Enter => Some('\n'),
            KeyCode::Backspace => Some('\x08'),
            
            _ => None,
        }
    }

    /// Update modifier state based on key press/release
    fn update_modifiers(&mut self, key_code: KeyCode, event_type: KeyEventType) {
        match (key_code, event_type) {
            (KeyCode::LeftShift | KeyCode::RightShift, KeyEventType::KeyPress) => {
                self.modifiers.insert(KeyModifiers::SHIFT);
            }
            (KeyCode::LeftShift | KeyCode::RightShift, KeyEventType::KeyRelease) => {
                self.modifiers.remove(KeyModifiers::SHIFT);
            }
            (KeyCode::LeftCtrl, KeyEventType::KeyPress) => {
                self.modifiers.insert(KeyModifiers::CTRL);
            }
            (KeyCode::LeftCtrl, KeyEventType::KeyRelease) => {
                self.modifiers.remove(KeyModifiers::CTRL);
            }
            (KeyCode::LeftAlt, KeyEventType::KeyPress) => {
                self.modifiers.insert(KeyModifiers::ALT);
            }
            (KeyCode::LeftAlt, KeyEventType::KeyRelease) => {
                self.modifiers.remove(KeyModifiers::ALT);
            }
            (KeyCode::CapsLock, KeyEventType::KeyPress) => {
                self.modifiers.toggle(KeyModifiers::CAPS_LOCK);
            }
            _ => {}
        }
    }

    /// Process a scancode and generate input events
    fn process_scancode(&mut self, scancode: u8) {
        // Handle extended scancodes (0xE0 prefix)
        if scancode == 0xE0 {
            self.extended_scancode = true;
            return;
        }

        // Determine if this is a key press or release
        let is_release = (scancode & 0x80) != 0;
        let base_scancode = scancode & 0x7F;
        let event_type = if is_release {
            KeyEventType::KeyRelease
        } else {
            KeyEventType::KeyPress
        };

        // Convert scancode to keycode
        let key_code = self.scancode_to_keycode(base_scancode);
        
        // Update modifier state
        self.update_modifiers(key_code, event_type);
        
        // Generate ASCII character if applicable
        let ascii_char = if event_type == KeyEventType::KeyPress {
            self.keycode_to_ascii(key_code)
        } else {
            None
        };

        // Create input event
        let event = InputEvent {
            event_type,
            key_code,
            scancode: base_scancode,
            modifiers: self.modifiers,
            ascii_char,
            timestamp: 0, // In real implementation, would use actual timestamp
        };

        // Add to event queue
        self.queue_event(event);
        
        // Reset extended scancode flag
        self.extended_scancode = false;
    }

    /// Add an event to the input queue
    fn queue_event(&mut self, event: InputEvent) {
        // Remove oldest events if queue is full
        while self.event_queue.len() >= self.max_queue_size {
            self.event_queue.pop_front();
        }
        
        self.event_queue.push_back(event);
    }

    /// Get the next input event from the queue
    pub fn get_next_event(&mut self) -> Option<InputEvent> {
        self.event_queue.pop_front()
    }

    /// Check if there are pending input events
    pub fn has_events(&self) -> bool {
        !self.event_queue.is_empty()
    }

    /// Get the number of queued events
    pub fn event_count(&self) -> usize {
        self.event_queue.len()
    }

    /// Clear all queued events
    pub fn clear_events(&mut self) {
        self.event_queue.clear();
    }

    /// Handle keyboard interrupt (would be called by interrupt handler)
    pub fn handle_interrupt(&mut self) {
        let status = self.read_status();
        
        if status.contains(PS2Status::OUTPUT_BUFFER_FULL) {
            let scancode = self.read_data();
            self.process_scancode(scancode);
        }
    }

    /// Initialize the PS/2 keyboard controller
    fn initialize_controller(&mut self) -> Result<(), DriverError> {
        // In a real implementation, this would:
        // 1. Disable interrupts
        // 2. Flush output buffer
        // 3. Set controller configuration
        // 4. Enable keyboard
        // 5. Set scan code set
        // 6. Enable interrupts
        
        // For now, just simulate successful initialization
        Ok(())
    }
}

impl KoshDriver for PS2KeyboardDriver {
    fn init(&mut self, _capabilities: Vec<Capability>) -> Result<(), DriverError> {
        self.status = DriverStatus::Initializing;
        
        // Initialize the PS/2 controller
        self.initialize_controller()?;
        
        // Clear any existing events
        self.clear_events();
        
        // Reset modifier state
        self.modifiers = KeyModifiers::empty();
        self.extended_scancode = false;
        
        self.status = DriverStatus::Ready;
        Ok(())
    }

    fn handle_request(&mut self, request: DriverRequest) -> Result<DriverResponse, DriverError> {
        match request {
            DriverRequest::Initialize => {
                self.init(Vec::new())?;
                Ok(DriverResponse::Success)
            }
            
            DriverRequest::Read { .. } => {
                // Return queued input events as serialized data
                let mut event_data = Vec::new();
                
                while let Some(event) = self.get_next_event() {
                    // In a real implementation, this would properly serialize the event
                    // For now, we'll create a simple byte representation
                    let mut event_bytes = Vec::new();
                    event_bytes.push(event.event_type as u8);
                    event_bytes.push(event.key_code as u8);
                    event_bytes.push(event.scancode);
                    event_bytes.push(event.modifiers.bits());
                    
                    if let Some(ascii) = event.ascii_char {
                        event_bytes.push(1); // Has ASCII
                        event_bytes.push(ascii as u8);
                    } else {
                        event_bytes.push(0); // No ASCII
                        event_bytes.push(0);
                    }
                    
                    event_data.extend(event_bytes);
                }
                
                Ok(DriverResponse::Data(event_data))
            }
            
            DriverRequest::Control { command, data } => {
                match command {
                    // Clear event queue
                    0x01 => {
                        self.clear_events();
                        Ok(DriverResponse::Success)
                    }
                    // Set queue size
                    0x02 => {
                        if !data.is_empty() {
                            let new_size = data[0] as usize;
                            if new_size > 0 && new_size <= 1024 {
                                self.max_queue_size = new_size;
                                // Trim queue if necessary
                                while self.event_queue.len() > self.max_queue_size {
                                    self.event_queue.pop_front();
                                }
                                Ok(DriverResponse::Success)
                            } else {
                                Err(DriverError::InvalidRequest)
                            }
                        } else {
                            Err(DriverError::InvalidRequest)
                        }
                    }
                    // Simulate key press (for testing)
                    0x03 => {
                        if !data.is_empty() {
                            self.process_scancode(data[0]);
                            Ok(DriverResponse::Success)
                        } else {
                            Err(DriverError::InvalidRequest)
                        }
                    }
                    _ => Err(DriverError::InvalidRequest)
                }
            }
            
            DriverRequest::Query { query_type } => {
                match query_type {
                    kosh_driver::QueryType::Status => {
                        Ok(DriverResponse::Status(self.status))
                    }
                    kosh_driver::QueryType::HardwareInfo => {
                        let info = self.get_driver_info();
                        Ok(DriverResponse::Info(info))
                    }
                    kosh_driver::QueryType::Statistics => {
                        // Return event queue statistics
                        let stats = vec![
                            self.event_count() as u8,
                            self.max_queue_size as u8,
                            self.modifiers.bits(),
                        ];
                        Ok(DriverResponse::Data(stats))
                    }
                    _ => Err(DriverError::InvalidRequest)
                }
            }
            
            _ => Err(DriverError::InvalidRequest)
        }
    }

    fn cleanup(&mut self) -> Result<(), DriverError> {
        self.status = DriverStatus::Stopping;
        
        // Clear event queue
        self.clear_events();
        
        // Reset modifier state
        self.modifiers = KeyModifiers::empty();
        
        // In a real implementation, this would disable the keyboard controller
        
        self.status = DriverStatus::Uninitialized;
        Ok(())
    }

    fn get_required_capabilities(&self) -> Vec<DriverCapabilityType> {
        vec![
            DriverCapabilityType::Hardware(HardwareCapability::IoPort { 
                start: PS2_DATA_PORT, 
                end: PS2_COMMAND_PORT 
            }),
            DriverCapabilityType::Hardware(HardwareCapability::Interrupt { irq: 1 }),
            DriverCapabilityType::HardwareAccess,
        ]
    }

    fn get_provided_capabilities(&self) -> Vec<DriverCapabilityType> {
        vec![
            DriverCapabilityType::Custom(String::from("keyboard_input")),
            DriverCapabilityType::Custom(String::from("input_events")),
        ]
    }

    fn get_driver_info(&self) -> DriverInfo {
        DriverInfo {
            name: String::from("PS/2 Keyboard Driver"),
            version: String::from("1.0.0"),
            vendor: String::from("Kosh OS"),
            description: String::from("PS/2 keyboard driver with scancode translation and input event queuing"),
            driver_type: DriverType::Input,
            hardware_ids: vec![
                HardwareId {
                    vendor_id: 0x0000, // Standard PS/2 keyboard
                    device_id: 0x0001,
                    subsystem_vendor_id: None,
                    subsystem_device_id: None,
                }
            ],
        }
    }

    fn handle_power_event(&mut self, event: PowerEvent) -> Result<(), DriverError> {
        match event {
            PowerEvent::Suspend => {
                self.status = DriverStatus::Suspended;
                // Clear events on suspend
                self.clear_events();
                Ok(())
            }
            PowerEvent::Resume => {
                self.status = DriverStatus::Ready;
                // Reinitialize controller
                self.initialize_controller()
            }
            PowerEvent::PowerDown => {
                self.cleanup()
            }
            _ => Ok(())
        }
    }

    fn get_status(&self) -> DriverStatus {
        self.status
    }
}

/// Global keyboard driver instance protected by mutex
static KEYBOARD_DRIVER: Mutex<Option<PS2KeyboardDriver>> = Mutex::new(None);

/// Initialize the global keyboard driver
pub fn init_keyboard_driver() -> Result<(), DriverError> {
    let mut driver_guard = KEYBOARD_DRIVER.lock();
    let mut driver = PS2KeyboardDriver::new();
    driver.init(Vec::new())?;
    *driver_guard = Some(driver);
    Ok(())
}

/// Get the next input event from the global keyboard driver
pub fn keyboard_get_event() -> Option<InputEvent> {
    let mut driver_guard = KEYBOARD_DRIVER.lock();
    if let Some(ref mut driver) = *driver_guard {
        driver.get_next_event()
    } else {
        None
    }
}

/// Check if there are pending keyboard events
pub fn keyboard_has_events() -> bool {
    let driver_guard = KEYBOARD_DRIVER.lock();
    if let Some(ref driver) = *driver_guard {
        driver.has_events()
    } else {
        false
    }
}

/// Handle keyboard interrupt (called by interrupt handler)
pub fn keyboard_interrupt_handler() {
    let mut driver_guard = KEYBOARD_DRIVER.lock();
    if let Some(ref mut driver) = *driver_guard {
        driver.handle_interrupt();
    }
}

/// Driver factory for creating PS/2 keyboard drivers
pub struct KeyboardDriverFactory;

impl kosh_driver::DriverFactory for KeyboardDriverFactory {
    fn create_driver(&self, _hardware_id: &HardwareId) -> Result<Box<dyn KoshDriver>, DriverError> {
        let driver = PS2KeyboardDriver::new();
        Ok(Box::new(driver))
    }
    
    fn can_handle(&self, hardware_id: &HardwareId) -> bool {
        // Check if this is a PS/2 keyboard device
        hardware_id.vendor_id == 0x0000 && hardware_id.device_id == 0x0001
    }
    
    fn get_driver_type(&self) -> DriverType {
        DriverType::Input
    }
}

/// Register the keyboard driver with the driver manager
pub fn register_keyboard_driver() -> Result<(), DriverError> {
    // This would typically register with the driver manager
    // For now, just initialize the global driver
    init_keyboard_driver()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod integration_test;