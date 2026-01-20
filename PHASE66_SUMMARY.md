# Phase 66: CLI Component

## Overview
Extracted and enhanced CLI commands from kernel_bootstrap into a proper reusable component backed by persistent storage.

## Changes Made

### 1. Enhanced CLI Commands (`cli_console/src/commands.rs`)
- **PersistentCommandHandler**: New handler that uses `PersistentFilesystem` as backend
  - `new()` / `open()`: Create or mount persistent filesystem
  - `ls()`: List directory contents from persistent storage
  - `cat()`: Read file contents as bytes
  - `mkdir()`: Create persistent directories
  - `write_file()`: Write and link files atomically
  - `rm()`: Remove files/directories (unlink operation)
  - `resolve_path()`: Path resolution (simplified, single-level for now)

- **CommandHandler**: Existing in-memory handler (kept for compatibility)
  - Uses `FileSystemViewService` for transient operations
  - Useful for testing and demos

### 2. Dependencies
- Added `hal` dependency to cli_console for BlockDevice access

### 3. Tests Added
- `test_persistent_handler_creation`: Verify filesystem initialization
- `test_persistent_mkdir_and_ls`: Directory creation and listing
- `test_persistent_write_and_cat`: File I/O roundtrip
- `test_persistent_rm`: File removal

All existing tests still pass (25 total tests).

## Design Decisions

### Why Two Handlers?
- **PersistentCommandHandler**: For real persistent storage (block devices)
- **CommandHandler**: For in-memory operations (testing, demos)

This follows PandaGen's principle: provide mechanism, let users choose policy.

### Path Resolution
Currently implements single-level path resolution (names in current directory only). Full path traversal (`/docs/notes/todo.txt`) deferred to future phases to keep this change minimal and surgical.

### Timestamp Handling
Uses a simple counter instead of real time, as we don't have a clock service yet. This is sufficient for demonstrating versioning semantics.

## Integration Points

### Workspace Integration (kernel_bootstrap)
The workspace.rs in kernel_bootstrap already has basic command handling:
- `open <editor|cli>`: Switch to component
- `list`: Show active components  
- `focus`: Focus switching (stub)
- `quit`: Close component

The PersistentCommandHandler can be instantiated in the workspace and called from `execute_command()`.

### Bare Metal Usage
Works in both hosted (sim_kernel with RamDisk) and bare metal (with real block device from HAL):

```rust
// Hosted
let disk = RamDisk::with_capacity_mb(10);
let mut cli = PersistentCommandHandler::new(disk, "user")?;

// Bare metal
let device = HardwareBlockDevice::detect()?;
let mut cli = PersistentCommandHandler::new(device, "user")?;
```

## Testing Strategy
- Unit tests with RamDisk verify all operations
- Integration with workspace tested manually (Phase 68)
- No external dependencies (kernel, filesystem, etc.)

## Known Limitations
- **No multi-level paths**: Only single directory level supported
- **No current directory switching**: Always operates on root
- **No symlinks**: Direct object references only
- **Simple timestamps**: Counter instead of real clock

## Next Steps (Phase 67)
Define system image layout:
- Kernel binary
- Services (storage, network, etc.)
- Components (CLI, editor)
- Package metadata

## Rationale
Traditional shells (bash, zsh) are monolithic and tightly coupled to POSIX semantics. PandaGen's CLI is:
1. **Modular**: Separate component with clear API
2. **Persistent**: Operations directly on block storage
3. **Testable**: Fast in-memory tests with RamDisk
4. **Typed**: Explicit object kinds (Blob, Log, Map)

This phase demonstrates that filesystem operations can be simple, explicit, and testable without POSIX baggage.
