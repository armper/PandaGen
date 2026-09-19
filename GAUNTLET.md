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
| F5 | kernel/smp | `response_channel` is drained by both the console task and the workspace loop, so a GUI command's output can be consumed by whichever CPU gets there first and is lost | OPEN |
| F6 | kernel/smp | `workspace_loop` holds `&mut Kernel` while every AP holds `&Kernel`; aliasing UB under the optimiser | OPEN |
| F7 | kernel/serial | `SERIAL_LOCK` is taken per `write_str` fragment, so one CPU's line splits another's; `write_byte` skips it entirely | OPEN |
| F8 | hal/virtio | `poll_receive` trusts the device's descriptor id and slot index; out-of-range values panic or read far past the DMA region | OPEN |

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

F1, F2 fixed (278); F3 fixed (279); F4 fixed (280). **Next: F5** (response
channel drained by both the console task and the workspace loop, so GUI
command output can vanish), then F6 (`&mut Kernel` aliased across CPUs), F7
(serial lock per fragment), F8 (virtio descriptor validation).

F5 is the most user-visible of the four and should get an end-to-end test:
issue a command from the graphical shell while application processors are
polling, and require the output to appear in the shell.
