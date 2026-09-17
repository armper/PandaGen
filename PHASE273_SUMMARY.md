# Phase 273: Authenticated Command Channel Over TCP

## Summary

Completes the "hardened remote IPC" half of `docs/next_steps.md` item 4 for TCP: the kernel serves read-only commands on TCP port 7780 to any socket client, with the same shared-token authentication and replay protection as the UDP path.

- `remote_ipc::line`: a one-line request format, `<nonce as 32 hex digits> <base64 HMAC-SHA256 tag> <command>`, where the tag covers the nonce and command under the shared token; replies are one line, `+<base64 output>` or `-<message>`. `sign`, `verify`, `reply`, and `parse_reply` are tested for round trip, wrong token, tampering, malformed input, and reply decoding. `ReplayGuard` gains `accept_key`/`is_replay_key` for raw 128-bit nonces.
- `net_stack::tcp`: two listen ports and `read_line`, which hands out a line only once its newline has arrived (or the buffer is full).
- `kernel_bootstrap`: `NetStack::service` now returns a `RemoteRequest` (a UDP envelope or a TCP line); command-port connections are logged with their local port, closed after the peer closes, and answered with `tcp_reply`. The remote command server replies through a `ReplyTarget` (UDP envelope or TCP connection); TCP lines are verified, checked against the replay window by nonce, and run through the same read-only allowlist. A bad signature or replayed nonce is answered `-unauthorized` (the connection already exists, so unlike UDP there is nothing to gain from silence).
- `cargo xtask remote-tcp <command...>` and the `remote-tcp:<command>;<expected>[;<token>]` script step; both `qemu*` paths forward host TCP 127.0.0.1:7780.

## Verification

- `cargo test --workspace` green (remote_ipc 7, net_stack 22).
- QEMU, all from host sockets: `remote-tcp:cpus` -> `cpus: online=4 total=4 bsp_lapic=0`; `halt` -> `command not allowed`; `cpus` signed with `wrong-token` -> `unauthorized` (serial: `remote: tcp line rejected (bad signature)`); `remote-tcp:net` -> the interface status `via dhcp`; UDP `remote:mem` and the TCP echo port keep working in between.

## Limits

One request in flight at a time across both transports, and TCP connections are single-line sessions in practice (the client closes after the reply). Per-caller keys remain the next hardening step.
