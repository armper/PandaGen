//! Threads on the machine (PROC-001): the mechanism under `sched`.
//!
//! The timer interrupt's entry saves every general register on the
//! running thread's stack and hands the stack pointer here; what comes
//! back is the stack to resume, whose registers the entry pops before
//! `iretq`. Switching threads is choosing a different stack. A new
//! thread's stack is made to look like one that was interrupted just as
//! it was about to start: zeroed registers, `rdi` holding its work, and
//! an interrupt frame whose return address is `thread_start`.
//!
//! Only the boot CPU schedules; the others keep their own loops. The
//! table is touched with interrupts off (the timer interrupt is the only
//! other toucher, on the same CPU), and nothing is allocated or locked
//! while they are off: a thread stopped mid-allocation holds the heap's
//! lock until it runs again, and waiting for it with the timer masked
//! would wait forever. So stacks are made before a thread joins the
//! table and freed after it leaves.
//!
//! There is no floating-point state to save: the kernel is built
//! soft-float, so the general registers are the whole of a thread.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::asm;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::sched::{Scheduler, State, MAX_THREADS};

/// A thread's stack, in bytes.
pub const STACK_BYTES: usize = 64 * 1024;
/// The vector a thread gives up the processor with.
pub const YIELD_VECTOR: u8 = 0x81;
/// Written at a stack's lowest word: a thread that overwrote it ran off
/// the end of its stack (into whatever the heap put below).
const CANARY: u64 = 0x5041_4E44_4153_5441; // "PANDASTA"

struct Table {
    sched: Scheduler,
    stacks: [Option<Box<[u64]>>; MAX_THREADS],
}

/// The table, for the one CPU that schedules.
struct Cell(UnsafeCell<Table>);
// SAFETY: only the boot CPU touches it, and only with interrupts off.
unsafe impl Sync for Cell {}

const NO_STACK: Option<Box<[u64]>> = None;
static TABLE: Cell = Cell(UnsafeCell::new(Table {
    sched: Scheduler::new(),
    stacks: [NO_STACK; MAX_THREADS],
}));

/// Times a thread gave the processor up (rather than having it taken).
static YIELDS: AtomicU32 = AtomicU32::new(0);

/// Threads found to have run off their stacks, since boot.
static OVERRUNS: AtomicU32 = AtomicU32::new(0);

/// Lines threads leave for the Terminal (the desk prints them).
static NOTES: hal_x86_64::SpinLock<Vec<String>> = hal_x86_64::SpinLock::new(Vec::new());

/// Run `f` with the table and interrupts off.
fn with_table<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    let flags: u64;
    // SAFETY: reads RFLAGS and masks interrupts; restored below.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) flags) };
    // SAFETY: interrupts are off, and only this CPU schedules, so nothing
    // else holds a reference to the table.
    let r = f(unsafe { &mut *TABLE.0.get() });
    if flags & 0x200 != 0 {
        // SAFETY: they were on before.
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
    r
}

/// From the timer and yield interrupts, with interrupts off: the running
/// thread stopped at `rsp`; the stack to resume.
pub fn on_interrupt(rsp: u64, now: u64, tick: bool) -> u64 {
    // SAFETY: interrupts are off (an interrupt gate), on the boot CPU.
    let table = unsafe { &mut *TABLE.0.get() };
    let current = table.sched.current();
    if !tick {
        YIELDS.fetch_add(1, Ordering::Relaxed);
    }
    if tick && !table.sched.others_ready(now) {
        // The usual case: nothing else to run. Count the tick and go on.
        return table.sched.switch(now, rsp, tick);
    }
    if let Some(stack) = table.stacks[current].as_ref() {
        if stack[0] != CANARY {
            // Its stack is gone; so, now, is it. (Stopping it holding a
            // lock is the lesser harm: it would crash anyway.)
            OVERRUNS.fetch_add(1, Ordering::Relaxed);
            table.sched.stop(current);
        }
    }
    table.sched.switch(now, rsp, tick)
}

/// Give up the processor to whoever is ready (returns at once when no
/// one is).
pub fn yield_now() {
    // SAFETY: the yield vector's handler saves and restores everything;
    // `int` is a full barrier as far as the compiler is concerned.
    unsafe { asm!("int 0x81") };
}

/// Sleep `ticks` (a spawned thread; the desk only yields).
pub fn sleep(ticks: u64) {
    let now = crate::get_tick_count();
    with_table(|t| {
        t.sched.sleep_current(now + ticks);
        // Interrupts are off, so the timer cannot run us again before
        // we are asleep; `int` is not masked by them.
        yield_now();
    });
}

/// Whether some other thread is ready to run.
pub fn others_ready() -> bool {
    let now = crate::get_tick_count();
    with_table(|t| t.sched.others_ready(now))
}

/// Whether the running thread has been asked to stop.
pub fn stop_asked() -> bool {
    with_table(|t| t.sched.stop_asked())
}

/// Ask thread `id` to stop.
pub fn ask_stop(id: u32) -> bool {
    with_table(|t| t.sched.ask_stop(id))
}

/// Leave a line for the Terminal.
pub fn note(line: String) {
    NOTES.lock().push(line);
}

/// The lines left since last asked.
pub fn take_notes() -> Vec<String> {
    match NOTES.try_lock() {
        Some(mut notes) if !notes.is_empty() => core::mem::take(&mut *notes),
        _ => Vec::new(),
    }
}

