# Phase 243: Compositor Work Benchmarks

## Summary

This phase implements `GFX-045` from the graphics roadmap.

- `graphics_rasterizer::CountingTarget`: wraps any target and counts pixel writes exactly, since every provided draw op bottoms out in `write_pixel`.
- `services_gui_host::bench`: deterministic scenarios on a 160x44-cell (1280x800) surface: full composes of 8 and 64 cascaded windows and an 8x6 tile grid; damage-limited frames for a pointer move, a caret blink, and a palette opening, with damage taken from `diff_scenes`. Each yields a `BenchReport` (pixels written, windows painted, damage area, surface area, repaint permille). `run_all` is asserted deterministic and bounded under `cargo test`.
- `examples/compositor_bench.rs` prints the same reports with wall-clock time per run (about 57 ms for all six scenarios in release on the host).

**Finding and fix**: the first run showed a caret blink repainting 33.5% of the surface because a changed window was damaged as a whole. `diff_scenes` now detects a caret-only change and damages just the old and new caret rectangles (`caret_pixel_rect`, which matches the painter exactly); the blink now writes 254 pixels. Remaining observations recorded as baselines: 64 cascaded windows overdraw about 6.4x the surface, and opening the palette costs its own area plus the overdraw of the windows beneath it.

## Rationale

Measuring work rather than time keeps the benchmark deterministic and usable as a regression test, which is what the roadmap needs before optimisation phases. The caret finding shows why: a bound that fails is a bug report with numbers attached.

## Tests

- `graphics_rasterizer`: counting target counts exact writes, including clipped ones being excluded.
- `services_gui_host`: scenario bounds (full composes paint every window and at least the surface once; 64 windows under 8x overdraw; pointer move and caret blink under 1% repaint; palette under 80%); caret-only delta damage is a few cells; results deterministic across runs.
