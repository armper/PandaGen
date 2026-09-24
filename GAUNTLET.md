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

## The shapes

Five of round 7's eight service-crate findings were repeats of shapes this
loop had already fixed somewhere else. The critic that found them said the
useful thing plainly:

> The highest-yield thing I did was not reading for new bugs; it was
> grepping for the old ones by shape.

So the shapes are written down. **Every critic prompt should carry this
list, and every audit of an unread crate should start by grepping it**,
before reading anything for its own sake. Each line names the first place it
was found and at least one place it was found *again* afterwards.

Shape 0 is about the loop's own method rather than the code, and it is first
because it is the one that lets every other shape through.

0. **A test that fails for a different reason than the one it names.** The
   check "revert the fix, watch the test fail" is not enough on its own: the
   test may fail because the reverted code broke something *else* it
   touches. Round 8 produced five of these in three consecutive phases, all
   written by me: an executable-parse test that never reached the
   arithmetic because the image was rejected for a bad magic number; an
   audit-log test that recorded nothing because the task had no budget; a
   process-manager test whose `RestartPolicy::Never` meant no restart was
   ever attempted; a budget-cap test where leaving one field unlimited
   tripped a different rule. The discipline is: revert the fix, watch the
   test fail, **and read the failure message**. `assert!(x.is_err())` is not
   an assertion -- pin the specific error, and make each case vary only the
   one thing it names.

1. **"Cannot read it" treated as "it is empty."** A load that falls back to
   a default, in code that can later save to the same place. The fallback is
   safe for *running* and never for *writing back*.
   *R6-6 (storage checkpoint) → R6-9 (settings) → R6-10 (boot profile).*
   Grep: `unwrap_or_default`, `unwrap_or_else(|_|`, `.ok()?` near a load.

2. **A guard given to one call site.** The fix is right and lands on one of
   several siblings. **Nine times now.** *F6, R6-7, R6-8, R6-10, K1, K2, K3,
   K4, C1, C2.* Grep for the *pattern*, never the line; then list every
   caller of the function you just changed and every peer of the field.

3. **Unbounded history.** An audit log, trace log, completed-job list or
   notification history documented as "kept for history" and kept for ever.
   *R6-2 (focus manager) → R6-12 (scheduler) → N1 (developer_sdk) → N5 (job
   scheduler).* Grep: `Vec<` fields named `*_log`, `history`, `events`,
   `completed_*`, with no `MAX` nearby.

4. **A caller-controlled integer as a loop bound.** A `u32` or `usize` off
   the wire used to iterate against a small real resource.
   *R6-20 (draw_line) → R6-21 (rounded rects) → C2 (draw_window).*
   Grep: `for .. in 0..` where the bound is a struct field of a
   `Deserialize` type.

5. **Arithmetic that panics in debug and wraps in release.** The kernel
   ships optimized, so a wrap is silent and a wrapped deadline is in the
   past. *R6-14 (Instant/Duration) → N8 (total_size_bytes).*
   Grep: bare `+` and `*` in files that mention `Deadline`, `Instant`,
   `timeout` or `size`.

6. **One dead consumer aborts a fan-out and is never removed.**
   `for x in &mut sinks { x.send(..)? }`. *C4 (remote UI) → N1
   (developer_sdk).* Grep: `?` inside a `for` over subscribers.

7. **Code that documents a property it does not enforce.** A doc claiming
   validation, a config knob nothing reads, a "contract test" that tests
   itself, a feature list naming something that does not exist.
   *R6-13 (SyscallGate), R6-16 (max_steps_per_tick), R6-17
   (formal_verification), R6-18 (contract_tests), N5 (dependencies), N8
   (core_types::memory).* Grep the doc comments, then grep for a caller.

8. **A test that passes with the thing it tests removed.** Including the
   tautology: an assertion that restates the function body.
   *F8, G5, N6 (service ids).* Check by reverting, every time.

9. **Per-slot state not re-qualified against what is in the slot now.** An
   index into a table of reusable slots, held across anything.
   *R6-4 (http_owed) → K2 (http_streams and ReplyTarget).*
   Grep: arrays indexed by a connection or slot number.

10. **Two notions of one thing, updated in two places.** They agree on the
    common path and diverge on every other.
    *R6-24 (focus ring vs key delivery) → C1 (the same, on launch) → N4
    (revoked vs unsubscribed).* Ask what keeps them in step, and grep for
    every writer of each.

11. **A refusal that is also a loss.** Work consumed before the decision to
    refuse it. *N2 (receive_packet pops before the policy runs).*
