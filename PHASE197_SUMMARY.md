# Phase 197 Summary: Workspace Stabilization After Render Snapshot Expansion

## What Changed
- Updated `services_remote_ui_host` test coverage to match the expanded `WorkspaceRenderSnapshot` contract introduced by the workspace layout work:
  - added reusable snapshot fixtures that populate `composed_main_view`, `composed_status_view`, `layout`, and `tiles`
  - added a JSON-line round-trip test to verify remote snapshot serialization preserves layout-aware fields
  - added `view_types` as a dev-dependency for fixture construction
- Fixed `kernel_bootstrap` host-build drift:
  - widened the `EditorMode` debug import in `src/main.rs` so host debug builds compile after incremental-render assertions reference editor mode
  - corrected bare-metal storage tests to use mutable filesystem handles only where required
- Fixed Cargo manifest correctness in `services_workspace_manager/Cargo.toml`:
  - moved `hashbrown` out of unsupported `target.'cfg(not(feature = "std"))'.dependencies`
  - declared it as a normal dependency so Cargo feature resolution matches the crate’s `#[cfg(not(feature = "std"))]` usage

## Rationale
- Phase 196 expanded render snapshots for split/tab composition, but downstream integration tests in `services_remote_ui_host` were still constructing the old snapshot shape, breaking workspace-wide build/test validation.
- `kernel_bootstrap` had compile gating that only imported `EditorMode` for bare-metal targets even though debug assertions referenced it in host builds as well.
- Cargo was explicitly warning that the workspace-manager manifest used unsupported feature-based target dependency selection, which made the manifest semantically misleading.
- Restoring a clean workspace build/test baseline is higher value than continuing feature work because it re-establishes the project’s core testability promise.

## Test Coverage
- Added/updated tests in `services_remote_ui_host/src/lib.rs`:
  - `test_remote_ui_host_revision_increments`
  - `test_ipc_snapshot_sink_sends_message`
  - `test_json_line_sink_round_trips_layout_snapshot_fields`
- Existing `kernel_bootstrap` bare-metal storage tests now compile and run again as part of crate and workspace test suites.

## Validation
- `cargo test -p services_remote_ui_host`
- `cargo test -p kernel_bootstrap --lib --tests`
- `cargo test -p services_workspace_manager`
- `cargo build`
- `cargo test --all`

## Residual Notes
- Workspace build/test is restored, but there are still non-failing compiler warnings in `kernel_bootstrap` and `third_party/serde_core` that should be addressed separately if the goal becomes warning-free or clippy-clean workspace builds.
