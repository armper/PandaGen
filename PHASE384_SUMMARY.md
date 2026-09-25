# Phase 384: the rest screen (GFX-092)

## What changed

- Ctrl+L, a palette row, or five minutes without a key or the pointer
  puts the desk to rest: the whole screen shows the time at eight times
  the font, the date under it, and how to wake. The palette and the
  overview close; the cards are untouched behind it.
- Any key or any pointer event wakes it, and the input that wakes does
  nothing else: a key pressed to wake is not typed into the card under it,
  a click is not a click on a card.
- The rest screen is a full-screen window in the top bar's style with an
  overlay (the same primitives as the tray), so no new painter was needed.
- `Desk::tick` measures idleness from the last input; the host tests all
  run far below five minutes of ticks.

## Tests

- `desk::tests::the_desk_rests_on_ctrl_l_or_when_idle_and_wakes_on_any_input`
- Machine: `cargo xtask gauntlet` exit 0.
