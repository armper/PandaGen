# Phase 354: keyboard window management, word count, dock labels (GFX-062)

## What changed

- **Ctrl+arrows manage the focused card.** Ctrl+Left / Ctrl+Right snap to
  the left/right half of the work area, Ctrl+Up fills it, Ctrl+Down
  returns the card to the size it had before the first snap. The palette's
  Snap rows show these keys.
- **The dock speaks through the top bar.** Hovering a tile puts
  `Files   -   click to open` or `Notepad   -   2 open` in the bar's left
  slot; leaving the dock restores the focused card's name. The tiles stay
  monograms.
- **Word count** in the Notepad footer (`5 words`), whitespace-separated.

## Why

Drag-to-edge snapping (GFX-052) was pointer-only. Every window action
should have a keyboard path that costs no more than the pointer one; the
window keys are the same `snap_focused` the palette already ran. The
restore keeps the *original* size (the first snap's `restore`), which is
what a person means by "put it back".

Labels on dock tiles would cost horizontal room in one 8x16 font; the bar
already has the room and the eye is already there.

## Tests

- `desk::tests::ctrl_arrows_snap_fill_and_put_a_window_back`
- `desk::tests::the_bar_names_the_dock_tile_under_the_pointer`
- `notepad::tests::the_footer_counts_words`
- Parser: Ctrl held with E0 arrows yields the window keys.
- Machine: `cargo xtask gauntlet` exit 0.
