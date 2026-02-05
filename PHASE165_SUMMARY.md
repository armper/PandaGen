# Phase 165 Summary

## Overview
- added macOS HVF default CPU selection to suppress SVM warnings without user flags.
- documented the new default `-cpu host` behavior and override environment variable.

## Rationale
- keep macOS QEMU runs clean and hands-free while retaining a safe override path.

## Tests
- not run (per request).
