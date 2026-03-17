#![allow(dead_code)]

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::types::HistoryEntry;

/// Default maximum number of history entries
const DEFAULT_MAX_HISTORY: usize = 1000;

/// Default history file path
const DEFAULT_HISTORY_FILE: &str = "/home/user/.kosh_history";

/// Command history manager with persistent storage capability.
///
/// Stores command history in a VecDeque for efficient push/pop at both ends,
/// supports navigation (up/down), search (prefix and substring), and
/// persistence to the file system for session recovery.
pub struct CommandHistory {
    /// Stored history entries (oldest at front, newest at back)
    entries: VecDeque<HistoryEntry>,
    /// Maximum number of entries to retain
    max_size: usize,
    /// Current navigation index. `None` means the user is at the "new command"
    /// position (past the end of history). `Some(i)` indexes into `entries`.
    current_index: Option<usize>,
    /// Path used for persistence
    history_file_path: String,
    /// Tracks whether the history has been modified since last save
    modified: bool,
}

impl CommandHistory {
    /// Create a new, empty history with the default capacity.
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(DEFAULT_MAX_HISTORY),
            max_size: DEFAULT_MAX_HISTORY,
            current_index: None,
            history_file_path: String::from(DEFAULT_HISTORY_FILE),
            modified: false,
        }
    }

    /// Create a history with a custom maximum size.
    pub fn with_capacity(max_size: usize) -> Self {
        let cap = if max_size == 0 { 1 } else { max_size };
        Self {
            entries: VecDeque::with_capacity(cap),
            max_size: cap,
            current_index: None,
            history_file_path: String::from(DEFAULT_HISTORY_FILE),
            modified: false,
        }
    }

    /// Set a custom history file path for persistence.
    pub fn set_history_file(&mut self, path: String) {
        self.history_file_path = path;
    }

    /// Get the history file path.
    pub fn history_file_path(&self) -> &str {
        &self.history_file_path
    }

    // ── Mutation ──────────────────────────────────────────────────────

    /// Add a command to the history.
    ///
    /// Duplicate consecutive commands are suppressed. If the history exceeds
    /// `max_size`, the oldest entry is evicted. Adding a command resets the
    /// navigation index.
    pub fn add(&mut self, command: String, working_directory: String) {
        self.add_entry(HistoryEntry {
            command,
            timestamp: 0, // Caller can set a real timestamp if available
            exit_code: None,
            working_directory,
        });
    }

    /// Add a fully populated `HistoryEntry`.
    pub fn add_entry(&mut self, entry: HistoryEntry) {
        // Skip empty commands
        if entry.command.trim().is_empty() {
            return;
        }

        // Suppress duplicate consecutive commands
        if let Some(last) = self.entries.back() {
            if last.command == entry.command {
                // Still reset navigation
                self.current_index = None;
                return;
            }
        }

        // Evict oldest if at capacity
        if self.entries.len() >= self.max_size {
            self.entries.pop_front();
        }

        self.entries.push_back(entry);
        self.current_index = None;
        self.modified = true;
    }

    /// Update the exit code of the most recent entry.
    pub fn set_last_exit_code(&mut self, exit_code: i32) {
        if let Some(last) = self.entries.back_mut() {
            last.exit_code = Some(exit_code);
            self.modified = true;
        }
    }

    /// Clear all history entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_index = None;
        self.modified = true;
    }

    // ── Queries ──────────────────────────────────────────────────────

    /// Number of entries currently stored.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Maximum capacity.
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Whether the history has been modified since the last save/load.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Get an entry by index (0 = oldest).
    pub fn get(&self, index: usize) -> Option<&HistoryEntry> {
        self.entries.get(index)
    }

    /// Get the most recent entry.
    pub fn last(&self) -> Option<&HistoryEntry> {
        self.entries.back()
    }

    /// Return all entries as a slice-like iterator (oldest first).
    pub fn entries(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter()
    }

    // ── Navigation (up / down arrow) ─────────────────────────────────

    /// Navigate up (older). Returns the command string at the new position,
    /// or `None` if already at the oldest entry or history is empty.
    pub fn navigate_up(&mut self) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }

        let new_index = match self.current_index {
            None => {
                // First press of up – go to the newest entry
                self.entries.len() - 1
            }
            Some(0) => {
                // Already at the oldest entry
                return Some(&self.entries[0].command);
            }
            Some(i) => i - 1,
        };

        self.current_index = Some(new_index);
        Some(&self.entries[new_index].command)
    }

    /// Navigate down (newer). Returns the command string at the new position,
    /// or `None` when moving past the newest entry (back to the empty prompt).
    pub fn navigate_down(&mut self) -> Option<&str> {
        match self.current_index {
            None => None, // Already past the end
            Some(i) => {
                if i + 1 >= self.entries.len() {
                    // Move past the newest entry → back to empty prompt
                    self.current_index = None;
                    None
                } else {
                    let new_index = i + 1;
                    self.current_index = Some(new_index);
                    Some(&self.entries[new_index].command)
                }
            }
        }
    }

    /// Reset navigation index (e.g. when the user submits a command).
    pub fn reset_navigation(&mut self) {
        self.current_index = None;
    }

    /// Current navigation index, if any.
    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    // ── Search ───────────────────────────────────────────────────────

    /// Search for entries whose command starts with `prefix`.
    /// Returns matches from newest to oldest.
    pub fn search_prefix(&self, prefix: &str) -> Vec<&HistoryEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|e| e.command.starts_with(prefix))
            .collect()
    }

    /// Search for entries whose command contains `substring`.
    /// Returns matches from newest to oldest.
    pub fn search_substring(&self, substring: &str) -> Vec<&HistoryEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|e| e.command.contains(substring))
            .collect()
    }

    /// Reverse incremental search: find the most recent entry containing `pattern`,
    /// starting from the current navigation position (or the end if not navigating).
    /// Returns the index and command if found.
    pub fn reverse_search(&self, pattern: &str) -> Option<(usize, &str)> {
        if pattern.is_empty() {
            return None;
        }

        let start = self.current_index.unwrap_or(self.entries.len());
        for i in (0..start).rev() {
            if self.entries[i].command.contains(pattern) {
                return Some((i, &self.entries[i].command));
            }
        }
        None
    }

    /// Continue reverse search from a given index (exclusive).
    pub fn reverse_search_from(&self, pattern: &str, from_index: usize) -> Option<(usize, &str)> {
        if pattern.is_empty() || from_index == 0 {
            return None;
        }

        for i in (0..from_index).rev() {
            if self.entries[i].command.contains(pattern) {
                return Some((i, &self.entries[i].command));
            }
        }
        None
    }

    /// Set navigation to a specific index (used after search selection).
    pub fn set_navigation_index(&mut self, index: usize) {
        if index < self.entries.len() {
            self.current_index = Some(index);
        }
    }

    // ── Persistence ──────────────────────────────────────────────────

    /// Serialize history to a byte vector for saving.
    /// Format: one entry per line as "timestamp:exit_code:cwd:command"
    /// where exit_code is "-" if None.
    pub fn serialize(&self) -> Vec<u8> {
        let mut output = Vec::new();
        for entry in &self.entries {
            let exit_str = match entry.exit_code {
                Some(code) => alloc::format!("{}", code),
                None => String::from("-"),
            };
            let line = alloc::format!(
                "{}:{}:{}:{}\n",
                entry.timestamp,
                exit_str,
                entry.working_directory,
                entry.command
            );
            output.extend_from_slice(line.as_bytes());
        }
        output
    }

    /// Deserialize history from bytes (loaded from file).
    /// Returns the number of entries loaded.
    pub fn deserialize(&mut self, data: &[u8]) -> usize {
        let text = match core::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let mut count = 0;
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }

            // Parse "timestamp:exit_code:cwd:command"
            // We need to be careful: command may contain ':'
            let mut parts = line.splitn(4, ':');
            let timestamp_str = match parts.next() {
                Some(s) => s,
                None => continue,
            };
            let exit_str = match parts.next() {
                Some(s) => s,
                None => continue,
            };
            let cwd = match parts.next() {
                Some(s) => s,
                None => continue,
            };
            let command = match parts.next() {
                Some(s) => s,
                None => continue,
            };

            let timestamp = timestamp_str.parse::<u64>().unwrap_or(0);
            let exit_code = if exit_str == "-" {
                None
            } else {
                exit_str.parse::<i32>().ok()
            };

            let entry = HistoryEntry {
                command: command.to_string(),
                timestamp,
                exit_code,
                working_directory: cwd.to_string(),
            };

            // Directly push without duplicate check during load
            if self.entries.len() >= self.max_size {
                self.entries.pop_front();
            }
            self.entries.push_back(entry);
            count += 1;
        }

        self.current_index = None;
        self.modified = false;
        count
    }

    /// Mark history as saved (clears modified flag).
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    fn make_entry(cmd: &str) -> HistoryEntry {
        HistoryEntry {
            command: cmd.to_string(),
            timestamp: 0,
            exit_code: None,
            working_directory: "/".to_string(),
        }
    }

    // ── Construction ─────────────────────────────────────────────────

    #[test]
    fn test_new_history_is_empty() {
        let h = CommandHistory::new();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert_eq!(h.max_size(), DEFAULT_MAX_HISTORY);
        assert!(h.current_index().is_none());
    }

    #[test]
    fn test_with_capacity() {
        let h = CommandHistory::with_capacity(50);
        assert_eq!(h.max_size(), 50);
    }

    #[test]
    fn test_zero_capacity_clamped_to_one() {
        let h = CommandHistory::with_capacity(0);
        assert_eq!(h.max_size(), 1);
    }

    // ── Adding entries ───────────────────────────────────────────────

    #[test]
    fn test_add_single_entry() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        assert_eq!(h.len(), 1);
        assert_eq!(h.last().unwrap().command, "ls");
    }

    #[test]
    fn test_add_multiple_entries() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        h.add("pwd".to_string(), "/".to_string());
        h.add("cd /tmp".to_string(), "/".to_string());
        assert_eq!(h.len(), 3);
        assert_eq!(h.get(0).unwrap().command, "ls");
        assert_eq!(h.get(2).unwrap().command, "cd /tmp");
    }

    #[test]
    fn test_add_skips_empty_commands() {
        let mut h = CommandHistory::new();
        h.add("".to_string(), "/".to_string());
        h.add("   ".to_string(), "/".to_string());
        assert!(h.is_empty());
    }

    #[test]
    fn test_add_suppresses_consecutive_duplicates() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        h.add("ls".to_string(), "/".to_string());
        h.add("ls".to_string(), "/".to_string());
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn test_add_allows_non_consecutive_duplicates() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        h.add("pwd".to_string(), "/".to_string());
        h.add("ls".to_string(), "/".to_string());
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn test_add_evicts_oldest_when_full() {
        let mut h = CommandHistory::with_capacity(3);
        h.add("a".to_string(), "/".to_string());
        h.add("b".to_string(), "/".to_string());
        h.add("c".to_string(), "/".to_string());
        h.add("d".to_string(), "/".to_string());
        assert_eq!(h.len(), 3);
        assert_eq!(h.get(0).unwrap().command, "b");
        assert_eq!(h.get(2).unwrap().command, "d");
    }

    #[test]
    fn test_set_last_exit_code() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        h.set_last_exit_code(0);
        assert_eq!(h.last().unwrap().exit_code, Some(0));
    }

    #[test]
    fn test_clear() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        h.add("pwd".to_string(), "/".to_string());
        h.clear();
        assert!(h.is_empty());
        assert!(h.current_index().is_none());
    }

    // ── Navigation ───────────────────────────────────────────────────

    #[test]
    fn test_navigate_up_empty_history() {
        let mut h = CommandHistory::new();
        assert!(h.navigate_up().is_none());
    }

    #[test]
    fn test_navigate_up_single_entry() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        assert_eq!(h.navigate_up(), Some("ls"));
        // Already at oldest, stays there
        assert_eq!(h.navigate_up(), Some("ls"));
    }

    #[test]
    fn test_navigate_up_multiple_entries() {
        let mut h = CommandHistory::new();
        h.add("first".to_string(), "/".to_string());
        h.add("second".to_string(), "/".to_string());
        h.add("third".to_string(), "/".to_string());

        assert_eq!(h.navigate_up(), Some("third"));
        assert_eq!(h.navigate_up(), Some("second"));
        assert_eq!(h.navigate_up(), Some("first"));
        // At oldest, stays
        assert_eq!(h.navigate_up(), Some("first"));
    }

    #[test]
    fn test_navigate_down_without_up_returns_none() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        assert!(h.navigate_down().is_none());
    }

    #[test]
    fn test_navigate_up_then_down() {
        let mut h = CommandHistory::new();
        h.add("first".to_string(), "/".to_string());
        h.add("second".to_string(), "/".to_string());
        h.add("third".to_string(), "/".to_string());

        // Go up to "third"
        assert_eq!(h.navigate_up(), Some("third"));
        // Go up to "second"
        assert_eq!(h.navigate_up(), Some("second"));
        // Go down to "third"
        assert_eq!(h.navigate_down(), Some("third"));
        // Go down past end → None (back to prompt)
        assert!(h.navigate_down().is_none());
    }

    #[test]
    fn test_add_resets_navigation() {
        let mut h = CommandHistory::new();
        h.add("first".to_string(), "/".to_string());
        h.add("second".to_string(), "/".to_string());

        h.navigate_up(); // "second"
        h.navigate_up(); // "first"

        h.add("third".to_string(), "/".to_string());
        assert!(h.current_index().is_none());
        // Next up should go to "third"
        assert_eq!(h.navigate_up(), Some("third"));
    }

    // ── Search ───────────────────────────────────────────────────────

    #[test]
    fn test_search_prefix() {
        let mut h = CommandHistory::new();
        h.add("ls -la".to_string(), "/".to_string());
        h.add("pwd".to_string(), "/".to_string());
        h.add("ls /tmp".to_string(), "/".to_string());
        h.add("cat file".to_string(), "/".to_string());

        let results = h.search_prefix("ls");
        assert_eq!(results.len(), 2);
        // Newest first
        assert_eq!(results[0].command, "ls /tmp");
        assert_eq!(results[1].command, "ls -la");
    }

    #[test]
    fn test_search_prefix_no_match() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        let results = h.search_prefix("xyz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_substring() {
        let mut h = CommandHistory::new();
        h.add("echo hello".to_string(), "/".to_string());
        h.add("cat hello.txt".to_string(), "/".to_string());
        h.add("ls".to_string(), "/".to_string());

        let results = h.search_substring("hello");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].command, "cat hello.txt");
        assert_eq!(results[1].command, "echo hello");
    }

    #[test]
    fn test_reverse_search() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        h.add("echo test".to_string(), "/".to_string());
        h.add("ls -la".to_string(), "/".to_string());
        h.add("pwd".to_string(), "/".to_string());

        let result = h.reverse_search("ls");
        assert!(result.is_some());
        let (idx, cmd) = result.unwrap();
        assert_eq!(idx, 2);
        assert_eq!(cmd, "ls -la");
    }

    #[test]
    fn test_reverse_search_from() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        h.add("echo test".to_string(), "/".to_string());
        h.add("ls -la".to_string(), "/".to_string());
        h.add("pwd".to_string(), "/".to_string());

        // Search from index 2 (exclusive), should find "ls" at index 0
        let result = h.reverse_search_from("ls", 2);
        assert!(result.is_some());
        let (idx, cmd) = result.unwrap();
        assert_eq!(idx, 0);
        assert_eq!(cmd, "ls");
    }

    #[test]
    fn test_reverse_search_empty_pattern() {
        let mut h = CommandHistory::new();
        h.add("ls".to_string(), "/".to_string());
        assert!(h.reverse_search("").is_none());
    }

    // ── Persistence ──────────────────────────────────────────────────

    #[test]
    fn test_serialize_empty() {
        let h = CommandHistory::new();
        let data = h.serialize();
        assert!(data.is_empty());
    }

    #[test]
    fn test_serialize_and_deserialize_roundtrip() {
        let mut h = CommandHistory::new();
        h.add_entry(HistoryEntry {
            command: "ls -la".to_string(),
            timestamp: 1000,
            exit_code: Some(0),
            working_directory: "/home".to_string(),
        });
        h.add_entry(HistoryEntry {
            command: "pwd".to_string(),
            timestamp: 2000,
            exit_code: None,
            working_directory: "/tmp".to_string(),
        });

        let data = h.serialize();
        let mut h2 = CommandHistory::new();
        let count = h2.deserialize(&data);

        assert_eq!(count, 2);
        assert_eq!(h2.len(), 2);
        assert_eq!(h2.get(0).unwrap().command, "ls -la");
        assert_eq!(h2.get(0).unwrap().timestamp, 1000);
        assert_eq!(h2.get(0).unwrap().exit_code, Some(0));
        assert_eq!(h2.get(0).unwrap().working_directory, "/home");
        assert_eq!(h2.get(1).unwrap().command, "pwd");
        assert_eq!(h2.get(1).unwrap().timestamp, 2000);
        assert_eq!(h2.get(1).unwrap().exit_code, None);
        assert_eq!(h2.get(1).unwrap().working_directory, "/tmp");
    }

    #[test]
    fn test_deserialize_invalid_utf8() {
        let mut h = CommandHistory::new();
        let data = [0xFF, 0xFE, 0xFD];
        let count = h.deserialize(&data);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_deserialize_malformed_lines_skipped() {
        let mut h = CommandHistory::new();
        let data = b"bad line\n1000:0:/home:ls\nincomplete:field\n";
        let count = h.deserialize(data);
        // Only the well-formed line should be loaded
        assert_eq!(count, 1);
        assert_eq!(h.get(0).unwrap().command, "ls");
    }

    #[test]
    fn test_deserialize_command_with_colons() {
        let mut h = CommandHistory::new();
        // Command contains colons: "echo a:b:c"
        let data = b"100:0:/:echo a:b:c\n";
        let count = h.deserialize(data);
        assert_eq!(count, 1);
        assert_eq!(h.get(0).unwrap().command, "echo a:b:c");
    }

    #[test]
    fn test_modified_flag() {
        let mut h = CommandHistory::new();
        assert!(!h.is_modified());

        h.add("ls".to_string(), "/".to_string());
        assert!(h.is_modified());

        h.mark_saved();
        assert!(!h.is_modified());

        // Deserialize clears modified
        let data = h.serialize();
        let mut h2 = CommandHistory::new();
        h2.deserialize(&data);
        assert!(!h2.is_modified());
    }

    #[test]
    fn test_set_history_file_path() {
        let mut h = CommandHistory::new();
        assert_eq!(h.history_file_path(), DEFAULT_HISTORY_FILE);
        h.set_history_file("/custom/path".to_string());
        assert_eq!(h.history_file_path(), "/custom/path");
    }

    #[test]
    fn test_set_navigation_index() {
        let mut h = CommandHistory::new();
        h.add("a".to_string(), "/".to_string());
        h.add("b".to_string(), "/".to_string());
        h.add("c".to_string(), "/".to_string());

        h.set_navigation_index(1);
        assert_eq!(h.current_index(), Some(1));

        // Out of bounds is ignored
        h.set_navigation_index(100);
        assert_eq!(h.current_index(), Some(1));
    }

    #[test]
    fn test_add_entry_with_full_metadata() {
        let mut h = CommandHistory::new();
        h.add_entry(HistoryEntry {
            command: "make build".to_string(),
            timestamp: 12345,
            exit_code: Some(2),
            working_directory: "/project".to_string(),
        });
        let e = h.last().unwrap();
        assert_eq!(e.command, "make build");
        assert_eq!(e.timestamp, 12345);
        assert_eq!(e.exit_code, Some(2));
        assert_eq!(e.working_directory, "/project");
    }

    #[test]
    fn test_entries_iterator() {
        let mut h = CommandHistory::new();
        h.add("a".to_string(), "/".to_string());
        h.add("b".to_string(), "/".to_string());
        h.add("c".to_string(), "/".to_string());

        let cmds: Vec<&str> = h.entries().map(|e| e.command.as_str()).collect();
        assert_eq!(cmds, vec!["a", "b", "c"]);
    }
}
