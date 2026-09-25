# Phase 399: the desk's own icons -- notices, now, shortcuts, bin (GFX-107)

## What changed

- Four more pictures, for the desk's own cards rather than apps: a bell
  (notices), a gauge (now), a keyboard (shortcuts) and a rubbish bin. They
  are drawn by `tools/art/drawn_icons.py` in the app icons' style -- a
  diagonal gradient with light from the top, one bold white glyph with a
  soft shadow -- and exported by `tools/art/icons.py` at the same six
  sizes, squircle-masked like the rest. `PICTURES` grows to 94; the
  thumbnails follow the icons as before.
- The palette's Notices, Now and Shortcuts rows wear them
  (`PaletteRow::picture`), so every row has an icon.
- A notice about no app in particular wears the bell; a save, a timer's
  or a search's still wears its app's.
- In the Files card's Bin, every row wears the bin.

## Why

The image model's account ran out of credits while these were being
made, so they are drawn in code instead -- which also means anyone can
regenerate them. Side by side with the generated set they read as one
family.

## Tests

- `desk::tests::welcome_is_drawn_and_a_notice_wears_its_apps_icon`: the
  bell on a plain notice; the palette rows' pictures; the bin's size.
- The picture-table test covers the new names at every size; Look's
  thumbnail ids moved past them.
- Machine: `cargo xtask gauntlet` exit 0.
