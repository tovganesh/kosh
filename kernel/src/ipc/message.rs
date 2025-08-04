use alloc::{vec, vec::Vec};
use alloc::string::String;
use core::fmt;
use crate::process::ProcessId;
use crate::ipc::capability::CapabilitySet;
use crate::{serial_println};

/// Message identifier type
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId(pub u64);

impl MessageId {
    /// Create a new message ID
    pub fn new(id: u64) -> Self {
        MessageId(id)
    }
    
    /// Get the raw ID value
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Message types for different kinds of IPC
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// System call from user space to kernel
    SystemCall,
    /// Request to a driver
    DriverRequest,
    /// Request to a system service
    ServiceRequest,
    /// Signal notification
    Signal,
    /// Response to a previous message
    Response,
    /// Error response
    Error,
}

/// Message data payload
#[derive(Debug, Clone)]
pub enum MessageData {
    /// Empty message (for signals, notifications)
    Empty,
    /// Raw byte data
    Bytes(Vec<u8>),
    /// Text message
    Text(String),
    /// Structured data with type identifier
    Structured {
        type_id: u32,
        data: Vec<u8>,
    },
    /// System call data
    SystemCall {
        call_number: u32,
        args: [u64; 6],
    },
    /// Error information
    Error {
        error_code: u32,
        message: String,
    },
}

impl MessageData {
    /// Get the size of the message data in bytes
    pub fn size(&self) -> usize {
        match self {
            MessageData::Empty => 0,
            MessageData::Bytes(data) => data.len(),
            MessageData::Text(text) => text.len(),
            MessageData::Structured { data, .. } => data.len() + 4, // +4 for type_id
            MessageData::SystemCall { .. } => 4 + 6 * 8, // call_number + 6 u64 args
            MessageData::Error { message, .. } => 4 + message.len(), // error_code + message
        }
    }
    
    /// Check if the message data is empty
    pub fn is_empty(&self) -> bool {
        matches!(self, MessageData::Empty)
    }
}

/// Message header containing metadata
#[derive(Debug, Clone)]
pub struct MessageHeader {
    /// Unique message identifier
    pub message_id: MessageId,
    /// Sender process ID
    pub sender: ProcessId,
    /// Receiver process ID
    pub receiver: ProcessId,
    /// Message type
    pub message_type: MessageType,
    /// Message priority (0 = highest, 255 = lowest)
    pub priority: u8,
    /// Timestamp when message was created (milliseconds since boot)
    pub timestamp: u64,
    /// Reply-to message ID (for responses)
    pub reply_to: Option<MessageId>,
    /// Message flags
    pub flags: MessageFlags,
}

/// Message flags for special handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageFlags {
    /// Message requires synchronous handling
    pub synchronous: bool,
    /// Message should not be queued if receiver is busy
    pub no_queue: bool,
    /// Message contains capabilities
    pub has_capabilities: bool,
    /// Message is a broadcast to multiple receivers
    pub broadcast: bool,
}

impl Default for MessageFlags {
    fn default() -> Self {
        Self {
            synchronous: false,
            no_queue: false,
            has_capabilities: false,
            broadcast: false,
        }
    }
}

/// Complete message structure
#[derive(Debug, Clone)]
pub struct Message {
    /// Message header with metadata
    pub header: MessageHeader,
    /// Message payload data
    pub data: MessageData,
    /// Capabilities attached to this message
    pub capabilities: CapabilitySet,
}

impl Message {
    /// Create a new message
    pub fn new(
        sender: ProcessId,
        receiver: ProcessId,
        message_type: MessageType,
        data: MessageData,
    ) -> Self {
        let message_id = generate_message_id();
        let timestamp = get_current_time_ms();
        
        let mut flags = MessageFlags::default();
        flags.has_capabilities = false; // Will be set when capabilities are added
        
        Self {
            header: MessageHeader {
                message_id,
                sender,
                receiver,
                message_type,
                priority: 128, // Default priority
                timestamp,
                reply_to: None,
                flags,
            },
            data,
            capabilities: CapabilitySet::new(),
        }
    }
    
