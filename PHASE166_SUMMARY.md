# Phase 166 Summary

## Overview
- changed macOS QEMU display preference to use SDL when available.
- documented the SDL-first choice for macOS display backends.

## Rationale
- avoid Cocoa black-screen issues on some macOS QEMU builds by preferring SDL.

## Tests
- not run (per request).
