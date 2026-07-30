//! Console commands.
//!
//! Every one of these reports live kernel state or performs a real action.
//! None of them return canned strings — if something is not implemented, it
//! says so rather than printing a plausible-looking answer.

use alloc::string::String;
use alloc::vec::Vec;

use crate::console::editor::LineEditor;
use crate::{print, println, serial_print, serial_println};

/// Print to both the VGA console and the serial line.
macro_rules! out {
    () => {{
        println!();
        serial_println!();
    }};
    ($($arg:tt)*) => {{
        println!($($arg)*);
        serial_println!($($arg)*);
    }};
}

pub fn execute(line: &str, cwd: &mut String, editor: &LineEditor) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }

    let mut parts = line.split_whitespace();
    let command = parts.next().unwrap_or("");
    let args: [&str; 8] = {
        let mut a = [""; 8];
        for (i, s) in parts.take(8).enumerate() {
            a[i] = s;
        }
        a
    };
    let arg_count = line.split_whitespace().count().saturating_sub(1).min(8);
    let rest = line[command.len()..].trim_start();

    match command {
        "help" | "?" => help(),
        "echo" => out!("{}", rest),
        "clear" => clear(),

        "uptime" => uptime(),
        "ticks" => out!("{}", crate::interrupts::timer::ticks()),
        "mem" | "free" => memory(),
        "ps" | "threads" => threads(),
        "uname" => uname(),

        "heartbeat" => heartbeat(&args, arg_count),
        "syscalls" => syscalls(),
        "trace" => trace(&args, arg_count),

        "translate" | "virt" => translate(&args, arg_count),
        "modules" => modules(),
        "history" => history(editor),

        // Filesystem. These exist now because there is a disk underneath them.
        "ls" | "dir" => ls(cwd, &args, arg_count),
        "cd" => cd(cwd, &args, arg_count),
        "pwd" => out!("{}", cwd),
        "cat" => cat(cwd, &args, arg_count),
        "stat" => stat(cwd, &args, arg_count),
        "lsblk" => lsblk(),
        "df" | "mount" => df(),

        "fault" => fault(&args, arg_count),

        "reboot" => reboot(),

        _ => {
            out!("unknown command: {}", command);
            out!("type 'help' for the list");
        }
    }
}

fn help() {
    out!("Kosh console commands:");
    out!();
    out!("  help                  this list");
    out!("  echo <text>           print text");
    out!("  clear                 clear the screen");
    out!();
    out!("  uptime                time since the timer started");
    out!("  ticks                 raw timer tick count");
    out!("  mem, free             physical and heap memory statistics");
    out!("  ps, threads           kernel thread table");
    out!("  uname                 system and platform information");
    out!("  modules               boot modules supplied by the bootloader");
    out!();
    out!("  syscalls              syscalls serviced, and trace state");
    out!("  trace on|off          per-syscall tracing to the serial line");
    out!("  heartbeat on|off      once-a-second timer proof-of-life");
    out!("  translate <hex>       walk the page tables for an address");
    out!("  history               previous commands");
    out!();
    out!("  ls [-a] [path]        list a directory");
    out!("  cd <path>             change directory");
    out!("  pwd                   print the working directory");
    out!("  cat <path>            print a file");
    out!("  stat <path>           file or directory details");
    out!("  lsblk                 block devices");
    out!("  df, mount             mounted filesystem");
    out!();
    out!("  fault <kind>          deliberately fault: null, page, breakpoint");
    out!("  reboot                triple-fault the machine");
}

fn clear() {
    crate::vga_buffer::clear_screen();
    // ANSI erase-screen plus cursor-home, for whatever is on the other end of
    // the serial line.
    serial_print!("\x1b[2J\x1b[H");
}

fn threads() {
    let mut buf = [None; 16];
    let count = crate::task::snapshot(&mut buf);

    out!("{} thread(s):", count);
    out!("  {:>3}  {:<12} {:<9} {:>8}", "id", "name", "state", "ticks");

    for slot in buf.iter().flatten() {
        out!(
            "  {:>3}  {:<12} {:<9} {:>8}",
            slot.id,
            slot.name,
            match slot.state {
                crate::task::State::Running => "running",
                crate::task::State::Ready => "ready",
                crate::task::State::Finished => "finished",
            },
            slot.ticks
        );
    }

    out!();
    out!("{} context switches since boot", crate::task::switch_count());
}

