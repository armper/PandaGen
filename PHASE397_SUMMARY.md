# Phase 397: Files' preview behind a drawn divider (GFX-105)

## What changed

- The Files card's list and preview are separated by a drawn hairline in
  the card's overlay, the card's height, down the middle of a three-cell
  gap -- instead of a typed `|` on every line, which also ran down empty
  rows past the end of the preview. The columns are where they were.
- With the list alone (Ctrl+U) or in the Bin there is no divider.

## Why

The typed pipe was the last text-mode seam on the card after icons,
type and a drawn close had arrived.

## Tests

- `desk::tests` Files preview test: rows carry the preview after the
  gap, no pipe; the overlay has a vertical line with the preview and none
  with the list alone. The icon test counts pictures, not the line.
- Machine: `cargo xtask gauntlet` exit 0.
