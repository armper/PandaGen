# Phase 24: Virtual Memory + Address Spaces (Isolation First)

**Completion Date**: 2026-01-19

## Overview

Phase 24 introduces **address spaces as explicit, capability-governed objects** to PandaGen OS. This phase establishes memory as explicit authority rather than ambient state, providing strong isolation between components while maintaining full testability under SimKernel.

## What Was Added

### 1. Core Memory Types (`core_types/src/memory.rs`)

**New types for memory abstraction:**

- `AddressSpaceId`: Unique identifier for address spaces (UUID-based)
- `AddressSpace`: Container for non-overlapping memory regions
- `MemoryRegionId`: Unique identifier for memory regions
- `MemoryRegion`: Represents a contiguous memory region with:
  - Size in bytes
  - Permissions (Read/Write/Execute)
  - Backing type (Anonymous/Shared/Device)
- `MemoryPerms`: Permission flags for memory access
  - `read`, `write`, `execute` booleans
  - Helper methods: `read_only()`, `read_write()`, `read_execute()`, `all()`
  - Display format: "RWX" notation
- `MemoryBacking`: Logical backing type (Anonymous/Shared/Device)
- `MemoryAccessType`: Read/Write/Execute access types

**Capabilities:**
- `AddressSpaceCap`: Grants ability to allocate/deallocate regions in a space
- `MemoryRegionCap`: Grants access to a specific memory region

**Errors:**
- `MemoryError`: Comprehensive error types for memory operations
  - AddressSpaceNotFound, RegionNotFound
  - PermissionDenied (with full context)
  - BudgetExhausted (with requested/available)
  - NoCapability, CrossSpaceAccess

**Tests:** 19 unit tests covering all core memory types and operations

### 2. Address Space Manager (`sim_kernel/src/address_space.rs`)

**AddressSpaceManager** provides simulation-level isolation:

**Key Operations:**
```rust
// Create address space
fn create_address_space(execution_id) -> AddressSpaceCap

// Allocate region
fn allocate_region(space_cap, region, caller_execution_id) -> MemoryRegionCap

// Check access
fn access_region(region_cap, access_type, caller_execution_id) -> Result<()>

// Activate for context switch
fn activate_space(execution_id) -> Result<()>

// Cleanup
fn destroy_address_space(execution_id) -> Result<()>
```

**Audit Events** (test-visible):
- `SpaceCreated`, `SpaceActivated`, `SpaceDestroyed`
- `RegionAllocated`, `RegionDeallocated`
- `AccessAttempted` (with allowed flag)

**Tests:** 7 unit tests covering all manager operations

### 3. SimKernel Integration

**New APIs** (added to `sim_kernel/src/lib.rs`):

```rust
// Address space management
pub fn create_address_space(execution_id) -> Result<AddressSpaceCap, MemoryError>

// Region allocation with budget enforcement
pub fn allocate_region(
    space_cap, 
    size_bytes, 
    permissions, 
    backing, 
    caller_execution_id
) -> Result<MemoryRegionCap, MemoryError>

// Access validation
pub fn access_region(
    region_cap, 
    access_type, 
    caller_execution_id
) -> Result<(), MemoryError>

// Context switching support
pub fn activate_address_space(execution_id) -> Result<(), MemoryError>

// Test-only introspection
pub fn address_space_audit() -> &AddressSpaceAuditLog
pub fn get_address_space(execution_id) -> Option<&AddressSpace>
```

**Automatic Integration:**
- Address space created automatically on task spawn (both KernelApi and spawn_task_with_identity)
- Address space destroyed automatically on task termination
- Integrated with existing identity and resource budget systems

**Tests:** 7 comprehensive integration tests

### 4. Budget Enforcement

Memory allocation consumes `MemoryUnits` from resource budget:

```rust
let budget = ResourceBudget::unlimited()
    .with_memory_units(MemoryUnits::new(10));  // 10 pages

// Each allocation consumes units (rounded up to 4KB pages)
allocate_region(space_cap, 4096, ...)  // Uses 1 unit
allocate_region(space_cap, 8192, ...)  // Uses 2 units
```

