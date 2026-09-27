# Phase 443: Tasks moves out, holding one document to read and write (PROC-011)

## What changed

**Tasks** is a program. It is the fifth dock app out of the kernel, and
the first program that keeps a document of its own. It reads its list,
the document `tasks`, when it starts, and saves it a second after the
last change (or at once, if its card closes with a change waiting). It
draws the same card the kernel used to (the existing pixel checks pass
unchanged), and it can reach exactly one document: `tasks`.
`kernel_bootstrap/src/tasks.rs` is gone.

### Documents have rights

A Documents capability now carries **rights** as well as its pattern,
and every call checks the one it needs:

| right | call | what it does |
|---|---|---|
| list | `list(docs)` | the names the pattern names, as a message |
| open | `open(docs, name)` | the desk opens it in a Notepad for the person |
| read | `read(docs, name)` (new) | its content, as a message, or "absent" |
| write | `write(docs, request)` (new) | replace its content: a new version, the old ones kept |

The Calendar asks for `####-##-##` with **list, open**: it can see which
days have notes but never read one. Tasks asks for `tasks` with **read,
write**: it can keep its list but not see what else exists. A right not
asked for is refused (`NotAllowed`), and so is a name the pattern doesn't
match, before anything leaves the kernel. `programs` shows both:
"documents named tasks (read, write)".

A program's write goes through the same storage as a Notepad's save, as
the person signed in, so it keeps versions and respects access control.
Like a save, it sends every list-holder its names again. The serial log
records each read and write ("documents: thread 1 wrote tasks (9
bytes)").

### Typed messages

Messages now say what they are: **names**, a **document** (name and
content), **absent**, **written** (whether it was kept). They are
defined once in `app_protocol::message`, used by the kernel to make them
and by programs to read them in place, so a program needs no heap to
read one. A write request is framed the same way (name length, name,
content). The Calendar reads its names through this now.

### `tasks_core` and `apps/tasks`

The list, its document format (`[x] done`, `[ ] not yet`) and its card
moved from the kernel with their tests, and are tested on the host. The
header chips it had while a new task is typed (Add, Cancel) are now drawn
buttons ("Add it", "Cancel") in place of the usual four. Every row is a
hit area.

### The desk

Tasks maps to its program like the other dock apps. The desk no longer
reads the `tasks` document for it, saves it for it, or keeps the quiet
save that did so. `tasks` still wears the Tasks icon in Files, and the
Calendar no longer asks for a file listing it no longer uses.

## Tests

- `app_protocol`: messages round-trip, unknown kinds are nothing, and a
  name running past the end is refused; write requests round-trip, and
  one too big for its buffer is refused.
- `program_image`: rights travel with the pattern and describe
  themselves; no rights, or rights nobody knows, are not an ask.
- `syscall_abi`: rights not granted are refused; the holders re-listed
  after saves are only those with list.
- `tasks_core`: the list round-trips as a document; keys and clicks tick,
  add, move and remove, then it saves itself; the prompt's buttons are
  "Add it" and "Cancel"; long lists scroll (tests moved, and now through
  decoded views).
- `desk`: Tasks is a program that keeps its own document: its card waits
  for its program, the desk asks for nothing, and `tasks` is the Tasks
  app's.
- Gauntlet: the Tasks shape now also expects its program and its write,
  and its pixel checks pass unchanged. A new two-boot shape, "Tasks keeps
  its list", adds "milk" (read absent, then wrote 9 bytes), reboots, and
  expects the restored card's program to read the 9 bytes back.
