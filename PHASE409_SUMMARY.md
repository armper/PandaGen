# Phase 409: the filesystem decides (FS-003)

## What changed

- Every directory entry carries a stable `doc_id` (kept through saves),
  an `owner` (a principal number, `NOBODY` until adopted) and a `label`.
  They default on disks from before, which stay readable.
- `BareMetalFilesystem` is the reference monitor: it holds a `Guard` (the
  authority, the accounts, the open sessions, and the *actor* -- the
  system at boot, then a session), and every public call asks it first:
  - reading takes `READ`, versions take `HISTORY` and only reach back as
    far as the actor's grant does (`read_version` counts the versions the
    actor can reach), writing over a document takes `WRITE`, tags take
    `TAG`, the bin and deleting take `DELETE`;
  - a document the actor may not see -- no rights on it, or labelled
    above their clearance -- is not there: listings leave it out and
    reads say "not found", and its name cannot be written over;
  - a new document is the actor's; a save keeps its id, owner and label,
    and a tag change cannot touch them;
  - a closed session acts for nobody: everything is refused.
- Sharing lives on the filesystem too: `share(name, to, terms)`,
  `revoke`, `grants_on` (for those who may share), `relabel` (the owner,
  within their clearance), and `adopt_unowned` (the system gives what it
  wrote before anyone existed to a person).
- The guard's state -- principals, grants, roles, audit log, verifiers --
  is kept in `.authority`: owned by the system, labelled secret, out of
  every person's reach, saved and loaded by `save_guard`/`load_guard`.
- Denials are their own error, `TransactionError::Denied`, carrying the
  reason. The by-object-id read is test-only and the by-id write is gone:
  a storage id is how the storage finds a document, not a way in.
- Files entries carry the owner's name, the label and what the viewer
  holds, for the views to come.

## Why

A decision nobody enforces is advice. Putting the guard inside the
filesystem object -- the one thing the desk, the Notepad and the shell
all pass around -- means there is no path to a document that does not
ask it.

## Tests

`guard::tests`: Armando reads Alice's plan but not its history, cannot
write, tag, bin, delete or re-share it, cannot see or overwrite her
diary, and after revoking sees nothing; a history window and a label
bound what is shared, a save keeps the label and owner and a tag change
cannot alter them; a closed session acts for nobody; adoption, and
`.authority` out of reach and carrying grants across a remount. Every
existing filesystem and desk test passes unchanged (the system acts
until someone signs in).

Machine: `cargo xtask gauntlet` exit 0.
