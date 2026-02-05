# Phase 167 Summary

## Overview
- defaulted macOS QEMU display to SDL with OpenGL disabled when SDL is available.
- documented the SDL `gl=off` preference for black-screen troubleshooting.

## Rationale
- avoid macOS QEMU black screens caused by Cocoa or SDL GL issues.

## Tests
- not run (per request).
