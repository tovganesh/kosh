//! ksh — the Kosh shell, running in ring 3.
//!
//! This is the shell the microkernel design always implied: a userspace process
//! that reaches the kernel only through system calls. It replaces the previous
//! `main.rs`, whose `InputHandler::read_line` returned entries from a hardcoded
//! array — `["help", "echo Hello, Kosh!", "ps", "ls", "pwd", "exit"]` — and
//! exited after six iterations, and whose output went through a `syscall`
//! instruction wrapped in `#[cfg(debug_assertions)]` into a kernel that had no
//! syscall entry point.
//!
//! It uses the crate's existing `parser` and `history` modules, which were
//! written long before anything could run them: 952 lines of tokenizer with
//! quote handling and variable expansion, and 769 lines of history ring. The old
//! binary did not even declare them as modules.
//!
//! What is deliberately not here: pipes, redirection and background jobs. The
//! parser understands all three, and this reports them as unsupported, because
//! executing them needs `fork` and `exec`. Accepting the syntax and quietly
//! ignoring it is how you end up with a shell that lies.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::panic::PanicInfo;

use linked_list_allocator::LockedHeap;

use kosh_shell::history::CommandHistory;
use kosh_shell::parser::AdvancedParser;
use kosh_shell::syscall as sys;
use kosh_shell::types::ParsedCommand;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// 256 KiB of heap, in .bss. The kernel's ELF loader zeroes it, so nothing is
/// needed beyond handing the range to the allocator.
const HEAP_SIZE: usize = 256 * 1024;
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

const MAX_LINE: usize = 512;

// --- output ----------------------------------------------------------------

fn print(s: &str) {
    sys::write(sys::STDOUT, s.as_bytes());
}

fn println(s: &str) {
    print(s);
    print("\n");
}

fn print_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if value == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    sys::write(sys::STDOUT, &buf[i..]);
}

/// Right-align a number in `width` columns.
fn print_i64(value: i64) {
    if value < 0 {
        print("-");
        print_u64(value.unsigned_abs());
    } else {
        print_u64(value as u64);
    }
}

fn print_u64_padded(value: u64, width: usize) {
    let mut digits = 1;
    let mut v = value;
    while v >= 10 {
        v /= 10;
        digits += 1;
    }
    for _ in digits..width {
        print(" ");
    }
    print_u64(value);
}

// --- line editing ----------------------------------------------------------

/// Read a line from fd 0, echoing as it goes.
///
/// The kernel does not echo — `read` hands over exactly the bytes that arrived,
/// which is what lets a shell implement its own editing. Arrow keys arrive as
/// ANSI escape sequences, the same as they would from a terminal.
struct LineReader {
    buffer: [u8; MAX_LINE],
    len: usize,
}

impl LineReader {
    const fn new() -> Self {
        Self {
            buffer: [0; MAX_LINE],
            len: 0,
        }
    }

    fn read_line(&mut self, history: &CommandHistory) -> &str {
        self.len = 0;
        let mut history_pos = 0usize;

        let mut chunk = [0u8; 64];
        loop {
            let n = sys::read(sys::STDIN, &mut chunk);
            if n <= 0 {
                continue;
            }

            let mut i = 0usize;
            while i < n as usize {
                let byte = chunk[i];
                i += 1;

                match byte {
                    b'\n' | b'\r' => {
                        print("\n");
                        return core::str::from_utf8(&self.buffer[..self.len]).unwrap_or("");
                    }

                    0x08 | 0x7f => {
                        if self.len > 0 {
                            self.len -= 1;
                            // Backspace, space, backspace: step over the
                            // character, erase it, step back again.
                            print("\x08 \x08");
                        }
                    }

                    0x1b => {
                        // ANSI escape: ESC [ <letter>. Anything else is ignored.
                        if i + 1 < n as usize && chunk[i] == b'[' {
                            let code = chunk[i + 1];
                            i += 2;
                            match code {
                                b'A' => self.recall(history, &mut history_pos, 1),
                                b'B' => self.recall(history, &mut history_pos, -1),
                                _ => {}
                            }
                        }
                    }

                    0x20..=0x7e => {
                        if self.len < MAX_LINE {
                            self.buffer[self.len] = byte;
                            self.len += 1;
                            sys::write(sys::STDOUT, &[byte]);
                        }
                    }

                    _ => {}
                }
            }
        }
    }

