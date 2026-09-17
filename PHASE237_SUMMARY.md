# Phase 237: CLI Stays Text-Native; Host Surface Scrollback

## Summary

This phase resolves `GFX-038` from the graphics roadmap.

**Decision**: the CLI component keeps rendering as text lines with a prompt inside the workspace window. It already gets the graphical host's chrome, focus ring, caret, title ("CLI" when active), and status strip; a bespoke CLI renderer would duplicate the text-native workspace surface without adding capability.

**What the host surface gained instead**: wheel scrollback. `WorkspaceSession::scroll_view(notches, visible)` moves a `scrollback_offset` clamped to the history; any new output snaps it back to the live tail. The desktop builder shows the older window of lines with the prompt still pinned at the bottom and appends `(scrolled n lines)` to the title. In the routing loop, wheel notches over the workspace window (no editor or palette open) scroll it; the picker and palette keep their own wheel behaviour.

## Rationale

A graphical host surface for text-native components should add what a text console cannot do rather than restyle the text. Scrolling history with the pointer is exactly that. Snapping to the tail on new output keeps a running command visible, and pinning the prompt keeps input predictable while browsing history.

## Verification

- `cargo test -p kernel_bootstrap` (scrollback model test: older lines shown, prompt pinned, title suffix, clamping at the top).
- QEMU (`cargo xtask qemu-script`): after three `help` commands, three wheel notches over the workspace showed earlier lines and the title read `Workspace (scrolled 3 lines)`.
