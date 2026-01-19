# Phase 23: Preemptive Scheduling Foundation

**Completion Date**: 2026-01-19

## Overview

Phase 23 introduces the minimal foundation for **preemptive scheduling** in PandaGen OS. The scheduler can interrupt long-running execution and switch between runnable tasks, prioritizing correctness and determinism over fairness and performance optimization.

## What Was Added

### 1. Scheduler Core (`sim_kernel/src/scheduler.rs`)

**New module providing deterministic preemptive scheduling:**

**Core Types:**
- `TaskState`: Enum representing task states (Runnable | Blocked | Exited | Cancelled)
- `SchedulerConfig`: Configuration for scheduler behavior
  - `quantum_ticks`: Time slice per task (default: 10 ticks)
  - `max_steps_per_tick`: Optional guard against infinite loops
- `Scheduler`: Main scheduler implementation with FIFO run queue
- `ScheduleEvent`: Audit log events (TaskSelected | TaskPreempted | TaskExited)
- `PreemptionReason`: Why a task was preempted (QuantumExpired | Yielded | Blocked)
- `ExitReason`: Why a task exited (Normal | ResourceExhaustion | Failed)

**Key Operations:**
```rust
// Create scheduler with config
let scheduler = Scheduler::with_config(config);

// Task lifecycle
scheduler.enqueue(task_id);           // Add task to run queue
scheduler.dequeue_next();             // Select next task
scheduler.preempt_current();          // Move current task to back of queue
scheduler.exit_task(task_id);         // Remove task (normal exit)
scheduler.cancel_task(task_id);       // Remove task (resource exhaustion)

// Scheduling decisions
scheduler.on_tick_advanced(delta);    // Update tick counters
scheduler.should_preempt(task_id);    // Check if quantum expired

// State queries
scheduler.has_runnable_tasks();       // Any tasks ready?
scheduler.task_state(task_id);        // Get task state
scheduler.current_task();             // Get running task

// Audit (test-only)
scheduler.audit_log();                // Get scheduling events
```

**Design Principles:**
- **Round-robin scheduling**: Tasks dequeued in FIFO order
- **Deterministic**: Same inputs + same ticks => same schedule
- **No priorities**: All tasks are equal
- **No fairness**: We don't compensate for uneven execution
- **Testable**: Full audit trail of scheduling decisions

**Tests Added:**
- 15 unit tests in `scheduler.rs` covering:
  - Basic enqueue/dequeue operations
  - Round-robin ordering
  - Quantum-based preemption
  - Task state transitions
  - Deterministic behavior
  - Audit log correctness

### 2. SimulatedKernel Integration

**Scheduler Integration:**
- Added `scheduler: Scheduler` field to `SimulatedKernel`
- Tasks automatically enqueued when spawned
- Tasks removed from scheduler when terminated or cancelled
- Scheduler ticks advanced in `advance_time()`

**New API Methods:**
```rust
// Configuration
kernel.with_scheduler_config(config);  // Set scheduler config

// Execution
kernel.run_for_ticks(n);               // Run for N ticks
kernel.run_for_steps(n);               // Run N scheduling rounds

// Observability (test-only)
kernel.scheduler_audit();              // Get scheduling events
kernel.scheduler();                    // Access scheduler state
```

**`run_for_ticks(ticks: u64)` behavior:**
- Dequeues tasks and runs them in quantum-sized chunks
- Advances time by actual ticks consumed
- Preempts tasks after quantum expires
- Stops when target ticks reached or no runnable tasks
- Integrates with CPU budget enforcement
- Returns number of scheduling rounds executed

**`run_for_steps(steps: usize)` behavior:**
- Runs N scheduling decisions (select task, run quantum, preempt)
- Each step runs one task for its quantum or until budget exhaustion
- Deterministic for testing specific scheduling scenarios
- Returns actual steps executed

### 3. CPU Tick Accounting Integration

