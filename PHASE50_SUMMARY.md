# Phase 59: Service Registry Runtime + Bootstrapping

**Completion Date**: 2026-01-20

## Overview

This phase brings a kernel-visible registry bootstrap path online for core
services and introduces stable IDs for built-in services.

## What Was Added

- Stable core service IDs in `core_types::service_ids` (console/command/input/timer).
- `SimulatedKernel::bootstrap_core_services` registering core services with
  version policies.
- Host runtime now invokes core service bootstrap at startup.

## Tests

- `cargo test -p sim_kernel`
