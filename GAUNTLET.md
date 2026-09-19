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
| S3 | services_storage | Reads assume contiguous allocation; `allocate_blocks` does not guarantee it, so an object can read another's blocks | FIXED (288) |
| S5 | services_storage | Object ids restart at 1 every boot, so files from different boots alias to one object; confirmed on a real image (`b.txt` and `p.txt` shared one id, `cat b.txt` printed p.txt) | FIXED (286) |
| S6 | services_storage | A failed commit returns `Ok` on retry having written nothing, and leaks its blocks | FIXED (289) |
| S8 | services_storage | Multi-step operations are not atomic; `write_file_by_name` has a window where neither version is reachable | FIXED (290) |
| S9 | services_storage | Nothing is ever freed; deletion leaks, and the disk fills monotonically | FIXED (291) |
| S10 | hal/virtio_blk | `VIRTIO_BLK_F_FLUSH` is never negotiated and `UNSUPP` is treated as success, so nothing survives host power loss | FIXED (293) |
| X1 | boot config | The shipped image authenticates with the published constant `pandagen-dev` and accepts any caller name | FIXED (285) |
| X2 | remote_ipc | The replay window is a ring: 260 ordinary commands pushed a captured request out of it and it was accepted again, against the running kernel, with no key (`remote_replay`) | FIXED (292) |
| X4 | remote_ipc | The message-envelope path keeps the bounded window: a `MessageId` is a random UUID, so there is no order to compare against and a captured envelope still comes back into range | OPEN |
| X3 | kernel/remote | `boot` is on the remote allowlist and discloses kernel physical/virtual addresses and the HHDM offset | FIXED (285) |
| F9 | net_stack/tcp | `listen` silently ignored a third port (only two slots), so HTTP was never bound and every client got a reset. Found within seconds of pointing real `curl` at the machine | FIXED (283) |
| F8 | hal/virtio | `poll_receive` trusts the device's descriptor id and slot index; out-of-range values panic or read far past the DMA region | FIXED (282) |
| F10 | kernel/http + net_stack/tcp | An ordinary keep-alive client that hangs up leaves its slot in CloseWait for the 120 s idle timeout; eight slots serve every port, so a handful of plain `curl` requests reset all later ones and take the command port with them (`http_slots`) | FIXED (287) |

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

### Round 2

Storage and security critics reported. Everything they raised that reproduced
is fixed: S1, S2, S4, S7 (Phase 284), X1 and X3 (285), S5 (286).

S5 is worth recording as a method note. The critic's claim was true on the
kernel and **invisible on the host**, because `ObjectId::new()` is a real v4
UUID off the kernel and a restarting counter on it. No host test could fail.
It was confirmed instead by scanning the raw disk image for two names sharing
an id, and then by `cat` in QEMU. The guard that shipped with the fix asserts
the *mechanism* (a serial is never handed out twice across a remount) rather
than the symptom, because only the mechanism is observable where tests run.

F10 came out of the suite rather than a critic: `tcp_slots` failed only when
run after the other scripts, and passed alone. The ordering dependency was the
finding, not a flaw in the test. Worth remembering — **an order-dependent
failure in this suite is evidence, not noise.**

### Round 2, continued

S3, S6, S8, S9, S10, X2 all reproduced and are fixed. Three method notes
worth carrying forward:

**S3 and S6 were one bug from two ends.** S6's failed commit leaked its
blocks, which punched a hole in the free list, which is the only condition
under which S3's contiguity assumption breaks. Neither reproduces without
the other. When two findings in the same area both look hard to trigger,
try triggering them with each other.

**A unit test can pass while the machine fails.** X2's first fix used an
eighteen-minute staleness slack. It passed tier 1 and failed tier 2 in the
same minute: 260 requests take three seconds, so the captured line was
comfortably inside the slack. Tier 1 proved the code did what I wrote; only
the real kernel proved what I wrote was the wrong rule.

**Say when a test cannot be written.** S10's data loss is not reproducible
here — killing QEMU leaves the host page cache intact. The fix is verified
at the level of the *mechanism* (the feature is negotiated against QEMU's
real device, the FLUSH is sent, a device that lies is now an error) and the
commit says plainly that the loss itself was never demonstrated.

## Resume here

**Rounds 1 and 2 are closed.** Sixteen findings fixed across Phases 278-287.
The whole gauntlet suite (11 scripts) passes in one run, in any order, and
`cargo test --workspace` is clean.

**Every finding from rounds 1 and 2 is closed.** Phases 278-293.

**Open:** X4 only — the message-envelope replay path. It cannot be fixed the
way X2 was, because a `MessageId` is a random UUID and there is no order to
compare against. Fixing it means changing how envelope ids are minted, which
is a wider change than the line protocol was.

**Next: round 3.** Dispatch critics at the three areas no critic has seen:

1. The graphics and compositor path.
2. The boot path.
3. `services_*` above the kernel (workspace manager, editor, file picker,
   fs view) — by far the largest body of code here and entirely unexamined.

**The canonical verification, which must pass before any commit:**

```
cargo test --workspace
cargo xtask iso
cargo xtask qemu-script --keys "sleep:3,<every gauntlet:… script>,sleep:1" \
  --expect-serial "flush=negotiated"
```

**Stop condition:** three consecutive critic rounds with no confirmed
finding, or Armando returns.