fn uptime() {
    let ms = crate::interrupts::timer::uptime_ms();
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;

    out!(
        "up {}h {}m {}s ({} ms, {} ticks at {} Hz)",
        hours,
        minutes % 60,
        seconds % 60,
        ms,
        crate::interrupts::timer::ticks(),
        crate::interrupts::timer::TIMER_HZ
    );
}

fn memory() {
    if let Some(stats) = crate::memory::physical::memory_stats() {
        out!("physical memory:");
        out!(
            "  total     {:>8} pages  {:>6} MB",
            stats.total_pages,
            stats.total_memory_mb()
        );
        out!(
            "  used      {:>8} pages  {:>6} MB",
            stats.used_pages,
            stats.used_memory_mb()
        );
        out!(
            "  free      {:>8} pages  {:>6} MB",
            stats.free_pages,
            stats.free_memory_mb()
        );
        out!("  reserved  {:>8} pages", stats.reserved_pages);
    } else {
        out!("physical memory manager not initialised");
    }

    let heap = crate::memory::heap::heap_stats();
    let (blocks, free_blocks) = crate::memory::heap::block_census();

    out!();
    out!("kernel heap:");
    out!("  size      {:>8} KB", heap.heap_size / 1024);
    out!("  in use    {:>8} bytes across {} allocations", heap.current_bytes, heap.current_allocations);
    out!("  free      {:>8} bytes", heap.free_bytes);
    out!("  peak      {:>8} bytes", heap.peak_bytes);
    out!("  blocks    {:>8} ({} free)", blocks, free_blocks);
}

fn uname() {
    out!("Kosh 0.1.0 x86_64");
    out!("  microkernel, Rust, multiboot2");

    let cs: u16;
    unsafe {
        core::arch::asm!("mov {0:x}, cs", out(reg) cs, options(nomem, nostack, preserves_flags));
    }
    out!("  running in ring {}", cs & 3);

    let (cr3, _) = x86_64::registers::control::Cr3::read();
    out!("  CR3            0x{:x}", cr3.start_address().as_u64());
    out!("  physmap base   0x{:x}", crate::memory::paging::PHYSMAP_BASE);
    out!("  heap base      0x{:x}", crate::memory::paging::KERNEL_HEAP_BASE);
    out!("  timer          {} Hz", crate::interrupts::timer::TIMER_HZ);
}

fn syscalls() {
    out!(
        "{} syscalls serviced since boot",
        crate::syscall::entry::syscall_count()
    );
    out!(
        "tracing is {}",
        if crate::syscall::dispatcher::syscall_trace_enabled() {
            "on"
        } else {
            "off"
        }
    );
}

fn heartbeat(args: &[&str; 8], count: usize) {
    if count == 0 {
        out!(
            "heartbeat is {}",
            if crate::interrupts::timer::heartbeat_enabled() { "on" } else { "off" }
        );
        out!("usage: heartbeat on|off");
        return;
    }

    match args[0] {
        "on" => {
            crate::interrupts::timer::set_heartbeat(true);
            out!("heartbeat on — it will interrupt what you are typing");
        }
        "off" => {
            crate::interrupts::timer::set_heartbeat(false);
            out!("heartbeat off");
        }
        other => out!("expected 'on' or 'off', got '{}'", other),
    }
}

fn trace(args: &[&str; 8], count: usize) {
    if count == 0 {
        out!("usage: trace on|off");
        return;
    }

    match args[0] {
        "on" => {
            crate::syscall::dispatcher::set_syscall_trace(true);
            out!("syscall tracing on (serial only — it is verbose)");
        }
        "off" => {
            crate::syscall::dispatcher::set_syscall_trace(false);
            out!("syscall tracing off");
        }
        other => out!("expected 'on' or 'off', got '{}'", other),
    }
}

