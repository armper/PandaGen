# PHASE 99: Bare-metal build fixes (no_std + dependency gating)

## Overview

This phase unblocked `cargo xtask iso` by removing std-only dependencies from the bare-metal build path and aligning no_std imports across core crates.

## Key Changes

- **Pinned serde version** to 1.0.228 to match `serde_derive` and avoid `serde_core` mismatch errors.
- **HAL cleanup**: removed `thiserror` from `hal`, added `#![no_std]`, implemented `MemoryError` Display manually, and replaced `HashMap` with `BTreeMap` in the interrupt registry.
- **x86_64 HAL (hal_x86_64)**: added `#![no_std]` + alloc usage and explicit core prelude imports to restore `Option`, `Result`, `Default`, and related constructs under no_std.
- **Kernel API fixes**: added `alloc::format` and `ToString` imports for syscall error formatting.
- **Policy + resources**: added missing `alloc` imports for `vec!`, `format!`, `Box`, and `ToString`.
- **Pipeline backoff**: replaced `f64::powi` with a no_std-friendly multiplication loop.
- **Sim kernel gating**: removed `sim_kernel` from the bare-metal dependency graph by gating it to non-`target_os = "none"` builds and wrapping the `StorageBudget` impl accordingly.
- **Services storage cleanup**: fixed crate attributes and duplicate `alloc` declarations after no_std conversion.
- **Workspace open flow**: resolved borrow conflicts and fixed filesystem assignment in `kernel_bootstrap` workspace file opening logic.

## Rationale

The kernel ISO build targets `x86_64-unknown-none` with `-Zbuild-std=core,alloc`, so any std-only dependency or missing alloc/core imports break compilation. The adjustments ensure all kernel-path crates are `no_std`-safe and that simulation-only crates stay out of the bare-metal graph.

## Tests

- `cargo xtask iso`

## Notes

The build now completes successfully; remaining warnings are non-fatal and can be addressed separately if desired.
