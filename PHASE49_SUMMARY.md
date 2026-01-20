# Phase 58: User Task Boundary v1

**Completion Date**: 2026-01-20

## Overview

This phase introduces a minimal user-task context with separate stacks and a
trap entry for syscalls, alongside a slightly expanded v0 API.

## What Was Added

- `sim_kernel::user_task` with `UserTaskContext`, `UserSyscall`, and trap entry.
- `KernelApiV0` gains `yield_now` and `sleep` for user-task syscalls.
- `SimulatedKernel::spawn_user_task` helper for deterministic tests.

## Tests

- `cargo test -p sim_kernel`
