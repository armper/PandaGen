# Phase 351: Look -- the theme manager (GFX-059)

## What changed

- A fourth desk app, **Look** (dock tile `Lk`, palette row "Look: themes
  and accents"). One card with two lists: **themes** (Dusk, Daylight,
  Ember, Forest, Mono) and **accents** (Mint, Sky, Amber, Rose).
- **Preview is the interaction.** Moving the highlight changes the whole
  desk at once. `Enter` keeps, `Esc` (or closing the card) reverts. The
  footer says "Previewing Ember + Sky   Enter keeps it   Esc goes back".
- **Kept on disk** as `.look` (`theme=Ember\naccent=Sky\n`) under the
  `settings/desk` schema, read on the first desk frame. Unknown names
  fall back per field to the defaults, so a damaged file cannot leave the
  desk unreadable.
- `services_gui_host::Theme` gained `PRESETS`, `ACCENTS`, `named`,
  `accent_named`, `with_accent`, and three new full palettes (`EMBER`,
  `FOREST`, `MONO`).
- Files hides dot-names: system settings are not the person's files.

## Why this shape

Settings dialogs with Apply/OK/Cancel exist because applying was once
expensive. Here a theme is a value the compositor reads every frame, so
the honest UI is: what you point at is what you see; Enter is "yes,
this". The kept/preview split (`Desk::look` vs `Desk::look_preview`)
keeps that reversible without snapshots of anything else.

## Plumbing

- `DeskRequest::SaveLook { text }` → `BareMetalEditorIo::write_setting`
  (`.look`, `settings/desk`) → a "Look kept" notice.
- `DeskRequest::LoadLook` (pushed once by the kernel) →
  `read_setting` → `Desk::apply_look(Option<&str>)`.
- `Desk::theme()` returns the preview if any, else the kept choice's
  theme; the kernel already asks the desk for its theme every frame.

## Tests

- `desk::tests::the_look_card_previews_as_you_move_keeps_on_enter_and_reverts_on_esc`:
  open from the palette, preview Ember, revert, keep Mono + Sky and see
  the `SaveLook` text, parse it back (case-insensitively), reject
  nonsense, and drop the preview when the card closes.
- `desk::tests::files_hides_the_systems_dot_names`.
- Machine: the gauntlet's Look shape opens the card from the bare desk,
  steps to Ember, and asserts the desk background, a card surface and
  the top bar by pixel in that palette; then keeps and checks the notice.
