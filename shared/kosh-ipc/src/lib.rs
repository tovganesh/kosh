#![no_std]

use kosh_types::{ProcessId, MessageType, Capability};

#[derive(Debug)]
pub struct Message {
    pub sender: ProcessId,
    pub receiver: ProcessId,
    pub message_type: MessageType,
    pub data: MessageData,
    pub capabilities: &'static [Capability],
}

#[derive(Debug)]
pub enum MessageData {
    Empty,
    Bytes(&'static [u8]),
    SystemCall(SystemCallData),
    DriverRequest(DriverRequestData),
}

#[derive(Debug)]
pub struct SystemCallData {
    pub call_number: u64,
    pub args: [u64; 6],
}

#[derive(Debug)]
pub struct DriverRequestData {
    pub driver_id: u32,
    pub request_type: u32,
    pub data: &'static [u8],
}

pub trait IpcChannel {
    fn send(&mut self, message: Message) -> Result<(), IpcError>;
    fn receive(&mut self) -> Result<Message, IpcError>;
}

#[derive(Debug)]
pub enum IpcError {
    InvalidReceiver,
    MessageTooLarge,
    ChannelFull,
    PermissionDenied,
    Timeout,
}