# Phase 396: a drawn close cross (GFX-104)

## What changed

- A card's close control is a drawn cross (`close_cross`) instead of the
  font's lowercase 'x': two diagonals about 1.6px thick, 9px across,
  centred in the same 16px hit box, each pixel inked by how much of it
  the stroke covers. The hit box, the hit test and the colour
  (`text_muted`) are unchanged.
- It is integer arithmetic in half-pixel coordinates -- no floating point
  in the compositor.

## Why

With the text in real type (Phase 394), the letter 'x' read as a letter
rather than a control, and it was smaller than the reference's cross.

## Tests

- `desk_cards::the_close_cross_is_two_smooth_diagonals`: solid centre and
  arm ends, soft pixels beside a diagonal, nothing between the arms or
  past their ends.
- Machine: `cargo xtask gauntlet` exit 0.
