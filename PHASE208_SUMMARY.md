# Phase 208: Add Render Target Abstraction

## Summary

This phase implements `GFX-012` from the graphics roadmap.

The new `graphics_rasterizer` crate no longer assumes that every render pass must target an owned `RgbaBuffer`. It now exposes a `RenderTarget` contract with shared default draw operations for:

- clear
- clipped fills
- borders
- glyph text rendering

Two concrete targets are now available:

- `RgbaBuffer` for off-screen deterministic testing and snapshot generation
- `LinearFramebufferTarget` for framebuffer-shaped pixel sinks with explicit stride and pixel format

`services_gui_host` now uses that abstraction through `render_desktop_to_target(...)`, which lets the desktop compositor paint into any compliant render target. The existing RGBA composition path now delegates through that generic route instead of being a special-case code path.

## Rationale

The previous phase proved PandaGen could rasterize a desktop into an RGBA buffer, but it still hard-coded one storage model. That was not enough for a clean-slate OS architecture because real presentation targets and test targets should share semantics without sharing storage assumptions.

This phase fixes that by separating:

- rendering semantics
- pixel target layout

That keeps the renderer deterministic, testable, and ready for the next step toward direct framebuffer presentation.

## Tests

Validated with:

- `cargo test -p graphics_rasterizer`
- `cargo test -p services_gui_host`
- `cargo test --all`
