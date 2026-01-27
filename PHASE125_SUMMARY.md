# Phase 125 Summary

## Summary
- Initialized command palette results on open for immediate feedback.
- Mapped Ctrl+P control byte to `KeyCode::P` so palette input stays responsive when Ctrl is held.

## Rationale
- Prevents the palette overlay from appearing empty or unresponsive after opening.
- Avoids dropping Ctrl+P input events while the palette is open, which felt like a freeze.

## Tests
- Not run (not requested).
