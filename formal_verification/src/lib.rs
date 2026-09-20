//! Model-based checking of `sim_kernel`, which lives entirely in `tests/`.
//!
//! This file used to hold a "formal verification pass scaffolding" API:
//! `VerificationReport`, `VerificationError`, and nine `verify_*` functions
//! over hand-written model structs. Not one of them was called -- not by any
//! crate in the workspace, and not by this crate's own tests, which is what
//! made it easy to keep. A crate named `formal_verification` whose library is
//! never invoked reads, to anyone scanning the tree, like a guarantee that
//! something is being verified.
//!
//! What verifies anything is in `tests/`, and it is considerably stronger
//! than what was here: each of these drives the real `SimulatedKernel`
//! through the same operations as a small reference model and compares them
//! after every step, exhaustively over a bounded domain and then over long
//! deterministic random sequences.
//!
//! - `capability_model` -- grant, lease, delegate, revoke, drop, expiry and
//!   task death, against I0-I5 (acceptance, agreement, single owner, no
//!   resurrection, audit, failed operations are inert).
//! - `scheduler_model` and `edf_model` -- run-queue and deadline ordering.
//! - `ipc_channel_model` -- channel lifecycle and delivery.
//! - `message_budget_model` -- per-identity message accounting.
//!
//! New invariants belong beside those, driving the real kernel. An invariant
//! checked against a model of the kernel and never against the kernel is a
//! statement about the model.
