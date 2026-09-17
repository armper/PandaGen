# Phase 263: Kernel Tasks Run On Any CPU

## Summary

Fifth SMP step (`docs/next_steps.md` item 1) and the first cross-CPU scheduling: the kernel's service tasks (command service, console service) are now polled by whichever CPU is free, not only by the boot CPU's main loop.

- `KERNEL_LOCK` (a `SpinLock<()>`) serialises every touch of the kernel object: the boot loop takes it around `run_once` and around each block that builds a `KernelContext`; idle application processors `try_lock` it in their idle loop and call `run_once` when they get it. `KERNEL_READY` gates AP polling until the kernel and its tasks exist.
- The AP idle loop is now: take a band/job if one is queued; otherwise poll a kernel task if the lock is free; then re-check the queue under `cli` and `sti; hlt`, so a wake-up IPI that lands during a poll is never lost. `smp poll on|off` toggles AP polling at runtime.
- `TASK_RUNS_BY_CPU` counts polls that made progress per CPU; `cpus` prints `cpuN lapic=.. ticks=.. runs=..`, and `smp diag` shows the LAPIC timer calibration, present fallbacks, AP poll counts/cycles, and boot-CPU lock contention.
- Removed a scaffold leftover in `CommandService::poll`: a "syscall demo" that called `sys_sleep(1)` on every other poll, stalling whichever CPU polled the task for up to a full 10 ms PIT tick while holding the kernel lock. On the boot CPU this had been silently throttling the main loop since the early phases.

## Verification

- `cargo test --workspace` green.
- QEMU `-smp 4`: after `mem` and a host `remote:cpus` call, `cpus` shows task runs on the boot CPU and on cpu2, so commands really execute on application processors; UDP echo and remote IPC keep working while APs poll.
- Present timing A/B in one boot (`gfx stats`, three runs each): parallel present averages 3.8M cycles with AP polling on and 4.9M after `smp poll off`, both within the Phase 256 band, with `fallbacks=0` (every band job was picked up by an AP in time). Before the `sys_sleep` removal the same measurement was 16-18M cycles.

## Not Yet

Tasks are still poll-based state machines serialised by one lock, so this is concurrency of *placement*, not parallel execution; two tasks cannot run at once. Serial output from tasks polled on an AP shares the UART with the boot CPU without a lock (lines may interleave).
