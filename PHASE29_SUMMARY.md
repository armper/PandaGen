# Phase 29: StorageOps Enforcement + Transactional Recovery

**Completion Date**: 2026-01-19

## Overview

Phase 29 strengthens PandaGen’s storage layer with **budgeted storage ops** and a **crash-consistent journaled backend**. This keeps storage deterministic, testable, and aligned with explicit resource accounting.

## What Was Added

### 1. Transaction Identifiers

`Transaction` now carries a `TransactionId` for durable journaling and recovery.

### 2. Journaled Storage Backend

**New module:** `services_storage::journaled_storage`

- `JournaledStorage` implements `TransactionalStorage`
- Write-ahead journal with `Write` + `Commit` entries
- `recover()` replays only committed transactions
- `read_data()` provides access to stored payloads

### 3. Storage Budget Enforcement

**New budget trait:**
- `StorageBudget` + `StorageOperation` (Read/Write/Commit)

**Service wrapper:**
- `StorageService<B>` enforces budgets on every operation
- Maps `KernelError` into `StorageServiceError`

### 4. SimKernel Integration

- `SimulatedKernel::try_consume_storage_op()`
- `ResourceEvent::StorageOpConsumed` now supports `Read`
- Budget exhaustion triggers cancellation and audit events

## Tests Added

- Journal recovery test (committed vs uncommitted)
- Storage budget enforcement test
- SimKernel storage budget exhaustion test

## Design Decisions

- **Journal-first**: recovery applies only committed transactions
- **Explicit budgeting**: every storage operation consumes `StorageOps`
- **Service wrapper**: budget enforcement is explicit and testable

## Files Changed

**New Files:**
- `services_storage/src/journaled_storage.rs`

**Modified Files:**
- `services_storage/src/lib.rs` (exports)
- `services_storage/src/transaction.rs` (TransactionId)
- `services_storage/Cargo.toml` (deps)
- `sim_kernel/src/lib.rs` (storage budget enforcement)
- `sim_kernel/src/resource_audit.rs` (StorageOperation::Read)

## Future Work

- Persist journal to real storage devices
- Add snapshot + compaction strategies
- Integrate storage policies (quotas, access controls)

## Conclusion

Phase 29 delivers crash-consistent storage with explicit budget enforcement, keeping PandaGen’s storage model deterministic and auditable without legacy filesystem semantics.
