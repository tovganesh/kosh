use super::*;
use alloc::vec;
use kosh_driver::{DriverRequest, DriverResponse, QueryType, DriverFactory};
use bitflags::Flags;

#[test]
fn test_keyboard_driver_creation() {
    let driver = PS2KeyboardDriver::new();
    assert_eq!(driver.get_status(), DriverStatus::Uninitialized);
    assert!(!driver.has_events());
    assert_eq!(driver.event_count(), 0);
}

#[test]
fn test_keyboard_driver_initialization() {
    let mut driver = PS2KeyboardDriver::new();
    let result = driver.init(vec![]);
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), DriverStatus::Ready);
}

#[test]
fn test_scancode_to_keycode_conversion() {
    let driver = PS2KeyboardDriver::new();
    
    // Test letter keys
    assert_eq!(driver.scancode_to_keycode(0x1E), KeyCode::A);
    assert_eq!(driver.scancode_to_keycode(0x30), KeyCode::B);
    assert_eq!(driver.scancode_to_keycode(0x2C), KeyCode::Z);
    
    // Test number keys
    assert_eq!(driver.scancode_to_keycode(0x02), KeyCode::Key1);
    assert_eq!(driver.scancode_to_keycode(0x0A), KeyCode::Key9);
    assert_eq!(driver.scancode_to_keycode(0x0B), KeyCode::Key0);
    
    // Test special keys
    assert_eq!(driver.scancode_to_keycode(0x01), KeyCode::Escape);
    assert_eq!(driver.scancode_to_keycode(0x39), KeyCode::Space);
    assert_eq!(driver.scancode_to_keycode(0x1C), KeyCode::Enter);
    
    // Test unknown scancode
    assert_eq!(driver.scancode_to_keycode(0xFF), KeyCode::Unknown);
}

#[test]
fn test_keycode_to_ascii_conversion() {
    let mut driver = PS2KeyboardDriver::new();
    
    // Test letters without modifiers
    assert_eq!(driver.keycode_to_ascii(KeyCode::A), Some('a'));
    assert_eq!(driver.keycode_to_ascii(KeyCode::Z), Some('z'));
    
    // Test letters with shift
    driver.modifiers.insert(KeyModifiers::SHIFT);
    assert_eq!(driver.keycode_to_ascii(KeyCode::A), Some('A'));
    assert_eq!(driver.keycode_to_ascii(KeyCode::Z), Some('Z'));
    
    // Test numbers without shift
    driver.modifiers.remove(KeyModifiers::SHIFT);
    assert_eq!(driver.keycode_to_ascii(KeyCode::Key1), Some('1'));
    assert_eq!(driver.keycode_to_ascii(KeyCode::Key0), Some('0'));
    
    // Test numbers with shift
    driver.modifiers.insert(KeyModifiers::SHIFT);
    assert_eq!(driver.keycode_to_ascii(KeyCode::Key1), Some('!'));
    assert_eq!(driver.keycode_to_ascii(KeyCode::Key0), Some(')'));
    
    // Test special characters
    driver.modifiers.clear();
    assert_eq!(driver.keycode_to_ascii(KeyCode::Space), Some(' '));
    assert_eq!(driver.keycode_to_ascii(KeyCode::Tab), Some('\t'));
    assert_eq!(driver.keycode_to_ascii(KeyCode::Enter), Some('\n'));
    
    // Test non-printable keys
    assert_eq!(driver.keycode_to_ascii(KeyCode::F1), None);
    assert_eq!(driver.keycode_to_ascii(KeyCode::ArrowUp), None);
}

#[test]
fn test_modifier_state_management() {
    let mut driver = PS2KeyboardDriver::new();
    
    // Test shift modifier
    driver.update_modifiers(KeyCode::LeftShift, KeyEventType::KeyPress);
    assert!(driver.modifiers.contains(KeyModifiers::SHIFT));
    
    driver.update_modifiers(KeyCode::LeftShift, KeyEventType::KeyRelease);
    assert!(!driver.modifiers.contains(KeyModifiers::SHIFT));
    
    // Test ctrl modifier
    driver.update_modifiers(KeyCode::LeftCtrl, KeyEventType::KeyPress);
    assert!(driver.modifiers.contains(KeyModifiers::CTRL));
    
    driver.update_modifiers(KeyCode::LeftCtrl, KeyEventType::KeyRelease);
    assert!(!driver.modifiers.contains(KeyModifiers::CTRL));
    
    // Test caps lock toggle
    driver.update_modifiers(KeyCode::CapsLock, KeyEventType::KeyPress);
    assert!(driver.modifiers.contains(KeyModifiers::CAPS_LOCK));
    
    driver.update_modifiers(KeyCode::CapsLock, KeyEventType::KeyPress);
    assert!(!driver.modifiers.contains(KeyModifiers::CAPS_LOCK));
}

