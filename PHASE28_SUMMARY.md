# Phase 28: Networking Stack as Services (Packet IO + Policy + Budgets)

**Completion Date**: 2026-01-19

## Overview

Phase 28 introduces a **networking service layer** that models packet I/O as explicit, policy-governed operations with budget enforcement. This keeps networking consistent with PandaGen’s explicit, testable resource model.

## What Was Added

### 1. Packet Budget Resource (`resources`)

- New `PacketCount` resource type
- Integrated into:
  - `ResourceBudget`
  - `ResourceUsage`
  - `ResourceExceeded`
  - `ResourceDelta`
- Added packet consumption helpers and tests

### 2. SimKernel Packet Enforcement

- `ResourceEvent::PacketConsumed` + `PacketOperation`
- `SimulatedKernel::try_consume_packet()`
- Audit logging + cancellation on budget exhaustion

### 3. Networking Service (`services_network`)

**Core types:**
- `NetworkInterfaceId`, `Endpoint`, `Packet`, `PacketProtocol`
- `PacketContext` + `PacketDirection`

**Policy:**
- `NetworkPolicy` trait
- `AllowAllPolicy`, `DenyAllPolicy`

**Budgets:**
- `PacketBudget` trait
- SimKernel implementation via `try_consume_packet()`

**Service API:**
- `send_packet()` and `receive_packet()`
- Policy evaluated before budget consumption

## Tests Added

- Packet budget enforcement tests in `resources`
- `resource_audit` packet event test
- `services_network` send/receive + policy + budget tests

## Design Decisions

- **Service-first networking**: packets are explicit messages, not sockets.
- **Budgeted delivery**: every packet consumes `PacketCount`.
- **Policy gating**: allow/deny decisions are explicit and testable.

## Files Changed

**New Files:**
- `services_network/Cargo.toml`
- `services_network/src/lib.rs`

**Modified Files:**
- `resources/src/lib.rs` (PacketCount integration)
- `sim_kernel/src/lib.rs` (packet budget enforcement)
- `sim_kernel/src/resource_audit.rs` (packet audit events)
- `Cargo.toml` (workspace member + dependency)

## Future Work

- Interface capabilities and routing policies
- Real network device HAL integration
- Packet filtering and flow control services

## Conclusion

Phase 28 establishes a clean, budgeted, policy-aware networking service that matches PandaGen’s message-driven architecture and avoids legacy socket semantics.
