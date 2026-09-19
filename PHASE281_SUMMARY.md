# Phase 281: A Reply Channel With Two Readers

## The finding

`ChannelId(1)` had two consumers. The serial `ConsoleService` drained it in
its `poll`, and the graphical workspace loop drained it directly. Since
Phase 275 the console task runs on whichever CPU happens to draw it, so a
command typed into the graphical shell could have its reply consumed by an
application processor polling the console task. Not delayed: gone. The shell
showed nothing for that command, and the reverse happened too.

Reproduced in an ordinary keyboard-driven boot. `ConsoleService` prints a bare
`> ` prompt only after it consumes a response, and nothing was typed on the
serial line, yet the log carried seven of them for the seven commands issued
through the graphical shell. Every one of those replies went to the wrong
reader.

## The fix

The workspace opens its own reply channel and drains only that. Channel 1
stays the serial console's. A channel now has exactly one reader.

## Verification

`qemu-script` gains `--forbid-serial TEXT`, the mirror of `--expect-serial`:
the run fails if the text appears. Both options now understand `\n`, so a
line-anchored marker can be asserted.

Four commands issued through the graphical shell with
`--forbid-serial "\n> "`: passes, with zero bare prompts where there were
seven before. The final screendump shows the `mem` output in the shell window,
which is the user-visible half of the same claim.

`cargo test --workspace` green.
