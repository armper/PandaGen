# Phase 194 Summary: Overview/System Help Text Synced to Shared Command Surface

## What Changed
- Updated `HelpCategory::overview_help()` in `services_workspace_manager/src/help.rs`:
  - now generates help-topic lines from shared `HELP_TOPIC_SPECS`.
  - now includes shared grammar usage line from `help_usage_pattern()`.
- Updated `HelpCategory::system_help()` in `services_workspace_manager/src/help.rs`:
  - now generates system command lines from shared `NON_LAUNCH_PALETTE_SPECS` (`System` category).
  - excludes recursive `help_system` descriptor while keeping boot-profile and other system descriptors synchronized.
  - removed stale static entries that were not backed by current command-surface metadata.
- Added small shared helper in `services_workspace_manager/src/help.rs`:
  - `topic_description()` to keep overview topic descriptions deterministic and centralized.
- Updated help tests in `services_workspace_manager/src/help.rs`:
  - overview assertions now validate shared usage/topic rendering.
  - system assertions now validate boot-profile command presence and stale-command removal.

## Rationale
- `Overview` and `System` help content still contained static text and stale command references, creating drift from executable/shared command definitions.
- Rendering from shared command-surface metadata keeps human-facing docs aligned with real command grammar and reduces future maintenance risk.

## Validation
- `cargo test -p services_workspace_manager` passed:
  - unit tests: `188 passed; 0 failed`
  - integration tests: `22 passed; 0 failed`
  - runtime tests: `11 passed; 0 failed`
