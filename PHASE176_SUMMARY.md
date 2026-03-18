# Phase 176 Summary: Persistent Boot Profile Load/Save

## What Changed
- Implemented persistent boot profile storage in `services_workspace_manager/src/boot_profile.rs`:
  - `BootProfileManager::load()` now accepts `Option<&mut JournaledStorage>` and performs transactional reads.
  - `BootProfileManager::save()` now accepts `Option<&mut JournaledStorage>` and performs transactional writes.
  - Boot profile data is stored under a deterministic `ObjectId` derived from a fixed UUID.
- Added robust fallback behavior:
  - If storage is absent, load falls back to default config and marks manager as loaded.
  - If stored object is missing, load falls back to default config.
  - If stored bytes are corrupt/invalid JSON, load falls back to default config.
  - Hard storage transaction/read/write/commit failures still return explicit errors.
- Added tests:
  - `test_boot_profile_manager_save_and_load_roundtrip`
  - `test_boot_profile_manager_load_corrupt_data_falls_back_to_default`

## Rationale
- Previous behavior had placeholder/no-op persistence and could not retain boot preferences across sessions.
- Deterministic object identity plus transactional storage provides stable, testable persistence without introducing path-based ambient authority.
- Corruption fallback keeps boot behavior reliable and deterministic.

## Validation
- `cargo test -p services_workspace_manager` passed (all tests green).
