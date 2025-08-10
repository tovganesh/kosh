/// Integration tests demonstrating keyboard driver communication with user space
/// These tests show how the driver would be used in a real system

use super::*;
use alloc::{vec, vec::Vec};
use kosh_driver::{DriverRequest, DriverResponse, QueryType};

/// Simulate a user space process requesting keyboard input
#[test]
fn test_user_space_communication() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Simulate user typing "Hello"
    let hello_scancodes = vec![
        0x23, // H
        0x12, // E  
        0x26, // L
        0x26, // L
        0x18, // O
    ];
    
    // Process the scancodes as if they came from hardware
    for scancode in hello_scancodes {
        driver.process_scancode(scancode);
    }
    
    // User space process requests input events
    let response = driver.handle_request(DriverRequest::Read { 
        offset: 0, 
        length: 1024 
    });
    
    match response {
        Ok(DriverResponse::Data(data)) => {
            // Verify we got data for 5 events (6 bytes each)
            assert_eq!(data.len(), 30);
            
            // Verify the first event is 'H' key press
            assert_eq!(data[0], KeyEventType::KeyPress as u8);
            assert_eq!(data[1], KeyCode::H as u8);
            assert_eq!(data[2], 0x23); // scancode
            assert_eq!(data[4], 1); // has ASCII
            assert_eq!(data[5], b'h'); // ASCII character
        }
        _ => panic!("Expected data response"),
    }
    
    // Events should be consumed after reading
    assert_eq!(driver.event_count(), 0);
}

/// Test driver status queries from user space
#[test]
fn test_status_queries() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // User space queries driver status
    let response = driver.handle_request(DriverRequest::Query { 
        query_type: QueryType::Status 
    });
    
    match response {
        Ok(DriverResponse::Status(status)) => {
            assert_eq!(status, DriverStatus::Ready);
        }
        _ => panic!("Expected status response"),
    }
    
    // User space queries hardware info
    let response = driver.handle_request(DriverRequest::Query { 
        query_type: QueryType::HardwareInfo 
    });
    
    match response {
        Ok(DriverResponse::Info(info)) => {
            assert_eq!(info.name, "PS/2 Keyboard Driver");
            assert_eq!(info.driver_type, DriverType::Input);
        }
        _ => panic!("Expected info response"),
    }
}

/// Test control commands from system services
#[test]
fn test_system_control() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Add some events
    driver.process_scancode(0x1E); // A
    driver.process_scancode(0x30); // B
    assert_eq!(driver.event_count(), 2);
    
    // System service clears the input buffer
    let response = driver.handle_request(DriverRequest::Control { 
        command: 0x01, // Clear queue
        data: vec![] 
    });
    
    assert!(matches!(response, Ok(DriverResponse::Success)));
    assert_eq!(driver.event_count(), 0);
    
    // System service adjusts queue size for memory management
    let response = driver.handle_request(DriverRequest::Control { 
        command: 0x02, // Set queue size
        data: vec![50] // New size
    });
    
    assert!(matches!(response, Ok(DriverResponse::Success)));
    assert_eq!(driver.max_queue_size, 50);
}

/// Test error handling in driver communication
#[test]
fn test_error_handling() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Test invalid control command
    let response = driver.handle_request(DriverRequest::Control { 
        command: 0xFF, // Invalid command
        data: vec![] 
    });
    
    assert!(matches!(response, Err(DriverError::InvalidRequest)));
    
    // Test invalid queue size
    let response = driver.handle_request(DriverRequest::Control { 
        command: 0x02, // Set queue size
        data: vec![0] // Invalid size (0)
    });
    
    assert!(matches!(response, Err(DriverError::InvalidRequest)));
    
    // Test unsupported request type
    let response = driver.handle_request(DriverRequest::Write { 
        offset: 0, 
        data: vec![1, 2, 3] 
    });
    
    assert!(matches!(response, Err(DriverError::InvalidRequest)));
}

/// Test modifier key combinations
#[test]
fn test_modifier_combinations() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Test Ctrl+C combination
    driver.process_scancode(0x1D); // Ctrl press
    driver.process_scancode(0x2E); // C press
    
    // Read the events
    let response = driver.handle_request(DriverRequest::Read { 
        offset: 0, 
        length: 1024 
    });
    
    match response {
        Ok(DriverResponse::Data(data)) => {
            // Should have 2 events (Ctrl press, C press)
            assert_eq!(data.len(), 12);
            
            // Check the C key event has Ctrl modifier
            let c_event_modifiers = data[9]; // Second event's modifiers
            assert_eq!(c_event_modifiers & KeyModifiers::CTRL.bits(), KeyModifiers::CTRL.bits());
        }
        _ => panic!("Expected data response"),
    }
}

/// Test special key handling (arrows, function keys)
#[test]
fn test_special_keys() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Test extended scancode sequence (arrow key)
    driver.process_scancode(0xE0); // Extended prefix
    driver.process_scancode(0x48); // Arrow up
    
    // Test function key
    driver.process_scancode(0x3B); // F1
    
    let response = driver.handle_request(DriverRequest::Read { 
        offset: 0, 
        length: 1024 
    });
    
    match response {
        Ok(DriverResponse::Data(data)) => {
            // Should have 2 events
            assert_eq!(data.len(), 12);
            
            // First event should be arrow up
            assert_eq!(data[1], KeyCode::ArrowUp as u8);
            assert_eq!(data[4], 0); // No ASCII for arrow keys
            
            // Second event should be F1
            assert_eq!(data[7], KeyCode::F1 as u8);
            assert_eq!(data[10], 0); // No ASCII for function keys
        }
        _ => panic!("Expected data response"),
    }
}

/// Test queue overflow behavior
#[test]
fn test_queue_overflow() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Set a small queue size
    driver.max_queue_size = 3;
    
    // Generate more events than the queue can hold
    let scancodes = [0x1E, 0x30, 0x2E, 0x20, 0x12]; // A, B, C, D, E
    for scancode in scancodes {
        driver.process_scancode(scancode);
    }
    
    // Should only have the last 3 events
    assert_eq!(driver.event_count(), 3);
    
    let response = driver.handle_request(DriverRequest::Read { 
        offset: 0, 
        length: 1024 
    });
    
    match response {
        Ok(DriverResponse::Data(data)) => {
            // Should have 3 events
            assert_eq!(data.len(), 18);
            
            // First event should be 'C' (oldest events were dropped)
            assert_eq!(data[1], KeyCode::C as u8);
        }
        _ => panic!("Expected data response"),
    }
}

/// Test power management integration
#[test]
fn test_power_management_integration() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Add some events
    driver.process_scancode(0x1E); // A
    assert!(driver.has_events());
    
    // System suspends
    let result = driver.handle_power_event(PowerEvent::Suspend);
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), DriverStatus::Suspended);
    assert!(!driver.has_events()); // Events cleared on suspend
    
    // System resumes
    let result = driver.handle_power_event(PowerEvent::Resume);
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), DriverStatus::Ready);
    
    // Driver should be functional after resume
    driver.process_scancode(0x30); // B
    assert!(driver.has_events());
}