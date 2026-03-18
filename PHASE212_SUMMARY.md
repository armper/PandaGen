# Phase 212: Add Framebuffer Desktop Presenter

## Summary

This phase implements `GFX-016` from the graphics roadmap.

`kernel_bootstrap/src/framebuffer.rs` now defines an explicit presentation contract for graphical desktop frames:

- `DesktopSurface` describes a full-frame desktop pixel buffer
- `DesktopSurfaceFormat::Rgba8888` captures the GUI host raster output format
- `present_desktop_surface(...)` validates dimensions and buffer length before presenting
- the presenter converts GUI-host RGBA pixels into framebuffer-native bytes while respecting framebuffer stride

The new path is intentionally separate from the existing text-console machinery. It creates the kernel-side boundary needed for a graphical desktop without yet rewriting the current framebuffer workspace loop.

## Rationale

The previous graphics phases could render a desktop into deterministic RGBA buffers, but the kernel framebuffer path still only knew how to blit raw native bytes. That left a gap between the next-gen desktop renderer and the hardware presentation layer.

This phase closes that gap with an explicit mechanism:

- GUI-side rendering remains policy and composition
- kernel-side presentation is now a strict, testable contract
- format conversion happens at the presentation edge instead of leaking framebuffer assumptions back into the compositor

That keeps the architecture aligned with the project philosophy: clean boundaries, explicit contracts, and no legacy console assumptions masquerading as graphics.

## Tests

Validated with:

- `cargo test -p kernel_bootstrap --bin kernel_bootstrap framebuffer::tests`
- `cargo test -p services_gui_host golden_fixture`
- `cargo test -p kernel_bootstrap`
- `cargo test --all`
