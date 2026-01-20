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
- Boot diagnostics are printed (HHDM offset, kernel addresses, memory map summary).
- Interrupt initialization messages appear.
- A serial prompt appears in the terminal (`PandaGen: kernel_bootstrap online`).
- **NEW in Phase 57**: Keyboard input is now IRQ-driven - typing on keyboard generates visible characters.
- **Editor mode**: The kernel now runs in editor mode, displaying typed characters in a simple text editor.
- Timer ticks are shown as dots (. printed every second at 100 Hz) in the old console mode.

## Expected Serial Output

```
PandaGen: kernel_bootstrap online
hhdm: offset=0xffff800000000000
kernel: phys=0x... virt=0x...
memory: entries=... total=... KiB usable=... KiB
allocator: ranges=... frames=... reserved=...
heap: base=0x... size=... bytes
Initializing interrupts...
IDT installed at 0x...
PIC remapped to IRQ base 32
PIT configured for 100 Hz
Interrupts enabled, timer at 100 Hz, keyboard IRQ 1
Type to see keyboard input (editor mode)...

--- Editor State ---
Text: hello world
Cursor: 11 | Length: 11
-------------------

--- Editor State ---
Text: hello world\n
Cursor: 12 | Length: 12
-------------------
```

## Kernel Features Demonstrated

### Bare-Metal Interrupt Infrastructure
- **IDT (Interrupt Descriptor Table)**: 256-entry x86_64 IDT with proper gate descriptors
- **PIC (8259 Controller)**: Initialized with IRQ remapping from 0-15 to 32-47
- **PIT (Programmable Interval Timer)**: Configured at 100 Hz periodic interrupts
- **Timer IRQ Handler**: Assembly entry stub with full register save/restore + EOI to PIC
- **Keyboard IRQ Handler** (Phase 57): IRQ1 handler reads PS/2 scancodes from port 0x60
- **Global Tick Counter**: Atomic u64 incremented by timer IRQ, queryable via `ticks` command

### Keyboard Input (Phase 57)
- **PS/2 Hardware**: Direct port I/O to keyboard controller (0x60/0x64)
- **IRQ-Driven**: No polling loops - keyboard generates IRQ1 on key press/release
- **Bounded Queue**: 64-entry lock-free ring buffer for scancode buffering
- **Drop Policy**: DropOldest - overwrites oldest scancode when queue is full
- **Scancode Translation**: PS/2 Set 1 scancodes to ASCII with shift modifier support
- **Editor State**: Simple text buffer (1024 bytes) with cursor tracking
- **Rate-Limited Rendering**: Updates displayed every 100ms when content changes

### Supported Keys
- **Letters**: A-Z (with shift for uppercase)
- **Numbers**: 0-9 (with shift for symbols: !@#$%^&*())
- **Special Keys**: Space, Enter, Backspace
- **Modifiers**: Left/Right Shift (tracked for uppercase/symbols)

### User Space Simulation (Old Console Mode - Not Active in Phase 57)
- **Syscall Infrastructure**: sys_yield, sys_sleep, sys_send, sys_recv implemented
- **User Tasks**: CommandService demonstrates syscalls by alternating yield/sleep
- **IPC**: Channel-based message passing between console and command services
- **Time Slicing**: Cooperative scheduling with quantum-based task switching

### Console Commands (Not Available in Editor Mode)
- `help` - List all commands
- `halt` - Halt the system
- `boot` - Display boot information (HHDM, kernel addresses)
- `mem` - Display memory allocator state
- `alloc` - Allocate a single physical frame
- `heap` - Display heap statistics
- `heap-alloc` - Allocate 64 bytes from heap
- `ticks` - Display current kernel tick count

## Troubleshooting

If the ISO doesn't build:
- Ensure `xorriso` is installed
- Verify Limine files are present in `third_party/limine/`

If QEMU doesn't boot:
- The ISO is UEFI-only (BIOS boot requires a native limine-deploy binary)
- Try running with `-d int` for interrupt debugging