fn translate(args: &[&str; 8], count: usize) {
    if count == 0 {
        out!("usage: translate <hex address>");
        out!("  e.g. translate 100000     (the kernel image)");
        out!("       translate ffff800000000000  (the physmap)");
        return;
    }

    let text = args[0].trim_start_matches("0x");
    let Ok(addr) = u64::from_str_radix(text, 16) else {
        out!("not a hex address: {}", args[0]);
        return;
    };

    match crate::memory::paging::translate(addr) {
        Some(phys) => out!("0x{:x} -> physical 0x{:x}", addr, phys),
        None => out!("0x{:x} is not mapped", addr),
    }
}

fn modules() {
    let mut found = false;
    for i in 0..4 {
        if let Some(m) = crate::usermode::boot_module(i) {
            out!(
                "module {}: 0x{:x}..0x{:x} ({} bytes)",
                i,
                m.start,
                m.end,
                m.len()
            );
            found = true;
        }
    }
    if !found {
        out!("no boot modules");
    }
}

fn history(editor: &LineEditor) {
    for (i, entry) in editor.history_entries().enumerate() {
        out!("{:>4}  {}", i + 1, entry);
    }
}

fn fault(args: &[&str; 8], count: usize) {
    if count == 0 {
        out!("usage: fault null|page|breakpoint");
        out!("  breakpoint returns; the others halt the kernel by design");
        return;
    }

    match args[0] {
        "breakpoint" | "bp" => {
            out!("raising int3 — the handler should print and resume");
            x86_64::instructions::interrupts::int3();
            out!("resumed");
        }
        "null" => {
            out!("dereferencing a null pointer — this will halt the kernel");
            unsafe {
                core::ptr::read_volatile(0 as *const u64);
            }
            out!("no fault: page 0 is mapped, which it should not be");
        }
        "page" => {
            out!("touching an unmapped higher-half address — this will halt the kernel");
            unsafe {
                core::ptr::read_volatile(0xFFFF_FF00_0000_0000u64 as *const u64);
            }
            out!("no fault: that address is mapped");
        }
        other => out!("unknown fault kind: {}", other),
    }
}

fn reboot() {
    out!("rebooting via triple fault...");

    // Load an empty IDT and interrupt: no handler, no fallback, reset.
    unsafe {
        let idt = x86_64::structures::DescriptorTablePointer {
            limit: 0,
            base: x86_64::VirtAddr::new(0),
        };
        x86_64::instructions::tables::lidt(&idt);
        core::arch::asm!("int3", options(noreturn));
    }
}


// --- filesystem ------------------------------------------------------------

/// Turn a user-supplied path into an absolute, normalised one.
///
/// Relative paths, `.` and `..` are resolved here rather than in the
/// filesystem layer: FAT has no notion of a working directory, and neither
/// should a filesystem driver.
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
                // `..` at the root stays at the root, as it does everywhere else.
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

fn require_fs() -> bool {
    if crate::fs::is_mounted() {
        return true;
    }
    out!("no filesystem mounted");
    out!("(attach a disk: -drive file=disk.img,format=raw,if=ide,index=0,media=disk)");
    false
}

fn ls(cwd: &str, args: &[&str; 8], count: usize) {
    if !require_fs() {
        return;
    }

    // Only one flag so far, so this stays a simple scan rather than a parser.
    let mut show_all = false;
    let mut target: Option<&str> = None;
    for i in 0..count {
        match args[i] {
            "-a" | "--all" => show_all = true,
            other if other.starts_with('-') => {
                out!("ls: unknown option {}", other);
                return;
            }
            other => target = Some(other),
        }
    }

    let path = match target {
        Some(t) => resolve(cwd, t),
        None => String::from(cwd),
    };

    match crate::fs::read_dir(&path) {
        Ok(entries) => {
            let mut shown = 0;
            let mut bytes = 0u64;

            for entry in entries.iter() {
                if !show_all && (entry.name == "." || entry.name == "..") {
                    continue;
                }

                out!(
                    "{}{}{:>10}  {}",
                    if entry.is_dir { "d" } else { "-" },
                    if entry.is_read_only() { "r-" } else { "rw" },
                    if entry.is_dir {
                        String::from("-")
                    } else {
                        let mut s = String::new();
                        use core::fmt::Write;
                        let _ = write!(s, "{}", entry.size);
                        s
                    },
                    entry.name
                );

                shown += 1;
                bytes += entry.size as u64;
            }

            out!();
            out!(
                "{} {}, {} bytes",
                shown,
                if shown == 1 { "entry" } else { "entries" },
                bytes
            );
        }
        Err(e) => out!("ls: {}: {}", path, describe_error(e)),
    }
}

