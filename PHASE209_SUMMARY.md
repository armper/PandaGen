# Phase 209: Add Desktop Font Rasterization

## Summary

This phase implements `GFX-013` from the graphics roadmap.

The graphics stack now has an explicit font abstraction instead of treating glyph drawing as an untyped side effect. `graphics_rasterizer` now provides:

- `BitmapFont` with measurable glyph width, height, and advance
- a compact source glyph set that is rasterized into larger desktop-friendly output
- a default desktop font used by generic text rendering
- a font-aware draw path so callers can render text with chosen font metrics

`services_gui_host` now uses that desktop font path for window titles and content. The GUI host also derives cell width from font advance, which means title spacing, content spacing, and cursor positioning are aligned to the font model rather than old hard-coded assumptions.

## Rationale

The project goal is a clean-slate desktop, not a terminal pretending to be one. That means text has to be treated as a real graphical primitive with explicit font metrics, not as a historical side effect of console-era glyph code.

This phase keeps the architecture clean:

- font behavior is defined in the rasterizer
- GUI layout consumes font metrics instead of duplicating them
- text rendering remains deterministic and unit-testable

That gives PandaGen a credible path toward a readable desktop without coupling graphics to legacy console implementations.

## Tests

Validated with:

- `cargo test -p graphics_rasterizer`
- `cargo test -p services_gui_host`
- `cargo test --all`
