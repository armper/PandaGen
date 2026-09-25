# Phase 393: a drawn Welcome, and notices with icons (GFX-101)

## What changed

- The Welcome card is drawn rather than typed: the panda mark (48px), a
  title twice the size of body text, a one-line subtitle, and the four
  tips (`WELCOME_TIPS`), each beside the icon of what it is about --
  Files for search, the panda for the Apps grid, Notepad for documents,
  Look for themes. The header says just "Welcome", since the card itself
  says the rest. The card is 480x250; its "Got it" chip and the header's
  x still close it for good. `WELCOME_LINES` keeps the plain text.
- A notice wears the icon of the app it is about (`notice_app`): saves,
  opens and restores are a document's (Notepad), a timer's is the
  Timer's, a failed search is Files'. The icon is a 20px picture in the
  card's overlay and the text moves over three cells for it. Notices
  about nothing in particular are as before.

## Why

Compared with the reference image, the first thing a new desk shows was
the plainest: five lines of monospace text. Icons tie the tips and the
notices to the dock the user is looking at.

## Tests

- `desk::tests::welcome_is_drawn_and_a_notice_wears_its_apps_icon`
- The Welcome and save-notice tests read the new title and indented text.
- Machine: `cargo xtask gauntlet` exit 0; the history shape's notice pins
  at x=1200 still land on the card.
