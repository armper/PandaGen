# Phase 371: Tasks, a to-do list that is a document (GFX-079)

## What changed

- `kernel_bootstrap/src/tasks.rs`: a task list kept as the document
  `tasks`, one line per task, `[x] done` / `[ ] not yet`. Readable in a
  Notepad, versioned like any document.
- The card: a row per task with the selected one highlighted; Enter or a
  second click ticks it, `a` asks for a new task in the footer (typed
  there, Enter adds, Esc drops), Delete removes, U/D reorder, C clears
  done ones. `[ Add ] [ Done ] [ Remove ] [ Clear done ]` buttons in the
  text do the same for the pointer. Chips follow the prompt: Add, Done,
  Remove, Close, or Add, Cancel while typing.
- The list saves itself a second after it changes, quietly (no notice
  card), through the same request path Notepad autosave uses.
- Opening a card that shows a document now goes through
  `DeskApp::launch_request`: Files and Calendar ask for the listing,
  Tasks asks for its document (`PreviewFile`), from the dock and from the
  palette alike. `preview_loaded` routes the text to a Tasks card.
- Desk: `DeskApp::Tasks` on the dock (ninth tile, "Td"), a palette row,
  an overview line with how many are left.
- Gauntlet: dock pins moved with the ninth tile; a shape opens Tasks from
  the palette, adds "milk" and waits for the save.

## Why this shape

The to-do list is the third card whose state is a plain document
(after a day's note and the settings): no new storage, no new schema
machinery, and the list can be edited as text when that is what someone
wants. The card is a better view of it, not a different thing.

## Tests

- `tasks::tests::the_list_is_a_document_and_round_trips`
- `tasks::tests::keys_and_clicks_tick_add_move_and_remove_then_it_saves_itself`
- `desk::tests::tasks_is_a_document_that_saves_itself_quietly`
- Machine: `cargo xtask gauntlet` exit 0.
