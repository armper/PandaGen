# Phase 365: per-line styles; headings and asides (GFX-073)

## What changed

- `services_gui_host::LineStyle { bold, tone: LineTone, underline }` and
  `DesktopWindow::line_styles: Vec<(line, LineStyle)>`. The card painter
  draws a styled line in its tone's theme colour, strikes it a second time
  one pixel right when bold, and rules under it when underlined. Row pitch
  is unchanged.
- Notepad styles lines by their first characters: `# ` heading (bold,
  accent, rule), `## ` subheading, `### ` strong, `> ` aside (muted). The
  marks remain in the text; the file is plain text.
- A wrapped heading is styled on every visual row and ruled on the last.

## Why this shape

The earlier phase declined rich text because the compositor had no
per-line primitive. This is that primitive, kept small: no second font,
no size changes, so nothing about the grid, the caret or the selection
fill needed to change. Emphasis in one font is tone, weight and rule --
which is what the design deliberately limited itself to.

## Tests

- `desk_cards::line_styles_change_tone_weight_and_rule_but_not_the_grid`
- `notepad::tests::headings_and_asides_are_styled_by_how_the_line_starts`
- Machine: a Notepad with `# Title`, `## Part`, `> aside` on screen;
  `cargo xtask gauntlet` exit 0.
