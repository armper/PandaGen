# Phase 235: Graphical Editor Renderer

## Summary

This phase implements `GFX-036` from the graphics roadmap.

- `MinimalEditor::set_viewport_rows` resizes the viewport while keeping the cursor visible; `line_count` exposes the document length. The document and cursor model are unchanged.
- `EditorModel` carries `first_line` (scroll offset), `line_count`, and `dirty`. The desktop builder renders a line-number gutter (`gutter_width`, `gutter_line`: at least three digits plus a space, blank past the end of the document), shifts the caret past the gutter, highlights the current line through the shell's new `workspace_highlight`, and marks a dirty document with `[+]` in the window title.
- The status strip shows `<mode>  <document>[+]  Ln n, Col m  (k lines)`.
- In graphics mode the loop sizes the editor viewport to the workspace window's content rows each frame, and restores the text console's 23 rows when switching back to text.

## Rationale

The roadmap asks for a graphical editor "using the same document state and cursor model it already has." Everything here is presentation over `MinimalEditor`: the gutter, highlight, and status are derived per frame from scroll offset, cursor, and dirty flag, so keyboard behaviour and file I/O are untouched and the text console renderer keeps working. Sizing the viewport to the window is the one state change, and it is reversible on mode switch.

## Verification

- `cargo test -p kernel_bootstrap` (editor model test covers gutter formatting, caret shift, highlight line, dirty title, status text, and blank gutter past EOF) and `cargo test -p services_gui_host`.
- QEMU (`cargo xtask qemu-script`): opened `readme.md`, moved down two lines and typed in insert mode. The capture shows numbered lines, the highlighted current line, the caret after the gutter, `readme.md [+]` as the title, the active launcher entry, and the status strip `-- INSERT --  readme.md [+]  Ln 3, Col 6  (5 lines)`. Switching back to text mode rendered the text console correctly.
