# Phase 395: one control per action, and documents that look like documents (GFX-103)

## What changed

- A drawn card's own buttons are its controls. The header chips that
  repeated them are gone: the Calculator's Clear (its C key), the Timer's
  Start/Pause and Reset, Tasks' Add, Done and Remove, and the Calendar's
  Earlier and Later (its arrows). The Calendar keeps Today and Note,
  which it does not draw; Tasks' Add/Cancel while typing a task stays.
  The keys are unchanged.
- Files and the palette give a document the icon of the app it opens in
  (`document_app`): a day's note the Calendar's, the task list the Tasks
  card's, a drawing the Sketch's, anything else the Notepad's page. Only
  a folder wears the Files folder. Before, every file without a text
  schema -- most of them -- showed the Files icon, so the list was a
  column of folders.

## Why

Next to the reference, cards with both a drawn button row and a chip row
said everything twice, and the Files list read as folders.

## Tests

- Chip and icon expectations updated in the Calculator, Timer, Tasks,
  sound and icon tests; the sound test clicks the Tiles card's New chip.
- Machine: `cargo xtask gauntlet` exit 0.
