# Phase 251: Composition Split And GPU HAL Exploration

## Summary

This phase implements `GFX-052` and `GFX-051`, the last stories of the graphics roadmap.

**Composition split (`GFX-052`)**: `services_gui_host::backend::CompositionStage` names the eight stages of composing a scene and fixes which stay CPU-side by contract (window ordering, hit testing, damage tracking, text layout) and which may be accelerated (shape fills, glyph runs, image blits, present). `BackendCapabilities` gains `accelerated_stages`, and `is_valid` rejects a backend that claims a CPU-side stage or claims stages without being accelerated. The software backend claims none. Tests cover the partition and both invalid claims.

**GPU HAL exploration (`GFX-051`)**: `hal::gpu::GpuSurfaceDevice` is the minimum a display device needs to host the backend contract: surface info, RGBA region upload, and an explicit flush, with typed errors for out-of-bounds rectangles, length mismatches, and device failure. `FakeGpu` (behind the `alloc` feature) records calls for host tests. The documented QEMU target is virtio-gpu, whose 2D scanout commands map one-to-one onto these operations.

## Rationale

The roadmap gated any GPU work on a stable, test-covered software path; that is now the case (golden fixtures, property tests, replay pixel identity, benchmarks). Rather than a driver, this phase pins down the two things a driver must respect: which semantics it may never take over, and the smallest device surface it must provide. Both are tested, so a future virtio-gpu backend has a contract to satisfy rather than a design to reconstruct.

Validated with `cargo test -p hal --features alloc`, `cargo test -p services_gui_host`, and the `no_std` build of the GUI host.
