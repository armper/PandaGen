# Phase 262: Per-CPU GDT/TSS And Exception Diagnostics

## Summary

Fourth SMP step (`docs/next_steps.md` item 1) and a debugging win for the whole kernel: until now any CPU exception was a silent triple fault (QEMU just exited, which is how the LAPIC mapping bug in Phase 255 first showed up). Every CPU now runs on kernel-owned descriptor tables with a dedicated double-fault stack, and faults print a register dump.

- `hal_x86_64::gdt`: `Gdt` (null, kernel code 0x08, kernel data 0x10, 16-byte TSS descriptor at 0x18), `Tss` (104-byte 64-bit layout, IST entries, I/O map disabled), `tss_descriptor` encoding, and `gdt::load`, which does `lgdt`, reloads CS through a far return, resets the data segments, and `ltr`. Host tests pin the code/data descriptor encodings, the TSS descriptor base/limit split, the 104-byte layout and field offsets.
- `kernel_bootstrap`: per-CPU `Gdt`/`Tss`/16 KiB IST stack for up to 8 CPUs; `init_cpu_tables(index)` runs on the boot CPU before the IDT is built (so IDT gates use selector 0x08) and on each AP right after it registers. `IdtEntry::set_handler_ist` selects an interrupt stack; the double fault uses IST1.
- Exception stubs for #DE, #UD, #DF, #GP, #PF normalise the error code, save all registers, and call `exception_handler`, which prints `KERNEL EXCEPTION <name> vector=.. err=.. rip=.. cs=.. rflags=.. rsp=.. cr2=.. cpu_lapic=..` plus the general registers, then halts that CPU.
- `fault pf|ud|de` (console only, not on the remote allowlist) triggers each exception on purpose; `qemu-script` fails a run that logs `KERNEL EXCEPTION` unless `--allow-exception` is given.

## Verification

- `cargo test --workspace` green (gdt tests added).
- QEMU `-smp 4` boot: `GDT/TSS installed for cpu0 (IST1 for double faults)`, all four CPUs online, `smp run 4` completes, and the graphics desktop still presents with no rejections, all on the new tables.
- `fault pf` -> `#PF page fault vector=14 err=0x0 ... cs=0x8 ... cr2=0x10`; `fault ud` -> `#UD invalid opcode vector=6`; `fault de` -> `#DE divide error vector=0`, each with the register dump and `cpu_lapic=Some(0)`.

## Next

With tables, timers, and fault reporting per CPU, the remaining SMP piece is a preemptible per-CPU run loop; the double-fault stack also makes stack-overflow detection possible with a guard page.