**Preemption Triggers:**
- After `quantum_ticks` consumed by current task
- When task's CPU budget is exhausted (automatic cancellation)

**Budget Exhaustion Handling:**
```rust
// In run_for_ticks/run_for_steps
if let Some(execution_id) = kernel.get_task_identity(task_id) {
    if kernel.try_consume_cpu_ticks(execution_id, ticks).is_err() {
        // Task cancelled - scheduler.cancel_task() already called
        // by cancel_identity() in try_consume_cpu_ticks
        break;
    }
}
```

**Cancellation Flow:**
1. `try_consume_cpu_ticks()` detects budget exhaustion
2. Calls `cancel_identity()` which records audit event
3. `cancel_identity()` calls `scheduler.cancel_task()`
4. Scheduler removes task from run queue and records exit event
5. Task no longer scheduled

### 4. Scheduling Observability

**Audit Events:**
```rust
pub enum ScheduleEvent {
    TaskSelected { task_id, timestamp_ticks },
    TaskPreempted { task_id, reason, timestamp_ticks },
    TaskExited { task_id, reason, timestamp_ticks },
}
```

**Test Verification:**
```rust
// Check scheduling decisions
let audit = kernel.scheduler_audit();

// Verify task was selected
assert!(audit.iter().any(|e| 
    matches!(e, ScheduleEvent::TaskSelected { task_id, .. } 
        if task_id == my_task)
));

// Verify preemption occurred
assert!(audit.iter().any(|e| 
    matches!(e, ScheduleEvent::TaskPreempted { 
        reason: PreemptionReason::QuantumExpired, 
        .. 
    })
));
```

### 5. Integration Tests

**Five comprehensive integration tests added:**

1. **`test_scheduler_integration_task_enqueued`**
   - Verifies tasks are enqueued on spawn
   - Verifies tasks are removed on termination

2. **`test_scheduler_integration_two_tasks_interleave`**
   - Two tasks with CPU budgets
   - Run for multiple steps
   - Verify both tasks get scheduled
   - Check audit log for interleaving

3. **`test_scheduler_integration_preemption_events`**
   - Single task running for 10 ticks with quantum of 3
   - Verify at least 2 preemption events recorded
   - Validates quantum-based preemption

4. **`test_scheduler_integration_budget_exhaustion`**
   - Task with small CPU budget (15 ticks)
   - Run for 20 ticks
   - Verify task cancelled in scheduler
   - Check audit for ResourceExhaustion exit

5. **`test_scheduler_integration_deterministic`**
   - Two kernels with same config
   - Spawn same tasks in same order
   - Run same number of steps
   - Verify identical audit event counts

**All tests pass (88 total sim_kernel tests passing).**

## What Was NOT Added (Intentionally)

Per the requirements, this phase does NOT include:

- ❌ Priorities or fairness policies
- ❌ SMP / multi-core scheduling
- ❌ Blocking syscalls or user/kernel mode
- ❌ Real interrupt controller integration
- ❌ Changes to the capability model
- ❌ Starvation prevention mechanisms

## Design Decisions

### 1. Scheduler Mechanism vs. Policy

The scheduler provides the mechanism for preemption without imposing policy:
- No priorities: All tasks are equal
- No fairness: We don't track or compensate for execution time
- No starvation prevention: Out of scope

This separation allows future phases to add policies without changing the core mechanism.

### 2. Deterministic FIFO Queue

Used `VecDeque` for the run queue to ensure:
- Deterministic ordering (insertion order preserved)
- O(1) enqueue and dequeue operations
- No hidden randomness or hashing

### 3. Quantum-Based Preemption

Simple time-slicing approach:
- Each task gets N ticks (default: 10)
- After quantum, task is preempted and re-enqueued
- No complex priority calculations or aging

### 4. Integration with Resource Budgets

Scheduler respects budget exhaustion:
- Cancelled tasks automatically removed from run queue
- No special handling needed in scheduler logic
- Budget enforcement remains in `try_consume_cpu_ticks()`

