# Phase 214: Route Framebuffer Blit Through The Desktop Presenter

## Summary

This phase implements `GFX-017` from the graphics roadmap.

`kernel_bootstrap/src/framebuffer.rs`:

- `BareMetalFramebuffer::blit_from` is now private. The only public way to write a whole frame into the hardware framebuffer is `present_desktop_surface`, which validates dimensions, buffer length, and stride first.
- `BareMetalFramebuffer::present_shadow(&shadow)` presents an off-screen shadow framebuffer as a `NativeRgb32` desktop surface. A shadow whose geometry does not match the target is rejected instead of silently copying a byte prefix.
- `DesktopPresentError::UnsupportedSourceFormat` covers a future pixel-format mismatch between shadow and target.
- `BareMetalFramebuffer::buffer()` gives read-only access to pixel data so the presenter can read a shadow without taking `&mut`.

`kernel_bootstrap/src/render_stats.rs` now counts desktop presents, rejected presents, and presented pixels, exposed through `RenderCumulativeStats`.

`kernel_bootstrap/src/main.rs` presents the workspace shadow through `present_shadow`. A rejected present is logged on serial and counted rather than discarded with `let _`.

## Rationale

Before this phase the desktop presenter existed next to the live blit path rather than underneath it. The workspace loop could still copy raw bytes into the hardware buffer with no validation, and the one call that did use the presenter threw the result away.

Now there is a single validated present edge shared by the text-workspace shadow and the future graphical desktop path. Present counts are observable, which is the input the frame pacing policy (`GFX-018`) needs, and a contract violation at the present edge is loud instead of silent.

## Tests

Added:

- `test_present_shadow_copies_backbuffer_into_target`
- `test_present_shadow_rejects_geometry_mismatch` (stride and dimension mismatch, target untouched)

Validated with:

- `cargo fmt --all`
- `cargo test -p kernel_bootstrap`
- `cargo build -p kernel_bootstrap --target x86_64-unknown-none -Zbuild-std=core,alloc`
- `cargo test --all` (baseline before the change; kernel_bootstrap re-run after)
