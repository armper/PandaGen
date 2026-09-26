# Phase 411: the shell's words for who may do what (FS-005)

## What changed

The Terminal answers, through the filesystem's guard (`access_shell`,
one line in, lines out -- testable on a RAM disk):

| Word | What it does |
|---|---|
| `whoami` | who the console acts for, their clearance, whether they administer |
| `people` | everyone, their clearance, whether they have a passphrase |
| `adduser <name> [clearance]` | a new person (the administrator) |
| `clearance <name> <level>` | set someone's clearance (the administrator) |
| `passwd <new>` / `passwd <old> <new>` | set or change your own passphrase |
| `setpass <name> <new>` | set someone's passphrase and close their sessions (the administrator) |
| `login <name> <passphrase>` / `logout` | sign in as someone; sign out (then nothing opens) |
| `share <doc> <person> <rights> [for=<secs>] [since=<secs ago>]` | a grant, derived from what you hold |
| `unshare <doc> <grant>` | revoke it, and what was derived from it |
| `grants <doc>` | who holds what on a document (if you may share it) |
| `label <doc> [level]` | show or set its label |
| `roles`, `role add/join/leave/remove` | roles over your documents by tag, one document, or all |
| `audit [doc]` | the decisions on your documents (all of them for the administrator), refusals and why |

The machine's administrator is the person the console signs in as at
boot. Administering adds people and sets passphrases and clearances; it
is not a way into anyone's documents.

Passphrase salts come from the time-stamp counter and a count, hashed.

## Why

The guard (409) and the kernel's sign-in (410) make every call decided;
this makes it usable before the desk has its own views of it (those come
next: a sign-in screen, sharing on a document, the audit log).

## Tests

- `access_shell::tests::the_shell_tells_the_whole_story`: the
  administrator adds Alice and Armando; Alice writes a plan with history
  and shares it for reading; Armando reads it, is refused its history,
  its grants, relabelling and its audit log; Alice sees his refusal in
  hers and revokes; a role over #work cannot reach a confidential plan;
  passphrases change, logout closes everything, login opens it again.
- In QEMU: the owner adds Armando, sets his passphrase, shares
  `readme.md` for reading; after a reboot Armando signs in, lists only
  `readme.md`, reads it, finds `test.txt` not there, and is refused its
  grants.
- Machine: `cargo xtask gauntlet` exit 0.
