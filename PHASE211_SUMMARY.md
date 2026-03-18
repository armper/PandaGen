# Phase 211: Add Golden Raster Surface Tests

## Summary

This phase implements `GFX-015` from the graphics roadmap.

`services_gui_host` now has explicit golden raster tests for the desktop renderer. The test harness converts RGBA output into a stable palette-coded text format and compares it against checked-in fixtures. Two golden paths are covered:

- direct desktop composition with layered windows
- workspace snapshot composition routed through the workspace-to-desktop adapter

The fixtures live under `services_gui_host/tests/golden/` and are exercised from the crate's unit tests.

## Rationale

PandaGen is building a next-gen desktop stack, not a one-off demo renderer. That means graphics behavior needs deterministic reviewable baselines the same way service contracts and text rendering already do.

Golden raster tests provide that without introducing boot-time coupling:

- they validate exact pixel-class output under `cargo test`
- they protect layering, borders, text placement, cursor rendering, and workspace mapping from silent regressions
- they keep graphics development compatible with the project's clean-slate, test-first design rules

This gives the graphics path a stable contract before it is wired into the framebuffer presenter.

## Tests

Validated with:

- `cargo fmt --all`
- `cargo test -p services_gui_host golden_fixture`
- `cargo test -p services_gui_host`
- `cargo test --all`
