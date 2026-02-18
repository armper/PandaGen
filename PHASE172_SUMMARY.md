# Phase 172 Summary: Workspace Settings Persistence Wiring

## What Changed
- Implemented real settings persistence in `services_workspace_manager/src/lib.rs`:
  - `save_settings()` now serializes overrides and writes them transactionally to `JournaledStorage`.
  - `load_settings()` now reads persisted bytes transactionally, uses `load_overrides_safe()` for corruption-safe import, and applies loaded user overrides.
- Added deterministic settings storage location support:
  - Canonical path constant: `settings/user_overrides.json`.
  - Deterministic object fallback ID when fs-view/root path capabilities are not present.
  - Optional fs-view integration now creates/links `settings/user_overrides.json` when needed.
- Added workspace manager tests for:
  - Save/load round-trip with storage context.
  - Corrupt persisted settings recovery behavior.
  - fs-view path link creation for persisted settings.
- Updated `TODO_HIGH_VALUE_RANKING.md` item 5 to completed.

## Rationale
- Previous behavior only validated serialization and reported success without writing data.
- This phase closes the persistence gap while preserving capability-driven behavior:
  - Uses explicit storage context (`EditorIoContext`) only.
  - Avoids ambient global filesystem assumptions.
  - Keeps load deterministic and resilient to malformed/corrupted bytes.

## Test Notes
- Added tests in `services_workspace_manager/src/lib.rs`:
  - `test_settings_save_load_roundtrip_with_storage_context`
  - `test_settings_load_recovers_from_corrupt_data`
  - `test_settings_save_links_path_when_fs_view_available`
- Attempted validation commands:
  - `cargo test -p services_workspace_manager`
  - `cargo test -p services_workspace_manager --lib`
- Current workspace has unrelated pre-existing compile failures in `sim_kernel` that block crate test execution:
  - missing `syscall_gate` field in `SimulatedKernel` initializer
  - borrow-check error in scheduler EDF dequeue closure
