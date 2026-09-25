# Phase 392: Look, drawn (GFX-100)

## What changed

- The Look card is a picture of its choices rather than a list of
  names: each theme is a little desk in its own colours (its background,
  a card with a hairline, the accent strip, text lines); each accent is a
  round swatch; each wallpaper is a thumbnail, and Gradient is drawn from
  the theme's two background colours.
- The highlighted tile has an accent ring and its name in the text
  colour; what is kept has an accent dot under its name.
- A tile answers a click with its row's key (`LOOK_KEY_FIRST + row`):
  another tile previews, the highlighted one keeps -- as a click on a
  row did before. The keyboard is unchanged.
- The thumbnails (100x62, rounded) are made by `tools/art/icons.py` from
  the wallpaper sources and sit in `PICTURES` after the icons
  (`thumbnail_id`). The card is wider and a little shorter.
- Gauntlet: the Look shape's focus-ring pin moved to the wider card's
  left edge.

## Tests

- `desk::tests::look_draws_previews_swatches_and_thumbnails_and_tiles_take_clicks`
- The Look and pointer tests read the drawn card and click tiles by pixel.
- Machine: `cargo xtask gauntlet` exit 0.
