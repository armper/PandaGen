//! Supervision (PROC-007): what a program may use, and what happens when
//! it breaks.
//!
//! The kernel's mechanisms -- budgets in the scheduler, address spaces
//! whose size is fixed at load, programs ended alone when they fault --
//! take numbers and decisions from here. This is the policy, and it is
//! plain code so it runs under `cargo test`:
//!
//! - **Memory.** A program's image and stack must fit in its limit before
//!   anything is mapped; a program cannot grow past what it was loaded
//!   with, so the check at load is the whole of it.
//! - **The processor.** A program gets at most a share of it in every
//!   window of time (`sched::Budget`), however it loops: the desk always
//!   has the rest.
//! - **Crashes.** A program with a card that the kernel ends is started
//!   again into the same card -- up to three times in a minute. A fourth
//!   crash in that minute leaves the card saying so, with a button to try
//!   once more: a program that fails every time is not restarted
//!   forever.

extern crate alloc;

use alloc::string::String;

use crate::sched::Budget;

/// What one program may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of memory: its image's pieces and its stack.
    pub memory: u64,
    /// Its share of the processor.
    pub cpu: Budget,
}

/// Every program's limits, for now: 16 MiB, and three quarters of the
/// processor (15 ticks in every 20, so 150 ms in every 200).
pub const PROGRAM_LIMITS: Limits = Limits {
    memory: 16 * 1024 * 1024,
    cpu: Budget {
        ticks: 15,
        window: 20,
    },
};

impl Limits {
    /// Whether a program laying out `image` bytes with a `stack`-byte
    /// stack fits; if not, why, as the Terminal says it.
    pub fn check_memory(&self, name: &str, image: u64, stack: u64) -> Result<(), String> {
        let needs = image.saturating_add(stack);
        if needs <= self.memory {
            return Ok(());
        }
        Err(alloc::format!(
            "run: {name} needs {} KiB; a program may have {} KiB",
            needs / 1024,
            self.memory / 1024
        ))
    }
}

/// Restarts allowed in [`RESTART_WINDOW`] ticks.
pub const RESTARTS: usize = 3;
/// A minute, in ticks.
pub const RESTART_WINDOW: u64 = 6000;

/// What to do about a program that crashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Start it again: this is restart `n` of [`RESTARTS`] in the window.
    Restart(usize),
    /// It has crashed too often: leave it, and let the person decide.
    GiveUp,
}

/// A program's crashes, for deciding about the next.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Crashes {
    /// When the recent ones were, oldest first.
    at: [u64; RESTARTS],
    count: usize,
}

impl Crashes {
    /// It crashed at `now`: restart it, or give up.
    pub fn crashed(&mut self, now: u64) -> Decision {
        // Forget crashes older than the window.
        let recent = self.at[..self.count]
            .iter()
            .filter(|&&t| now.saturating_sub(t) < RESTART_WINDOW)
            .count();
        let skip = self.count - recent;
        self.at.copy_within(skip..self.count, 0);
        self.count = recent;
        if self.count == RESTARTS {
            return Decision::GiveUp;
        }
        self.at[self.count] = now;
        self.count += 1;
        Decision::Restart(self.count)
    }

    /// The person started it again: a fresh count.
    pub fn forgive(&mut self) {
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_too_big_for_its_limit_is_refused_before_loading() {
        let limits = PROGRAM_LIMITS;
        assert_eq!(
            limits.check_memory("calculator", 288 * 1024, 16 * 1024),
            Ok(())
        );
        let why = limits
            .check_memory("huge", 20 * 1024 * 1024, 16 * 1024)
            .unwrap_err();
        assert!(
            why.contains("huge needs 20496 KiB; a program may have 16384 KiB"),
            "{why}"
        );
        assert!(limits.check_memory("wrap", u64::MAX, 1).is_err());
    }

    #[test]
    fn three_restarts_a_minute_then_the_person_decides() {
        let mut crashes = Crashes::default();
        assert_eq!(crashes.crashed(100), Decision::Restart(1));
        assert_eq!(crashes.crashed(200), Decision::Restart(2));
        assert_eq!(crashes.crashed(300), Decision::Restart(3));
        assert_eq!(crashes.crashed(400), Decision::GiveUp);
        assert_eq!(
            crashes.crashed(500),
            Decision::GiveUp,
            "still within the minute"
        );
        // A minute after the first, one slot is free again.
        assert_eq!(crashes.crashed(100 + RESTART_WINDOW), Decision::Restart(3));
        // Starting it by hand forgives the past.
        crashes.forgive();
        assert_eq!(
            crashes.crashed(100 + RESTART_WINDOW + 1),
            Decision::Restart(1)
        );
    }

    #[test]
    fn a_crash_now_and_then_is_always_restarted() {
        let mut crashes = Crashes::default();
        for i in 0..20u64 {
            assert_eq!(crashes.crashed(i * RESTART_WINDOW), Decision::Restart(1));
        }
    }

    #[test]
    fn a_program_never_has_all_of_the_processor() {
        let cpu = PROGRAM_LIMITS.cpu;
        assert!(cpu.ticks < cpu.window, "never all of it");
        assert_eq!(cpu.ticks * 100 / cpu.window, 75);
    }
}
