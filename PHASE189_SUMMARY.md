# Phase 189 Summary: Command Surface Parity for Recent and File Picker Flows

## What Changed
- Extended command parsing in `services_workspace_manager/src/commands.rs`:
  - Added executable helper aliases:
    - `recent`
    - `recent files`
    - `open recent`
    - `open recent files`
    - `open file`
    - `open file-picker`
  - Enforced usage validation for file-picker alias forms with extra args.
  - Canonicalized history formatting for recent command output to `recent`.
- Aligned prompt suggestions and validation in `services_workspace_manager/src/workspace_status.rs`:
  - Added `open file` to open-prefix and empty-input suggestions.
  - Added `open recent` to recent-prefix suggestions.
  - Updated `validate_command()` to mirror parser semantics for:
    - helper aliases (`open file`, `open file-picker`, `open recent`, `recent files`)
    - `open editor` as valid complete
    - `open custom` as valid prefix and `open custom <entry>` as valid complete.
- Tightened command-palette parity in `services_workspace_manager/src/command_registry.rs`:
  - Added `open_file_picker` descriptor with prompt pattern `open file`.
  - Added canonical prompt pattern `recent` to the recent-files descriptor.

## Tests Added/Updated
- `services_workspace_manager/src/commands.rs`:
  - `test_parse_recent_command_variants`
  - `test_parse_open_file_picker_command_variants`
  - `test_parse_open_file_picker_rejects_extra_args`
- `services_workspace_manager/src/workspace_status.rs`:
  - Updated open-prefix suggestion and open-validation expectations.
  - Added `test_validate_command_recent`.
- `services_workspace_manager/src/command_registry.rs`:
  - Added `test_registry_has_open_file_picker`.
  - Extended parametric/prompt-pattern assertions for `open_file_picker` and `recent`.

## Rationale
- Users were shown helper variants (`recent`, `open recent`) that were not always parseable, and parseable helper flows (`open file`) were not consistently surfaced.
- This phase aligns parser, prompt validation, and suggestions so command discovery matches execution behavior and avoids misleading UI guidance.

## Validation
- `cargo test -p services_workspace_manager` passed:
  - unit tests: `180 passed; 0 failed`
  - integration tests: `22 passed; 0 failed`
  - runtime tests: `11 passed; 0 failed`
