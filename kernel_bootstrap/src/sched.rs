//! Who runs next (PROC-001): the scheduler's decisions, and nothing else.
//!
//! The machine used to have one thread of control: the desk's main loop,
//! which did everything, one piece after another, and waited for each.
//! A slow piece -- a TLS handshake, a big page, a long calculation --
//! froze the desk while it ran. Threads fix that: the timer interrupts
//! whatever is running a hundred times a second, and this decides what
//! runs next.
//!
//! This is the policy, kept apart from the mechanism so it runs under
//! `cargo test`: a fixed table of threads (no allocation, since it is
//! called from the timer interrupt, where the heap's lock may be held by
//! the very thread that was interrupted), round robin among the ready,
//! sleepers woken by the clock, and the exited left for the kernel to
//! reap. The kernel's `threads` module saves and restores registers.
//!
//! Slot 0 is the thread the machine booted on -- the desk. It never
//! sleeps or exits, so there is always something to run.

/// Threads at most, the desk's included.
pub const MAX_THREADS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ready,
    Running,
    /// Until this tick.
    Sleeping(u64),
    /// Finished (or stopped); its stack is the kernel's to free.
    Exited,
}

#[derive(Debug, Clone, Copy)]
pub struct Thread {
    pub id: u32,
    pub name: &'static str,
    pub state: State,
    /// The stack pointer it was stopped at, its registers saved beneath.
    pub rsp: u64,
    /// Timer ticks it was running when they came.
    pub ticks: u64,
    /// Times it was switched to.
    pub runs: u64,
    /// Asked to stop: it finishes at its next look (`stop_asked`).
    /// Threads are stopped by asking, never from outside, because one
    /// stopped mid-step could be holding a lock -- the heap's -- forever.
    pub stop_asked: bool,
}

#[derive(Debug)]
pub struct Scheduler {
    slots: [Option<Thread>; MAX_THREADS],
    current: usize,
    next_id: u32,
    /// Switches from one thread to another.
    pub switches: u64,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    /// A scheduler whose only thread is the one running now, the desk.
    pub const fn new() -> Self {
        let mut slots = [None; MAX_THREADS];
        slots[0] = Some(Thread {
            id: 0,
            name: "desk",
            state: State::Running,
            rsp: 0,
            ticks: 0,
            runs: 1,
            stop_asked: false,
        });
        Self {
            slots,
            current: 0,
            next_id: 1,
            switches: 0,
        }
    }

    /// A new thread, ready, that will start from `rsp`. `None` when the
    /// table is full. Returns its slot and id.
    pub fn add(&mut self, name: &'static str, rsp: u64) -> Option<(usize, u32)> {
        let slot = self.slots.iter().position(Option::is_none)?;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.slots[slot] = Some(Thread {
            id,
            name,
            state: State::Ready,
            rsp,
            ticks: 0,
            runs: 0,
            stop_asked: false,
        });
        Some((slot, id))
    }

    /// The slot running now.
    pub fn current(&self) -> usize {
        self.current
    }

    /// Whether any thread but the running one could run.
    pub fn others_ready(&self, now: u64) -> bool {
        self.slots.iter().enumerate().any(|(i, t)| {
            i != self.current
                && t.is_some_and(|t| match t.state {
                    State::Ready => true,
                    State::Sleeping(until) => until <= now,
                    _ => false,
                })
        })
    }

    /// The running thread sleeps until tick `until` (the desk never does).
    pub fn sleep_current(&mut self, until: u64) {
        if self.current == 0 {
            return;
        }
        if let Some(t) = self.slots[self.current].as_mut() {
            t.state = State::Sleeping(until);
        }
    }

    /// The running thread is done (the desk never is).
    pub fn exit_current(&mut self) {
        self.stop(self.current);
    }

    /// Stop slot `slot`: exited, never run again (not the desk).
    pub fn stop(&mut self, slot: usize) {
        if slot == 0 {
            return;
        }
        if let Some(t) = self.slots.get_mut(slot).and_then(Option::as_mut) {
            t.state = State::Exited;
        }
    }

