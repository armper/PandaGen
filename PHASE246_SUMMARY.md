# Phase 246: Freeing Global Allocator

## Summary

`kernel_bootstrap/src/free_list_heap.rs` replaces the bump global allocator with a first-fit, address-ordered, coalescing free-list heap at 16-byte granularity. Each allocated block carries a header (total size and the pad between block start and header) right before the payload, so `dealloc` recovers the block from the pointer alone. Allocation splits front and tail fragments when they are usable blocks and absorbs them otherwise; freeing merges with both neighbours. `stats()` reports used, free, total, allocations, frees, largest free block, and free block count. `init_heap` installs the arena with `init` instead of the previous pointer-cast into the static. The kernel's internal `BumpHeap` remains for its own allocation records; only the `#[global_allocator]` changed. The `mem` command prints the fuller stats.

## Measurement

Same scripted QEMU scenario as Phase 245 (boot, switch to graphics, ten pointer moves), followed by `help`, opening and closing the editor, and opening and closing the palette:

```
heap: used=4322 KiB ... allocs=265   frees=135    blocks=2   (text mode)
heap: used=8323 KiB ... allocs=2811  frees=2678   blocks=2   (graphics)
heap: used=8323 KiB ... allocs=13841 frees=13708  blocks=2   (after ten pointer moves)
heap: used=8326 KiB ... allocs=37588 frees=37454  blocks=2   (after help, editor, palette)
```

Before this phase the same ten pointer moves grew the heap by 580 KiB; now usage is flat, and the free space stays in two blocks (no fragmentation growth) through 37,000 allocations.

## Rationale

The desktop is built as fresh data every frame by design; that is only viable if memory comes back. A simple free list is enough for a single-CPU kernel with this allocation pattern and is far easier to reason about than a size-class allocator. It is host-tested (alignment up to 4 KiB, full coalescing after arbitrary free order, exhaustion returning null and recovering, 10,000 alloc/free cycles with zero growth) before it runs under the kernel.

Validated with `cargo test -p kernel_bootstrap`, the bare-metal build, and the QEMU session above (no allocation errors or panics).
