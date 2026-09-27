# Phase 440: the Timer and Tiles move out of the kernel (PROC-008)

## What changed

Two more dock apps are programs. The **Timer** and **Tiles** run in ring
3 with cards of their own, drawing the same cards the kernel used to
(the existing gauntlet pixel checks pass unchanged). The kernel no
longer contains either: `kernel_bootstrap/src/timer.rs` and `game.rs`
are gone, along with the desk's timer ticking and redraw code.

### What they needed

- **Notices, a capability.** A Timer's countdown that is up has always
  been said in the desk's notices centre, with a chime, so the person
  hears it from any space. A program now asks for that
  (`Ask::Notices`): `send` on a Notices handle becomes a desk notice with
  the chime, queued first so it does not also blip. The Timer asks for
  its card and notices; nothing else.
- **A clock and a heartbeat.** Neither is new: `time_ms` gives the time,
  and `wait` with a timeout (`Card::event_within(100)`) wakes a running
  Timer ten times a second to move its display on. A stopped Timer
  sleeps until something happens and costs nothing.
- **Randomness.** A new call, `random(ptr, len)` (at most 256 bytes),
  fills a program's buffer from the machine's generator. It needs no
  capability, because it reveals nothing and reaches nothing. Tiles seeds
  each game from it. The generator's lock may be held by a preempted
  thread, so it is taken with interrupts on, as `send`'s allocation is.
- **Colours by value.** Tiles' tiles are the classic 2048 colours, which
  the theme has no roles for. `Role::Rgb(r, g, b)` says a colour by value
  (encoded as `0xFF r g b`). It is drawn only inside the card, so it
  cannot dress up as the desk's own chrome.
- **Centred text** (`Op::TextCentered`): a tile's number, the Timer's
  time.

### `timer_core` and `tiles_core`

The Timer's stopwatch, laps, countdowns and card, and Tiles' game and
card, moved from the kernel with their tests and tested on the host. The
cards are now `app_protocol` views with the same geometry. Each says
"Make me bigger" on a card too small for it rather than drawing outside
it. The Timer's title shows the running time ("Timer - 00:02.7"), as
before.

### `apps/timer` and `apps/tiles`

Each is the short loop that joins its core to its card. Esc still closes
either (the program exits and its card goes with it), and Ctrl+W still
closes the card like any other.

## Tests

- `timer_core`: the stopwatch runs on the ticks it is given, pauses and
  laps; a countdown says so once when it is up, its bar shows what is
  left, and its buttons are hit by pixel (through decoded views); laps
  and small cards never draw outside the card.
- `tiles_core`: lines slide and merge once per pair; the board slides
  four ways, scores and knows the end; sixteen tiles are drawn with
  numbers at twice the size; New is hit by pixel; a seeded game repeats;
  too small a card says so.
- `app_protocol`: a colour by value survives the trip; centred text
  decodes.
- `desk`: the Timer and Tiles open waiting cards and queue their
  programs. A program's notice is heard from another space with one
  chime and no blip. A click on the New button that Tiles' real view
  draws reaches the program as `n`.
- Gauntlet: the Timer, Tiles and Apps-grid shapes now also expect their
  programs (`desk: timer is thread 1`, `desk: tiles is thread 1`), and
  their pixel checks pass unchanged. By hand in QEMU, a real one-minute
  countdown was said in the notices centre ("Timer: 1 minute up");
  sixty-odd seconds is too slow for the gauntlet.
