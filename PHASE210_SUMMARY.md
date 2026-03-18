# Phase 210: Add Raster Clipping And Damage Redraw

## Summary

This phase implements `GFX-014` from the graphics roadmap.

`graphics_rasterizer` now provides explicit clipping support through `ScissorTarget`, backed by new `RasterRect` intersection helpers. That gives the graphics stack a deterministic way to constrain fills, borders, and glyph drawing to a bounded region without depending on framebuffer-specific logic.

`services_gui_host` now uses that clipping model to render desktop windows through a damage-aware path:

- `render_desktop_to_target(...)` now delegates to a damage-capable render path
- `render_desktop_to_target_with_damage(...)` can repaint only a supplied damaged region
- raster window painting clips chrome and content separately so long titles or body lines do not spill outside their window
- render stats now report both the damaged region and how many windows were actually repainted

## Rationale

A clean-slate desktop should not redraw like a terminal-era system that assumes the entire screen is the only safe paint unit. Clipping and damage-aware redraw are foundational graphics primitives for any modern compositor because they make window composition spatially explicit and keep redraw work bounded.

This phase keeps that model testable:

- clipping lives in the rasterizer as a general mechanism
- the GUI host consumes clipping as compositor policy
- damage accounting is visible in deterministic unit-test results

That separation fits PandaGen's broader design goal: modern primitives first, with minimal hidden behavior and no legacy rendering assumptions.

## Tests

Validated with:

- `cargo fmt --all`
- `cargo test -p graphics_rasterizer`
- `cargo test -p services_gui_host`
- `cargo test --all`
