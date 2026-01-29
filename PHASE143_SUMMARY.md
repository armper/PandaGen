# Phase 143 Summary

## Summary
- Added platform-aware QEMU display selection for xtask QEMU commands.
- Added `QEMU_DISPLAY` override for explicit display backend choice.

## Rationale
The previous hard-coded display backend (`cocoa`) only works on macOS. Choosing a default per OS prevents QEMU launch failures on Linux while preserving macOS behavior, and the environment override supports custom setups.

## Tests
- Not run (launcher change only).
