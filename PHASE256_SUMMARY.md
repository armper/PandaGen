# Phase 256: Parallel Desktop Present Across CPUs

## Summary

First real use of the application processors (`docs/next_steps.md` item 1). The desktop present converts a 1280x800 RGBA surface to the framebuffer's format on every frame; that loop is now split into row bands and run on all online CPUs.

- `framebuffer::PresentBand` describes one band of the RGBA to RGB32 conversion with raw pointers so another CPU can run it; `split_bands` divides the frame into up to `MAX_PRESENT_BANDS` (8) equal bands; `convert_rgba_rows` does the per-row conversion with 32-bit volatile stores (the old path wrote four volatile bytes per pixel). `present_desktop_surface_with(surface, workers, run)` keeps the strict dimension/length contract of `present_desktop_surface` and hands the bands to a caller-supplied runner; the sequential path is the same code with one band.
- `kernel_bootstrap`: job kind `JOB_PRESENT_BAND` reads its band from the `PRESENT_BANDS` spinlock slot table and converts it. `run_present_bands` stores the bands, queues bands 1..n, sends a wake IPI, converts band 0 on the boot CPU, then waits for each job; a band whose job is never picked up is converted locally, so a missing or slow CPU degrades to the old behaviour instead of tearing.
- `smp present [on|off]` toggles the split (default on when other CPUs are online). `PARALLEL_PRESENT` and `present_workers()` decide the band count per frame.
- `hal_x86_64::rdtsc` and `GfxTelemetry::record_present_cycles` add cycle-level present timing; `gfx stats` gains a `present: workers=N avg_cycles=M` line (ticks were too coarse to see a 3x change).

## Verification

- `cargo test --workspace` green. New host tests: band splitting covers every row exactly once for 800/4, 800/3, 5/8, 1/4, 7/1 and yields no bands for an empty frame; conversion honours source and destination strides and leaves padding untouched; telemetry averages cycles and reports workers.
- QEMU `-smp 4`, graphics mode, three pointer moves each, `gfx stats`:

| mode | workers | avg present cycles |
| --- | --- | --- |
| parallel (default) | 4 | 3,775,940 |
| `smp present off` | 1 | 12,021,420 |

  Presents stay at 0 rejected and 0 slow, and the screendumps for both modes show the same desktop with no band seams.

## Next

The remaining per-frame cost is scene rasterization on the boot CPU; the same band mechanism can carry tiles of the compositor once the rasterizer targets can be split by rows. Per-CPU GDT/TSS and a LAPIC timer are still needed before APs can run anything preemptively.
