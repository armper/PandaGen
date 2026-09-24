# Phase 363: Files preview pane and tag collections (GFX-071)

## What changed

- **Preview pane.** The Files card shows the selected file's first lines
  (up to 40) in the right two fifths; the list keeps the left. `Ctrl+U`
  / the `List`/`Preview` chip toggles; cards under 60 cells wide show the
  list alone; the bin never previews.
- **Collections by tag.** `Ctrl+G` / the `All`/`#tag` chip cycles the list
  through every tag present (All → first tag → … → All); the footer says
  `#work only`.

## Design

The preview is requested from `Desk::tick`, which the kernel already
polls: when a Files card's selected name differs from the preview it
holds and no request is outstanding, one `DeskRequest::PreviewFile` goes
out; `preview_loaded` fills it. Selection changes therefore need no new
plumbing at any of their sites (keys, clicks, refresh).

Tag collections are a filter on `visible()`, so sort, filter and bin
compose with them unchanged.

## Tests

- `desk::tests::files_previews_the_selected_file_beside_the_list_and_gathers_by_tag`
  (one request per selection, "..." while pending, two-column rows, list
  only, no requests while list-only, tag cycling and footer).
- Machine: `cargo xtask gauntlet` exit 0 (the Files shapes' pinned pixels
  are on the list half and the ring, unchanged).
