# Phase 370: Tiles, the 2048 game (GFX-078)

## What changed

- `kernel_bootstrap/src/game.rs`: the sliding-tiles game on a 4x4 board.
  Arrows (or WASD/HJKL) slide, equal neighbours merge once per move and
  score their sum, a 2 (one time in ten a 4) lands on a free cell, and
  the game knows when no move would change anything. The randomness is
  a seeded xorshift: the desk seeds it from the clock and the launch
  count, tests from a constant.
- The board is text with `+------+` borders; the moves are `[ Up ]`,
  `[ Left ] [ Down ] [ Right ]` and `[ New ]` buttons in the text, so the
  game plays with the pointer alone.
- Desk: `DeskApp::Tiles` on the dock (eighth tile, "2k"), a palette row
  ("Tiles: the 2048 game"), chips New and Close, an overview line with
  the score.
- Gauntlet: dock pins moved with the eighth tile; a shape opens Tiles
  from the palette and makes four moves.

## Why this shape

A game is the quickest proof that the desk's cards are general: nothing
here needed a new primitive, and the same `button_at` scanner the Timer
introduced turns any bracketed word into a control.

## Tests

- `game::tests::a_line_slides_and_merges_once_per_pair`
- `game::tests::the_board_slides_in_four_directions_scores_and_knows_the_end`
- `desk::tests::tiles_plays_by_arrow_and_by_button`
- Machine: `cargo xtask gauntlet` exit 0.
