# Phase 436: program images -- Rust programs, loaded from the boot image (PROC-004)

## What changed

Programs are written in Rust now, built separately from the kernel,
shipped as images, and loaded into ring 3. Phase 435 ran a few programs
built into the kernel as hand-written machine code. Now `apps/primes` and
`apps/ticker` are ordinary Rust crates built against an SDK. The build
turns each into a *program image* and puts it in the ISO, and the
kernel finds the images at boot and runs them on request. This is step 3
of the road to programs that run on their own.

### `program_image`: what a program is

A program is not an executable file the kernel has to understand in all
its generality. It is a small, explicit image: a name, an entry point,
the pieces of memory to lay out, and the capabilities it asks for.

- **Each piece is writable or runnable, never both.** An image with a
  writable, runnable piece is refused, and so is an ELF segment that is
  both.
- **Pieces stay in the program's range** (above the first 4 MiB, so a
  null pointer faults; below 0x7F_0000_0000), don't share pages, don't
  wrap, and lay out at most 64 MiB. The entry must be in runnable code.
- **The asking is in the image**, so whoever starts a program can see
  what it wants before it runs, and grants what they choose. A program
  cannot reach for anything later, because it has no way to name it.
- Parsing refuses damaged images and never misreads them: every
  truncation and trailing byte is caught, and byte flips are fuzzed.
- `from_elf` turns a statically linked x86-64 ELF executable into an
  image at build time. It refuses position-independent executables,
  interpreters and write+execute segments. The kernel only ever reads
  images.

### `pandagen_app`: the SDK

The system calls (`exit`, `yield_now`, `sleep_ms`, `time_ms`), handles
(`Handle`, `Console`), `say!` for formatted lines in a fixed buffer (a
program needs no heap), and `entry!`, which defines `_start` (handing
`main` the console, handle 0) and a panic handler that says why and
exits with 101. Errors come back as `Error::NoSuchHandle`,
`Error::BadPointer` and so on.

### The programs and the build

- `apps/primes` counts the primes below a million, flat out, and says
  how long it took (78,498 primes, about 0.7 s under QEMU's emulated CPU).
- `apps/ticker` says the time three times, half a second apart. Then it
  sends on a handle it was never given and finds it is not there.
- Each program is its own Cargo workspace, excluded from the main one,
  and linked by `apps/app.ld` at `0x400000`, with code, read-only data
  and data in separate pages and segments. `cargo xtask iso` builds each
  with `RUSTFLAGS`, so none of the kernel's flags (its linker script
  above all) apply. It converts each ELF to an image with the
  capabilities listed in `PROGRAMS`, writes it to `boot/programs/`, and
  adds a `module_path`/`module_cmdline` pair to every boot entry.

### The kernel

- A Limine `ModuleRequest` collects the images at boot. They live where
  Limine loaded them, in memory the frame allocator already keeps for
  itself: `programs: 2 images (primes, ticker)`.
- `run` looks for an image first. It parses and checks the image and
  lays out each piece where it says (`UserSpace::lay_out`, which now
  handles a piece starting mid-page). Pieces get their access (read-only
  data is neither writable nor runnable), and the program gets a stack
  and what it asked for, as handles in the order asked. The built-in
  programs go through the same path, as one-piece images.
- `programs` lists the images, with their memory and what they ask for,
  then the built-ins.

## Docs

`docs/programs.md`: how to write and add a program, and what a program
is refused.

## Tests

- `program_image`: round trip; write+execute refused; the null page, the
  kernel's half, wrapping, shared pages and oversize refused; the entry
  must be runnable; damaged images refused at every truncation, trailing
  bytes, unknown asks, byte flips; ELF to image; foreign ELF refused.
- `pandagen_app`: answers are counts or errors; a long line is cut at a
  character boundary.
- `hal_x86_64::user_space`: a piece starting mid-page lands where it
  says, sized by its size and not its bytes; overfull refused.
- Gauntlet, "program images: Rust programs loaded from boot modules":
  `programs`, `run primes`, `run ticker`; the image list, the count of
  primes, all three ticks, the missing handle, both exits.

Not yet exercised: a program's panic path (`panicked: ...`, exit 101).
No program panics.
