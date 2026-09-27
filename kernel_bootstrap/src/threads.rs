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
//!
//! A thread can be a program (PROC-002): it runs in ring 3 in its own
//! address space, and its first frame drops there. Switching to one also
//! switches CR3 and points the TSS's `rsp0` at the top of its kernel
//! stack, where the CPU puts the frame when it is interrupted or calls.
//! A program is ended from outside at once when it is between its own
//! instructions -- it holds nothing of the kernel's there -- and
//! otherwise at its next call or tick in ring 3.

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::asm;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

use hal_x86_64::user_space::UserSpace;

use crate::sched::{Scheduler, State, MAX_THREADS};
use crate::syscall_abi::{Call, End, Error, Handles, SEND_MAX};
use app_protocol::{EVENT_BYTES, VIEW_MAX};

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
    /// A program's kernel-stack top (its `rsp0`), by slot.
    tops: [u64; MAX_THREADS],
    programs: [Option<Program>; MAX_THREADS],
    /// The kernel's own CR3, for its threads; 0 until `init`.
    kernel_cr3: u64,
    hhdm: u64,
    nx: bool,
}

/// The table, for the one CPU that schedules.
struct Cell(UnsafeCell<Table>);
// SAFETY: only the boot CPU touches it, and only with interrupts off.
unsafe impl Sync for Cell {}

const NO_STACK: Option<Box<[u64]>> = None;
const NO_PROGRAM: Option<Program> = None;
static TABLE: Cell = Cell(UnsafeCell::new(Table {
    sched: Scheduler::new(),
    stacks: [NO_STACK; MAX_THREADS],
    tops: [0; MAX_THREADS],
    programs: [NO_PROGRAM; MAX_THREADS],
    kernel_cr3: 0,
    hhdm: 0,
    nx: false,
}));

/// Note the kernel's address space and the direct map, before any
/// program runs.
pub fn init(hhdm: u64) {
    let efer: u64;
    // SAFETY: reads EFER; bit 11 is no-execute enable.
    unsafe {
        let (lo, hi): (u32, u32);
        asm!("rdmsr", in("ecx") 0xC000_0080u32, out("eax") lo, out("edx") hi,
             options(nomem, nostack, preserves_flags));
        efer = (hi as u64) << 32 | lo as u64;
    }
    let cr3 = read_cr3();
    with_table(|t| {
        t.kernel_cr3 = cr3;
        t.hhdm = hhdm;
        t.nx = efer & (1 << 11) != 0;
    });
}

/// The kernel's CR3, whose upper half every program shares.
pub fn kernel_cr3() -> u64 {
    with_table(|t| t.kernel_cr3)
}

/// Whether pages can be marked no-execute.
pub fn nx() -> bool {
    with_table(|t| t.nx)
}

/// Times a thread gave the processor up (rather than having it taken).
static YIELDS: AtomicU32 = AtomicU32::new(0);

/// Threads found to have run off their stacks, since boot.
static OVERRUNS: AtomicU32 = AtomicU32::new(0);

/// What programs asked the desk to say as notices (PROC-008).
static NOTICES: hal_x86_64::SpinLock<Vec<String>> = hal_x86_64::SpinLock::new(Vec::new());

/// The notices programs sent since last asked.
pub fn take_notices() -> Vec<String> {
    match NOTICES.try_lock() {
        Some(mut notices) if !notices.is_empty() => core::mem::take(&mut *notices),
        _ => Vec::new(),
    }
}

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
    if table.programs[current].is_some() && table.sched.stop_asked() && frame_is_user(rsp) {
        return end_current(table, rsp, now, End::Stopped);
    }
    if tick && !table.sched.others_ready(now) {
        // The usual case: nothing else to run. Count the tick and go on
        // -- unless that tick used up a program's budget (PROC-007) and
        // the desk runs instead, in its own address space.
        let next = table.sched.switch(now, rsp, tick);
        if table.sched.current() != current {
            enter(table, table.sched.current());
        }
        return next;
    }
    if let Some(stack) = table.stacks[current].as_ref() {
        if stack[0] != CANARY {
            // Its stack is gone; so, now, is it. (Stopping it holding a
            // lock is the lesser harm: it would crash anyway.)
            OVERRUNS.fetch_add(1, Ordering::Relaxed);
            table.sched.stop(current);
        }
    }
    let next = table.sched.switch(now, rsp, tick);
    enter(table, table.sched.current());
    next
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

