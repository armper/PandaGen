# Phase 284: The Filesystem Was Losing Most Of Itself On Every Reboot

Gauntlet round 2. A storage critic reviewed `services_storage` against what a
real filesystem guarantees, and built its own probe crate to confirm what it
found. Four of its findings are closed here, each reproduced first as a test
in `services_storage/tests/durability.rs` — the crate had no test that
mounted the same bytes twice, which is why none of this was visible.

## An empty file killed the machine

`commit` did `let first_block = blocks[0]` after allocating blocks for the
payload. A zero-length write allocates none, so this indexed an empty vector.
The kernel aborts on panic, so `create_file(name, b"")` was fatal, and any
boot-time code that wrote an empty file would have bricked the machine
permanently.

## A remount discarded most of the filesystem

Recovery scanned the commit log in **block** order and accepted a record only
when its sequence beat the highest seen so far. The log is a ring, so block
order and sequence order diverge the moment it wraps: the scan kept the few
records after the wrap point and threw away every older one, intact and
checksummed though they were.

Fixing the ordering was necessary but not sufficient, and the test said so:
55 of 80 files were still unreadable. The deeper problem is that the commit
ring was the *only* persistent metadata. Once a record is physically
overwritten by a later one, its object is unreachable however you scan.

Meanwhile the "allocation bitmap" region reserved at format time — 48 blocks
on a 512-block disk — was never written and never read. Dead space.

That region now holds a checkpoint: the whole allocation map, written
whenever the ring is half-consumed, with a length-and-checksum header written
*after* the payload so a torn checkpoint fails validation instead of
vouching for itself. A mount loads the checkpoint and then replays only the
records newer than it. All 80 files survive.

The checkpoint is taken after the in-memory map is updated, not before. Taken
before, it omitted the very commit that triggered it, which the test caught as
exactly one missing file out of eighty.

## A stale sequence destroyed an already-committed object

The superblock update that records a commit is a separate write from the
commit record. A crash between them left the sequence stale, so the next
commit reused it, mapped to the same ring slot, and overwrote a live record —
an object that had been committed and had already survived one reboot would
vanish at the next unrelated write. Recovery now reconciles the superblock's
sequence with what it actually found.

## A byte edit made the machine unbootable

`open` read `total_blocks` from block 0 and immediately built a free set over
that range with no check against the device. Patching it to 200 billion made
the mount allocate until it was killed; the test had to be SIGKILLed after 60
seconds. On the kernel that is an allocation failure and an abort, from a
disk that still passes the magic-number check, with no way back short of
wiping it.

The geometry is now validated against `device.block_count()` before anything
is sized from it, `commit_log_blocks` is bounded by what `format` would ever
choose, and a recovered allocation whose extent falls outside the device is
refused rather than looped over.

## Verification

Four new tests, each failing before and passing after:
`an_empty_file_is_ordinary_input`,
`every_committed_file_survives_a_remount_past_the_commit_log_wrap`,
`a_directory_listing_never_outlives_the_files_it_names`,
`a_hostile_superblock_cannot_hang_or_exhaust_the_mount`.

`cargo test --workspace` green, `services_storage` 65 unit plus 4 durability.
End to end: five files written, the machine rebooted on the same disk image,
all five read back with correct contents.

## Still open from this critic

Object ids come from a counter that restarts at 1 every boot, so two files
created on different boots can alias. Reads reconstruct a block list as a
contiguous range although allocation does not guarantee one. A failed commit
returns `Ok` on retry having written nothing. Multi-step operations are not
atomic, and `write_file_by_name` has a window where neither the old nor the
new content is reachable. Nothing is ever freed. And `flush` is a no-op,
because `VIRTIO_BLK_F_FLUSH` is never negotiated, so nothing survives host
power loss — which is the one finding on the list I cannot demonstrate with
the tools I have, since killing QEMU leaves the host page cache intact.
