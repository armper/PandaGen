# Phase 244: Compositor Property Tests

## Summary

This phase implements `GFX-046` from the graphics roadmap with dependency-free property tests (a deterministic xorshift generator, a few hundred cases each).

`services_gui_host/tests/property.rs`:
- rendering a scene is independent of window list order
- every pixel outside all window rectangles and the cursor sprite is the theme background
- `hit_test` returns exactly the top-most window in `composition_order` at any pixel
- repainting an arbitrary damage rectangle over an old frame matches a full render of the new scene inside that rectangle
- the damage computed by `diff_scenes` is sufficient: an incremental repaint from any old scene to any new scene (including caret-only nudges) equals a full render everywhere

`graphics_rasterizer`: random fills, lines, rounded shapes, text, and borders drawn through a random `ScissorTarget` or `ContainerTarget` never write outside the region.

**Bug found and fixed**: the title-bar separator line is painted one row below the chrome rectangle, but `raster_title_bar` returned early when the damage region did not intersect the chrome, so an incremental repaint whose damage covered only the content rows lost the separator (visible as a missing focus-ring segment). The separator is now painted first, on its own clip, before the chrome check. Full renders were unaffected, which is why golden fixtures did not change.

## Rationale

The benchmarks bound work on typical desktops; the properties are the guarantees the transport and the incremental viewer rely on for arbitrary ones. The separator bug is exactly the class of error damage-based repainting invites, and the "delta damage is sufficient" property is the test that will keep catching it.

Validated with `cargo test -p services_gui_host` (unit and property targets) and `cargo test -p graphics_rasterizer`.
