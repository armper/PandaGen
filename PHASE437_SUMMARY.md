# Phase 437: a program's card -- drawn from ring 3, clicked and typed into (PROC-005)

## What changed

A program can have a card on the desk. `tally`, a tally counter written
in Rust against `pandagen_app`, runs in ring 3 in its own address space,
describes its card (the count, large, and three buttons), and the desk
draws it in the desk's theme. Clicks on its buttons and keys typed into
it reach the program, and closing the card tells the program, which says
goodbye and exits. This is step 4 of the road to programs that run on
their own: the first app that lives outside the kernel and still has a
place on the desk.

### `app_protocol`: views out, events in

- **A program never draws pixels.** It describes a view: a title, a
  footer, and at most 256 widgets (fills, outlines, text, right-aligned
  text, buttons) in at most 8 KiB. It writes the view into a fixed
  buffer with `ViewWriter`, so no heap is needed.
- **Colours are roles** (surface, raised, text, muted, accent, on-accent,
  hairline), not values. Every program follows the theme, and none can
  dress up as the desk's own chrome.
- **A button stands for a key.** A click on it arrives as that key,
  exactly as if typed, so a program works with the mouse alone and the
  keyboard alone without doing anything for either.
- **Events** are eight bytes each: `Key`, `Size` (sent before any key, and
  again whenever the card changes size), and `Closed`.
- **The desk's side (`decode`) checks everything.** Every widget must be
  inside the canvas, every text valid UTF-8, short, and free of control
  characters, and nothing may trail. Anything else is refused, never half
  drawn. Truncations and byte flips are tested.

### The kernel

- A new capability, **Card**, which an image asks for (`Ask::Card`).
  `run` opens the card when it starts such a program:
  `run: tally is thread 1, in ring 3, with a card on the desk`.
- Three calls: `present(card, ptr, len)` hands the desk a view (copied
  after the same page-by-page check as `send`; only the latest view per
  program is kept); `poll(card, ptr)` writes the next event into the
  program's memory; `wait(ms)` sleeps until an event comes or the time
  is up. A waiting program costs nothing, and an event wakes it
  (`Scheduler::wake`).
- Each program has a fixed ring of 32 events, filled with interrupts off,
  so it allocates nothing.
- When a card closes, its program is told and has two seconds to finish
  before it is stopped. When a program ends, its card closes. If the
  kernel ended it, the desk leaves a notice saying why.

### The desk

- `ProgramCard` (`program_card.rs`) draws a program's view with the
  desk's widgets, maps roles to the theme's colours, turns clicks into
  the buttons' keys, keeps the last good view when a new one is refused
  (the footer says why), and tells the program its size once per change.
- `DeskApp::Program` is not on the dock and not restored after a reboot.
  The bar and the card's closing animation use the program's name.
- Ctrl+W closes a program's card like any other. Every other key belongs
  to the program.

### The SDK and the program

- `pandagen_app::Card`: `present`, `poll`, `next_event` (sleeps until
  there is one) and `event_within(ms)`. `line!` formats into a fixed
  buffer.
- `apps/tally` asks for the console (handle 0) and a card (handle 1).

## Tests

- `app_protocol`: events round-trip; a full buffer, too many widgets
  and too-long text are errors, never cut views; views round-trip;
  widgets outside the card are refused; damaged views are refused at
  every truncation and survive byte flips; control characters are
  refused.
- `program_card`: a click on a program's button is its key; a view that
  doesn't fit is refused and the last good one stays; the size is sent
  once per change; roles follow the theme.
- `desk`: a program's card carries its views and keys, sends its size
  first, isn't restored, and closing it tells the program. A program the
  kernel ends takes its card and leaves a notice; one that exits quietly
  leaves none.
- `sched`: a sleeper is woken early by an event. `syscall_abi`: card
  calls need a card.
- Gauntlet, "a program's card: drawn from ring 3, clicked and typed
  into": `run tally`, two real mouse clicks on its + and a Space (count
  3), then Ctrl+W; the program says `counted to 3; goodbye` and exits
  with 0.