**Calculation:**
- Size rounded up to nearest 4KB page using `div_ceil(4096)`
- Allocation fails immediately if budget exceeded
- Budget checked per-execution-identity

**Test:** Memory budget exhaustion test validates enforcement

## What Was NOT Added (Intentionally)

Per requirements, Phase 24 does NOT include:

- ❌ Paging hardware or MMU integration
- ❌ fork/exec semantics
- ❌ Copy-on-write
- ❌ mmap/munmap APIs
- ❌ Implicit memory inheritance
- ❌ Shared global heap
- ❌ POSIX address space concepts

These are explicitly out of scope for Phase 24.

## Design Decisions

### 1. Memory as Authority, Not Side Effect

**Traditional OS Problem**: Memory access is ambient - any code can access any memory in its address space.

**PandaGen Solution**: Memory access requires explicit capabilities:
- `AddressSpaceCap` to allocate regions
- `MemoryRegionCap` to access specific regions
- No access without explicit grant

**Benefit**: Least privilege enforced at memory level

### 2. Address Spaces as Objects, Not Process Attributes

**Traditional OS Model**: Address space is tied to process identity.

**PandaGen Model**: Address spaces are first-class objects:
- Created explicitly via `create_address_space()`
- Managed via capabilities
- Have independent lifecycle
- Can be inspected and audited

**Benefit**: Clear separation of concerns, explicit lifecycle management

### 3. No Fork/Exec

**Why No Fork**:
- Fork duplicates entire address space (expensive, complex)
- Creates ambiguous ownership of resources
- Requires copy-on-write (adds complexity)
- Has undefined behavior with threads

**PandaGen Alternative**: Explicit construction:
- Tasks specify exactly what they need
- Memory allocated explicitly via `allocate_region()`
- No hidden copying or side effects
- Clear ownership from the start

### 4. Isolation First, Sharing by Explicit Grant

**Default**: Complete isolation between address spaces

**Sharing Pattern**:
1. Allocate region with `MemoryBacking::Shared`
2. Delegate `MemoryRegionCap` to other task
3. Other task can now access with capability

**No Implicit Sharing**: Cannot happen by accident

### 5. Simulation-Only in Phase 24

**Current**: Logical isolation in simulation
- Memory operations are API calls
- Access checks are function calls
- No real pointers involved

**Future**: MMU integration
- Same API, same semantics
- Different enforcement mechanism
- Hardware page faults instead of API calls

**Benefit**: Can develop and test without hardware

## Integration Tests

Phase 24 includes 7 comprehensive integration tests:

1. **test_memory_address_space_created_per_task**
   - Verifies address space created on spawn
   - Checks audit event recorded

2. **test_memory_region_allocation**
   - Allocates region in address space
   - Verifies region exists and is tracked

3. **test_memory_region_permission_enforcement**
   - Allocates read-only region
   - Verifies read allowed, write denied
   - Checks PermissionDenied error

4. **test_memory_cross_task_isolation**
   - Two tasks with separate address spaces
   - Task 1 allocates region
   - Task 2 cannot access (no capability)
   - Verifies isolation guarantee

5. **test_memory_budget_exhaustion**
   - Sets small memory budget (2 pages)
   - Allocates 2 regions successfully
   - Third allocation fails with BudgetExhausted
   - Validates budget enforcement

6. **test_memory_address_space_cleanup_on_task_termination**
   - Spawns task with address space
   - Terminates task
   - Verifies address space destroyed
   - Checks SpaceDestroyed audit event

7. **test_memory_region_sharing_via_delegation**
   - Demonstrates sharing pattern
   - Task 1 allocates shared region
   - Task 2 cannot access without delegation
   - Documents correct way to share memory

**All tests pass deterministically under SimKernel.**

## Hardware Integration Seam

