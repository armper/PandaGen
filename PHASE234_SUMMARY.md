# Phase 234: Notification Layer With Toasts And Persistent Status Cards

## Summary

This phase implements `GFX-034` from the graphics roadmap.

- `ShellNotice` gains an optional `title` (`ShellNotice::new`, `with_title`, `card_title`), so a card can say what it is ("UNSAVED") instead of only how severe it is. Transient notices keep the level label as their title.
- The workspace records the open document's path (`editor_title` returns it, or "new buffer"), and the editor window title now shows it.
- Persistent status card: while the editor buffer is dirty, the desktop model prepends an "Unsaved changes: <document>" card titled UNSAVED. It is computed from state on every frame rather than queued, so it appears the moment the buffer changes and vanishes the moment it is saved or discarded, with no expiry bookkeeping.

## Rationale

Toasts (Phase 231) are events: they arrive, they expire. Status cards are conditions: they are true or false right now. Modelling the second kind as derived state instead of a queued notice with an infinite TTL means there is nothing to dismiss and nothing that can go stale. Both kinds share one card renderer and one notification layer in the shell, which is what the roadmap asks for.

## Verification

- `cargo test -p services_gui_host` and `cargo test -p kernel_bootstrap` (shell test now covers a titled card and card ordering).
- QEMU (`cargo xtask qemu-script`): with `readme.md` open, notice-title pixels in the top-right region doubled after typing a character in insert mode (the UNSAVED card joined the mode-switch toast) and returned to the previous count after `:q!`.