#[test]
fn test_scancode_processing() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Test key press
    driver.process_scancode(0x1E); // 'A' key press
    assert!(driver.has_events());
    assert_eq!(driver.event_count(), 1);
    
    let event = driver.get_next_event().unwrap();
    assert_eq!(event.event_type, KeyEventType::KeyPress);
    assert_eq!(event.key_code, KeyCode::A);
    assert_eq!(event.scancode, 0x1E);
    assert_eq!(event.ascii_char, Some('a'));
    
    // Test key release
    driver.process_scancode(0x9E); // 'A' key release (0x1E | 0x80)
    assert!(driver.has_events());
    
    let event = driver.get_next_event().unwrap();
    assert_eq!(event.event_type, KeyEventType::KeyRelease);
    assert_eq!(event.key_code, KeyCode::A);
    assert_eq!(event.ascii_char, None);
}

#[test]
fn test_extended_scancode_processing() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Test extended scancode sequence (arrow key)
    driver.process_scancode(0xE0); // Extended scancode prefix
    assert!(!driver.has_events()); // No event generated yet
    
    driver.process_scancode(0x48); // Arrow up
    assert!(driver.has_events());
    
    let event = driver.get_next_event().unwrap();
    assert_eq!(event.key_code, KeyCode::ArrowUp);
    assert_eq!(event.scancode, 0x48);
}

#[test]
fn test_event_queue_management() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Test queue size limit
    driver.max_queue_size = 3;
    
    // Add events to fill the queue
    driver.process_scancode(0x1E); // A
    driver.process_scancode(0x30); // B
    driver.process_scancode(0x2E); // C
    assert_eq!(driver.event_count(), 3);
    
    // Add one more event (should remove oldest)
    driver.process_scancode(0x20); // D
    assert_eq!(driver.event_count(), 3);
    
    // First event should be 'B' now (A was removed)
    let event = driver.get_next_event().unwrap();
    assert_eq!(event.key_code, KeyCode::B);
}

#[test]
fn test_driver_requests() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Test initialization request
    let response = driver.handle_request(DriverRequest::Initialize);
    assert!(matches!(response, Ok(DriverResponse::Success)));
    
    // Test status query
    let response = driver.handle_request(DriverRequest::Query { 
        query_type: QueryType::Status 
    });
    assert!(matches!(response, Ok(DriverResponse::Status(DriverStatus::Ready))));
    
    // Test hardware info query
    let response = driver.handle_request(DriverRequest::Query { 
        query_type: QueryType::HardwareInfo 
    });
    assert!(matches!(response, Ok(DriverResponse::Info(_))));
}

#[test]
fn test_control_commands() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Add some events
    driver.process_scancode(0x1E); // A
    driver.process_scancode(0x30); // B
    assert_eq!(driver.event_count(), 2);
    
    // Test clear queue command
    let response = driver.handle_request(DriverRequest::Control { 
        command: 0x01, 
        data: vec![] 
    });
    assert!(matches!(response, Ok(DriverResponse::Success)));
    assert_eq!(driver.event_count(), 0);
    
    // Test set queue size command
    let response = driver.handle_request(DriverRequest::Control { 
        command: 0x02, 
        data: vec![10] 
    });
    assert!(matches!(response, Ok(DriverResponse::Success)));
    assert_eq!(driver.max_queue_size, 10);
    
    // Test simulate key press command
    let response = driver.handle_request(DriverRequest::Control { 
        command: 0x03, 
        data: vec![0x1E] // A key
    });
    assert!(matches!(response, Ok(DriverResponse::Success)));
    assert_eq!(driver.event_count(), 1);
}

