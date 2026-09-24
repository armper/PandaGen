//! The CMOS real-time clock (GFX-052).
//!
//! The desk's top bar showed seconds since boot dressed up as a clock. This
//! reads the actual time of day from the RTC so the bar can show a clock
//! that is one. The read is a handful of port operations; it is done once
//! per frame and never spins for long -- an update in progress is waited
//! out for a bounded number of polls and then the previous reading stands.
//!
//! Host-testable: the register decoding takes a `PortIo`, so a fake device
//! can present any register contents.

extern crate alloc;

use hal_x86_64::PortIo;

const CMOS_SELECT: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;
const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
/// The century register most firmware keeps; 0 when it does not.
const REG_CENTURY: u8 = 0x32;
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;
/// Status A bit 7: an update is in progress and the time registers are
/// not stable.
const UPDATE_IN_PROGRESS: u8 = 0x80;
/// Status B bit 2: registers are binary rather than BCD.
const BINARY_MODE: u8 = 0x04;
/// Status B bit 1: 24-hour mode.
const HOURS_24: u8 = 0x02;
/// How many times to poll for the update flag to clear before giving up.
const MAX_UPDATE_POLLS: usize = 4_000;

/// A time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// A calendar date and time (GFX-056), as the RTC keeps it: no zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub time: TimeOfDay,
}

impl DateTime {
    /// Seconds since 1970-01-01 00:00, treating the RTC as UTC.
    pub fn unix_seconds(&self) -> u64 {
        let days = days_from_civil(self.year as i64, self.month as i64, self.day as i64);
        let secs = days * 86_400
            + self.time.hour as i64 * 3_600
            + self.time.minute as i64 * 60
            + self.time.second as i64;
        secs.max(0) as u64
    }

