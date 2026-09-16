# Phase 228: Clipping Containers And Scroll Regions

## Summary

This phase implements `GFX-028` from the graphics roadmap.

**`graphics_rasterizer::ContainerTarget`**: a `RenderTarget` adapter that gives children their own coordinate space. Local `(x, y)` maps to `viewport.x + x - scroll_x, viewport.y + y - scroll_y` and is clipped to the viewport. `new` is a clipping container with a local origin; `scrolled` is a viewport onto larger content. Containers nest, and every provided draw op (text, images, lines, rounded shapes) works inside one because they all go through `write_pixel`.

**`services_gui_host::scene::ScrollRegion`**: a vertical viewport over line content. It owns the offset and content height and provides `scroll_by`, `clamp`, `scroll_line_into_view`, `visible_lines`, `scrollbar_thumb` (computed from offset, with a minimum grabbable length), `line_offsets`, and `render_lines`, which paints only intersecting lines through a `ContainerTarget`.

## Rationale

Editors, command output, file lists, and the palette all show content taller than their window. Making the viewport a target adapter rather than a drawing-time subtraction means child code paints at content coordinates and cannot escape its bounds, which is the same isolation argument the scissor already made for window chrome. Keeping the scrollbar thumb as a pure function of the offset removes a whole class of "thumb disagrees with content" bugs.

## Tests

- `graphics_rasterizer`: container translation and clipping through provided ops, scrolled window onto content (including text drawn at content coordinates matching an unscrolled reference), nested containers clip to the intersection.
- `services_gui_host::scene`: offset clamping under scrolling and content shrink, visible-line ranges and scroll-into-view with least movement, scrollbar thumb geometry including track end, minimum length, and no-scrollbar when content fits, and pixel-exact viewport-clipped rendering.

Validated with `cargo test -p graphics_rasterizer -p services_gui_host` and `cargo build -p services_gui_host --no-default-features`.
