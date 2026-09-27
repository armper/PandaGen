# Phase 439: supervision -- budgets, and restarts after a crash (PROC-007)

## What changed

Programs are supervised. Each runs within a memory limit and a share of
the processor. When the kernel ends a program with a card, its card
stays and the program is started again into it, up to three times a
minute. After that the card says it has given up and offers a button to
try once more. This is step 5, the last of the roadmap to programs that
run on their own: they are isolated (435), loaded from images (436),
have cards (437), include a real dock app (438), and are now looked
after (439).

### Budgets in the scheduler

- `sched::Budget { ticks, window }`: at most `ticks` timer ticks of
  running in every `window`. A thread that has used its share is
  **held back** (`State::Throttled`) until the window turns. Unlike a
  sleeper, no event wakes it early. The desk can never be given a
  budget, so it can always run.
- Every program gets 15 ticks in 20 (75%). A program that loops forever
  therefore can't take the processor from the desk, even when the desk
  is idle and yielding. `threads` shows each program's share and how
  often it was held back ("75% of the CPU at most (held back 3 times)"),
  and a program waiting for an event shows as `waiting` rather than as
  asleep for 2^64 ticks.
- **Found on the way:** the timer's quick path (nothing else ready)
  returned the next stack without switching address spaces. That was
  harmless while the quick path always resumed the same thread. Once a
  budget can move the processor from a program to the desk there, it
  would have left the desk running on the program's page tables, which
  are freed when the program is reaped. It now switches CR3 and `rsp0`
  whenever the running thread changes.

### The policy: `supervision.rs`

Plain code, tested on the host:

- **`Limits`**: 16 MiB of memory (image pieces and stack), checked by
  `run` before anything is mapped ("run: huge needs 20496 KiB; a program
  may have 16384 KiB"), and the CPU budget above.
- **`Crashes`**: a restart for each crash, up to three in a minute
  (`Decision::Restart(n)`); a fourth in the same minute gives up
  (`Decision::GiveUp`) until the person starts the program again, which
  forgives the past. A program that crashes once in a while is always
  restarted.

### Restarts into the same card

- `threads::reap` now reports which programs the kernel ended
  (`Gone::faulted`), as opposed to programs that exited or were stopped.
- For a program that faulted, the desk keeps its card, which waits
  (thread 0) and says why: "fragile: ended by the kernel: it touched 0x10,
  not its memory (at 0x400252). Starting it again (1 of 3)." The program
  is queued to run again, and `ProgramCard::bind` hands it the same card,
  blank, and tells it the canvas size afresh. A notice records each
  crash.
- At the fourth crash in a minute the card shows the reason, "It has been
  ended 4 times in a minute.", and a **Start it again** button (R, or
  Enter). Clicks and keys on a waiting card go to the card, not to a
  program that isn't there.
- A program that exits, or is stopped, takes its card with it, as before.

### `apps/fragile`

A card with one button, Break, which writes through a pointer into the
unmapped first 4 MiB. It shows supervision at work. It asks for nothing
but its card.

## Tests

- `sched`: a thread on a budget gets its share and no more (about 3 in
  10 over 100 ticks, held back every window); an event does not wake
  the held back; the desk cannot be given a budget.
- `supervision`: a program too big for its limit is refused before
  loading; three restarts a minute, then the person decides; a crash now
  and then is always restarted; a program never has all of the
  processor.
- `program_card`: a card whose program keeps crashing restarts it three
  times (each new program hears its size), gives up at the fourth,
  offers "Start it again" as the only thing to click, and is forgiven
  when the person uses it.
- `desk`: a program the kernel ends is restarted into its card with a
  notice; one that exits takes its card and leaves none.
- Gauntlet, "supervision: a crashing program restarted, then given up
  on": `run fragile`, B four times (threads 2, 3 and 4 are restarts; no
  fifth automatic one), R (thread 5), `threads` (fragile `waiting`, and
  the CPU share).
