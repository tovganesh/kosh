#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec, string::String, boxed::Box};
use kosh_driver::{
    KoshDriver, DriverInfo, DriverType, HardwareId, DriverStatus, PowerEvent,
    DriverRequest, DriverResponse, DriverCapabilityType
};
use kosh_types::{DriverError, Capability};
use volatile::Volatile;
use spin::Mutex;

/// VGA text mode colors
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VgaColor {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// VGA color code combining foreground and background colors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VgaColorCode(u8);

impl VgaColorCode {
    pub fn new(foreground: VgaColor, background: VgaColor) -> VgaColorCode {
        VgaColorCode((background as u8) << 4 | (foreground as u8))
    }
}

/// A character in VGA text mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct VgaChar {
    ascii_character: u8,
    color_code: VgaColorCode,
}

/// VGA text mode buffer dimensions
const VGA_BUFFER_HEIGHT: usize = 25;
const VGA_BUFFER_WIDTH: usize = 80;
const VGA_BUFFER_ADDRESS: usize = 0xb8000;

/// VGA text mode buffer
#[repr(transparent)]
pub struct VgaBuffer {
    chars: [[Volatile<VgaChar>; VGA_BUFFER_WIDTH]; VGA_BUFFER_HEIGHT],
}

/// VGA text mode driver implementation
pub struct VgaTextDriver {
    buffer: &'static mut VgaBuffer,
    cursor_row: usize,
    cursor_col: usize,
    color_code: VgaColorCode,
    status: DriverStatus,
    #[cfg(test)]
    test_buffer: Option<Box<VgaBuffer>>,
}

impl VgaTextDriver {
    /// Create a new VGA text mode driver instance
    pub fn new() -> Self {
        #[cfg(not(test))]
        {
            Self {
                buffer: unsafe { &mut *(VGA_BUFFER_ADDRESS as *mut VgaBuffer) },
                cursor_row: 0,
                cursor_col: 0,
                color_code: VgaColorCode::new(VgaColor::White, VgaColor::Black),
                status: DriverStatus::Uninitialized,
                #[cfg(test)]
                test_buffer: None,
            }
        }
        #[cfg(test)]
        {
            Self::new_for_test()
        }
    }

    #[cfg(test)]
    pub fn new_for_test() -> Self {
        use alloc::boxed::Box;
        use core::array;
        
        // Create a test buffer on the heap using from_fn to avoid Copy requirement
        let test_buffer = Box::new(VgaBuffer {
            chars: array::from_fn(|_| {
                array::from_fn(|_| {
                    Volatile::new(VgaChar {
                        ascii_character: b' ',
                        color_code: VgaColorCode::new(VgaColor::White, VgaColor::Black),
                    })
                })
            }),
        });
        
        // Leak the box to get a static reference for testing
        let buffer_ref = Box::leak(test_buffer);
        
        Self {
            buffer: buffer_ref,
            cursor_row: 0,
            cursor_col: 0,
            color_code: VgaColorCode::new(VgaColor::White, VgaColor::Black),
            status: DriverStatus::Uninitialized,
            test_buffer: None,
        }
    }

    /// Write a single byte to the VGA buffer
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.cursor_col >= VGA_BUFFER_WIDTH {
                    self.new_line();
                }

                let vga_char = VgaChar {
                    ascii_character: byte,
                    color_code: self.color_code,
                };

                self.buffer.chars[self.cursor_row][self.cursor_col].write(vga_char);
                self.cursor_col += 1;
            }
        }
    }

    /// Write a string to the VGA buffer
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // Printable ASCII characters and newline
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                // Non-printable characters are replaced with ■
                _ => self.write_byte(0xfe),
            }
        }
    }

    /// Set the color for subsequent text output
    pub fn set_color(&mut self, foreground: VgaColor, background: VgaColor) {
        self.color_code = VgaColorCode::new(foreground, background);
    }

    /// Clear the entire screen
    pub fn clear_screen(&mut self) {
        let blank = VgaChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };

        for row in 0..VGA_BUFFER_HEIGHT {
            for col in 0..VGA_BUFFER_WIDTH {
                self.buffer.chars[row][col].write(blank);
            }
        }

        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    /// Move to a new line
    fn new_line(&mut self) {
        if self.cursor_row >= VGA_BUFFER_HEIGHT - 1 {
            // Scroll up
            for row in 1..VGA_BUFFER_HEIGHT {
                for col in 0..VGA_BUFFER_WIDTH {
                    let character = self.buffer.chars[row][col].read();
                    self.buffer.chars[row - 1][col].write(character);
                }
            }
            self.clear_row(VGA_BUFFER_HEIGHT - 1);
        } else {
            self.cursor_row += 1;
        }
        self.cursor_col = 0;
    }

    /// Clear a specific row
    fn clear_row(&mut self, row: usize) {
        let blank = VgaChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..VGA_BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }

    /// Set cursor position
    pub fn set_cursor(&mut self, row: usize, col: usize) {
        if row < VGA_BUFFER_HEIGHT && col < VGA_BUFFER_WIDTH {
            self.cursor_row = row;
            self.cursor_col = col;
        }
    }

    /// Get current cursor position
    pub fn get_cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }
}

