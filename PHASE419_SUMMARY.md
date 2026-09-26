# Phase 419: the Notepad finds any case, finds backwards, and redoes (KBD-014)

## What changed

- **Smart-case Find and Replace.** A query with no capitals matches any
  case; one with a capital matches exactly. Find was case-sensitive only,
  so "todo" missed every "TODO". The match count, Replace and Replace all
  use the same rule. ASCII folding keeps every byte offset, so selections
  land on the text as written.
- **Find backwards.** In the Find prompt, Up (or the new "Back" chip) goes
  to the previous match, wrapping; Down and Enter go forward.
- **Find starts at the caret.** Typing a query used to skip a match sitting
  right at the caret -- at the top of a freshly opened document the first
  line was never found first. The incremental search now starts at the
  caret itself (`find_from_here`), as Replace's already did. The old test
  that pinned the skip was corrected.
- **Redo.** Ctrl+Shift+Z brings back what Ctrl+Z took (Ctrl+Y was already
  the history browser). The parser delivers Ctrl+Shift+Z as its own byte
  (`KEY_CTRL_SHIFT_Z`, 0xA2), since the control byte cannot carry Shift.
  A new edit forgets what was undone, as in every editor.
- **Palette.** "Undo" and "Redo" rows, so both are reachable with the
  mouse alone.

## Why

Phases 72, 74 and 88 left Find forward-only and case-sensitive and the
Notepad without redo; these were the gaps a writer meets first.

## Tests

- `redo_brings_back_what_undo_took_and_a_new_edit_forgets_it`
- `find_is_smart_case_and_goes_back_with_up`
- `find_back_on_one_line_takes_the_earlier_match`
- `replace_all_is_smart_case_too` (also `rfind_in` on multi-byte text)
- `find_selects_the_next_match_incrementally_and_wraps`: now expects the
  match at the caret first.
- `notepad_chips_follow_its_mode`: the Back chip.
- QEMU by hand: type, Ctrl+Z twice, Ctrl+Shift+Z, then Find "todo" finds
  "Todo" and "todo" (2 found).
- `cargo xtask gauntlet`: exit 0.
