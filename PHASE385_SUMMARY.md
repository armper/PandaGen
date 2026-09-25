# Phase 385: hold Ctrl and every chip says its key (GFX-093)

## What changed

- The keyboard parser reports Ctrl going down (`KEY_CTRL_PRESSED`,
  0x9E) as well as up. The text console ignores bytes above ASCII, as it
  does the arrows.
- While Ctrl is held, every header chip shows its key beside its name:
  "Save ^S", "Find ^F", "Next Enter", "Close Esc". Letting go puts the
  names back. Key repeat while held changes nothing.
- `key_name` writes a chip's key: `^S` for a control key, the name of a
  named key, the letter of a plain one.
- Ctrl+Tab and the overview's pick-on-release work as before; a Ctrl
  press also wakes a resting desk.

## Why

The shortcut sheet lists every key, but it is a card to open. Holding
Ctrl shows the keys where they are used, the moment a hand reaches for
them.

## Tests

- `desk::tests::holding_ctrl_shows_every_chips_key`
- The parser tests expect the Ctrl press byte.
- Machine: `cargo xtask gauntlet` exit 0.
