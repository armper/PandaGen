# Phase 389: three new wallpapers (GFX-097)

## What changed

- Look offers Aurora, Bamboo and Nebula beside the Picture and Gradient:
  generated with OpenAI `gpt-image-2` in PandaGen's palette, cropped to
  16:10, and converted with `cargo xtask wallpaper <ppm> --name <n>`.
  The 768px sources are in `kernel_bootstrap/assets/wallpapers/`.
- They are kept at 640x400 in 256 colours -- a quarter megabyte each
  instead of one, remembering H3 -- and the compositor enlarges any
  wallpaper smaller than the screen with bilinear sampling
  (`Wallpaper::sample_smooth`), so they are soft rather than blocky.
- `cargo xtask wallpaper` takes `--name` and 640x400 as well as
  1280x800. The Look card is taller for the longer list.

## Tests

- `desk_cards::a_small_wallpaper_is_enlarged_smoothly`
- `desk::tests::every_wallpaper_but_gradient_is_a_picture`
- The first wallpaper test probes inside a quadrant rather than on the
  seam, which an enlarged picture now blends.
- Machine: `cargo xtask gauntlet` exit 0.
