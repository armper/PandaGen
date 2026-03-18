# Phase 201: Graphics Roadmap Document

**Completion Date**: 2026-03-17

## Overview

Phase 201 adds a dedicated roadmap document for the path from PandaGen's current framebuffer and desktop-surface foundation to a real graphical desktop.

The roadmap is explicitly written for PandaGen's clean-slate, next-generation OS direction. It avoids legacy compatibility goals and instead defines the work in terms of native surface composition, typed scene content, explicit input routing, deterministic rendering, and later optional GPU acceleration.

## What Changed

- Added `docs/path_to_graphics.md`

The new document includes:

- current graphics-related foundation already present in the repo
- target architecture for native graphical desktop composition
- a story list grouped by epics
- milestone checkpoints from first visible desktop to native graphical apps
- a recommended implementation order
- the immediate next story to execute

## Rationale

The repository had the right low-level ingredients and a recent desktop compositor foundation in `services_gui_host`, but it did not have a single roadmap that connected those pieces into a coherent graphics plan.

This phase creates that plan so future graphics work can be executed as a sequence of explicit stories rather than scattered ad hoc tasks.

## Tests

No code-path behavior changed in this phase.

No tests were run.

## Files Changed

- `docs/path_to_graphics.md`
- `PHASE201_SUMMARY.md`
