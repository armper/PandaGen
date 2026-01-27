# Phase 137 Summary

## Summary
- Scoped the panic handler, global allocator, and alloc error handler to bare-metal targets.

## Rationale
- Prevents duplicate `panic_impl` and allocator handler conflicts when building the workspace for host targets with `std`.

## Tests
- Not run (not requested).
