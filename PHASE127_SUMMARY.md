# Phase 127 Summary

## Summary
- Forced a full redraw when the command palette closes to clear the overlay on framebuffer and VGA consoles.

## Rationale
- The palette state was closing, but the overlay could persist visually because incremental rendering didn't overwrite those rows.

## Tests
- Not run (not requested).