fn cd(cwd: &mut String, args: &[&str; 8], count: usize) {
    if !require_fs() {
        return;
    }

    // Bare `cd` goes to the root, since there are no home directories.
    let target = if count == 0 { "/" } else { args[0] };
    let path = resolve(cwd, target);

    match crate::fs::lookup(&path) {
        Ok(entry) if entry.is_dir => {
            *cwd = path;
        }
        Ok(_) => out!("cd: {}: not a directory", path),
        Err(e) => out!("cd: {}: {}", path, describe_error(e)),
    }
}

/// Largest file `cat` will print. Big enough for anything on the test image,
/// small enough that a stray `cat` of a huge file does not lock up the console
/// for a minute of PIO reads.
const CAT_LIMIT: usize = 64 * 1024;

fn cat(cwd: &str, args: &[&str; 8], count: usize) {
    if !require_fs() {
        return;
    }
    if count == 0 {
        out!("usage: cat <path>");
        return;
    }

    for i in 0..count {
        let path = resolve(cwd, args[i]);

        match crate::fs::read_file(&path, CAT_LIMIT) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    continue;
                }

                // Print a line at a time. Character by character would be two
                // lock acquisitions and a format call per byte, which is
                // painfully slow for anything but the smallest file.
                let mut line = String::new();
                let mut binary = 0usize;

                for &b in bytes.iter() {
                    match b {
                        b'\n' => {
                            out!("{}", line);
                            line.clear();
                        }
                        b'\r' => {}
                        b'\t' => line.push('\t'),
                        0x20..=0x7e => line.push(b as char),
                        _ => {
                            binary += 1;
                            line.push('.');
                        }
                    }
                }
                if !line.is_empty() {
                    out!("{}", line);
                }

                if binary > 0 {
                    out!("[{} non-printable byte(s) shown as '.']", binary);
                }
                if bytes.len() == CAT_LIMIT {
                    out!("[truncated at {} bytes]", CAT_LIMIT);
                }
            }
            Err(e) => out!("cat: {}: {}", path, describe_error(e)),
        }
    }
}

fn stat(cwd: &str, args: &[&str; 8], count: usize) {
    if !require_fs() {
        return;
    }
    if count == 0 {
        out!("usage: stat <path>");
        return;
    }

    let path = resolve(cwd, args[0]);
    match crate::fs::lookup(&path) {
        Ok(entry) => {
            out!("{}", path);
            out!("  name          {}", entry.name);
            out!("  type          {}", if entry.is_dir { "directory" } else { "file" });
            out!("  size          {} bytes", entry.size);
            out!("  first cluster {}", entry.first_cluster);
            out!("  attributes    0x{:02x}{}", entry.attributes,
                 if entry.is_read_only() { " (read-only)" } else { "" });
        }
        Err(e) => out!("stat: {}: {}", path, describe_error(e)),
    }
}

fn lsblk() {
    match crate::block::with_device(|d| (String::from(d.name()), d.block_count())) {
        Some((name, blocks)) => {
            out!(
                "{}  {} blocks x {} bytes  ({} MB)",
                name,
                blocks,
                crate::block::BLOCK_SIZE,
                blocks * crate::block::BLOCK_SIZE as u64 / (1024 * 1024)
            );
        }
        None => out!("no block devices"),
    }
}

fn df() {
    match crate::fs::describe() {
        Some(d) => out!("{}", d),
        None => out!("nothing mounted"),
    }
}

fn describe_error(e: crate::fs::fat32::FsError) -> &'static str {
    use crate::fs::fat32::FsError;
    match e {
        FsError::NotFound => "no such file or directory",
        FsError::NotADirectory => "not a directory",
        FsError::IsADirectory => "is a directory",
        FsError::NotFat32 => "not a FAT32 filesystem",
        FsError::BadGeometry(why) => why,
        FsError::CorruptChain => "corrupt cluster chain",
        FsError::NameTooLong => "name too long",
        FsError::Block(_) => "disk read error",
    }
}
