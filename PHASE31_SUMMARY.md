# Phase 31: Capability Revocation and Time-Bound Leases

**Completion Date**: 2026-01-19

## Overview

Phase 31 adds **real “take back” authority** to capabilities through explicit revocation and time-bound leases. Leases expire deterministically as simulated time advances, and revocations are auditable.

## What Was Added

### 1. Capability Metadata Extensions (`core_types`)

- `revoked: bool`
- `lease_expires_at_nanos: Option<u64>`
- New `CapabilityEvent` variants: `Revoked`, `LeaseExpired`
- New invalid reason: `LeaseExpired`

### 2. SimKernel Enforcement

- `grant_capability_with_lease()`
- `revoke_capability()`
- Lease expiration on time advancement (`expire_capability_leases()`)
- Validation checks for revoked/expired caps

### 3. Capability Audit Updates

- Audit log includes `Revoked` and `LeaseExpired` events

## Tests Added

- `test_capability_lease_expiration`
- `test_capability_revocation`

## Design Decisions

- **Deterministic expiry**: leases expire on simulated time updates
- **Explicit revocation**: revocation is a first-class audit event
- **No ambient authority**: only the kernel can revoke/lease

## Files Changed

**Modified Files:**
- `core_types/src/capability.rs`
- `sim_kernel/src/lib.rs`
- `sim_kernel/src/capability_audit.rs`

## Future Work

- Lease renewal semantics
- Policy-based revocation control
- Revocation propagation across delegated caps

## Conclusion

Phase 31 makes capability authority reversible and time-scoped, aligning PandaGen with real security expectations.
