# Phase 138 Summary

## Summary
- Fixed host-build errors by gating global allocator initialization to bare-metal targets and avoiding a mutable borrow conflict in palette execution.

## Rationale
- The bare-metal allocator and panic/alloc handlers must not compile on host targets with `std`, and palette execution needed to drop the immutable borrow before mutating state.

## Tests
- Not run (not requested).
