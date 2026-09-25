# Phase 387: smooth text (GFX-095)

## What changed

- A new glyph source, `GlyphSource::Ascii8x16Smooth`, and `SMOOTH_FONT`:
  the same 8x16 font the console uses, drawn with antialiased edges.
- The coverage comes from `tools/art/font_smooth.py`: each glyph is
  enlarged with Scale2x (an edge-directed pixel-art upscaler that keeps
  straight stems straight and turns stair-steps into diagonals and
  corners into curves), then reduced by area. Two tables sit beside the
  font data: 8x16 alpha for 1x text (16 KB) and the 8x enlargement as
  bits (128 KB), which scaled text samples sixteen times a pixel.
- Text is blended by its coverage over what is under it; a straight
  stem stays fully opaque, so the text colour is exact where it matters.
- The desk's painters (cards, the dock, the top bar, graphics content)
  draw smoothly; the text console and the classic compositor keep the
  plain bitmap, so their golden fixtures are untouched.

## Tests

- `graphics_rasterizer::tests::smooth_text_keeps_stems_and_softens_steps`
- `tests::test_graphics_content_draws_ops_clipped_to_the_content_area`
  now asks for a fully inked stem pixel rather than a (now soft) corner.
- Machine: `cargo xtask gauntlet` exit 0.
