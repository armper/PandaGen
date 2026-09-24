# Phase 374: Timer and Tiles in real graphics (GFX-082)

## What changed

- **Timer.** The time is drawn at three times the font's size; a
  countdown shows what is left as an accent bar under it; Start/Pause,
  Lap or Stopwatch, and Reset are real buttons (Start is the primary
  fill while stopped); the four presets are accent buttons; laps are
  listed below in text. `TimerLayout::new(width)` is the geometry the
  drawing and the desk's hit test share. The text-line layout and its
  `click` are gone; `button_at` stays for the Tasks card.
- **Tiles.** The board is a grid of rounded, coloured tiles (the classic
  warm-to-gold palette by value, dark ink on 2 and 4, light above) on a
  raised grid; values at twice the font's size; Left, Up, Down, Right and
  New are real buttons under the board. `GameLayout::new(w, h)` sizes the
  board to the canvas.
- Desk: both cards render graphics; a content click is mapped to canvas
  pixels and asked of the card's `Ui`, and the answer is the key the
  button stands for, handed to the same `handle_byte` the keyboard uses.
- Gauntlet: the Timer shape pins the Reset key's raised fill; the Tiles
  shape's surface pin still lands beside the board.

## Why this shape

Both cards had controls before, as bracketed words; now they have the
same controls as buttons, with the same keys behind them, and their
displays read at a glance. Nothing about how they work changed, which is
the point of the widget layer: the app's state machine stays put and
only the drawing and the hit test move.

## Tests

- `timer::tests::the_stopwatch_runs_on_the_desks_ticks_pauses_and_laps`
  (reads the drawn texts)
- `timer::tests::a_countdown_says_so_once_when_it_is_up_and_buttons_are_hit_by_pixel`
  (presets and controls hit by pixel, the bar's widths)
- `timer::tests::bracketed_words_are_still_found_by_column`
- `game::tests::the_board_slides_in_four_directions_scores_and_knows_the_end`
  (sixteen large tile values, the New button by pixel)
- `desk::tests::tiles_plays_by_arrow_and_by_button` (through the router)
- Machine: `cargo xtask gauntlet` exit 0.
