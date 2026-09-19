# Phase 282: Aliasing, Interleaved Output, And A Trusted Device

Closes the last three findings from gauntlet round 1. All three came from the
SMP and packet-parser critics; none is reachable from the network, so each is
verified by inspection or at tier 1 rather than end to end.

## Aliasing undefined behaviour

`workspace_loop` held `&mut Kernel` for the life of the machine while every
application processor simultaneously held `&Kernel` derived from the same
storage. A live unique reference aliased by shared ones is undefined
behaviour whatever the interior locks do: the compiler is entitled to assume
nothing else writes through that pointer and to cache the non-atomic fields
across another CPU's stores. Nothing wrote those fields after boot, so it was
latent rather than firing, and it would have started corrupting silently the
first time anyone added a `&mut self` method.

`create_channel` now takes `&self` and reserves its index with a
`fetch_update` on an atomic `channel_count`. The kernel reference is narrowed
to `&Kernel` immediately after initialisation, and both long-running loops
take a shared reference.

## Interleaved serial output

`SERIAL_LOCK` was taken per `write_str`. A `writeln!` lowers to several of
those, so one CPU's line could land inside another's, and `write_byte` (the
terminal echo) skipped the lock entirely. Now `write_fmt` is overridden to
take the lock once and format through a borrowing shim that does not retake
it, and `write_byte` locks around a single raw byte. The fatal exception
handler still uses the unlocked writer, so a CPU that faults while holding
the lock can still report; verified by `fault pf` after the change.

## A device that lies

`poll_receive` took the descriptor id straight out of the device-writable
used ring and indexed the descriptor table with it, then took a buffer slot
out of that descriptor and used it to offset into the DMA region. An
out-of-range id panics, which on this kernel is fatal; an out-of-range slot
reads far past the 16 KiB DMA region, and for a bound UDP port that memory
would then be echoed back to the network. Both are now checked, an
overlong frame is dropped rather than truncated, and `rx_errors` counts what
was refused.

Only a malicious or buggy hypervisor can reach this, so it is verified at
tier 1: three tests drive a fake device that reports an out-of-range
descriptor id, an out-of-range buffer slot, and an impossible length. Each is
dropped, and the driver keeps working afterwards.

## Verification

`cargo test --workspace` green, `hal_x86_64` now 114 tests. The whole gauntlet
suite passes end to end: `tcp_slots`, `cmd_pipeline`, `cmd_halfclose`,
`tcp_concurrent`, `tcp_bulk_echo`, `tcp_abandon`. Graphics mode with mouse
movement, `smp run 16`, and `fault pf` all behave as before.
