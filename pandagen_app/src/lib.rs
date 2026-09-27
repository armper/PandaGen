//! Writing a PandaGen program (PROC-004).
//!
//! A program runs in ring 3 in an address space of its own and can do
//! nothing outside it but ask the kernel. This crate is the asking: the
//! system calls, the handles a program was given, and the entry point.
//!
//! ```ignore
//! #![no_std]
//! #![no_main]
//! use pandagen_app::{entry, Console};
//!
//! entry!(main);
//!
//! fn main(console: Console) -> u64 {
//!     pandagen_app::say!(console, "hello from {}", "Rust");
//!     0
//! }
//! ```
//!
//! What a program may use it asks for in its image (`program_image`);
//! whoever starts it grants what they choose, as handles numbered in the
//! order asked. A program that asks for the console gets it as handle 0,
//! and `entry!` hands it to `main`. There is nothing else to reach for:
//! no paths, no global names, no ambient "standard output".

#![no_std]

use core::fmt;

/// System call numbers (`kernel_bootstrap::syscall_abi`).
pub mod call {
    pub const EXIT: u64 = 0;
    pub const YIELD: u64 = 1;
    pub const SLEEP: u64 = 2;
    pub const SEND: u64 = 3;
    pub const TIME: u64 = 4;
    pub const DROP: u64 = 5;
    pub const PRESENT: u64 = 6;
    pub const POLL: u64 = 7;
    pub const WAIT: u64 = 8;
    pub const RANDOM: u64 = 9;
}

/// Fill `bytes` from the machine's generator (at most 256 at a time).
pub fn random_bytes(bytes: &mut [u8]) -> Result<(), Error> {
    for chunk in bytes.chunks_mut(SEND_MAX) {
        check(syscall(
            call::RANDOM,
            chunk.as_mut_ptr() as u64,
            chunk.len() as u64,
            0,
        ))?;
    }
    Ok(())
}

/// A random `u64` from the machine's generator (0 on the host).
pub fn random_u64() -> u64 {
    let mut bytes = [0u8; 8];
    let _ = random_bytes(&mut bytes);
    u64::from_le_bytes(bytes)
}

/// Notices on the desk (PROC-008), when the program asked for them: what
/// it says appears in the desk's notices centre, with a chime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notices(pub Handle);

impl Notices {
    pub fn say(&self, text: &str) -> Result<usize, Error> {
        Console(self.0).send(text.as_bytes())
    }
}

/// What a card shows and what happens to it (`app_protocol`).
pub use app_protocol::{Area, Event, Kind, Op, Role, ViewError, ViewWriter, VIEW_MAX};

/// A program's heap (PROC-006), with the `heap` feature: `HEAP_BYTES` of
/// its own memory -- zeroed pages the image lays out -- managed by the
/// same first-fit allocator the kernel uses. `entry!` sets it up before
/// `main`, so `alloc`'s `String`, `Vec` and `Box` just work.
#[cfg(feature = "heap")]
pub mod heap {
    use free_list_heap::FreeListHeap;

    pub const HEAP_BYTES: usize = 256 * 1024;

    #[repr(C, align(16))]
    struct Arena([u8; HEAP_BYTES]);

    static mut ARENA: Arena = Arena([0; HEAP_BYTES]);

    #[cfg_attr(all(target_os = "none", not(test)), global_allocator)]
    static HEAP: FreeListHeap = FreeListHeap::empty();

    pub(crate) fn init() {
        // SAFETY: once, before anything allocates; the arena is the
        // program's own and nothing else refers to it.
        unsafe { HEAP.init(core::ptr::addr_of_mut!(ARENA) as usize, HEAP_BYTES) }
    }
}

/// What `entry!` does before `main`.
#[doc(hidden)]
pub fn __start() {
    #[cfg(feature = "heap")]
    heap::init();
}

/// Most bytes one `send` carries.
pub const SEND_MAX: usize = 256;

