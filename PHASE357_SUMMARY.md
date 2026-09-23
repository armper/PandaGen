# Phase 357: the Welcome card (GFX-065)

## What changed

On a disk's first desk boot a small card appears at the bottom left,
above the dock: "Welcome to PandaGen" and five lines, each a thing to
try (typing searches; Ctrl+Space or a click on the bar opens the palette;
chips hold every command; documents save themselves; Look changes the
theme as you point). "Got it", Esc, Enter or the close glyph closes it;
the kernel writes `.welcomed` and it never returns.

## Design constraints it meets

- **No focus taken**: typing on the bare desk is still a palette search.
- **No cascade slot used**: the first Notepad opens where it always does,
  so nothing that pins card positions (the gauntlet's pixels included)
  moves.
- **Not a dock tile**: `DeskApp::Welcome` is filtered from the dock.
- **Out of the way**: bottom-left, above the dock, 440x200.

## Plumbing

`LoadLook` also checks `.welcomed`; absent → `Desk::show_welcome()`.
Closing sets `welcome_dismissed`; the next `tick` turns that into one
`DeskRequest::Welcomed`, which the kernel answers by writing the file.

## Tests

- `desk::tests::the_welcome_card_sits_out_of_the_way_and_closes_for_good`
- Machine: the gauntlet now runs from a blank disk (Phase 355, H1), so
  every desk shape boots with the Welcome card present and still passes;
  `cargo xtask gauntlet` exit 0.