12. **A validator that covers one field of a record of several**, where the
    uncovered fields are the ones that matter. Distinct from shape 2: the
    guard is not missing from a sibling *call site*, it is missing from
    sibling *fields of the same struct at the one call site that has it* --
    which makes it invisible to "grep for every caller".
    *A1 (`ResourceBudget` in `spawn_task`) → 7a (`BudgetCapPolicy`, one of
    six) → 5a (the additive budget check, in two of five budget paths).*
    Grep: find the struct, list its fields, check the validator names each
    one. `None`-means-unlimited fields are the ones it forgets first.


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
| R1 | services_storage | **My own regression.** A new `#[serde]` field changed the checksum of every commit record an older build wrote, so recovery discarded all of them: everything since the last checkpoint lost, silently, on upgrade | FIXED (307) |
| R2 | services_storage | A commit reported as *failed* was durable anyway and its blocks were reused, so it came back after reboot holding another object's data | FIXED (308) |
| R3 | services_storage | S9 half-fixed: superseded versions of a live object were never freed; 435 rewrites of a one-block directory filled a 512-block disk | FIXED (309) |
| R4 | services_storage | Extents made a save fail with an opaque error on a fragmented disk that was 85% empty | FIXED (309) |
| R5 | remote_ipc | The replay floor was shared, so clock skew or one caller's far-future nonces locked every other caller out for the boot | FIXED (310) |
| R6 | services_editor_vi | Phase 304 fixed undo-dirty in `editor_core` and left it in the editor the workspace hosts, so a window became unclosable with nothing to save | FIXED (310) |
| R7 | services_storage | A failed save kept the whole file in the heap for ever; forty failed megabyte saves held forty megabytes | FIXED (308) |
| R8 | kernel/present | Phase 298's generation guard was still check-then-act; re-checking a value you act on outside the lock narrows a window, it does not remove one | FIXED (315) |
| H1 | kernel/http | HEAD returned a body, so every conformant keep-alive client desynchronised | FIXED (311) |
| H2 | kernel/http | A second request was served *into the middle* of a streaming body, and clobbered the stream state | FIXED (311) |
| H3 | net_stack/http | Request smuggling: `Content-Length` was not parsed and the body became the next request; `Transfer-Encoding` was ignored | FIXED (311) |
| H4 | kernel/http | No deadline on an incomplete head: six dribbling sockets took the whole machine off the network, unauthenticated | FIXED (311) |
| H5 | net_stack/http | `MAX_HEAD_BYTES == tcp::BUFFER_BYTES` exactly, with nothing asserting it; raising either makes the 431 path unreachable and wedges the connection | FIXED (316) |
| H6 | net_stack/http | A stray blank line before the request line was answered 400 | FIXED (311) |
| D1 | net_stack/dhcp | A one-second lease spun the kernel twice a second with the network lock held; nothing validated the offered address or netmask; a request arriving in that window was lost for ever | FIXED (313) |
| N1 | net_stack | An unresolved next hop consumed the segment it could not send, spent retransmissions on nothing, and never asked for the address | FIXED (312) |
| N2 | net_stack | The ARP cache was poisoned by data-plane source addresses, and believed unsolicited replies | FIXED (312) |
| N3 | net_stack | The machine answered broadcast echoes to a forged source: a one-packet reflector | FIXED (312) |
| N4 | net_stack/wire | IPv4 fragments were processed as whole datagrams | FIXED (312) |
| P1 | xtask | `remote:` steps written with no expectation asserted nothing — `reply.contains("")` is always true — and the canonical verification's only remote assertion was one of them | FIXED (307/315) |
| P2 | xtask | Screendumps were never cleared and monitor errors discarded, so a refused screendump left the previous run's image and the run passed | FIXED (307/315) |
| P3 | xtask | The gauntlet never started from a clean disk | FIXED (307/315) |
| P4 | xtask | `--port-base` collided with adjacent bases, defeating its whole purpose | FIXED (307/315) |
| P5 | xtask | `boot/limine.cfg` was staged verbatim with no remote token, so X1's fix rested on a Limine name precedence nothing pins | FIXED (307/315) |
| C1 | distributed_storage | The leader demoted itself on every replication: a cluster committed exactly one entry, ever | FIXED (314) |
| C2 | distributed_storage | `merge` duplicated on every re-sync and `compact` was non-deterministic, so two nodes with identical inputs disagreed about an object's contents | FIXED (314) |
| C3 | distributed_storage | A `LogEntry` with index 0 off the wire panicked, or silently desynchronised every later index | FIXED (314) |
| C4 | services_remote_ui_host | One dead viewer aborted the fan-out and was never removed, so the whole remote UI went dark for everyone, permanently | FIXED (314) |
| C5 | pandagend | With no arguments it had no reachable exit: a spinning core, silently | FIXED (314) |
| C6 | pandagend | The HAL input context was lost on any error, killing the keyboard for the life of the process — in code `cargo test --workspace` never compiled | FIXED (316) |
| C7 | services_command_palette | The capability gate was declared, documented and read by nothing | FIXED (314) |
| C8 | cli_console | Filesystem timestamps came from an unsynchronised `static mut` counter restarting at 1001 every run | FIXED (314) |
| A1 | resources | `is_subset_of` skipped any field the child left unlimited, so a task under a hundred-tick parent could declare itself unmetered and run unmetered | FIXED (321*) |
| A2 | sim_kernel + workspace | The budget-inheritance guard was unreachable dead code, and the workspace took a budget straight from the package manifest unchecked | FIXED (321*) |
| A3 | services_workspace_manager | `PolicyDecision::Require` was silently treated as Allow, handing a sandboxed component keyboard focus across a trust boundary | FIXED (321*) |
| A4 | secure_boot | Verifies no signature against no root of trust; an empty policy returned Ok for a fully tampered log; nothing in the workspace calls it | PARTLY FIXED (322) |
| A5 | package_registry + app store | The source digest it exists to pin was never compared, and the storefront planned installs with an empty one | FIXED (321) |
| A6 | sim_kernel | Spawn-time capabilities were plumbed three layers deep and granted by nothing | FIXED (323) |
| A7 | core_types | `Cap<T>`'s phantom type is erased by serde and `Cap::new` is public, while the header claimed unforgeability | FIXED (323) |
| A8 | workspace_access | Delegation required admin; *becoming* admin checked nothing, and nothing could be revoked | FIXED (322) |
| A9 | packages | `format_version` was read by nothing; a manifest could overwrite its own derived entry | FIXED (321) |
| F1 | services_storage | **Phase 307's own fix**: compatible with pre-288 disks and broke every disk written by Phases 291-306 | FIXED (317) |
| F2 | services_storage | `release_object` threw away `landed`: a delete reported as failed happened anyway at the next mount | FIXED (317) |
| F3 | net_stack + kernel | Phase 312's ARP request was built, counted and never transmitted; and one unresolvable peer held up every other connection | FIXED (319) |
| F4 | kernel/http | Phase 311's body drain stopped at the buffer, so the smuggle survived a split segment -- i.e. every body above 2 KiB | FIXED (320) |
| F5 | kernel/http | The head deadline was never cleared, so a new client inherited an expired one and was reset on sight | FIXED (320) |
| F6 | kernel/tcp | The head deadline was given to the HTTP port only; the signed command port kept the identical slowloris | FIXED (320) |
| F7 | services_editor_vi | `:w` with no I/O reported "Saved version N" having written nothing | FIXED (321) |
| F8 | several | Tests that pass with the defect restored: xcompat skipped silently, the ARP test asserted a counter, the index-zero test asserted nothing | FIXED (317, 319, 321) |
| SC1 | kernel/smp | `smp run` on a two-CPU machine waited on itself: the console answered nothing for twenty seconds | FIXED (318) |
| SC2 | hal/work_queue | `complete` was check-then-act, so a result could be published into a recycled slot and accepted by the wrong job | FIXED (318) |
| SC3 | kernel/net | `net ping` held the whole stack for seconds with no TCP timer advancing | FIXED (324) |
| SC4 | kernel/present | The worker drain is bounded, so R8's window is narrowed rather than removed | OPEN (deliberate trade) |
| SC5 | kernel/storage | The virtqueue and DMA area are `static mut`, safe only by an unstated single-CPU confinement | FIXED (325) |
| SC6 | kernel | The keyboard queue raced on `read_pos`; the keyboard IRQ's debug path would self-deadlock on SERIAL_LOCK | FIXED (325) |
| P6 | workspace | `workspace_access` and `console_vga` did not build alone -- they passed only through workspace feature unification | FIXED (322) |
| X4 | remote_ipc | The message-envelope path keeps the bounded window: a `MessageId` is a random UUID, so there is no order to compare against and a captured envelope still comes back into range | OPEN |
| X3 | kernel/remote | `boot` is on the remote allowlist and discloses kernel physical/virtual addresses and the HHDM offset | FIXED (285) |
| F9 | net_stack/tcp | `listen` silently ignored a third port (only two slots), so HTTP was never bound and every client got a reset. Found within seconds of pointing real `curl` at the machine | FIXED (283) |
| F8 | hal/virtio | `poll_receive` trusts the device's descriptor id and slot index; out-of-range values panic or read far past the DMA region | FIXED (282) |
| F10 | kernel/http + net_stack/tcp | An ordinary keep-alive client that hangs up leaves its slot in CloseWait for the 120 s idle timeout; eight slots serve every port, so a handful of plain `curl` requests reset all later ones and take the command port with them (`http_slots`) | FIXED (287) |
| R6-1 | services_focus_manager | `request_focus` pushed a subscriber that was already on the stack, so one client could hold several entries and a pop gave focus back to itself | FIXED (326) |
| R6-2 | services_focus_manager | The audit trail was unbounded: a client that takes and drops focus in a loop grows it without limit | FIXED (326) |
| R6-3 | net_stack | With no peer needing anything, the poll still emitted an ARP request every tick -- an idle machine flooded the segment | FIXED (326) |
| R6-4 | kernel/http | `http_owed` survived into the next connection to reuse the slot, so a new client was answered with the previous client's owed body | FIXED (326) |
| R6-5 | kernel/http | An HTTP connection whose peer had gone was never swept, so it sat in CloseWait until the idle timeout -- the same slot exhaustion as F10, by a different door | FIXED (326) |
| R6-6 | services_storage | An unreadable checkpoint was treated as absent: the mount came up on a stale tail and silently lost everything after it | FIXED (326) |
| R6-7 | kernel/storage | Only two of the six storage entry points asserted the boot CPU, so the single-CPU confinement SC5 documented was enforced on a third of its doors | FIXED (326) |
| R6-8 | net_stack | Three of the four `NeedArp` transmit sites left the pending frame set, so the retry re-sent a frame the caller had already abandoned | FIXED (326) |
| R6-9 | services_settings + workspace_manager | A settings file this build cannot parse -- including one written by another format version -- loaded as empty defaults, and the next save wrote that emptiness over it. Every user setting destroyed by booting the wrong build once | FIXED (327) |
| R6-10 | workspace_manager/boot_profile | The same defect for the stored boot profile, found by grepping for R6-9's siblings | FIXED (327) |
| G5 | harness | Two assertions that passed with their fix reverted: the `--smp 2` case asserted only a banner printed before the command ran, and the work-queue test asserted an id check that predates the fix | FIXED (326) |
| G6 | harness | Two gauntlet runs at once silently corrupt each other -- same ISO, same disk, same ports -- and the loser failed with a bare `NotFound` naming none of it. A verifier that can produce a wrong answer is worse than none | FIXED (326) |
| R6-11 | sim_kernel/scheduler | A late periodic task was caught up one period at a time, one audit event each: a task with `period_ticks == 1` a million ticks behind spent tens of ms and a million allocations, long enough to fall further behind | FIXED (328) |
| R6-12 | sim_kernel/scheduler | The audit log had no bound at all -- one entry per task selection, for as long as the machine stays up | FIXED (328) |
| R6-13 | sim_kernel/syscall_gate | The gate took `caller` on every syscall and only wrote it down. An identity the kernel had cancelled kept making syscalls, each recorded `Invoked` then `Completed`. `default_trap` carried the comment "All syscalls must go through the gate" directly above a direct call into the kernel | FIXED (328) |
| R6-14 | kernel_api/time | `Instant + Duration` and `Duration + Duration` were unchecked: wrapping in the optimized build the kernel ships, so a deadline could land before `now`. `Duration::from_secs(u64::MAX)` wrapped to a few hundred ms. Subtraction already saturated, which hid the asymmetry | FIXED (329) |
| R6-15 | services_notification | Expiry *deleted*: a toast's TTL removed it from the history that `get_recent_notifications` reads, so an error not read within six seconds left no trace | FIXED (329) |
| R6-16 | sim_kernel/scheduler | `max_steps_per_tick` was documented as a guard against infinite loops, defaulted to `Some(1000)`, was set at ten call sites and read by nothing | FIXED (329) |
| R6-17 | formal_verification | `src/lib.rs` -- nine `verify_*` functions and a report type -- was called by nothing, including this crate's own tests. The real model checking is in `tests/` and is much stronger | FIXED (330) |
| R6-18 | contract_tests | "Golden" contract tests that depend on none of the services they name: every payload type is declared in the crate itself and round-tripped against itself, and the action strings appear nowhere else in the tree. Cannot fail when a service changes | DOCUMENTED (330) -- not fixable until a service dispatches on them |
| R6-19 | services_pipeline_executor | No pipeline can complete a single stage against the real kernel: a channel is one queue, so `execute_stage_once` sends its request and receives that same request back. Every passing test in the crate passes because its mock fabricates the reply | OPEN -- reproduced in `tests/against_the_real_kernel.rs`; the error is now truthful, but closing it needs a service-side dispatch loop |
| R6-20 | graphics_rasterizer | `draw_line` walked the whole segment, testing each point for being in bounds. `DrawOp::Line` carries `i32`, so one draw op from one view was 4_294_967_296 iterations -- measured at 7.9 s in an optimized build with the compositor frozen throughout | FIXED (331) |
| R6-21 | graphics_rasterizer | `fill_rounded_rect` and `draw_rounded_border` looped `rect.height` times whatever the target's height was, and `DrawOp` carries height as a `u32` | FIXED (331) |
| R6-22 | services_gui_host | A keyframe carried the producer's "what changed since the last frame" as its damage. A viewer honouring it repaints one rectangle of an otherwise blank surface | FIXED (331) |
| R6-23 | services_remote_ui_host | `add_sink` did not force a keyframe, so a viewer that joined mid-stream decoded deltas against a scene it had never seen -- for up to `DEFAULT_KEYFRAME_INTERVAL` frames. `request_keyframe`'s own doc says "(a viewer joined)" and nothing called it there | FIXED (331) |
| R6-24 | services_workspace_manager | Two notions of focus: `window_layout.focused_tile` draws the ring, `focus_manager` delivers keys. Closing the focused component moved each independently and they landed on different components -- the user saw a highlighted window and typed into another | FIXED (332) |
| R6-25 | services_gui_host | A scene with a repeated view id cannot be expressed as a delta (`changed` updates the first match, `removed` drops every match). Encoding one anyway produced a *different* scene at the viewer -- the update was lost and one window took another's content -- and every later delta built on that | FIXED (332) |
| K1 | hal_x86_64/virtio | `poll_receive` validated the device-supplied descriptor id; `transmit` passed the same untrusted id from the same used ring straight to `free_desc`, which indexes the table. Reverting the guard gives `index out of bounds: the len is 8 but the index is 9999` -- a panic, i.e. a dead machine. Latent: needs a hostile or buggy device, which QEMU will not produce | FIXED (333) |
| K2 | kernel/http + remote | `progress_matches_connection`'s doc says "Anything indexed by TCP slot has to ask this" and two siblings did not. An aborted download handed its remaining body to whoever landed in its slot next, whose own request was then never answered and whose connection was closed when the stranger's stream ended. And `ReplyTarget::Tcp { conn }` held a bare slot index across a command's execution, so a **signed privileged command's output could be written to an unauthenticated stranger** on any port | FIXED (333) |
| K3 | kernel/http | `tcp::write` returns how many bytes fitted and every HTTP responder ignored it. Three pipelined `GET /` overflow the 2048-byte send buffer and the third response's own status line is cut in half; the client hangs until the 120 s reaper. `tcp_reply` on the command port checked its write; its siblings three functions away did not | FIXED (333) |
| K4 | kernel/net | SC3's `keep_tcp_alive` reached three of the four spin loops; `ping`'s ARP wait is the fourth | FIXED (333) |
| C1 | services_workspace_manager | **R6-24's own fix was incomplete.** `reconcile_focus` was wired only to the closing path. Launching a non-focusable component, or one whose focus the policy denies (the error is swallowed by `let _ =`), leaves the ring on the new window and the keys on the old one | FIXED (333) |
| C2 | services_gui_host | **R6-21's own fix was incomplete.** The row clamp went into `graphics_rasterizer`'s rounded shapes and not into `draw_window`, the text compositor next door. `SurfaceRect` is four `usize`s off the wire: 200M rows on a 24-row canvas measured 1.57 s, and `usize::MAX` never finishes | FIXED (333) |
| N1 | developer_sdk | C4 exactly, in a crate C4's fix never touched: `for sink in &mut self.sinks { sink.send(..)? }` aborted the fan-out and never removed the failed sink, so no sink after it received another event for the life of the process. Whether you got tracing depended on registration order. `TraceLog` was also unbounded, in the crate whose job is to record continuously | FIXED (334) |
| N2 | services_network | `receive_packet` popped the packet *before* evaluating the policy and charging the budget, so a refusal returned `Err` with the packet already gone -- undeliverable for ever. `send_packet` three functions up has the order right | FIXED (334) |
| N3 | services_network | `NetworkInterfaceId` is a caller-supplied parameter with no registration, so any id created a queue, and neither the queue depth nor the number of queues had a ceiling. 100 000 invented ids all accepted, all retained, every packet answered `Ok(())` and going nowhere | FIXED (334) |
| N4 | services_input | "Revoked" and "unsubscribed" were two states and the re-subscribe path knew only one: a revoked task got `SubscriptionAlreadyExists` for the life of the service, and the revoked record was never reclaimed | FIXED (334) |
| N5 | services_job_scheduler | Three: one job returning `Yielded` starved every higher-priority job for ever and `cancel_job` searched only `pending_jobs`, so it could not be stopped; `completed_jobs` was unbounded; `set_progress` had no route from a job to its own descriptor, so "progress tracking" was a listed feature with no API. The feature list also promised dependencies, which do not exist | FIXED (334) |
| N6 | core_types | `test_core_service_ids_stable` restated the function body. Setting `CONSOLE_SERVICE_ID` to `0xdead_beef...` left the crate's 54 tests green -- a tautology in the shape of a guarantee, over the four constants that are the wire identity of the core services | FIXED (334) |
| N7 | console_fb | `Line::from_text` truncated by bytes and `as_str` returns `""` for invalid UTF-8, so a line whose right margin fell inside a multi-byte character displayed as **nothing at all** -- the whole line lost, not the character | FIXED (334) |
| N8 | core_types::memory | 675 lines of memory-authority model with no references outside the file, whose `add_region` doc promises an error it cannot return and whose type doc defers enforcement to a kernel that does not enforce it | DOCUMENTED (334) |
| V9 | services_view_host | The header said "Publishing and subscribing require capabilities"; reading took a bare `ViewId` and `ViewSubscriptionCap` was minted, stored and read by nothing at all | FIXED (340) |
| V10 | services_view_host | Subscriptions were unbounded, undeduplicated and impossible to end -- there was no `unsubscribe`. And `list_views` iterated a `HashMap` (E9's shape) | FIXED (340) |
| V11 | services_storage | `JournaledStorage` -- the backend `pandagend` hands the editor -- kept every version of every object and every journal entry for ever. 200 saves of a 64 KiB file retained 13 MB of journal and 13 MB again in `objects`, on a kernel whose heap floor is 12 MiB. R3's shape, in the sibling backend R3's fix never touched | FIXED (340) |
| V12 | services_storage | `Capability` is documented "unforgeable" and `Capability::new` is `pub`, while `check_access` compared only the fields of the struct it was handed: anyone could mint themselves `Own` on another principal's object. A7's defect, in the module A7's fix never touched | FIXED (340) |
| S8-1 | services_fs_view | `test_cannot_traverse_through_blob` passed with its guard deleted -- the blob's id is absent from `directories` so the next lookup answers `NotFound`, which `assert!(is_err())` accepts. Shape 0 | FIXED (341) |
| S11b | services_input_hal_bridge | `poll_packet` consumes the packet and `translate` advances the button state, then a `?` mid-batch dropped every later event for ever: a release for a button never seen pressed, or a stuck drag nothing retries | FIXED (341) |
| D1 | kernel_bootstrap::desk | The kernel had two key entry points into the desk -- `handle_key` when a card had focus and `handle_shell_key` when none did -- and the palette's Ctrl+Space was added to one. The first Ctrl+Space of every session, before any card had focus, went to the invisible console, and everything typed after it followed. Shape 2, found by a screendump that showed `WS > term` in the text console. One entry point now; the gauntlet types on the bare desk | FIXED (345) |
| H1 | xtask gauntlet | The gauntlet's private disk was seeded by copying `dist/pandagen.disk` -- the disk a person uses with `cargo xtask qemu`. The first time a person kept a theme there (Daylight, from the Look card), every desk shape booted into it and five pixels failed for a change that had not touched them. Test state came from outside the test. The gauntlet now creates a blank image the kernel formats on first mount | FIXED (355) |
| H2 | kernel_bootstrap (size) | Building the wallpaper in as packed RGB added 3 MB to the kernel (11 MB) and the 16 MiB machine stopped booting: no `PandaGen Workspace` on serial at all. Every host test passed; the "boot on a small machine" shape is the only thing on this project that sees binary size. Indexed colour (a palette and one byte a pixel) put the kernel at 9.1 MB and the small machine back | FIXED (358) |
| H3 | kernel_bootstrap (size) | Six apps later the kernel ELF was 9.9 MB and the 16 MiB machine stopped booting again: `usable=2316 KiB`, no room for a 2 MiB heap. 6.9 MB of the file was DWARF line tables (`debug = "line-tables-only"`), which the bootloader reads into memory with the rest of the file though nothing on the machine ever uses them. `strip = "debuginfo"` in the kernel profile: 3.1 MB, `usable=9012 KiB`, an 8 MiB heap on the small machine | FIXED (372) |
| D2 | xtask gauntlet | The ping shape typed `net ping 10.99.99.99` blind at boot and asserted nothing about it; when the desk began reading bare-desk typing as a palette search the ping stopped running and the shape stayed green. Shape 0 in the harness itself. It requires the console's echo now, and both blind-typing shapes open a Terminal card first | FIXED (345) |

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

**Round 3 is closed.** Twenty-eight findings, Phases 296-306. Nothing from
it is open.

One thing deliberately left alone: `:wq` on a clean buffer exits without
writing, which is `:x` semantics under the name `wq`. The critic called it
harmless in isolation and it is; changing it only adds a disk write to every
`:wq`. Recorded so a later round does not re-raise it.

**Round 5 is closed.** Twenty-six findings, Phases 317-325. Its three
critics were the authorization crates (`identity`, `secure_boot`, `policy`,
`workspace_access`, `package_registry`, capabilities), a second regression
pass over rounds 3-4, and SMP concurrency.

Phases marked `321*` were fixed in the same commit as Phase 321's other
work; see that commit for which.

**Open, deliberately:**

- **SC4** — the present worker drain is bounded, so a worker descheduled
  longer than the bound can still be inside `convert_rgba_rows` when the
  boot CPU moves on. The trade (a hung CPU must not stop the display for
  ever) is defensible but it is a trade, not a proof. Closing it properly
  means a one-deep retirement list for the pixel buffer.
- **A4** — `secure_boot` now refuses an empty or incomplete policy, but
  there is still no signature and no root of trust, and nothing in the
  workspace calls it: the shipped image has no boot chain. Real verified
  boot needs a key and a signing step in the build.
- The kernel does not check *who* is granting a capability; any holder of a
  `&mut SimulatedKernel` may grant.

**Round 4 was closed with** thirty-one findings, Phases 307-316. Its three
critics were the regression critic on this loop's own work, the HTTP and
non-TCP network critic, and the host crates nobody had read.

**The regression critic was the single most valuable one so far** and should
be repeated every few rounds. Eight findings, and its first was the worst
defect this loop has produced: a field I added changed the checksum of every
commit record written by an older build, so upgrading silently discarded
everything committed since the last checkpoint.

**Known and deliberately left:** `text_renderer_host::render_incremental`
leaks cache entries for lines a shrinking buffer removed, so a deleted last
line is never redrawn away — but it is called only from a perf demo binary,
so the blast radius is zero. `core_types` identity types are sequential and
predictable on the kernel (the documented no-`std` fallback); nothing should
treat one as unguessable.

**Round 6 is closed.** Twenty-seven findings, Phases 326-332.

**Round 8 is closed.** Twenty-four findings, Phases 335-341. Three
critics: the regression critic on phases 333-334, a **shapes sweep** that ran
the eleven known patterns across the whole tree rather than reading for
novelty, and a critic on the crates a user's data passes through.

The sweep is the format to repeat. It confirmed shape 1 is now genuinely
absent from the tree, verified that eight past shape-2 fixes are complete
today, and still found the round's most serious defect -- a CPU budget guard
defeated by asking for more than it could add.

**Round 7 is closed.** Fourteen findings, Phases 333-334 (K1-K4, C1, C2,
N1-N8). Three critics: the regression critic on this loop's own phases
326-332, the crates no previous critic had read, and the bare-metal kernel.

The round's most useful output was not a finding at all. Five of the eight
service-crate findings were repeats of shapes this loop had already fixed
elsewhere, so **the shapes are now written down** in their own section above,
and every critic prompt carries them.

**Still open from round 6's critics, not yet fixed:**

- **R6-19** -- the pipeline executor cannot complete a stage against the
  real kernel. Reproduced and committed as a passing test that asserts the
  *failure*; the error now says what happened instead of blaming a handler
  that does not exist. Closing it properly needs a service-side dispatch
  loop, which is a feature, not a fix.
- **R6-18** -- `contract_tests` cannot detect drift. Its docs now say so in
  the first line. It becomes a real drift detector the moment a service
  depends on those types and dispatches on those action strings.
- Compositor: **all six are closed** (R6-20 .. R6-25). Note for a later
  round: the *unstable* half of the view-id finding is real but was not
  judged a defect. Restoring a workspace from a snapshot mints fresh view
  ids and remaps frames onto them, so every window looks new to a remote
  viewer. Nothing today keys per-view state on the id across a restore, so
  there is no consequence to point at -- but anything that starts to will
  find this waiting.

**Earlier candidates, still unscoped:** a third regression pass (it
has been the most valuable role every time); the compositor and
`services_gui_host` internals, which round 3 examined only partly; and the
crates round 5's security critic did not reach -- `intent_router`,
`pipeline`, `services_job_scheduler`, `services_registry`,
`services_device_manager`, `services_settings`, `services_notification`,
`services_logger`, `services_input`, `services_focus_manager`,
`services_view_host`, `services_network`, `developer_sdk`,
`formal_verification`, `contract_tests`, `view_types`, `console_fb`.

**Areas a critic has cleared, which a later round should not re-read:** the
rasterizer's pixel addressing, the glyph tables and cache, pointer clamping
and the PS/2 packet parser, `services_gui_host::layout`, the interrupt stub
stack alignment, the PIC EOI paths, the AP bring-up window, LAPIC
calibration arithmetic, virtqueue sizing, `mmio_map`, `fs_view::PathResolver`
(no root escape), and `cli_console`'s line editing.

### Round 6

Findings in two halves: the critics' (R6-1 .. R6-10) and the harness's own
(G5, G6). Three method notes:

**A guard given to one call site is half a fix -- for the seventh time.**
R6-7 and R6-8 are both this: SC5's single-CPU confinement was asserted on
two of six storage doors, and the pending-frame clear was at one of four
`NeedArp` sites. R6-10 was found only because R6-9 prompted a grep for
siblings, and it was sitting in the next file. **Grep for the pattern, not
the line, before calling a fix done.**

**"Cannot read it" and "it is empty" are not the same answer, and the
difference is the user's data.** R6-6 and R6-9 are the same shape in two
unrelated crates: a load that could not parse its bytes fell back to a
default, and the next save wrote the default back. The storage checkpoint
lost every commit since the last one it could read; the settings file lost
every override the user had, from booting an older build once. A fallback is
safe for *running* and never safe for *writing back*. Anything that loads
with `unwrap_or_default` and can later save to the same place is this bug.

**The harness produced a failure it could not explain, and that is a
finding.** A run died with `Os { code: 2, kind: NotFound }` and nothing
else. The cause was three gauntlets running at once over one disk image --
my own doing, not the kernel's -- but an unexplainable failure from the
verifier is exactly how a false finding gets recorded as real. Both halves
are now fixed: a run refuses to start while another holds the lock, and the
one call that could fail that way names what it was trying to start.

### Round 5

Three critics, twenty-six findings. Four method notes:

**The regression critic is the highest-yield role, and it stays that way.**
A second pass over my own work found eight more, including F1: Phase 307's
*fix* for silent upgrade loss reintroduced the identical loss for the twenty
phases in between. The general fix -- checksum the bytes as read, never a
re-serialisation -- is the one I should have written the first time.

**Check that a test fails without the fix, every time, without exception.**
Round 5 found four that did not: one skipped silently on any machine but
mine, one asserted a counter incremented at frame-build time rather than the
frame reaching the wire, one asserted nothing at all, and one of my own
head-of-line tests passed either way until I re-aimed it. Every fix in this
round was checked by reverting.

**A guard given to one port, one editor or one call site is half a fix.**
This loop has now made that mistake five times: undo-dirty, the save-failure
guard, the slowloris deadline, `landed`, and the ARP transmit. When fixing
something, grep for its siblings before committing.

**`--workspace` can hide a crate that does not build.** Cargo unifies
features across a workspace build, so two crates compiled only because their
neighbours turned features on. And my first fix for that -- declaring the
feature -- unified into the no_std kernel and broke it with 5829 errors. The
ISO step caught it. Verification now checks all 58 crates one at a time.

### Round 4

Three critics, thirty-one findings. Four method notes, all of them about
how the loop itself goes wrong:

**Point a critic at your own fixes.** Round 4's regression critic read
Phases 278-306 as a body of work and found eight defects the individual
verifications could not see, because each fix was checked against its own
reproduction and nothing checked them against each other. R1 (a serde field
invalidating every older commit record) and R2 (Phase 289's rollback undoing
a commit that was already durable) were both *introduced* by fixes for other
defects. Repeat this every few rounds.

**A test that passes is not a test that works.** Several round-3 fixes
shipped with tests I had not checked by reverting the fix. R3's existing
test passed with the defect fully restored, because it exercised only the
half of the fix that worked. Revert and re-run, every time.

**The verification had holes that made it agree with me.** A `remote:` step
with no expectation asserted nothing; a refused screendump left the previous
run's image and still passed; the disk was never clean; a non-default
feature was never compiled. Four separate ways for the canonical check to
report success it had not established. The harness deserves a critic of its
own, regularly.

**Say what you could not prove, and then act on the reasoning.** D1's DHCP
freeze needs a server the harness cannot provide; R8's race is a preemption
between two instructions. Both are fixed, both commits say plainly that the
fix rests on reasoning rather than a reproduction, and R8's says so *because
the previous commit claimed a race was closed when it was not*.

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

It also boots into the desk (GFX-050), opens a Notepad from the keyboard,
types into it, and asserts pixels with `--expect-pixel x,y,r,g,b[,tol]`
against the final screendump. Screendumps were verified by eye before that,
which is to say not by the gauntlet.

It exists because a verification run once went green against a **stale
image**: the kernel had stopped building and I read the log for "ISO ready"
instead of checking the exit status. It happened a second time one phase
later, with `cargo test --workspace` failing on `error: type ... is private`,
which matches neither "FAILED" nor "error[". **Check the exit status, never
the output.**

**Stop condition:** three consecutive critic rounds with no confirmed
finding, or Armando returns.
