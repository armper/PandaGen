# Phase 353: the palette searches inside files (GFX-061)

## What changed

- Typing two or more characters into the palette also searches the
  *contents* of the person's text files. Matching lines appear as rows --
  `memo: the quick fox   in file` -- after the recent files and before the
  actions. Enter opens the file in a Notepad and lands on the line, with
  the query selected.
- Results are answered by the kernel (`DeskRequest::SearchFiles` →
  `BareMetalEditorIo::search` → `Desk::search_results`) and shown only
  while the palette still asks the same query, so a slow answer never
  labels the wrong search.
- One hit per file, at most six, files in name order; dot-names and the
  bin are skipped.

## Why

The palette is already "type what you want". Names and tags cover what
a file is called and what it is about; content search covers what it
says. With a handful of small files on a bare-metal disk, reading them
per keystroke is cheap and honest; when it stops being cheap, an index
is the next step and the interface does not change.

## Tests

- `desk::tests::the_palette_searches_inside_files_and_enter_opens_the_file_on_the_line`
- `Notepad::find_on_open` reuses the incremental find from GFX-054, so
  the selection on open is the same code path as Ctrl+F.
- Machine: `cargo xtask gauntlet` exit 0.
