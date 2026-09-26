# Phase 418: the keyboard's locks -- Caps Lock, Num Lock, the keypad, LEDs (KBD-013)

## What changed

The PS/2 parser (`Ps2ParserState` in `kernel_bootstrap/src/main.rs`) knew
Shift, Ctrl and the E0 prefix and nothing else of the keyboard's state:

- **Caps Lock** did nothing. It now toggles on the press -- once, however
  long the key is held, since the keyboard repeats make codes -- and Shift
  undoes it for a letter, as everywhere.
- **Num Lock and the keypad**: the keypad's keys produced nothing at all,
  and keypad Enter (E0 1C) was dropped, so the keypad could not run a
  command. With Num Lock on (the default) the keypad types digits, `.`,
  `*`, `-`, `+`, `/` and Enter; with it off (or with Shift) the keys are
  Home, the arrows, Page Up/Down, End and Delete.
- **LEDs**: the kernel now sends 0xED and the lock bits when a lock
  changes, and once at boot so the LEDs match. The keyboard's ACK bytes
  (and resend, echo and overrun codes) are dropped, not read as keys.
- **Fake shifts**: a keyboard wraps its grey arrows in E0 2A / E0 AA while
  Num Lock is on. They were taken as Shift, so on such a keyboard every
  arrow key grew a selection. They are ignored now.
- **Pause** (E1 1D 45 E1 9D C5) is swallowed whole: its middle bytes used
  to press Ctrl and toggle Num Lock.
- The twenty-six letter arms are one table lookup by keyboard row.

## Why

Phases 57 and 59 left the locks and the keypad as "not yet". Caps Lock
doing nothing, and a keypad that types nothing, are the kind of thing a
person notices in the first minute at a real machine.

## Tests

- `test_caps_lock_toggles_once_and_shift_undoes_it`: held key toggles
  once, LEDs byte, Shift inverts, Ctrl+letter unchanged, ACK dropped.
- `test_every_letter_key_is_its_letter`: the row table covers a..z.
- `test_keypad_digits_with_num_lock_and_arrows_without`.
- `test_fake_shifts_and_pause_are_not_keys`.
- Gauntlet: the Terminal shape now types `mem` under Caps Lock and runs it
  with the keypad's Enter: `WS > MEM` on serial.
- QEMU by hand: `HI there 12*3`, keypad Enter as a new line.
- `cargo xtask gauntlet`: exit 0.