    /// Stop the thread with `id`; whether there was one.
    pub fn stop_id(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(slot) if slot != 0 => {
                self.stop(slot);
                true
            }
            _ => false,
        }
    }

    /// Ask the thread with `id` to stop; whether there was one to ask.
    pub fn ask_stop(&mut self, id: u32) -> bool {
        match self.slot_of(id) {
            Some(slot) if slot != 0 => {
                if let Some(t) = self.slots[slot].as_mut() {
                    t.stop_asked = true;
                }
                true
            }
            _ => false,
        }
    }

    /// Whether the running thread has been asked to stop.
    pub fn stop_asked(&self) -> bool {
        self.slots[self.current].is_some_and(|t| t.stop_asked)
    }

    pub fn slot_of(&self, id: u32) -> Option<usize> {
        self.slots
            .iter()
            .position(|t| t.is_some_and(|t| t.id == id))
    }

    /// Switch: the running thread stopped at `rsp` (a timer tick when
    /// `tick`, else it yielded); returns the stack pointer to resume.
    pub fn switch(&mut self, now: u64, rsp: u64, tick: bool) -> u64 {
        let current = self.current;
        if let Some(t) = self.slots[current].as_mut() {
            t.rsp = rsp;
            if tick {
                t.ticks += 1;
            }
            if t.state == State::Running {
                t.state = State::Ready;
            }
        }
        // Sleepers whose time has come.
        for t in self.slots.iter_mut().flatten() {
            if let State::Sleeping(until) = t.state {
                if until <= now {
                    t.state = State::Ready;
                }
            }
        }
        // Round robin: the next ready after the current, the current last.
        let next = (1..=MAX_THREADS)
            .map(|step| (current + step) % MAX_THREADS)
            .find(|&i| self.slots[i].is_some_and(|t| t.state == State::Ready))
            .unwrap_or(0);
        self.current = next;
        let t = self.slots[next].as_mut().expect("the desk is always there");
        if next != current {
            self.switches += 1;
            t.runs += 1;
        }
        // Only the desk can be chosen not ready (nothing else was, and it
        // is never asleep or finished); it runs.
        t.state = State::Running;
        t.rsp
    }

    /// Take the exited threads out of the table: their slots, for the
    /// kernel to free their stacks. Never the running one.
    pub fn reap(&mut self) -> ([Option<usize>; MAX_THREADS], usize) {
        let mut out = [None; MAX_THREADS];
        let mut n = 0;
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if i != self.current && slot.is_some_and(|t| t.state == State::Exited) {
                *slot = None;
                out[n] = Some(i);
                n += 1;
            }
        }
        (out, n)
    }

    /// The table, for `threads` in the Terminal.
    pub fn threads(&self) -> impl Iterator<Item = &Thread> {
        self.slots.iter().flatten()
    }
}

