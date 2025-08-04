use alloc::collections::VecDeque;
use alloc::collections::BTreeMap;
use spin::Mutex;
use crate::process::ProcessId;
use crate::ipc::message::{Message, MessageError};
use crate::{serial_println};

/// Maximum number of messages per process queue
const MAX_MESSAGES_PER_QUEUE: usize = 256;

/// Maximum total message size per queue (in bytes)
const MAX_QUEUE_SIZE_BYTES: usize = 64 * 1024; // 64KB per queue

/// Message queue for a single process
#[derive(Debug)]
pub struct MessageQueue {
    /// Process ID that owns this queue
    pub process_id: ProcessId,
    /// Queue of pending messages
    pub messages: VecDeque<Message>,
    /// Current total size of messages in bytes
    pub total_size: usize,
    /// Maximum number of messages allowed
    pub max_messages: usize,
    /// Maximum total size allowed
    pub max_size: usize,
    /// Statistics
    pub messages_received: u64,
    pub messages_sent: u64,
    pub queue_full_count: u64,
}

impl MessageQueue {
    /// Create a new message queue for a process
    pub fn new(process_id: ProcessId) -> Self {
        Self {
            process_id,
            messages: VecDeque::new(),
            total_size: 0,
            max_messages: MAX_MESSAGES_PER_QUEUE,
            max_size: MAX_QUEUE_SIZE_BYTES,
            messages_received: 0,
            messages_sent: 0,
            queue_full_count: 0,
        }
    }
    
    /// Check if the queue can accept a new message
    pub fn can_accept_message(&self, message: &Message) -> bool {
        let message_size = message.total_size();
        
        // Check message count limit
        if self.messages.len() >= self.max_messages {
            return false;
        }
        
        // Check size limit
        if self.total_size + message_size > self.max_size {
            return false;
        }
        
        true
    }
    
    /// Add a message to the queue
    pub fn enqueue(&mut self, message: Message) -> Result<(), MessageError> {
        if !self.can_accept_message(&message) {
            self.queue_full_count += 1;
            return Err(MessageError::QueueFull);
        }
        
        let message_size = message.total_size();
        
        // Insert message in priority order (higher priority = lower number)
        let insert_pos = self.messages.iter()
            .position(|m| m.header.priority > message.header.priority)
            .unwrap_or(self.messages.len());
        
        self.messages.insert(insert_pos, message);
        self.total_size += message_size;
        self.messages_received += 1;
        
        serial_println!("Enqueued message for process {} (queue size: {})", 
                       self.process_id.0, self.messages.len());
        
        Ok(())
    }
    
    /// Remove and return the next message from the queue
    pub fn dequeue(&mut self) -> Result<Message, MessageError> {
        if let Some(message) = self.messages.pop_front() {
            let message_size = message.total_size();
            self.total_size = self.total_size.saturating_sub(message_size);
            
            serial_println!("Dequeued message for process {} (queue size: {})", 
                           self.process_id.0, self.messages.len());
            
            Ok(message)
        } else {
            Err(MessageError::NoMessage)
        }
    }
    
    /// Peek at the next message without removing it
    pub fn peek(&self) -> Option<&Message> {
        self.messages.front()
    }
    
    /// Get the number of messages in the queue
    pub fn len(&self) -> usize {
        self.messages.len()
    }
    
    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
    
    /// Check if the queue is full
    pub fn is_full(&self) -> bool {
        self.messages.len() >= self.max_messages || self.total_size >= self.max_size
    }
    
    /// Get queue statistics
    pub fn get_statistics(&self) -> MessageQueueStatistics {
        MessageQueueStatistics {
            process_id: self.process_id,
            pending_messages: self.messages.len(),
            total_size_bytes: self.total_size,
            messages_received: self.messages_received,
            messages_sent: self.messages_sent,
            queue_full_count: self.queue_full_count,
            max_messages: self.max_messages,
            max_size_bytes: self.max_size,
        }
    }
    
    /// Clear all messages from the queue
    pub fn clear(&mut self) {
        self.messages.clear();
        self.total_size = 0;
        serial_println!("Cleared message queue for process {}", self.process_id.0);
    }
}

/// Message queue statistics
#[derive(Debug, Clone)]
pub struct MessageQueueStatistics {
    pub process_id: ProcessId,
    pub pending_messages: usize,
    pub total_size_bytes: usize,
    pub messages_received: u64,
    pub messages_sent: u64,
    pub queue_full_count: u64,
    pub max_messages: usize,
    pub max_size_bytes: usize,
}

/// Message queue management errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageQueueError {
    /// Queue not found for the specified process
    QueueNotFound,
    /// Queue already exists for the process
    QueueAlreadyExists,
    /// System resource exhausted
    ResourceExhausted,
    /// Invalid process ID
    InvalidProcessId,
}

