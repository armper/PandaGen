# Phase 223: Desktop Hit Testing

## Summary

This phase implements `GFX-023` from the graphics roadmap in `services_gui_host`.

- `composition_order(windows)` returns window indices bottom-most to top-most using one shared `composition_sort_key` (layer policy, then z-index, then view id). The painter now sorts with the same key, so paint order and hit order cannot drift apart.
- `Compositor::hit_test(windows, x, y)` walks that order top-down and returns a `HitTarget { window_index, view_id, role, region, local_x, local_y }` for the first window whose pixel rectangle contains the point, or `None` over bare desktop.
- `HitRegion::{Border, Chrome, Content { line, column }}` classifies the hit using the same chrome and content rectangles the painter draws, so a content hit reports the text cell under the pointer.
- `window_pixel_rect` is exported for callers that need a window's pixel bounds.

## Rationale

Hit testing is compositor policy: only the compositor knows the z-order and the chrome geometry it painted. Sharing the sort key with the painter is the important design choice, since a hit test that disagrees with what is visible is worse than none. Reporting the content cell directly lets text-oriented components (editor, CLI, palette) react to a click without re-deriving the layout from pixels.

## Tests

- regions and content cells for a single window, including bottom border and bare desktop
- paint-order semantics: palette above main regardless of z-index, higher z-index wins within a layer, `composition_order` matches the painter
- windows too small for a content area report chrome and border only

Validated with `cargo test -p services_gui_host`, `cargo build -p services_gui_host --no-default-features`, and `cargo test -p kernel_bootstrap`.
