# Phase 94: QEMU Keyboard IRQ Recovery + PS/2 Enable
**Completion Date**: 2026-01-21

## Overview
This phase restores QEMU keyboard input reliability by explicitly enabling IRQ1 in the i8042 controller, adding targeted serial-only interrupt pipeline diagnostics, and documenting the legacy PIC/PS/2 requirement. A manual QEMU smoke check is included to verify scancodes in serial logs.

## Root Cause
The boot path assumed the PS/2 controller command byte already enabled IRQ1. On some QEMU setups this bit can be cleared, which prevents keyboard IRQ delivery even when the PIC is remapped and unmasked.

## What Was Added
- **PS/2 controller enablement**: i8042 command byte is read and updated to ensure IRQ1 is enabled and keyboard clock is not disabled.
- **Serial-only IRQ diagnostics** (debug builds): logs at IRQ entry, scancode read, scancode→key event translation, and runtime consumption.
- **Interrupt visibility**: logs for IDT load status, IF flag, and PIC masks to confirm routing.
- **QEMU explicit machine type**: use `-machine pc` for the legacy PIC/PS/2 path.
- **Manual QEMU smoke test**: `cargo xtask qemu-smoke` validates scancode logs after a key press.
- **Docs**: updated QEMU boot guide with machine constraints and debugging checklist.
- **Tests**: new hosted scancode decoding tests for `Ps2ParserState`.

## Tests
- Not run (not requested).
