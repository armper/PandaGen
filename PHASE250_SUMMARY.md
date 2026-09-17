# Phase 250: Graphical View Content And The Desktop Contract

## Summary

This phase closes Epic 1 of the graphics roadmap (`GFX-001` to `GFX-005`).

- `view_types::ViewContent::Graphics { ops }` with `DrawOp::{Fill, Border, RoundedFill, RoundedBorder, Line, Text}`, `Color` (RGBA), `PixelRect` (view-space pixels), and `TextStyle` (compact, muted). Ops are serde-tagged by name; optional fields default. Round-trip tests cover every op and a frame carrying graphics.
- The compositor executes ops inside the window's content area through a `ContainerTarget` whose origin is the text origin and whose clip is the content area intersected with damage, so views draw in their own space and cannot escape their window. Text renderers and the workspace manager summarise graphics content as `[graphics: n ops]`.
- `docs/desktop_contract.md` records the canonical contract: the retained-windows-plus-immediate-content decision, and where surface dimensions, layers, damage, revision, and presentation timing live.

## Tests

- `view_types`: graphics content and frame serialisation round trip, wire tagging, defaults, line accessors.
- `services_gui_host`: ops paint at the content origin, an out-of-window op is clipped entirely, a line stops at the inner window edge, text uses the theme text colour, hit testing still reports content cells for graphics windows.
- All existing suites, including the property tests and golden fixtures, pass unchanged.

Validated with `cargo test` on `view_types`, `services_gui_host`, `text_renderer_host`, `services_workspace_manager`, `services_remote_ui_host`, `kernel_bootstrap`, and the bare-metal kernel build.
