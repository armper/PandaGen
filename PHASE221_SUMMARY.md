# Phase 221: Pointer HAL, PS/2 Mouse Driver, And Pointer Bridge

## Summary

This phase delivers the host-testable half of `GFX-022` (pointer device bridge). The kernel IRQ wiring follows in Phase 222.

**`hal`**
- `pointer::HalPointerPacket` (relative dx/dy counts, wheel notches, button bits) and the `PointerDevice` trait.
- `pointer_translation::PointerTranslator`: owns absolute position confined to a surface and the held-button set, and expands one packet into at most one move, one event per changed button, and one wheel event, in that order. Allocation-free via a fixed-capacity `PointerEventBatch`. Device Y-up is flipped to desktop Y-down; clamped motion reports the delta actually applied and emits nothing when pinned in a corner.

**`hal_x86_64::mouse`**
- `Ps2MousePacketParser`: 3-byte standard and 4-byte IntelliMouse packets, sign extension, overflow saturation, and resync on a byte without the sync bit.
- `Ps2MouseInit::initialize`: enables the aux port, sets IRQ 12 and the aux clock in the controller command byte, sets defaults, runs the IntelliMouse sample-rate probe to detect a wheel, and enables reporting. Every handshake is bounded and every non-ACK is a typed error.
- `X86Ps2Mouse`: polling `PointerDevice` that only consumes bytes flagged as auxiliary data, leaving keyboard bytes in place.

**`services_input_hal_bridge::PointerHalBridge`**: mirrors `InputHalBridge`. Owns a `PointerDevice` and a `PointerTranslator`, stamps keyboard modifiers onto pointer events, and delivers each expanded event through the existing input subscription, either via the kernel API or an arbitrary sink.

## Rationale

The keyboard path already separates hardware (HAL device), translation (scancode to typed key), and delivery (bridge through a capability). The pointer path follows the same shape so the two stay comparable and both remain testable without hardware. Position and button state live in the translator, not in consumers, matching the `PointerEvent` contract from Phase 220 where every event is self-describing.

The PS/2 bring-up sequence is scripted against `FakePortIo` byte for byte, so a regression in controller handshakes shows up in `cargo test` rather than as a silent dead mouse in QEMU.

## Tests

- `hal`: packet helpers, translator centring/clamping, button edges with held sets, event ordering, scale/resize/reset/warp, extra buttons.
- `hal_x86_64`: parser signs, overflow, resync and reset, IntelliMouse wheel and extra buttons, polling device ignores keyboard bytes, init negotiates IRQ 12 and wheel with the exact write sequence, plain mouse without wheel, NACK and stuck-controller failures.
- `services_input_hal_bridge`: packet expands to typed events with modifiers, revoked subscription delivers nothing, kernel channel delivery, resize keeps the pointer inside.

Validated with `cargo test -p hal -p hal_x86_64 -p services_input_hal_bridge`.
