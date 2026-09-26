# Phase 425: the host simulator types what the keys say and lists what runs (HOST-004)

## What changed

`pandagend`, the host-side simulator, had two early placeholders:

- **Typing in host control mode** mapped keys through its own table:
  lowercase letters only, Shift ignored, most punctuation missing, and
  `;` typed `:`. `input_types::KeyCode::to_char(shift)` is now the one US
  table for turning a key into text -- letters, digits and their shifted
  symbols, all punctuation keys, the keypad -- and the simulator uses it.
- **`list`** was parsed and accepted, then did nothing ("List will be
  rendered in status (future enhancement)"). It now says what runs: each
  running component's name and kind, the focused one marked, sorted by
  name; printed by the host, put in the status line, and kept for
  embedders and tests (`HostRuntime::last_listing`).

## Not done, and why

`services_workspace_manager` records a settings change (theme, tab size,
line numbers) in its status line and applies none of it: applying needs a
theme system and live editor configuration in that crate. It is host-only
-- the booted machine runs `kernel_bootstrap`'s own workspace and desk,
where the look is applied and kept (Phase 389 onwards) -- so it was left
as it is rather than grown for the simulator alone.

## Tests

- `keys_type_their_characters_with_and_without_shift` (`input_types`).
- `list_says_what_runs` (`pandagend`): one more line after opening an
  editor, and it is the focused one.
- `cargo xtask gauntlet`: exit 0.
