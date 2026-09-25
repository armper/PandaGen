# Phase 383: the palette goes to a heading (GFX-091)

## What changed

- With a Notepad focused, the palette lists the document's `#`, `##`
  and `###` headings first, indented by level, with the Notepad icon and
  "heading" where a shortcut would be. Typing filters them with
  everything else; Enter puts the caret on the heading and the heading
  at the top of the view.
- `Notepad::headings()` and `Notepad::go_to_line(row)`.
- The desk refreshes the palette's headings from the focused document
  whenever the palette is drawn or keyed, so the list is never stale.

## Why

Headings (Phase 365) gave long documents a shape. The palette is where
the desk already goes to get somewhere, so it is where the shape becomes
a way to move.

## Tests

- `notepad::tests::headings_are_listed_by_level_and_going_to_one_puts_it_at_the_top`
- `desk::tests::the_palette_goes_to_a_heading_in_the_focused_document`
- Machine: `cargo xtask gauntlet` exit 0.
