# Phase 420: the simulated kernel's trap takes every syscall, and a grant is the granter's (SEC-021)

## What changed

`sim_kernel` has two doors into the kernel: `SyscallGate::execute` (over
`&mut dyn KernelApi`) and `user_task::default_trap` (over the
`SimulatedKernel`, the one a user task's context calls). Since Phase 61:

- **`default_trap` answered three syscalls** -- CreateChannel, Send and
  Recv -- and refused everything else as "not supported in default_trap":
  a user task could not sleep, read the clock, yield, spawn, register or
  look up a service, or touch memory.
- **`Grant` checked nothing about the granter.** The gate called
  `KernelApi::grant_capability(task, cap)`, which records a new grant for
  `task`. Any task could give any task any capability id it chose -- a
  forged capability, minted. GAUNTLET listed it as open: "the kernel does
  not check *who* is granting a capability".

Now:

- `syscall_gate::dispatch` is the one match that carries out a syscall
  over `KernelApi`; `execute` and the trap share it.
- The trap routes **every** syscall: the `KernelApi` ones through
  `dispatch`; memory (CreateAddressSpace, AllocateRegion, AccessRegion)
  through the kernel's own `MemoryOps`, which check the caller's
  capabilities; each recorded in the gate's audit log as before.
- **`Grant` through the trap** finds the calling task
  (`SimulatedKernel::task_of_execution`) and calls `delegate_capability`,
  which requires the caller to hold the capability, runs the delegation
  policy, and moves it (the granter no longer has it).
- **`Grant` through `execute`** is refused: `KernelApi` cannot say who
  the caller is, so it cannot check.

## Why

A capability system whose grant syscall mints is not one. This was the
last open item in GAUNTLET's capability notes, and the trap that refused
most syscalls was one of the earliest "not yet implemented" paths
(Phase 61).

## Tests

- `test_trap_answers_every_syscall_not_three`: Now, Yield, and the memory
  syscalls through the trap -- a read-only region refuses a write.
- `test_a_task_grants_only_what_it_holds`: a forged capability is
  refused; a held one moves; granting it again fails (it is gone).
- All 169 `sim_kernel` unit tests and its integration tests pass
  unchanged.
- `cargo xtask gauntlet`: exit 0.
