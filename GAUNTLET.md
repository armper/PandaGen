# The Gauntlet

A standing adversarial loop against PandaGen. This file is the loop's memory:
it survives across sessions and context compaction. **If you are resuming, read
this file first and continue from "Resume here".**

## The three elements

**Goal.** Standard, unmodified host tools work against PandaGen the way they
work against a Linux box. `curl` fetches a page. `nc` holds a conversation. A
Python socket client can push and pull real volumes of data. Out-of-order,
lossy, truncated, and hostile input do not break it. Nothing deadlocks.

**Critic.** The specifications (RFC 9293 for TCP, RFC 2131 for DHCP, the virtio
spec) and the observed behaviour of Linux, judged by real client software. Not
"good for a hobby OS". The bar is "software that has never heard of PandaGen
works against it".

**Stop condition.** Three consecutive critic rounds that produce no confirmed
new finding, or the operator returns. The working tree is green and committed
at the end of every round.

## The rule that keeps it honest

**A finding counts only when it reproduces as a failing automated test.**

A critic's claim is a hypothesis. It becomes a finding when a test fails. It
becomes fixed when that same test passes and `cargo test --workspace` is still
green. Claims that do not reproduce are recorded as rejected, with the reason,
so no future round wastes time on them again.

There are two tiers, and a finding must name the tier it reproduced at:

- **Tier 1, library.** A test against the crate API (`net_stack`, `hal_x86_64`)
  where the test *is* the peer and controls timing, loss, and window exactly.
  This is where protocol logic is judged.
- **Tier 2, end to end.** A `gauntlet/*.py` client script run against a booted
  kernel by `qemu-script --keys "gauntlet:<name>"`. This is where "real
  software works against it" is judged.

**Tier 2 has a blind spot worth remembering.** QEMU user-mode networking is a
TCP *proxy*, not a wire. The host connection terminates in slirp, which opens
its own well-behaved connection to the guest. Slirp therefore hides wire-level
defects: it paces sends to the advertised window, retransmits on our behalf,
and never reorders. A tier-2 pass is not evidence that the wire behaviour is
correct. Protocol claims must be judged at tier 1.

## Roles

- **Lead (the main session).** Owns QEMU, owns the git history, verifies every
  claim, applies fixes, commits one phase per closed finding.
- **Critics (fresh-context subagents).** Read code against a specification,
  produce ranked, reproducible hypotheses. They never run QEMU (host port
  conflicts) and never modify files.
- **Builders (subagents, optional).** Implement a fix for a confirmed finding
  when it is large enough to be worth delegating.

## Harness

- `cargo xtask qemu-script --keys "..."` drives a headless boot with keystrokes,
  mouse, screenshots, and host-side network steps (`udp:`, `tcp:`, `remote:`,
  `remote-tcp:`, `replay:`).
- `--port-base N` shifts the four forwarded ports and uses a private disk image,
  so several QEMU instances can run at once.
- `gauntlet/` holds the adversarial scripts. Each finding gets one script that
  fails before the fix and passes after.
- `cargo xtask gauntlet` runs every script in `gauntlet/` against one boot.

## Findings

Status values: `OPEN` (confirmed, not fixed), `FIXED` (with the phase number),
`REJECTED` (did not reproduce; reason recorded).

