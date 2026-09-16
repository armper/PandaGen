# Phase 227: Image And Icon Surfaces

## Summary

This phase implements `GFX-027` from the graphics roadmap in `graphics_rasterizer`.

- `RgbaImage`: owned RGBA8888 pixels with `new` (transparent), `from_rgba` (length-checked), `from_ascii_art` (rows plus a character-to-colour palette, padded to the widest row), `pixel`, `set_pixel`, and `as_ref`.
- `ImageRef`: borrowed view with the same validation, so static assets and off-screen buffers blit without copying; `RgbaBuffer::as_image` exposes a buffer this way.
- `RenderTarget::blit_image(x, y, image)`: alpha-aware blit with signed origin and clipping. Alpha 0 leaves the target untouched, 255 overwrites, in between blends source-over.
- `blend_over(dst, src)`: integer source-over blend producing an opaque result, shared by any future translucent drawing.

## Rationale

Shell visuals (launcher icons, status glyphs, notification badges) and future app surfaces need pictures, not only glyphs and shapes. Keeping the image type a plain RGBA byte array with explicit dimensions matches the presenter contract at the framebuffer edge and stays trivially serialisable for remote UI. ASCII-art authoring lets small icons live in source and be reviewed in diffs, in the same way the cursor sprite already is. Alpha handling lives in the trait so icons compose identically on every target, including under a scissor.

## Tests

- construction: byte-length validation for owned and borrowed images, ASCII art with transparent cells and short rows, `ImageRef` validation
- blitting: opaque, transparent, and half-alpha pixels; negative, overflowing, and fully off-target origins touch only the overlap; buffer-as-image blit under a scissor
- blending: endpoints, midpoint values, blend over white

Validated with `cargo test -p graphics_rasterizer` (18 tests) and `cargo build -p graphics_rasterizer --no-default-features`.
