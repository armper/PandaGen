# Phase 231: Minimal Graphical Shell Surface

## Summary

This phase implements `GFX-031` from the graphics roadmap.

**`services_gui_host::shell`**
- `ShellModel` (status left/right text, launcher items, notices, workspace frame and title, optional palette frame) and `ShellViewIds` (stable ids for status, launcher, workspace, palette, and notice slots).
- `shell_tree`/`shell_layout`: the shell geometry declared with the layout vocabulary: a 2-cell status bar across the top, an 18-cell launcher strip on the left, the workspace filling the rest, up to four 36x3 notice cards anchored top-right inside the workspace area, and the palette centred at half size.
- `compose_shell`: turns the model into windows with roles `Main`, `Status` (chrome-less), `Launcher`, `Notification` (newest on top), and `Palette`. Status text is left/right aligned with truncation.
- New `DesktopWindowRole::Launcher` (workspace layer, not focusable) and `DesktopWindow::without_chrome` so shell strips start content at the border; hit testing and painting honour the chrome flag.

**Kernel**
- `desktop_frame` now builds the desktop through the shell; `DesktopLayout` wraps `ShellRects`.
- `WorkspaceSession::launcher_items` lists enabled, argument-free palette commands sorted by name and marks the open component; `activate_launcher_line` runs one. `run_palette_command` was extracted from the palette key handler so the launcher and the palette share one execution path, and `clear` now actually clears when run from either.
- Shell notices: `push_notice`, `stamp_and_expire_notices` (8 s TTL, scheduling the next expiry on the animation clock), `notices`. Display switches and unknown commands raise notices.
- The status bar's right side shows the display mode and tick count.
- In the routing loop, a primary press on a launcher content line executes that entry with a real kernel context.

## Rationale

The shell is the frame everything else lives in, so it is built from data and a declared layout rather than drawn ad hoc: the same model and size always yield the same windows, and every region has a stable identity for focus and hit testing. Feeding the launcher from the command palette means there is one registry of what the system can do. Notices reuse the animation clock so an expiring card is just another scheduled redraw.

## Verification

- `cargo test -p services_gui_host` (52 tests, 4 new shell tests including a routed launcher click) and `cargo test -p kernel_bootstrap` (83 lib + 77 bin, desktop tests rewritten for the shell).
- QEMU (`cargo xtask qemu-script`): after `display graphics`, screendumps show the status bar with `graphics | t=…`, the launcher strip, the workspace, an INFO notice for the mode switch and a WARN notice for a bogus command stacked newest-first, and the pointer. Moving the mouse over the launcher and clicking ran `clear` (`launcher: line=0 cmd=clear` on serial).
