# Path To Graphics

## Goal

Build a real graphical PandaGen desktop that is native to the system's clean-slate design:

- composited surfaces, not terminal transcripts
- explicit scene and input contracts, not legacy window-system compatibility
- capability-routed input and display authority
- deterministic logic that is still testable under `cargo test`
- a software-rendered path first, with room for later GPU acceleration

## Non-Goals

This roadmap does not target:

- X11 compatibility
- Wayland compatibility
- POSIX GUI assumptions
- byte-stream terminal rendering as the primary desktop model
- mouse-first desktop behavior with hidden focus rules

## Current Foundation

The repo already has useful pieces:

- framebuffer hardware access in `hal` and `kernel_bootstrap`
- mature text and framebuffer console paths
- workspace split/tab layout metadata and composed multi-tile snapshots
- a clean-slate desktop compositor primitive in `services_gui_host`
- typed `ViewFrame` publishing across services

What is still missing is the path from those primitives to an actual on-screen graphical desktop.

## Target Architecture

The intended stack should look like this:

1. Services publish typed views or scene content.
2. `services_workspace_manager` decides layout, focus, and surface ownership.
3. `services_gui_host` composes windows and overlays into a desktop surface.
4. A renderer converts that desktop surface or scene graph into pixels.
5. `kernel_bootstrap` presents those pixels through the framebuffer.
6. Input services route keyboard and pointer events back through explicit focus and hit-test rules.

The kernel should provide mechanism:

- framebuffer access
- timers
- interrupts
- input device events
- optional GPU/device primitives later

User-space services should provide policy:

- window chrome
- layout rules
- focus policy
- interaction behavior
- visual language

## Stories

### Epic 1: Desktop Surface Contract

- [ ] `GFX-001` Define the canonical desktop output contract for PandaGen: surface dimensions, layers, damage regions, frame revision, and presentation timing.
- [ ] `GFX-002` Decide whether the long-term contract is a retained scene graph, immediate render commands, or a hybrid surface-plus-scene model.
- [ ] `GFX-003` Extend `view_types` with first-class graphical content types instead of only text/status/panel payloads.
- [ ] `GFX-004` Add serialization tests for graphical view payloads so remote UI and replay stay deterministic.
- [ ] `GFX-005` Define a small color, brush, border, and text-style model that does not depend on ANSI or terminal semantics.

### Epic 2: Workspace To Window Mapping

- [x] `GFX-006` Add an adapter that turns `WorkspaceRenderSnapshot` tile/layout state into `DesktopWindow` values for `services_gui_host`.
- [x] `GFX-007` Define window roles such as main, status, overlay, palette, notification, and modal.
- [x] `GFX-008` Render split/tab state as actual desktop chrome instead of text-only composed summaries.
- [x] `GFX-009` Add explicit z-layer policy for workspace windows, overlays, notifications, and system surfaces.
- [ ] `GFX-010` Add deterministic tests for tile-to-window mapping, focus visuals, and overlay ordering.

### Epic 3: Software Rasterizer

- [ ] `GFX-011` Add a pure Rust software rasterizer crate that can paint rectangles, borders, glyphs, and solid fills into an RGBA buffer.
- [ ] `GFX-012` Define a render target abstraction that can target both off-screen test buffers and the real framebuffer backend.
- [ ] `GFX-013` Add font raster or bitmap atlas support for readable desktop text without relying on console glyph code.
- [ ] `GFX-014` Add clipping, scissoring, and damage-aware redraw support.
- [ ] `GFX-015` Add golden surface tests for raster output so pixel logic can be validated without booting QEMU.

### Epic 4: Framebuffer Presentation

- [ ] `GFX-016` Add a framebuffer presentation path that accepts a desktop pixel buffer from the GUI host path.
- [ ] `GFX-017` Route the current framebuffer backbuffer/blit machinery through that new presentation path.
- [ ] `GFX-018` Add a stable frame pacing and present policy so redraws are explicit and bounded.
- [ ] `GFX-019` Add damage-region presentation so unchanged areas do not force full-screen redraw.
- [ ] `GFX-020` Add boot/runtime switching between text console mode and graphics desktop mode.

### Epic 5: Pointer And Graphical Input Model

- [ ] `GFX-021` Add pointer event types to `input_types` for move, button, wheel, and capture transitions.
- [ ] `GFX-022` Add a pointer device bridge comparable to the keyboard HAL bridge.
- [ ] `GFX-023` Define hit testing against desktop surfaces and windows.
- [ ] `GFX-024` Define explicit pointer focus, keyboard focus, and capture semantics for PandaGen.
- [ ] `GFX-025` Add cursor composition as a first-class surface, not a side effect of text rendering.

### Epic 6: Basic Scene Primitives

