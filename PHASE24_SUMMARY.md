# Phase 24: User/Kernel Boundary (Syscalls as Typed Messages)

**Completion Date**: 2026-01-19

## Overview

Phase 24 introduces a **message-based syscall boundary** that maps `KernelApi` calls to typed IPC messages. This creates a clean, testable user/kernel seam without adopting POSIX-style syscalls.

## What Was Added

### 1. Syscall Message Types (`kernel_api/src/syscalls.rs`)

**Typed request/response payloads:**
- `SyscallRequest` + `SyscallRequestPayload`
- `SyscallResponse` + `SyscallResponsePayload`
- `SyscallError` / `SyscallErrorKind` (serializable error model)

**Design goals:**
- Preserve **typed semantics** across the boundary
- Ensure **schema versioning** with `SchemaVersion::new(1, 0)`
- Keep error propagation explicit and structured

### 2. Syscall Codec

`SyscallCodec` encodes/decodes typed payloads into `ipc::MessageEnvelope`:
- Action names: `kernel.syscall.request` / `kernel.syscall.response`
- Schema compatibility checks enforced
- Payload serialization uses existing `MessagePayload`

### 3. Client/Server Adapters

**Server:**
- `SyscallServer<K: KernelApi>` decodes requests and executes the matching kernel call
- Results are serialized into typed responses with correlation IDs

**Client:**
- `SyscallClient<T: SyscallTransport>` implements `KernelApi`
- Each call performs a **round-trip** message exchange

### 4. Transport Abstraction + Test Loopback

- `SyscallTransport` trait abstracts send/receive over IPC
- `LoopbackTransport` provides a deterministic in-process transport for tests

## Tests Added

- `test_syscall_spawn_round_trip` verifies full request/response flow
- `test_syscall_codec_round_trip` verifies encode/decode correctness

## Design Decisions

- **No POSIX syscalls**: the boundary is explicitly message-based and typed.
- **Schema-aware**: message schema versioning is enforced at the boundary.
- **Test-first**: loopback transport makes the syscall layer fully deterministic.

## Files Changed

**New Files:**
- `kernel_api/src/syscalls.rs`

**Modified Files:**
- `kernel_api/src/lib.rs` (exports)
- `kernel_api/src/kernel.rs` (serialization derives)

## Future Work

- Add streaming syscall transports (IPC channels, network)
- Introduce typed capability delegation over syscalls
- Extend kernel-side validation and policy hooks

## Conclusion

Phase 24 establishes a modern, typed syscall boundary that stays true to PandaGen’s philosophy: explicit, testable, and message-driven—no legacy interfaces.
