//! How a program asks the kernel for things (PROC-003): the system-call
//! interface, and the capabilities it is checked against.
//!
//! A program runs in ring 3 in its own address space (`hal_x86_64::
//! user_space`) and can do nothing outside it but ask, with `int 0x80`:
//! the call's number in `rax`, its arguments in `rdi`, `rsi`, `rdx`, the
//! answer back in `rax`. There are no paths, no ambient "the console", no
//! global names: anything outside the program is reached through a
//! *handle*, a small number that means something only in its own table
//! of capabilities, given to it by whoever started it. A handle it was
//! not given is not refused -- it simply does not exist.
//!
//! This is the whole interface for now:
//!
//! | rax | call                     | answer                       |
//! |-----|--------------------------|------------------------------|
//! | 0   | `exit(code)`             | does not return              |
//! | 1   | `yield()`                | 0                            |
//! | 2   | `sleep(ms)`              | 0                            |
//! | 3   | `send(handle, ptr, len)` | bytes sent, or an error      |
//! | 4   | `time()`                 | milliseconds since boot      |
//! | 5   | `drop(handle)`           | 0, or an error               |
//!
//! Errors are small negative numbers (as `i64`), so a program checks the
//! sign. It is kept free of the machine so it runs under `cargo test`.

/// The vector programs call the kernel through.
pub const SYSCALL_VECTOR: u8 = 0x80;
/// Most bytes one `send` carries.
pub const SEND_MAX: usize = 256;
/// Handles a program can hold.
pub const MAX_HANDLES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    Exit { code: u64 },
    Yield,
    Sleep { ms: u64 },
    Send { handle: u64, ptr: u64, len: u64 },
    Time,
    Drop { handle: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum Error {
    /// No such call.
    NoSuchCall = -1,
    /// The handle names nothing in this program's table.
    NoSuchHandle = -2,
    /// A pointer that is not the program's own readable memory.
    BadPointer = -3,
    /// More than a call takes at once.
    TooBig = -4,
    /// The capability is held but does not allow this.
    NotAllowed = -5,
}

impl Error {
    /// The value a program sees in `rax`.
    pub fn to_rax(self) -> u64 {
        self as i64 as u64
    }
}

impl Call {
    /// The call a program's registers ask for.
    pub fn decode(rax: u64, rdi: u64, rsi: u64, rdx: u64) -> Result<Call, Error> {
        Ok(match rax {
            0 => Call::Exit { code: rdi },
            1 => Call::Yield,
            2 => Call::Sleep { ms: rdi },
            3 => Call::Send {
                handle: rdi,
                ptr: rsi,
                len: rdx,
            },
            4 => Call::Time,
            5 => Call::Drop { handle: rdi },
            _ => return Err(Error::NoSuchCall),
        })
    }
}

/// Something outside a program that it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Lines to the Terminal that started it.
    Console,
    /// A channel it may only look at (to show a held capability without
    /// the right to send refuses rather than vanishes).
    ReadOnlyConsole,
}

impl Capability {
    pub fn can_send(self) -> bool {
        matches!(self, Capability::Console)
    }
}

/// A program's capabilities, by handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handles {
    slots: [Option<Capability>; MAX_HANDLES],
}

impl Default for Handles {
    fn default() -> Self {
        Self::new()
    }
}

impl Handles {
    pub const fn new() -> Self {
        Self {
            slots: [None; MAX_HANDLES],
        }
    }

    /// Give the program `cap`; its handle, or `None` when the table is full.
    pub fn grant(&mut self, cap: Capability) -> Option<u64> {
        let slot = self.slots.iter().position(Option::is_none)?;
        self.slots[slot] = Some(cap);
        Some(slot as u64)
    }

    pub fn get(&self, handle: u64) -> Result<Capability, Error> {
        usize::try_from(handle)
            .ok()
            .and_then(|h| self.slots.get(h).copied().flatten())
            .ok_or(Error::NoSuchHandle)
    }

    pub fn drop_handle(&mut self, handle: u64) -> Result<(), Error> {
        self.get(handle)?;
        self.slots[handle as usize] = None;
        Ok(())
    }

