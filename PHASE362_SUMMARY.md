# Phase 362: clipboard history, history diff, `xtask wallpaper` (GFX-070)

## What changed

- **Clipboard history.** `Desk::clipboard_history` keeps the last ten
  copied texts, newest first, deduplicated. Over a focused Notepad the
  palette lists them as `Paste: …` rows (`clipboard`); Enter sets the
  clipboard to that text and runs Ctrl+V through the ordinary path.
- **Diff in History.** While browsing a version, `viewport_selection`
  returns full-width spans for every visual row whose line is not in the
  current document, and the footer says `N lines differ`.
- **`cargo xtask wallpaper <picture.ppm>`** quantises a 1280x800 P6 to 256
  colours (median cut + Floyd-Steinberg) and writes
  `kernel_bootstrap/assets/wallpaper.{pal,idx}`.

## Why

Copy/paste with one slot loses work the moment a second thing is copied;
the palette already lists recent things, so recently copied text belongs
there. The history browser answered "what was it?" but not "what
changed?"; reusing the selection fill keeps the compositor unchanged.
The wallpaper recipe was a Python one-liner in a chat; it is a repo
command now.

## Tests

- `desk::tests::the_palette_offers_what_was_copied_before`
- `notepad::tests::browsing_history_marks_the_lines_that_differ`
- Machine: `cargo xtask gauntlet` exit 0; `cargo xtask wallpaper` run on
  the fitted PNG's PPM export reproduces the committed assets' shape.
