# Phase 417: the Terminal's line -- a caret, history, Tab, and stars (KBD-012)

## What changed

The workspace prompt (the desk's Terminal card, and the text-mode console)
was an append-only buffer from the first phases. Every byte above ASCII --
arrows, Home, End, Delete -- was dropped before routing, so:

- Up and Down were `// not implemented yet`: no history.
- Left and Right did nothing: a typo at the start of a line meant
  backspacing over all of it.
- The palette's own arrow handling and the file picker's arrow keys were
  unreachable (only `j`/`k` worked there).
- The text editor moved only with `h`/`j`/`k`/`l`, and not at all in
  insert mode.

Now:

- **`kernel_bootstrap/src/line_edit.rs`** -- a line editor with no screen:
  bytes in, an `Edit` out (`Appended`, `Redraw`, `Submit`, `Choices`...).
  Left/Right/Home/End move the caret; Backspace and Delete edit at it;
  Ctrl+U clears; Up/Down walk a 64-line history and bring back what was
  being typed when the walk began; Tab finishes a command's name (one
  match completes it, several extend it as far as they agree and are
  listed). The prompt and the CLI share the one line and its history.
- **Passphrases** (`login`, `passwd`, `setpass`) show as stars while typed,
  are starred in the echoed command line (the transcript and the serial
  port), and are never kept in history. `access_shell::secret_from` says
  where the secret starts in a line.
- **The Terminal card's caret** was drawn five cells past the text: the
  prompt's width was added twice. Fixed.
- **The per-keystroke `route_input` trace** -- three serial lines for every
  key, which would also have carried a passphrase's keys -- is behind
  `ROUTE_TRACE` (off).
- **The editor**: arrows and Delete are mapped in `minimal_editor`, and
  `editor_core` moves with arrows in insert mode and deletes with Delete
  in both modes.
- **`help`** lists `editor [path]` and the line's keys.

## Why

These were the earliest "not yet" items (Phases 68, 72, 141) and the most
felt: every session at the Terminal hits them. Keeping the editor apart
from the serial port and the desk means all of it runs under
`cargo test`.

## Tests

- `line_edit`: caret moves and mid-line edits; history walk with the draft
  kept and a recalled line becoming the line when edited; passphrases
  starred and not remembered; Tab completing, extending and listing.
- `editor_core`: arrows in insert mode stop after the last character;
  Delete deletes in insert and normal mode.
- `workspace` tests moved onto the line.
- Gauntlet shape "the Terminal: history, a moved caret, Tab, a passphrase
  in stars": Up/Home/`x` gives `WS > xmem`, `whoa`+Tab runs `whoami`,
  `login a hunter` is echoed as `WS > login a ******` and `hunter` never
  reaches the serial port.
- `cargo xtask gauntlet`: exit 0.
