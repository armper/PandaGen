# Phase 249: Renderer Backend Contract

## Summary

This phase implements `GFX-050` from the graphics roadmap.

- `services_gui_host::backend::RenderBackend`: `capabilities()` (name, damage repaint support, accelerated flag, maximum surface pixels), `surface_size()`, `render(scene)` (full), `render_damage(scene, rect)`, `pixels()`, and `frames_rendered()`. Errors are typed: `SurfaceTooLarge` at construction, `SizeMismatch` when a scene exceeds the surface.
- `SoftwareBackend`: the authoritative CPU implementation over the compositor, capped at a 4K surface so a misconfigured mode cannot exhaust a bare-metal heap. Tests prove its output is byte-identical to direct compositor rendering for full and damage-limited frames, that it rejects oversized surfaces and scenes, and that it works as a trait object.
- The kernel's `DesktopFrameRenderer` now owns a `SoftwareBackend` and builds a `DesktopScene` per frame (also exposed via `scene()` for the remote UI path), so what runs on bare metal is exactly the contract a GPU backend would have to satisfy.

## Rationale

The compositor could already paint into any `RenderTarget`; that is the wrong seam for acceleration because a GPU backend would not paint pixel by pixel. The backend seam is one level up: a whole scene in, presentable pixels out. Keeping the software backend authoritative (golden fixtures and property tests target it) means an accelerated backend is an optimisation that must match it, never a semantic dependency, which is the roadmap's Milestone D definition.

Validated with `cargo test -p services_gui_host -p kernel_bootstrap`, the bare-metal build, and a QEMU session showing the desktop rendering through the backend with 26 frames and 26 presents and no rejections.
