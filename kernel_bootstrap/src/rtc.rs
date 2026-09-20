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

use hal_x86_64::PortIo;

const CMOS_SELECT: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;
const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
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
    Some(TimeOfDay {
        hour,
        minute,
        second,
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