#[test]
fn test_read_events() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Add some events
    driver.process_scancode(0x1E); // A key press
    driver.process_scancode(0x9E); // A key release
    
    // Test read request
    let response = driver.handle_request(DriverRequest::Read { 
        offset: 0, 
        length: 1024 
    });
    
    match response {
        Ok(DriverResponse::Data(data)) => {
            assert!(!data.is_empty());
            // Should have data for 2 events (6 bytes each)
            assert_eq!(data.len(), 12);
        }
        _ => panic!("Expected data response"),
    }
    
    // Events should be consumed
    assert_eq!(driver.event_count(), 0);
}

#[test]
fn test_power_management() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Add some events
    driver.process_scancode(0x1E);
    assert!(driver.has_events());
    
    // Test suspend
    let result = driver.handle_power_event(PowerEvent::Suspend);
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), DriverStatus::Suspended);
    assert!(!driver.has_events()); // Events should be cleared
    
    // Test resume
    let result = driver.handle_power_event(PowerEvent::Resume);
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), DriverStatus::Ready);
    
    // Test power down
    let result = driver.handle_power_event(PowerEvent::PowerDown);
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), DriverStatus::Uninitialized);
}

#[test]
fn test_driver_capabilities() {
    let driver = PS2KeyboardDriver::new();
    
    let required = driver.get_required_capabilities();
    assert!(!required.is_empty());
    
    // Should require hardware access
    assert!(required.iter().any(|cap| matches!(cap, DriverCapabilityType::HardwareAccess)));
    
    // Should require I/O port access
    assert!(required.iter().any(|cap| matches!(
        cap, 
        DriverCapabilityType::Hardware(HardwareCapability::IoPort { .. })
    )));
    
    // Should require interrupt access
    assert!(required.iter().any(|cap| matches!(
        cap, 
        DriverCapabilityType::Hardware(HardwareCapability::Interrupt { irq: 1 })
    )));
    
    let provided = driver.get_provided_capabilities();
    assert!(!provided.is_empty());
    
    // Should provide keyboard input capability
    assert!(provided.iter().any(|cap| matches!(
        cap, 
        DriverCapabilityType::Custom(name) if name == "keyboard_input"
    )));
}

#[test]
fn test_driver_factory() {
    let factory = KeyboardDriverFactory;
    
    // Test driver type
    assert_eq!(factory.get_driver_type(), DriverType::Input);
    
    // Test hardware compatibility
    let ps2_keyboard = HardwareId {
        vendor_id: 0x0000,
        device_id: 0x0001,
        subsystem_vendor_id: None,
        subsystem_device_id: None,
    };
    assert!(factory.can_handle(&ps2_keyboard));
    
    let other_device = HardwareId {
        vendor_id: 0x1234,
        device_id: 0x5678,
        subsystem_vendor_id: None,
        subsystem_device_id: None,
    };
    assert!(!factory.can_handle(&other_device));
    
    // Test driver creation
    let result = factory.create_driver(&ps2_keyboard);
    assert!(result.is_ok());
}

#[test]
fn test_caps_lock_behavior() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Test normal letter
    driver.process_scancode(0x1E); // A key
    let event = driver.get_next_event().unwrap();
    assert_eq!(event.ascii_char, Some('a'));
    
    // Enable caps lock
    driver.process_scancode(0x3A); // Caps lock press
    let _caps_event = driver.get_next_event().unwrap(); // Consume the caps lock event
    assert!(driver.modifiers.contains(KeyModifiers::CAPS_LOCK));
    
    // Test letter with caps lock
    driver.process_scancode(0x1E); // A key
    let event = driver.get_next_event().unwrap();
    assert_eq!(event.ascii_char, Some('A'));
    
    // Test with shift + caps lock (should be lowercase)
    driver.process_scancode(0x2A); // Left shift press
    let _shift_event = driver.get_next_event().unwrap(); // Consume the shift event
    driver.process_scancode(0x1E); // A key
    let event = driver.get_next_event().unwrap();
    assert_eq!(event.ascii_char, Some('a'));
}

#[test]
fn test_cleanup() {
    let mut driver = PS2KeyboardDriver::new();
    driver.init(vec![]).unwrap();
    
    // Add some events and set modifiers
    driver.process_scancode(0x2A); // Shift press
    driver.process_scancode(0x1E); // A key
    assert!(driver.has_events());
    assert!(driver.modifiers.contains(KeyModifiers::SHIFT));
    
    // Test cleanup
    let result = driver.cleanup();
    assert!(result.is_ok());
    assert_eq!(driver.get_status(), DriverStatus::Uninitialized);
    assert!(!driver.has_events());
    assert!(driver.modifiers.is_empty());
}