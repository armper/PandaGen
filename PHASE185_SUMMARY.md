# Phase 185 Summary: Custom Components Now Run in a Typed Interactive Host

## What Changed
- Replaced the `ComponentType::Custom` placeholder runtime path in `services_workspace_manager/src/lib.rs`:
  - Added `CustomComponentRegistry` with typed handler resolution (`Generic`, `Kiosk`, `PackageEntry`).
  - Added `CustomComponentRuntime` backed by `InlineConsole`.
  - Added `ComponentInstance::Custom` so custom components have a real live instance.
- Launch flow changes:
  - `ComponentType::Custom` now creates and publishes a real main/status view via `CustomComponentRuntime`.
  - Runtime resolution is metadata-aware:
    - `package.entry` routes to package-entry/default handler mapping.
    - `kiosk.app` routes to kiosk/default handler mapping.
    - fallback to `custom.generic`.
- Input/runtime behavior changes:
  - `route_input()` now routes focused custom component input into the custom host.
  - Added custom host command execution:
    - `help`
    - `status`
    - `meta`
    - `ping [payload]`
  - Output is appended to component views deterministically, matching existing CLI/pipeline patterns.
- Minor cleanup:
  - Scoped `ComponentInstance::None` to non-`std` builds only, since `std` builds now instantiate all current component types.

## Tests Added
- `test_custom_component_launches_runtime_host`
- `test_custom_component_host_handles_commands`

## Rationale
- Custom components previously launched as inert shells (`ComponentInstance::None`), creating running metadata entries without executable behavior.
- This phase makes custom components first-class runtime participants with typed handler routing and deterministic interaction surfaces.

## Validation
- `cargo test -p services_workspace_manager` passed.
