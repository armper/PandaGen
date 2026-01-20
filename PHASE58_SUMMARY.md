# Phase 58: Timer-Driven Sleep and Blocking (Kill Busy-Wait)

**Completion Date**: 2026-01-20

## Overview

This phase replaces busy-wait loops with real blocking semantics in the scheduler. Tasks now enter a `Blocked { wake_tick }` state when sleeping, and the scheduler automatically wakes them when the timer advances past their wake time. This proves idle CPU behavior and deterministic wakeups, establishing the foundation for proper multi-tasking sleep.

## What Was Built

### 1. Enhanced TaskState with Wake Tick

**Modified `TaskState` Enum**:
```rust
pub enum TaskState {
    Runnable,
    Blocked { wake_tick: u64 },  // NEW: tracks when to wake
    Exited,
    Cancelled,
}
```

Previously, `Blocked` was a unit variant with no associated data. Now it carries a `wake_tick` field that specifies the exact tick count when the task should be woken up.

### 2. Scheduler Block with Wake Time

**Updated `block_task` Method**:
```rust
pub fn block_task(&mut self, task_id: TaskId, wake_tick: u64) {
    if let Some(task_info) = self.tasks.get_mut(&task_id) {
        task_info.state = TaskState::Blocked { wake_tick };
        task_info.ticks_in_quantum = 0;
    }
    self.run_queue.remove(task_id);
    if self.current_task == Some(task_id) {
        self.current_task = None;
    }
}
```

Tasks are removed from the run queue and marked with their wake time. The scheduler will no longer consider them for scheduling until they're unblocked.

### 3. Automatic Wake on Tick Advance

**New `wake_ready_tasks` Method**:
```rust
pub fn wake_ready_tasks(&mut self) {
    let current_ticks = self.current_ticks;
    let tasks_to_wake: Vec<TaskId> = self.tasks
        .iter()
        .filter_map(|(task_id, info)| {
            if let TaskState::Blocked { wake_tick } = info.state {
                if current_ticks >= wake_tick {
                    return Some(*task_id);
                }
            }
            None
        })
        .collect();

    for task_id in tasks_to_wake {
        self.unblock_task(task_id);
    }
}
```

Called from `on_tick_advanced()`, this method scans all blocked tasks and wakes those whose wake_tick has been reached. This is deterministic and efficient—tasks wake at the exact tick specified.

### 4. SimulatedKernel Sleep Integration

**Updated `sleep` Implementation**:
```rust
fn sleep(&mut self, duration: Duration) -> Result<(), KernelError> {
    let ticks_to_sleep = duration.as_nanos() / self.nanos_per_tick;
    let wake_tick = self.timer.poll_ticks() + ticks_to_sleep;

    // Block current task if one is running
    if let Some(task_id) = self.scheduler.current_task() {
        self.scheduler.block_task(task_id, wake_tick);
    }

    // Still advance time for simulation
    self.advance_time(duration);
    Ok(())
}
```

When a task calls `sleep()`, the kernel:
1. Calculates the wake tick based on the requested duration
2. Blocks the task in the scheduler with that wake tick
3. Advances simulation time (because this is a simulated kernel)

In a real kernel, step 3 would not happen—time advances naturally. But for simulation testing, we need to advance time explicitly.

## What's Proven

✅ **Tasks block properly**: `block_task()` removes tasks from run queue and sets wake_tick  
✅ **Tasks wake automatically**: `on_tick_advanced()` wakes tasks when current_tick >= wake_tick  
✅ **Multiple tasks wake correctly**: Tasks with different wake times wake in correct order  
✅ **Deterministic wakeup**: Same tick inputs produce same wakeup behavior  
✅ **No busy-wait in scheduler**: Blocked tasks are not considered for scheduling  
✅ **Idle CPU behavior**: Scheduler can detect when no tasks are runnable (all blocked)

## Tests

### New Tests Added

1. **`test_blocked_task_automatic_wakeup`**:
   - Blocks a task at tick 0 until tick 50
   - Advances to tick 30 → task still blocked
   - Advances to tick 50 → task wakes automatically
   - Verifies state transitions: Runnable → Blocked → Runnable

2. **`test_multiple_blocked_tasks_wake_at_different_times`**:
   - Three tasks blocked at ticks 10, 20, 30
   - Advances time incrementally
   - Verifies each task wakes at correct time
   - Verifies runnable_count increases as tasks wake

### Updated Tests

1. **`test_block_unblock`**:
   - Updated to pass wake_tick parameter to `block_task()`
   - Updated assertions to match `Blocked { wake_tick }` pattern

### All Tests Passing

