# Phase 361: the notices centre (GFX-069)

## What changed

- Every notice -- the desk's (saves, opens, file operations, the kept
  look) and the workspace's (unknown commands, display switches) -- is
  logged, newest first, fifty deep, with the tick it arrived.
- The top bar shows **"N new"** before the clock while there are notices
  not yet looked at. Clicking it opens the **Notices** card; so does the
  palette row. The card lists `  12s ago   INFO   Saved memo` rows;
  `Clear` (Delete) empties the log, `Close` / Esc closes. Looking at the
  card zeroes the count.
- Not a dock tile.

## Design

`Desk::log_notice` is called from `notify` and from `windows_at` for
workspace notices that were not in the previous frame's list and were not
logged in the last half minute, so a notice shown for four seconds is one
entry even when a frame is built without the workspace's list (the
pointer path does that). Times are relative (`Desk::ago`),
computed from the frame tick, because the machine may have no set RTC and
a relative time is what a person wants from a log anyway.

The bar's right text is `bar_right(clock)`; `notices_at_column` maps a
click column to the indicator using the same right-alignment arithmetic
the compositor paints with.

## Tests

- `desk::tests::every_notice_is_kept_and_the_bar_counts_the_unseen_ones`
  (log order, dedupe of shell notices, bar text, `ago`, click on the
  indicator opens, looking clears the count, rows, chips, Clear, Esc,
  dock unchanged, bound).
- Machine: `cargo xtask gauntlet` exit 0.
