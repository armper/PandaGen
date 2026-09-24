# Phase 369: the Timer card (GFX-077)

## What changed

- `kernel_bootstrap/src/timer.rs`: a stopwatch with laps and a countdown
  with presets (1, 5, 10, 25 minutes). Time is the kernel's 100 Hz tick,
  handed in through `Desk::tick`; the card never reads a clock, so a
  test runs a whole countdown instantly.
- When a countdown is up, the desk raises a notice ("Timer: 5 minutes
  up") through the notices centre, once, whether or not the card is on
  the current space. Start runs it again.
- Controls are `[ buttons ]` in the text, hit by line and column with a
  general `button_at` scanner (shared with later cards); each has a key:
  Space start/pause, L lap, R reset, S stopwatch, 1-4 presets.
- `DeskRequest::Repaint`: the desk asks the kernel to draw again ten
  times a second while a timer runs, so the display moves; the kernel
  serves it by marking output dirty.
- Desk: `DeskApp::Timer` on the dock (seventh tile, "Ti"), a palette row,
  chips Start/Pause, Reset, Close; the title carries the time while
  running; the overview line shows it.
- Gauntlet: dock pins moved with the seventh tile; a shape opens the
  Timer from the palette, starts it and laps.

## Why this shape

A timer that only ticks while its card is drawn would be a clock that
stops when you look away. Putting the clock in `tick`, and the alarm in
the notices log, makes it a service the desk performs rather than a
picture the card paints.

## Tests

- `timer::tests::the_stopwatch_runs_on_the_desks_ticks_pauses_and_laps`
- `timer::tests::a_countdown_says_so_once_when_it_is_up_and_buttons_are_hit_by_column`
- `desk::tests::a_countdown_that_is_up_becomes_a_notice_from_any_space`
- Machine: `cargo xtask gauntlet` exit 0.
