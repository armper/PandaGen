# Phase 394: real type -- the desk's text drawn from Fira Mono (GFX-102)

## What changed

- The desk's smooth font (`SMOOTH_FONT`) draws printable ASCII from Fira
  Mono Medium instead of the 8x16 console bitmap smoothed by Scale2x. The
  cells are the same 8x16, so every layout, hit test and wrap is
  unchanged; only the ink inside each cell is different.
- `tools/art/font_smooth.py` bakes both tables from the face: the 1x
  coverage (`font_aa_8x16.bin`) and the 8x one-bit drawing that scaled
  text samples (`font_hi8_8x16.bin`). Each glyph is moved sideways by up
  to half a pixel to wherever its 1x coverage is sharpest, so stems land
  on whole pixels -- the one hint the face gets; the baseline never moves.
  Glyphs outside printable ASCII still come from the bitmap.
- The face and its SIL Open Font License are in `assets/fonts/`; the
  kernel never reads them, only the baked tables. The tables are the
  same size as before, so the kernel is too.
- The text console and the classic compositor keep the plain bitmap.

## Why

Held next to the reference image, the text was the largest gap left: the
icons, wallpaper, glass and shadows had become pictures, and the words
on them were still a 1980s VGA face. Fira Mono is monospace at a 0.6em
advance, so at 13.33px it fits the existing cell exactly.

## Tests

- `graphics_rasterizer::tests::smooth_text_keeps_stems_and_softens_steps`
  now checks properties of any face: a solid stem, soft diagonals, and
  scaled coverage that matches the 1x glyph times the scale squared.
- Machine: `cargo xtask gauntlet` exit 0.
