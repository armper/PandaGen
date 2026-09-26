# Phase 421: the desk comes back as it was left (DESK-020)

## What changed

A reboot used to bring back the look, the recent files and nothing else:
every card had to be opened and placed again (Phases 26 and 84, "No Saved
Layouts"). Now:

- **`Desk::layout_text`** writes the desk down: the space on screen, then
  one line per card from the bottom up -- app, space, bounds, whether it
  is tucked into the dock, and for a Notepad the document it shows.
- **`Desk::apply_layout`** opens those cards where they were, fitted to
  this screen (it may not be the one they were left on), on their spaces,
  and asks the kernel to read each Notepad's document and each Files or
  Calendar card's listing. It runs once per desk; signing in makes a new
  desk, so each person gets their own back.
- **Kept as it changes.** The desk looks at its layout twice a second and
  writes it (`.desk`, a per-person setting like `.look`) once it has held
  still for two seconds, so a card being dragged is not written at every
  stop. Nothing is written before the saved layout has been read, or a
  fresh boot's empty desk would overwrite the one to bring back.
- Cards brought back: Notepad (named documents), Files, Terminal, Look,
  Notices, Now, Shortcuts, Calculator, Calendar, Timer, Tiles, Tasks and
  Sketch. Not the Welcome card, the Apps grid, or Sharing, Access and
  Audit, which are opened for a moment and a purpose.

Two bugs the reboot test found, fixed here:

- **Keys a card ignores leaked into the hidden console.** With a
  Calculator focused, typed letters went to the console under the desk,
  and Enter ran them as a command ("Unknown command: hi"). In desk mode a
  key no card wants now goes nowhere.
- **Ctrl+N** opened a Notepad only from the bare desk or a Terminal. It
  now does from any card that does not use Ctrl+N itself (the Notepad's
  is "new document", Files' is "new file").

## Gauntlet

`Shape::then` gives a shape a second boot on the disk the first one left,
instead of a blank one. The new shape "the desk after a reboot" opens a
Calculator and saves a Notepad as `plan`, then reboots and expects
`desk: layout restored, 2 cards`. It is the gauntlet's first check that
anything survives a restart (the unsettled Phase 101 item).

## Tests

- `the_layout_is_kept_and_comes_back_where_it_was`: untitled Notepads and
  the Apps grid are left out; a fresh desk reopens Files (listing asked),
  the Calculator where it was, and `plans` (read asked); once per desk.
- `a_layout_from_a_bigger_screen_is_fitted_and_nonsense_is_skipped`.
- `the_layout_is_written_once_it_holds_still`: nothing before the read;
  one write after two still seconds; not again while unchanged.
- `ctrl_n_from_a_card_that_does_not_use_it_opens_a_notepad`.
- QEMU by hand: Calculator, `plan` and a Terminal on space 3 all return
  after a reboot, with `plan`'s text.
- `cargo xtask gauntlet`: exit 0.
