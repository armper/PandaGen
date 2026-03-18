# Phase 186 Summary: `open custom <entry>` Command Path and Help Surface

## What Changed
- Extended command parsing in `services_workspace_manager/src/commands.rs`:
  - `open custom <entry>` is now a recognized `open` component type.
  - Parser now rejects `open custom` without an entry using explicit usage guidance.
- Extended command execution metadata routing in `cmd_open()`:
  - Added custom-open guard for missing/empty entry args.
  - For `ComponentType::Custom`, first arg is now mapped to:
    - `package.entry`
    - `custom.entry`
  - Existing positional metadata (`arg0`, `arg1`, ...) remains intact.
- Updated interactive CLI help in `services_workspace_manager/src/lib.rs`:
  - Added `custom` to the open component list.
  - Added explicit `open custom <entry> [args...]` usage line.

## Tests Added
- `test_parse_open_custom_command`
- `test_parse_open_custom_requires_entry`
- `test_execute_open_custom_sets_entry_metadata`

## Rationale
- Custom hosts became runtime-capable in the previous phase, but operator launch ergonomics from the command surface were incomplete.
- This phase closes that gap by providing a first-class custom launch command and deterministic metadata routing into the custom host resolver.

## Validation
- `cargo test -p services_workspace_manager` passed.
