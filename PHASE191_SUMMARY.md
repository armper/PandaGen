# Phase 191 Summary: Fully Data-Driven Palette Registry for Non-Launch Commands

## What Changed
- Extended shared command-surface metadata in `services_workspace_manager/src/command_surface.rs`:
  - Added `NON_LAUNCH_PALETTE_SPECS` to describe all previously manual registry descriptors:
    - workspace navigation/control: `list`, `focus_next`, `focus_prev`, `close`
    - help descriptors: `help`, `help_workspace`, `help_editor`, `help_keys`, `help_system`
    - editor/system descriptors: `save`, `quit`
    - boot profile descriptors: `boot_profile_show`, `boot_profile_set`, `boot_profile_save`
  - Each descriptor now carries category, tags, keybinding hints, prompt patterns, and arg requirements in one shared spec.
- Simplified `services_workspace_manager/src/command_registry.rs`:
  - Removed duplicated literal registration blocks for all non-launch descriptors above.
  - Registry now composes from:
    - `LAUNCH_COMMAND_SPECS`
    - `HELPER_COMMAND_SPECS`
    - `NON_LAUNCH_PALETTE_SPECS`
  - Reused common registration helper to build command descriptors from shared metadata.

## Rationale
- Registry metadata for non-launch commands was still duplicated and manually maintained after the initial command-surface introduction.
- Centralizing these descriptors in `command_surface` reduces drift risk and keeps palette behavior aligned with the workspace command model.

## Validation
- `cargo test -p services_workspace_manager` passed:
  - unit tests: `182 passed; 0 failed`
  - integration tests: `22 passed; 0 failed`
  - runtime tests: `11 passed; 0 failed`
