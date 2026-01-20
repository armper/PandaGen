# Phase 65: Persistent Filesystem

## Overview
Integrated block-backed storage with filesystem semantics, providing persistent directory and file operations on top of the transactional storage layer.

## Changes Made

### 1. PersistentFilesystem Implementation (`services_storage/src/persistent_fs.rs`)
- **PersistentDirectory**: Hierarchical directory structure with parent links, metadata (timestamps, owner), and hash-based entry storage
- **DirectoryEntry**: Named entries pointing to objects (Blobs, Logs, Maps)
- **PersistentFilesystem**: High-level filesystem operations:
  - `format()` / `open()`: Initialize or mount filesystem
  - `mkdir()`: Create directories with automatic parent linking
  - `link()` / `unlink()`: Add/remove entries from directories
  - `list()`: Enumerate directory contents
  - `write_file()` / `read_file()`: Store and retrieve file content
  - `read_directory()` / `write_directory()`: Atomic directory updates

### 2. BlockStorage Enhancements (`services_storage/src/block_storage.rs`)
- **Latest Version Tracking**: Added `latest_versions` HashMap to track the most recent version of each object
- **read_object_data()**: New method to retrieve actual data given an ObjectId and VersionId
- **Fixed read()**: Returns the latest committed version instead of an arbitrary version
- **Versioning Semantics**: Every write creates a new version; reads always return the latest

### 3. API Corrections
- Fixed transaction mutability (`&mut Transaction` for write/commit operations)
- Proper use of BlockStorage methods with version tracking
- Consistent error handling using `TransactionError`

## Design Principles

### Why Not POSIX?
- **No inodes or paths**: Objects addressed by UUID, directories are just maps
- **Explicit versions**: Every modification creates a new version (like S3, not ext4)
- **Transactional**: All operations atomic via transactions
- **Typed objects**: Blob, Log, Map instead of generic "file"

### Testability
- All operations run in-memory via `RamDisk`
- Fast deterministic tests (34 tests, <20ms)
- No kernel dependencies

## Tests Added
- `test_format_and_root`: Basic filesystem creation
- `test_read_after_write`: Version persistence
- `test_mkdir`: Hierarchical directory creation
- `test_link_then_read`: Linking and reading entries
- `test_link_and_list`: Directory enumeration
- `test_write_read_file`: File I/O
- `test_unlink`: Entry removal

## Known Limitations
- **No garbage collection**: Old versions accumulate
- **No directory deletion**: Only unlinking entries supported
- **Simplified allocation**: Linear block allocation, no extents
- **No fsck**: Crash recovery is simplistic

## Next Steps (Phase 66)
Extract CLI commands from `kernel_bootstrap` into a real component that uses PersistentFilesystem:
- `open <file>`: Navigate to file
- `ls`: List directory
- `cat <file>`: Show file contents
- `rm <file>`: Remove file
- `focus`: Switch to editor

## Rationale
Traditional filesystems (ext4, NTFS) carry decades of POSIX baggage (inodes, dentries, path resolution). PandaGen's object-based approach provides:
1. **Simpler testing**: No mount points or kernel VFS layer
2. **Better versioning**: Every change is a new version by default
3. **Cleaner semantics**: Explicit object types and capabilities
4. **Modern abstractions**: More like S3 than Unix filesystem

This phase establishes the foundation for persistent storage without compromising PandaGen's core principle: mechanism (in kernel) vs policy (in user space).
