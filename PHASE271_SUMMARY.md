# Phase 271: Model-Based Checking Of The Kernel Heap

## Summary

Applies the Phase 264-270 approach to code that runs on the metal: the kernel's global allocator (`kernel_bootstrap::free_list_heap`, a coalescing first-fit free list behind a spinlock since Phase 254).

- `kernel_bootstrap/tests/heap_model.rs` drives a `FreeListHeap` over a host arena and checks after every allocation and free:
  - H1 alignment of every returned pointer;
  - H2 containment inside the arena;
  - H3 live blocks never overlap;
  - H4 integrity: every live block still holds the byte pattern written into it, so no header or free-list write ever lands inside a live block;
  - H5 stats: `used + free == total`, `largest_free <= free`, `free_blocks == 0` exactly when nothing is free, allocation and free counters match, `used` covers the live payload;
  - H6 coalescing: with everything freed the heap is one free block of the full size, whatever the free order;
  - H7 availability: a request that fits in the largest free block with room for header, padding, and a remainder block never returns null.
- Runs: 400 fixed-seed random sequences of 200 allocate/free operations (sizes 1-300, alignments 1-64, up to 64 live blocks), all 24 free orders of four mixed blocks, and exhaust-then-release from the middle. About a second.
- Host builds of the kernel binary (which integration tests trigger) now compile: `GLOBAL_HEAP` has a plain host stand-in that is never installed as an allocator.

## Result

No defect found. The allocator that has been carrying the desktop's per-frame allocations since Phase 254 satisfies all seven properties under adversarial free orders.

## Verification

- `cargo test --workspace` green; the bare-metal kernel build is unchanged.
