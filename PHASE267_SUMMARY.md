# Phase 267: Replay Protection For Remote IPC

## Summary

Closes the replay gap noted in Phase 260: a captured, correctly signed datagram could be sent again to re-run the same read-only command.

- `remote_ipc::ReplayGuard<N>`: a fixed-size ring of the last `N` accepted message ids (`accept` records and refuses duplicates, `is_replay` queries). Message ids are random UUIDs from the client, so within the window a replay is exactly a repeated id. Tested for immediate refusal, window eviction, and re-acceptance after eviction.
- The kernel keeps a `ReplayGuard<256>` and checks it right after signature verification and before authorization; a hit is logged as `remote: replayed message dropped`, counted as denied, and gets no reply.
- `qemu-script` gains a `replay:<command>` step: it sends one signed call, requires a reply, resends the identical bytes, and requires silence.

## Verification

- `cargo test --workspace` green.
- QEMU: `replay:cpus` and `replay:help` are both answered once and refused on resend (`remote: replayed message dropped` twice in serial); a `remote:mem` call in between is served normally, so a dropped replay leaves the server ready.

## Limits

The window is 256 accepted messages; a replay older than that would be accepted. There is still one shared token rather than per-caller keys.
