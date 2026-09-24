# Phase 368: the Calendar card (GFX-076)

## What changed

- `kernel_bootstrap/src/calendar.rs`: a month grid, Monday first, seven
  four-character cells per row. Today is marked `*`, a day with a note
  carries `.`, the selected day is the card's selection fill. Below the
  grid: today in words and the selected day's state.
- Keys: arrows move a day or a week, PageUp/Down a month, `t` today,
  Enter opens the day's note. Clicks: the title's ends turn the month, a
  day selects, the selected day opens. Chips: Today, Earlier, Later,
  Note, Close.
- A day's note is an ordinary document named by the day (`2026-09-24`):
  it is in Files, keeps versions, opens in a Notepad. When the day has no
  document, a Notepad opens with that name so it saves itself as soon as
  something is typed. The Calendar learns which days have a note from the
  same listing Files gets, and lists again after any save lands.
- `rtc`: `weekday`, `days_in_month`; `days_from_civil`/`civil_from_days`
  are public. The kernel now reads the full clock each frame and hands
  the desk today's date (`Desk::set_today`), so the mark moves at
  midnight.
- Desk: `DeskApp::Calendar` on the dock (sixth tile, "Cl"), a palette
  row, an overview line with the selected day; the dock's launch-with-
  listing now covers Files and Calendar.
- Calculator: the footer is fitted to the 280px card (it was clipped in
  the Phase 367 screenshot).
- Gauntlet: dock pixel pins moved with the sixth tile; a shape opens the
  Calendar from the palette and turns the month.

## Why this shape

No second text editor and no second storage: a day's note is a document,
so everything the desk already does for documents (versions, history,
autosave, search, tags) applies to notes without new code. The calendar
is only a way of finding the day.

## Tests

- `calendar::tests::september_2026_starts_on_a_tuesday_and_the_grid_says_so`
- `calendar::tests::keys_and_clicks_move_the_day_and_enter_opens_its_note`
- `desk::tests::the_calendar_marks_noted_days_and_opens_a_day_as_a_document`
- Machine: `cargo xtask gauntlet` exit 0.
