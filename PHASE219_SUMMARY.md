# Phase 219: Full-ASCII Desktop Font And Caret Fix

## Summary

This phase implements `GFX-053` from the graphics roadmap and fixes a compositor caret bug found while validating Phase 218 in QEMU.

**Shared 8x16 font.** `font_data_8x16.in` moved from `kernel_bootstrap` into `graphics_rasterizer`, exposed as `FONT_8X16` with `ascii_8x16_glyph(ch)` (non-ASCII and control characters map to `?`). The kernel text console now reads glyphs from the rasterizer, so text mode and graphics mode share one glyph set.

**`BitmapFont` glyph sources.** `GlyphSource::{Compact5x7, Ascii8x16}` selects which bitmaps a font samples; `BitmapFont::new` keeps the compact source for compatibility, `BitmapFont::ascii_8x16(advance)` is the new constructor, and `scale_glyph` does nearest-neighbour scaling from either source. `DESKTOP_FONT` is now `ascii_8x16(8)`, so a desktop cell is 8x18 pixels.

**Caret origin fix.** `services_gui_host` drew content text at `rect.x + 2` but the caret at `rect.x + CELL_WIDTH + 2`, one cell to the right of the character it marked. Both now share the text origin.

**Golden fixtures.** `assert_raster_golden` gained a `PANDAGEN_UPDATE_GOLDEN=1` switch that rewrites the fixture from current output. Both fixtures were regenerated for the new cell size and reviewed. Five GUI-host tests that hardcoded 9x10 pixel coordinates were rewritten in terms of `RASTER_CELL_WIDTH` and `RASTER_CELL_HEIGHT` so future font changes do not require touching them.

## Rationale

The graphical desktop from Phase 218 rendered everything uppercase and replaced `'`, `>`, and `|` with `?`. The kernel already carried a complete 8x16 font for the text console; sharing it is cheaper than drawing a second set and guarantees that the same string looks the same in both display modes. Keeping the compact 5x7 source available preserves an option for very small UI text later.

## Tests

Added or rewritten:

- `graphics_rasterizer`: case and punctuation glyphs differ from `?`, native 8x16 rasterization matches the source bitmap bit for bit, scaling to smaller cells stays in bounds, desktop text advances one cell per glyph.
- `services_gui_host`: border/fill/text/caret positions, layer policy, linear framebuffer target, damage repaint, and title spacing, all metric-agnostic.
- `kernel_bootstrap::desktop_frame`: layout expectations expressed through the cell constants.

Validated with:

- `cargo test -p graphics_rasterizer -p services_gui_host -p kernel_bootstrap`
- `cargo xtask iso` and a `cargo xtask qemu-script` graphics-mode session (help, editor, palette) with screendumps reviewed: lowercase and punctuation render, caret directly after the prompt.
