# Phase 124 Summary

## Summary
- Increased the kernel bootstrap heap allocation to reduce early runtime allocation failures under QEMU.

## Rationale
- The previous 64-page heap (256 KiB) exhausted quickly during initialization and input handling. Expanding to 256 pages (1 MiB) provides headroom while keeping the allocator simple and deterministic.

## Tests
- Not run (not requested).
