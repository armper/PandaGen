# Phase 434: threads -- the machine does two things at once (PROC-001)

## What changed

The bare-metal kernel preempts. Until now the desk's main loop was the
machine's only thread of control: everything it did, it did one piece
after another, and a slow piece froze everything else. Now the timer
takes the processor back a hundred times a second and gives it to
whichever thread is next.

This is step 1 of the road to programs that run on their own (user mode
and address spaces, a program format and loader, an app-to-desk protocol,
supervision): none of those can exist until the kernel can run more than
one thing.

### The mechanism

- **The timer's entry switches stacks.** `irq_timer_entry` already saved
  all fifteen general registers on the interrupted stack; it now passes
  that stack pointer to `timer_irq_handler` and resumes whatever stack
  comes back. A switch is choosing a different stack; the interrupt
  frame under the saved registers (rip, cs, rflags, rsp, ss) takes
  `iretq` back to where that thread was.
- **Yielding** is `int 0x81`, whose entry saves and restores the same
  way, so a thread stopped either way can be resumed by either.
- **A new thread** gets a 64 KiB stack laid out as though it had been
  interrupted just before starting: zeroed registers with its work in
  `rdi`, an interrupt frame whose return address is `thread_start`, and
  a zero return address above, placed so it starts with the stack
  aligned as though called.
- **No floating-point state** to save: the kernel is soft-float, so the
  general registers are the whole of a thread.
- **Stack overruns** are caught, after the fact, by a canary at each
  stack's lowest word, checked at every switch; a thread that ran off its
  stack is stopped and counted.
- Only the boot CPU schedules. The other CPUs keep their own loops.

### The policy: `sched.rs`

A fixed table of 16 threads -- fixed because the timer interrupt consults
it, and the interrupt must never allocate. Round robin among the ready;
sleepers woken by the tick; finished threads left for the desk to reap.
Slot 0 is the thread the machine booted on, the desk: it never sleeps or
finishes, so there is always something to run. It is a plain struct with
no machine in it, tested under `cargo test`.

### Two rules that keep it from deadlocking

- **Nothing is allocated or locked with interrupts off.** The spin locks
  do not mask interrupts, so a thread can be preempted holding the heap's
  lock. Anyone else who wants it spins until the timer lets the holder
  finish -- slow, but it ends. With interrupts off it would never end. So
  a thread's stack is made before it joins the table and freed (by the
  desk, `threads::reap`) after it leaves, and the table itself is only
  touched with interrupts off.
- **Threads are stopped by asking, never from outside.** A thread stopped
  mid-step could be holding that same lock forever. `stop <id>` sets a
  flag the thread looks at (`threads::stop_asked`).

### The desk

The desk loop is thread 0. When it has nothing to do it yields to a
waiting thread instead of spending the rest of its slice pausing, and
each pass it reaps finished threads and prints what they left for the
Terminal (`threads::note`).

### In the Terminal

- `threads` -- the table: each thread's state and CPU time, and the
  switch count.
- `spin [secs]` -- a thread that counts primes flat out and never waits.
- `after <secs> [words]` -- a thread that sleeps, then says the words.
- `stop <id>` -- ask a thread to stop.

## Tests

- `sched`: round robin, sleep and wake, the desk never sleeping or
  finishing, reaping and slot reuse, the bounded table, stopping by
  asking, and the first frame's layout and alignment.
- Gauntlet, "threads: the desk answers while a thread spins; sleep,
  stop": `spin 60`, `after 1 hello`, `threads` (listed while the spinner
  is still counting -- impossible if the timer did not take the
  processor back), `stop 1`; the spinner reports it stopped and never
  that it finished.
- Verified in QEMU: with a spinner running the desk kept drawing and
  answering, and the two shared the processor turn about (about 5.7 s of
  CPU each over 11.5 s).

## Found on the way

Measuring why the spinner got only half the processor with nobody
typing: the desk is never idle under QEMU's emulated CPU while a
Terminal is open. Each frame takes about half a second to draw there, and
the caret's blink asks for one twice a second, so the desk spends nearly
all its time drawing. Round robin rightly splits the processor. A
blink should redraw the caret, not the frame -- a job for a later phase.
(`threads` now reports how many switches were a thread giving the
processor up rather than having it taken, which is how this showed.)

## Next

Step 2: user mode -- a ring-3 program in its own address space, reaching
the kernel only through capability-checked system calls.
