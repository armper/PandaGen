# Phase 255: LAPIC, Wake-Up IPIs, And Jobs On Application Processors

## Summary

Second SMP step (`docs/next_steps.md` item 1). Phase 254 parked the application processors in `hlt`; now the boot CPU can hand them work and wake them.

- `hal_x86_64::lapic`: xAPIC driver over an `ApicMmio` trait (`RealApicMmio` for the mapped window, a fake in tests): APIC id, software enable through the spurious vector register, end-of-interrupt, fixed IPIs to one APIC id or to all-excluding-self with a delivery-pending wait. `SharedLapic` lets interrupt handlers on any CPU reach the window without owning a driver value.
- `hal_x86_64::mmio_map`: a real page-table walker (`translate`, `map_4k`) over a `PhysMemory` trait. Limine's direct map only covers memory-map entries, so touching 0xFEE00000 through it triple-faulted; the kernel now maps the LAPIC page uncached from CR3 using boot frames. Host tests cover table creation, huge-page detection at 1 GiB and 2 MiB, idempotent remaps, conflicting remaps, and frame exhaustion.
- `hal_x86_64::work_queue::WorkQueue<N>`: spinlock-protected FIFO of `(kind, arg)` jobs with atomic result slots (`submit`, `take`, `complete`, `wait`, `reset`). Host-tested with four worker threads draining 32 jobs.
- `kernel_bootstrap`: IDT vectors 0xF0 (wake IPI, acknowledges through the LAPIC) and 0xFF (spurious, plain `iretq`). Each AP enables its LAPIC and runs `ap_idle_loop`: `cli`, take a job, `sti` and run it, else `sti; hlt`, which closes the lost-wake-up window. The boot CPU maps and enables its LAPIC before releasing the APs.
- New command `smp run <n>` (up to 32): resets the queue, submits `n` sum-of-squares jobs, sends a wake IPI to every other CPU, waits for each result, and reports the executing CPU's LAPIC id, the value, whether it matches a local recomputation, and how many IPIs were received. The summary line comes first because the response buffer is 256 bytes.

## Verification

- `cargo test --workspace` green (hal_x86_64 98 tests).
- QEMU `-smp 4`: `LAPIC: bsp id 0 enabled at 0xffff8000fee00000`, `SMP: 4 of 4 CPUs online`, then `smp run 8` reports `8/8 jobs done, ipi_sent=true, ipis_received=3`; jobs land on LAPIC ids 1, 2, and 3 with no `MISMATCH`. A second `smp run` in the same boot also completes, so the idle loop and IPI path are reusable.
- Graphics regression under the new interrupt vectors: `display graphics` plus `gfx stats` reports 47 frames, 47 presents, 0 rejected, 0 slow.

## Not Yet

APs still have no per-CPU GDT/TSS, LAPIC timer, or scheduler; the BSP waits synchronously for job results. Next: LAPIC timer ticks on every CPU and moving real work (for example rasterizer tiles) onto the job queue.
