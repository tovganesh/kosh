//! The Kosh console: an in-kernel shell.
//!
//! ## Why in-kernel, in a microkernel
//!
//! Because a kernel you can interrogate while it is running is worth more than
//! architectural purity at this stage. Every command here reports live kernel
//! state — real frame counts, real heap statistics, the real thread table, the
//! real page tables. That makes it a debugging instrument, not a demo, and it
//! is the reason serious kernels keep a built-in debug console permanently
//! even after userspace exists.
//!
//! The userspace shell is still the goal. It needs a blocking read syscall and
//! a filesystem to be worth using; this console needs neither, and it is what
//! lets you inspect the kernel *while* building those.
//!
//! ## What it deliberately does not do
//!
//! No `ls`, `cat`, or `cd`. There is no filesystem yet, and inventing commands
//! that return plausible-looking strings is exactly the habit that let this
//! project accumulate 27 commits of code that had never run. Every command
//! below either reports something real or says it is not implemented.

pub mod commands;
pub mod editor;

use alloc::string::String;

use editor::LineEditor;

use crate::{print, println, serial_print, serial_println};

/// Run the console. Never returns.
///
/// Runs as a kernel thread, so the timer keeps ticking and anything else that
/// is runnable keeps running while this blocks on the keyboard.
pub fn run(_arg: usize) {
    let mut editor = LineEditor::new();
    let mut cwd = String::from("/");

    // The supervisor already silenced the boot heartbeat; do it again in case
    // the console was reached by some other path.
    crate::interrupts::timer::set_heartbeat(false);

    banner();

    loop {
        // The prompt carries the working directory, so `pwd` is rarely needed.
        let prompt = build_prompt(&cwd);
        let line = editor.read_line(&prompt);

        // `read_line` borrows the editor for the lifetime of the returned
        // string, but `history` needs to read the editor too. Copy the line out
        // so the borrow ends here.
        let mut owned = [0u8; editor::MAX_LINE];
        let n = line.len().min(owned.len());
        owned[..n].copy_from_slice(&line.as_bytes()[..n]);
        let line = core::str::from_utf8(&owned[..n]).unwrap_or("");

        commands::execute(line, &mut cwd, &editor);
    }
}

/// `kosh:/docs> ` — short enough to keep the line usable, informative enough to
/// know where `ls` will look.
fn build_prompt(cwd: &str) -> String {
    let mut p = String::from("kosh:");
    p.push_str(cwd);
    p.push_str("> ");
    p
}

fn banner() {
    println!();
    println!("Kosh console. Type 'help' for commands.");
    serial_println!();
    serial_println!("Kosh console. Type 'help' for commands.");
}
