# Phase 254: SMP Bring-Up — Application Processors Online

## Summary

First bare-metal step of `docs/next_steps.md` item 1 (multi-core). Until now the kernel ran on one vCPU and every shared structure was justified with a "single CPU" comment. This phase brings the other CPUs into kernel code and puts the first real lock in place.

- `hal_x86_64::spin::SpinLock<T>`: test-and-set spinlock with a guard, `try_lock`, and an unchecked accessor for early boot. Host-tested with 8 threads hammering a counter.
- `hal_x86_64::smp::CpuRegistry`: lock-free registry (up to 64 CPUs) that APs use before any allocator or lock is trusted on them: `register(lapic_id)`, `online()`, `lapic_id(index)`, and `wait_for(expected, max_spins)`. Host-tested including 16 concurrent registrations.
- `kernel_bootstrap`: a Limine `MpRequest`; `install_idt` now splits out `load_idt`, so each AP loads the shared IDT; `ap_entry` registers the CPU and parks in `hlt`. `start_application_processors` releases every non-BSP CPU through its `goto_address`, waits for registration, and logs `SMP: N of M CPUs online (bsp lapic X)`.
- The global free-list heap now sits behind a `SpinLock` instead of a bare `UnsafeCell`, so APs may allocate. Interrupt handlers still do not allocate, so no interrupt masking is needed around the lock.
- New workspace command `cpus` (palette "Show CPUs", alias `smp`) prints online/total counts, the BSP LAPIC id, and each logical CPU's LAPIC id.
- `cargo xtask qemu`, `qemu-smoke`, and `qemu-script` now pass `-smp 4`.

## Verification

- `cargo test --workspace` green (hal_x86_64 84, kernel_bootstrap 92 + 86).
- QEMU (`-smp 4`): serial shows `SMP: 4 of 4 CPUs online (bsp lapic 0)`; `cpus` lists cpu0..cpu3 with LAPIC ids 0, 1, 3, 2 (registration order is arrival order). Storage still mounts the existing disk.
- Graphics desktop regression check under the locked heap: `display graphics`, pointer moves, `gfx stats` reports 44 frames, 44 presents, 0 rejected, 0 slow, average present 0.63 ticks.

## Not Yet

APs only park. No per-CPU GDT/TSS, LAPIC timer, IPIs, or cross-CPU scheduling; the bare-metal kernel still runs its single cooperative loop on the BSP. Those are the next SMP phases.
