# Phase 183 Summary: File Picker Launch Now Fails Fast with Actionable Errors

## What Changed
- Reworked `WorkspaceManager::launch_component` in `services_workspace_manager/src/lib.rs` to validate launch prerequisites before creating component records/views:
  - Added `validate_launch_config(&LaunchConfig)` preflight checks.
  - For `ComponentType::FilePicker`, launch now requires:
    - editor/storage context to be present
    - root directory context to be present
- Added a new workspace error variant:
  - `WorkspaceError::MissingLaunchContext { component_type, reason }`
  - Integrated into actionable error formatting via `actionable_message()` and `format_with_actions()`.
- Removed `FilePicker` fallback to `ComponentInstance::None`:
  - Launch now returns a concrete error instead of creating a running shell with no backing instance.
- Updated command-surface feedback:
  - `cmd_open_file_picker()` now reports `err.format_with_actions()` to return recovery hints directly in command output.

## Tests Updated
- `services_workspace_manager/src/lib.rs`
  - `test_launch_file_picker_without_storage` now expects `MissingLaunchContext` and zero created components.
  - Added `test_launch_file_picker_without_root_context`.
  - Added `test_actionable_error_missing_launch_context_for_file_picker`.
- `services_workspace_manager/src/commands.rs`
  - Added `test_execute_open_file_picker_without_storage_context`.
  - Added `test_execute_open_file_picker_with_storage_context`.

## Rationale
- The previous behavior created a running `FilePicker` component even when required context was missing, which produced a non-functional instance and misleading runtime state.
- Failing fast with explicit, actionable errors improves determinism, correctness, and operator visibility.

## Validation
- `cargo test -p services_workspace_manager` passed.
