# Phase 265: Model-Based Checking Of The Scheduler

## Summary

Second step on `docs/next_steps.md` item 5, using the Phase 264 harness shape on `sim_kernel::scheduler::Scheduler` (round-robin policy).

- `formal_verification/tests/scheduler_model.rs`: a reference model of spawn (`enqueue`), `dequeue_next`, tick advance with wake-ups, `preempt_current`, `block_task`, `unblock_task`, `exit_task`, and `cancel_task`, compared with the real scheduler after every operation:
  - S1 agreement of every task's state, the current task, and the runnable count;
  - S2 `dequeue_next` returns exactly the task the FIFO model predicts (this pins round-robin fairness and wake-up order);
  - S3 exited or cancelled tasks are never selected and cannot be revived;
  - S4 `should_preempt` agrees with the quantum accounting;
  - S5 selected, preempted, and exited audit counts match the operations that took effect.
- Exhaustive: all 14^5 = 537,824 sequences of five operations over two tasks. Random: 500 fixed-seed sequences of 60 operations over four tasks. About four seconds in total.

## Findings And Fixes

- `exit_task` and `cancel_task` logged a `TaskExited` event for task ids the scheduler had never seen and again for tasks that had already exited (the exhaustive search hit it with `Dequeue x4, Exit(0)` on an unspawned task). Both are now no-ops for unknown or finished tasks, so the audit log carries one exit event per task.
- `block_task` overwrote any state with `Blocked`, so blocking an exited task and letting its wake tick pass put a dead task back on the run queue. Finished tasks now stay finished.
- `wake_ready_tasks` collected due tasks from a `HashMap`, so two tasks becoming due on the same tick were re-queued in hash order, contradicting the scheduler's "deterministic ordering" contract. `TaskInfo` records a `block_seq` at block time and wake-ups are sorted by (wake tick, block order).

All 160 existing `sim_kernel` tests still pass.

## Verification

- `cargo test --workspace` green.

## Next

The remaining guarantee named by item 5 is IPC: channel access (`grant_channel_access` / `revoke_channel_access`) and message delivery ordering can be modelled the same way.
