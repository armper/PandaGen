# Phase 203: Explicit Desktop Window Roles

**Completion Date**: 2026-03-17

## Overview

Phase 203 implements `GFX-007` by defining explicit semantic roles for desktop windows in `services_gui_host`. The compositor now has a typed role model for primary content, status surfaces, overlays, palettes, notifications, and modals, which removes the need to infer intent from ad hoc construction patterns.

This keeps PandaGen aligned with its clean-slate, next-generation OS direction: window meaning should be explicit in data structures and serializable state, not hidden in historical UI conventions.

## What Changed

### 1. Added `DesktopWindowRole`

`services_gui_host` now exposes a `DesktopWindowRole` enum with these roles:

- `Main`
- `Status`
- `Overlay`
- `Palette`
- `Notification`
- `Modal`

`DesktopWindow` now carries a `role` field, and the default role is `Main`.

### 2. Backward-Compatible Serialization Default

The new `role` field is marked with `#[serde(default)]`, and `DesktopWindowRole` defaults to `Main`. That keeps serialized `DesktopWindow` payloads forward-compatible with older data that did not include a role yet.

### 3. Workspace Adapter Assigns Roles Explicitly

The workspace-to-window adapter now assigns window roles deliberately:

- tile `main_view` maps to `DesktopWindowRole::Main`
- tile `status_view` maps to `DesktopWindowRole::Status` when no main view is present
- empty placeholder tiles remain `Main`

This establishes the semantic contract that later stories can use for shell chrome, overlay policy, and layer ordering.

## Rationale

Without explicit roles, future graphics work would have to reverse-engineer whether a surface is primary content, transient UI, or shell status. That is exactly the kind of implicit historical baggage PandaGen is trying to avoid.

By making role part of the desktop window contract now, later graphics stories can build policy on top of a stable mechanism rather than introducing one-off heuristics.

## Tests

Added and validated focused coverage in `services_gui_host/src/lib.rs`:

- `test_desktop_window_defaults_to_main_role`
- `test_workspace_snapshot_uses_status_role_when_tile_has_only_status_view`

Validation run:

- `cargo test -p services_gui_host`
- `cargo test --all`

## Files Changed

- `services_gui_host/src/lib.rs`
- `docs/path_to_graphics.md`
- `PHASE203_SUMMARY.md`
