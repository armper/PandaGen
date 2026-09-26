# Phase 415: the Audit card -- who tried what (FS-009)

## What changed

- A card for the audit log (`audit_card::AuditView`, from the palette's
  "Audit"): newest first, when, who, what they asked for, which document,
  and what the guard said -- "ok  grant 1", or "REFUSED  holds only read"
  in red on a lit row. The **Refused** chip narrows it to refusals (R),
  **Again** reads it again (Ctrl+R); arrows scroll.
- Each person sees the decisions on their own documents; the
  administrator sees them all (`guard::audit_lines`).
- The log no longer keeps an owner's own allowed reads and writes of
  their own documents: the desk's search reads every document, and those
  entries crowded the bounded log until other people's reads and the
  refusals -- what the log is for -- fell out of it.
- A write with no time given (the shell's `write`) is stamped with the
  filesystem's clock, so its audit line and its entry have a real time.

## Tests

- `audit_card::tests`: lines, the refusal filter, scrolling, again.
- `guard::tests::the_audit_card_shows_the_owner_what_was_refused`: Alice
  sees Armando's refused history read first, and his allowed read;
  Armando sees nothing of hers.
- `desk::tests::audit_opens_from_the_palette_narrows_and_asks_again`.
- Machine: `cargo xtask gauntlet` exit 0.
