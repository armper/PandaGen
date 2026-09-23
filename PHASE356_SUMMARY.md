# Phase 356: word wrap in Notepad (GFX-064)

## What changed

Lines longer than the card wrap onto the next visual row. The break is
after the last space that fits; a word longer than the card is cut, not
lost. The wrap width is the card's width this frame, so resizing (or
Ctrl+Left/Right snapping) reflows at once. The document is unchanged --
a line is still a line -- only its presentation is.

## How

One table, `Notepad::visual_rows() -> Vec<(line, first byte, end byte)>`,
drives everything that used to index document rows: `viewport_lines`,
`viewport_cursor`, `place_cursor` (clicks and drags), `viewport_selection`
(the selection fill per segment), `scroll_by` and `keep_cursor_visible`.
A caret exactly on a break is shown at the start of the next segment,
which is where typing lands. `wrap == 0` restores the old one-row-per-line
behaviour, which the tests still exercise.

## Tests

- `notepad::tests::long_lines_wrap_at_spaces_and_the_caret_and_clicks_map_through`
  covers the split, caret mapping both ways, a click on a wrapped row, a
  selection across a wrap, a word longer than the card, no-wrap, and the
  view following the caret in visual rows.
- Machine: a long typed line wraps in a 720px card and reflows to a 640px
  half after Ctrl+Left (screenshots); `cargo xtask gauntlet` exit 0.