    /// Create a reply message to this message
    pub fn create_reply(
        &self,
        sender: ProcessId,
        data: MessageData,
    ) -> Self {
        let message_id = generate_message_id();
        let timestamp = get_current_time_ms();
        
        Self {
            header: MessageHeader {
                message_id,
                sender,
                receiver: self.header.sender, // Reply to original sender
                message_type: MessageType::Response,
                priority: self.header.priority, // Inherit priority
                timestamp,
                reply_to: Some(self.header.message_id),
                flags: MessageFlags::default(),
            },
            data,
            capabilities: CapabilitySet::new(),
        }
    }
    
    /// Set message priority (0 = highest, 255 = lowest)
    pub fn set_priority(&mut self, priority: u8) {
        self.header.priority = priority;
    }
    
    /// Set synchronous flag
    pub fn set_synchronous(&mut self, synchronous: bool) {
        self.header.flags.synchronous = synchronous;
    }
    
    /// Add capabilities to the message
    pub fn add_capabilities(&mut self, capabilities: CapabilitySet) {
        self.capabilities = capabilities;
        self.header.flags.has_capabilities = !self.capabilities.is_empty();
    }
    
    /// Get the total size of the message
    pub fn total_size(&self) -> usize {
        core::mem::size_of::<MessageHeader>() + 
        self.data.size() + 
        self.capabilities.size()
    }
    
    /// Check if this is a reply message
    pub fn is_reply(&self) -> bool {
        self.header.reply_to.is_some()
    }
    
    /// Check if this message requires synchronous handling
    pub fn is_synchronous(&self) -> bool {
        self.header.flags.synchronous
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Message[{}] {} -> {} ({:?})", 
               self.header.message_id.0,
               self.header.sender.0,
               self.header.receiver.0,
               self.header.message_type)
    }
}

/// Message handling errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageError {
    /// Invalid message format or content
    InvalidMessage,
    /// Sender process not found
    SenderNotFound,
    /// Receiver process not found
    ReceiverNotFound,
    /// Message queue is full
    QueueFull,
    /// No message available to receive
    NoMessage,
    /// Permission denied for this operation
    PermissionDenied,
    /// Message too large
    MessageTooLarge,
    /// Timeout waiting for message
    Timeout,
    /// System resource exhausted
    ResourceExhausted,
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageError::InvalidMessage => write!(f, "Invalid message"),
            MessageError::SenderNotFound => write!(f, "Sender process not found"),
            MessageError::ReceiverNotFound => write!(f, "Receiver process not found"),
            MessageError::QueueFull => write!(f, "Message queue is full"),
            MessageError::NoMessage => write!(f, "No message available"),
            MessageError::PermissionDenied => write!(f, "Permission denied"),
            MessageError::MessageTooLarge => write!(f, "Message too large"),
            MessageError::Timeout => write!(f, "Timeout waiting for message"),
            MessageError::ResourceExhausted => write!(f, "System resource exhausted"),
        }
    }
}

/// Global message ID counter
static mut NEXT_MESSAGE_ID: u64 = 1;

/// Generate a unique message ID
fn generate_message_id() -> MessageId {
    unsafe {
        let id = NEXT_MESSAGE_ID;
        NEXT_MESSAGE_ID += 1;
        MessageId::new(id)
    }
}

/// Create a new message
pub fn create_message(
    sender: ProcessId,
    receiver: ProcessId,
    message_type: MessageType,
    data: MessageData,
) -> Message {
    let message = Message::new(sender, receiver, message_type, data);
    serial_println!("Created message {} from {} to {}", 
                   message.header.message_id.0, sender.0, receiver.0);
    message
}

