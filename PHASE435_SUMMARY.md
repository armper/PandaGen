# Phase 435: programs -- ring 3, an address space each, capabilities only (PROC-002, PROC-003)

## What changed

The machine runs code that is not the kernel. Until now every card and
every command was compiled into the kernel and ran with all of its
authority: one mistake anywhere could take down everything. A *program*
now runs in ring 3, in an address space of its own, and can reach
anything outside that only by asking the kernel, through one gate,
naming things by handles it was given. When it breaks a rule the CPU
stops it, the kernel ends it, and the machine carries on.

This is step 2 of the road to programs that run on their own (after
Phase 434's threads; next come a program format and loader, then an
app-to-desk protocol, then supervision).

### An address space each (`hal_x86_64::user_space`)

- Every program gets its own top-level page table. The upper half -- the
  kernel, the direct map, the heap -- is shared by copying the kernel's
  256 top-level entries, so the kernel is where it always is whichever
  program runs. Those pages are not marked USER; the program cannot
  touch them.
- The lower half is the program's alone: its code at `0x400000`
  (read-only, executable), its stack below `0x800000` (writable, and
  no-execute when the CPU has NX).
- A program's pointers are never followed by the kernel. `read_into` and
  `write` check that every page is the program's -- present and USER at
  every level, writable if written -- and copy through physical frames.
  A bad pointer is an error returned to the program, never a fault in
  the kernel. `read_into` allocates nothing, so a system call can use it
  with interrupts off.
- The space records every frame it takes (tables and pages) and gives
  them all back when the program is reaped.
- Walks go through `PhysMemory`, like `mmio_map`, so all of it is tested
  on the host against an in-memory arena.

### Ring 3

- The GDT has user code and data segments now (DPL 3; data before code,
  the order `sysret` would want): null, kernel code, kernel data, user
  data `0x1B`, user code `0x23`, TSS `0x28`.
- A program is a thread (Phase 434) whose first frame `iretq`s to ring 3
  (`sched::user_frame`). Switching to one switches CR3 and points the
  TSS's `rsp0` at the top of its kernel stack, where the CPU puts the
  frame when the program is interrupted or calls.
- Ring 3 has no I/O permission (IOPL 0, no I/O bitmap), so `in`, `out`,
  `cli` and `hlt` fault. Every interrupt gate but `0x80` is DPL 0, so a
  program that tries `int 0x81` or `int 32` faults too.

### Asking the kernel (`syscall_abi`)

`int 0x80`: the call in `rax`, arguments in `rdi`, `rsi`, `rdx`, the
answer in `rax` (errors are small negative numbers). `exit`, `yield`,
`sleep(ms)`, `send(handle, ptr, len)`, `time`, `drop(handle)`.

There are no paths and no ambient "console". A program reaches anything
through a *handle*, a small number that means something only in its own
table of capabilities, filled by whoever started it. A handle it was not
given is not refused -- it does not exist (`NoSuchHandle`). A capability
held without the right refuses (`NotAllowed`). `run` gives each program
exactly one: lines to the Terminal, as handle 0.

A call that waits (`sleep`, `yield`) saves the program's own ring-3 frame
and switches away, so it never waits inside the kernel. `send` enables
interrupts before it allocates, like any thread (a preempted thread may
hold the heap's lock).

### When a program breaks a rule

The exception entry now returns for a fault in ring 3: the program's
thread ends with the vector, the address (for a page fault) and where it
was, and the entry resumes whichever thread runs next. A fault in the
kernel is still fatal, as it should be. The desk reaps the program, frees
its frames, and says how it ended:

    crash: ended by the kernel: it touched 0xffff800000100000, not its memory (at 0x40000a)
    rogue: ended by the kernel: it did what a program may not (general protection) (at 0x400000)

`stop <id>` ends a program at once when it is between its own
instructions (its saved frame is ring 3, so it holds nothing of the
kernel's), and otherwise at its next call or tick in ring 3. Kernel
threads are still stopped only by asking.

### In the Terminal

- `programs` -- what there is to run.
- `run <program>` -- `hello` (says hello three times, then tries a handle
  and a pointer it was not given), `crash` (writes to the kernel's
  memory), `rogue` (turns interrupts off), `hog` (loops forever).
- `threads` marks programs and their memory: "a program in 40 KiB of its
  own".

The programs are a few built in, as position-independent machine code
copied into fresh pages the way a loaded program would be. A program
format, a loader and an SDK to write them in Rust are next.

## Tests

- `hal_x86_64::user_space`: the kernel half shared and the program half
  empty; loaded code readable and not writable; data writable and not
  executable (and no NX bit without NXE); pointers unmapped, straddling,
  in the kernel's half, wrapping or at the edge refused, and a refused
  write writes nothing; a kernel leaf under a program's tables is not the
  program's; freeing returns every frame and none of the kernel's.
- `hal_x86_64::gdt`: user segments are DPL 3 and match their selectors;
  they differ from the kernel's only in privilege; the TSS moved.
- `syscall_abi`: decoding, a handle not given does not exist, a held
  capability without the right refuses, dropped handles are gone, the
  table is bounded, ends read plainly.
- `sched`: a program's first frame is ring 3 with an empty kernel stack
  and a 16-aligned `rsp0`; the running thread's stack is live, not saved.
- Gauntlet, "programs: ring 3, refused handles, faults end only the
  program": `run hello`, `run crash`, `run rogue`, `run hog`, `threads`,
  `stop 4`, `mem`; every line above, no kernel exception or panic, and
  the desk still answering at the end.
