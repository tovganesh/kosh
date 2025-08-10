#![cfg(test)]

use alloc::{vec, vec::Vec};
use crate::{VgaTextDriver, VgaColor};
use kosh_driver::{KoshDriver, DriverRequest, DriverResponse, QueryType};
use kosh_types::DriverError;

#[test]
fn test_vga_driver_initialization() {
    let mut driver = VgaTextDriver::new();
    
    // Test initialization
    let result = driver.init(Vec::new());
    assert!(result.is_ok());
    
    // Check driver status after initialization
    let status = driver.get_status();
    assert_eq!(status, kosh_driver::DriverStatus::Ready);
}

#[test]
fn test_vga_driver_write_text() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test writing text
    let text = "Hello, World!";
    let request = DriverRequest::Write {
        offset: 0,
        data: text.as_bytes().to_vec(),
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_ok());
    assert!(matches!(response.unwrap(), DriverResponse::Success));
}

#[test]
fn test_vga_driver_color_control() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test setting color
    let request = DriverRequest::Control {
        command: 0x02, // Set color command
        data: vec![VgaColor::Red as u8, VgaColor::Blue as u8],
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_ok());
    assert!(matches!(response.unwrap(), DriverResponse::Success));
}

#[test]
fn test_vga_driver_clear_screen() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test clearing screen
    let request = DriverRequest::Control {
        command: 0x01, // Clear screen command
        data: vec![],
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_ok());
    assert!(matches!(response.unwrap(), DriverResponse::Success));
}

#[test]
fn test_vga_driver_cursor_control() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test setting cursor position
    let request = DriverRequest::Control {
        command: 0x03, // Set cursor command
        data: vec![5, 10], // Row 5, Column 10
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_ok());
    assert!(matches!(response.unwrap(), DriverResponse::Success));
    
    // Verify cursor position
    let (row, col) = driver.get_cursor();
    assert_eq!(row, 5);
    assert_eq!(col, 10);
}

#[test]
fn test_vga_driver_query_status() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test querying status
    let request = DriverRequest::Query {
        query_type: QueryType::Status,
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_ok());
    
    match response.unwrap() {
        DriverResponse::Status(status) => {
            assert_eq!(status, kosh_driver::DriverStatus::Ready);
        }
        _ => panic!("Expected status response"),
    }
}

#[test]
fn test_vga_driver_query_info() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test querying driver info
    let request = DriverRequest::Query {
        query_type: QueryType::HardwareInfo,
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_ok());
    
    match response.unwrap() {
        DriverResponse::Info(info) => {
            assert_eq!(info.name, "VGA Text Mode Driver");
            assert_eq!(info.version, "1.0.0");
            assert_eq!(info.vendor, "Kosh OS");
        }
        _ => panic!("Expected info response"),
    }
}

#[test]
fn test_vga_driver_error_handling() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test invalid control command
    let request = DriverRequest::Control {
        command: 0xFF, // Invalid command
        data: vec![],
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_err());
    assert!(matches!(response.unwrap_err(), DriverError::InvalidRequest));
}

#[test]
fn test_vga_driver_invalid_color() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test invalid color values
    let request = DriverRequest::Control {
        command: 0x02, // Set color command
        data: vec![16, 17], // Invalid color values (> 15)
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_err());
    assert!(matches!(response.unwrap_err(), DriverError::InvalidRequest));
}

#[test]
fn test_vga_driver_power_management() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test suspend
    let result = driver.handle_power_event(kosh_driver::PowerEvent::Suspend);
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), kosh_driver::DriverStatus::Suspended);
    
    // Test resume
    let result = driver.handle_power_event(kosh_driver::PowerEvent::Resume);
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), kosh_driver::DriverStatus::Ready);
}

#[test]
fn test_vga_driver_cleanup() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test cleanup
    let result = driver.cleanup();
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), kosh_driver::DriverStatus::Uninitialized);
}

#[test]
fn test_vga_driver_capabilities() {
    let driver = VgaTextDriver::new();
    
    // Test required capabilities
    let required = driver.get_required_capabilities();
    assert!(required.contains(&kosh_driver::DriverCapabilityType::MemoryAccess));
    assert!(required.contains(&kosh_driver::DriverCapabilityType::HardwareAccess));
    
    // Test provided capabilities
    let provided = driver.get_provided_capabilities();
    assert!(provided.contains(&kosh_driver::DriverCapabilityType::TextOutput));
    assert!(provided.contains(&kosh_driver::DriverCapabilityType::GraphicsOutput));
}

#[test]
fn test_vga_driver_invalid_utf8() {
    let mut driver = VgaTextDriver::new();
    driver.init(Vec::new()).unwrap();
    
    // Test writing invalid UTF-8 data
    let request = DriverRequest::Write {
        offset: 0,
        data: vec![0xFF, 0xFE, 0xFD], // Invalid UTF-8 sequence
    };
    
    let response = driver.handle_request(request);
    assert!(response.is_err());
    assert!(matches!(response.unwrap_err(), DriverError::InvalidRequest));
}