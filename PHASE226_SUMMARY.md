# Phase 226: Line And Rounded-Rectangle Primitives

## Summary

This phase implements `GFX-026` from the graphics roadmap in `graphics_rasterizer`.

New provided methods on `RenderTarget`, available to every target type including `ScissorTarget` and `LinearFramebufferTarget`:

- `draw_hline` / `draw_vline`: one-pixel runs.
- `draw_line(x0, y0, x1, y1)`: Bresenham over signed coordinates; endpoints may be off-target and only in-bounds pixels are written.
- `fill_rounded_rect(rect, radius)`: quarter-circle corners via per-row spans; radius zero is a plain fill and oversized radii clamp to a pill.
- `draw_rounded_border(rect, radius, thickness)`: outer span minus inner span per row, so the interior is never touched; a thickness that covers the shape degenerates to the filled version.

`RgbaBuffer` gains matching inherent wrappers. Helpers `rounded_row_span`, `clamp_radius`, and an integer `isqrt` keep the shape math in `core` without floats.

## Rationale

The desktop shell and graphical app surfaces (Epics 7 and 8) need more than axis-aligned boxes and glyphs: separators, focus rings with soft corners, pills for tabs and badges. Putting these on the trait rather than on one buffer type means clipping, damage, and framebuffer targets all draw the same pixels, which the scissor test in this phase confirms. Integer-only math keeps results deterministic across hosts and the kernel.

## Tests

- lines: endpoints, one pixel per row on a diagonal, steep and reversed lines, off-target clipping, hline/vline lengths
- rounded fills: radius zero equals `fill_rect`, corners clear while edge midpoints and centre are filled, horizontal and vertical symmetry, pill clamping
- rounded borders: two-thick ring on straight edges, clear interior and corners, full thickness equals fill, and primitives honour a scissor

Validated with `cargo test -p graphics_rasterizer` (15 tests) and `cargo test -p services_gui_host`; the full workspace suite passed at the start of the phase.