```
test result: ok. 120 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

- All scheduler tests pass
- All sim_kernel integration tests pass
- No performance regressions

## Code Changes

### Modified Files

1. **sim_kernel/src/scheduler.rs** (~50 lines changed):
   - Changed `TaskState::Blocked` to `Blocked { wake_tick: u64 }`
   - Updated `block_task()` to accept wake_tick parameter
   - Updated `unblock_task()` to use pattern matching for Blocked state
   - Added `wake_ready_tasks()` method to scan and wake tasks
   - Updated `on_tick_advanced()` to call `wake_ready_tasks()`
   - Added two new tests for automatic wakeup behavior
   - Updated existing `test_block_unblock()` to use new wake_tick parameter

2. **sim_kernel/src/lib.rs** (~10 lines changed):
   - Updated `sleep()` to calculate wake_tick and block current task
   - Added scheduler blocking logic to sleep implementation

3. **kernel_bootstrap/src/main.rs** (~5 lines changed):
   - Updated comment on `sys_sleep()` to reference Phase 58
   - Noted that busy-wait is acceptable for single-task bare-metal
   - Documented future multi-tasking integration path

4. **PHASE58_SUMMARY.md** (NEW):
   - This file

## Architecture Notes

### Why wake_tick Instead of duration?

Storing `wake_tick` (absolute time) instead of `duration` (relative time) avoids race conditions:
- **With duration**: Task blocks at tick 10 for 5 ticks. If scheduler pauses for 3 ticks before checking, we'd need to subtract elapsed time.
- **With wake_tick**: Task blocks at tick 10 until tick 15. At tick 13, 14, 15, or later, the condition `current_tick >= 15` is always correct.

Absolute time is easier to reason about and less error-prone.

### Why Scan All Tasks on Each Tick?

In a production scheduler, you'd use a priority queue or timer wheel to wake tasks efficiently. But:
1. **Simplicity**: Linear scan is ~10 lines of code, easy to verify
2. **Testability**: Behavior is obvious and deterministic
3. **Performance**: In simulation with <100 tasks, linear scan is fast enough
4. **Correctness**: No complex data structure bugs

For Phase 58, correctness > performance. Future phases can optimize if needed.

### Why Still advance_time() in SimulatedKernel?

SimulatedKernel is a *simulated* kernel—it runs in-process for testing. Unlike a real kernel where time advances naturally via hardware timer, we must advance time explicitly:
- **Real kernel**: Task sleeps, CPU halts, timer IRQ fires, task wakes
- **Simulated kernel**: Task sleeps, we call `advance_time()`, task wakes

This allows deterministic testing: "advance 100ms, verify task woke up."

### Bare-Metal Busy-Wait Acceptable?

Yes, for now. kernel_bootstrap is single-task (no scheduler yet). The busy-wait with `idle_pause()` (CPU pause instruction) is fine:
- **Phase 58**: Proves scheduler blocking semantics in simulation
- **Future phase**: When bare-metal gets multi-tasking, it will use the same scheduler

The comment documents this explicitly for future implementers.

## What's Still Minimal

- **No priority queue**: Tasks are woken via linear scan
- **No early wakeup**: Tasks wake exactly at wake_tick, no spurious wakeups
- **No sleep cancellation**: Can't unblock a task early (e.g., for signal/interrupt)
- **No fractional ticks**: Wake time is rounded to nearest tick
- **No timeout on block**: Tasks block forever if wake_tick is never reached
- **Bare-metal still busy-waits**: kernel_bootstrap not integrated yet

## Next Steps (Future Phases)

### Phase 59+: Interrupt-Driven Wakeup
- Bare-metal timer IRQ calls `scheduler.wake_ready_tasks()`
- Tasks wake from real hardware timer, not simulation

### Phase 60+: I/O Blocking
- Tasks block on `recv()` with optional timeout
- Wake on message arrival OR wake_tick, whichever comes first
- Requires combining I/O events with timer events

### Phase 61+: Signal/Interrupt Handling
- Tasks can be unblocked early by signals
- Requires `unblock_task()` to return wake reason (timeout vs signal)

### Phase 62+: Priority Queue for Timers
- Replace linear scan with heap or timer wheel
- Improves performance for systems with many sleeping tasks

## Conclusion

Phase 58 successfully replaces busy-wait loops with real blocking semantics. Tasks now enter a `Blocked { wake_tick }` state, the scheduler skips them during scheduling, and they wake automatically when the timer advances. This proves:
- **Idle CPU behavior**: Scheduler knows when no work is available
- **Deterministic wakeup**: Tasks wake at exact tick specified
- **No busy-wait**: Blocked tasks don't consume CPU time

The implementation is minimal (~50 lines), testable (2 new tests, all pass), and deterministic (same inputs → same outputs). All tests pass, clippy is clean, and the design is ready for multi-tasking integration in both simulation and bare-metal.

**Blocking works.** ⏰
