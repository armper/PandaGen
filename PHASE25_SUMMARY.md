# Phase 25: Executable Format + Component Loader (PandaGen Packages)

**Completion Date**: 2026-01-19

## Overview

Phase 25 introduces the **PandaGen package format** and a **component loader** that turns declarative manifests into workspace launch plans. This is the foundation for a modern, explicit executable format (no POSIX binaries, no ambient launch state).

## What Was Added

### 1. Package Manifest Format (`packages` crate)

**Manifest (`pandagend.json`)**
- `format_version` (major/minor)
- `name` / `version`
- `components[]` with:
  - `id`, `name`, `component_type`, `entry`
  - `focusable`, `metadata`, optional `budget`

**Types:**
- `PackageManifest`
- `ComponentSpec`
- `PackageComponentType`
- `PackageFormatVersion`

### 2. Loader + Validation

**PackageLoader**
- Loads `pandagend.json` from a directory or explicit path
- Validates:
  - non-empty names
  - unique component IDs
  - unique component names
  - non-empty entry points

**ComponentLoader**
- Produces a clean launch plan (`ComponentLaunchSpec`) from a manifest

### 3. Workspace Integration

`WorkspaceManager::launch_package()`
- Converts package components into `LaunchConfig`
- Preserves package metadata (`package.name`, `package.version`, `package.entry`)
- Launches components in manifest order

## Tests Added

**packages**:
- `test_load_manifest`
- `test_duplicate_component_ids_fail`
- `test_component_loader_builds_plan`

**services_workspace_manager**:
- `test_launch_package_components`

## Design Decisions

- **Explicit format**: JSON manifest defines components directly.
- **No ambient execution**: everything becomes a typed launch plan.
- **Separation of concerns**: packages define *what* to launch; workspace decides *how*.

## Files Changed

**New Files:**
- `packages/Cargo.toml`
- `packages/src/lib.rs`

**Modified Files:**
- `Cargo.toml` (workspace member + dependency)
- `services_workspace_manager/src/lib.rs` (launch package API)
- `services_workspace_manager/Cargo.toml` (dependency)

## Future Work

- Signed package metadata
- Dependency graphs between components
- Capability grants derived from manifest policies

## Conclusion

Phase 25 establishes PandaGen’s package format and a clean component loader, enabling explicit, testable component launches without legacy executable assumptions.
