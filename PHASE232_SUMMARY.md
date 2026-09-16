# Phase 232: Theme Tokens And Graphical Window Chrome

## Summary

This phase implements `GFX-035` (theme tokens) and `GFX-032` (graphical title bar, focus ring, tab strip) from the graphics roadmap.

**`services_gui_host::theme::Theme`**: role-named colour tokens (background, surface, focused/unfocused border, focused/unfocused/notice/palette title fills, text, muted text, caret, active/inactive tab, pointer fill/outline) with `Theme::DEFAULT` (dark slate, matching the previous hard-coded colours) and `Theme::LIGHT`. `Compositor` now carries a theme (`Compositor::new()` uses the default, `Compositor::with_theme`), and every painted colour in windows, cursor, and background comes from it.

**Graphical chrome** in `raster_title_bar`:
- the title strip is filled with a tint chosen by role (notification, palette) and otherwise by focus
- a separator line under the strip is drawn in the border colour, so the focused window's accent ring continues through it
- windows with tabs draw each tab as a rounded box: the active tab in the surface colour (joining its content), inactive tabs in a muted fill with muted text
- unfocused window titles use muted text

Golden fixtures were regenerated with new colour classes (`T` focused title, `u` unfocused title, `n` notice, `p` palette, `m` muted text, `i` inactive tab) and reviewed. The palette header separator became ASCII so it renders in the desktop font.

## Rationale

Theme tokens first, chrome second: a title bar drawn from named tokens is a design that can be restyled by data, which is what the roadmap means by a coherent visual language rather than a fixed look. Tints by role make notifications and the palette recognisable without reading their titles, and tying the separator to the border colour turns the focus indication into one continuous ring instead of a coloured line plus a coloured box.

## Tests

- title bar tint per focus and role, separator colour, content untouched, muted unfocused title glyphs
- tab strip: active box colour, inactive box colour, rounded corner reveals the title fill, strip beyond the tabs is title fill
- a light theme changes background, border, title, caret, surface, and pointer pixels
- regenerated golden fixtures for the desktop and workspace-snapshot surfaces

Validated with `cargo test -p services_gui_host` (55 tests), `cargo test -p kernel_bootstrap`, `cargo build -p services_gui_host --no-default-features`, and QEMU screendumps of the shell with the palette open and the editor open.
