//! Console commands.
//!
//! Every one of these reports live kernel state or performs a real action.
//! None of them return canned strings — if something is not implemented, it
//! says so rather than printing a plausible-looking answer.

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

pub fn execute(line: &str, editor: &LineEditor) {
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
