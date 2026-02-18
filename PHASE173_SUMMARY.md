# Phase 173 Summary: Real Focused-Editor Save Action

## What Changed
- Replaced placeholder workspace save behavior with real editor save wiring.
- Added `Editor::save_current_document()` in `services_editor_vi/src/editor.rs` as a public, direct save entrypoint for workspace-level actions.
- Updated `Action::Save` in `services_workspace_manager/src/lib.rs` to:
  - Require a focused component.
  - Require that the focused component is an editor.
  - Call the focused editor instance save path (`save_current_document`).
  - Publish refreshed editor views after save.
  - Emit explicit failure status messages instead of reporting settings-save success.
- Added workspace manager tests:
  - `test_action_save_clears_dirty_editor_state`
  - `test_action_save_fails_when_focused_component_is_not_editor`
- Marked TODO item #6 as complete in `TODO_HIGH_VALUE_RANKING.md`.

## Rationale
- `Ctrl+S` / `Action::Save` should save the active document, not unrelated workspace settings.
- The previous placeholder could report success even when no document save occurred, which is misleading and risks data loss.
- Exposing a narrow, explicit editor save method keeps save semantics capability-driven and testable without simulating command-mode key sequences.

## Test Notes
- Attempted:
  - `cargo test -p services_workspace_manager`
- Validation remains blocked by unrelated pre-existing compile failures in `sim_kernel` (outside this change set), so full test execution could not complete in this environment.
