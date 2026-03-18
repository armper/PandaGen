# Phase 200: Clean-Slate Desktop Surface Composition

**Completion Date**: 2026-03-17

## Overview

Phase 200 moves `services_gui_host` from a string-concatenation placeholder to a deterministic desktop surface compositor. The design explicitly targets PandaGen's clean-slate, next-generation OS direction: compose native desktop surfaces with positioned windows, focus state, z-order, and clipping instead of inheriting terminal-era rendering assumptions.

## What Changed

### 1. Explicit Desktop Surface Model

`services_gui_host` now exposes structured desktop primitives:

- `SurfaceSize` for desktop dimensions
- `SurfaceRect` for window placement
- `DesktopWindow` for positioned view composition with focus and z-order
- `SurfaceFrame` rows, width, and height for deterministic inspection and downstream rendering

This keeps the compositor testable under `cargo test` while establishing a real graphics-oriented surface contract.

### 2. Native Desktop Compositor

Added `Compositor::compose_desktop(...)`, which:

- fills a desktop background
- sorts windows deterministically by z-order and `ViewId`
- renders window chrome directly onto a shared surface
- clips windows at desktop edges
- overlays content and cursor state inside each window

This is a direct desktop composition model, not a serialized terminal transcript.

### 3. Legacy Compose Path Preserved

The existing `compose(Vec<ViewFrame>)` API remains available for simple text aggregation, but now returns richer `SurfaceFrame` metadata (`rows`, `width`, `height`) so the output is inspectable as a surface instead of only as a flat string.

### 4. Repository Guidance Updated

`AGENTS.md` now explicitly states the clean-slate, next-generation OS rule so future work continues to prefer new primitives over inherited historical abstractions.

## Rationale

The old GUI host was not a graphics system. It only concatenated `ViewFrame` text, which was useful for early validation but could not represent a desktop with multiple windows or overlapping views.

The new model is still intentionally deterministic and text-serializable because PandaGen values testability first. But the abstraction boundary is now correct: a desktop surface made from windows on a plane, rather than a transcript that happens to look like UI.

That gives the project a solid bridge toward later framebuffer or GPU-backed presentation without locking rendering policy into the kernel.

## Tests

Added/updated tests in `services_gui_host/src/lib.rs`:

- `test_compositor_renders_frames`
- `test_compose_desktop_renders_window_chrome_and_cursor`
- `test_compose_desktop_honors_window_z_order`
- `test_compose_desktop_clips_windows_at_surface_edge`

Validation run:

- `cargo test -p services_gui_host`
- `cargo test --all`

## Files Changed

- `AGENTS.md`
- `services_gui_host/src/lib.rs`
- `PHASE200_SUMMARY.md`
