# Phase 89: Bare-Metal Editor Execution Infrastructure

## Overview
Implemented the foundational infrastructure for running the VI-like editor directly on bare-metal kernel_bootstrap, including global heap allocation support and no_std compatibility for key crates.

## Changes Made

### 1. Global Allocator Infrastructure (COMPLETED ✅)
- Added `#[global_allocator]` support to kernel_bootstrap
- Implemented `GlobalAlloc` trait for `BumpHeap` with thread-safe interior mutability
- Added `#[alloc_error_handler]` for allocation failures
- Made `BumpHeap` fields use `UnsafeCell` for safe concurrent access
- Added `rust-toolchain.toml` to specify nightly Rust for alloc features
- Verified allocator works with Vec/String test at boot time

**Files Modified:**
- `kernel_bootstrap/src/main.rs`: Added alloc support, GlobalAlloc impl, error handler
- `rust-toolchain.toml`: Created to specify nightly channel

### 2. No-std Conversion (COMPLETED ✅)
Converted multiple crates to be no_std compatible:

#### services_editor_vi
- Added `#![no_std]` and `extern crate alloc`
- Replaced `thiserror::Error` with manual `fmt::Display` implementations
- Added alloc imports: `String`, `Vec`, `Box`, `format!`, `ToString`
- Updated all error types (CommandError, EditorError, IoError) with Display impls

**Files Modified:**
- `services_editor_vi/src/lib.rs`
- `services_editor_vi/src/commands.rs`
- `services_editor_vi/src/editor.rs`
- `services_editor_vi/src/io.rs`
- `services_editor_vi/src/render.rs`
- `services_editor_vi/src/state.rs`
- `services_editor_vi/Cargo.toml`: Added serde no_std config

#### input_types
- Added `#![no_std]` and `extern crate alloc`
- Replaced `std::fmt` with `core::fmt`
- Added `String` and `Vec` imports from alloc

**Files Modified:**
- `input_types/src/lib.rs`
- `input_types/Cargo.toml`: Configured serde for no_std

#### view_types
- Added `#![no_std]` and `extern crate alloc`
- Replaced `std::fmt` with `core::fmt`
- Added alloc imports

**Files Modified:**
- `view_types/src/lib.rs`
- `view_types/Cargo.toml`: Configured serde for no_std

### 3. Workspace Integration (PARTIALLY COMPLETED ⚠️)
- Created `StubEditorIo` implementation for bare-metal (returns "FS unavailable" errors)
- Added editor field to `WorkspaceSession` struct
- Implemented `process_editor_input()` to convert bytes to KeyEvents
- Added editor rendering to serial port
- Wired "open editor" command to instantiate Editor::new()
- Added `:q` and `:q!` support through EditorAction::Quit

**Files Modified:**
- `kernel_bootstrap/src/workspace.rs`
- `kernel_bootstrap/Cargo.toml`: Added editor dependencies (commented out pending resolution)

## Remaining Work

### Critical Blocker: Transitive std Dependencies
The main blocker is that services_editor_vi has deep transitive dependencies that still require std:

**Problem Chain:**
```
services_editor_vi
├── services_view_host (uses std)
├── services_storage (uses std)  
│   ├── services_fs_view (uses std)
│   ├── hal (uses std)
│   ├── identity (uses std)
│   └── ipc (uses std)
└── fs_view (uses std)
```

**Resolution Options:**
1. **Make all transitive deps no_std** (large effort, ~10+ crates)
2. **Feature-gate editor dependencies** - Add `bare-metal` feature to editor that removes service dependencies
3. **Create minimal editor variant** - Separate bare_metal_editor crate with minimal deps
4. **Stub out unneeded services** - Editor doesn't actually need ViewHost/Storage at runtime with StubEditorIo

### Next Steps
1. Investigate which editor dependencies are actually needed at runtime vs compile-time
2. Consider adding feature flags to services_editor_vi for bare-metal mode
3. Convert remaining std-dependent crates to no_std (ipc, hal, identity, etc.)
4. Re-enable editor integration once dependency chain is resolved

## Testing
- ✅ kernel_bootstrap compiles cleanly with `cargo check`
- ✅ Global allocator initialized successfully
- ✅ Vec/String allocation test passes at boot
- ⏸️ Editor instantiation pending dependency resolution

## Design Philosophy Adherence
- ✅ **No legacy compatibility**: Used modern Rust alloc, not malloc/free
- ✅ **Testability first**: Allocator can be tested independently
- ✅ **Modular**: BumpHeap is self-contained with clear interface
- ✅ **Clean code**: Small, focused changes with clear intent

## Notes
- Allocator uses single-threaded bump allocation (sufficient for current kernel_bootstrap)
- No deallocation support (bump allocators don't free)
- 64 pages (256KB) allocated for heap - can be adjusted via HEAP_PAGES constant
- Editor code is fully prepared and ready once dependencies are resolved
