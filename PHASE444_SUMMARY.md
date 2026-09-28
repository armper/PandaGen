# Phase 444: the pointer is a program's, and Sketch moves out (PROC-012)

## What changed

A program can now take the pointer. Press, drag and release on a
program's card reach its program as events. Programs can draw paths,
and a view can hold a whole drawing. **Sketch**, the last dock app that
keeps a single document, is a program: the sixth dock app out of the
kernel after the Calculator, the Timer, Tiles, the Calendar and Tasks.
It draws the same toolbar the kernel used to (the existing pixel checks
pass unchanged). `kernel_bootstrap/src/sketch.rs` is gone.

### Pointer events

- **`Event::Pointer { kind, x, y }`**, in the canvas's own pixels, with
  kinds `Down`, `Move` and `Up`. The desk sends one when the pointer
  presses on the canvas where no button or hit area takes it; a click
  on a button is still that button's key.
- **Moves come only between a press and its release** (the desk tracks
  the drag), so an idle or passing pointer costs a program nothing.
- **A fast drag never floods a program.** A move replaces a move not
  yet taken, both in the desk's outgoing list and in the program's
  32-event ring. The program sees where the pointer is rather than every
  place it passed, and a long drag can never fill the ring and lose its
  release.
- The drag ends on release wherever the pointer is. Moves outside the
  canvas are not sent (they would be outside the card).

### Paths, and room for a drawing

- **`Op::Path`**: a stroke, up to 4096 points joined by lines of a given
  thickness, or a dot for one point. Every point must be inside the
  card; an empty or oversized path is refused.
- **A view can be 64 KiB** (from 8 KiB), enough for a whole drawing. The
  kernel copies it straight to the heap, with interrupts on, rather than
  onto the program's kernel stack.
- **Messages and writes can be 128 KiB**, enough for a full drawing's
  document (about 100 KB). `write` now copies to the heap too.
- **A program's heap is 1 MiB** (`pandagen_app`'s `heap` feature), room
  for a view and a document's worth of messages each way beside its own
  data.

### `sketch_core` and `apps/sketch`

The strokes, the document format (`colour: x,y x,y ...`), the toolbar
and the card moved from the kernel with their tests, and are tested on
the host. Each stroke is one path; the swatches are hit areas and Undo
and Clear are buttons. `pointer(kind, x, y)` begins a stroke below the
toolbar, extends it while pressed, and ends it on release.

- **Bounded**, so a drawing always fits one view: 200 strokes and 12,000
  points in all (2,000 a stroke). Past that a new stroke is refused and
  the footer says "the drawing is full".
- Kernel-era drawings, whose points were signed, still load.

The program holds its card and exactly one document, `sketch`, to read
and write. It loads the drawing at start, saves a second after the last
change (never mid-stroke), and saves at once if its card closes with a
change waiting.

### The desk

Sketch maps to its program like the other dock apps. The desk's own
stroke tracking (`sketching`), its reading and quiet saving of `sketch`,
and its Sketch card code are gone. It has a general program drag
(`program_drag`) instead. `sketch` still wears the Sketch icon in Files.

## Tests

- `app_protocol`: a path is a stroke inside the card, and an empty path
  is refused; pointer events round-trip.
- `sketch_core`: strokes are drawn as paths and kept as text; the pointer
  draws only below the toolbar and only while pressed; the toolbar's
  swatches and buttons are hit by pixel; round trips, including
  kernel-era points; undo, clear and the save clock; a full drawing
  still fits one view, says it is full and refuses more; too small a
  card says so.
- `desk`: Sketch is a program, and the pointer on its canvas is its own.
  A press, two drags and a release through the real router reach it as
  down, one move (folded) and up. A move with the button up sends
  nothing, and a click on its third swatch is key `3`.
- Gauntlet: the Sketch shape now also expects its program, and its three
  pixel checks (the red swatch, its ring, the empty canvas) pass
  unchanged. A new two-boot shape, "Sketch: a stroke drawn with a real
  drag, kept across a reboot", draws with a real QEMU mouse drag (read
  absent, wrote the stroke), reboots, and expects the restored card's
  program to read the stroke back.
