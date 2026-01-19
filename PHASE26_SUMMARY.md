# Phase 26: Persistent Workspace Sessions (Save/Restore Component Graph)

**Completion Date**: 2026-01-19

## Overview

Phase 26 adds **persistent workspace session snapshots**, enabling save/restore of the component graph, focus state, and last published view frames. This allows deterministic session restoration without POSIX-style process snapshots.

## What Was Added

### 1. Session Snapshot Types

**New serializable snapshot structs:**
- `WorkspaceSessionSnapshot`
- `WorkspaceComponentSnapshot`
- `WorkspaceSessionFormat`

Snapshots include:
- Component IDs, types, names, metadata
- Identity kind + trust domain + budget
- Component state + exit reason
- Focused component
- Latest main/status `ViewFrame`s

### 2. WorkspaceManager Save/Restore

**`save_session()`**
- Captures focused component
- Captures all components (sorted by ID for determinism)
- Includes latest view frames per component

**`restore_session()`**
- Clears live state and rehydrates component graph
- Recreates views/subscriptions
- Publishes last known frames (view IDs remapped)
- Restores focus when possible

### 3. Frame Remapping

`remap_frame()` ensures restored frames:
- Use new view IDs
- Maintain content and cursor
- Enforce monotonic revision (>= 1)

## Tests Added

- `test_save_restore_session` (workspace save/restore flow)

All existing workspace tests remain green.

## Design Decisions

- **No process image snapshotting**: restore builds fresh components with preserved metadata.
- **Deterministic ordering**: component snapshots are sorted by ID.
- **View remapping**: view IDs are regenerated while preserving frame content.

## Files Changed

**Modified Files:**
- `services_workspace_manager/src/lib.rs`

## Future Work

- Store and restore input focus stacks
- Serialize audit trails (optional)
- Add session persistence to disk via storage service

## Conclusion

Phase 26 makes workspace sessions first-class, enabling durable, testable save/restore without legacy OS assumptions.
