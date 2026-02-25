# Phase 190 Summary: Shared Command-Surface Spec for Parser, Palette, and Prompt

## What Changed
- Added `services_workspace_manager/src/command_surface.rs` as a shared command-spec layer:
  - launch command specs (`open editor|cli|pipeline|custom`) with component mapping, arg requirements, and palette metadata.
  - helper command specs (`recent*`, `open recent*`, `open file*`) with alias patterns, usage guidance, and palette metadata.
  - shared suggestion specs and valid-prefix groups used by prompt UX.
  - shared open-command validation logic (`validate_open_invocation`).
- Updated parser in `services_workspace_manager/src/commands.rs` to consume shared specs:
  - helper alias resolution now comes from `helper_command_by_alias`.
  - launch target resolution now comes from `launch_command_by_token`.
  - invalid helper-open forms now use shared usage guidance.
- Updated registry in `services_workspace_manager/src/command_registry.rs`:
  - launch/helper descriptors are now generated from shared palette spec metadata instead of duplicated literal definitions.
- Updated prompt generation/validation in `services_workspace_manager/src/workspace_status.rs`:
  - suggestions now use shared suggestion specs.
  - `open ...` validation now uses shared open-invocation validator.
  - command-prefix fallback now uses shared prefix groups.

## Tests Added/Updated
- Added new shared-spec tests in `services_workspace_manager/src/command_surface.rs`:
  - `test_helper_alias_resolution`
  - `test_open_validation`
- Existing parser/registry/workspace-status tests now validate behavior through the shared spec wiring.

## Rationale
- The same command surface was being defined in multiple places (parser, palette metadata, and prompt validation), creating drift risk.
- Centralizing launch/helper command semantics and descriptor metadata reduces mismatch risk and keeps command discovery aligned with actual execution.

## Validation
- `cargo test -p services_workspace_manager` passed:
  - unit tests: `182 passed; 0 failed`
  - integration tests: `22 passed; 0 failed`
  - runtime tests: `11 passed; 0 failed`
