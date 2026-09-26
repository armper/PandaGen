# Phase 406: the dock in liquid glass (GFX-114)

## What changed

- The dock is a floating capsule of glass (`liquid_glass`): the desk behind
  it is read back and blurred (a box blur run twice each way, radius
  `DOCK_BLUR`), and within `DOCK_RIM` of the edge it is sampled from nearer
  the capsule's spine, so the rim bends what is behind it like a lens. Over
  that go the theme's raised tint (`DOCK_TINT`) and a little white frost,
  a sheen on the top half, a specular rim brightest along the top, a
  second fainter line inside it, a shade inside the bottom rim, and an
  antialiased edge from the capsule's signed distance. A soft shadow of the
  capsule's own shape falls under it. All integer arithmetic.
- The tile under the pointer sits on a puck of lighter glass, drawn at its
  large size (64px; 48px at rest), and its name floats over the dock on a
  small glass label (`glass_label`) -- the concept's hover state.
- Running apps have a glowing light under them instead of a dot; the
  focused app's is longer (`DesktopTab.focused`); a tucked one's is an
  outline.
- After a divider (`DesktopTab.divider_before`, `DOCK_DIVIDER`) the dock
  has an Apps button, the panda, which opens the Apps grid
  (`Desk::dock_app`), so every app is a click away with the mouse alone.
- Geometry: 64px tiles 6px apart, an 84px capsule; `dock_row_width` is the
  one place the row is measured, for the desk and the hit test.
- The top bar is the same frosted glass (`frosted`), so bar and dock are
  one material.
- Gauntlet: the top bar's three pins, the dock glass pin (now between the
  first two tiles, with a small tolerance) and the two running-light pins
  moved.

## Why

Asked for a contemporary, bleeding-edge dock. The design follows the
translucent, refracting glass of current desktop design (Apple's Liquid
Glass, 2025-26): content shows through the chrome, blurred and bent at
the edges, with specular light on the rim. A concept image generated for
the purpose (`docs/design/dock_concept.jpg`) set the target: the hover
puck and label, the light bars, the divider and Apps button come from it.

## Cost

The blur reads back about 60k pixels for the dock and 45k for the bar a
frame; `gfx stats` in QEMU showed no slow presents (avg 0.11 ticks).

## Tests

- `desk_cards::the_dock_paints_a_tile_per_app_and_names_the_tile_that_was_hit`:
  frosted glass lighter than the desk, a round end, the new tile geometry
  and the running light.
- `desk_cards::the_dock_divides_names_the_hovered_tile_and_lengthens_the_focused_light`.
- Dock icon, picture and top-bar tests moved to the new geometry; desk
  tests count the Apps tile and check it wears the panda past a divider.
- Machine: `cargo xtask gauntlet` exit 0.