/// Ask thread `id` to stop. A program between its own instructions is
/// ended there and then.
pub fn ask_stop(id: u32) -> bool {
    with_table(|t| {
        if !t.sched.ask_stop(id) {
            return false;
        }
        if let Some(slot) = t.sched.slot_of(id) {
            let parked = t.sched.saved_rsp(slot).is_some_and(frame_is_user);
            if parked {
                if let Some(program) = t.programs[slot].as_mut() {
                    program.end = Some(End::Stopped);
                    t.sched.stop(slot);
                }
            }
        }
        true
    })
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

/// A program that has ended.
pub struct Gone {
    pub id: u32,
    /// How, as the Terminal says it: "tally: exited with 0".
    pub how: String,
    /// The kernel ended it for breaking a rule.
    pub faulted: bool,
}

/// Free the stacks of threads that have finished, and hand `free` the
/// address spaces of programs that have, saying how each ended. The
/// desk calls this; the frees happen with interrupts on. Returns the
/// programs that ended, with how, for their cards.
pub fn reap(mut free: impl FnMut(UserSpace)) -> Vec<Gone> {
    let mut freed: [Option<Box<[u64]>>; MAX_THREADS] = [NO_STACK; MAX_THREADS];
    let mut ended: [Option<Program>; MAX_THREADS] = [NO_PROGRAM; MAX_THREADS];
    with_table(|t| {
        let (slots, n) = t.sched.reap();
        for (i, slot) in slots[..n].iter().flatten().enumerate() {
            freed[i] = t.stacks[*slot].take();
            ended[i] = t.programs[*slot].take();
            t.tops[*slot] = 0;
        }
    });
    drop(freed);
    let mut gone = Vec::new();
    for program in ended.into_iter().flatten() {
        let end = program.end.unwrap_or(End::Stopped);
        let mut why = String::new();
        let _ = end.describe(&mut why);
        let line = alloc::format!("{}: {why}", program.name);
        note(line.clone());
        free(program.space);
        gone.push(Gone {
            id: program.id,
            how: line,
            faulted: matches!(end, End::Fault { .. }),
        });
    }
    gone
}

/// The table, as lines for `threads` in the Terminal.
pub fn listing() -> Vec<String> {
    let mut rows: [(Option<crate::sched::Thread>, usize); MAX_THREADS] = [(None, 0); MAX_THREADS];
    let (n, switches) = with_table(|t| {
        let mut n = 0;
        for thread in t.sched.threads() {
            let frames = t
                .sched
                .slot_of(thread.id)
                .and_then(|slot| t.programs[slot].as_ref().map(|p| p.space.frames()));
            rows[n] = (Some(*thread), frames.unwrap_or(0));
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
    for (thread, frames) in &rows[..n] {
        let Some(thread) = thread else { continue };
        let state = match thread.state {
            State::Running => String::from("running"),
            State::Ready => String::from("ready"),
            // `wait(0)`: asleep until an event, however long.
            State::Sleeping(u64::MAX) => String::from("waiting"),
            State::Sleeping(until) => {
                alloc::format!("asleep {} ms", until.saturating_sub(now).saturating_mul(10))
            }
            State::Throttled(until) => {
                alloc::format!(
                    "held back {} ms",
                    until.saturating_sub(now).saturating_mul(10)
                )
            }
            State::Exited => String::from("finished"),
        };
        let program = if *frames > 0 {
            alloc::format!(", a program in {} KiB of its own", frames * 4)
        } else {
            String::new()
        };
        // Its share of the processor (PROC-007), and how often it hit it.
        let budget = match thread.budget {
            Some(b) => alloc::format!(
                ", {}% of the CPU at most (held back {} times)",
                b.ticks * 100 / b.window,
                thread.throttled
            ),
            None => String::new(),
        };
        out.push(alloc::format!(
            "  {:>3} {:<10} {state:<14} {} ms of CPU, run {} times{program}{budget}",
            thread.id,
            thread.name,
            thread.ticks * 10,
            thread.runs
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

// ---- Programs (PROC-002): threads that run in ring 3 ----

/// A thread that is a program: its address space, what it holds, and
/// (once it has ended) why.
struct Program {
    id: u32,
    name: &'static str,
    space: UserSpace,
    handles: Handles,
    end: Option<End>,
    /// Events for its card, oldest first (PROC-005): a fixed ring, since
    /// they are put here with interrupts off.
    events: [[u8; EVENT_BYTES]; EVENTS_MAX],
    events_head: usize,
    events_len: usize,
    /// Asleep in `wait`, to be woken by the next event.
    waiting: bool,
}

/// Events a program's card holds for it before the oldest are dropped.
const EVENTS_MAX: usize = 32;

impl Program {
    fn push_event(&mut self, event: [u8; EVENT_BYTES]) -> bool {
        if self.events_len == EVENTS_MAX {
            return false;
        }
        self.events[(self.events_head + self.events_len) % EVENTS_MAX] = event;
        self.events_len += 1;
        true
    }

    fn peek_event(&self) -> Option<[u8; EVENT_BYTES]> {
        (self.events_len > 0).then(|| self.events[self.events_head])
    }

    fn pop_event(&mut self) {
        if self.events_len > 0 {
            self.events_head = (self.events_head + 1) % EVENTS_MAX;
            self.events_len -= 1;
        }
    }
}

/// Views programs have presented, for the desk to take: the latest per
/// program (an older one not yet drawn is never worth drawing).
static VIEWS: hal_x86_64::SpinLock<Vec<(u32, Vec<u8>)>> = hal_x86_64::SpinLock::new(Vec::new());

/// The views presented since last asked, per program.
pub fn take_views() -> Vec<(u32, Vec<u8>)> {
    match VIEWS.try_lock() {
        Some(mut views) if !views.is_empty() => core::mem::take(&mut *views),
        _ => Vec::new(),
    }
}

/// An event for program `id`'s card; wakes it if it is waiting for one.
/// Whether it was taken (not when the program is gone or its ring full).
pub fn post_event(id: u32, event: [u8; EVENT_BYTES]) -> bool {
    with_table(|t| {
        let Some(slot) = t.sched.slot_of(id) else {
            return false;
        };
        let Some(program) = t.programs[slot].as_mut() else {
            return false;
        };
        let taken = program.push_event(event);
        if program.waiting {
            program.waiting = false;
            t.sched.wake(slot);
        }
        taken
    })
}

/// The saved frame at `rsp` was interrupted in ring 3: the program was
/// between its own instructions, holding nothing of the kernel's.
fn frame_is_user(rsp: u64) -> bool {
    // SAFETY: `rsp` is a thread's saved stack: fifteen registers, then
    // rip and cs.
    let cs = unsafe { *((rsp + 16 * 8) as *const u64) };
    cs & 3 == 3
}

fn read_cr3() -> u64 {
    let cr3: u64;
    // SAFETY: reads a control register.
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    cr3
}

/// Make slot `slot` the one the processor runs as: its address space,
/// and, for a program, the stack the CPU takes on the way into the
/// kernel.
fn enter(t: &mut Table, slot: usize) {
    let cr3 = match &t.programs[slot] {
        Some(program) => {
            crate::set_kernel_entry_stack(t.tops[slot]);
            program.space.cr3()
        }
        None => t.kernel_cr3,
    };
    if cr3 != 0 && read_cr3() != cr3 {
        // SAFETY: every space shares the kernel's half, where this code
        // and its stack are.
        unsafe { asm!("mov cr3, {}", in(reg) cr3, options(nostack, preserves_flags)) };
    }
}

/// End the running program (or thread) for `why`, and switch away.
fn end_current(t: &mut Table, rsp: u64, now: u64, why: End) -> u64 {
    let current = t.sched.current();
    if let Some(program) = t.programs[current].as_mut() {
        program.end = Some(why);
    }
    t.sched.stop(current);
    let next = t.sched.switch(now, rsp, false);
    enter(t, t.sched.current());
    next
}

/// A CPU exception in ring 3 (with interrupts off): the program ends,
/// the machine carries on. `None` when the running thread is not a
/// program -- a fault in the kernel, which is fatal.
pub fn on_user_fault(rsp: u64, vector: u64, address: u64, rip: u64) -> Option<u64> {
    // SAFETY: interrupts are off (an interrupt gate), on the boot CPU.
    let t = unsafe { &mut *TABLE.0.get() };
    t.programs[t.sched.current()].as_ref()?;
    let now = crate::get_tick_count();
    Some(end_current(
        t,
        rsp,
        now,
        End::Fault {
            vector,
            address,
            rip,
        },
    ))
}

/// A program's system call (`int 0x80`, interrupts off): the answer goes
/// in its `rax`; the stack to resume comes back -- its own, or another
/// thread's when the call waits or ends it.
pub fn on_syscall(rsp: u64) -> u64 {
    // The registers as the entry saved them: r15 lowest ... rax highest.
    let regs = rsp as *mut u64;
    // SAFETY: the entry just pushed them.
    let reg = |i: usize| unsafe { *regs.add(i) };
    let set_rax = |value: u64| unsafe { *regs.add(14) = value };
    let (rax, rdi, rsi, rdx) = (reg(14), reg(8), reg(9), reg(12));
    let now = crate::get_tick_count();
    // SAFETY: interrupts are off, on the boot CPU.
    let t = unsafe { &mut *TABLE.0.get() };
    let current = t.sched.current();
    if t.programs[current].is_none() {
        set_rax(Error::NoSuchCall.to_rax());
        return rsp;
    }
    if t.sched.stop_asked() {
        return end_current(t, rsp, now, End::Stopped);
    }
    let call = match Call::decode(rax, rdi, rsi, rdx) {
        Ok(call) => call,
        Err(e) => {
            set_rax(e.to_rax());
            return rsp;
        }
    };
    match call {
        Call::Exit { code } => end_current(t, rsp, now, End::Exited(code)),
        Call::Yield => {
            set_rax(0);
            let next = t.sched.switch(now, rsp, false);
            enter(t, t.sched.current());
            next
        }
        Call::Sleep { ms } => {
            set_rax(0);
            t.sched.sleep_current(now + ms.div_ceil(10).max(1));
            let next = t.sched.switch(now, rsp, false);
            enter(t, t.sched.current());
            next
        }
        Call::Time => {
            set_rax(now * 10);
            rsp
        }
        Call::Drop { handle } => {
            let program = t.programs[current].as_mut().expect("checked above");
            set_rax(match program.handles.drop_handle(handle) {
                Ok(()) => 0,
                Err(e) => e.to_rax(),
            });
            rsp
        }
        Call::Present { handle, ptr, len } => {
            let program = t.programs[current].as_ref().expect("checked above");
            if let Err(e) = program.handles.check_card(handle, len) {
                set_rax(e.to_rax());
                return rsp;
            }
            let mut buffer = [0u8; VIEW_MAX];
            let len = len as usize;
            let memory = crate::programs::Direct::new(t.hhdm);
            if !program.space.read_into(&memory, ptr, &mut buffer[..len]) {
                set_rax(Error::BadPointer.to_rax());
                return rsp;
            }
            let id = program.id;
            // Keeping it takes the heap: interrupts on, as for `send`.
            // SAFETY: nothing of the table is held across this.
            unsafe { asm!("sti", options(nomem, nostack)) };
            {
                let bytes = buffer[..len].to_vec();
                let mut views = VIEWS.lock();
                views.retain(|(of, _)| *of != id);
                views.push((id, bytes));
            }
            unsafe { asm!("cli", options(nomem, nostack)) };
            set_rax(0);
            rsp
        }
        Call::Poll { handle, ptr } => {
            let hhdm = t.hhdm;
            let program = t.programs[current].as_mut().expect("checked above");
            if let Err(e) = program.handles.check_card(handle, 0) {
                set_rax(e.to_rax());
                return rsp;
            }
            let Some(event) = program.peek_event() else {
                set_rax(0);
                return rsp;
            };
            let mut memory = crate::programs::Direct::new(hhdm);
            if !program.space.write(&mut memory, ptr, &event) {
                set_rax(Error::BadPointer.to_rax());
                return rsp;
            }
            program.pop_event();
            set_rax(1);
            rsp
        }
        Call::Wait { ms } => {
            set_rax(0);
            let program = t.programs[current].as_mut().expect("checked above");
            if program.events_len > 0 {
                return rsp;
            }
            program.waiting = true;
            let until = if ms == 0 {
                u64::MAX
            } else {
                now + ms.div_ceil(10).max(1)
            };
            t.sched.sleep_current(until);
            let next = t.sched.switch(now, rsp, false);
            enter(t, t.sched.current());
            next
        }
        Call::Random { ptr, len } => {
            if len > SEND_MAX as u64 {
                set_rax(Error::TooBig.to_rax());
                return rsp;
            }
            let len = len as usize;
            let mut bytes = [0u8; SEND_MAX];
            // The generator's lock may be held by a preempted thread:
            // interrupts on while it is taken, as for `send`.
            // SAFETY: nothing of the table is held across this.
            unsafe { asm!("sti", options(nomem, nostack)) };
            #[cfg(target_os = "none")]
            crate::random::fill(&mut bytes[..len]);
            // A host build has no generator; the bytes stay zero.
            #[cfg(not(target_os = "none"))]
            bytes[..len].fill(0);
            unsafe { asm!("cli", options(nomem, nostack)) };
            // SAFETY: interrupts are off again, on the boot CPU.
            let t = unsafe { &mut *TABLE.0.get() };
            let hhdm = t.hhdm;
            let program = t.programs[current].as_mut().expect("checked above");
            let mut memory = crate::programs::Direct::new(hhdm);
            set_rax(if program.space.write(&mut memory, ptr, &bytes[..len]) {
                0
            } else {
                Error::BadPointer.to_rax()
            });
            rsp
        }
        Call::Send { handle, ptr, len } => {
            let program = t.programs[current].as_ref().expect("checked above");
            let cap = match program.handles.check_send(handle, len) {
                Ok(cap) => cap,
                Err(e) => {
                    set_rax(e.to_rax());
                    return rsp;
                }
            };
            let mut buffer = [0u8; SEND_MAX];
            let len = len as usize;
            let memory = crate::programs::Direct::new(t.hhdm);
            if !program.space.read_into(&memory, ptr, &mut buffer[..len]) {
                set_rax(Error::BadPointer.to_rax());
                return rsp;
            }
            let name = t.sched.running_name();
            // Saying it takes the heap, which a preempted thread may hold:
            // so with interrupts on, like any thread. The program is in
            // the kernel on its own stack until this returns.
            // SAFETY: nothing of the table is held across this.
            unsafe { asm!("sti", options(nomem, nostack)) };
            let text = String::from_utf8_lossy(&buffer[..len]);
            match cap {
                // Notices (PROC-008): for the desk to say, with a chime.
                crate::syscall_abi::Capability::Notices => {
                    NOTICES.lock().push(String::from(text.trim_end()));
                }
                _ => note(alloc::format!("{name}: {}", text.trim_end())),
            }
            unsafe { asm!("cli", options(nomem, nostack)) };
            set_rax(len as u64);
            rsp
        }
    }
}

/// Start a program: a thread whose first frame drops to ring 3 at
/// `entry` with its stack at `user_rsp`, in `space`, holding `handles`.
/// On failure the space comes back, to be freed.
pub fn spawn_program(
    name: &'static str,
    space: UserSpace,
    handles: Handles,
    entry: u64,
    user_rsp: u64,
    budget: crate::sched::Budget,
) -> Result<u32, (&'static str, UserSpace)> {
    let mut stack = vec![0u64; STACK_BYTES / 8].into_boxed_slice();
    stack[0] = CANARY;
    let base = stack.as_ptr() as u64;
    let (rsp, top) = crate::sched::user_frame(
        &mut stack,
        base,
        entry,
        user_rsp,
        hal_x86_64::USER_CODE_SELECTOR as u64,
        hal_x86_64::USER_DATA_SELECTOR as u64,
    );
    let mut parts = Some((stack, space));
    let added = with_table(|t| {
        let (slot, id) = t.sched.add(name, rsp)?;
        t.sched.set_budget(slot, budget);
        let (stack, space) = parts.take().expect("once");
        let program_id = id;
        t.stacks[slot] = Some(stack);
        t.tops[slot] = top;
        t.programs[slot] = Some(Program {
            id: program_id,
            events: [[0; EVENT_BYTES]; EVENTS_MAX],
            events_head: 0,
            events_len: 0,
            waiting: false,
            name,
            space,
            handles,
            end: None,
        });
        Some(id)
    });
    match (added, parts) {
        (Some(id), _) => Ok(id),
        (None, Some((_, space))) => Err(("threads: the table is full", space)),
        (None, None) => unreachable!("taken only when added"),
    }
}
