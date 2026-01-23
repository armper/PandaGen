# Phase 102 Summary: Recover Filesystem Before Opening Editor

**Date**: 2026-01-22

## Overview
This phase fixes a regression where `open editor <path>` could still lead to `:w` reporting "Filesystem unavailable" if a stale editor instance retained the filesystem capability. The workspace now proactively recovers the filesystem from any lingering editor before launching a new editor session.

## Root Cause
The workspace holds the filesystem capability and transfers ownership to the editor when opening a file. If an editor instance became stale (e.g., active component cleared without returning the IO adapter), the filesystem remained trapped in that instance. Subsequent `open editor` commands saw `self.filesystem` as `None`, leading to "Filesystem unavailable" on `:w`.

## Fix
**File**: `kernel_bootstrap/src/workspace.rs`

- Added a recovery step in the `open editor` command handler:
  - If `self.filesystem` is `None`, attempt to extract the filesystem from any existing editor instance.
  - This reclaims the IO adapter via `into_filesystem()` before creating a new editor.

## Files Modified
- `kernel_bootstrap/src/workspace.rs`
  - `execute_command` "open editor" handler now recovers filesystem from stale editor instances.

## Testing
- Manual QEMU flow:
  1. `open editor hi.txt`
  2. Type content, `Esc`, `:w` → should show `Saved to hi.txt`
  3. `:q`, then `open editor hi.txt` again → should still save

## Notes
This preserves the explicit capability handoff model while preventing accidental loss of the filesystem capability between editor sessions.