/// Why the kernel said no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NoSuchCall,
    /// The handle names nothing this program holds.
    NoSuchHandle,
    /// A pointer that is not this program's memory.
    BadPointer,
    TooBig,
    /// Held, but without the right for this.
    NotAllowed,
    Other(i64),
}

impl Error {
    fn from_answer(answer: i64) -> Error {
        match answer {
            -1 => Error::NoSuchCall,
            -2 => Error::NoSuchHandle,
            -3 => Error::BadPointer,
            -4 => Error::TooBig,
            -5 => Error::NotAllowed,
            n => Error::Other(n),
        }
    }
}

/// An answer: a count or value when non-negative, an error when not.
pub fn check(answer: i64) -> Result<u64, Error> {
    if answer < 0 {
        Err(Error::from_answer(answer))
    } else {
        Ok(answer as u64)
    }
}

/// Ask the kernel: `int 0x80`, the call in `rax`, arguments in `rdi`,
/// `rsi`, `rdx`, the answer back in `rax`. The kernel keeps every other
/// register as it was.
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn syscall(number: u64, a: u64, b: u64, c: u64) -> i64 {
    let answer: i64;
    // SAFETY: the kernel checks everything it is given; a bad argument is
    // an error answer, never a fault here.
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") number as i64 => answer,
            in("rdi") a,
            in("rsi") b,
            in("rdx") c,
            options(nostack),
        );
    }
    answer
}

/// On the host there is no kernel to ask; everything is "no such call".
#[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
pub fn syscall(_number: u64, _a: u64, _b: u64, _c: u64) -> i64 {
    -1
}

/// End the program with `code` (0 is success).
pub fn exit(code: u64) -> ! {
    syscall(call::EXIT, code, 0, 0);
    // Only on the host does exit return.
    #[allow(clippy::empty_loop)]
    loop {}
}

/// Let another thread run.
pub fn yield_now() {
    syscall(call::YIELD, 0, 0, 0);
}

/// Sleep at least `ms` milliseconds (the clock ticks every 10).
pub fn sleep_ms(ms: u64) {
    syscall(call::SLEEP, ms, 0, 0);
}

/// Milliseconds since the machine started.
pub fn time_ms() -> u64 {
    check(syscall(call::TIME, 0, 0, 0)).unwrap_or(0)
}

/// A capability held, by its handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle(pub u64);

impl Handle {
    /// Give it up; it is gone for good.
    pub fn drop_handle(self) -> Result<(), Error> {
        check(syscall(call::DROP, self.0, 0, 0)).map(|_| ())
    }
}

/// Lines to the Terminal that started the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Console(pub Handle);

impl Console {
    /// The console, when the program asked for it first (handle 0).
    pub const FIRST: Console = Console(Handle(0));

    /// Send one line (at most [`SEND_MAX`] bytes; more is refused).
    pub fn send(&self, line: &[u8]) -> Result<usize, Error> {
        check(syscall(
            call::SEND,
            (self.0).0,
            line.as_ptr() as u64,
            line.len() as u64,
        ))
        .map(|n| n as usize)
    }

    /// Format a line and send it; a line too long is cut, at a character.
    pub fn say(&self, args: fmt::Arguments) -> Result<usize, Error> {
        let mut line = Line::new();
        let _ = fmt::write(&mut line, args);
        self.send(line.as_bytes())
    }
}

