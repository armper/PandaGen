# Phase 144 Summary

## Summary
- Added a visible prompt prefix for workspace vs CLI mode in the bare-metal prompt rendering.
- Made prompt rendering and cursor math prefix-length aware for VGA and framebuffer paths.
- Aligned serial/echoed command lines with the on-screen prompt prefix.

## Rationale
Users were seeing a mode marker only in output history, not on the active input line. A consistent prompt prefix removes ambiguity without adding new UI chrome.

## Tests
- Not run (UI-only change).
