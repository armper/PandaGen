# Phase 445: the desk repaints only what changed (GFX-120)

## What changed

The desk used to repaint the whole screen for every frame: every caret
blink, every keystroke, every tick of a running Timer. Under QEMU's
emulated CPU that took about a third of a second a frame (32.5 ticks on
average, measured). With a Terminal open, the caret's blink alone kept
the processor busy, so typing felt slow and a thread doing work got only
half the processor. Now a frame repaints only what changed:

| (QEMU, a Terminal open, 20 s) | before | after |
|---|---|---|
| average frame | 32.5 ticks | 3.0 ticks (the first, full frame included) |
| share of the screen repainted | 100% | 7% |
| frames needing a present | all | about half (nothing changed in the rest) |

The compositor already knew how to repaint a damage rectangle; nothing
used it. Using it turned up two compositor bugs, and the new
pixel-for-pixel test caught both.

### What a frame repaints: `desktop_frame::frame_damage`

The renderer keeps the scene it last drew, and compares the next one
with it:

- **A card whose only change is its caret** repaints the caret, a few
  pixels round it: the blink is a 10×24 rectangle, not the screen.
- **A card whose changes are all inside its header, content or footer**
  (typing, a program's new view, "saving...", a Timer's title) repaints
  just those strips. Each is drawn clipped to its own strip, so nothing
  else can have changed.
- **Anything else about a card** (it moved, was raised, gained focus,
  opened or closed) repaints where it was and where it is, with exactly
  the shadow the painter draws (14 px to the sides and up, 19 down).
- **Glass.** The dock and the top bar blur what is behind them, so
  damage that touches what a glass window's blur reads repaints the
  whole of it. A part repainted alone would blur its already-frosted
  neighbours back in.
- **A new theme, wallpaper or size, a veil (the rest screen), or the
  first frame** repaints everything.
- **Separate changes stay separate:** up to eight rectangles, merged
  only where they overlap, so a card near the top and the dock at the
  bottom are not the whole screen between them.
- **Nothing changed:** nothing is repainted, and nothing is presented.

### Two compositor bugs, found by the new test

- **A card repainted through a damage rectangle painted outside it.**
  Its clip was the damaged part of the card grown by the shadow spread,
  so the body was repainted up to 19 px beyond the damage but the text
  only within it (erasing lines), and the shadow was blended over
  itself. The clip is now the card's shadowed area intersected with the
  damage, never beyond it.
- **Damage touching only a card's (or the dock's) shadow skipped the
  window**, so the shadow wasn't repainted. They are now painted
  whenever their shadowed area meets the damage.

### Telemetry

`gfx` in the Terminal now also says how long frames take and how much
of the screen they repaint: `render: avg=3.01 ticks max=33 ticks
avg_cycles=... repainted=7% full=1`.

## Tests

- `desktop_frame::damage_tests`: a damage-limited frame equals a fresh
  full render **pixel for pixel**, on a real desk, after each of these:
  nothing (and nothing is repainted); a caret blink (under 1% of the
  screen); typing (under half); the pointer moving; a second card
  opening over the first; the first raised; the second closed; and
  after invalidation (everything). Damage that touches the frosted bar
  takes all of it.
- `services_gui_host`: render telemetry reports the average, the worst
  frame and the share repainted; its other tests still pass with the
  card and dock clipping fixed.
- Gauntlet: every shape's pixel checks (cards, the dock, swatches,
  keys) pass with damage-limited rendering.
