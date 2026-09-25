# Phase 390: a bigger dock, and glass (GFX-098)

## What changed

- The dock's icons are 48 px with 10 px between them in a 64 px pill with
  softer corners, as in the reference; the icons are exported at 48 px
  as well.
- The top bar and the dock's pill are glass: their colour laid over the
  wallpaper at `GLASS_ALPHA` (214 of 255) with
  `RenderTarget::blend_rounded_rect`, so the picture shows faintly
  through them.
- Gauntlet: the bar, pill and dock-dot pins were re-measured for the new
  geometry and the glass (the values depend on the default wallpaper,
  which the gauntlet always uses).

## Tests

- The dock and top bar tests in `desk_cards` read the new geometry and
  accept glass within a few levels of the surface colour.
- Machine: `cargo xtask gauntlet` exit 0.