/// Lay out a new thread's first frame in `stack` (whose first word is at
/// address `base`), as the interrupt entry would have left a thread
/// stopped just before `entry(arg)`: fifteen registers to pop (r15
/// lowest, rax highest; rdi, the ninth from the bottom, is `arg`) and an
/// interrupt frame -- rip, cs, rflags, rsp, ss -- to `iretq` through.
/// Returns the stack pointer to hand the entry.
///
/// Above the frame is a zero return address, placed so the thread begins
/// with its stack 8 past a multiple of 16: aligned as though `entry` had
/// been called, which is what compiled code assumes.
pub fn initial_frame(stack: &mut [u64], base: u64, entry: u64, arg: u64, cs: u64, ss: u64) -> u64 {
    let mut top = stack.len() - 1;
    if (base + top as u64 * 8) % 16 != 8 {
        top -= 1;
    }
    stack[top] = 0;
    let entry_rsp = base + top as u64 * 8;
    let frame = top - 5;
    stack[frame] = entry;
    stack[frame + 1] = cs;
    stack[frame + 2] = 0x202; // interrupts on
    stack[frame + 3] = entry_rsp;
    stack[frame + 4] = ss;
    let regs = frame - 15;
    stack[regs..frame].fill(0);
    stack[regs + 8] = arg;
    base + regs as u64 * 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_one_thread_the_desk_runs_on() {
        let mut s = Scheduler::new();
        assert_eq!(s.switch(1, 0x1000, true), 0x1000);
        assert_eq!(s.current(), 0);
        assert_eq!(s.switches, 0);
        assert!(!s.others_ready(1));
    }

    #[test]
    fn ready_threads_take_turns_round_robin() {
        let mut s = Scheduler::new();
        let (a, _) = s.add("a", 0xA000).unwrap();
        let (b, _) = s.add("b", 0xB000).unwrap();
        assert!(s.others_ready(0));
        // The desk is interrupted: a runs, from its initial stack.
        assert_eq!(s.switch(1, 0x1000, true), 0xA000);
        assert_eq!(s.current(), a);
        // a is interrupted at 0xA100: b.
        assert_eq!(s.switch(2, 0xA100, true), 0xB000);
        assert_eq!(s.current(), b);
        // b: back to the desk, where it stopped.
        assert_eq!(s.switch(3, 0xB100, true), 0x1000);
        // And a resumes where it stopped.
        assert_eq!(s.switch(4, 0x1100, true), 0xA100);
        assert_eq!(s.switches, 4);
        let ticks: u64 = s.threads().map(|t| t.ticks).sum();
        assert_eq!(ticks, 4);
    }

    #[test]
    fn a_sleeper_waits_for_its_tick_and_the_desk_never_sleeps() {
        let mut s = Scheduler::new();
        let (a, _) = s.add("a", 0xA000).unwrap();
        s.switch(0, 0x1000, true); // a runs
        s.sleep_current(10);
        assert_eq!(s.switch(1, 0xA100, false), 0x1000, "a sleeps; the desk");
        assert!(!s.others_ready(5));
        assert_eq!(s.switch(5, 0x1100, true), 0x1100, "still asleep");
        assert!(s.others_ready(10));
        assert_eq!(s.switch(10, 0x1200, true), 0xA100, "awake");
        assert_eq!(s.current(), a);
        // The desk asked to sleep: ignored.
        let mut d = Scheduler::new();
        d.sleep_current(100);
        assert_eq!(d.switch(1, 0x1000, false), 0x1000);
    }

    #[test]
    fn an_exited_thread_is_never_run_again_and_is_reaped() {
        let mut s = Scheduler::new();
        let (a, id) = s.add("a", 0xA000).unwrap();
        s.switch(0, 0x1000, true);
        s.exit_current();
        // Not reaped while it is still the running one.
        assert_eq!(s.reap().1, 0);
        assert_eq!(s.switch(1, 0xA100, false), 0x1000);
        let (slots, n) = s.reap();
        assert_eq!((slots[0], n), (Some(a), 1));
        assert_eq!(s.slot_of(id), None);
        // Its slot is free again.
        assert_eq!(s.add("b", 0xB000).map(|(slot, _)| slot), Some(a));
        // The desk cannot be stopped.
        assert!(!s.stop_id(0));
    }

    #[test]
    fn the_table_is_bounded() {
        let mut s = Scheduler::new();
        for _ in 1..MAX_THREADS {
            assert!(s.add("t", 0x1).is_some());
        }
        assert_eq!(s.add("one too many", 0x1), None);
        // A stopped one is found by id.
        let id = s.threads().nth(3).unwrap().id;
        assert!(s.stop_id(id));
    }

    #[test]
    fn a_new_thread_starts_aligned_with_its_work_in_rdi() {
        for base in [0x10_0000u64, 0x10_0008] {
            let mut stack = [0xFFu64; 64];
            let rsp = initial_frame(&mut stack, base, 0xE000, 0xA46, 0x28, 0x30);
            let at = |addr: u64| ((addr - base) / 8) as usize;
            let regs = at(rsp);
            assert!(stack[regs..regs + 15]
                .iter()
                .enumerate()
                .all(|(i, &w)| { w == if i == 8 { 0xA46 } else { 0 } }));
            let frame = &stack[regs + 15..regs + 20];
            assert_eq!(frame[0], 0xE000, "rip");
            assert_eq!((frame[1], frame[2], frame[4]), (0x28, 0x202, 0x30));
            let entry_rsp = frame[3];
            assert_eq!(entry_rsp % 16, 8, "as though called");
            assert_eq!(stack[at(entry_rsp)], 0, "a zero return address");
            assert!(at(entry_rsp) < stack.len());
            // The frame sits wholly below the thread's first stack word.
            assert!(rsp + 20 * 8 <= entry_rsp);
        }
    }

    #[test]
    fn a_thread_asked_to_stop_sees_it_when_it_runs() {
        let mut s = Scheduler::new();
        let (_, id) = s.add("a", 0xA000).unwrap();
        assert!(s.ask_stop(id));
        assert!(!s.stop_asked(), "the desk was not asked");
        s.switch(1, 0x1000, true);
        assert!(s.stop_asked());
        assert!(!s.ask_stop(0) && !s.ask_stop(999));
    }
}