impl KoshDriver for VgaTextDriver {
    fn init(&mut self, _capabilities: Vec<Capability>) -> Result<(), DriverError> {
        // Initialize VGA text mode
        self.status = DriverStatus::Initializing;
        
        // Clear the screen and set default colors
        self.clear_screen();
        self.set_color(VgaColor::White, VgaColor::Black);
        
        // Write a test message to verify functionality
        self.write_string("VGA Text Mode Driver Initialized\n");
        
        self.status = DriverStatus::Ready;
        Ok(())
    }

    fn handle_request(&mut self, request: DriverRequest) -> Result<DriverResponse, DriverError> {
        match request {
            DriverRequest::Initialize => {
                self.init(Vec::new())?;
                Ok(DriverResponse::Success)
            }
            
            DriverRequest::Write { data, .. } => {
                if let Ok(text) = core::str::from_utf8(&data) {
                    self.write_string(text);
                    Ok(DriverResponse::Success)
                } else {
                    Err(DriverError::InvalidRequest)
                }
            }
            
            DriverRequest::Control { command, data } => {
                match command {
                    // Clear screen command
                    0x01 => {
                        self.clear_screen();
                        Ok(DriverResponse::Success)
                    }
                    // Set color command
                    0x02 => {
                        if data.len() >= 2 {
                            let fg = data[0];
                            let bg = data[1];
                            if fg <= 15 && bg <= 15 {
                                let fg_color = unsafe { core::mem::transmute(fg) };
                                let bg_color = unsafe { core::mem::transmute(bg) };
                                self.set_color(fg_color, bg_color);
                                Ok(DriverResponse::Success)
                            } else {
                                Err(DriverError::InvalidRequest)
                            }
                        } else {
                            Err(DriverError::InvalidRequest)
                        }
                    }
                    // Set cursor position command
                    0x03 => {
                        if data.len() >= 2 {
                            let row = data[0] as usize;
                            let col = data[1] as usize;
                            self.set_cursor(row, col);
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
                    _ => Err(DriverError::InvalidRequest)
                }
            }
            
            _ => Err(DriverError::InvalidRequest)
        }
    }

    fn cleanup(&mut self) -> Result<(), DriverError> {
        self.status = DriverStatus::Stopping;
        // Clear screen on cleanup
        self.clear_screen();
        self.status = DriverStatus::Uninitialized;
        Ok(())
    }

    fn get_required_capabilities(&self) -> Vec<DriverCapabilityType> {
        vec![
            DriverCapabilityType::MemoryAccess,
            DriverCapabilityType::HardwareAccess,
        ]
    }

    fn get_provided_capabilities(&self) -> Vec<DriverCapabilityType> {
        vec![
            DriverCapabilityType::TextOutput,
            DriverCapabilityType::GraphicsOutput,
        ]
    }

    fn get_driver_info(&self) -> DriverInfo {
        DriverInfo {
            name: String::from("VGA Text Mode Driver"),
            version: String::from("1.0.0"),
            vendor: String::from("Kosh OS"),
            description: String::from("VGA text mode display driver with color support"),
            driver_type: DriverType::Graphics,
            hardware_ids: vec![
                HardwareId {
                    vendor_id: 0x1234, // Generic VGA
                    device_id: 0x1111,
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
                Ok(())
            }
            PowerEvent::Resume => {
                self.status = DriverStatus::Ready;
                // Reinitialize display
                self.clear_screen();
                Ok(())
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

/// Global VGA driver instance protected by mutex
static VGA_DRIVER: Mutex<Option<VgaTextDriver>> = Mutex::new(None);

/// Initialize the global VGA driver
pub fn init_vga_driver() -> Result<(), DriverError> {
    let mut driver_guard = VGA_DRIVER.lock();
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new())?;
    *driver_guard = Some(driver);
    Ok(())
}

/// Write text using the global VGA driver
pub fn vga_write(text: &str) {
    let mut driver_guard = VGA_DRIVER.lock();
    if let Some(ref mut driver) = *driver_guard {
        driver.write_string(text);
    }
}

/// Set color using the global VGA driver
pub fn vga_set_color(foreground: VgaColor, background: VgaColor) {
    let mut driver_guard = VGA_DRIVER.lock();
    if let Some(ref mut driver) = *driver_guard {
        driver.set_color(foreground, background);
    }
}

/// Clear screen using the global VGA driver
pub fn vga_clear_screen() {
    let mut driver_guard = VGA_DRIVER.lock();
    if let Some(ref mut driver) = *driver_guard {
        driver.clear_screen();
    }
}

/// Driver factory for creating VGA text mode drivers
pub struct VgaDriverFactory;

impl kosh_driver::DriverFactory for VgaDriverFactory {
    fn create_driver(&self, _hardware_id: &HardwareId) -> Result<Box<dyn KoshDriver>, DriverError> {
        let driver = VgaTextDriver::new();
        Ok(Box::new(driver))
    }
    
    fn can_handle(&self, hardware_id: &HardwareId) -> bool {
        // Check if this is a VGA-compatible device
        hardware_id.vendor_id == 0x1234 && hardware_id.device_id == 0x1111
    }
    
    fn get_driver_type(&self) -> DriverType {
        DriverType::Graphics
    }
}

/// Register the VGA driver with the driver manager
pub fn register_vga_driver() -> Result<(), DriverError> {
    // This would typically register with the driver manager
    // For now, just initialize the global driver
    init_vga_driver()
}

#[cfg(test)]
mod tests;
