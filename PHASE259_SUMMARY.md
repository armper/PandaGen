# Phase 259: Remote IPC Over UDP — Host Calls Into The Kernel

## Summary

Completes the first cut of `docs/next_steps.md` item 4 ("advanced network protocols + hardened remote IPC"). A host tool can now invoke a kernel capability over the network, with the same `remote_ipc` envelope code on both ends.

- `remote_ipc` is now `no_std` + `alloc` (manual `Display` for the error, `Vec` instead of `HashSet`), so the kernel links it. New public pieces: `authorize_call` (decode plus server-list and caller-authority checks, for servers that answer asynchronously), `encode_call`/`encode_response`/`decode_*`, `envelope_to_bytes`/`envelope_from_bytes` for datagram transports, and the contract constants `CAP_KERNEL_COMMAND`, `ACTION_KERNEL_COMMAND_RUN`, `KERNEL_REMOTE_PORT` (7778).
- Byte fields (`RemoteCall.payload`, `RemoteResponse.result`, and the wire envelope's payload) travel as base64 via a dependency-free encoder in `remote_ipc::b64`. serde_json's default spells each byte as a number, and with the response nested inside the envelope a 100-byte command output no longer fit one UDP datagram; a test now asserts a full 256-byte response stays under 1472 bytes. `ipc::MessagePayload::from_raw` wraps already-serialized bytes.
- `kernel_bootstrap`: `RemoteCommandServer` runs in the main loop. `NetStack::service` hands it datagrams for port 7778; it decodes and authorizes the call, checks the command against a read-only allowlist (`help`, `boot`, `mem`, `cpus`, `heap`, `ticks`, and `net` without arguments), forwards it to the command service on its own reply channel, and answers the caller from the response (or `unauthorized` / `command not allowed` / `timeout` after 3 s). One call is served at a time; failures to send a reply are logged.
- `cargo xtask remote <command...>` calls a running kernel from the host through a `UdpTransport` implementing `RemoteTransport`. `qemu-script` gains `remote:<command>;<expected>` steps (`;!text` expects a rejection containing `text`), and both `cargo xtask qemu*` paths forward host port 7778.

## Verification

- `cargo test --workspace` green (remote_ipc 4 tests incl. envelope byte round trip, authorization, base64 edge cases and the datagram budget).
- QEMU, all from the host through remote IPC: `cpus` -> `cpus: online=4 total=4 bsp_lapic=0`, `mem` -> memory and heap lines, `net` -> interface status, `help` -> command list; `halt` and `net ping 10.0.2.2` are refused with `command not allowed`. Serial shows `remote: running "cpus" for 10.0.2.2:<port>` and `remote: refused command "halt"`.

## Not Yet

Calls are unauthenticated beyond the capability id carried in the envelope (anyone who can reach the port can read status), single-flight, and limited to a 256-byte response. Signing or a shared secret, plus a request queue, are the natural hardening steps.
