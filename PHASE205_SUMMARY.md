# Phase 205: Explicit Desktop Layer Policy

**Completion Date**: 2026-03-17

## Overview

Phase 205 implements `GFX-009` by making window layering a first-class part of the desktop contract in `services_gui_host`. The compositor no longer treats `z_index` as the only ordering mechanism; it now applies an explicit layer policy for workspace surfaces, overlays, palettes, notifications, modals, and future system surfaces.

This keeps PandaGen aligned with its clean-slate direction: desktop ordering policy is encoded in typed state instead of being hidden in incidental numeric conventions.

## What Changed

### 1. Added `DesktopWindowLayer`

`services_gui_host` now defines a `DesktopWindowLayer` enum with canonical layer classes:

- `Workspace`
- `Overlay`
- `Palette`
- `Notification`
- `Modal`
- `System`

`DesktopWindow` now carries a `layer` field with a serde default.

### 2. Role-To-Layer Policy Is Explicit

`DesktopWindowRole` now maps to a default layer through `DesktopWindowLayer::for_role(...)`.

- `Main` and `Status` map to `Workspace`
- `Overlay` maps to `Overlay`
- `Palette` maps to `Palette`
- `Notification` maps to `Notification`
- `Modal` maps to `Modal`

This turns previous layering convention into an explicit, reusable policy.

### 3. Composition Sorts By Layer Then Local Z

`Compositor::compose_desktop(...)` now sorts windows by:

1. canonical desktop layer
2. per-layer `z_index`
3. `ViewId` for deterministic tie-breaking

That means a notification or modal can reliably appear above workspace content even if its local `z_index` is lower.

## Rationale

Before this phase, PandaGen had semantic window roles and visible tab chrome, but desktop ordering was still effectively a flat numeric stack. That would have made later shell work brittle because every overlay or system surface would need to coordinate raw z-values manually.

The new policy separates two concerns cleanly:

- layer class decides broad desktop ordering
- `z_index` decides local order within that class

That is the right mechanism boundary for a modern compositor.

## Tests

Added and validated focused coverage in `services_gui_host/src/lib.rs`:

- `test_notification_role_assigns_notification_layer`
- `test_compose_desktop_layer_policy_beats_raw_z_index`

Validation run:

- `cargo test -p services_gui_host`
- `cargo test --all`

## Files Changed

- `services_gui_host/src/lib.rs`
- `docs/path_to_graphics.md`
- `PHASE205_SUMMARY.md`
