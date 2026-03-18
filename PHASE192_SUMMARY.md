# Phase 192 Summary: Shared Non-Launch Grammar for Parser, Validator, and Help Execution

## What Changed
- Extended shared command-surface grammar in `services_workspace_manager/src/command_surface.rs`:
  - Added component-target command specs (`focus`, `close`, `status`) with explicit usage contracts.
  - Added help topic specs (`workspace`, `editor`, `keys`/`keyboard`/`shortcuts`, `system`) and shared help invocation validation.
  - Added shared help suggestions metadata used by prompt suggestions.
- Updated parser and execution flow in `services_workspace_manager/src/commands.rs`:
  - Added `WorkspaceCommand::Help { category: HelpCategory }`.
  - Parser now uses shared grammar to resolve help topics and component-target command requirements.
  - Added `cmd_help()` to render category help via `HelpCategory::content()`.
  - Component-target parsing now uses shared usage metadata rather than hardcoded command-name formatting.
- Updated prompt UX wiring in `services_workspace_manager/src/workspace_status.rs`:
  - Help suggestions now come from shared `HELP_PREFIX_SUGGESTIONS`.
  - `validate_command()` now delegates help and component-target validation to shared grammar functions.
- Updated CLI help execution path in `services_workspace_manager/src/lib.rs`:
  - Removed hardcoded `if trimmed == "help"` command list block.
  - `help` now executes through parser + `WorkspaceCommand::Help`, sharing one command path.
- Updated `services_workspace_manager/src/help.rs`:
  - `HelpCategory` now derives `Serialize`/`Deserialize` to support command serialization.

## Tests Added/Updated
- `services_workspace_manager/src/command_surface.rs`:
  - `test_component_id_validation`
  - `test_help_validation_and_parse`
- `services_workspace_manager/src/commands.rs`:
  - `test_parse_help_command_variants`
  - `test_parse_help_invalid_topic`
  - `test_parse_focus_requires_component_id_only`
  - `test_execute_help_workspace`
- `services_workspace_manager/src/workspace_status.rs`:
  - extended help validation coverage for alias form (`help keyboard`).

## Rationale
- Non-launch grammar behavior (help forms and component-id argument contracts) still lived in duplicated parser/validator logic.
- Consolidating these contracts in shared command-surface grammar reduces drift and makes CLI execution and prompt validation behavior consistent.

## Validation
- `cargo test -p services_workspace_manager` passed:
  - unit tests: `188 passed; 0 failed`
  - integration tests: `22 passed; 0 failed`
  - runtime tests: `11 passed; 0 failed`
