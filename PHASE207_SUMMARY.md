# Phase 207: Add Software Raster Desktop Path

## Summary

This phase implements `GFX-011` from the graphics roadmap.

PandaGen now has a dedicated `graphics_rasterizer` crate that owns deterministic software pixel painting into an RGBA buffer. The crate provides:

- clipped solid fills
- rectangular borders with explicit thickness
- bitmap glyph rendering for a small built-in ASCII subset
- an owned off-screen RGBA buffer suitable for tests and future framebuffer presentation

`services_gui_host` now consumes that rasterizer to produce a pixel-space desktop frame from the clean-slate desktop model. The compositor can still emit text-serializable test surfaces, but it can now also render the same window/layout/layer state into RGBA output using explicit border, fill, text, and cursor primitives.

## Rationale

The project goal is a next-gen operating system that does not inherit terminal-era or POSIX-era presentation assumptions. A real desktop path needs a real pixel pipeline.

This phase keeps the architecture disciplined:

- low-level raster logic lives in its own crate
- GUI policy stays in `services_gui_host`
- output remains deterministic and unit-testable under `cargo test`

That is the right clean-slate direction: graphics should be a native system capability, not an afterthought layered on top of text transcripts.

## Tests

Validated with:

- `cargo test -p graphics_rasterizer`
- `cargo test -p services_gui_host`
- `cargo test --all`