/// A line being formatted, in a fixed buffer (programs need no heap).
pub struct Line {
    bytes: [u8; SEND_MAX],
    len: usize,
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

impl Line {
    pub const fn new() -> Self {
        Self {
            bytes: [0; SEND_MAX],
            len: 0,
        }
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub fn as_str(&self) -> &str {
        // Only whole characters are ever written.
        core::str::from_utf8(self.as_bytes()).unwrap_or("")
    }
}

/// Format into a [`Line`]: `line!("{n} left")`.
#[macro_export]
macro_rules! line {
    ($($arg:tt)*) => {{
        let mut line = $crate::Line::new();
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!($($arg)*));
        line
    }};
}

/// The program's card on the desk (PROC-005), when it asked for one.
///
/// It describes what the card shows with [`ViewWriter`] and hands the
/// view to [`Card::present`]; the desk draws it in the desk's theme.
/// Keys typed into the card and buttons clicked in it come back as
/// [`Event::Key`], the card's size as [`Event::Size`] (first, and on
/// every change), and its closing as [`Event::Closed`], after which the
/// program has two seconds to finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Card(pub Handle);

impl Card {
    /// Show `view` (the bytes [`ViewWriter::finish`] gives).
    pub fn present(&self, view: &[u8]) -> Result<(), Error> {
        check(syscall(
            call::PRESENT,
            (self.0).0,
            view.as_ptr() as u64,
            view.len() as u64,
        ))
        .map(|_| ())
    }

    /// The next event, if one is waiting.
    pub fn poll(&self) -> Option<Event> {
        let mut bytes = [0u8; app_protocol::EVENT_BYTES];
        match check(syscall(
            call::POLL,
            (self.0).0,
            bytes.as_mut_ptr() as u64,
            0,
        )) {
            Ok(1) => Event::decode(&bytes),
            _ => None,
        }
    }

    /// The next event, sleeping until there is one.
    pub fn next_event(&self) -> Event {
        loop {
            if let Some(event) = self.poll() {
                return event;
            }
            syscall(call::WAIT, 0, 0, 0);
        }
    }

    /// The next event, or `None` after `ms` milliseconds without one.
    pub fn event_within(&self, ms: u64) -> Option<Event> {
        if let Some(event) = self.poll() {
            return Some(event);
        }
        syscall(call::WAIT, ms.max(1), 0, 0);
        self.poll()
    }
}

impl fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for ch in s.chars() {
            let mut buf = [0u8; 4];
            let enc = ch.encode_utf8(&mut buf).as_bytes();
            if self.len + enc.len() > SEND_MAX {
                return Err(fmt::Error);
            }
            self.bytes[self.len..self.len + enc.len()].copy_from_slice(enc);
            self.len += enc.len();
        }
        Ok(())
    }
}

/// `say!(console, "fmt", args...)`: format and send a line.
#[macro_export]
macro_rules! say {
    ($console:expr, $($arg:tt)*) => {
        $console.say(format_args!($($arg)*))
    };
}

/// The program's entry: `entry!(main)` with `fn main(console: Console)
/// -> u64`. `main`'s answer is the exit code; a panic says why on the
/// console and exits with 101.
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[no_mangle]
        pub extern "C" fn _start() -> ! {
            $crate::__start();
            let code = $main($crate::Console::FIRST);
            $crate::exit(code)
        }

        #[panic_handler]
        fn panic(info: &core::panic::PanicInfo) -> ! {
            let _ = $crate::say!($crate::Console::FIRST, "panicked: {}", info.message());
            $crate::exit(101)
        }
    };
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::fmt::Write;

    #[test]
    fn answers_are_counts_or_errors() {
        assert_eq!(check(17), Ok(17));
        assert_eq!(check(-2), Err(Error::NoSuchHandle));
        assert_eq!(check(-3), Err(Error::BadPointer));
        assert_eq!(check(-99), Err(Error::Other(-99)));
    }

    #[test]
    fn line_formats_without_a_heap() {
        let n = 42;
        let l = line!("{n} left");
        assert_eq!(l.as_str(), "42 left");
    }

    #[test]
    fn a_long_line_is_cut_at_a_character() {
        let mut line = Line::new();
        let long = "é".repeat(200); // 400 bytes
        assert!(line.write_str(&long).is_err());
        assert_eq!(line.as_bytes().len(), SEND_MAX);
        assert!(core::str::from_utf8(line.as_bytes()).is_ok());
    }
}
