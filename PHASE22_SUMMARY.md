# Phase 22: HAL Timer + Deterministic Timekeeping

**Completion Date**: 2026-01-19

## Overview

Phase 22 introduces hardware-backed time measurement to PandaGen OS while preserving determinism for testing. This provides a foundation for time-based resource accounting, delays, and retries without introducing wall-clock dependencies or blocking operations.

## What Was Added

### 1. HAL Timer Interface (`hal` crate)

**New trait: `TimerDevice`**
- `fn poll_ticks(&mut self) -> u64` - Non-blocking tick counter
- Monotonic: ticks never go backwards
- Cumulative: returns total ticks since boot
- Frequency-agnostic at this layer

**Key principles:**
- Time is a service, not a global variable
- No blocking, no sleeping, no waiting
- Suitable for mocking and testing

### 2. Simulated Timer (`sim_kernel`)

**Implementation: `SimTimerDevice`**
- Deterministic tick progression
- Controllable via `advance_ticks(n)`
- Used by default in tests and simulation mode
- Integrated into `SimulatedKernel`:
  - Configurable tick resolution (default: 1 microsecond per tick)
  - `current_time` derived from timer ticks
  - Direct timer access for test manipulation

**Benefits:**
- Tests are fully deterministic
- No race conditions or timing-dependent flakes
- Trivial to test time-based logic

### 3. x86_64 Hardware Timer (`hal_x86_64`)

**Implementations:**
- `FakeTimerDevice` - Scripted timer for testing without hardware
- `PitTimer<P: PortIo>` - 8254 Programmable Interval Timer
  - ~1 kHz tick rate (~1ms resolution)
  - Non-blocking polling interface
  - Monotonic cumulative tracking
  - Minimal unsafe code (isolated to port I/O)

**Design notes:**
- Generic over `PortIo` trait for testability
- Can use `RealPortIo` for hardware or `FakePortIo` for tests
- PIT chosen for simplicity and universal availability
- APIC timer could be added in the future if needed

### 4. Kernel Integration

**Changes to `SimulatedKernel`:**
- Added `timer: SimTimerDevice` field
- Added `nanos_per_tick: u64` for configurable resolution
- `advance_time(duration)` now:
  1. Converts duration to ticks
  2. Advances timer
  3. Syncs `current_time` from timer
  4. Processes delayed messages

**New public methods:**
- `with_tick_resolution(Duration)` - Create kernel with custom tick granularity
- `timer()` / `timer_mut()` - Access timer for test manipulation
- `sync_time_from_timer()` - Update current_time from timer ticks

### 5. CPU Budget Integration

**Timer ticks ↔ CPU accounting:**
- `try_consume_cpu_ticks(exec_id, amount)` can now use timer-derived values
- Tests demonstrate advancing timer and consuming corresponding CPU ticks
- Deterministic: same tick sequence = same budget behavior

**Example workflow:**
```rust
// Advance timer by 100 ticks
kernel.timer_mut().advance_ticks(100);
kernel.sync_time_from_timer();

// Consume CPU budget based on time elapsed
kernel.try_consume_cpu_ticks(exec_id, 100)?;
```

### 6. Tests Added

**Unit tests (19 total):**
- `hal::timer` - 2 tests (timer trait behavior)
- `sim_kernel::timer` - 12 tests (deterministic timer)
- `hal_x86_64::timer` - 7 tests (fake and PIT timer)

**Integration tests:**
- `test_timer_integration_with_cpu_budget` - Timer + budget accounting
- `test_timer_deterministic_behavior` - Identical sequences = identical results
- `test_timer_monotonic_with_advance_time` - Monotonicity verification

**Compatibility tests:**
- All existing sim_kernel tests pass (68 total)
- Fault injection tests pass (9 total)
- Full suite: 100% passing

## What Was NOT Added

**Intentionally excluded (per requirements):**
- ❌ Preemptive scheduling
- ❌ Wall-clock APIs (UTC, timezones, dates)
- ❌ Blocking or sleeping inside HAL
- ❌ Async runtimes
- ❌ Global time access
- ❌ Automatic timer polling in event loops (deferred to future phases)

