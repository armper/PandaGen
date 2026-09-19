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
| E1 | services_editor_vi | `:e <missing>` kept the previous file's buffer *and* handle, so the next `:w` overwrote a file the user never opened | FIXED (296) |
| E2 | services_editor_vi | The undo stack survived loading a new file; one `u` pulled the previous file's text in and `:w` wrote it | FIXED (296) |
| E3 | editor_core + vi | Byte offsets used as character offsets: a cursor inside a multi-byte character panics, which aborts the kernel | FIXED (296) |
| E4 | kernel/minimal_editor | `:wq` quit whether or not the write happened; with no path everything typed disappeared silently | FIXED (299) |
| E5 | kernel/minimal_editor | `:w` with no filesystem said so and then marked the buffer clean, so the next `:q` discarded the work | FIXED (299) |
| E6 | services_workspace_manager | Closing or quitting dropped a dirty editor with no prompt; the editor's own `:q` guard was decorative | FIXED (300) |
| E7 | editor_core + vi | Keys that changed nothing pushed undo snapshots and evicted real history from a 100-entry stack | FIXED (299) |
| E8 | services_editor_vi | Every save stripped the file's trailing newline; opening and saving changed the file | FIXED (296) |
| E9 | fs_view | `ls` iterated a HashMap, so the order changed on every invocation | FIXED (300) |
| B1 | kernel/boot | `calibrate_lapic_timer` spun on a PIT tick with no bound; no 8254 meant no boot, no message, no recovery | FIXED (297) |
| B2 | kernel/boot | Reserved ranges rounded *inward*, exposing the kernel image's final partial page and dropping sub-page regions | FIXED (297) |
| B3 | kernel/boot | The heap demanded 32 MiB contiguous or nothing, then walked ten steps past a fatal condition into the allocator. Floor was 48 MiB, now 12 | FIXED (301) |
| B3b | kernel/boot | A *refused* `allocate_contiguous` left the cursor at the end, so the allocator was spent by a request it had declined | FIXED (301) |
| B4 | kernel/boot | 32-entry reserved table overflowed silently | FIXED (297) |
| B5 | kernel/boot | CPUs past the eighth ran with no GDT/TSS, so a double fault there triple-faults | FIXED (302) |
| B6 | kernel/ps2 | The keyboard IRQ handler did not check the AUX bit the mouse handler checks | FIXED (303) |
| B7 | hal/mouse | A keystroke during mouse bring-up was eaten as the ACK; no pointer for the rest of the boot | FIXED (303) |
| G1 | kernel/present | A present band waited 100M polls -- measured 4.6 s -- for microseconds of work; the desktop froze while the APs were busy | FIXED (298) |
| G2 | kernel/palette + picker | Clicking blank space below a list ran an unseen command, or opened a file | FIXED (298) |
| G3 | kernel/display | In text mode the palette was invisible while the editor was open and still swallowed every keystroke | FIXED (301) |
| G4 | kernel/palette | The selection could walk past the last drawn row; text mode drew 4 of 10 | FIXED (298) |
| G6 | kernel/present | The worker's generation guard was checked before the band was read, not after | FIXED (298) |
| G8 | kernel/desktop_frame | A framebuffer past the backend's limit `expect`ed, which aborts the kernel | FIXED (301) |
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

**Round 3's three critics have all reported** — graphics/compositor, the boot
path, and `services_*` above the kernel. Twenty-two findings confirmed and
fixed across Phases 296-303. X4 was fixed in 294.

**Still open from round 3, in the order to take them:**

1. **G5** — the editor writes straight to VRAM while every other text
   surface goes through the shadow framebuffer, so the 3 MiB shadow is
   reserved and unused whenever the editor is open and the two buffers
   diverge. Not currently harmful (nothing marks the pacer dirty in that
   state) but it is a trap for the next caller that does.
2. **B9** — PCI enumeration sees only bus 0, function 0: no multifunction
   check, no bridge recursion. A virtio device behind a PCIe root port, the
   normal topology on `-machine q35`, is invisible. Fails safe.
3. **G7** — the framebuffer assumes 32 bpp and a 4-byte-aligned pitch
   whatever the bootloader reports. Not reachable under QEMU + Limine.
4. Minor, all confirmed by the services critic: `undo`/`redo` set dirty
   unconditionally so undoing back to the on-disk text still blocks `:q`;
   `:wq` on a clean buffer exits without writing (`:x` semantics under the
   name `wq`); the workspace CLI's command history is uncapped while
   `cli_console`'s caps at 100; `services_editor_vi::render.rs` compares a
   char index against a byte column so the cursor is drawn wrong on a line
   with a multi-byte character; `services_file_picker` pushes a
   `DirectoryView` per descent with no depth cap, and nothing forbids a
   directory cycle (unconfirmed — the critic did not construct one).

### Round 3

Three critics, three areas none had seen, twenty-two confirmed findings.
Three method notes:

**A critic that says what it could *not* prove is worth more.** All three
separated confirmed defects from unconfirmed ones and from well-argued
negative results, and the negatives saved real time: the graphics critic
cleared the rasterizer's pixel addressing, the glyph tables and the pointer
clamping; the boot critic cleared the interrupt stub stack alignment, the
PIC EOI paths and the AP bring-up window. Those are areas a later round
should not re-visit.

**Reachability and severity are different questions.** E3 (a cursor inside a
multi-byte character panics, which aborts the kernel) had no route from the
booted machine — every input path is ASCII-only — and the critic said so
plainly rather than overstating it. It is still fixed: a latent kernel abort
is worth closing whatever today's reachability.

**One fix's first attempt was defeated by another defect.** B3's shrinking
heap reported "no contiguous run of 2048 KiB" on a machine with 23 MiB free.
The loop was right; `allocate_contiguous` left its cursor at the end of the
last range on failure, so the allocator was spent by a request it had
*refused*, and only the first size was ever really tried. When a fix
produces an impossible number, suspect the thing underneath it.

**The canonical verification, which must pass before any commit:**

```
cargo xtask gauntlet
```

It runs the workspace tests, builds the ISO, discovers every judge in
`gauntlet/` and runs them all through a booted kernel, and checks each step's
exit status. Judges are discovered, not listed, so a new one counts the
moment it is written, and the command refuses to pass if it finds none.

It now also boots a machine with 16 MiB, one with sixteen CPUs, and one with
no 8254, because each of those was a defect that only a differently-shaped
machine could show.

It exists because a verification run once went green against a **stale
image**: the kernel had stopped building and I read the log for "ISO ready"
instead of checking the exit status. It happened a second time one phase
later, with `cargo test --workspace` failing on `error: type ... is private`,
which matches neither "FAILED" nor "error[". **Check the exit status, never
the output.**

**Stop condition:** three consecutive critic rounds with no confirmed
finding, or Armando returns.