| # | Area | Finding | Status |
|---|------|---------|--------|
| F1 | kernel/dhcp | `maybe_renew` froze its own clock, so the reply wait `0 < 100` never ended and the boot CPU spun until reset | FIXED (278) |
| F2 | kernel/tcp | Command port served one request per received packet, so a pipelined client stalled until unrelated traffic arrived (`cmd_pipeline`) | FIXED (278) |
| F3 | net_stack/tcp | Four idle connections to the unauthenticated echo port take the signed command port offline permanently; no reaper, and a full table black-holes instead of resetting (`tcp_slots`) | FIXED (279) |
| F4 | kernel/smp | One `WORK` queue with two independent owners that both call `reset()`; present bands and `smp run` jobs corrupt each other's ids and results | FIXED (280) |
| F5 | kernel/smp | `response_channel` is drained by both the console task and the workspace loop, so a GUI command's output can be consumed by whichever CPU gets there first and is lost | FIXED (281) |
| F6 | kernel/smp | `workspace_loop` holds `&mut Kernel` while every AP holds `&Kernel`; aliasing UB under the optimiser | FIXED (282) |
| F7 | kernel/serial | `SERIAL_LOCK` is taken per `write_str` fragment, so one CPU's line splits another's; `write_byte` skips it entirely | FIXED (282) |
| S1 | services_storage | Remount discarded most of the filesystem: ring scanned in block order, and the ring was the only persistent metadata | FIXED (284) |
| S2 | services_storage | `blocks[0]` on a zero-length write panicked, and the kernel aborts on panic: an empty file bricked the machine | FIXED (284) |
| S4 | services_storage | A stale commit sequence overwrote a live ring record, destroying an already-committed object at a later unrelated write | FIXED (284) |
| S7 | services_storage | `total_blocks` from block 0 was trusted; a byte edit turned a mount into an unbounded allocation | FIXED (284) |
| S3 | services_storage | Reads assume contiguous allocation; `allocate_blocks` does not guarantee it, so an object can read another's blocks | OPEN |
| S5 | services_storage | Object ids restart at 1 every boot, so files from different boots alias to one object | OPEN |
| S6 | services_storage | A failed commit returns `Ok` on retry having written nothing, and leaks its blocks | OPEN |
| S8 | services_storage | Multi-step operations are not atomic; `write_file_by_name` has a window where neither version is reachable | OPEN |
| S9 | services_storage | Nothing is ever freed; deletion leaks, and the disk fills monotonically | OPEN |
| S10 | hal/virtio_blk | `VIRTIO_BLK_F_FLUSH` is never negotiated and `UNSUPP` is treated as success, so nothing survives host power loss | OPEN |
| X1 | boot config | The shipped image authenticates with the published constant `pandagen-dev` and accepts any caller name | OPEN |
| X2 | remote_ipc | The 256-entry replay window is flushable, and nonces are not ordered or time-bound | OPEN |
| X3 | kernel/remote | `boot` is on the remote allowlist and discloses kernel physical/virtual addresses and the HHDM offset | OPEN |
| F9 | net_stack/tcp | `listen` silently ignored a third port (only two slots), so HTTP was never bound and every client got a reset. Found within seconds of pointing real `curl` at the machine | FIXED (283) |
| F8 | hal/virtio | `poll_receive` trusts the device's descriptor id and slot index; out-of-range values panic or read far past the DMA region | FIXED (282) |

## Rejected claims

Claims that did not reproduce. Do not re-investigate these without new evidence.

| Claim | Why it did not reproduce |
|-------|--------------------------|
| Command port loses the reply when the client half-closes | `cmd_halfclose` passes. Slirp does not coalesce the client's FIN onto the data segment, so `peer_closed()` is false when the line is read and the early `close()` never fires. Real on a direct wire; unreachable here. |
| Concurrent SYNs lose their SYN-ACKs to the single `reply` slot | `tcp_concurrent` passes with four simultaneous clients. Slirp opens its guest connections sequentially enough that the replies are not overwritten. Needs tier 1 to judge. |
| Echo service silently drops mid-stream data when its send buffer fills | `tcp_bulk_echo` passes on 64 KiB byte for byte. Slirp paces to the advertised window, so the send buffer never fills at loopback speed. Real code defect; needs tier 1. |
| Abandoned (closed) connections leak slots | `tcp_abandon` passes: closing sends RST/FIN and the slot is freed. Only *held-open* connections wedge the table, which is F3. |

## Round log

### Round 1

Critics dispatched: TCP conformance (RFC 9293 + Linux), packet parser
robustness (malformed input reachability), SMP concurrency (lock-order graph
after the Phase 275 move to fine-grained locks).

Harness built: `--port-base N` for concurrent QEMU instances with private disk
images, and a `gauntlet:<name>` step that runs a real client script and fails
the run on a non-zero exit.

First tier-2 sweep, five scripts against the unmodified kernel: **all passed.**
`tcp_concurrent`, `tcp_halfclose`, `tcp_pipeline`, `tcp_bulk_echo`,
`tcp_abandon`. Two lessons, both recorded above: the echo service drains its
whole receive buffer (so pipelining and half-close had to be re-aimed at the
*command* port, which reads one line and closes early), and slirp masks wire
behaviour, so the TCP conformance claims must be judged at tier 1.

The SMP critic returned a clean lock-order graph (acyclic; `GLOBAL_HEAP` and
`SERIAL_LOCK` are true leaves) and no deadlock, which is a meaningful negative
result. Its positive findings are about shared state, not lock order.

## Resume here

**Round 1 is closed.** All eight findings are fixed across Phases 278-282, and
the whole gauntlet suite passes.

**Next: round 2.** Two things in parallel:

1. Done (Phase 283). PandaGen serves HTTP on port 8080 and `gauntlet:http_curl`
   holds it to real `curl`. Pointing curl at it found F9 immediately.
2. Storage and security critics have reported. Storage S1, S2, S4, S7 are
   fixed (Phase 284). **Next: X1**, which is the most serious thing on the
   board — the shipped ISO authenticates with a constant published in this
   repository — then X3 (address disclosure), then the remaining storage
   findings in the critic's suggested order: S5 (id aliasing), S3
   (contiguity), S6 (lying commit), S8 (atomicity).

Still unvisited by any critic: the graphics and compositor path, the boot
path, and `services_*` above the kernel.
