# Phase 56: Timer Tick + Preemption Enablement

**Completion Date**: 2026-01-20

## Overview

This phase wires a selectable tick source for x86_64 and a kernel tick
counter, keeping scheduling preemption-ready while staying deterministic.

## What Was Added

- `hal_x86_64::tick` with `TickSource` (PIT/HPET) and `KernelTickCounter`.
- Public PIT getters for configured frequency and interrupt enable state.
- Fake-timer tests that verify monotonic kernel tick aggregation.

## Tests

- `cargo test -p hal_x86_64`
