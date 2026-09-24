# Phase 376: icons on the dock (GFX-084)

## What changed

- `DesktopTab.icon: Option<[u16; 16]>`: a one-bit 16x16 icon a dock tile
  may carry. The dock painter draws it two pixels a bit, centred in the
  tile, in the text colour; a tile without one still shows its monogram.
- `DeskApp::icon()`: an icon for each of the ten dock apps, drawn by hand
  as bit rows (a page with lines, a stack of documents, a `>_`, a
  palette, a keypad, a month grid, a clock, four tiles, a ticked list, a
  pencil). The system cards that are not on the dock have none. The
  monogram stays as the bar's name for the tile under the pointer.
- Nothing about hit-testing or the running dot changed; the gauntlet's
  dock pins are the dots, unaffected.

## Why this shape

One-bit art in the theme's text colour keeps the dock the desk's: it
follows every Look, needs no image format, and costs 32 bytes an app in
the binary (H3 is remembered).

## Tests

- `desk_cards::a_dock_tile_draws_its_icon_in_place_of_the_monogram`
- `desk::tests::every_dock_tile_carries_its_apps_icon`
- Machine: `cargo xtask gauntlet` exit 0.
