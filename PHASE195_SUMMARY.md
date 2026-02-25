# Phase 195 Summary: Canonical Help-Topic Parsing via Shared Command Surface

## What Changed
- Updated `HelpCategory::parse()` in `services_workspace_manager/src/help.rs`:
  - now delegates to shared `command_surface::parse_help_topic()`
  - removed duplicated per-alias matching logic from `help.rs`
- Updated shared help-topic parser in `services_workspace_manager/src/command_surface.rs`:
  - `parse_help_topic()` now performs case-insensitive alias matching
  - added explicit `overview` token support alongside existing empty-input handling
- Existing parser/validation call sites continue to use `parse_help_topic()` as the canonical decoder.

## Rationale
- Help-topic alias decoding existed in more than one place (`help.rs` and `command_surface`), creating drift risk.
- Consolidating parsing behavior behind `parse_help_topic()` gives one canonical source for help-topic alias semantics.

## Validation
- `cargo test -p services_workspace_manager` passed:
  - unit tests: `188 passed; 0 failed`
  - integration tests: `22 passed; 0 failed`
  - runtime tests: `11 passed; 0 failed`
