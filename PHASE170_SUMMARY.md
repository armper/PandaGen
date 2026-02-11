# Phase 170: Real Delivery in Input HAL Bridge Default Poll Path

## Summary

Implemented real message delivery semantics in `services_input_hal_bridge` default polling flow.

Previously, `InputHalBridge::poll()` translated hardware input but used a placeholder delivery function that only incremented a counter. This phase replaces that behavior with actual input-service validation and kernel delivery.

## Rationale

The bridge’s default path should be production-safe and behaviorally correct. Placeholder delivery made the API misleading: callers could see `EventDelivered` without any IPC message sent.

This phase ensures that the default path:

- validates subscription state through `InputService`
- constructs a typed input envelope with source attribution
- sends via kernel API to exercise real budget/policy enforcement points
- surfaces budget/policy/channel failures as bridge errors

## Change Set

- `services_input_hal_bridge/src/lib.rs`
  - Changed `poll()` signature to:
    - `poll(&InputService, &mut impl KernelApiV0)`
  - Replaced placeholder `deliver_event()` flow with real delivery:
    - `InputService::deliver_event(...)`
    - `build_input_event_envelope(...)`
    - `kernel.send(...)`
  - Added kernel error mapping for default path:
    - `ResourceBudgetExceeded/ResourceBudgetExhausted` -> `BridgeError::BudgetExhausted`
    - `InsufficientAuthority` -> `BridgeError::PolicyDenied`
    - channel/send/receive failures -> `BridgeError::ChannelError`
  - Updated docs and comments to reflect new default behavior.
  - Updated unit tests for new `poll()` API.
  - Added tests for:
    - inactive subscription => `NoEvent`
    - kernel channel delivery failure => `BridgeError::ChannelError`

- `pandagend/src/runtime.rs`
  - Updated HAL runtime path to call the new default `bridge.poll(...)`.
  - Removed now-unneeded `KernelInputSink` usage in runtime.
  - Simplified HAL context accordingly.

- `TODO_HIGH_VALUE_RANKING.md`
  - Marked item #3 complete.

## Validation

Executed:

- `cargo check -p services_input_hal_bridge --lib` ✅

Attempted:

- `cargo test -p services_input_hal_bridge`

Blocked by pre-existing unrelated `sim_kernel` compile failures:

- `sim_kernel/src/lib.rs`: missing field `syscall_gate` in `SimulatedKernel` initializer
- `sim_kernel/src/scheduler.rs`: borrow checker error (`E0502`)
