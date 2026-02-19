# Phase 178 Summary: Runtime Boot Profile Activation

## What Changed
- Integrated boot profile loading into workspace runtime startup:
  - `services_workspace_manager/src/lib.rs`
  - `WorkspaceRuntime::new` now loads persisted `BootConfig` through `BootProfileManager` before attaching storage context to the workspace manager.
  - Added `WorkspaceRuntime::boot_config()` for deterministic inspection of loaded startup configuration.
- Added deterministic boot-profile application during runtime initialization:
  - `Workspace` profile keeps startup empty (no component auto-launch).
  - `Editor` profile launches an editor component at startup, optionally with configured `editor_file`.
  - `Kiosk` profile launches a tagged custom component with kiosk metadata (`boot.profile=kiosk`, `kiosk.app=<name>`).
- Added runtime integration tests:
  - `services_workspace_manager/tests/runtime_tests.rs`
  - Verifies startup behavior for persisted `Editor` and `Kiosk` profiles from storage.
  - Verifies default `Workspace` profile launches no components.

## Rationale
- Boot profile persistence existed but had no effect on runtime behavior, which left startup mode configuration effectively inert.
- Applying the loaded profile in `WorkspaceRuntime::new` closes that gap and makes stored startup policy immediately observable and testable.
- Keeping `Workspace` profile conservative (no implicit launches) avoids regressions in current workflows while enabling explicit profile-driven startup for `Editor` and `Kiosk`.

## Validation
- `cargo test -p services_workspace_manager` passed.
