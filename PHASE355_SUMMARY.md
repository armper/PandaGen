# Phase 355: pointer-only parity (GFX-063)

## What changed

- **Palette rows run on click.** A press on a row of the palette card
  selects it and runs it, exactly as Enter does.
- **The top bar opens the palette.** Clicking the bar's left slot is the
  pointer's Ctrl+Space.
- **Notices dismiss on click.** The desk's notices are cleared; the
  workspace's are hidden until its list changes.
- **Look rows pick with the pointer.** One click previews a theme or
  accent, a second click on the highlighted row keeps it (the same
  select-then-act two-step Files uses).
- **Notepad chips follow its mode.** History: Older / Newer / Restore /
  Back. Find prompt: Next / Close. Name prompt: Cancel. Nothing in the
  Notepad now needs a key that is not typing.

## Why

A modern desk should not assume a keyboard is at hand for anything but
text. Every mode was audited for "can this be left, advanced, or chosen
with the pointer?" and the gaps closed at the desk layer, reusing the key
bytes each mode already answered.

## Tests

- `desk::tests::the_pointer_alone_runs_the_palette_dismisses_notices_and_picks_a_look`
- `desk::tests::notepad_chips_follow_its_mode`
- Machine: `cargo xtask gauntlet` exit 0 (qemu-script has no pointer
  input; the pointer paths are pinned by the host tests through the real
  router).
