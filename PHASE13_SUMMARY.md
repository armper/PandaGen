# Phase 13 — Filesystem Illusion Service

## Summary

This phase introduces a **filesystem illusion** as a user-space service that provides a familiar directory-like interface while maintaining PandaGen's capability-based security model.

## What Was Implemented

### 1. Core `fs_view` Crate

A library providing the fundamental concepts for filesystem views:

- **Directory Model**: Directories are represented as `Map` objects with entries mapping names to object capabilities
- **Path Resolution**: Clean, testable path parsing and validation logic
- **No Global State**: Each view is isolated with its own root capability

**Key Files:**
- `fs_view/src/path.rs`: Path parsing and validation
- `fs_view/src/directory.rs`: Directory and entry data structures
- `fs_view/src/lib.rs`: Public API exports

**Tests:** 15 unit tests covering path parsing, edge cases, and validation

### 2. Service `services_fs_view` Crate

A service implementation providing filesystem operations:

- **Operations**: `ls`, `stat`, `open`, `mkdir`, `link`, `unlink`
- **Capability-Driven**: All operations require explicit capabilities
- **Immutability Preserved**: Unlinking names doesn't delete objects
- **No Authority Escalation**: Service never grants access not already held

**Key Files:**
- `services_fs_view/src/operations.rs`: Operation trait definitions
- `services_fs_view/src/service.rs`: Service implementation
- `services_fs_view/tests/integration_tests.rs`: Integration tests

**Tests:** 13 unit tests + 9 integration tests

### 3. CLI Integration

Extended `cli_console` with filesystem commands:

- `pg ls <path>`: List directory contents
- `pg cat <path>`: Display object information (stub)
- `pg mkdir <path>`: Create directories
- `pg link <path> <obj_id>`: Create name → object links

**Key Files:**
- `cli_console/src/commands.rs`: Command handler implementation

**Tests:** 9 tests covering command operations

## Design Principles Maintained

### ✅ No Global Filesystem
- Each user/component can have different root directories
- No ambient authority via global paths
- No kernel-level path resolution

### ✅ Capability-Based Security
- Cannot traverse into directories without their capability
- Cannot name objects you weren't explicitly given
- Path resolution never creates new authority

### ✅ Immutability
- Unlinking removes the name → object mapping
- The underlying object remains unchanged
- Storage objects are still immutable and versioned

### ✅ Testability
- All code runs under `cargo test`
- Comprehensive test coverage (46 tests total)
- No external dependencies required

## Key Architectural Decisions

1. **Directories are Map Objects**: Leverages existing storage primitives rather than inventing new ones

2. **No Relative Paths**: Explicitly reject `.` and `..` to avoid confusion and potential security issues

3. **Service-Managed Registry**: The service maintains a registry of directories to enable traversal while keeping capability safety

4. **Separate Concerns**: `fs_view` handles pure logic, `services_fs_view` handles service operations, `cli_console` handles user interface

## Test Coverage

| Component | Unit Tests | Integration Tests | Total |
|-----------|-----------|-------------------|-------|
| fs_view | 15 | - | 15 |
| services_fs_view | 13 | 9 | 22 |
| cli_console | 9 | - | 9 |
| **Total** | **37** | **9** | **46** |

## What This Enables

- **Human-Friendly Interface**: Users can think in terms of paths and directories
- **Capability Safety**: System maintains strict authority checking
- **Flexibility**: Different users can have completely different filesystem views
- **Compatibility Path**: Future services can build on this foundation

## What This Does NOT Do

- ❌ Add paths to KernelApi
- ❌ Introduce a global root (`/`)
- ❌ Add POSIX semantics
- ❌ Bypass capability checks
- ❌ Store mutable global state

## Future Extensions

1. **Symbolic Links**: Could be added as a special entry type
2. **Mount Points**: Different storage backends for different subtrees
3. **Virtual Files**: Dynamic content generation (like `/proc` in Linux)
4. **Permission Metadata**: Additional access control layers
5. **Search/Indexing**: Efficient path-based lookups at scale

## Usage Example

```rust
use fs_view::DirectoryView;
use services_fs_view::{FileSystemOperations, FileSystemViewService};
use services_storage::{ObjectId, ObjectKind};

// Create service and root directory
let mut service = FileSystemViewService::new();
let root_id = ObjectId::new();
let mut root = DirectoryView::new(root_id);

// Create directory structure
service.mkdir(&mut root, "docs").unwrap();

// Link a file
let file_id = ObjectId::new();
service.link(&mut root, "docs/readme.txt", file_id, ObjectKind::Blob).unwrap();

// Open the file
let opened = service.open(&root, "docs/readme.txt").unwrap();
assert_eq!(opened, file_id);

// List directory
let entries = service.ls(&root, "docs").unwrap();
println!("Files: {:?}", entries);
```

## Validation

All requirements from the problem statement have been met:

- ✅ Filesystem View Core with directory model, root concept, and path resolution
- ✅ Filesystem View Service with all required operations
- ✅ Capability safety rules enforced and tested
- ✅ CLI integration for basic commands
- ✅ Comprehensive test suite
- ✅ No global state introduced
- ✅ All operations respect capability model
- ✅ Code quality verified (clippy, fmt, tests pass)