## Design Decisions

### 1. Why Ticks, Not Nanoseconds at HAL Layer?

The `TimerDevice` trait returns `u64` ticks, not nanoseconds, because:
- Tick frequency varies by hardware (PIT ~1kHz, TSC GHz, APIC MHz)
- Conversion to nanoseconds is policy, not mechanism
- Kernel can define its own time abstraction (`Instant`, `Duration`)
- Simpler trait = easier to implement

### 2. Why SimKernel Owns the Timer?

- Determinism: kernel controls when time advances
- Testing: explicit `advance_ticks()` prevents race conditions
- Abstraction: hardware details hidden from services
- No globals: time flows through kernel API only

### 3. Why PIT Instead of APIC/HPET/TSC?

- **PIT (chosen)**: Legacy, universally available, simple, well-documented
- **APIC**: More complex, requires APIC setup, not always present
- **HPET**: Modern but not universally available, complex
- **TSC**: High-res but non-monotonic without OS support

PIT is the right choice for Phase 22. Future phases can add alternatives.

### 4. Why Non-Blocking Only?

Blocking inside HAL would:
- Break determinism (sim kernel can't block)
- Introduce hidden control flow
- Violate separation of concerns
- Make testing impossible

Polling is explicit, testable, and predictable.

## Integration Points

### For Services

Services don't access timers directly. They use kernel APIs:
- `kernel.now()` - Get current `Instant`
- `kernel.sleep(duration)` - Advance simulated time (non-blocking in sim)

### For Tests

Tests can manipulate time directly:
```rust
let mut kernel = SimulatedKernel::new();
kernel.timer_mut().advance_ticks(1000);
kernel.sync_time_from_timer();
```

### For Host Runtime (Future)

In a real host runtime event loop:
```rust
let mut timer = PitTimer::new(RealPortIo::new());
loop {
    let ticks = timer.poll_ticks();
    // Update kernel internal time
    // Process events
}
```

## Compatibility

**Backward compatible:**
- All existing tests pass
- No breaking API changes
- Fault injection still works
- Resource accounting unaffected

**Forward compatible:**
- Timer trait can support other hardware (APIC, HPET, TSC)
- Tick resolution is configurable
- Budget accounting can use timer ticks directly

## Documentation Updates

- Updated `hal/src/timer.rs` - Comprehensive trait documentation
- Updated `sim_kernel/src/timer.rs` - Simulated timer guide
- Updated `hal_x86_64/src/timer.rs` - Hardware timer notes
- Updated `docs/architecture.md` - Time philosophy section
- Updated `docs/interfaces.md` - TimerDevice trait reference
- Created `PHASE22_SUMMARY.md` - This document

## Testing Summary

**All quality gates passed:**
- ✅ `cargo fmt --all` (clean)
- ✅ `cargo clippy --all -- -D warnings` (no warnings)
- ✅ `cargo test --all` (100% passing)

**Test coverage:**
- 19 new timer-specific tests
- 3 new integration tests
- 68 existing sim_kernel tests (all passing)
- 9 fault injection tests (all passing)
- Total: 99+ tests passing

## Lessons Learned

1. **Trait-based design pays off**: `PortIo` trait made PIT timer trivially testable
2. **Determinism is powerful**: Simulated timer eliminates entire classes of test flakes
3. **Separation of concerns works**: HAL knows hardware, kernel knows policy, tests know both
4. **Start simple**: PIT is "good enough" for Phase 22, can optimize later

## Future Work (Not in Phase 22)

- [ ] Integrate timer polling into host runtime event loop
- [ ] Add APIC timer as alternative to PIT
- [ ] Add TSC support for high-resolution timing
- [ ] Time-based preemptive scheduling (Phase N)
- [ ] Periodic timer interrupts for preemption
- [ ] Watchdog timers for fault detection

## Conclusion

Phase 22 successfully introduces hardware-backed time measurement while preserving the determinism that makes PandaGen testable. The timer abstraction is clean, the implementations are minimal, and the integration is seamless. All tests pass, and the system is ready for time-based features like delays, retries, and CPU budget enforcement.

**Status: Complete ✅**
