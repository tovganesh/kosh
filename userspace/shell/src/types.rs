#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
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

/// Environment variable management using BTreeMap for ordered, efficient storage.
#[derive(Debug, Clone)]
pub struct Environment {
    variables: BTreeMap<String, String>,
    pub working_directory: String,
}

impl Environment {
    /// Create a new empty environment.
    pub fn new() -> Self {
        Self {
            variables: BTreeMap::new(),
            working_directory: String::from("/"),
        }
    }

    /// Create a new environment pre-populated with built-in variables.
    pub fn with_defaults() -> Self {
        let mut env = Self::new();
        env.set_var("PWD".to_string(), "/".to_string());
        env.set_var("HOME".to_string(), "/home/user".to_string());
        env.set_var("PATH".to_string(), "/bin:/usr/bin".to_string());
        env.set_var("SHELL".to_string(), "/bin/kosh-shell".to_string());
        env.set_var("USER".to_string(), "user".to_string());
        env.set_var("HOSTNAME".to_string(), "kosh".to_string());
        env
    }

    /// Get the value of an environment variable.
    pub fn get_var(&self, name: &str) -> Option<&str> {
        self.variables.get(name).map(|v| v.as_str())
    }

    /// Set an environment variable. Updates PWD/working_directory in sync.
    pub fn set_var(&mut self, name: String, value: String) {
        if name == "PWD" {
            self.working_directory = value.clone();
        }
        self.variables.insert(name, value);
    }

    /// Remove an environment variable. Built-in variables like PWD cannot be unset.
    pub fn unset_var(&mut self, name: &str) -> bool {
        self.variables.remove(name).is_some()
    }

    /// Return an iterator over all environment variables in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.variables.iter()
    }

    /// Return the number of environment variables.
    pub fn len(&self) -> usize {
        self.variables.len()
    }

    /// Check if the environment has no variables.
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }

    /// Expand environment variables in the given input string.
    /// Supports $VAR and ${VAR} syntax. Single-quoted strings are not expanded.
    pub fn expand_variables(&self, input: &str) -> String {
        crate::parser::expand_variables(input, &|name: &str| {
            self.get_var(name).map(|v| String::from(v))
        })
    }

    /// Get the PATH entries as a vector of strings.
    pub fn path_entries(&self) -> Vec<String> {
        match self.get_var("PATH") {
            Some(path) => path.split(':').map(|s| String::from(s)).collect(),
            None => Vec::new(),
        }
    }

    /// Format all variables for display (used by `env` command).
    pub fn format_all(&self) -> String {
        let mut output = String::new();
        for (key, value) in self.variables.iter() {
            output.push_str(key);
            output.push('=');
            output.push_str(value);
            output.push('\n');
        }
        // Remove trailing newline
        if output.ends_with('\n') {
            output.pop();
        }
        output
    }

    /// Parse an "export" argument of the form NAME=VALUE.
    /// Returns (name, value) if valid, or None if the format is invalid.
    pub fn parse_assignment(arg: &str) -> Option<(String, String)> {
        let eq_pos = arg.find('=')?;
        let name = &arg[..eq_pos];
        if name.is_empty() || !Self::is_valid_var_name(name) {
            return None;
        }
        let value = &arg[eq_pos + 1..];
        Some((String::from(name), String::from(value)))
    }

    /// Check if a variable name is valid (starts with letter or _, contains only alphanumeric or _).
    pub fn is_valid_var_name(name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
            _ => return false,
        }
        chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
}