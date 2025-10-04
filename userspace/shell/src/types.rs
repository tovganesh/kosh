#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use kosh_types::ProcessId;

/// Core types for the enhanced shell

/// Represents a parsed command with all its components
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    pub command: String,
    pub args: Vec<String>,
    pub input_redirect: Option<String>,
    pub output_redirect: Option<RedirectType>,
    pub pipe_to: Option<Box<ParsedCommand>>,
    pub background: bool,
    pub conditional: Option<ConditionalType>,
}

/// Types of output redirection
#[derive(Debug, Clone)]
pub enum RedirectType {
    Overwrite(String),  // >
    Append(String),     // >>
    Error(String),      // 2>
}

/// Conditional execution types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalType {
    And,    // &&
    Or,     // ||
}

/// Command execution result
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub exit_code: i32,
    pub output: String,
    pub error: Option<String>,
}

/// Background job information
#[derive(Debug, Clone)]
pub struct BackgroundJob {
    pub job_id: u32,
    pub pid: ProcessId,
    pub command: String,
    pub status: JobStatus,
}

/// Job status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Stopped,
    Completed(i32),
}

/// Command history entry
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub command: String,
    pub timestamp: u64, // Simple timestamp for now
    pub exit_code: Option<i32>,
    pub working_directory: String,
}

/// Special key types for input handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKey {
    Backspace,
    Enter,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Delete,
    Home,
    End,
    CtrlC,
    CtrlD,
    CtrlZ,
}

/// Key action results
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Continue,
    Complete,
    Interrupt,
    Suspend,
    Exit,
}

/// Text color enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

/// File listing flags
#[derive(Debug, Clone, Copy, Default)]
pub struct LsFlags {
    pub long_format: bool,      // -l
    pub show_hidden: bool,      // -a
    pub human_readable: bool,   // -h
    pub recursive: bool,        // -R
    pub sort_by_time: bool,     // -t
    pub reverse_sort: bool,     // -r
}

/// Process information for ps command
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: ProcessId,
    pub ppid: ProcessId,
    pub name: String,
    pub state: String,
    pub cpu_time: u64,
    pub memory_usage: usize,
}

/// System information structure
#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub os_name: String,
    pub version: String,
    pub architecture: String,
    pub uptime: u64,
    pub load_average: [f32; 3],
    pub total_memory: usize,
    pub free_memory: usize,
}

/// File system information
#[derive(Debug, Clone)]
pub struct FileSystemInfo {
    pub device: String,
    pub mount_point: String,
    pub fs_type: String,
    pub total_space: u64,
    pub free_space: u64,
    pub used_space: u64,
}

/// Environment variable management
#[derive(Debug, Clone)]
pub struct Environment {
    pub variables: Vec<(String, String)>,
    pub working_directory: String,
    pub path: Vec<String>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            variables: Vec::new(),
            working_directory: String::from("/"),
            path: Vec::new(),
        }
    }
    
    pub fn get_var(&self, name: &str) -> Option<&str> {
        self.variables.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
    
    pub fn set_var(&mut self, name: String, value: String) {
        if let Some(pos) = self.variables.iter().position(|(key, _)| key == &name) {
            self.variables[pos].1 = value;
        } else {
            self.variables.push((name, value));
        }
    }
    
    pub fn unset_var(&mut self, name: &str) {
        self.variables.retain(|(key, _)| key != name);
    }
}