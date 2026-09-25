# Phase 381: Apps, every app as a grid (GFX-089)

## What changed

- `kernel_bootstrap/src/launcher.rs`: a card showing every dock app as a
  cell with its icon at four pixels a bit and its name under it. Typing
  filters (names that start with the query first, then names that contain
  it); arrows move the selection; Enter or a click opens the app exactly
  as its dock tile would (a tucked card comes back, an open one is raised,
  a new one asks for its listing or document) and the grid closes. Esc
  clears the query, then closes.
- Three ways in: Ctrl+Space twice (the second press with nothing typed),
  a click on the top bar's left end, and a palette row.
- `Ui::push` for any draw operation (the large icons).
- The grid does not use up a cascade slot, so an app opened from it lands
  where it would from the dock. The first gauntlet run caught this: the
  Timer opened one cascade step lower than a first card should.
- `DeskApp::Launcher` is a system card, not a dock tile.
- Gauntlet: a shape opens Apps with Ctrl+Space twice, types "tim", and
  checks the Timer opened.

## Why

The dock shows pictures without names and the palette names without
pictures. The grid is both, for a person who does not yet know which
tile is which.

## Tests

- `launcher::tests::typing_filters_by_name_and_enter_or_a_click_launches`
- `desk::tests::apps_opens_from_the_palette_or_the_bar_and_launches_by_name_or_click`
- Machine: `cargo xtask gauntlet` exit 0.