### 5. Test-Visible Audit Log

Scheduling decisions are fully auditable for testing:
- Every task selection recorded
- Every preemption recorded with reason
- Every exit recorded with reason
- Timestamps use scheduler ticks (deterministic)

## Testing Philosophy

**Test Coverage:**
- 15 scheduler unit tests (pure logic)
- 5 kernel integration tests (end-to-end)
- All existing 68 tests still pass (regression prevention)

**Determinism Verification:**
- Multiple tests verify identical inputs => identical outputs
- No timing-dependent flakiness
- Fully reproducible scheduling decisions

**Audit Verification:**
- Tests inspect audit logs to verify expected behavior
- Can trace exact scheduling decisions made
- Validates preemption timing and reasons

## Hardware Interrupt Seam

Though not implemented, the design accommodates future hardware integration:

**Current (Simulation):**
- Scheduler ticks advanced in `advance_time()`
- Explicit calls to `run_for_ticks()` or `run_for_steps()`
- Preemption checked after each task execution

**Future (Hardware):**
- Timer interrupt triggers `scheduler.on_tick_advanced()`
- Interrupt handler checks `scheduler.should_preempt()`
- Context switch saves/restores task state
- Scheduler state and logic unchanged

**What Stays the Same:**
- Identity model
- Capability model
- Resource budgets
- Policy enforcement
- Audit logging

**What Changes:**
- Trigger mechanism (interrupt vs. explicit call)
- Context save/restore (hardware-specific)
- Stack switching (hardware-specific)

## Files Changed

**New Files:**
- `sim_kernel/src/scheduler.rs` (710 lines) - Scheduler implementation

**Modified Files:**
- `sim_kernel/src/lib.rs` (379 lines added) - Kernel integration
- `docs/architecture.md` (169 lines added) - Documentation

## Performance Impact

**Minimal overhead in simulation:**
- Scheduler operations are O(1) or O(n) where n = task count
- No impact on non-scheduled execution paths
- Deterministic tick advancement still fast

**Memory overhead:**
- ~40 bytes per task in scheduler tables
- Audit log grows with events (test-only)

## Backward Compatibility

**API Changes:**
- New optional methods: `with_scheduler_config()`, `run_for_ticks()`, `run_for_steps()`, `scheduler_audit()`
- Existing APIs unchanged
- All existing tests pass without modification

**Behavioral Changes:**
- Tasks now automatically enqueued when spawned
- CPU budget exhaustion now removes task from scheduler
- Time advancement now updates scheduler state

**No Breaking Changes:**
- `run_until_idle()` still works as before
- Manual task execution still possible
- Scheduler is transparent to most code

## Future Work

Phase 23 provides the foundation for:

1. **Priority Scheduling** (Phase 24+?)
   - Add priority levels to tasks
   - Implement priority-based queue selection
   - Keep determinism where possible

2. **I/O Blocking** (Future)
   - Tasks blocked on message receive
   - Unblock when message arrives
   - Integrate with event loop

3. **Cooperative Yielding** (Future)
   - Explicit yield syscall
   - Tasks can voluntarily preempt
   - Useful for well-behaved tasks

4. **Hardware Interrupt Integration** (Future)
   - Timer interrupt handler
   - Context switching
   - Stack management

5. **SMP Support** (Future)
   - Per-CPU run queues
   - Load balancing
   - Lock-free synchronization

## Conclusion

Phase 23 successfully implements a minimal, deterministic, preemptive scheduler that:
- ✅ Runs multiple tasks in time-sliced manner
- ✅ Preempts tasks after configured quantum
- ✅ Produces deterministic scheduling decisions
- ✅ Integrates with CPU tick accounting
- ✅ Is fully testable under SimKernel

The implementation prioritizes correctness and determinism over fairness and performance, staying true to PandaGen's philosophy of "mechanism, not policy."
