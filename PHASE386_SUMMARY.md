# Phase 386: colour icons (GFX-094)

## What changed

- **Pictures.** `services_gui_host::Picture` is straight-alpha RGBA with
  a size; the `Theme` carries a `'static` table of them (skipped by serde,
  empty by default) and `DrawOp::Picture { x, y, id }` draws one by
  number, blended by its alpha over what is there (`draw_picture`). A
  theme without the table draws nothing for it, so every host and test
  that does not ask is unchanged.
- **Icons.** Eleven icons (ten apps and a panda mark) generated with
  OpenAI `gpt-image-2` in one shared style, kept as 256px sources in
  `kernel_bootstrap/assets/icons/src/` with their prompts in `PROMPTS.md`,
  and exported by `tools/art/icons.py`: masked to a squircle with an
  antialiased edge, at 64, 40, 32, 20 and 16 px (about 325 KB in the
  kernel). `DeskApp::picture(size)` names each.
- **Where they show.** The dock's tiles are the icons (a halo behind the
  one under the pointer); the Apps grid at 64 px; Files rows and palette
  rows at 16 px (a document wears the icon of the app that made it); the
  overview's mini cards at 32 px; the panda mark at the top bar's left,
  with the title moved right of it (`TOP_BAR_TITLE_X`).
- `docs/design/reference.jpg` is the generated mockup the desk's graphics
  are compared against from here on.

## Tests

- `desk_cards::pictures_are_blended_by_their_alpha_in_the_dock_and_on_cards`
- `desk::tests::every_app_has_its_picture_at_every_size_and_the_dock_uses_them`
- Tests that compared the desk's theme with a preset compare it without
  the picture table.
- Machine: `cargo xtask gauntlet` exit 0.
