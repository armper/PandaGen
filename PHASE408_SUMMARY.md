# Phase 408: proving who you are -- passphrases, accounts, sessions (FS-002)

## What changed

- `authority::credential`: an account keeps a random salt and a verifier,
  PBKDF2-HMAC-SHA256 of the passphrase (`ROUNDS` = 4096), never the
  passphrase. A login derives the same and compares in constant time. The
  hash is `remote_ipc`'s own SHA-256; the PBKDF2 is checked against the
  RFC 7914 vector.
- A name with no account costs the same work as a wrong passphrase and
  gets the same answer, so a login does not say which names are real.
- After `FREE_TRIES` (5) misses an account locks for `LOCK_SECS` (30s),
  doubling with each further miss up to ten minutes.
- A person changes their own passphrase only by giving the old one; the
  system sets anyone's; the system itself has no passphrase.
- `Sessions`: a good login opens a session, held as a slot and a
  generation. Closing it bumps the generation, so an old handle -- or a
  forged one -- speaks for no one.

## Why

Deciding who may do what (Phase 407) needs someone to be sure *who*. The
two are kept apart: nothing below a session sees a passphrase, and
nothing above it sees a verifier.

## Tests

`authority::credential::tests`: the RFC vector; a passphrase opens a
session and nothing else does (unknown names answer the same, forged and
closed handles are nobody); guessing locks for longer each time; a
person changes their own passphrase only with the old one; what is kept
holds no passphrase and still verifies after a round trip.

Machine: `cargo xtask gauntlet` exit 0.