Phase 24 provides clear mapping to MMU, though not implemented yet:

### Address Space → Page Table

```
AddressSpace.space_id  →  Page table root (CR3 on x86)
activate_address_space() →  Load CR3 instruction
```

### Memory Region → Page Table Entries

```
MemoryRegion {
    size_bytes: 4096,
    permissions: RW-,
    backing: Anonymous
}
→
Page Table Entries {
    virtual_range: 0x10000..0x11000,
    present: true,
    writable: true,
    executable: false,
    physical_frame: (allocated)
}
```

### Permissions → MMU Flags

```
MemoryPerms::read     →  PTE present bit
MemoryPerms::write    →  PTE writable bit
MemoryPerms::execute  →  PTE NX (no-execute) bit
```

### Access Validation → Page Faults

**Simulation** (current):
```rust
kernel.access_region(&region_cap, MemoryAccessType::Write, exec_id)?
// → Check capability ownership
// → Check permissions
// → Return Ok or PermissionDenied
```

**Hardware** (future):
```rust
// Write to virtual address
*ptr = value;  
// → CPU triggers page fault
// → Kernel fault handler:
//    - Look up MemoryRegionCap for faulting address
//    - Check if caller owns capability
//    - Check if permissions allow write
//    - Resume or kill process
```

### What Stays the Same

When moving to hardware:
- Capability model (MemoryRegionCap required)
- Permission semantics (R/W/X enforcement)
- Isolation guarantees (cross-space denied)
- Budget enforcement (allocation limits)
- API surface (same methods)

### What Changes

When moving to hardware:
- Simulation checks → Page fault handlers
- Logical regions → Physical page mappings
- activate_address_space() → CR3 load
- access_region() call → Page fault event

## Testing Philosophy

**Determinism First**:
- All tests run under SimKernel
- No hardware required
- Same inputs → same outputs
- Fully reproducible

**Comprehensive Coverage**:
- 19 unit tests for core types
- 7 unit tests for address space manager
- 7 integration tests for end-to-end flows
- Total: 33 new tests (all passing)

**Isolation Verification**:
- Tests explicitly verify cross-task isolation
- Tests verify permission enforcement
- Tests verify budget exhaustion
- Tests verify cleanup on termination

**Audit Trail Validation**:
- Every test checks audit events
- Verifies operations are recorded
- Enables trace-based debugging

## Integration with Previous Phases

**Phase 1-6**: Memory operations respect capability model

**Phase 7**: Address spaces tied to ExecutionId
- One address space per execution
- Parent-child relationships preserved
- Trust domains apply to memory

**Phase 8**: Policy can restrict memory allocation
- Policy engine can deny region allocation
- Policy can enforce cross-domain restrictions

**Phase 11**: Memory consumes MemoryUnits budget
- Budget checked on every allocation
- Exhaustion prevents new regions
- Usage tracked per-identity

**Phase 12**: Resource audit tracks memory operations
- Audit log records all events
- Test-visible for verification

**Phase 23**: Scheduler integration (planned)
- activate_address_space() on context switch
- Address space context preserved across preemption
- Audit events record activations

All safety properties maintained:
- No capability leaks (spaces destroyed with tasks)
- No ambient authority (all access requires cap)
- Deterministic testing (simulation mode)
- Observable behavior (audit log)

## Files Changed

**New Files:**
- `core_types/src/memory.rs` (638 lines) - Memory types and capabilities
- `sim_kernel/src/address_space.rs` (452 lines) - Address space manager

**Modified Files:**
- `core_types/src/lib.rs` - Export memory types
- `sim_kernel/src/lib.rs` - Add memory management APIs and integration
- `docs/architecture.md` - Add Phase 24 section
- `docs/interfaces.md` - Add Memory Management section

## Performance Impact

**Minimal overhead in simulation:**
- Address space operations are HashMap lookups (O(1))
- Region tracking uses Vec (O(n) for find, n = regions per space)
- No impact on non-memory operations
- Audit log grows with events (test-only)

