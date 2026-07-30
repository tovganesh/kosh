//! The Kosh shell.
//!
//! What remains here is the code that was always real: `parser` (a tokenizer
//! with quote handling, escapes, variable expansion, pipes, redirects and
//! conditionals) and `history` (a ring with search and navigation). Both were
//! written before anything could run them; `main.rs` now does.
//!
//! What was removed, and why:
//!
//! * `service_client.rs` and `infrastructure.rs` — 1,399 and ~450 lines with
//!   zero `asm!` between them. `wait_for_response` returned a fabricated
//!   success, service discovery was three hardcoded PIDs, and the two files were
//!   near-duplicates of each other. `syscall.rs` replaces both with actual
//!   system calls.
//! * `fs_commands.rs` — every path fell through to `simulated_listing()` and
//!   `simulated_file_content()`.
//! * `commands.rs` — `ps`, `ls`, `cat`, `pwd` and the rest returned canned
//!   strings. The real versions live in `main.rs` and go through the kernel.
//! * `input.rs` — `read_line` replayed a hardcoded array of six commands and
//!   `read_char` always returned `None`.
//! * `output.rs` — wrote through a `syscall` wrapped in
//!   `#[cfg(debug_assertions)]`, so release builds printed nothing.
//! * `tests.rs` — 558 host-side `#[test]`s against the modules above.
//!
//! They are in git history. What is here runs.

#![no_std]

extern crate alloc;

pub mod error;
pub mod history;
pub mod parser;
pub mod syscall;
pub mod types;

pub use error::{ShellError, ShellResult};
pub use history::CommandHistory;
pub use parser::AdvancedParser;
pub use types::*;
