# Phase 352: autosave and recent files (GFX-060)

## What changed

- **Autosave.** A named, changed document that sits still for
  `AUTOSAVE_IDLE_TICKS` (200 ticks, two seconds) saves itself through the
  ordinary `Save` effect. The footer says "Saved itself"; no notice card.
  Unnamed documents, and documents with a prompt or the history browser
  open, are left alone.
- **Recent files in the palette.** Opened or saved names go to the front
  of a five-long list (`.recent`, read with the look on the first desk
  frame). The palette shows them as "Open memo" rows ahead of the
  actions; Enter opens the file in a new Notepad.

## Why

With versions kept on every write, the cost of saving is zero and the
cost of not saving is the document. "Unsaved changes" dialogs are a
1970s artefact of saving being slow and destructive. The `*` in the
title is kept as a brief truthful signal, not a warning.

The palette is where the desk already answers "what next"; the files a
person touched last are the likeliest answer, so they sit at the top.

## Plumbing

- `Notepad::autosave_due(now)` is a small state machine: an edit marks
  `idle_since`; the first tick past the threshold yields `Save` once.
- `Desk::tick(now) -> Vec<DeskRequest>`; the kernel calls it every loop in
  desk mode and pushes the requests. `Desk::quiet_saves` remembers whose
  save should land without a notice.
- `Desk::io_done` now returns an optional follow-up request
  (`SaveRecent`) that the kernel pushes; `LoadLook` also reads `.recent`.
- `Palette::matches(has_window, has_notepad, recent)` returns
  `PaletteRow`s.

## Tests

- `notepad::tests::a_named_document_saves_itself_after_sitting_still`
- `desk::tests::a_still_document_saves_itself_quietly_and_the_palette_remembers_it`
- Machine: covered by the existing Notepad shapes (the quiet save does
  not change pixels the shapes pin); `cargo xtask gauntlet` exit 0.
