# Phase 423: typing is undone a word at a time (KBD-015)

## What changed

Every key typed in the Notepad was its own undo step, and every step held
a copy of the whole document (`Vec<(TextBuffer, Position)>`, capped at a
hundred). Undoing a sentence took a keystroke per letter, a hundred steps
reached back barely two lines, and a long document held a hundred copies
of itself to do it (Phases 88 and 90 noted the full copies).

Now a run of typing is one step:

- A printable key typed exactly where the last one left the caret joins
  the step; a space after a word starts the next, so "hello big world"
  undoes as " world", " big", "hello".
- Any other key ends the run -- an arrow, even one that comes straight
  back; Backspace, Delete, Enter and Tab are steps of their own; undo and
  redo end it too. So does an edit in another card on the same document
  (`push_undo` ends it for everyone).
- The joined keys still mark the document edited (unsaved, due its
  autosave, and forgetting redo): `mark_edited` is that half of
  `push_undo`, split out.

The hundred-step cap now reaches a hundred words back, not a hundred
letters, and holds as many fewer copies for the same text.

## Tests

- `typing_is_undone_a_word_at_a_time`: word steps; a caret move ends a
  step even when it comes back; Backspace is its own step; typing still
  marks the document unsaved.
- `redo_brings_back_what_undo_took_and_a_new_edit_forgets_it` and
  `undo_restores_what_the_last_edit_changed_and_no_ops_do_not_count` type
  words now, and keep their meaning.
- `cargo xtask gauntlet`: exit 0.