- [ ] `GFX-026` Add primitive draw ops for fills, lines, borders, rounded rectangles, and text.
- [ ] `GFX-027` Add image or icon surface support for future app and shell visuals.
- [ ] `GFX-028` Add scrollable regions and clipping containers as scene primitives.
- [ ] `GFX-029` Add a simple layout vocabulary for stack, split, overlay, and anchored elements.
- [ ] `GFX-030` Add animation timing hooks for transitions without requiring a game-engine model.

### Epic 7: Desktop Shell

- [ ] `GFX-031` Build a minimal graphical shell surface: background, launcher area, status area, notifications, and workspace area.
- [ ] `GFX-032` Replace text-only window chrome with a graphical title bar, focus ring, and tab strip design.
- [ ] `GFX-033` Add a graphical command palette overlay backed by the existing command surface data.
- [ ] `GFX-034` Add a system notification layer that can render toasts and persistent status cards.
- [ ] `GFX-035` Add shell-level theme tokens so the desktop has a coherent next-gen visual language.

### Epic 8: Graphical App Surfaces

- [ ] `GFX-036` Add a graphical renderer for the editor using the same document state and cursor model it already has.
- [ ] `GFX-037` Add a graphical renderer for the file picker with selection, directory breadcrumbs, and status strip.
- [ ] `GFX-038` Add a graphical renderer for the CLI component or decide to keep CLI text-native inside a graphical host surface.
- [ ] `GFX-039` Add a graphical runtime for pipeline execution and status reporting.
- [ ] `GFX-040` Add a custom-component graphical host contract so future apps are not locked to text frames.

### Epic 9: Remote And Replayability

- [ ] `GFX-041` Extend `services_remote_ui_host` to ship graphical desktop snapshots or scene payloads.
- [ ] `GFX-042` Define a compact transport format for graphical updates and damage regions.
- [ ] `GFX-043` Preserve deterministic replay for graphical sessions the same way text snapshots are replayable today.
- [ ] `GFX-044` Add round-trip tests for graphical snapshot serialization and remote transport.

### Epic 10: Performance And Correctness

- [ ] `GFX-045` Add compositor benchmarks for many windows, many tiles, and frequent cursor or overlay updates.
- [ ] `GFX-046` Add property tests for clipping, z-order, and damage accumulation invariants.
- [ ] `GFX-047` Add memory budgeting for large surfaces, atlases, and off-screen buffers.
- [ ] `GFX-048` Add failure behavior for low-memory or missing-framebuffer cases so the system degrades cleanly.
- [ ] `GFX-049` Add observability hooks for present latency, redraw counts, and dropped frames.

### Epic 11: Hardware Acceleration Later

- [ ] `GFX-050` Define a renderer backend trait so software and GPU renderers can share the same desktop contract.
- [ ] `GFX-051` Add a GPU HAL exploration phase only after the software renderer path is stable and test-covered.
- [ ] `GFX-052` Define which parts of composition remain CPU-side and which parts may move to GPU execution.

## Milestones

### Milestone A: Graphical Desktop Visible In QEMU

Definition:

- desktop surface is composed from workspace windows
- rasterizer paints into a pixel buffer
- framebuffer presents the desktop
- keyboard still works for focus and command flow

Required stories:

- `GFX-001` to `GFX-020`

### Milestone B: Interactive Graphical Shell

Definition:

- command palette, notifications, and shell chrome are graphical
- pointer can move, focus, and click
- split/tab state is visible as windows instead of text summaries

Required stories:

- `GFX-021` to `GFX-035`

### Milestone C: Native Graphical Apps

Definition:

- editor and file picker render as real graphical app surfaces
- remote UI and snapshot replay still work
- performance is observable and bounded

Required stories:

- `GFX-036` to `GFX-049`

### Milestone D: Optional Accelerated Renderer

Definition:

- software path remains authoritative and testable
- GPU backend is an optimization layer, not a semantic dependency

Required stories:

- `GFX-050` to `GFX-052`

## Recommended Order

If we want the shortest path to a visible graphical desktop, the implementation order should be:

1. `GFX-006` to `GFX-020`
2. `GFX-021` to `GFX-025`
3. `GFX-031` to `GFX-039`
4. `GFX-041` to `GFX-049`
5. `GFX-050` to `GFX-052`

The reason is simple: PandaGen already has enough layout and framebuffer foundation to get a software-rendered desktop visible before it has a complete app scene toolkit.

## Immediate Next Story

The best next implementation step is:

- `GFX-010` Add deterministic tests for tile-to-window mapping, focus visuals, and overlay ordering.

`GFX-009` is now in place, which means layering is no longer an accident of raw `z_index` values. The next step is to widen deterministic coverage around focus chrome, layer interactions, and tile mapping invariants so later graphics work can build on a stable compositor contract.
