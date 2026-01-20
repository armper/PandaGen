# Phase 64: Process Manager + Supervision

**Completion Date**: 2026-01-20

## Overview

This phase brings a minimal process manager runtime online for service
supervision, restart policy enforcement, and exit handling.

## What Was Added

- `ProcessManager` with restart policy enforcement and exit handling.
- `ExitNotificationSource` trait for draining kernel notifications.
- Supervision tests for restart-on-failure and no-restart-on-normal-exit.

## Tests

- `cargo test -p services_process_manager`
