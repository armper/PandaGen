# Phase 350: Notepad history (GFX-058)

## What changed

Every save already kept the content it replaced as a version (Phase 349,
five deep, bounded). This phase makes those versions visible and useful
from the Notepad itself.

- **History chip / Ctrl+Y** on a named document asks the kernel for the
  newest kept version and shows it in place of the text. The title reads
  `memo - earlier version`; the footer reads `History: 1 of 3 earlier
  2026-09-22 16:26   <- older   -> newer   Enter restores   Esc back`.
- **Left/Up** steps to an older version, **Right/Down** to a newer one;
  past the newest is the document itself, with any unsaved edit intact.
  **Esc** puts the document back from anywhere.
- **Enter restores by saving**: the shown text becomes the current content
  through the ordinary `Save` effect, so what it replaced becomes a
  version. Nothing is lost in either direction.
- Typing does nothing while browsing; the browser is read-only by
  construction, not by a flag on every edit path.
- The palette lists "Earlier versions of this document" with `Ctrl+Y`.

## Why this shape

A "versions" dialog would be a third place to look. The document *is* the
view: browsing swaps the buffer and the footer says where you are. Restore
being a save keeps one code path for "content changed on disk" and gives
the restore its own undo (the previous current is now version 0).

## Plumbing

- `NotepadEffect::Version { path, index }` → `DeskRequest::ReadVersion` →
  kernel reads through `BareMetalEditorIo::{list_versions, read_version}`
  → `Desk::version_loaded` → `Notepad::show_version(index, total,
  content, when)`. `total == 0` closes the browser with "No earlier
  versions yet".
- `BareMetalFilesystem::list_versions(name)` returns the `VersionRecord`s
  (when current, size), newest first.

## Tests

- `notepad::tests::history_browses_kept_versions_and_enter_restores_by_saving`
  walks the whole flow on the host: open, older, newer, past-newest,
  Esc, Enter → `Save`, `io_done` → "Restored", and the empty case.
- `desk::tests::the_history_chip_asks_the_kernel_and_the_answer_reaches_the_notepad`
  pins the request/reply routing and that Enter yields an `Io { Save }`.
- Machine: the gauntlet's Notepad history shape saves three contents,
  opens history, steps older, restores, and asserts the "Saved" notice
  card by pixel. `cargo xtask gauntlet` exit 0.
