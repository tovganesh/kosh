//! A line editor: the part of a shell that is not about commands.
//!
//! Cursor movement, insert and delete anywhere in the line, and a history ring
//! you walk with the arrow keys. Fixed-size buffers throughout — this runs
//! before the point where the kernel wants to be allocating on every keystroke,
//! and a console that cannot fail is worth more than one with unbounded lines.

use crate::interrupts::keyboard::{read_key_blocking, Key};
use crate::{print, serial_print};

/// Longest line the editor accepts.
pub const MAX_LINE: usize = 256;

/// Lines remembered.
const HISTORY_DEPTH: usize = 32;

pub struct LineEditor {
    buffer: [u8; MAX_LINE],
    len: usize,
    cursor: usize,

    history: [[u8; MAX_LINE]; HISTORY_DEPTH],
    history_len: [usize; HISTORY_DEPTH],
    /// Number of lines stored, saturating at HISTORY_DEPTH.
    history_count: usize,
    /// Where the next line goes; the ring wraps here.
    history_head: usize,
    /// How far back we have walked. 0 means "the line being typed".
    history_pos: usize,
}

impl LineEditor {
    pub const fn new() -> Self {
        Self {
            buffer: [0; MAX_LINE],
            len: 0,
            cursor: 0,
            history: [[0; MAX_LINE]; HISTORY_DEPTH],
            history_len: [0; HISTORY_DEPTH],
            history_count: 0,
            history_head: 0,
            history_pos: 0,
        }
    }

    /// Read one line, echoing as the user types. Blocks.
    pub fn read_line(&mut self, prompt: &str) -> &str {
        self.len = 0;
        self.cursor = 0;
        self.history_pos = 0;

        print!("{}", prompt);
        serial_print!("{}", prompt);

        loop {
            match read_key_blocking() {
                Key::Enter => {
                    print!("\n");
                    serial_print!("\n");
                    self.push_history();
                    break;
                }

                Key::Char(c) if (c as u8) >= 0x20 && (c as u8) < 0x7f => {
                    self.insert(c as u8, prompt);
                }

                Key::Backspace => {
                    if self.cursor > 0 {
                        self.cursor -= 1;
                        self.remove_at_cursor(prompt);
                    }
                }

                Key::Delete => {
                    if self.cursor < self.len {
                        self.remove_at_cursor(prompt);
                    }
                }

                Key::Left => {
                    if self.cursor > 0 {
                        self.cursor -= 1;
                        self.redraw(prompt);
                    }
                }

                Key::Right => {
                    if self.cursor < self.len {
                        self.cursor += 1;
                        self.redraw(prompt);
                    }
                }

                Key::Home => {
                    self.cursor = 0;
                    self.redraw(prompt);
                }

                Key::End => {
                    self.cursor = self.len;
                    self.redraw(prompt);
                }

                Key::Up => self.history_step(1, prompt),
                Key::Down => self.history_step(-1, prompt),

                // Tab completion belongs with a filesystem to complete against.
                Key::Tab | Key::Escape | Key::Char(_) => {}
            }
        }

        core::str::from_utf8(&self.buffer[..self.len]).unwrap_or("")
    }

    fn insert(&mut self, byte: u8, prompt: &str) {
        if self.len >= MAX_LINE {
            return;
        }

        // Shift the tail right to make room.
        let mut i = self.len;
        while i > self.cursor {
            self.buffer[i] = self.buffer[i - 1];
            i -= 1;
        }

        self.buffer[self.cursor] = byte;
        self.len += 1;
        self.cursor += 1;

        // Appending at the end is the common case and only needs one character
        // echoed; anything else has to redraw the tail.
        if self.cursor == self.len {
            print!("{}", byte as char);
            serial_print!("{}", byte as char);
        } else {
            self.redraw(prompt);
        }
    }

    fn remove_at_cursor(&mut self, prompt: &str) {
        let mut i = self.cursor;
        while i + 1 < self.len {
            self.buffer[i] = self.buffer[i + 1];
            i += 1;
        }
        self.len -= 1;
        self.redraw(prompt);
    }

    /// Repaint the whole line.
    ///
    /// Crude but honest: carriage return, rewrite, then walk the cursor back.
    /// A terminal-aware editor would emit cursor-movement escapes, but this has
    /// to drive a raw VGA text buffer as well as a serial line, and the two do
    /// not agree on escape sequences.
    fn redraw(&self, prompt: &str) {
        print!("\r{}", prompt);
        serial_print!("\r{}", prompt);

        for i in 0..self.len {
            print!("{}", self.buffer[i] as char);
            serial_print!("{}", self.buffer[i] as char);
        }

        // Erase whatever the previous, longer line left behind.
        print!(" ");
        serial_print!(" ");

        // Walk back to the cursor.
        let back = self.len + 1 - self.cursor;
        for _ in 0..back {
            print!("\x08");
            serial_print!("\x08");
        }
    }

    fn push_history(&mut self) {
        if self.len == 0 {
            return;
        }

        // Skip consecutive duplicates: holding Up through ten identical lines
        // is nobody's idea of history.
        if self.history_count > 0 {
            let last = (self.history_head + HISTORY_DEPTH - 1) % HISTORY_DEPTH;
            if self.history_len[last] == self.len
                && self.history[last][..self.len] == self.buffer[..self.len]
            {
                return;
            }
        }

        self.history[self.history_head][..self.len].copy_from_slice(&self.buffer[..self.len]);
        self.history_len[self.history_head] = self.len;
        self.history_head = (self.history_head + 1) % HISTORY_DEPTH;
        if self.history_count < HISTORY_DEPTH {
            self.history_count += 1;
        }
    }

    /// Walk history. `direction` is +1 for older, -1 for newer.
    fn history_step(&mut self, direction: i32, prompt: &str) {
        if self.history_count == 0 {
            return;
        }

        let next = self.history_pos as i32 + direction;
        if next < 0 || next > self.history_count as i32 {
            return;
        }
        self.history_pos = next as usize;

        if self.history_pos == 0 {
            // Back to a fresh line.
            self.len = 0;
            self.cursor = 0;
        } else {
            let idx =
                (self.history_head + HISTORY_DEPTH - self.history_pos) % HISTORY_DEPTH;
            let n = self.history_len[idx];
            self.buffer[..n].copy_from_slice(&self.history[idx][..n]);
            self.len = n;
            self.cursor = n;
        }

        self.redraw(prompt);
    }

    /// Iterate stored history, oldest first.
    pub fn history_entries(&self) -> impl Iterator<Item = &str> {
        let count = self.history_count;
        let head = self.history_head;

        (0..count).filter_map(move |i| {
            let idx = (head + HISTORY_DEPTH - count + i) % HISTORY_DEPTH;
            core::str::from_utf8(&self.history[idx][..self.history_len[idx]]).ok()
        })
    }
}
