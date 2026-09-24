# Phase 373: the widget layer; the Calculator in real graphics (GFX-081)

## What changed

- `kernel_bootstrap/src/widgets.rs`: an immediate-mode widget layer for
  cards. An app builds a `Ui` each frame in its canvas's pixel space:
  `button`, `fill`, `outline`, `text`, `text_right`, `text_centered`, a
  `grid` helper and `text_width`. The `Ui` yields the compositor's draw
  operations and a list of hit rectangles; `hit(x, y)` answers with the
  *key byte* the control stands for, so a click reaches exactly the code
  a keystroke would. Buttons come in four kinds (plain, accent, quiet,
  primary), outline in the accent under the pointer, and use the large
  glyphs when they have the room. A `Palette` is six colours taken from
  the theme, so widgets follow the Look.
- Scaled text: `TextStyle.scale` on `DrawOp::Text`, drawn by a new
  `RenderTarget::draw_text_scaled` (each glyph pixel a `scale` block).
  One bitmap font gives a display or a heading without a second font in
  the binary.
- The Calculator is the first card converted: a raised display well with
  the expression small and the result at twice the size, twenty rounded
  keys on a grid (operators in the accent, `=` an accent fill, C and
  delete quiet), the tape in the muted tone below. `Layout::new(w, h)`
  is the geometry both the drawing and the desk's hit test use. The
  text-cell layout is gone.
- Desk: the pointer's canvas position is passed into the card each frame
  for hover; a content click is mapped into canvas pixels and asked of
  the card's `Ui`. `canvas_point_in` and `canvas_size` are shared.
- Gauntlet: the Calculator shape pins the accent `=` key and a raised
  digit key by pixel.

## Why this shape

Immediate mode with rectangles is the smallest thing that makes real
controls possible, and it keeps two rules the desk already has: every
frame is rebuilt from the app's state, and pointer and keyboard reach
the same code. No retained widget tree, no layout solver, no editor:
ten cards written in Rust are better described in code, under test.

## Tests

- `widgets::tests::a_grid_cuts_whole_cells_with_gaps`
- `widgets::tests::a_button_is_a_key_a_hit_answers_and_hover_outlines`
- `calculator::tests::keys_are_real_buttons_hit_by_pixel_and_typed_keys_reach_the_same_code`
- `desk::tests::the_calculator_takes_clicks_on_its_keys_and_typed_keys_alike`
  (clicks through the router at the layout's key centres; hover adds one op)
- `graphics_rasterizer::tests::scaled_text_is_the_glyph_enlarged`
- Machine: `cargo xtask gauntlet` exit 0.
