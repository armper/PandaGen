# Phase 55: Interrupt Dispatch + IRQ Plumbing

**Completion Date**: 2026-01-20

## Overview

This phase completes the interrupt plumbing scaffold for x86_64 with a
safe registration path, deterministic dispatch, and explicit PIC/APIC
acknowledgment handling.

## What Was Added

- `hal_x86_64::interrupts` with `InterruptDispatcher`, `IrqLine`, and
  `AckStrategy` for deterministic IRQ routing.
- PIC/APIC acknowledge counters and last-IRQ tracking for test visibility.
- Safe registration that rolls back registry state if IDT registration fails.
- `InterruptRegistry` now supports `unregister` to keep IDT/registry consistency.

## Tests

- `cargo test -p hal_x86_64`
