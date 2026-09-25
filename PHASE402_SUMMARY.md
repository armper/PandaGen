# Phase 402: Notices with icons, and a quiet desk that says so (GFX-110)

## What changed

- Each row of the Notices card wears an icon in three cells kept for it:
  the app the notice is about (`notice_app`, as the toasts use), or the
  bell for anything else -- the console's warnings, say.
- With nothing kept, the card shows the bell large in its middle with
  "All quiet" under it, drawn in the overlay; the footer still says what
  lands there.

## Why

The notices log was the one list on the desk without icons, and empty it
was a blank card.

## Tests

- `desk::tests::every_notice_is_kept_and_the_bar_counts_the_unseen_ones`:
  rows move over three cells; the warning wears the bell and the save
  the Notepad's icon.
- Machine: `cargo xtask gauntlet` exit 0.
