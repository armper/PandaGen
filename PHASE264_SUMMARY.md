# Phase 264: Model-Based Checking Of The Capability Lifecycle

## Summary

First substantive step on `docs/next_steps.md` item 5 (formal verification of critical paths). The existing `formal_verification` crate checks static snapshots (unique ids, non-overlapping regions). This phase adds an executable specification of the capability lifecycle and checks `sim_kernel` against it over every short operation sequence and many long random ones.

- `formal_verification/tests/capability_model.rs`: a reference `Model` of grant, lease, delegate (move semantics), revoke, drop, time advance, and task death. A `Harness` drives `SimulatedKernel` with the same operations and after every step checks:
  - I0 the kernel accepts exactly the operations the model accepts;
  - I1 `is_capability_valid(cap, task)` agrees with the model for every (capability, task) pair, alive or dead;
  - I2 a capability is valid for at most one task;
  - I3 no resurrection: once valid for nobody, a capability stays that way;
  - I4 audit completeness: revoke, delegate, drop counts match the log, and each expired lease is logged exactly once;
  - I5 a rejected operation changes no validity.
- Two explorations: exhaustive, every sequence of three operations from the 27 possible over two tasks and two capabilities (19,683 sequences, complete for that bounded domain), and 300 fixed-seed random sequences of 40 operations over three tasks and four capabilities. Deterministic, dependency-free, under a second.

## Finding And Fix

The very first exhaustive sequence failed I0: `grant(task0, cap1)` twice was accepted. `record_capability_grant` inserted into the table unconditionally, so re-granting an existing id overwrote the owner and cleared `revoked`: a live capability could be stolen from its holder by anyone able to call grant, and a revoked one resurrected. The random search reproduced it as a lease grant taking a live capability away from another task.

`sim_kernel` now rejects a grant or lease grant while the id is still usable by its current holder (`KernelError::InvalidCapability("capability N is still held by ...")`). Re-issuing an id whose previous incarnation is dead (revoked, dropped, expired, or owner terminated) stays allowed; the crash-restart resilience test depends on that, and the model treats such a re-issue as a new life for the id. One resilience test (`test_explicit_capability_grant`) had asserted the old overwrite as if both tasks held the capability; it now asserts single ownership and uses delegation to move the capability, which is what its own doc comment ("cannot be implicitly inherited or duplicated") describes.

The lease-expiry audit check also had to become incarnation-aware: the kernel logs `LeaseExpired` once per lease that is still live when its expiry passes, so the model tracks `(cap, expiry)` pairs rather than one event per id.

## Verification

- `cargo test --workspace` green (sim_kernel 160, resilience suites, and the two new model tests, which run in about 0.7 s).

## Next

The same harness shape fits the scheduler (fairness and quantum accounting in `sim_kernel::scheduler`) and IPC channel access (`grant_channel_access`/`revoke_channel_access`), which are the other two guarantees item 5 names.
