# Phase 222: PS/2 Mouse In The Kernel (GFX-022 Complete)

## Summary

This phase completes `GFX-022` by wiring the Phase 221 pointer stack into the bare-metal kernel and verifying it in QEMU.

`kernel_bootstrap/src/main.rs`:
- `MOUSE_EVENT_QUEUE`, a second IRQ-to-loop byte ring, fed by `mouse_irq_handler` on vector 44 (IRQ 12). The handler only reads a byte flagged as auxiliary data and acknowledges both PICs.
- `irq_mouse_entry` assembly stub and an IDT entry for vector 44; `unmask_mouse_irq` clears IRQ 12 on the slave PIC and the IRQ 2 cascade on the master.
- `Ps2MouseInit::initialize` runs after keyboard IRQ setup and before `enable_interrupts`, so handshake ACKs are read by the init code rather than stolen by the interrupt handler. The negotiated report (device id, wheel, command byte) is logged and passed to the workspace loop; on failure the mouse IRQ stays masked and the loop reports no device.
- The loop frames queued bytes with `Ps2MousePacketParser`, translates packets with `PointerTranslator` confined to the framebuffer (or the 640x400 VGA text area), and records the latest position and button set in the workspace.

`kernel_bootstrap/src/workspace.rs`: new `pointer` command prints position, buttons, and event count without allocating (`append_i32` helper). `help` lists it.

`xtask qemu-script`: `mouse:dx;dy[;dz]` and `mbtn:<mask>` directives drive the QEMU monitor's `mouse_move` and `mouse_button`.

## Rationale

The interrupt handler stays minimal by design: one status read, one data read, one queue push, two EOIs. Packet framing and translation happen in the loop where they are ordinary testable code, the same split the keyboard path uses. Initialising the mouse with interrupts disabled avoids a classic bring-up race where IRQ 12 fires on the first ACK and the init code waits forever on an output buffer the handler already drained.

Pointer state is exposed through a workspace command first so the whole path can be asserted from the serial log before any cursor is drawn (`GFX-025`).

## Verification

`cargo test -p kernel_bootstrap` (81 lib + 75 bin) and `cargo build --profile kernel --target x86_64-unknown-none`.

Scripted QEMU session (`cargo xtask qemu-script` with mouse directives), serial log:

```
ps2 mouse: ready (id=3 wheel=true cmd_byte=0x43)
Pointer: x=640 y=400 buttons=000 events=0      (boot: centred, nothing held)
Pointer: x=670 y=410 buttons=100 events=2      (mouse:30;10 then left down)
Pointer: x=162 y=0   buttons=000 events=7      (left up, mouse:-1000;-1000; QEMU chunks to 127/packet)
Pointer: x=0   y=0   buttons=000 events=14     (wheel notch flushed remaining motion; clamped to corner)
```

Direction, button edges, wheel delivery, and clamping all match the translator's unit tests.
