# Phase 225: Pointer Focus, Keyboard Focus, And Capture Semantics

## Summary

This phase implements `GFX-024` from the graphics roadmap.

**`services_gui_host::input_routing::DesktopInputRouter`** turns a `PointerEvent` plus the current window list into explicit `Delivery` values:

- `Pointer { target, event, hit }` to a window, or `Desktop { event }` over bare desktop.
- `Enter`/`Leave` when the hovered window changes.
- `Capture { target, transition }` when a press starts implicit capture and when the last button is released, the capturing window vanishes, or the device layer reports capture lost.
- `FocusChanged { previous, current }` when a press lands on a focusable window or a focused window disappears.

Rules, all covered by tests: hover never focuses; a press on a focusable role moves keyboard focus and captures; `Status` and `Notification` roles are not focusable but still capture their own clicks; motion and additional buttons during capture go to the capturer regardless of position, with hover frozen; capture ends only when every button is up, after which hover catches up to the window under the pointer; wheel goes to the capturer or the hovered window without touching focus; negative coordinates hit nothing. `apply_focus` marks the focus owner on a window list; `DesktopWindowRole::label` names roles for logs.

**Kernel integration**: `DesktopViewIds` gives the main, status, and palette windows stable ids across frames, since focus and capture are tracked by id. The loop owns a router, routes each pointer event against the windows the user currently sees, applies focus before rendering, and records hovered role, focused role, and capture state for the `pointer` command's new `Routing:` line.

## Rationale

The roadmap asks for input semantics that are "explicit, not hidden." Every ownership change here is a delivery a consumer can observe, not a side effect it must infer. Implicit capture on press is the one conventional behaviour kept, because without it a drag that crosses a window edge silently changes recipients. Making status and notification surfaces non-focusable prevents a click on chrome from stealing the keyboard from the editor.

Routing is done in the GUI host, not in the kernel, because it depends on z-order and window geometry the compositor owns; the kernel only feeds events and reads back state.

## Verification

- `cargo test -p services_gui_host` (36 tests, 7 new in `input_routing`), `cargo test -p kernel_bootstrap` (82 lib + 76 bin).
- Scripted QEMU session (`cargo xtask qemu-script`) in graphics mode:

```
Routing: over=main   focus=none capture=0   (hover after moving up-left)
Routing: over=main   focus=main capture=1   (left button held, dragged off the bottom edge)
Routing: over=status focus=main capture=0   (released, moved into the status bar)
Routing: over=status focus=main capture=0   (clicked the status bar: focus unchanged)
```
