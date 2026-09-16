# Phase 233: Graphical Command Palette

## Summary

This phase implements `GFX-033` from the graphics roadmap.

**`services_gui_host`**: `DesktopWindow::with_highlight(line)` paints one content row in the new `Theme.selection` colour before the text, giving any list window a selected-row look. `ShellModel.palette_selection` highlights the selected palette result, and the launcher highlights its active entry the same way.

**Kernel**
- `PaletteOverlayState::set_selection` selects a result directly (clamped).
- `WorkspaceSession` gains `palette_hover_result`, `palette_scroll`, `palette_click_result` (select and run through the shared palette execution path), and `palette_dismiss`.
- The desktop builder maps palette content lines to result indices (`palette_result_at_line`; line 0 is the search row).
- Pointer routing: over the palette, motion moves the selection to the hovered result, wheel notches move it up or down, and a primary press runs the hovered result. A primary press anywhere else while the palette is open dismisses it. Launcher clicks keep working as before.

## Rationale

The palette already had the right data and keyboard behaviour; what made it "text-only" was that the pointer could not touch it and selection was a `>` prefix. A highlighted row is a general window capability rather than palette-specific drawing, so the launcher got it for free and Epic 8 list surfaces (file picker) will too. Selection follows hover because a palette is a transient chooser: the user's intent is where the pointer is, and a click should never run something other than the highlighted row.

## Verification

- `cargo test -p services_gui_host` (56 tests) and `cargo test -p kernel_bootstrap`: highlight band painting and clipping, launcher and palette highlight lines, palette line-to-result mapping.
- QEMU (`cargo xtask qemu-script`): with the palette open, moving the mouse onto "Open Editor" moved the highlight there; a wheel notch moved it one row and back; a click logged `palette click: index=1 cmd=open_editor` and the editor opened (`New buffer [filesystem available]`).
