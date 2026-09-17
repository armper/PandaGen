# Phase 275: Parallel Task Execution (Fine-Grained Kernel Locks)

## Summary

Closes the SMP item's last open piece (`docs/next_steps.md` item 1): kernel tasks now run in parallel on different CPUs instead of taking turns under one kernel lock, and the boot CPU no longer executes commands at all when other CPUs exist, so a slow command cannot freeze the desktop.

- `Kernel` is shared (`Sync`) and locked per piece: each channel is a `SpinLock<Channel>`, each task slot a `SpinLock<Option<TaskSlot>>`, the scheduler its own lock, `next_message_id` an atomic, and the frame allocator and bump heap locked `Option`s. `KernelContext` holds shared references and takes the relevant lock inside `send`/`recv`/`try_recv`; `Kernel::context()` replaces the five hand-built contexts in the main loop.
- `run_once(&self)` picks the next task under the scheduler lock, `try_lock`s its slot (another CPU already running it means "nothing to do here"), and polls it with no global lock held. The `KERNEL_LOCK` from Phase 263 and its contention counters are gone; application processors use a shared reference to the kernel.
- The boot CPU calls `run_once_filtered(.., skip_commands)` and skips command-service tasks whenever other CPUs are online (`smp bsp-commands on|off` toggles it for experiments), so command execution lands on application processors.
- Serial output is serialised by a writer lock now that several CPUs print; the exception handler uses an unlocked writer so a fault on a CPU that holds the lock still reports.
- `spin <ticks>` (up to 5 s, on the remote allowlist) reports which CPU ran it; `qemu-script` gains `remote-tcp-bg:<command>` and `remote-join` so mouse and key steps can overlap a remote call.

## Verification

- `cargo test --workspace` green.
- QEMU `-smp 4`, one boot, graphics mode, `gfx reset` before each run, five pointer moves during a background `spin 150` (1.5 s):

| boot CPU runs commands | spin ran on | frames during the window | animation wakes |
| --- | --- | --- | --- |
| off (default) | cpu1 | 27 | 12 |
| on (`smp bsp-commands on`) | cpu0 | 21 | 9 |

  Before this phase every command ran on cpu0 (Phase 274 baseline: `spin 200` -> `cpu Some(0)`). UDP echo, `smp run 4`, and `fault pf` (exception report through the unlocked writer) all still work.

## Limits

Tasks are still cooperative polls, and one task can only run on one CPU at a time; parallelism is across tasks. The console task shares COM1 input with whichever CPU polls it.
