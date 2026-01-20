# QEMU Boot (Limine ISO)

This document covers the bare-metal bootstrap ISO pipeline using Limine.

## Prerequisites

- Rust toolchain (stable is fine) with the target installed:
  - `rustup target add x86_64-unknown-none`
- `xorriso`
- `qemu-system-x86_64`
- `git` (used by `cargo xtask limine-fetch` unless you provide `--source`)
- Limine bootloader files:
  - Run `cargo xtask limine-fetch` to populate `third_party/limine/`
  - Ensure a Limine host utility is available (`limine` or `limine-deploy`) for BIOS boot
    (install via your package manager or build from Limine source, then place it in
    `third_party/limine/` if `limine-fetch` does not provide it)

## Build the ISO

```
cargo xtask iso
```

Output:
- `dist/pandagen.iso`

## Run in QEMU

```
cargo xtask qemu
```

This runs:

```
qemu-system-x86_64 -m 512M -cdrom dist/pandagen.iso -serial stdio -display none -no-reboot
```

## Expected Behavior

- Limine menu appears.
- Selecting the entry boots the stub kernel.
- A serial prompt appears in the terminal (`PandaGen: kernel_bootstrap online`).
- Typing happens in the terminal window (QEMU runs without a graphical display).
- Typing `help` prints available commands.
