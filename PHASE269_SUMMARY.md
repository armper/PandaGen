# Phase 269: Model-Based Checking Of The EDF Scheduling Policy

## Summary

Extends the Phase 265 scheduler checker to the earliest-deadline-first policy (`docs/next_steps.md` item 5).

- `formal_verification/tests/edf_model.rs`: the model keeps the same run-queue list as the scheduler (real-time tasks inserted before the first task with a later deadline, others appended) and predicts admission (`set_real_time_params` with budget and period sanity plus the 100% utilisation bound), selection (queued task with the earliest deadline, first of equals, else the queue head), budget accounting and `should_preempt`, budget-exhausted parking until the deadline, period rollover, deadline-miss events per period as multisets, and the usual state/current/runnable-count agreement.
- Exhaustive: all 16^5 = 1,048,576 sequences of five operations over two tasks with two parameter sets. Random: 400 fixed-seed sequences of 60 operations over three tasks and four parameter sets. About nine seconds.

## Findings And Fixes

- Exited tasks kept accruing `DeadlineMissed` events every period (found by the five-step sequence `Spawn, SetRt, Tick(2), Exit, Tick(2)`). `refresh_deadlines` now skips finished tasks.
- Exited and cancelled tasks kept their real-time reservation, so admission control still counted their bandwidth. `exit_task` and `cancel_task` now release it.
- `refresh_deadlines` walked the task `HashMap`, so the order of `DeadlineMissed` events for tasks missing on the same tick was arbitrary; it now visits tasks in a fixed order.

All 160 `sim_kernel` tests still pass.

## Verification

- `cargo test --workspace` green.

## Coverage Note

With the capability, round-robin, EDF, and IPC channel models in place, every guarantee named by item 5 has an executable specification and a bounded exhaustive check. Message budgets (`try_consume_message`) remain the one enforcement path without a model.
