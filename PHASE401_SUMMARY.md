# Phase 401: the meter says its share (GFX-109)

## What changed

- The top bar's memory meter is followed by its share in words,
  `25% mem`, as part of the bar's own text; the track and fill are still
  drawn over the six cells kept for them (`METER_CELLS`), and the
  hairlines between the tray's parts fall where they did around the
  whole part.
- `Desk::memory_percent` is the whole-percent share of the heap in use.

## Why

A bar with no number reads as decoration; the reference's tray names
the share beside it.

## Tests

- `desk::tests::the_tray_shows_the_date_a_memory_meter_and_a_badge`:
  the bar's text carries `25% mem`; the meter's track and fill widths are
  unchanged; the clock and badge are still the click targets they were.
- Machine: `cargo xtask gauntlet` exit 0.
