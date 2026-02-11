# Phase 169: Host Runtime HAL Input Pumping

## Summary

Implemented real HAL-mode input pumping in `pandagend` runtime by integrating the host event loop with `services_input_hal_bridge` and `services_input`.

This removes the HAL input stub and makes `HostMode::Hal` route keyboard events through the same typed input path used by the system.

## Rationale

`HostMode::Hal` previously returned `Ok(())` without polling any keyboard source or delivering events, making HAL mode effectively non-functional.

The new implementation:

- Polls a HAL keyboard source via `InputHalBridge::poll_with_sink`
- Delivers input events through a kernel-backed sink
- Receives typed input envelopes from the subscribed channel
- Deserializes `InputEvent` and routes it into workspace input handling
- Preserves the host-control hotkey (`Ctrl+Space`) behavior in HAL mode

## Change Set

- `pandagend/src/runtime.rs`
  - Added `HostRuntimeError::HalInputError` (feature-gated).
  - Added feature-gated HAL runtime context (`HalInputContext`) containing:
    - bridge
    - input service
    - subscribed channel
    - source task id
    - shared keyboard event queue
  - Added `SharedQueueKeyboard` (`KeyboardDevice`) to provide deterministic HAL events for host runtime integration/tests.
  - Replaced `pump_hal_input()` stub with real bridge+sink+receive+route flow.
  - Added `inject_hal_event()` helper for deterministic HAL-mode tests.
  - Added HAL-mode tests:
    - `test_hal_mode_ctrl_space_toggles_host_control`
    - `test_hal_mode_routes_input_to_workspace`

- `pandagend/src/main.rs`
  - Updated help text to remove "hal mode is not yet functional" note.

- `TODO_HIGH_VALUE_RANKING.md`
  - Marked item #2 complete.

## Tests

Attempted:

- `cargo test -p pandagend`
- `cargo test -p pandagend --features hal_mode`

Result:

- Both blocked by pre-existing `sim_kernel` compilation issues unrelated to this phase:
  - missing field `syscall_gate` in `SimulatedKernel` initializer
  - borrow checker error in `sim_kernel/src/scheduler.rs`

Directly validated in this phase:

- Rustfmt run on updated files.
- New HAL integration tests added in `pandagend/src/runtime.rs` under `#[cfg(feature = "hal_mode")]`.
