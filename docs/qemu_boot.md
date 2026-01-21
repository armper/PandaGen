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
qemu-system-x86_64 -m 512M -cdrom dist/pandagen.iso \
  -drive file=dist/pandagen.disk,format=raw,if=none,id=hd0 \
  -device virtio-blk-pci,drive=hd0 \
  -serial file:dist/serial.log \
  -display cocoa \
  -no-reboot
```

**Phase 78 Update**: The QEMU window now displays a **VGA text console (80x25)**! The workspace and UI
are visible in the QEMU window, not in the host terminal.

### VGA Text Console Mode

- **Main UI**: VGA text mode in QEMU window (80x25 characters)
- **Serial Logs**: Output goes to `dist/serial.log` for debugging
- **Interaction**: Click QEMU window to capture keyboard input
- **Fallback**: If VGA unavailable, falls back to framebuffer console

## Expected Behavior

- Limine menu appears.
- Selecting the entry boots the kernel.
- Boot diagnostics are printed to serial log.
- **Phase 78**: The QEMU window shows **VGA text console with workspace prompt**
- **Phase 89**: You can run `open editor` to launch the vi-like editor
- Keyboard input is visible in the QEMU window, not the host terminal
- Serial logs written to `dist/serial.log`
- Workspace prompt: `> ` with command history and editing

## Using the Editor (Bare-Metal)

### Opening the Editor

At the workspace prompt:
```
> open editor
```

The editor will open in the QEMU window.

### Editor Commands

**Insert Mode:**
- `i` - Enter insert mode at cursor
- `a` - Enter insert mode after cursor
- Type text normally
- `Backspace` - Delete character before cursor
- `Enter` - New line
- `Esc` - Return to normal mode

**Normal Mode (Navigation):**
- `h` - Move left
- `j` - Move down
- `k` - Move up
- `l` - Move right
- `x` - Delete character at cursor
- `dd` - Delete current line
- `:` - Enter command mode

**Command Mode:**
- `:q` - Quit (fails if buffer is dirty)
- `:q!` - Force quit (discard changes)
- `:w` - Write (shows "Filesystem unavailable" in bare-metal mode)
- `:wq` - Write and quit
- `Esc` - Cancel command

### Limitations in Bare-Metal Mode

- **No Filesystem**: Bare-metal editor operates in-memory only. `:w` command shows a message that filesystem is unavailable.
- **In-Memory Editing Only**: All edits are lost when you quit the editor or reboot.
- **VGA Text Mode**: 80x25 character display, no syntax highlighting.
- **PS/2 Keyboard Only**: USB keyboards may not work depending on BIOS/UEFI compatibility layer.

### Editor Implementation

The bare-metal editor uses a minimal implementation (`kernel_bootstrap/src/minimal_editor.rs`) that:
- Shares the same modal editing philosophy as `services_editor_vi`
- Uses alloc (Vec/String) with the global BumpHeap allocator
- Renders directly to VGA without services_view_host
- Has no external service dependencies

For full-featured editing with filesystem support, use the simulator mode (`pandagend`).

## Expected Behavior

## VGA Text Console (Phase 78)

The kernel now initializes a VGA text console at boot:

- **Physical Address**: 0xB8000 (standard VGA text buffer)
- **Virtual Mapping**: Uses HHDM offset to map physical VGA memory
- **Resolution**: 80 columns × 25 rows
- **Colors**: 
  - Normal text: Light gray on black (0x07)
  - Prompts: Bright green on black
  - Errors: Bright red on black
  - Banners: Bright cyan on black
- **Serial Logging**: Always available in `dist/serial.log` for debugging

### VGA Details

- Each character cell is 2 bytes: ASCII byte + attribute byte
- Attribute byte: `(bg_color << 4) | fg_color`
- Direct memory writes using volatile operations
- No ANSI codes, no terminal emulation
- Deterministic: same input → same display

### Troubleshooting VGA Mode

**"I don't see anything in the QEMU window":**
- Make sure you're running `cargo xtask qemu`, not manually running QEMU
- Ensure `-display cocoa` (or `-display gtk`/`-display sdl`) is used
- Don't use `-nographic` flag
- Click the QEMU window to capture keyboard focus

**"Where is the serial output?":**
- Serial logs are in `dist/serial.log`
- Use `tail -f dist/serial.log` to monitor logs in real-time
- Serial is for debugging only; UI is in QEMU window

**"VGA console says 'unavailable'":**
- Check serial log: `cat dist/serial.log | grep VGA`
- System will fall back to framebuffer console if available
- In worst case, falls back to serial-only mode

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

=== PandaGen Workspace ===
VGA text console initialized (80x25)
Main UI in QEMU window, serial logs here
Boot complete. Type 'help' for commands.
```

## Kernel Features Demonstrated

### VGA Text Console (Phase 78)
- **Direct Hardware Access**: Writes to VGA text buffer at 0xB8000
- **HHDM Mapping**: Uses Higher Half Direct Mapping to access physical memory
- **Color Attributes**: Style-based color mapping (normal, bold, error, success)
- **Cursor Rendering**: Visible cursor with attribute inversion
- **No Allocations**: Stack-only implementation, no heap usage
- **Testable**: 11 pure unit tests validate VGA console logic

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
- **Workspace Integration**: Full line editing with history in VGA window

### Supported Keys
- **Letters**: A-Z (with shift for uppercase)
- **Numbers**: 0-9 (with shift for symbols: !@#$%^&*())
- **Special Keys**: Space, Enter, Backspace
- **Modifiers**: Left/Right Shift (tracked for uppercase/symbols)

### Console Commands (Available in Workspace)
- `help` - List all commands
- `open editor` - Launch vi-like text editor (requires pandagend/sim mode)
- `list` - List active components
- `halt` - Halt the system
- `boot` - Display boot information (HHDM, kernel addresses)
- `mem` - Display memory allocator state
- `ticks` - Display current kernel tick count

### Using the Editor (Sim Mode Only)

The vi-like editor is available when running in simulation mode (pandagend):

```bash
# Run in simulation mode
cargo run --package pandagend

# In the workspace prompt:
> open editor

# Editor commands:
# - `i` - Enter INSERT mode to type text
# - `Escape` - Return to NORMAL mode
# - `:w` - Save file (when filesystem capability provided)
# - `:q` - Quit editor
# - `:q!` - Quit without saving

# Navigate with arrow keys (in NORMAL mode)
# Type text in INSERT mode
# Status line shows current mode and file state
```

**Note**: Full editor functionality with file save/load requires:
- Simulation mode (pandagend)
- Filesystem capability (PersistentFilesystem)
- services_workspace_manager integration

The bare-metal workspace shows a message directing users to pandagend for full editor support.

## Troubleshooting

If the ISO doesn't build:
- Ensure `xorriso` is installed
- Verify Limine files are present in `third_party/limine/`

If QEMU doesn't boot:
- The ISO is UEFI-only (BIOS boot requires a native limine-deploy binary)
- Try running with `-d int` for interrupt debugging

If VGA console doesn't appear:
- Check that you're using `-display cocoa` (or gtk/sdl), not `-nographic`
- Verify serial log shows "VGA text console initialized"
- Click QEMU window to capture keyboard

If keyboard doesn't work:
- Click inside the QEMU window to capture input
- Check serial log for keyboard IRQ messages
- Ensure PS/2 keyboard emulation is enabled in QEMU
