# Phase 378: graphics on text cards; icons in Files, the palette and the overview (GFX-086)

## What changed

- `DesktopWindow.overlay: Vec<DrawOp>`: draw operations laid over a text
  card's content, after the lines, in the content area's pixel space. A
  text card can now carry pictures without giving up its lines. This is
  the mixed text-and-graphics primitive the compositor lacked.
- `DrawOp::Icon { x, y, scale, bits, color }`: a 16x16 one-bit icon as a
  draw operation, in a colour or the theme's text colour, so the dock's
  icons can be drawn anywhere.
- **Files** rows carry the icon of the app that made the document: a
  day's note the Calendar's, `tasks` the Tasks card's, `sketch` the
  Sketch's, plain text the Notepad's, anything else a document. Rows are
  indented three columns for it.
- **Palette** rows that open an app carry that app's icon; recents and
  search hits a document; actions none.
- **Overview** mini cards carry their app's icon, large, top-right.

## Why this shape

An overlay is the least the compositor can add to let a text card have
pictures: the text path is untouched, hit-testing by line and column is
untouched, and the overlay is clipped like everything else in the
content area. The uses chosen are the three places a person scans a
list and wants to recognise things without reading.

## Tests

- `desk_cards::a_text_card_draws_its_overlay_over_the_lines`
- `desk::tests::files_the_palette_and_the_overview_carry_icons_over_their_text`
- Existing palette and Files tests read rows past the indent.
- Machine: `cargo xtask gauntlet` exit 0.