    /// Replace the line with a history entry and repaint.
    fn recall(&mut self, history: &CommandHistory, pos: &mut usize, direction: i32) {
        let count = history.len();
        if count == 0 {
            return;
        }

        let next = *pos as i32 + direction;
        if next < 0 || next > count as i32 {
            return;
        }
        *pos = next as usize;

        // Erase what is on screen now.
        for _ in 0..self.len {
            print("\x08 \x08");
        }

        self.len = 0;
        if *pos > 0 {
            if let Some(entry) = history.get(count - *pos) {
                let bytes = entry.command.as_bytes();
                let n = core::cmp::min(bytes.len(), MAX_LINE);
                self.buffer[..n].copy_from_slice(&bytes[..n]);
                self.len = n;
                sys::write(sys::STDOUT, &self.buffer[..self.len]);
            }
        }
    }
}

// --- paths -----------------------------------------------------------------

/// Resolve a possibly-relative path against `cwd`, collapsing `.` and `..`.
///
/// The kernel's filesystem has no notion of a working directory, so this is the
/// shell's job — exactly as it is on a real system.
fn resolve(cwd: &str, arg: &str) -> String {
    let combined = if arg.starts_with('/') {
        String::from(arg)
    } else {
        let mut s = String::from(cwd);
        if !s.ends_with('/') {
            s.push('/');
        }
        s.push_str(arg);
        s
    };

    let mut parts: Vec<&str> = Vec::new();
    for component in combined.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }

    if parts.is_empty() {
        return String::from("/");
    }

    let mut out = String::new();
    for part in parts {
        out.push('/');
        out.push_str(part);
    }
    out
}

// --- builtins --------------------------------------------------------------

fn cmd_help() {
    println("ksh - the Kosh shell, running in ring 3");
    println("");
    println("  help                  this list");
    println("  echo <text>           print text");
    println("  ls [path]             list a directory");
    println("  cat <path>            print a file");
    println("  cd <path>             change directory");
    println("  pwd                   print the working directory");
    println("  stat <path>           file details");
    println("  history               previous commands");
    println("  parsetest             run the tokenizer over sample inputs");
    println("  getpid                this process's id");
    println("  exit                  leave the shell");
    println("");
    println("Anything else is treated as a program name: the kernel loads it,");
    println("runs it on its own task, and this shell blocks until it exits.");
    println("Try 'hello'.");
    println("");
    println("Pipes, redirection and background jobs parse but do not run:");
    println("they need per-process address spaces, which the kernel does not");
    println("have yet — spawn puts a second program in the same one.");
}

fn cmd_ls(cwd: &str, args: &[String]) {
    let path = if args.is_empty() {
        String::from(cwd)
    } else {
        resolve(cwd, &args[0])
    };

    let mut entries = [sys::RawDirEntry::zeroed(); 64];
    let n = sys::getdents(&path, &mut entries);

    if n < 0 {
        print("ls: ");
        print(&path);
        println(": cannot read directory");
        return;
    }

    let mut shown = 0u64;
    let mut bytes = 0u64;

    for entry in entries.iter().take(n as usize) {
        let name = entry.name_str();
        if name == "." || name == ".." {
            continue;
        }

        print(if entry.is_dir != 0 { "d  " } else { "-  " });
        if entry.is_dir != 0 {
            print("         -");
        } else {
            print_u64_padded(entry.size as u64, 10);
        }
        print("  ");
        println(name);

        shown += 1;
        bytes += entry.size as u64;
    }

    print("\n");
    print_u64(shown);
    print(if shown == 1 { " entry, " } else { " entries, " });
    print_u64(bytes);
    println(" bytes");
}

