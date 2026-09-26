# Phase 404: keycaps on the Shortcuts sheet (GFX-112)

## What changed

- Every key name on the Shortcuts sheet sits in a keycap: a hairline
  rounded box three pixels outside the word (`keycaps`, `KEYCAP_PAD`),
  drawn in the card's overlay over the text rows. `is_key_name` decides
  what is a key -- modifiers, named keys, one letter or digit, a run like
  `1..4` -- so the words between keys ("or", "drag it onto the dock")
  stay plain.
- The sheet spaces its combinations (`Ctrl + Shift + N`, `spaced_keys`)
  so neighbouring caps have room, and its key column moved from 35 to 41
  (`SHORTCUT_KEY_COLUMN`) so the longest palette labels no longer push
  their keys out of line. The card is 640px wide to hold it.

## Why

A sheet of shortcuts reads faster when the keys look like keys; and two
labels longer than the old column had shifted their keys right, which a
fixed-column keycap pass made visible.

## Tests

- `desk::tests::the_clock_opens_now_which_asks_the_kernel_once_a_second`
  (which opens the sheet): what is and is not a key; caps at the right
  pixels for `Ctrl + Shift + 1..4`; every row's keys start in the key
  column.
- Machine: `cargo xtask gauntlet` exit 0.
