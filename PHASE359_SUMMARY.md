# Phase 359: spaces (GFX-067)

## What changed

- Four **spaces**, each with its own cards. `Ctrl+1..4` switches;
  `Ctrl+Shift+1..4` moves the focused card and follows it. The palette
  lists "Go to space N" and "Move this card to space N".
- The top bar's centre shows a strip, `[1].  2   3   4` -- brackets on
  the current space, a dot after any space that holds cards -- and a click
  on a number switches. The rest of the bar still opens the palette.
- Raising a card on another space (dock tile, palette recents, content
  search) takes you to that space.

## Design

`Desk::on_stage(window)` = not tucked and on the current space. The four
sites that filtered on `!tucked` (draw loop, focus fallback in `close` and
`tuck`, `cycle_focus`) use it; everything else -- pointer routing, chips,
apps -- is untouched because it only ever sees the windows the draw loop
produced. `DeskWindow.space` is set at launch to the current space.

The parser turns Ctrl+digit into private bytes (`KEY_CTRL_1 + n`,
`KEY_CTRL_SHIFT_1 + n`); Ctrl+letters stay what they were.

The compositor gained a centred second content line for `TopBar` and
`top_bar_centre_column`, and the bar's hit region is now
`Content { line: 0, column }` so the desk maps clicks to the cells it
painted.

## Tests

- `desk::tests::spaces_hold_their_own_cards_and_the_strip_says_where_things_are`
  (switch, hint, per-space Ctrl+Tab, move-and-follow, raise-follows,
  bounds, strip geometry, strip click, bar click).
- Parser: Ctrl+2 → `KEY_CTRL_1 + 1`, Ctrl+Shift+3 → `KEY_CTRL_SHIFT_1 + 2`.
- Machine: `cargo xtask gauntlet` exit 0; a Ctrl+2 / Ctrl+1 screenshot pair.
