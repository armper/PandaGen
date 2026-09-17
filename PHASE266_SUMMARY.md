# Phase 266: Model-Based Checking Of IPC Channel Access

## Summary

Third and last of the three guarantees `docs/next_steps.md` item 5 names (capabilities, scheduling, IPC), with the same harness shape as Phases 264-265.

- `formal_verification/tests/ipc_channel_model.rs`: a reference model of per-channel access lists (`grant_channel_access`, `revoke_channel_access`) and bounded FIFO delivery, driven against `SimulatedKernel` with send (identified or anonymous), receive (with or without a receive context), grant, and revoke:
  - C1 send and receive are accepted exactly when the model accepts them;
  - C2 every received message is the head of the model's FIFO for that channel;
  - C3 revoking the last holder leaves the channel closed;
  - C4 the kernel's pending message count matches the model.
- Exhaustive: all 14^4 = 38,416 sequences of four operations over one channel, two tasks, and the anonymous sender. Random: 400 fixed-seed sequences of 60 operations over two channels and three tasks, with channel capacity 2 so the full-queue path is exercised. About a second in total.

## Findings And Fixes

Reading the enforcement code for the model exposed two fail-open paths, both now closed in `sim_kernel`:

- `revoke_channel_access` deleted the access list once it became empty, so revoking the only holder of a channel made the channel usable by every task again. The list now persists when empty: a channel whose holders were all revoked is closed. Channels that never had a list stay open, which is what the existing bootstrap and service tests rely on.
- Access was only checked when the message carried a `source` (send) or a receive context was set (receive). An anonymous message or a context-less receive bypassed the list entirely. On a channel with an access list these are now refused; one kernel test that sent anonymously onto a restricted channel was updated to identify its sender.

All 160 `sim_kernel` tests and the rest of the workspace pass with the stricter rules.

## Verification

- `cargo test --workspace` green.

## Next

Item 5's three named guarantees now each have a checker. Deeper coverage would model the real-time (EDF) scheduling policy and message budgets; those checks would reuse the same harnesses with the policy switched on.
