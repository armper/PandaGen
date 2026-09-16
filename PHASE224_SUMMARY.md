# Phase 224: Cursor As A Desktop Surface

## Summary

This phase implements `GFX-025` from the graphics roadmap.

`services_gui_host`:
- `DesktopCursor { x, y, shape, visible }` with `CursorShape::Arrow`, `hidden()`, and `bounds()` for damage tracking.
- `Compositor::render_desktop_to_target_with_cursor(target, windows, damage, cursor)` paints windows then the cursor sprite last. `render_desktop_to_target_with_damage` delegates to it with no cursor, so existing callers and golden fixtures are unchanged.
- A 12x19 arrow sprite with outline and fill colours, clipped at target edges by the target itself. The cursor is painted even when the damage rectangle excludes it, so a partial repaint never erases a stationary pointer.

`kernel_bootstrap`:
- `DesktopModel.pointer: Option<(usize, usize)>`; `DesktopFrameRenderer::render` composes a `DesktopCursor` from it.
- `build_desktop_model` fills the pointer from the workspace when a mouse was brought up.
- Pointer events in graphics mode set `input_dirty`, so cursor motion re-renders the desktop through the existing paced present path. Text mode is unaffected.

## Rationale

In the roadmap's words, the cursor must be "a first-class surface, not a side effect of text rendering." Here it is a compositor input with its own position, shape, and visibility, drawn in its own pass above the z-ordered windows. Nothing in the text grid knows about it, and hiding it is a policy decision expressed by `visible`, not by overdrawing a cell.

Re-rendering the whole desktop on each motion event is the simplest correct approach and stays well within budget on the optimized kernel (a full 1280x800 present is under one 10 ms tick, and the pacer caps presents at one per tick). Cursor-only damage repaint is a later optimisation under Epic 10.

## Tests

- `services_gui_host`: cursor composes above a window with correct hotspot, fill, and untouched neighbours; hidden cursor paints nothing; sprite clipped at the surface corner; cursor survives a damage repaint elsewhere.
- `kernel_bootstrap::desktop_frame`: a model with a pointer paints the sprite hotspot at that pixel.

Verified in QEMU with `cargo xtask qemu-script`: after `display graphics`, screendumps show the arrow centred, then moved up-left by the injected delta, then pinned at the bottom-right corner after a large move.
