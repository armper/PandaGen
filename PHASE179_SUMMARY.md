# Phase 179 Summary: Boot Profile Command Surface

## What Changed
- Added boot profile commands to workspace command model and parser:
  - `services_workspace_manager/src/commands.rs`
  - New commands: `BootProfileShow`, `BootProfileSet { profile }`, `BootProfileSave`
  - New CLI syntax:
    - `boot profile show`
    - `boot profile set <workspace|editor|kiosk>`
    - `boot profile save`
- Added boot profile management methods to `WorkspaceManager`:
  - `services_workspace_manager/src/lib.rs`
  - `boot_profile_config()`, `set_boot_profile()`, `load_boot_profile()`, `save_boot_profile()`
  - `set_editor_io_context()` now loads persisted boot profile when storage is attached.
- Updated command discovery and docs:
  - `services_workspace_manager/src/command_registry.rs`
  - Added command palette descriptors for boot profile show/set/save.
  - `services_workspace_manager/src/help.rs`
  - Extended system help with boot profile command references.
- Added tests:
  - Parser coverage for new boot profile commands.
  - Execution coverage for set/show.
  - Persistence coverage for save + reload from storage context.
  - Command registry coverage for boot profile command presence.

## Rationale
- Runtime boot profile activation was implemented, but there was no first-class command path for users to inspect/update/persist startup mode.
- This change closes that control-plane gap and keeps startup policy management explicit, typed, and testable within the workspace command surface.

## Validation
- `cargo test -p services_workspace_manager` passed.
