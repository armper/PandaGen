# Phase 188 Summary: Command Palette Launch Descriptor Parity

## What Changed
- Updated launch command descriptors in `services_workspace_manager/src/command_registry.rs`:
  - `open_editor` now keeps prompt guidance (`open editor `) without requiring args, matching parser behavior where editor args are optional.
  - Added `open_cli` descriptor with prompt guidance `open cli `.
  - Added `open_pipeline` descriptor with prompt guidance `open pipeline `.
- Expanded registry tests in `services_workspace_manager/src/command_registry.rs`:
  - Added `test_registry_has_open_cli`.
  - Added `test_registry_has_open_pipeline`.
  - Updated `test_registry_parametric_commands` to assert prompt/argument semantics for `open_editor`, `open_cli`, `open_pipeline`, and `open_custom`.

## Rationale
- Command mode previews and discovery are driven by registry descriptors, while actual execution semantics come from parser/executor logic.
- Descriptor metadata needed to reflect real accepted syntax to avoid misleading prompts and incomplete launch discovery.

## Validation
- `cargo test -p services_workspace_manager` passed:
  - unit tests: `175 passed; 0 failed`
  - integration tests: `22 passed; 0 failed`
  - runtime tests: `11 passed; 0 failed`