/// Global message queue manager
struct MessageQueueManager {
    /// Map of process ID to message queue
    queues: BTreeMap<ProcessId, MessageQueue>,
    /// Total number of messages across all queues
    total_messages: u64,
    /// Total number of queues created
    total_queues_created: u64,
}

impl MessageQueueManager {
    /// Create a new message queue manager
    fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
            total_messages: 0,
            total_queues_created: 0,
        }
    }
    
    /// Create a message queue for a process
    fn create_queue(&mut self, process_id: ProcessId) -> Result<(), MessageQueueError> {
        if self.queues.contains_key(&process_id) {
            return Err(MessageQueueError::QueueAlreadyExists);
        }
        
        let queue = MessageQueue::new(process_id);
        self.queues.insert(process_id, queue);
        self.total_queues_created += 1;
        
        serial_println!("Created message queue for process {}", process_id.0);
        Ok(())
    }
    
    /// Get a message queue for a process (create if it doesn't exist)
    fn get_or_create_queue(&mut self, process_id: ProcessId) -> &mut MessageQueue {
        if !self.queues.contains_key(&process_id) {
            let _ = self.create_queue(process_id);
        }
        self.queues.get_mut(&process_id).unwrap()
    }
    
    /// Remove a message queue for a process
    fn remove_queue(&mut self, process_id: ProcessId) -> Result<MessageQueue, MessageQueueError> {
        self.queues.remove(&process_id)
            .ok_or(MessageQueueError::QueueNotFound)
    }
    
    /// Enqueue a message to a process's queue
    fn enqueue_message(&mut self, process_id: ProcessId, message: Message) -> Result<(), MessageError> {
        let queue = self.get_or_create_queue(process_id);
        queue.enqueue(message)?;
        self.total_messages += 1;
        Ok(())
    }
    
    /// Dequeue a message from a process's queue
    fn dequeue_message(&mut self, process_id: ProcessId) -> Result<Message, MessageError> {
        let queue = self.queues.get_mut(&process_id)
            .ok_or(MessageError::ReceiverNotFound)?;
        
        let message = queue.dequeue()?;
        self.total_messages = self.total_messages.saturating_sub(1);
        Ok(message)
    }
    
    /// Get queue statistics for a process
    fn get_queue_statistics(&self, process_id: ProcessId) -> Option<MessageQueueStatistics> {
        self.queues.get(&process_id).map(|q| q.get_statistics())
    }
    
    /// Get global queue system statistics
    fn get_global_statistics(&self) -> GlobalQueueStatistics {
        let mut total_pending = 0;
        let mut total_received = 0;
        let mut total_sent = 0;
        let mut total_queue_full = 0;
        
        for queue in self.queues.values() {
            total_pending += queue.len();
            total_received += queue.messages_received;
            total_sent += queue.messages_sent;
            total_queue_full += queue.queue_full_count;
        }
        
        GlobalQueueStatistics {
            active_queues: self.queues.len(),
            total_pending_messages: total_pending,
            total_messages_sent: total_sent,
            total_messages_received: total_received,
            total_queue_full_events: total_queue_full,
            total_queues_created: self.total_queues_created,
        }
    }
}

/// Global queue system statistics
#[derive(Debug, Clone)]
pub struct GlobalQueueStatistics {
    pub active_queues: usize,
    pub total_pending_messages: usize,
    pub total_messages_sent: u64,
    pub total_messages_received: u64,
    pub total_queue_full_events: u64,
    pub total_queues_created: u64,
}

/// Global message queue manager instance
static MESSAGE_QUEUE_MANAGER: Mutex<Option<MessageQueueManager>> = Mutex::new(None);

/// Initialize the message queue system
pub fn init_message_queues() -> Result<(), &'static str> {
    serial_println!("Initializing message queue system...");
    
    let manager = MessageQueueManager::new();
    *MESSAGE_QUEUE_MANAGER.lock() = Some(manager);
    
    serial_println!("Message queue system initialized");
    Ok(())
}

/// Create a message queue for a process
pub fn create_message_queue(process_id: ProcessId) -> Result<(), MessageQueueError> {
    let mut manager = MESSAGE_QUEUE_MANAGER.lock();
    let manager = manager.as_mut().ok_or(MessageQueueError::ResourceExhausted)?;
    manager.create_queue(process_id)
}

/// Get message queue statistics for a process
pub fn get_message_queue(process_id: ProcessId) -> Option<MessageQueueStatistics> {
    let manager = MESSAGE_QUEUE_MANAGER.lock();
    let manager = manager.as_ref()?;
    manager.get_queue_statistics(process_id)
}

/// Enqueue a message to a process's queue
pub fn enqueue_message(process_id: ProcessId, message: Message) -> Result<(), MessageError> {
    let mut manager = MESSAGE_QUEUE_MANAGER.lock();
    let manager = manager.as_mut().ok_or(MessageError::ResourceExhausted)?;
    manager.enqueue_message(process_id, message)
}

