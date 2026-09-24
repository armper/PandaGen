# Phase 377: motion (GFX-085)

## What changed

- Cards move. A new card rises into place from a little smaller and a
  little lower; a snap or an un-snap slides from where the card was; a
  card brought back from the dock grows up from the dock's edge; the
  overview's cards fly from their places to their slots when it opens,
  and the picked card flies back; a closed card leaves its chrome
  shrinking away. Every motion takes `MOTION_TICKS` (180 ms) with an
  ease-out curve.
- The desk keeps a list of motions (card, where from, when) and ghosts
  (title, where from, when); a card's *logical* bounds are always the
  destination, and only the frame given to the compositor is partway,
  so hit-testing, snapping and every existing test that reads
  `window.bounds` are untouched. A second motion on a moving card starts
  from where it is drawn, so nothing jumps.
- `tick` is the clock: it prunes finished motions and asks the kernel to
  repaint every tick while anything moves (reusing `DeskRequest::Repaint`
  from the Timer, deduplicated). Before the first tick there is no clock
  and nothing moves, which is why a desk built and read in one breath, as
  the host tests do, is where it will be.
- `overview_slot` is factored out of the overview drawing so the pick can
  animate from it.

## Why this shape

Motion is the single largest "this is real" signal a desk can give, and
the cheapest: interpolating one rectangle per moving card per frame. The
kernel already repainted on request, and the compositor already drew
cards wherever it was told. Keeping logical bounds fixed and interpolating
only the drawn frame kept the change out of every other code path.

## Tests

- `desk::tests::cards_rise_slide_and_shrink_away_on_the_desks_clock`
  (no clock, no motion; rise; repaint every tick; snap slide; ghost;
  easing and lerp helpers)
- Machine: `cargo xtask gauntlet` exit 0 (every shape waits a second
  before its screenshot, so pixel pins are unaffected).
