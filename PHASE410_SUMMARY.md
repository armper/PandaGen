# Phase 410: the kernel signs someone in (FS-004)

## What changed

- At boot, after mounting and seeding, the kernel boots the guard
  (`guard::boot`): it takes up `.authority`, or -- on a disk without one --
  makes the disk's first person, named by `owner=<name>` on the kernel
  command line (`owner` by default), cleared for secret. That person is
  who the console signs in as (`GuardState.console`, kept across boots).
- Whatever nobody owns -- the example files the kernel seeds, or every
  document on a disk from before owners -- is adopted by that person and
  given a document id. `.authority` stays the system's.
- The console then acts for that person: the desk, the Notepad and the
  shell all go through their session from here on.
- The kernel keeps a changed guard -- new grants, the audit log -- every
  `SAVE_EVERY` ticks (five seconds).
- The serial log says who signed in: `authority: signed in as owner (the
  disk's first person)`.

## Why

A guard that always acts as the system enforces nothing. Until the desk
has a sign-in screen, the machine's first person is who is at the
console; the shell (Phase 411) lets anyone else sign in.

## Tests

- `guard::tests::the_first_boot_makes_a_person_and_later_boots_sign_them_back_in`:
  the owner's name from the command line; the first boot makes the
  person and adopts the seeded file; a later boot signs the same person
  back in from `.authority`, whatever the command line now says.
- In QEMU: two boots on one disk -- the first makes the owner and adopts
  the examples, the second signs them back in and Files shows what they
  saved on the first.
- Machine: `cargo xtask gauntlet` exit 0.
