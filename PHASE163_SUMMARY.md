# Phase 163 Summary

## Overview
- removed unsupported cfg(feature) dependency gating in services_workspace_manager.
- made hashbrown a normal dependency to silence Cargo warning.

## Rationale
- Cargo does not support feature predicates inside target dependency sections.
- hashbrown is used unconditionally in this crate, so it should be a direct dependency.

## Tests
- not run (per request).
