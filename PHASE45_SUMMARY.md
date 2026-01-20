# Phase 45-54: Scheduler, Kernel API v0, IPC Refinement, Memory v2, Preemption, IDT, Input, Registry

**Completion Date**: 2026-01-20

## Overview

This phase set advances PandaGen’s minimal kernel track from a single-loop console to
explicit services with cooperative scheduling, a tiny kernel API v0, typed IPC
messages with versioning, and early hardware scaffolding. It also upgrades the
bootstrap frame allocator, introduces a pluggable kernel heap interface, adds
preemption/time-slice scaffolding, and extends the input pipeline and service
registry for dynamic discovery.

## What Was Added

### 1. Cooperative Scheduler + Console Service Split (Phase 45)
- Cooperative scheduler in `kernel_bootstrap` with round-robin task selection.
- Console split into a user-domain `ConsoleService` and in-kernel `CommandService`.
- Message passing between services via typed request/response messages.

### 2. Kernel API v0 (Phase 46)
- New `KernelApiV0` trait with minimal task/channel/capability surface.
- Simulated kernel implements `KernelApiV0` alongside the full kernel API.

### 3. User-Task Stub + Console Offload (Phase 47)
- Console runs as a user-domain task stub (same address space), using the v0 API
  to send commands and receive responses.

### 4. IPC Refinement + Typed Messages (Phase 48)
- New typed IPC schema for command requests/responses with versioning and
  structured errors (`ipc::typed`).

### 5. Physical Memory Manager v2 (Phase 49)
- Frame allocator now tracks reclaimed frames and excludes reserved ranges.
- Deterministic allocator tests added to the bootstrap kernel.

### 6. Kernel Heap + Alloc Traits (Phase 50)
- Kernel allocator trait with explicit allocation lifetime and accounting.
- Bump allocator implements the trait with usage/alloc count reporting.

### 7. Timer + Preemption Scaffold (Phase 51)
- Timer interrupt hooks added to HAL.
- PIT/HPET scaffolding in `hal_x86_64` and time-slice tracking in bootstrap.

### 8. Interrupt/IDT Skeleton (Phase 52)
- Safe interrupt registration registry in HAL.
- Minimal x86_64 IDT skeleton with install/register helpers.

### 9. Keyboard Input Service (Phase 53)
- Input HAL bridge can now deliver KeyEvents via an input service sink.
- Kernel-backed delivery sink for real IPC routing added.

### 10. Service Registry v0 (Phase 54)
- Registry extended with descriptors and name-based lookup for dynamic discovery.

## Tests Added / Updated

- `cargo test -p kernel_bootstrap`
- `cargo test -p ipc -p kernel_api -p sim_kernel -p services_input -p services_input_hal_bridge -p services_registry -p hal -p hal_x86_64`

## Files Changed (Highlights)

- `kernel_bootstrap/src/main.rs`
- `kernel_api/src/lib.rs`, `kernel_api/src/v0.rs`, `sim_kernel/src/lib.rs`
- `ipc/src/typed.rs`, `ipc/src/lib.rs`
- `hal/src/interrupts.rs`, `hal/src/timer.rs`, `hal/src/lib.rs`
- `hal_x86_64/src/idt.rs`, `hal_x86_64/src/timer.rs`, `hal_x86_64/src/lib.rs`
- `services_input/src/lib.rs`, `services_input_hal_bridge/src/lib.rs`
- `services_registry/src/lib.rs`
- `docs/interfaces.md`, `docs/architecture.md`

## Conclusion

These phases establish a coherent boot-to-service path: a cooperative scheduler,
minimal kernel API, versioned IPC, and early hardware scaffolding. The bootstrap
kernel now exposes explicit task boundaries and message routing, and the service
layer gains typed messages and dynamic discovery while staying testable and
capability-driven.
