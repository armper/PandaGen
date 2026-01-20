# Phase 63: Editor Wiring (Open/Save via Capabilities)

**Completion Date**: 2026-01-20

## Overview

This phase connects the editor to real storage-backed I/O with versioned saves
and optional fs_view path resolution.

## What Was Added

- `EditorIo` + `StorageEditorIo` with JournaledStorage-backed open/save.
- `Editor::open_with` + storage-backed save path in `:w` / `:wq`.
- Integration test covering open+save with real versioned storage.

## Tests

- `cargo test -p services_editor_vi`
