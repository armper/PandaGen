# Phase 382: find and replace in Notepad (GFX-090)

## What changed

- Ctrl+R (and a Replace chip, and a palette row) opens a two-field
  prompt in the footer: what to find, what to put there. Tab switches
  fields. Typing in the find field selects the first match as it goes,
  like Find. Enter replaces the selected match and selects the next;
  the All chip replaces every match. Each is one undo step.
- A one-line selection seeds the find field, and the caret then starts
  in the replacement field.
- The prompt's footer shows how many matches there are and what the
  last action did ("Replaced 2").
- While the prompt is open the chips are Replace, All and Close.

## Tests

- `notepad::tests::find_and_replace_one_at_a_time_or_all_at_once`
- `desk::tests::header_chips_run_the_apps_own_keys` (the new chip)
- Machine: `cargo xtask gauntlet` exit 0.
