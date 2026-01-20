# Phase 60: Console + Command Move to User Task

**Completion Date**: 2026-01-20

## Overview

This phase finishes moving the bootstrap command loop into the user task domain
so the kernel retains only primitives and message routing.

## What Was Added

- `kernel_bootstrap` now spawns the command task in the `User` domain.

## Tests

- Not run (no dedicated `kernel_bootstrap` tests in this pass)