    /// The inverse of [`DateTime::unix_seconds`].
    pub fn from_unix_seconds(secs: u64) -> Self {
        let days = (secs / 86_400) as i64;
        let rem = secs % 86_400;
        let (year, month, day) = civil_from_days(days);
        Self {
            year: year as u16,
            month: month as u8,
            day: day as u8,
            time: TimeOfDay {
                hour: (rem / 3_600) as u8,
                minute: ((rem % 3_600) / 60) as u8,
                second: (rem % 60) as u8,
            },
        }
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The day of the week, Monday 0 to Sunday 6 (GFX-076). 1970-01-01 was
/// a Thursday.
pub fn weekday(year: u16, month: u8, day: u8) -> usize {
    (days_from_civil(year as i64, month as i64, day as i64) + 3).rem_euclid(7) as usize
}

/// How many days `month` of `year` has.
pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// `2026-09-20 23:17` for a listing; "-" when the time was never set.
pub fn format_unix_minutes(secs: u64) -> alloc::string::String {
    if secs == 0 {
        return alloc::string::String::from("-");
    }
    let d = DateTime::from_unix_seconds(secs);
    alloc::format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        d.year,
        d.month,
        d.day,
        d.time.hour,
        d.time.minute
    )
}

fn read_register<P: PortIo>(port: &mut P, register: u8) -> u8 {
    // Bit 7 of the select register controls NMI; leave it set so an NMI
    // cannot arrive between the select and the read.
    port.outb(CMOS_SELECT, 0x80 | register);
    port.inb(CMOS_DATA)
}

fn from_bcd(value: u8) -> u8 {
    (value >> 4) * 10 + (value & 0x0F)
}

/// Read the time of day, or `None` if the clock never settled or answered
/// something no clock says.
pub fn read_time<P: PortIo>(port: &mut P) -> Option<TimeOfDay> {
    read_clock(port).map(|d| d.time)
}

/// Read the date and time. A century register of 0 means "20xx".
pub fn read_clock<P: PortIo>(port: &mut P) -> Option<DateTime> {
    let mut polls = 0;
    while read_register(port, REG_STATUS_A) & UPDATE_IN_PROGRESS != 0 {
        polls += 1;
        if polls >= MAX_UPDATE_POLLS {
            return None;
        }
    }
    let status_b = read_register(port, REG_STATUS_B);
    let raw_seconds = read_register(port, REG_SECONDS);
    let raw_minutes = read_register(port, REG_MINUTES);
    let raw_hours = read_register(port, REG_HOURS);
    let raw_day = read_register(port, REG_DAY);
    let raw_month = read_register(port, REG_MONTH);
    let raw_year = read_register(port, REG_YEAR);
    let raw_century = read_register(port, REG_CENTURY);

    let binary = status_b & BINARY_MODE != 0;
    let decode = |value: u8| if binary { value } else { from_bcd(value) };

    let second = decode(raw_seconds);
    let minute = decode(raw_minutes);
    // In 12-hour mode bit 7 of the hour register is PM.
    let pm = raw_hours & 0x80 != 0;
    let mut hour = decode(raw_hours & 0x7F);
    if status_b & HOURS_24 == 0 {
        hour %= 12;
        if pm {
            hour += 12;
        }
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let day = decode(raw_day);
    let month = decode(raw_month);
    let century = if raw_century == 0 {
        20
    } else {
        decode(raw_century)
    };
    let year = century as u16 * 100 + decode(raw_year) as u16;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(DateTime {
        year,
        month,
        day,
        time: TimeOfDay {
            hour,
            minute,
            second,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake CMOS: a register file behind the select/data ports.
    struct FakeCmos {
        selected: u8,
        registers: [u8; 128],
    }

    impl FakeCmos {
        fn new(status_b: u8, hours: u8, minutes: u8, seconds: u8) -> Self {
            let mut registers = [0u8; 128];
            registers[REG_STATUS_B as usize] = status_b;
            registers[REG_HOURS as usize] = hours;
            registers[REG_MINUTES as usize] = minutes;
            registers[REG_SECONDS as usize] = seconds;
            // A real date, so a time-only test is not failed by the
            // calendar check.
            registers[REG_DAY as usize] = 0x01;
            registers[REG_MONTH as usize] = 0x01;
            registers[REG_YEAR as usize] = 0x26;
            Self {
                selected: 0,
                registers,
            }
        }
    }

    impl PortIo for FakeCmos {
        fn inb(&mut self, port: u16) -> u8 {
            assert_eq!(port, CMOS_DATA);
            self.registers[(self.selected & 0x7F) as usize]
        }
        fn outb(&mut self, port: u16, value: u8) {
            assert_eq!(port, CMOS_SELECT);
            self.selected = value;
        }
    }

    #[test]
    fn the_date_registers_decode_and_round_trip_through_unix_seconds() {
        let mut cmos = FakeCmos::new(HOURS_24, 0x23, 0x17, 0x05);
        cmos.registers[REG_DAY as usize] = 0x20;
        cmos.registers[REG_MONTH as usize] = 0x09;
        cmos.registers[REG_YEAR as usize] = 0x26;
        let clock = read_clock(&mut cmos).unwrap();
        assert_eq!((clock.year, clock.month, clock.day), (2026, 9, 20));
        let secs = clock.unix_seconds();
        assert_eq!(secs, 1_789_946_225);
        assert_eq!(DateTime::from_unix_seconds(secs), clock);
        assert_eq!(format_unix_minutes(secs), "2026-09-20 23:17");
        assert_eq!(format_unix_minutes(0), "-");
        // The epoch itself, and a leap day.
        assert_eq!(DateTime::from_unix_seconds(0).year, 1970);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        // A month no calendar has is not a date.
        cmos.registers[REG_MONTH as usize] = 0x13;
        assert_eq!(read_clock(&mut cmos), None);
    }

    #[test]
    fn bcd_24_hour_registers_decode() {
        let mut cmos = FakeCmos::new(HOURS_24, 0x23, 0x59, 0x07);
        assert_eq!(
            read_time(&mut cmos),
            Some(TimeOfDay {
                hour: 23,
                minute: 59,
                second: 7
            })
        );
    }

    #[test]
    fn binary_registers_decode_without_bcd() {
        let mut cmos = FakeCmos::new(HOURS_24 | BINARY_MODE, 17, 45, 30);
        assert_eq!(
            read_time(&mut cmos).map(|t| (t.hour, t.minute)),
            Some((17, 45))
        );
    }

    #[test]
    fn twelve_hour_mode_folds_pm_in() {
        // 12-hour BCD: 0x05 with the PM bit = 17:00; 0x12 without = 00:xx.
        let mut cmos = FakeCmos::new(0, 0x80 | 0x05, 0x00, 0x00);
        assert_eq!(read_time(&mut cmos).map(|t| t.hour), Some(17));
        let mut midnight = FakeCmos::new(0, 0x12, 0x00, 0x00);
        assert_eq!(read_time(&mut midnight).map(|t| t.hour), Some(0));
    }

    #[test]
    fn a_clock_that_never_settles_or_talks_nonsense_is_none() {
        let mut stuck = FakeCmos::new(HOURS_24, 0x10, 0x10, 0x10);
        stuck.registers[REG_STATUS_A as usize] = UPDATE_IN_PROGRESS;
        assert_eq!(read_time(&mut stuck), None);

        let mut nonsense = FakeCmos::new(HOURS_24, 0x29, 0x00, 0x00);
        assert_eq!(read_time(&mut nonsense), None);
    }
}