    /// Check a `send` before any memory is touched: the handle, the
    /// right, the size.
    pub fn check_send(&self, handle: u64, len: u64) -> Result<Capability, Error> {
        let cap = self.get(handle)?;
        if !cap.can_send() {
            return Err(Error::NotAllowed);
        }
        if len > SEND_MAX as u64 {
            return Err(Error::TooBig);
        }
        Ok(cap)
    }

    pub fn count(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

/// Why a program ended, as the Terminal says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Exited(u64),
    /// Stopped by `stop` (between its instructions, or at a call).
    Stopped,
    /// A CPU exception in the program: the vector, the address it
    /// touched (page faults), and where it was.
    Fault {
        vector: u64,
        address: u64,
        rip: u64,
    },
}

impl End {
    /// "exited with 0", "stopped", "stopped: a page fault at 0x... (not
    /// its memory), at 0x...".
    pub fn describe(self, out: &mut impl core::fmt::Write) -> core::fmt::Result {
        match self {
            End::Exited(code) => write!(out, "exited with {code}"),
            End::Stopped => write!(out, "stopped"),
            End::Fault {
                vector: 14,
                address,
                rip,
            } => write!(
                out,
                "ended by the kernel: it touched 0x{address:x}, not its memory (at 0x{rip:x})"
            ),
            End::Fault { vector, rip, .. } => {
                let what = match vector {
                    0 => "divided by zero",
                    6 => "ran an instruction that does not exist",
                    13 => "did what a program may not (general protection)",
                    _ => "faulted",
                };
                write!(out, "ended by the kernel: it {what} (at 0x{rip:x})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::String;

    #[test]
    fn registers_decode_to_calls_and_unknown_numbers_are_refused() {
        assert_eq!(Call::decode(0, 7, 0, 0), Ok(Call::Exit { code: 7 }));
        assert_eq!(
            Call::decode(3, 0, 0x40_0000, 17),
            Ok(Call::Send {
                handle: 0,
                ptr: 0x40_0000,
                len: 17
            })
        );
        assert_eq!(Call::decode(99, 0, 0, 0), Err(Error::NoSuchCall));
        assert_eq!(Error::NoSuchHandle.to_rax() as i64, -2);
    }

    #[test]
    fn a_handle_not_given_does_not_exist() {
        let mut h = Handles::new();
        assert_eq!(h.grant(Capability::Console), Some(0));
        assert_eq!(h.check_send(0, 10), Ok(Capability::Console));
        assert_eq!(h.check_send(1, 10), Err(Error::NoSuchHandle));
        assert_eq!(h.check_send(u64::MAX, 10), Err(Error::NoSuchHandle));
        assert_eq!(h.check_send(0, SEND_MAX as u64 + 1), Err(Error::TooBig));
    }

    #[test]
    fn a_held_capability_without_the_right_refuses() {
        let mut h = Handles::new();
        let ro = h.grant(Capability::ReadOnlyConsole).unwrap();
        assert_eq!(h.check_send(ro, 1), Err(Error::NotAllowed));
    }

    #[test]
    fn a_dropped_handle_is_gone_for_good() {
        let mut h = Handles::new();
        let c = h.grant(Capability::Console).unwrap();
        assert_eq!(h.drop_handle(c), Ok(()));
        assert_eq!(h.check_send(c, 1), Err(Error::NoSuchHandle));
        assert_eq!(h.drop_handle(c), Err(Error::NoSuchHandle));
        assert_eq!(h.count(), 0);
        for _ in 0..MAX_HANDLES {
            assert!(h.grant(Capability::Console).is_some());
        }
        assert_eq!(h.grant(Capability::Console), None);
    }

    #[test]
    fn ends_read_plainly() {
        let say = |e: End| {
            let mut s = String::new();
            e.describe(&mut s).unwrap();
            s
        };
        assert_eq!(say(End::Exited(0)), "exited with 0");
        assert!(say(End::Fault {
            vector: 14,
            address: 0xffff_8000_0010_0000,
            rip: 0x40_0010
        })
        .contains("touched 0xffff800000100000, not its memory"));
        assert!(say(End::Fault {
            vector: 13,
            address: 0,
            rip: 1
        })
        .contains("general protection"));
    }
}
