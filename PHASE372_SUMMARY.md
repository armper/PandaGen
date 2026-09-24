# Phase 372: Sketch, drawing with the pointer (GFX-080)

## What changed

- `kernel_bootstrap/src/sketch.rs`: strokes as lists of points in the
  canvas's own pixel space. Press begins a stroke, moving with the button
  down extends it, release ends it. Six colours cycled by the Colour chip
  (or `c`), Undo (`z`, Ctrl+Z) drops the last stroke, Clear (`x`) all.
- The first card whose content is graphics, not text: the desk fills
  `ViewContent::Graphics` with `DrawOp::Line`s (doubled one pixel down so
  a stroke has body), a dot for a lone point, and a swatch of the current
  colour in the canvas's top-right corner. The compositor already drew
  graphics content inside cards; nothing there changed.
- `Desk::canvas_point` maps a screen pixel to the canvas (the card's
  content origin, the same one the compositor's graphics clip uses); the
  desk tracks the stroke in `sketching` from press to release, alongside
  the text-selection drag it already had.
- The drawing is the document `sketch`: one stroke per line, `colour:
  x,y x,y ...`. Loaded when the card opens (through `launch_request`),
  saved quietly a second after it changes, never mid-stroke.
- Desk: `DeskApp::Sketch` on the dock (tenth tile, "Sk"), a palette row,
  chips Colour, Undo, Clear, Close, an overview line with the stroke
  count.
- Gauntlet: dock pins moved with the tenth tile; a shape opens Sketch,
  presses C and pins the red swatch's pixel.
- **H3.** The "boot on a small machine" shape failed on this phase's
  first run: the kernel ELF had grown to 9.9 MB, 6.9 MB of it DWARF line
  tables, and the bootloader reads the whole file into memory, so a
  16 MiB machine had 2.3 MiB left and no room for the 2 MiB heap. The
  kernel profile now strips debug info from the binary (3.1 MB; the small
  machine reports 9 MiB usable and takes an 8 MiB heap). Recorded in
  `GAUNTLET.md`.

## Why this shape

A drawing card is the proof that cards are not text boxes. The
compositor's graphics content and the pointer's pixel position were both
already there; what was missing was a card that used them, and a way to
keep the result. Keeping it as a text document keeps every other rule:
it is in Files, it has versions, it saves itself.

## Tests

- `sketch::tests::strokes_are_drawn_as_lines_and_kept_as_text`
- `desk::tests::sketch_draws_a_stroke_with_the_pointer_and_keeps_it_as_a_document`
- Machine: `cargo xtask gauntlet` exit 0.
