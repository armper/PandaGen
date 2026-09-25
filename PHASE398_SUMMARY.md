# Phase 398: Sketch's drawn toolbar (GFX-106)

## What changed

- Sketch has a toolbar drawn across the top of its canvas
  (`SketchView::ui`): a round swatch for each of the six colours, the
  current one ringed in the text colour and the others outlined in the
  accent under the pointer, and Undo and Clear buttons on the right.
  Each control answers its key to a click through the same `Ui` hit
  areas the other drawn cards use.
- `1` to `6` pick a colour directly; `c` still cycles, `z` undoes, `x`
  clears. The footer says `1-6 colour`.
- Strokes begin only below the toolbar; a press on it is a control or
  nothing. The old 12px swatch in the corner and the Colour / Undo /
  Clear header chips are gone (one control per action, as in Phase 395).
- Gauntlet: the Sketch shape pins the red swatch and the ring around it
  after one C, instead of the corner swatch.

## Why

A colour you can only reach by cycling with a key, shown as a 12px
square, was the least discoverable control on the desk -- and mouse-only
use has to work.

## Tests

- `sketch::tests::strokes_are_drawn_as_lines_and_kept_as_text`: the
  toolbar's hit areas and the number keys.
- `desk::tests::sketch_draws_a_stroke_with_the_pointer_and_keeps_it_as_a_document`:
  strokes drawn below the toolbar, a click on a swatch picks its colour
  without drawing, a click on Undo takes a stroke back.
- Machine: `cargo xtask gauntlet` exit 0.
