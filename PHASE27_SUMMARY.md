# Phase 27: Remote UI Host (Snapshot Streaming Over IPC/Network)

**Completion Date**: 2026-01-19

## Overview

Phase 27 introduces a **remote UI snapshot streaming service** that publishes `WorkspaceRenderSnapshot` frames over IPC or network transports. This enables decoupled UI hosts and remote rendering without terminal emulation or POSIX streams.

## What Was Added

### 1. Remote UI Host Service (`services_remote_ui_host`)

**Core types:**
- `RemoteSnapshotFrame` (revision + timestamp + snapshot)
- `SnapshotSink` trait (pluggable transport)
- `RemoteUiHost` (fan-out publisher with revision tracking)

### 2. Transport Sinks

- **IPC sink**: `IpcSnapshotSink<K: KernelApi>`
  - Wraps snapshots in `MessageEnvelope`
  - Action: `ui.snapshot`
  - Schema version: `v1.0`

- **Network/stream sink**: `JsonLineSink<W: Write>`
  - Emits newline-delimited JSON frames for TCP/UDP-style transports

- **Test sink**: `InMemorySink`

### 3. Serializable Render Snapshots

`WorkspaceRenderSnapshot` now derives `Serialize`/`Deserialize` for transport-safe encoding.

## Tests Added

- `test_remote_ui_host_revision_increments`
- `test_ipc_snapshot_sink_sends_message`

## Design Decisions

- **Snapshot-first**: UI rendering is frame-based, not stream-based.
- **Transport-agnostic**: sinks abstract IPC vs network delivery.
- **Deterministic revisioning**: the host increments revisions on publish.

## Files Changed

**New Files:**
- `services_remote_ui_host/Cargo.toml`
- `services_remote_ui_host/src/lib.rs`

**Modified Files:**
- `Cargo.toml` (workspace member + dependency)
- `services_workspace_manager/src/lib.rs` (snapshot serialization)

## Future Work

- Remote UI subscription negotiation
- Delta compression between frames
- Encrypted snapshot transport

## Conclusion

Phase 27 enables a clean, message-based remote UI pipeline that keeps PandaGen’s UI model explicit, testable, and transport-neutral.
