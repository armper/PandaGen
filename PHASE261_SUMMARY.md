# Phase 261: Per-CPU LAPIC Timer Ticks

## Summary

Third SMP step (`docs/next_steps.md` item 1). Every application processor now has its own periodic time source, the prerequisite for preemptive scheduling on those CPUs. The boot CPU keeps the PIT.

- `hal_x86_64::lapic`: timer registers (`LVT_TIMER`, `TIMER_INITIAL`, `TIMER_CURRENT`, `TIMER_DIVIDE`) with `set_timer_divide`, `start_timer_masked` (one-shot, masked, for calibration), `start_timer_periodic(vector, initial)`, `stop_timer`, and `timer_current`. Tested against the fake MMIO.
- `kernel_bootstrap`: vector 0xF1 (`irq_lapic_timer_entry`) counts a tick into `CPU_TICKS[cpu index]` (index found by LAPIC id in the CPU registry) and acknowledges. After the boot CPU enables interrupts, `calibrate_lapic_timer` runs the LAPIC timer masked at divide 16 across 10 PIT ticks, derives the initial count for 100 Hz, publishes it in `LAPIC_TIMER_INITIAL`, and sends a wake IPI; each AP's idle loop starts its periodic timer the first time it sees the value. Timer interrupts also wake `hlt`, so the idle loop keeps re-checking the work queue.
- `cpus` now prints the calibration (`lapic timer: initial=N (100 Hz), bsp ticks=T`) and each CPU's tick count.

## Verification

- `cargo test --workspace` green (lapic timer programming test added).
- QEMU `-smp 4`: `LAPIC timer: 6265250 counts per 10 ticks, initial=626525 for 100 Hz`. Two `cpus` samples about 3 s apart:

| sample | bsp ticks (PIT) | cpu1..3 ticks (LAPIC) |
| --- | --- | --- |
| 1 | 1321 | 1306 |
| 2 | 1615 | 1599 |

  The AP counters advance by 293 while the PIT advances by 294, so the per-CPU timers run at the PIT rate; the constant offset is the calibration window. `smp run 4` still completes with timers running.

## Next

With per-CPU ticks in place, the next SMP phase can give each AP a preemptible run loop (per-CPU GDT/TSS and interrupt stacks first), then move kernel tasks onto them.