fn cmd_cat(cwd: &str, args: &[String]) {
    if args.is_empty() {
        println("usage: cat <path>");
        return;
    }

    for arg in args {
        let path = resolve(cwd, arg);
        let fd = sys::open(&path);
        if fd < 0 {
            print("cat: ");
            print(&path);
            println(": no such file");
            continue;
        }

        // Read in chunks through the descriptor. This is what makes it a real
        // read rather than a whole-file slurp: the kernel tracks the offset.
        let mut buf = [0u8; 256];
        loop {
            let n = sys::read(fd as u64, &mut buf);
            if n <= 0 {
                break;
            }
            sys::write(sys::STDOUT, &buf[..n as usize]);
        }

        sys::close(fd as u64);
    }
}

fn cmd_cd(cwd: &mut String, args: &[String]) {
    let target = if args.is_empty() { "/" } else { args[0].as_str() };
    let path = resolve(cwd, target);

    let mut info = sys::RawDirEntry::zeroed();
    if sys::stat(&path, &mut info) < 0 {
        print("cd: ");
        print(&path);
        println(": no such directory");
        return;
    }
    if info.is_dir == 0 {
        print("cd: ");
        print(&path);
        println(": not a directory");
        return;
    }

    *cwd = path;
}

fn cmd_stat(cwd: &str, args: &[String]) {
    if args.is_empty() {
        println("usage: stat <path>");
        return;
    }

    let path = resolve(cwd, &args[0]);
    let mut info = sys::RawDirEntry::zeroed();
    if sys::stat(&path, &mut info) < 0 {
        print("stat: ");
        print(&path);
        println(": no such file or directory");
        return;
    }

    println(&path);
    print("  name  ");
    println(info.name_str());
    print("  type  ");
    println(if info.is_dir != 0 { "directory" } else { "file" });
    print("  size  ");
    print_u64(info.size as u64);
    println(" bytes");
}

fn cmd_history(history: &CommandHistory) {
    for i in 0..history.len() {
        if let Some(entry) = history.get(i) {
            print_u64_padded((i + 1) as u64, 4);
            print("  ");
            println(&entry.command);
        }
    }
}

/// Run the crate's parser over a fixed set of inputs and print what it found.
///
/// This exists because the parser handles more grammar than the shell can
/// execute — pipes, redirects, quoting, variable expansion — and most of it is
/// otherwise unreachable. It is also the only practical way to test the pipe
/// path: QEMU's `sendkey` cannot produce a `|` through this keyboard layout, so
/// a scripted console session can never type one.
fn cmd_parsetest(parser: &AdvancedParser) {
    const CASES: [&str; 6] = [
        "echo hello",
        "echo \"two words\"",
        "cat a.txt | grep b",
        "echo x > out.txt",
        "ls &",
        "echo one && echo two",
    ];

    println("parser check (the tokenizer, over inputs the shell cannot all run):");

    for case in CASES.iter() {
        print("  ");
        print(case);
        print("\n    -> ");

        match parser.parse(case) {
            Ok(p) => {
                print("cmd '");
                print(&p.command);
                print("' args ");
                print_u64(p.args.len() as u64);

                if p.pipe_to.is_some() {
                    print(", pipe yes");
                }
                if p.output_redirect.is_some() {
                    print(", redirect out");
                }
                if p.input_redirect.is_some() {
                    print(", redirect in");
                }
                if p.background {
                    print(", background");
                }
                if p.conditional.is_some() {
                    print(", conditional");
                }
                print("\n");
            }
            Err(_) => println("parse error"),
        }
    }
}

/// Report the parts of the grammar that parse but cannot run.
///
/// The parser handles pipes, redirection, conditionals and background jobs.
/// Executing any of them needs `fork`/`exec`. Reporting that is the honest
/// option; accepting the syntax and ignoring it is not.
fn report_unsupported(parsed: &ParsedCommand) -> bool {
    if parsed.pipe_to.is_some() {
        println("ksh: pipes need fork and exec, which the kernel does not have yet");
        return true;
    }
    if parsed.output_redirect.is_some() || parsed.input_redirect.is_some() {
        println("ksh: redirection needs a writable filesystem, which does not exist yet");
        return true;
    }
    if parsed.background {
        println("ksh: background jobs need fork, which the kernel does not have yet");
        return true;
    }
    false
}

