# Phase 204: Workspace Tab Chrome In Desktop Windows

**Completion Date**: 2026-03-17

## Overview

Phase 204 implements `GFX-008` by rendering workspace tab state as actual desktop chrome in `services_gui_host`. Multi-tab workspace tiles now carry explicit tab metadata into desktop windows, and the compositor renders a tab strip in the window header instead of reducing tab state to invisible metadata or composed text summaries.

This keeps PandaGen aligned with its clean-slate direction: workspace structure is visible through native desktop primitives rather than being flattened into terminal-style summaries.

## What Changed

### 1. Added Desktop Tab Metadata

`services_gui_host` now defines a `DesktopTab` type and `DesktopWindow` carries a `tabs` field with a serde default.

This makes visible tab chrome part of the desktop contract instead of an implementation detail hidden inside workspace-specific structures.

### 2. Workspace Adapter Projects Tile Tabs Into Window Chrome

The workspace-to-window adapter now maps `WorkspaceTileRenderSnapshot.tabs` and `active_component` into ordered `DesktopTab` values.

- the active tab uses the current frame title when available
- inactive tabs get deterministic fallback labels such as `Tab 2`
- empty-tab windows remain valid and render without a strip

### 3. Compositor Renders A Tab Strip

Window header rendering now prefers tab chrome when tabs are present. Multi-tab tiles therefore render visible tab state directly in the desktop surface, while split state continues to be represented by actual window placement.

## Rationale

Before this phase, PandaGen could place split tiles as separate desktop windows, but it still treated a multi-tab tile as though it were a single unnamed content region. That hid one of the core workspace concepts just when the system is trying to move toward native graphical interaction.

Adding a small tab-chrome model now gives later shell work a stable foundation for richer window chrome without forcing future layers to reconstruct tab state from composed text.

## Tests

Added and validated focused coverage in `services_gui_host/src/lib.rs`:

- `test_workspace_snapshot_maps_tile_tabs_into_window_metadata`
- `test_compose_workspace_snapshot_renders_tab_strip_for_multi_tab_tile`

Validation run:

- `cargo test -p services_gui_host`
- `cargo test --all`

## Files Changed

- `services_gui_host/src/lib.rs`
- `docs/path_to_graphics.md`
- `PHASE204_SUMMARY.md`
