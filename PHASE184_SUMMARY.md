# Phase 184 Summary: Resilient Package Launch with Structured Per-Component Failures

## What Changed
- Upgraded package launch output in `services_workspace_manager/src/lib.rs`:
  - Added `PackageLaunchFailure` (component name/type/entry + `WorkspaceError`).
  - Added `PackageLaunchReport` (`created_component_ids`, `failures`, `is_success()`).
- Changed `WorkspaceManager::launch_package()` behavior:
  - Previously: returned `Result<Vec<ComponentId>, WorkspaceError>` and aborted on first component launch failure.
  - Now: returns `Result<PackageLaunchReport, WorkspaceError>`, continues launching remaining components, and records structured failures per component.
  - Manifest parsing/launch-plan construction errors still return `Err(WorkspaceError::InvalidCommand(...))`.
- Added partial-failure coverage:
  - `test_launch_package_reports_partial_failures` uses a selective policy that denies only CLI spawn, validating one success + one captured failure in the report.
- Updated existing package launch test to assert report semantics:
  - `test_launch_package_components` now checks `report.is_success()`, created IDs, and empty failure list.

## Rationale
- Package startup should not fail atomically when one component is unavailable or denied.
- Structured failure capture enables graceful degradation and deterministic post-launch inspection while preserving strict failure for invalid manifests.

## Validation
- `cargo test -p services_workspace_manager` passed.
