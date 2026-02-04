# Phase 151: SMP Scheduling Integration (Simulated Kernel)

## Summary
- Wired the simulated kernel to route task scheduling through the SMP runtime when enabled.
- Added SMP-aware `run_for_ticks()`/`run_for_steps()` execution paths that advance per-core time and preempt by quantum.
- Added multi-core task cancellation/exit helpers for SMP scheduler state cleanup.

## Rationale
SMP existed as a standalone runtime, but the simulated kernel never used it for actual scheduling. Integrating SMP into the execution loop provides a concrete, deterministic foundation for multi-core behavior without introducing nondeterminism or hidden concurrency.

## Changes
- sim_kernel/src/smp.rs
  - Added `core_current_task()`, `core_has_runnable()`, and `has_runnable_tasks()` helpers.
  - Added `exit_task_any()` and `cancel_task_any()` to remove tasks without requiring a known core.
- sim_kernel/src/lib.rs
  - Tasks enqueue into the SMP scheduler when SMP is enabled.
  - Task cancellation and exit paths notify the SMP scheduler when enabled.
  - Added SMP-specific execution loops for `run_for_ticks()`/`run_for_steps()`.
  - Added a new SMP scheduling test to validate per-core tick progression.

## Tests
- Not run (not requested).
