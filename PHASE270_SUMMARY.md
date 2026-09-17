# Phase 270: Model-Based Checking Of Message Budgets

## Summary

Covers the last enforcement path without an executable specification (`docs/next_steps.md` item 5): per-identity message budgets in `sim_kernel`.

- `formal_verification/tests/message_budget_model.rs`: tasks are spawned with identities; the model tracks each identity's usage, optional limit, and cancellation, plus bounded FIFO channels, and predicts:
  - B1 send/receive acceptance (not cancelled, budget not spent, channel not full/empty);
  - B2 usage equals the number of messages actually sent or received;
  - B3 the first attempt beyond the limit cancels the identity once, cancels its scheduler task, and refuses everything after;
  - B4 `MessageConsumed` events match successful operations and `BudgetExhausted` events match cancellations;
  - B5 FIFO order per channel.
- Exhaustive: all 8^5 = 32,768 sequences of five operations over one channel and two tasks. Random: 300 fixed-seed sequences of 60 operations over two channels and three tasks. About a second.

## Findings And Fixes

- A send to a full channel and a receive from an empty channel charged the sender's or receiver's budget before failing (first random sequence: `SetBudget(0,1), Recv(0,0)` left usage at 1 with nothing received). `send_message` now checks capacity, and `receive_message` checks for a queued message, before `try_consume_message` runs; a budget is only charged for messages that are actually queued or delivered.
- Identities without a budget were not metered at all, so a budget attached later applied to a usage count that ignored everything sent before. Usage is now counted for every identity; only the limit is optional.

All 160 `sim_kernel` tests and the rest of the workspace pass.

## Verification

- `cargo test --workspace` green.

## Coverage

Item 5's checkers now cover the capability lifecycle, round-robin and EDF scheduling, IPC channel access, and message budgets, each as an exhaustive bounded search plus random long runs, all deterministic and dependency-free. Across Phases 264-270 they surfaced nine real defects in `sim_kernel`.