/// Run an external program: spawn it, wait for it, report a non-zero exit.
///
/// The shell blocks in the kernel's `wait` — `State::Blocked`, not a yield loop —
/// so while the child runs, the shell costs nothing.
///
/// Only `ENOENT` becomes "command not found". Any other failure is a real error
/// and gets said out loud, because "command not found" for what is actually
/// "out of memory" or "that program's address range is already occupied" sends
/// you looking in the wrong place.
fn cmd_run(name: &str) {
    let task = sys::spawn(name);

    if task == sys::ENOENT {
        print("ksh: ");
        print(name);
        println(": command not found");
        return;
    }

    if task < 0 {
        print("ksh: ");
        print(name);
        print(": could not start (error ");
        print_i64(task);
        println(")");
        return;
    }

    let mut status: i32 = 0;
    let waited = sys::wait(task, &mut status);
    if waited < 0 {
        print("ksh: ");
        print(name);
        println(": wait failed");
        return;
    }

    if status != 0 {
        print("ksh: ");
        print(name);
        print(": exited ");
        print_i64(status as i64);
        println("");
    }
}

// --- entry -----------------------------------------------------------------

// System V says RSP is 16-byte aligned at process entry, but a Rust
// `extern "C"` function is compiled as an ordinary callee and assumes RSP is 8
// *past* a boundary — the state a `call` leaves. Wiring Rust straight to the
// entry point makes the first SSE spill fault with #GP.
core::arch::global_asm!(
    r#"
.section .text._start, "ax"
.global _start
.type _start, @function
_start:
    xorq    %rbp, %rbp
    andq    $-16, %rsp
    call    ksh_main
1:
    jmp     1b
"#,
    options(att_syntax)
);

#[no_mangle]
pub extern "C" fn ksh_main() -> ! {
    unsafe {
        ALLOCATOR.lock().init(&raw mut HEAP as *mut u8, HEAP_SIZE);
    }

    let parser = AdvancedParser::new();
    let mut history = CommandHistory::new();
    let mut reader = LineReader::new();
    let mut cwd = String::from("/");

    println("");
    println("ksh: the Kosh shell, in ring 3. Type 'help'.");

    loop {
        print("ksh:");
        print(&cwd);
        print("$ ");

        let line = reader.read_line(&history);

        // Copy out so the reader's borrow ends before anything else runs.
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        history.add(line.clone(), cwd.clone());

        // The crate's tokenizer: quotes, escapes, variable expansion, pipes,
        // redirects and conditionals. This is its first use in a running program.
        let parsed = match parser.parse(&line) {
            Ok(p) => p,
            Err(_) => {
                println("ksh: parse error");
                continue;
            }
        };

        if report_unsupported(&parsed) {
            continue;
        }

        match parsed.command.as_str() {
            "help" | "?" => cmd_help(),
            "echo" => println(&parsed.args.join(" ")),
            "ls" | "dir" => cmd_ls(&cwd, &parsed.args),
            "cat" => cmd_cat(&cwd, &parsed.args),
            "cd" => cmd_cd(&mut cwd, &parsed.args),
            "pwd" => println(&cwd),
            "stat" => cmd_stat(&cwd, &parsed.args),
            "history" => cmd_history(&history),
            "parsetest" => cmd_parsetest(&parser),
            "getpid" => {
                print("pid ");
                print_u64(sys::getpid() as u64);
                print("\n");
            }
            "exit" | "quit" => {
                println("ksh: exiting");
                sys::exit(0);
            }
            // Not a builtin: ask the kernel to run a program by that name. This
            // is the one thing a shell exists for, and until `spawn` landed it
            // was the one thing this shell could not do.
            other => cmd_run(other),
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println("ksh: panic");
    sys::exit(1)
}
