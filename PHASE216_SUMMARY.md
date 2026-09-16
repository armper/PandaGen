# Phase 216: Add Damage-Region Presentation

## Summary

This phase implements `GFX-019` from the graphics roadmap.

`kernel_bootstrap/src/framebuffer.rs`:

- `DamageRect` is a pixel rectangle with `union` and `intersect`.
- `DamageTracker` accumulates a single bounding box of everything drawn since the last present.
- `BareMetalFramebuffer` owns a tracker. Every draw entry point records damage: `draw_char_at`, `draw_text_at` (both paths), `draw_line`, `clear_text_span`, `clear_text_row`, `draw_cursor`, `clear`, `scroll_up_text_lines`, whole-frame presents, and raw `buffer_mut` access (conservatively marks everything).
- `present_shadow_damage(&mut shadow)` consumes the shadow's damage and copies only that region into the target, row by row, respecting stride. A clean shadow presents as a no-op with zero copied pixels. Geometry mismatches are rejected before any bytes move.
- `damage()`, `take_damage()`, and `full_rect()` expose the tracker for callers and tests.

`kernel_bootstrap/src/main.rs` now presents through `present_shadow_damage` at the single paced present point.

## Rationale

A paced present (`GFX-018`) bounds how often the hardware framebuffer is touched. Damage presentation bounds how much of it is touched. Together they turn a keystroke into one row-band copy per tick at most, instead of a full-frame copy per render pass.

The tracker is a single bounding box on purpose. The text workspace draws in row bands, so the box is usually tight, and one box means one predictable copy loop with no allocation. The presenter contract takes a rectangle, so a finer region list can be introduced later without changing callers.

Damage is recorded at the draw edge rather than inferred by diffing buffers, because diffing would cost a full-frame read on every present and defeat the purpose.

## Tests

Added in `framebuffer::tests`:

- `test_damage_rect_union_and_intersect`
- `test_draw_calls_accumulate_tight_damage` (every draw entry point, take/clear semantics, scroll and clear mark everything)
- `test_present_shadow_damage_copies_only_damaged_region` (pixel-exact check that bytes outside the box are untouched, no-op on clean shadow)
- `test_present_shadow_damage_rejects_geometry_mismatch_and_consumes_damage`

Validated with:

- `cargo fmt --all`
- `cargo test -p kernel_bootstrap`
- `cargo build -p kernel_bootstrap --target x86_64-unknown-none -Zbuild-std=core,alloc` (no new warnings)
