# Phase 199 Summary: Shared Prompt Suggestion Derivation for Workspace Commands

## What Changed
- Added shared non-launch prompt suggestion helpers in `services_workspace_manager/src/command_surface.rs`:
  - `component_id_usage_pattern()`
  - `non_launch_prompt_suggestion_by_id()`
  - `non_launch_prefix_suggestions()`
  - canonical prompt-pattern derivation for:
    - `list`
    - `focus_next` -> `next`
    - `focus_prev` -> `prev`
    - `close` -> `close <component_id>`
- Updated `services_workspace_manager/src/workspace_status.rs`:
  - `generate_suggestions()` now consumes shared non-launch prefix suggestions instead of hand-maintained literals for `list`, `next`, `prev`, and `close`
  - empty-input suggestions now pull `list` from the shared command surface instead of a static literal
  - added duplicate-safe prompt suggestion insertion helpers
- Updated `services_workspace_manager/src/help.rs`:
  - workspace help now uses the same canonical prompt-pattern helpers for workspace commands and component-id usage display
- Marked item 29 complete in `TODO_HIGH_VALUE_RANKING.md`

## Rationale
- Prompt suggestions were one of the last places where workspace command grammar still drifted away from the shared command surface.
- `generate_suggestions()` contained manual string literals for navigation and close commands, while parser/help/registry behavior was already increasingly centralized.
- Moving these patterns into shared derivation keeps prompt UX aligned with parser grammar and reduces the chance of future behavior/help/suggestion mismatches.

## Test Coverage
- Added command-surface test coverage:
  - `test_workspace_non_launch_suggestion_patterns_are_shared`
- Added workspace-status suggestion coverage:
  - `test_generate_suggestions_navigation_prefixes_use_shared_patterns`
  - `test_generate_suggestions_close_prefix_uses_component_id_grammar`
- Existing suggestion/help/runtime tests continue to validate deterministic behavior and user-visible command flows.

## Validation
- `cargo test -p services_workspace_manager`
- `cargo test --all`

## Residual Notes
- `TODO_HIGH_VALUE_RANKING.md` now has no remaining unchecked ranked items.
- Workspace-wide warnings still exist outside this change set (notably `third_party/serde_core` future-compat warnings), but this phase introduces no new failures.
