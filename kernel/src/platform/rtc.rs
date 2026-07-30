//! The CMOS real-time clock.
//!
//! ## Why this exists
//!
//! `sys_time` returned `Ok(0)` under a `// TODO: Implement time getting`. Zero is
//! a perfectly valid Unix timestamp — midnight, 1 January 1970 — so a caller had
//! no way to distinguish "the clock says 1970" from "there is no clock". This is
//! the smallest thing that makes the answer true.
//!
//! ## Reading it without getting a torn value
//!
//! The RTC updates its registers once a second, and reading part-way through an
//! update gives a mix of old and new fields: 10:59:59 can be read as 10:59:00 or
//! 11:59:59 depending on which registers were caught. Status register A bit 7 is
//! the update-in-progress flag; the standard approach is to wait for it to clear,
//! read everything, then read everything again and accept the values only if the
//! two agree. That is what [`read_raw`] does.
//!
//! ## BCD
//!
//! By default the RTC reports binary-coded decimal — 0x59 means fifty-nine, not
//! eighty-nine. Status register B bit 2 says whether the values are binary
//! instead, and bit 1 whether the hour is 24-hour. Both are checked rather than
//! assumed, because QEMU and real hardware do not always agree, and a kernel that
//! assumes BCD on a binary RTC reports plausible-looking nonsense.

use x86_64::instructions::port::Port;

use crate::serial_println;

const CMOS_ADDRESS: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;

/// A reading, in whatever encoding the RTC uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Raw {
    second: u8,
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    year: u8,
}

fn cmos_read(register: u8) -> u8 {
    let mut address: Port<u8> = Port::new(CMOS_ADDRESS);
    let mut data: Port<u8> = Port::new(CMOS_DATA);
    unsafe {
        // Bit 7 disables NMI while the index is latched. Leaving it clear is the
        // conventional choice and matches what every other reader does.
        address.write(register);
        data.read()
    }
}

fn update_in_progress() -> bool {
    cmos_read(REG_STATUS_A) & 0x80 != 0
}

fn read_once() -> Raw {
    Raw {
        second: cmos_read(REG_SECONDS),
        minute: cmos_read(REG_MINUTES),
        hour: cmos_read(REG_HOURS),
        day: cmos_read(REG_DAY),
        month: cmos_read(REG_MONTH),
        year: cmos_read(REG_YEAR),
    }
}

/// Read the clock twice and only accept a value both reads agree on.
fn read_raw() -> Option<Raw> {
    // Bounded rather than a bare `while`: a machine with no RTC leaves the
    // update flag set forever, and hanging the kernel on `time()` would be a
    // worse failure than not having a clock.
    for _ in 0..1_000_000 {
        if !update_in_progress() {
            break;
        }
    }
    if update_in_progress() {
        return None;
    }

    let mut previous = read_once();
    for _ in 0..16 {
        let current = read_once();
        if current == previous {
            return Some(current);
        }
        previous = current;
    }

    None
}

fn bcd_to_binary(value: u8) -> u8 {
    (value & 0x0F) + ((value >> 4) * 10)
}

/// Days from 1 January 1970 to 1 January of `year`.
fn days_before_year(year: u64) -> u64 {
    let mut days = 0;
    for y in 1970..year {
        days += if is_leap(y) { 366 } else { 365 };
    }
    days
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_before_month(year: u64, month: u64) -> u64 {
    const LENGTHS: [u64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut days = 0;
    for m in 1..month {
        days += LENGTHS[(m - 1) as usize];
        if m == 2 && is_leap(year) {
            days += 1;
        }
    }
    days
}

/// Seconds since the Unix epoch, or `None` if the clock cannot be read.
///
/// The RTC keeps no timezone, and there is no configuration to tell us what it
/// is set to. UTC is assumed — which is what QEMU provides by default and what
/// most systems set it to. Getting that wrong shifts the answer by hours, not
/// years, and there is nothing here that could detect it.
pub fn unix_time() -> Option<u64> {
    let raw = read_raw()?;
    let status_b = cmos_read(REG_STATUS_B);
    let binary = status_b & 0x04 != 0;
    let twenty_four_hour = status_b & 0x02 != 0;

    let convert = |v: u8| if binary { v } else { bcd_to_binary(v) };

    let second = convert(raw.second) as u64;
    let minute = convert(raw.minute) as u64;
    let day = convert(raw.day) as u64;
    let month = convert(raw.month) as u64;
    let year_in_century = convert(raw.year & 0x7F) as u64;

    // In 12-hour mode bit 7 of the hour register is the PM flag, and it survives
    // the BCD conversion, so it has to come off first.
    let hour = if twenty_four_hour {
        convert(raw.hour) as u64
    } else {
        let pm = raw.hour & 0x80 != 0;
        let h = convert(raw.hour & 0x7F) as u64;
        match (pm, h) {
            (false, 12) => 0,
            (false, h) => h,
            (true, 12) => 12,
            (true, h) => h + 12,
        }
    };

    // No century register is guaranteed, so the century is inferred. 70..99 maps
    // to the 1900s and 00..69 to the 2000s, the same window the C library uses.
    let year = if year_in_century >= 70 {
        1900 + year_in_century
    } else {
        2000 + year_in_century
    };

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59
        || second > 60
    {
        serial_println!(
            "RTC returned an implausible reading: {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            year,
            month,
            day,
            hour,
            minute,
            second
        );
        return None;
    }

    let days = days_before_year(year) + days_before_month(year, month) + (day - 1);
    Some(((days * 24 + hour) * 60 + minute) * 60 + second)
}

/// Log the clock once at boot, so a wrong reading is visible without a program
/// having to ask for it.
pub fn report() {
    match unix_time() {
        Some(secs) => {
            let status_b = cmos_read(REG_STATUS_B);
            serial_println!(
                "RTC: {} seconds since the epoch ({}, {}-hour)",
                secs,
                if status_b & 0x04 != 0 { "binary" } else { "BCD" },
                if status_b & 0x02 != 0 { "24" } else { "12" }
            );
        }
        None => serial_println!("RTC: unreadable; time() will report ENOSYS"),
    }
}
