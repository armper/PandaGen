# Phase 441: the Calendar moves out, holding only the documents named like days (PROC-009)

## What changed

The **Calendar** is a program. It is the fourth dock app to leave the
kernel, after the Calculator, the Timer and Tiles. It draws the same
month the kernel used to (the existing pixel checks pass unchanged),
marks the days that have a note, and opens a day's note in a Notepad.
It can do that without being able to see or touch any other document a
person keeps. `kernel_bootstrap/src/calendar.rs` is gone.

### Documents, by pattern

A new capability, **Documents**, is asked for with a name pattern that
travels in the program's image: `#` is a digit, `?` any character, `*`
any run. The Calendar asks for `####-##-##`.

- **`list(docs)`**: the names of the documents that match, and only
  those. The answer comes later as a message, because the listing
  happens in the desk's loop, which has the filesystem. After any save
  every holder is sent its list again unasked, which is how a new
  day's note gets its dot.
- **`open(docs, name)`**: the desk opens that document in a Notepad,
  empty if it doesn't exist yet, as the Calendar's day notes always were.
  A name the pattern doesn't match is refused before it leaves the
  kernel.
- A program with Documents **cannot read or write** them. The desk's
  Notepad does the writing, and the person sees it.
- `programs` shows the pattern ("asks for a card, documents named
  ####-##-##"), so whoever runs a program can see what it can reach.

### Messages

Anything the desk has for a program beyond an eight-byte event now
arrives as a **message**: `Event::Message` says one is waiting, and
`receive(ptr, len)` takes it. If the buffer is too small the call says
so and keeps the message. Messages are at most 8 KiB and kept per
program. Taking one frees memory, so it happens with interrupts on, like
every other allocation made during a call.

### Also new

- **`date()`**: today, packed, when the machine's clock is set (the
  desk's loop records it; the call only reads).
- **`Op::Hit`**: a place in a card that stands for a key without looking
  like a button (a calendar's day), outlined by the desk under the
  pointer.
- **`Event` is `#[non_exhaustive]`**: programs ignore events they don't
  know, so new kinds of event don't break them.
- The Calendar draws a **Today** button, so the header chip it used to
  have is still reachable with the pointer alone.

### `calendar_core` and `apps/calendar`

The dates, the month and the card moved from the kernel with their tests,
and are tested on the host (plus a new test: names that aren't real
days, such as 2026-13-01 or 1999-02-29, are not notes). The desk now
takes its `Date` from here too. The program lists its days at start,
sets them from each message, opens a day with `open`, and wakes once a
minute so the day can turn over.

## Tests

- `program_image`: documents are asked for by a pattern that travels
  with the image and describes itself; patterns match digits, any
  character and any run, and refuse empty or spaced patterns.
- `syscall_abi`: documents are reached through their pattern's handle
  only; `receive` decodes.
- `calendar_core`: its tests (moved), through decoded views and hit
  areas, plus names that are not days.
- `desk`: the Calendar is a program whose days open as documents: its
  card waits for the program; a new day opens as an empty Notepad; a
  save marks documents changed once; an existing day opens by reading.
  The sound test now presses a chip on the Audit card.
- Gauntlet: the Calendar shape now also expects its program. A new
  shape, "the Calendar program: a day's note opened, saved and listed",
  opens the Calendar, presses Enter, types, saves and closes the note,
  and expects the list to go from 0 to 1 day-named documents
  (`documents: thread 1 has 1 named ####-##-##`).