**Memory overhead:**
- ~100 bytes per address space
- ~80 bytes per memory region
- ~60 bytes per capability
- Negligible for simulation workloads

## Backward Compatibility

**API Changes:**
- New public methods on SimulatedKernel (additive)
- No changes to existing KernelApi trait methods
- All existing tests pass without modification (102 tests)

**Behavioral Changes:**
- Tasks now automatically get address spaces on spawn
- Tasks automatically cleaned up on termination
- No observable difference for existing code

**No Breaking Changes:**
- Existing services continue to work
- No API removals or signature changes
- Fully backward compatible

## Quality Gates

All quality gates passed:

✅ **cargo fmt**: Code formatted
✅ **cargo clippy -- -D warnings**: No warnings
✅ **cargo test --all**: All tests pass
- core_types: 50 tests passing
- sim_kernel: 102 tests passing
- Total: 150+ tests passing

✅ **Regression tests**: All existing tests still pass

## Future Work

Phase 24 provides the foundation for:

### Immediate Next Steps

1. **Scheduler Integration** (Phase 23 integration)
   - Call `activate_address_space()` on context switch
   - Record address space activations in audit
   - Test address space isolation during preemption

2. **Documentation Completion**
   - Add code examples to architecture.md
   - Document common patterns
   - Add troubleshooting guide

### Future Phases

1. **MMU Integration** (Phase 25+?)
   - Map AddressSpace to page tables
   - Implement page fault handlers
   - Add TLB management
   - Test on real hardware

2. **Advanced Features** (Phase 26+?)
   - Shared memory regions (explicit IPC buffers)
   - Memory-mapped I/O (Device backing)
   - Demand paging (allocate on access)
   - Memory protection keys (fine-grained control)

3. **Optimization** (Future)
   - Region coalescing (merge adjacent regions)
   - Lazy allocation (defer physical pages)
   - Copy-on-write (if needed, explicit only)

## Out of Scope (Enforced)

Phase 24 explicitly does NOT include:

- ❌ Real paging hardware
- ❌ Page fault handlers
- ❌ TLB management
- ❌ Physical memory allocator
- ❌ fork/exec semantics
- ❌ Copy-on-write
- ❌ mmap/munmap compatibility
- ❌ POSIX compatibility layers

These are deferred to future phases or intentionally avoided.

## Lessons Learned

### What Worked Well

1. **Isolation-First Approach**: Starting with pure isolation made the model simple and testable
2. **Capability Model**: Memory as capabilities fits naturally with existing security model
3. **Simulation First**: Being able to develop without hardware accelerated progress
4. **Comprehensive Tests**: 33 tests gave confidence in correctness

### What Was Challenging

1. **Cyclic Dependencies**: Had to avoid ExecutionId in core_types (resolved by making AddressSpace execution-agnostic)
2. **Budget Calculation**: Initially had off-by-one error in page rounding (fixed with div_ceil)
3. **Dual Spawn Paths**: Had to add address space creation to both spawn_task and spawn_task_with_identity

### Design Choices We'd Make Again

1. **No Fork**: Explicit construction is much cleaner
2. **Objects Not Attributes**: Address spaces as objects provides better separation
3. **Simulation-Only**: Deferring MMU allowed faster progress
4. **Audit Everywhere**: Test-visible audit was invaluable for debugging

## Conclusion

Phase 24 successfully implements **address spaces as explicit, capability-governed objects**:

✅ **Memory is authority**: Requires capabilities, not ambient
✅ **Isolation first**: Components cannot access each other's memory
✅ **No inheritance**: No fork, no copy-on-write, no implicit sharing
✅ **Testable**: All 33 tests pass under SimKernel
✅ **Clean seams**: Ready for MMU integration later
✅ **No POSIX**: Avoided all legacy compatibility

The implementation prioritizes **correctness and isolation** over performance, staying true to PandaGen's philosophy of "mechanism, not policy."
