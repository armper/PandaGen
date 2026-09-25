# Phase 391: cards that float (GFX-099)

## What changed

- **Soft shadows.** A card's solid two-pixel lift is replaced by a soft
  shadow: the theme's shadow colour, darkest just under the card, fading
  over 14 px and falling 5 px, blended over whatever is below. It is
  drawn only outside the card body, which paints over the rest, so it
  costs a band around each card rather than its whole area.
- **Rounder corners.** Cards are rounded at 10 px (from 6).
- **Fewer chips.** The header's × closes every card, so the "Close" chip
  that repeated it is gone from Notices, Now, Shortcuts, Calculator,
  Calendar, Timer, Tiles, Tasks, Sketch and Apps. A prompt's Close, which
  closes the prompt and not the card, stays.
- **Calculator.** The result is three times the font when it fits (two
  or one for longer text), in a taller display; all four operators are
  drawn in one weight.

## Tests

- `desk_cards::a_card_has_rounded_corners_a_ring_and_a_header` checks the
  shadow darkens under the card, fades, and stops.
- The chip-list and calculator layout tests read the new values.
- Machine: `cargo xtask gauntlet` exit 0.
