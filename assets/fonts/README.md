# Fonts

`FiraMono-Medium.woff2` is Fira Mono Medium, by The Mozilla Foundation and
Telefonica S.A., under the SIL Open Font License 1.1
(`FiraMono-LICENSE.txt`). It is not loaded at run time:
`tools/art/font_smooth.py` bakes it into the desk's glyph tables
(`graphics_rasterizer/src/font_aa_8x16.bin`, `font_hi8_8x16.bin`), which the
kernel includes.

Fira Mono was chosen because it is monospace with a 0.6em advance, so at
13.33px each glyph is exactly the 8-pixel cell the desk lays text out in,
and because a medium weight keeps its stems solid at that size on dark
cards.
