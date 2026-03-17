#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use crate::history::CommandHistory;
use crate::types::{SpecialKey, KeyAction};

/// Enhanced input handler with command history integration.
///
/// Manages the current input buffer, cursor position, and delegates
/// history navigation to the embedded `CommandHistory`.
pub struct InputHandler {
    /// Raw input buffer (bytes from keyboard)
    input_buffer: Vec<u8>,
    /// Current line being edited (character-level)
    line_buffer: Vec<char>,
    /// Cursor position within `line_buffer`
    cursor_position: usize,
    /// Command history
    history: CommandHistory,
    /// Saved partial input when navigating history
    saved_input: Option<String>,
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            input_buffer: Vec::with_capacity(256),
            line_buffer: Vec::with_capacity(256),
            cursor_position: 0,
            history: CommandHistory::new(),
            saved_input: None,
        }
    }

    /// Create an InputHandler with a pre-existing history.
    pub fn with_history(history: CommandHistory) -> Self {
        Self {
            input_buffer: Vec::with_capacity(256),
            line_buffer: Vec::with_capacity(256),
            cursor_position: 0,
            history,
            saved_input: None,
        }
    }

    /// Get a reference to the command history.
    pub fn history(&self) -> &CommandHistory {
        &self.history
    }

    /// Get a mutable reference to the command history.
    pub fn history_mut(&mut self) -> &mut CommandHistory {
        &mut self.history
    }

    pub fn read_line(&mut self) -> String {
        self.input_buffer.clear();
        self.line_buffer.clear();
        self.cursor_position = 0;
        self.saved_input = None;

        // In a real implementation, this would:
        // 1. Read from keyboard driver via IPC
        // 2. Handle special keys (backspace, enter, arrows, etc.)
        // 3. Echo characters to display
        // 4. Return complete line when enter is pressed

        // For now, simulate some basic commands for testing
        static mut COMMAND_INDEX: usize = 0;
        let test_commands = [
            "help",
            "echo Hello, Kosh!",
            "ps",
            "ls",
            "pwd",
            "exit",
        ];

        unsafe {
            let command = test_commands[COMMAND_INDEX % test_commands.len()];
            COMMAND_INDEX += 1;
            String::from(command)
        }
    }

    fn read_char(&self) -> Option<char> {
        // In a real implementation, this would read a single character
        // from the keyboard driver via system calls
        None
    }

    /// Handle a special key press. Returns a `KeyAction` indicating what
    /// the shell main loop should do.
    pub fn handle_special_key(&mut self, key: SpecialKey) -> KeyAction {
        match key {
            SpecialKey::Backspace => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                    self.line_buffer.remove(self.cursor_position);
                }
                KeyAction::Continue
            }
            SpecialKey::Delete => {
                if self.cursor_position < self.line_buffer.len() {
                    self.line_buffer.remove(self.cursor_position);
                }
                KeyAction::Continue
            }
            SpecialKey::Enter => {
                let line: String = self.line_buffer.iter().collect();
                if !line.trim().is_empty() {
                    self.history.add(line, String::from("/"));
                }
                self.history.reset_navigation();
                self.saved_input = None;
                KeyAction::Complete
            }
            SpecialKey::Tab => {
                // Tab completion placeholder
                KeyAction::Continue
            }
            SpecialKey::ArrowUp => {
                self.navigate_history_up();
                KeyAction::Continue
            }
            SpecialKey::ArrowDown => {
                self.navigate_history_down();
                KeyAction::Continue
            }
            SpecialKey::ArrowLeft => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
                KeyAction::Continue
            }
            SpecialKey::ArrowRight => {
                if self.cursor_position < self.line_buffer.len() {
                    self.cursor_position += 1;
                }
                KeyAction::Continue
            }
            SpecialKey::Home => {
                self.cursor_position = 0;
                KeyAction::Continue
            }
            SpecialKey::End => {
                self.cursor_position = self.line_buffer.len();
                KeyAction::Continue
            }
            SpecialKey::CtrlC => KeyAction::Interrupt,
            SpecialKey::CtrlD => KeyAction::Exit,
            SpecialKey::CtrlZ => KeyAction::Suspend,
        }
    }

    /// Insert a character at the current cursor position.
    pub fn insert_char(&mut self, ch: char) {
        self.line_buffer.insert(self.cursor_position, ch);
        self.cursor_position += 1;
    }

    /// Get the current line buffer as a string.
    pub fn current_line(&self) -> String {
        self.line_buffer.iter().collect()
    }

    /// Get the current cursor position.
    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    // ── History navigation helpers ───────────────────────────────────

    fn navigate_history_up(&mut self) {
        // Save current input on first up-press
        if self.history.current_index().is_none() {
            self.saved_input = Some(self.current_line());
        }

        if let Some(cmd) = self.history.navigate_up() {
            let cmd_owned = String::from(cmd);
            self.set_line(&cmd_owned);
        }
    }

    fn navigate_history_down(&mut self) {
        match self.history.navigate_down() {
            Some(cmd) => {
                let cmd_owned = String::from(cmd);
                self.set_line(&cmd_owned);
            }
            None => {
                // Restore saved input when moving past newest entry
                if let Some(saved) = self.saved_input.take() {
                    self.set_line(&saved);
                } else {
                    self.line_buffer.clear();
                    self.cursor_position = 0;
                }
            }
        }
    }

    /// Replace the line buffer contents and move cursor to end.
    fn set_line(&mut self, text: &str) {
        self.line_buffer.clear();
        self.line_buffer.extend(text.chars());
        self.cursor_position = self.line_buffer.len();
    }
}
