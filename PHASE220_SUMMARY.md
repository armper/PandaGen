# Phase 220: Pointer Event Types

## Summary

This phase implements `GFX-021` from the graphics roadmap.

`input_types` gains a pointer event model alongside the keyboard one:

- `InputEvent::Pointer(PointerEvent)` with `pointer()`, `is_pointer()`, and `as_pointer()` accessors.
- `PointerEvent { kind, position, buttons, modifiers }`. Every event carries the absolute `PointerPosition`, the full held `PointerButtons` set, and keyboard `Modifiers`.
- `PointerEventKind::{Move { delta }, Button { button, state }, Wheel { dx, dy }, Capture(PointerCapture)}`.
- `PointerButton::{Primary, Secondary, Middle, Extra(u8)}` and the `PointerButtons` bit set with `with`, `without`, `contains`, and `union`.
- `PointerPosition` (signed pixels) with `offset` and `clamp_to`; `PointerDelta` for relative motion.
- `PointerCapture::{Gained, Lost}` for explicit capture transitions.
- Constructors `moved`, `button_pressed`, `button_released`, `wheel`, `capture`, plus `is_press`, `is_release`, `button`, `is_move` helpers.

`pandagend` host-control input, the one consumer that destructured `InputEvent` irrefutably, now ignores pointer events explicitly.

## Rationale

Pointer input in PandaGen is typed and self-describing rather than a stream of deltas. Carrying position and the held-button set on every event means a component that gains focus mid-drag, or a remote UI replaying a session, never has to reconstruct state from events it did not see. Capture is a first-class transition rather than an implied side effect of pressing a button, matching the roadmap's requirement for explicit focus and capture semantics (`GFX-024`).

Positions are signed so relative motion and capture-while-outside-the-surface use the same type without wrapping; `clamp_to` is where a desktop policy decides to confine the pointer.

## Tests

Added in `input_types`:

- button bit set operations including extra buttons
- constructor button-state tracking, movement with modifiers, wheel, capture
- position offset saturation and clamping
- `InputEvent` accessor behaviour across variants
- serialization round trip for every pointer kind and the unchanged `Key` wire shape
- `Display` for buttons

Validated with:

- `cargo test -p input_types`
- `cargo build -p kernel_bootstrap --profile kernel --target x86_64-unknown-none -Zbuild-std=core,alloc`
- `cargo test --all`