/// Dequeue a message from a process's queue
pub fn dequeue_message(process_id: ProcessId) -> Result<Message, MessageError> {
    let mut manager = MESSAGE_QUEUE_MANAGER.lock();
    let manager = manager.as_mut().ok_or(MessageError::ResourceExhausted)?;
    manager.dequeue_message(process_id)
}

/// Remove a message queue for a process
pub fn remove_message_queue(process_id: ProcessId) -> Result<(), MessageQueueError> {
    let mut manager = MESSAGE_QUEUE_MANAGER.lock();
    let manager = manager.as_mut().ok_or(MessageQueueError::ResourceExhausted)?;
    manager.remove_queue(process_id)?;
    Ok(())
}

/// Get global queue system statistics
pub fn get_queue_statistics() -> GlobalQueueStatistics {
    let manager = MESSAGE_QUEUE_MANAGER.lock();
    if let Some(manager) = manager.as_ref() {
        manager.get_global_statistics()
    } else {
        GlobalQueueStatistics {
            active_queues: 0,
            total_pending_messages: 0,
            total_messages_sent: 0,
            total_messages_received: 0,
            total_queue_full_events: 0,
            total_queues_created: 0,
        }
    }
}

/// Print message queue information
pub fn print_queue_info() {
    let stats = get_queue_statistics();
    
    serial_println!("Message Queue Statistics:");
    serial_println!("  Active queues: {}", stats.active_queues);
    serial_println!("  Pending messages: {}", stats.total_pending_messages);
    serial_println!("  Messages sent: {}", stats.total_messages_sent);
    serial_println!("  Messages received: {}", stats.total_messages_received);
    serial_println!("  Queue full events: {}", stats.total_queue_full_events);
    serial_println!("  Total queues created: {}", stats.total_queues_created);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::message::{MessageType, MessageData};
    use alloc::string::ToString;
    
    #[test_case]
    fn test_message_queue_creation() {
        let process_id = ProcessId::new(1);
        let mut queue = MessageQueue::new(process_id);
        
        assert_eq!(queue.process_id, process_id);
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
        assert!(!queue.is_full());
    }
    
    #[test_case]
    fn test_message_queue_enqueue_dequeue() {
        let process_id = ProcessId::new(1);
        let mut queue = MessageQueue::new(process_id);
        
        let message = crate::ipc::message::Message::new(
            ProcessId::new(2),
            process_id,
            MessageType::ServiceRequest,
            MessageData::Text("test".to_string()),
        );
        
        // Test enqueue
        assert!(queue.enqueue(message.clone()).is_ok());
        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());
        
        // Test dequeue
        let dequeued = queue.dequeue().unwrap();
        assert_eq!(dequeued.header.message_id, message.header.message_id);
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
        
        // Test dequeue from empty queue
        assert_eq!(queue.dequeue().unwrap_err(), MessageError::NoMessage);
    }
    
    #[test_case]
    fn test_message_priority_ordering() {
        let process_id = ProcessId::new(1);
        let mut queue = MessageQueue::new(process_id);
        
        // Create messages with different priorities
        let mut low_priority = crate::ipc::message::Message::new(
            ProcessId::new(2),
            process_id,
            MessageType::ServiceRequest,
            MessageData::Text("low".to_string()),
        );
        low_priority.set_priority(200);
        
        let mut high_priority = crate::ipc::message::Message::new(
            ProcessId::new(2),
            process_id,
            MessageType::ServiceRequest,
            MessageData::Text("high".to_string()),
        );
        high_priority.set_priority(50);
        
        // Enqueue in reverse priority order
        queue.enqueue(low_priority).unwrap();
        queue.enqueue(high_priority).unwrap();
        
        // High priority message should come out first
        let first = queue.dequeue().unwrap();
        assert_eq!(first.header.priority, 50);
        
        let second = queue.dequeue().unwrap();
        assert_eq!(second.header.priority, 200);
    }
    
    #[test_case]
    fn test_queue_size_limits() {
        let process_id = ProcessId::new(1);
        let mut queue = MessageQueue::new(process_id);
        
        // Set small limits for testing
        queue.max_messages = 2;
        queue.max_size = 100;
        
        let message = crate::ipc::message::Message::new(
            ProcessId::new(2),
            process_id,
            MessageType::ServiceRequest,
            MessageData::Text("test".to_string()),
        );
        
        // Should be able to add up to the limit
        assert!(queue.enqueue(message.clone()).is_ok());
        assert!(queue.enqueue(message.clone()).is_ok());
        
        // Third message should fail
        assert_eq!(queue.enqueue(message).unwrap_err(), MessageError::QueueFull);
        assert_eq!(queue.queue_full_count, 1);
    }
}