# Phase 239: Custom-Component Graphical Host Contract

## Summary

This phase implements `GFX-040` from the graphics roadmap.

**`services_gui_host::host`**
- `HostedSurface { title, lines, caret, highlight, status }`: what a component shows for the rows it is given, with `into_window` producing the workspace window.
- `HostEvent::{Hover, Activate, Wheel, Key}` and `HostResponse::{Ignored, Redraw, Close}`.
- `trait HostedComponent { surface(rows); handle(event) }`.
- `ListComponent`: the reference implementation (hover/Up/Down/wheel move a selection, Enter or click activates, Esc or q closes, activation is handed back to the host).

**Kernel**
- `DesktopModel.hosted: Option<HostedSurface>` takes the workspace window when no editor or picker is open, including its status text and highlight.
- The workspace hosts one `ListComponent` at a time. `open about` (also the About PandaGen palette/launcher entry) opens the reference component showing system facts; keyboard bytes go to it as `HostEvent::Key`, and pointer motion, wheel, and primary presses over the workspace window become `Hover`, `Wheel`, and `Activate` generically. Activation copies the entry to the workspace output; `Close` drops the component.

## Rationale

Before this phase the workspace window hosted four apps by special-casing the desktop builder. The contract names the shape they all share (a text-cell surface plus a tiny event vocabulary) so a new app is a `HostedComponent`, not a new branch in the builder and a new arm in the routing loop. The host keeps geometry, chrome, focus, hit testing, and painting; the component never sees pixels or screen positions, which is what keeps future apps from being locked to any particular frame. Richer content can be added as surface fields without changing the event side.

## Verification

- `cargo test -p services_gui_host` (contract and `ListComponent` behaviour) and `cargo test -p kernel_bootstrap` (hosted surface takes the workspace window; an open editor still wins).
- QEMU (`cargo xtask qemu-script`): `open about` showed the component with the first entry highlighted; `j` moved the highlight; hovering with the mouse moved it to the entry under the pointer; a click logged `hosted: activated 4` and copied that line to the output; Esc logged `About closed` and returned to the workspace view.
