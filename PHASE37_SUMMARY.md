# Phase 37: Networked Capability Calls (Remote IPC with Explicit Authority)

**Completion Date**: 2026-01-19

## Overview

Phase 37 introduces **remote capability calls** over IPC with explicit, portable authority tokens. Calls require both client authority and server allowlists.

## What Was Added

### 1. Remote IPC Crate (`remote_ipc`)

- `CapabilityAuthority` (caller + allowed caps)
- `RemoteCall` + `RemoteResponse`
- Codec helpers for message envelopes

### 2. Client/Server

- `RemoteIpcClient` validates authority before sending
- `RemoteIpcServer` validates authority and allowlist
- `RemoteTransport` trait + loopback test transport

## Tests Added

- Success path with valid authority
- Denied call when authority mismatches

## Files Changed

**New Files:**
- `remote_ipc/Cargo.toml`
- `remote_ipc/src/lib.rs`

**Modified Files:**
- `Cargo.toml` (workspace member + dependency)

## Conclusion

Phase 37 provides a deterministic, explicit authority model for remote IPC capability calls.
