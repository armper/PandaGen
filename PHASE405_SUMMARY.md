# Phase 405: the palette's query behind a magnifier (GFX-113)

## What changed

- The palette's header reads as a search field: the compositor draws a
  magnifier (`magnifier`: an 11px ring two pixels thick and a short
  handle) where another card's title starts, and the query after it,
  `SEARCH_GLYPH_W` (20px) further right. Only a card with the Palette
  role gets it; the classic compositor is untouched.
- The desk's palette title is the query and its caret (`memo_`) without
  the old `> ` prompt.

## Why

The Apps grid (Phase 388) already had a drawn search field; the palette,
which is the same idea, still looked like a shell prompt.

## Tests

- `desk_cards::the_palettes_header_has_a_magnifier`: the ring where a
  plain card's title starts, and the query moved past it.
- Machine: `cargo xtask gauntlet` exit 0.