/// Start a thread running `work`; its id, or why not.
pub fn spawn(
    name: &'static str,
    work: impl FnOnce() + Send + 'static,
) -> Result<u32, &'static str> {
    let work: Box<Box<dyn FnOnce() + Send>> = Box::new(Box::new(work));
    let arg = Box::into_raw(work) as u64;
    let mut stack = vec![0u64; STACK_BYTES / 8].into_boxed_slice();
    stack[0] = CANARY;
    let (cs, ss) = segments();
    let base = stack.as_ptr() as u64;
    let entry = thread_start as extern "C" fn(u64) -> ! as usize as u64;
    let rsp = crate::sched::initial_frame(&mut stack, base, entry, arg, cs, ss);
    let mut stack = Some(stack);
    let added = with_table(|t| {
        let (slot, id) = t.sched.add(name, rsp)?;
        t.stacks[slot] = stack.take();
        Some(id)
    });
    match added {
        Some(id) => Ok(id),
        None => {
            // SAFETY: never handed to a thread; take the work back.
            drop(unsafe { Box::from_raw(arg as *mut Box<dyn FnOnce() + Send>) });
            Err("threads: the table is full")
        }
    }
}

fn segments() -> (u64, u64) {
    let (cs, ss): (u16, u16);
    // SAFETY: reads segment registers.
    unsafe {
        asm!("mov {0:x}, cs", "mov {1:x}, ss", out(reg) cs, out(reg) ss,
             options(nomem, nostack, preserves_flags));
    }
    (cs as u64, ss as u64)
}

/// Where every thread begins: its work, then its end.
extern "C" fn thread_start(arg: u64) -> ! {
    // SAFETY: `spawn` made `arg` from exactly this type and hands it to
    // exactly one thread.
    let work = unsafe { Box::from_raw(arg as *mut Box<dyn FnOnce() + Send>) };
    work();
    exit()
}

/// The running thread is done. Its stack is freed by `reap`, later, by
/// the desk -- not here, since this is still running on it.
pub fn exit() -> ! {
    with_table(|t| {
        t.sched.exit_current();
        yield_now();
    });
    unreachable!("an exited thread was run again")
}

/// Free the stacks of threads that have finished. The desk calls this;
/// the frees happen with interrupts on.
pub fn reap() {
    let mut freed: [Option<Box<[u64]>>; MAX_THREADS] = [NO_STACK; MAX_THREADS];
    with_table(|t| {
        let (slots, n) = t.sched.reap();
        for (i, slot) in slots[..n].iter().flatten().enumerate() {
            freed[i] = t.stacks[*slot].take();
        }
    });
    drop(freed);
}

/// The table, as lines for `threads` in the Terminal.
pub fn listing() -> Vec<String> {
    let mut rows: [(u32, &'static str, State, u64, u64); MAX_THREADS] =
        [(0, "", State::Exited, 0, 0); MAX_THREADS];
    let (n, switches) = with_table(|t| {
        let mut n = 0;
        for thread in t.sched.threads() {
            rows[n] = (
                thread.id,
                thread.name,
                thread.state,
                thread.ticks,
                thread.runs,
            );
            n += 1;
        }
        (n, t.sched.switches)
    });
    let now = crate::get_tick_count();
    let mut out = Vec::with_capacity(n + 2);
    out.push(alloc::format!(
        "threads: {n}, {switches} switches ({} given up), stacks {} KiB each",
        YIELDS.load(Ordering::Relaxed),
        STACK_BYTES / 1024
    ));
    for (id, name, state, ticks, runs) in &rows[..n] {
        let state = match state {
            State::Running => String::from("running"),
            State::Ready => String::from("ready"),
            State::Sleeping(until) => {
                alloc::format!("asleep {} ms", until.saturating_sub(now) * 10)
            }
            State::Exited => String::from("finished"),
        };
        out.push(alloc::format!(
            "  {id:>3} {name:<10} {state:<14} {} ms of CPU, run {runs} times",
            ticks * 10
        ));
    }
    let overruns = OVERRUNS.load(Ordering::Relaxed);
    if overruns > 0 {
        out.push(alloc::format!(
            "threads: {overruns} stopped for running off their stacks"
        ));
    }
    out
}

/// `after <seconds> <words>`: the words, that much later -- a thread
/// that spends its life asleep.
pub fn start_after(seconds: u64, words: String) -> Result<u32, &'static str> {
    spawn("after", move || {
        sleep(seconds * 100);
        note(alloc::format!("after {seconds} s: {words}"));
    })
}

/// `spin <seconds>`: count primes, flat out, for that long -- a thread
/// that never waits, to show the desk carries on around it.
pub fn start_spin(seconds: u64) -> Result<u32, &'static str> {
    spawn("spin", move || {
        let start = crate::get_tick_count();
        let until = start + seconds * 100;
        let (mut n, mut primes) = (2u64, 0u64);
        loop {
            // A slice of work between looks at the clock.
            for _ in 0..2000 {
                if is_prime(n) {
                    primes += 1;
                }
                n += 1;
            }
            let now = crate::get_tick_count();
            if now >= until {
                note(alloc::format!(
                    "spin: done, {primes} primes below {n} in {} s",
                    seconds
                ));
                break;
            }
            if stop_asked() {
                note(alloc::format!("spin: stopped, {primes} primes below {n}"));
                break;
            }
        }
    })
}

fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    let mut d = 2;
    while d * d <= n {
        if n % d == 0 {
            return false;
        }
        d += 1;
    }
    true
}
