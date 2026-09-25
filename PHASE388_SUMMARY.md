# Phase 388: chrome polish from the reference (GFX-096)

## What changed

- **Spaces.** The top bar's strip is the numbers alone; the active one is
  drawn in the accent with a rule under it, and a space with cards has a
  dot under its number. The cells are where they were, so a click still
  lands on the space it looks like.
- **Tray.** A hairline between the date, the memory meter and the clock.
- **Apps.** The search line is a field: a raised well with a hairline
  and a drawn magnifier (`Ui::search_field`).
- **Calculator.** Divide and multiply are drawn as the ÷ and × people
  know (the font has neither); the keys they stand for are unchanged.
- **Rest.** The rest screen is a veil over the desk -- the wallpaper,
  darkened -- rather than a flat fill: `WindowStyle::Veil` blends the
  theme's shadow colour over what is under it and draws its overlay.
- `Ui::line` draws a line of any thickness; the widget palette has the
  theme's hairline.

## Tests

- `desk_cards::a_veil_darkens_what_is_under_it`
- The space strip and tray tests read the new strip and overlay.
- Machine: `cargo xtask gauntlet` exit 0.
