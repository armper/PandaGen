# Phase 274: Larger Command Replies And An Accurate README

## Summary

Two housekeeping items after the Phase 252-273 run.

- The kernel command service's reply buffer (`RESPONSE_MAX`) grows from 256 to 512 bytes. `net` and `cpus` status had outgrown 256 bytes and were cut mid-line (for example `net: tcp port 7779 conns=0 accepte`). The `remote_ipc` datagram-budget test now pins a 512-byte reply under one UDP payload once base64-wrapped in an envelope; TCP replies fit the 2 KiB send buffer comfortably.
- `README.md` described the system as of the text-only era and, worse, listed every future item as done. The status, crate list (`graphics_rasterizer`, `net_stack`), bare-metal track, a new "Talking To A Running Kernel" section (`cargo xtask remote`, `remote-tcp`, and scripted host-side verification steps), and the roadmap now match the repository: graphics (213-251), storage/SMP/networking/verification (252-273), and the genuinely open items.

## Verification

- `cargo test --workspace` green.
- QEMU: `net` prints the full TCP status line (`... rexmit=0 rst=0 echoed=0B`), `cpus` prints all four CPU lines, and `remote:net` over UDP and `remote-tcp:net` over TCP both return the complete multi-line status.
