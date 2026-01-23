# Phase 103 Summary: Allow Small Block Devices to Format

**Date**: 2026-01-22

## Overview
This phase fixes bare-metal filesystem initialization failing on small RAM disks. The storage formatter previously required a minimum 32-block commit log, which exceeded the total blocks of the default 32-block RamDisk and caused `InvalidSuperblock`. As a result, the workspace booted without a filesystem and the editor reported “Filesystem unavailable.”

## Fix
**File**: `services_storage/src/block_storage.rs`

- Reduced the minimum commit-log size from 32 blocks to 1 block in `BlockStorage::format`.
- This allows very small block devices (like the 32-block RamDisk used in bare-metal) to format successfully while still reserving space for the bitmap and data regions.

## Files Modified
- `services_storage/src/block_storage.rs`
  - `BlockStorage::format` commit-log sizing logic.

## Testing
- Manual QEMU validation (expected):
  1. Boot kernel, verify serial log shows “Filesystem ready”.
  2. `open editor hi.txt` → edit → `:w` → should show `Saved to hi.txt`.

## Notes
This change is a minimal compatibility fix for constrained environments. Larger disks still allocate commit-log space proportionally (5% of total, capped at 256 blocks).
