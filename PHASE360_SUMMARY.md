# Phase 360: the overview (GFX-068)

## What changed

- **Ctrl+Tab opens the overview** instead of cycling blind: every card on
  every space (tucked ones too) drawn as a 280x150 mini card in a grid,
  four to a row, with title, up to three content lines and `space N`.
  The next card is ringed.
- Ctrl+Tab / Right / Down move the ring forward, Left / Up back;
  **releasing Ctrl**, Enter, or a click on a mini card picks; Esc closes
  without change. Other keys are swallowed while it is open.
- Picking goes through `Desk::raise`, so a tucked card comes back and a
  card on another space brings you to its space.
- The palette lists "Overview: every card at once"; "Next window" remains
  the blind cycle, now without a shortcut.

## Design

The parser delivers the Ctrl break code as `KEY_CTRL_RELEASED` (0x9F), a
private byte like the arrows; the workspace's `>= 0x80` gate ignores it,
the desk only acts on it while the overview is open.

Mini cards are `DesktopWindow::card` at a small size with
`frame.view_id = window.id`, so the existing router delivers a click on a
mini card as a click on that window; the desk's pointer handler picks
when the overview is open. No new compositor code.

## Tests

- `desk::tests::ctrl_tab_shows_the_overview_and_letting_go_picks`
  (open, ring on the next card, swallow typing, step, release picks, Esc,
  tucked card on another space, click picks, empty desk, stray release).
- Parser: Ctrl release yields `KEY_CTRL_RELEASED`.
- Machine: `cargo xtask gauntlet` exit 0; the desk shape's Ctrl+Tab-free
  flows are unchanged.
