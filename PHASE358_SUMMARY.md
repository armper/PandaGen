# Phase 358: the wallpaper (GFX-066)

## What changed

- The desk has a picture: `kernel_bootstrap/assets/wallpaper.{pal,idx}`
  (1280x800 in 256 colours -- a 768-byte palette and one byte a pixel,
  Floyd-Steinberg dithered from `Default Wallpaper.png`, kept beside them
  as the source) is built into the kernel with `include_bytes!` and
  painted behind everything.
- `services_gui_host::Wallpaper { width, height, palette, indices }` is
  carried on the `DesktopScene` (serde-skipped: the remote viewer has no
  use for a megabyte per frame) and sampled nearest-neighbour by
  `fill_background`, so any surface size works and a damage repaint reads
  exactly the pixels the full frame did.
- The first build stored packed RGB (3 MB). The kernel grew to 11 MB and
  the gauntlet's 16 MiB machine no longer booted -- the harness caught a
  size regression no host test could. Indexed colour brought the kernel to
  9.1 MB and the small machine back.
- Look gained a third section, **Wallpaper**: Picture / Gradient,
  previewed live and kept in `.look` (`wallpaper=Picture`). Default is
  the picture; Gradient is the theme's own, as before.

## Why

A desk should be the person's. The picture is the first thing on screen,
so it is compiled in rather than read from disk: there is no first frame
without it. Sampling per pixel rather than blitting keeps one code path
for full and damage repaints and makes the surface size irrelevant.

## Tests

- `desk_cards::a_wallpaper_is_sampled_to_the_surface_behind_the_cards`
  (2x2 picture stretched to the surface; the card paints over it; a
  damage repaint matches; the scene carries it).
- `desk::tests::the_look_card_previews_as_you_move_keeps_on_enter_and_reverts_on_esc`
  extended for the third section and the asset's size.
- Machine: the two gauntlet pixels that read the gradient read the
  picture now (`100,400 -> 3,3,6`); `cargo xtask gauntlet` exit 0.
