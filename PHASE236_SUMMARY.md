# Phase 236: Graphical File Picker

## Summary

This phase implements `GFX-037` from the graphics roadmap.

**Workspace**: `FilePickerState { entries, selection }` opened by `open file` / `open picker` / `open files`, the `Open File` palette command (which the launcher lists), or a launcher click. While open it owns the keyboard: Up/Down or j/k move, Enter opens the selection in the editor, Esc or q closes. Opening requires the filesystem to be free (the editor must be closed, since it holds the storage handle). Pointer helpers `picker_hover`, `picker_scroll`, `picker_click`.

**Desktop**: `PickerModel` renders in the workspace window titled "Files" with a `ROOT /` breadcrumb on line 0, one entry per line, the selected entry highlighted, no caret, and a status strip `Files: n entries  Up/Down select  Enter open  Esc close`. `picker_entry_at_line` maps content lines to entries.

**Routing**: over the workspace window while the picker is open, motion moves the selection, wheel notches scroll it, and a primary press opens the entry.

## Rationale

The picker reuses the same three facilities as the palette: a highlighted row, hover-follows-pointer selection, and one shared "activate" path for keyboard and click, so the two choosers behave identically. It renders inside the workspace window rather than as an overlay because choosing a file is the workspace's activity, not a transient command. The text console gets the listing printed when the picker opens, so the command remains useful in both display modes.

## Verification

- `cargo test -p kernel_bootstrap` (84 lib + 78 bin, including the picker model test) and the bare-metal build.
- QEMU (`cargo xtask qemu-script`): `open file` listed three entries; `j` moved the highlight to `test.txt`; hovering with the mouse kept it there; a click logged `picker: open test.txt` and `Opened: test.txt`, and the graphical editor showed the file.
