# Phase 206: Deterministic Compositor Coverage Expansion

**Completion Date**: 2026-03-17

## Overview

Phase 206 implements `GFX-010` by widening deterministic test coverage for the desktop compositor in `services_gui_host`. The goal of this phase is not a new rendering feature; it is to lock in the behavior already established for tile mapping, focus chrome, and cross-layer ordering so later graphics work can move faster without destabilizing the desktop contract.

This stays consistent with PandaGen's clean-slate direction: explicit surface contracts are only useful if they are protected by direct, deterministic tests.

## What Changed

### 1. Added Horizontal Tile Mapping Coverage

The compositor test suite now covers the horizontal split path in addition to the vertical split path. This validates that workspace tiles partition the desktop surface top-to-bottom with deterministic focus assignment.

### 2. Added Focus Chrome Coverage

The test suite now verifies that focused and unfocused windows render distinct chrome. This ensures focus state remains visible and testable instead of becoming an incidental styling detail.

### 3. Added Multi-Layer Ordering Coverage

The test suite now verifies that modal surfaces outrank lower layers even when their local `z_index` is smaller. This complements the earlier notification-layer test and gives the compositor a clearer ordering contract.

## Rationale

By this point, PandaGen's text-serializable desktop compositor had several meaningful behaviors:

- workspace tile placement
- focus-dependent chrome
- role and layer policy
- tab-strip rendering

But some of those behaviors were only indirectly covered. This phase raises the bar before the project moves into rasterization work, where regressions become more expensive to spot and reason about.

## Tests

Added and validated focused coverage in `services_gui_host/src/lib.rs`:

- `test_workspace_snapshot_maps_horizontal_tiles_to_desktop_windows`
- `test_compose_desktop_focus_visuals_distinguish_focused_from_unfocused`
- `test_compose_desktop_modal_layer_outranks_notification_and_overlay`

Validation run:

- `cargo test -p services_gui_host`
- `cargo test --all`

## Files Changed

- `services_gui_host/src/lib.rs`
- `docs/path_to_graphics.md`
- `PHASE206_SUMMARY.md`
