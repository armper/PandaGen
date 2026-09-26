# Phase 426: line numbers in the Notepad (ED-020)

## What changed

No editor on the machine showed line numbers (Phases 15, 19, 62, 69, 86
and 90 all listed them as later; the host's `editor.line_numbers` setting
was stored and drawn by nothing). The Notepad now has them, per card:

- **Palette → "Line numbers"** shows or hides them for the focused
  Notepad (mouse-only reachable, like every Notepad action).
- **The gutter** is the widest number and a space, at least two digits
  wide, and grows with the document (a hundred lines make it four
  columns). A wrapped line's later segments have no number, so the
  numbers are document lines, not screen rows. A hairline runs down the
  gutter's trailing space.
- **Everything that speaks in columns counts the gutter**, inside the
  Notepad, so the desk needs no arithmetic of its own: wrapping gives the
  gutter its columns from the text's, the caret and the selection are
  drawn after it, and a click -- or a drag -- in the gutter lands at the
  start of that line.

## Tests

- `line_numbers_take_a_gutter_that_the_caret_selection_and_clicks_count`:
  numbered and wrapped rows; a click on the text and in the gutter; the
  caret column; the selection span shifted by the gutter; the gutter
  widening at a hundred lines; toggling off.
- QEMU: three numbered lines with the hairline, from the palette.
- `cargo xtask gauntlet`: exit 0.
