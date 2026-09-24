# Phase 375: Calendar and Tasks in real graphics; one label size (GFX-083)

## What changed

- **Calendar.** The month is a header with `<` and `>` buttons around the
  title at twice the font's size, the weekday names, and a rounded cell
  per day: the selected day is an accent fill, today is outlined in the
  accent, a day with a note carries a dot, and the pointer's cell is
  outlined. Today and the selected day are written under the grid. A day
  cell answers a click with its own key byte (`DAY_KEY_FIRST + day - 1`,
  in the desk's private range above the parser's), which the card handles
  like any key: another day selects, the selected day opens its note.
  `CalendarLayout::new(width)` is the shared geometry.
- **Tasks.** A row per task with a real checkbox (accent fill with a
  tick when done, muted outline when not), the selected row raised, done
  tasks in the muted tone; Add (primary), Done, Remove and Clear done as
  buttons under the list. Rows answer clicks with `ROW_KEY_FIRST + row`,
  and a list longer than the card scrolls so the selection is always on
  screen. The `[ button ]` text layout and `button_at` use are gone.
- **Widgets.** `Ui::hit_area` registers a hit rectangle for something the
  app drew itself (a day, a row). Button labels are drawn large only when
  they are one character and the key is tall enough; words stay at the
  font's size, so a row of buttons is one size (the Phase 374 screenshot
  had "Up" large beside "Left" small).
- Desk: both cards render graphics; clicks are mapped to canvas pixels,
  asked of the card's `Ui`, and the answer goes through `handle_byte`.
- Gauntlet: the Calendar shape pins a day cell's raised fill.

## Why this shape

Days and rows are controls without being buttons, so the widget layer
gained one small thing: a hit rectangle for what the app draws itself.
Giving those controls key bytes keeps the one rule that has held since
the header chips: a click is a key, and the app has one code path.

## Tests

- `calendar::tests::september_2026_starts_on_a_tuesday_and_the_grid_says_so`
  (cells by date, one accent cell, one dot, the texts)
- `calendar::tests::keys_and_clicks_move_the_day_and_enter_opens_its_note`
  (month buttons and day cells hit by pixel; a day's key selects, then opens)
- `tasks::tests::the_list_is_a_document_and_round_trips`
- `tasks::tests::keys_and_clicks_tick_add_move_and_remove_then_it_saves_itself`
  (rows and buttons by pixel; a long list scrolls to the selection)
- `desk::tests::the_calendar_marks_noted_days_and_opens_a_day_as_a_document`
- `desk::tests::tasks_is_a_document_that_saves_itself_quietly`
- Machine: `cargo xtask gauntlet` exit 0.
