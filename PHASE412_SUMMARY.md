# Phase 412: the sign-in screen (FS-006)

## What changed

- **Who is here.** When the console's person has a passphrase, the kernel
  boots with nobody signed in (`Actor::Nobody`: everything is refused) and
  the desk shows the sign-in screen (`sign_in::SignInView`): the whole
  screen veiled, the time and date bottom-left as on the rest screen, and
  a panel with a tile for each person who has a passphrase, a passphrase
  field that shows a dot per character and never the characters, a Sign in
  button, and what went wrong -- "Wrong name or passphrase", "Too many
  tries: wait 30 s" -- in red. Keys: type, Backspace, Esc clears, Enter
  signs in, Tab and the arrows pick someone else; the pointer picks a
  tile and presses the button.
- Without a passphrase there is nothing to ask: the console is the first
  person's, as before (so a new disk, and the gauntlet, boot straight to
  the desk).
- The view holds the passphrase only until Enter; the request
  (`DeskRequest::SignIn`) takes it to the kernel, which asks the guard
  (`guard::sign_in`) and answers `Desk::signed_in` or
  `Desk::sign_in_refused`. The serial log says who signed in, or that a
  sign-in was refused.
- **Lock.** A rested desk (Ctrl+L, five idle minutes, or the palette's new
  "Lock") wakes into the same screen with only its person on it when they
  have a passphrase; the right one (`guard::unlock`) gives the desk back
  exactly as it was. A passphrase set at the shell locks the next rest.
- **Sign out.** A palette action: the session closes, the desk is made
  new -- every card closed, the look back to the default -- the console's
  transcript is cleared, and the sign-in screen asks who is here.
- **Settings are each person's.** `.look`, `.recent` and `.welcomed` are
  kept as `.look.<name>` and so on for whoever is signed in (the first
  person's old ones are read if they have none yet), so two people on one
  disk have their own look, recents and Welcome.
- A gauntlet shape (on its own disk, Phase 416) sets a passphrase in the Terminal, signs out
  from the palette, gives a wrong passphrase and then the right one.

## Why

Phases 407-411 made every document decided by who is signed in; until
now the console simply was the first person. Signing in, locking and
signing out are what make "who" true.

## Tests

- `sign_in::tests`: typing shows dots (the characters are never drawn),
  Enter asks once and forgets the passphrase, a refusal says why; people
  are picked by key or click; unlocking has only its person.
- `desk::tests::the_sign_in_screen_asks_who_is_here_and_locks_the_rested_desk`:
  only the screen shows and answers; the button by pointer; signing in
  gives the desk back; rest wakes plain without a passphrase and into the
  lock with one; unlocking keeps the cards; signing out closes them.
- `guard::tests::a_passphrase_makes_the_boot_ask_who_is_here`: nobody
  reads anything until sign-in; wrong and right passphrases; unlock; sign
  out.
- In QEMU and in the gauntlet: passphrase, sign out, refused, signed in.
- Machine: `cargo xtask gauntlet` exit 0.
