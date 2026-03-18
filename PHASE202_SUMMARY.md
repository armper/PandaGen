# Phase 202: Workspace Snapshot To Desktop Window Adapter

**Completion Date**: 2026-03-17

## Overview

Phase 202 implements `GFX-006` from the graphics roadmap by bridging real workspace state into the clean-slate desktop compositor. `services_gui_host` can now derive `DesktopWindow` values directly from `WorkspaceRenderSnapshot`, which means split and tab state is no longer trapped in workspace-only metadata.

This keeps graphics work aligned with PandaGen's next-generation OS direction: the desktop is composed from explicit structured state, not from terminal-era summary text or implicit ordering assumptions.

## What Changed

### 1. Workspace Snapshot Adapter In `services_gui_host`

Added a dependency on `services_workspace_manager` and introduced two new compositor entry points:

- `Compositor::desktop_windows_from_workspace_snapshot(...)`
- `Compositor::compose_workspace_snapshot(...)`

These APIs map visible workspace tiles into `DesktopWindow` instances using the workspace split axis and tile render payloads.

### 2. Deterministic Tile Placement

The adapter now sorts tile render snapshots by `tile_index` before partitioning them onto the desktop surface. That prevents transport or serialization order from changing on-screen placement, which is important for deterministic replay and for any future remote graphical transport.

### 3. Clean Fallback Behavior

When a workspace does not expose per-tile render snapshots yet, the adapter falls back to the focused `main_view` and promotes it to a single full-surface desktop window. Untitled or empty tiles also receive stable fallback frames and titles so the compositor always has something explicit to render.

## Rationale

The desktop compositor was already capable of drawing positioned windows, but it was not yet connected to the workspace manager's split/tab model. Without this bridge, the graphics path would remain a demo surface disconnected from the real shell state.

Sorting by `tile_index` is also part of the design, not just a test fix. A next-gen OS should prefer explicit, replayable structure over hidden dependence on incidental vector order.

## Tests

Added and validated focused coverage in `services_gui_host/src/lib.rs`:

- `test_workspace_snapshot_maps_vertical_tiles_to_desktop_windows`
- `test_workspace_snapshot_maps_single_tile_to_full_surface_window`
- `test_workspace_snapshot_orders_tiles_by_tile_index`

Validation run:

- `cargo test -p services_gui_host`
- `cargo test --all`

## Files Changed

- `services_gui_host/Cargo.toml`
- `services_gui_host/src/lib.rs`
- `docs/path_to_graphics.md`
- `PHASE202_SUMMARY.md`
