# Phase 193 Summary: Workspace Help Text Generated from Shared Command-Surface Specs

## What Changed
- Updated `HelpCategory::workspace_help()` in `services_workspace_manager/src/help.rs` to generate command lines from shared command-surface metadata and grammar:
  - launch command specs (`LAUNCH_COMMAND_SPECS`)
  - helper alias specs (`HELPER_COMMAND_SPECS`)
  - non-launch workspace descriptor specs (`NON_LAUNCH_PALETTE_SPECS`)
  - component-id grammar specs (`COMPONENT_ID_COMMAND_SPECS`)
  - shared help-topic usage grammar (`help_usage_pattern()`)
- Added shared help-usage formatter in `services_workspace_manager/src/command_surface.rs`:
  - `help_usage_pattern()` now derives `help [workspace|editor|keys|system]` from `HELP_TOPIC_SPECS`.
- Updated parser usage messaging in `services_workspace_manager/src/commands.rs`:
  - invalid help topic/path now reports usage derived from shared grammar (`help_usage_pattern()`), instead of hardcoded text.
- Added/extended tests to validate generated help output contains grammar-driven forms and aliases.

## Rationale
- `workspace_help` previously used static text that could drift from executable parser/registry grammar.
- Generating help content from shared command-surface specs keeps operator-facing docs aligned with actual command behavior, aliases, and usage forms.

## Validation
- `cargo test -p services_workspace_manager` passed:
  - unit tests: `188 passed; 0 failed`
  - integration tests: `22 passed; 0 failed`
  - runtime tests: `11 passed; 0 failed`