/// Send a message to another process
pub fn send_message(message: Message) -> Result<(), MessageError> {
    serial_println!("Sending message {} from {} to {}", 
                   message.header.message_id.0, 
                   message.header.sender.0, 
                   message.header.receiver.0);
    
    // Validate sender exists
    if crate::process::get_process(message.header.sender).is_none() {
        return Err(MessageError::SenderNotFound);
    }
    
    // Validate receiver exists
    if crate::process::get_process(message.header.receiver).is_none() {
        return Err(MessageError::ReceiverNotFound);
    }
    
    // Check capabilities for message sending
    if !crate::ipc::capability::check_capability(
        message.header.sender,
        crate::ipc::capability::CapabilityType::SendMessage,
        &crate::ipc::capability::ResourceId::Process(message.header.receiver),
    ) {
        serial_println!("Permission denied: sender {} lacks SendMessage capability for receiver {}", 
                       message.header.sender.0, message.header.receiver.0);
        return Err(MessageError::PermissionDenied);
    }
    
    // Add message to receiver's queue
    crate::ipc::queue::enqueue_message(message.header.receiver, message)?;
    
    Ok(())
}

/// Receive a message for the current process
pub fn receive_message(receiver: ProcessId) -> Result<Message, MessageError> {
    serial_println!("Process {} attempting to receive message", receiver.0);
    
    // Validate receiver exists
    if crate::process::get_process(receiver).is_none() {
        return Err(MessageError::ReceiverNotFound);
    }
    
    // Check capabilities for message receiving
    // Note: For now, we allow any process to receive messages sent to them
    // In a more sophisticated system, we might require ReceiveMessage capability
    
    // Get message from receiver's queue
    let message = crate::ipc::queue::dequeue_message(receiver)?;
    
    serial_println!("Process {} received message {} from {}", 
                   receiver.0, message.header.message_id.0, message.header.sender.0);
    
    Ok(message)
}

/// Send a reply message
pub fn reply_message(
    original_message: &Message,
    sender: ProcessId,
    data: MessageData,
) -> Result<(), MessageError> {
    let reply = original_message.create_reply(sender, data);
    send_message(reply)
}

/// Placeholder function for getting current time
/// TODO: Replace with actual timer implementation
fn get_current_time_ms() -> u64 {
    // For now, return 0. This will be replaced with actual timer implementation
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    
    #[test_case]
    fn test_message_id_creation() {
        let id = MessageId::new(42);
        assert_eq!(id.as_u64(), 42);
    }
    
    #[test_case]
    fn test_message_data_size() {
        assert_eq!(MessageData::Empty.size(), 0);
        assert_eq!(MessageData::Bytes(vec![1, 2, 3]).size(), 3);
        assert_eq!(MessageData::Text("hello".to_string()).size(), 5);
        
        let syscall_data = MessageData::SystemCall {
            call_number: 1,
            args: [0; 6],
        };
        assert_eq!(syscall_data.size(), 4 + 6 * 8);
    }
    
    #[test_case]
    fn test_message_creation() {
        let sender = ProcessId::new(1);
        let receiver = ProcessId::new(2);
        let data = MessageData::Text("test message".to_string());
        
        let message = Message::new(sender, receiver, MessageType::ServiceRequest, data);
        
        assert_eq!(message.header.sender, sender);
        assert_eq!(message.header.receiver, receiver);
        assert_eq!(message.header.message_type, MessageType::ServiceRequest);
        assert_eq!(message.header.priority, 128);
        assert!(!message.is_reply());
        assert!(!message.is_synchronous());
    }
    
    #[test_case]
    fn test_reply_message_creation() {
        let sender = ProcessId::new(1);
        let receiver = ProcessId::new(2);
        let original_data = MessageData::Text("request".to_string());
        
        let original = Message::new(sender, receiver, MessageType::ServiceRequest, original_data);
        let reply_data = MessageData::Text("response".to_string());
        let reply = original.create_reply(receiver, reply_data);
        
        assert_eq!(reply.header.sender, receiver);
        assert_eq!(reply.header.receiver, sender); // Reply goes back to original sender
        assert_eq!(reply.header.message_type, MessageType::Response);
        assert_eq!(reply.header.reply_to, Some(original.header.message_id));
        assert!(reply.is_reply());
    }
    
    #[test_case]
    fn test_message_flags() {
        let mut message = Message::new(
            ProcessId::new(1),
            ProcessId::new(2),
            MessageType::SystemCall,
            MessageData::Empty,
        );
        
        assert!(!message.is_synchronous());
        message.set_synchronous(true);
        assert!(message.is_synchronous());
        
        message.set_priority(0);
        assert_eq!(message.header.priority, 0);
    }
}