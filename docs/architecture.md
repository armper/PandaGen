# Architecture Overview

This document explains PandaGen's architecture, design decisions, and the reasoning behind them.

## Table of Contents

- [Core Principles](#core-principles)
- [System Layers](#system-layers)
- [Key Components](#key-components)
- [Design Patterns](#design-patterns)
- [Comparison with Traditional OS](#comparison-with-traditional-os)

## Core Principles

### 1. Testability First

**Problem**: Most operating system code is difficult to test because it was never designed with testing in mind.

**Solution**: 
- Separate mechanism from policy
- Make the kernel API a trait
- Provide a fully functional simulated kernel
- Most logic runs under `cargo test`

**Impact**:
- Fast, deterministic tests
- No hardware required for development
- Easy to reproduce bugs
- Continuous integration works out of the box

### 2. Capability-Based Security

**Problem**: Traditional OS security (UIDs, permissions) is ambient authority. Any code running as a user inherits all that user's privileges.

**Solution**:
- `Cap<T>` - strongly typed, unforgeable capabilities
- No ambient authority
- Explicit grant/transfer semantics with move-only default
- Automatic invalidation on owner termination
- Type system + runtime enforcement

**Impact**:
- Least privilege by default
- Can't accidentally inherit dangerous capabilities
- Fine-grained security without complexity
- Clear ownership model prevents confused deputy attacks

**Phase 3 Enhancements**:
- **Capability Lifecycle**: Explicit grant, delegate, drop, and invalidate operations
- **Move Semantics**: Capabilities transfer ownership (no implicit cloning)
- **Authority Table**: Kernel tracks capability ownership and validity
- **Audit Trail**: All capability operations logged for test verification
- **Automatic Cleanup**: Capabilities invalidated when owner task dies

**Example**:
```rust
// Grant capability to task
kernel.grant_capability(task_id, cap)?;

// Delegate with move semantics
kernel.delegate_capability(cap_id, from_task, to_task)?;
// from_task can NO LONGER use cap_id

// Automatic invalidation on crash
kernel.terminate_task(task_id);
// All capabilities owned by task_id are now invalid
```

### 3. Message Passing, Not Shared Memory

**Problem**: Shared memory leads to race conditions, undefined behavior, and hard-to-debug issues.

**Solution**:
- All IPC is message passing
- Messages are structured, versioned, and traceable
- No shared mutable state between tasks

**Impact**:
- Easier to reason about concurrency
- Natural fit for distributed systems
- Testable communication patterns

### 4. No Legacy Compatibility

**Problem**: Backward compatibility constrains design and perpetuates bad decisions.

**Solution**:
- Explicitly reject POSIX
- Design from first principles
- Allow innovation without compromise

**Impact**:
- Cleaner interfaces
- No historical baggage
- Free to make optimal choices

## System Layers

```
┌─────────────────────────────────────────┐
│         Applications / Services         │
│  (logger, storage, process_manager)     │
├─────────────────────────────────────────┤
│           Service Registry              │
│     (capability-based discovery)        │
├─────────────────────────────────────────┤
│              IPC Layer                  │
│      (typed message passing)            │
├─────────────────────────────────────────┤
│            Kernel API                   │
│      (trait-based interface)            │
├─────────────────────────────────────────┤
│       Kernel Implementation             │
│    (simulated or real hardware)         │
├─────────────────────────────────────────┤
│      Hardware Abstraction Layer         │
│      (CPU, memory, interrupts)          │
├─────────────────────────────────────────┤
│            Hardware                     │
└─────────────────────────────────────────┘
```

### Layer Responsibilities

**Applications/Services**
- Implement specific functionality
- Consume capabilities
- Send/receive messages
- Independent, replaceable

**Service Registry**
- Maps service IDs to channels
- Capability-based lookup
- No global namespace pollution

**IPC Layer**
- Message envelope structure
- Schema versioning
- Correlation IDs
- Type-erased transport

**Kernel API**
- Task spawning
- Channel creation
- Message send/receive
- Time management
- Capability management

**Kernel Implementation**
- Scheduling (not yet implemented)
- Memory management (not yet implemented)
- Hardware interaction
- Resource accounting

**HAL**
- CPU operations
- Memory operations
- Interrupt handling
- Architecture-specific details

## Key Components

### Core Types (`core_types`)

**Purpose**: Fundamental types used throughout the system.

**Key Types**:
- `Cap<T>`: Strongly-typed capability handle
- `ServiceId`: Unique service identifier
- `TaskId`: Unique task identifier

**Design**:
- Zero-cost abstractions (newtype pattern)
- Type safety via phantom types
- Cannot forge capabilities

### IPC (`ipc`)

**Purpose**: Message passing primitives.

**Key Types**:
- `MessageEnvelope`: Routing and metadata
- `MessagePayload`: Type-erased data
- `SchemaVersion`: Compatibility tracking
- `ChannelId`: Communication endpoints

**Design**:
- Structured, not byte streams
- Versioned for evolution
- Traceable via correlation IDs

### Kernel API (`kernel_api`)

**Purpose**: Interface between user space and kernel.

**Key Trait**: `KernelApi`

**Operations**:
- `spawn_task()`: Create new tasks
- `create_channel()`: IPC setup
- `send_message()`: Non-blocking send
- `receive_message()`: Blocking receive
- `now()`: Current time
- `sleep()`: Yield with timeout
- `grant_capability()`: Transfer authority
- `register_service()`: Make discoverable
- `lookup_service()`: Find services

**Design**:
- Trait-based (multiple implementations)
- No ambient authority
- Explicit time (testable)

### Simulated Kernel (`sim_kernel`)

**Purpose**: Full kernel implementation for testing.

**Features**:
- Runs in-process
- Controlled time
- Inspectable state
- Deterministic

**Not a Mock**: This is a real implementation of `KernelApi`, just optimized for testing rather than hardware.

### HAL (`hal`, `hal_x86_64`)

**Purpose**: Abstract hardware details.

**Traits**:
- `CpuHal`: CPU operations
- `MemoryHal`: Memory management
- `InterruptHal`: Interrupt handling

**Design**:
- No architecture leakage
- Fully swappable
- x86_64 is one implementation, not the only one

### Storage (`services_storage`)

**Purpose**: Rethink storage from first principles.

**Concepts**:
- `ObjectId`: Not paths
- `VersionId`: Every change is versioned
- `ObjectKind`: Blob, Log, or Map
- `Transaction`: Atomic operations

**Not a Filesystem**: No hierarchy, no paths, no inodes.

### Process Manager (`services_process_manager`)

**Purpose**: Service lifecycle management.

**Concepts**:
- `ServiceDescriptor`: What to run
- `LifecycleState`: Current state
- `RestartPolicy`: Failure handling

**Not Init**: No shell scripts, explicit policies.

## Design Patterns

### 1. Trait-Based Interfaces

**Why**: Enable multiple implementations, testing, and flexibility.

**Example**: `KernelApi` trait can be implemented by:
- `SimulatedKernel` (testing)
- Real kernel (hardware)
- Remote kernel (distributed)

### 2. Type-Safe Handles

**Why**: Prevent confusion between different resource types.

**Example**: `ServiceId`, `TaskId`, `ChannelId` are distinct types, even though they're all UUIDs internally.

### 3. Explicit Construction

**Why**: No hidden state or ambient authority.

**Example**: `TaskDescriptor` specifies everything about a task before spawning it.

### 4. Message Passing

**Why**: Avoid shared mutable state.

**Example**: All IPC uses `MessageEnvelope` with typed payloads.

### 5. Capability Passing

**Why**: Authority is data, not ambient.

**Example**: `Cap<T>` can be transferred in messages.

## Comparison with Traditional OS

| Aspect | Traditional (POSIX) | PandaGen |
|--------|-------------------|----------|
| Process creation | `fork()` duplicates everything | `spawn_task()` constructs explicitly |
| IPC | Pipes, sockets, shared memory | Typed message passing |
| Security | UIDs, file permissions | Capabilities |
| Storage | Path-based filesystem | Versioned objects with IDs |
| Commands | Text-based shell | Typed intents |
| Time | Ambient, global | Explicit, controllable |
| Inheritance | Implicit (fork inherits) | Explicit (grant capabilities) |
| Testing | Difficult (needs hardware) | Easy (simulated kernel) |

## Rationale for Key Decisions

### Why No Fork?

**Problem**: `fork()` duplicates process state unpredictably. Memory, file descriptors, threads, locks all copied in complex ways.

**Solution**: Explicit construction. Specify exactly what the new task needs.

**Benefit**: Clearer semantics, easier to reason about.

### Why No Filesystem Paths?

**Problem**: Paths are stringly-typed, hierarchical structure is often wrong, permissions are complex.

**Solution**: Objects have typed IDs. No hierarchy imposed.

**Benefit**: Simpler, more flexible, version-friendly.

### Why Typed Messages?

**Problem**: Byte streams (pipes) lose type information. Debugging is hard.

**Solution**: Structured messages with schema versions.

**Benefit**: Type safety, evolution support, better debugging.

### Why Simulated Kernel?

**Problem**: Testing real kernel code requires hardware, slow cycles, difficult debugging.

**Solution**: Make the kernel API a trait. Implement it in pure Rust for testing.

**Benefit**: Fast tests, no hardware needed, full debugging support.

## Future Directions

### Resilience and Fault Injection

**Phase 2 (Current)**: Deterministic fault injection framework integrated into SimulatedKernel.

The system now includes:
- **Fault Plans**: Composable, deterministic fault injection for testing
- **Message Faults**: Drop, delay, and reorder messages predictably
- **Lifecycle Faults**: Simulate service crashes at specific points
- **Test Utilities**: Helpers for writing resilience tests (`run_until_idle`, `with_fault_plan`)

**Philosophy**:
- Testability is a first-class design constraint
- Failures must be tested, not just success paths
- Deterministic testing (no flaky tests from randomness)
- Safety properties must hold even under faults

**Resilience Testing Approach**:

Tests validate that the system maintains invariants under failure:
1. **Capability Non-Leak**: Capabilities cannot be used after crash/revocation
2. **Storage Consistency**: No partial commits or corruption after crash
3. **Registry Consistency**: Service registry remains coherent through restarts
4. **Restart Correctness**: Services restart according to policy

The fault injection framework enables:
- Testing message loss scenarios (at-most-once semantics)
- Validating crash recovery procedures
- Ensuring no undefined behavior under faults
- Proving safety properties hold under adversarial conditions

**Usage Example**:
```rust
use sim_kernel::fault_injection::{FaultPlan, MessageFault};
use sim_kernel::test_utils::with_fault_plan;

let plan = FaultPlan::new()
    .with_message_fault(MessageFault::DropNext { count: 2 })
    .with_lifecycle_fault(LifecycleFault::CrashAfterMessages { count: 5 });

with_fault_plan(plan, |kernel| {
    // Test system behavior under faults
});
```

### Phase 3: Capability Lifecycle and Delegation Semantics

**Phase 3 (Current)**: Hardened capability security contract with lifecycle tracking and audit.

The system now includes:
- **Capability Lifecycle Model**: Explicit grant, delegate, drop, and invalidate operations
- **Move Semantics**: Capabilities use move-only transfer (no implicit cloning)
- **Authority Table**: Kernel maintains ownership and validity state for all capabilities
- **Audit Trail**: Comprehensive logging of capability operations (test/simulation mode)
- **Automatic Invalidation**: Capabilities invalidated when owner task terminates

**Philosophy**:
- Explicit over implicit (no ambient authority, no automatic inheritance)
- Capabilities over permissions (unforgeable tokens, not UIDs)
- Testability first (audit log for verification)
- Mechanism not policy (kernel provides primitives, services decide policy)
- No confusion (clear ownership, move semantics prevent aliasing)

**Capability Lifecycle Operations**:

1. **Grant**: Kernel issues capability to a task
   ```rust
   kernel.grant_capability(task_id, cap)?;
   // Creates authority table entry: cap_id -> task_id (Valid)
   ```

2. **Delegate**: Transfer ownership between tasks (move semantics)
   ```rust
   kernel.delegate_capability(cap_id, from_task, to_task)?;
   // from_task loses access, to_task gains access
   // Authority table updated: cap_id -> to_task
   ```

3. **Drop**: Explicit release
   ```rust
   kernel.drop_capability(cap_id, task_id)?;
   // Capability marked Invalid
   ```

4. **Invalidate**: Automatic on task death
   ```rust
   kernel.terminate_task(task_id);
   // All capabilities owned by task_id marked Invalid
   ```

**Enforcement Model**:

SimulatedKernel enforces:
- **Ownership validation**: Every operation checks authority table
- **Liveness checking**: Validates owner task is still alive
- **Move semantics**: After delegation, original owner cannot use capability
- **Type safety**: Compile-time type checking + runtime ownership checks

Tests validate:
- No capability use after transfer (move semantics work)
- No capability use after owner death (automatic invalidation)
- No capability leaks through message faults (resilience)
- Delegation chains work correctly (A→B→C)
- Audit trail accurately reflects operations

**Example Test**:
```rust
#[test]
fn test_capability_move_semantics() {
    let mut kernel = SimulatedKernel::new();
    let task1 = kernel.spawn_task(...).task_id;
    let task2 = kernel.spawn_task(...).task_id;
    
    // Grant to task1
    kernel.grant_capability(task1, Cap::new(42))?;
    assert!(kernel.is_capability_valid(42, task1));
    
    // Delegate to task2
    kernel.delegate_capability(42, task1, task2)?;
    
    // Move semantics: task1 can no longer use it
    assert!(!kernel.is_capability_valid(42, task1));
    assert!(kernel.is_capability_valid(42, task2));
}
```

**Audit Trail**:

The capability audit log (test-only) records:
- Timestamp (simulated time)
- Event type (Granted, Delegated, Dropped, Invalidated, InvalidUseAttempt)
- Actor identities (grantor, grantee, from/to tasks)
- Capability ID and type

Tests query the audit log to verify security properties:
```rust
let audit = kernel.audit_log();

// No unexpected delegations
assert!(!audit.has_event(|e| matches!(e, CapabilityEvent::Delegated { to_task: untrusted, .. })));

// All capabilities properly invalidated
let invalid_count = audit.count_events(|e| matches!(e, CapabilityEvent::Invalidated { .. }));
assert_eq!(invalid_count, expected_count);
```

**Design Rationale**:

**Why move-only semantics?**
- Prevents confused deputy attacks (only one task can act with a capability)
- Clear ownership model (no ambiguity about who has authority)
- Easier to reason about (no aliasing)
- Matches Rust's ownership semantics (feels natural to developers)

**Why automatic invalidation?**
- Prevents use-after-free of authority
- No manual cleanup needed in most cases
- Natural fit for crash recovery (no dangling capabilities)
- Testable invariant (all dead tasks have invalid capabilities)

**Why no revocation (yet)?**
- Revocation requires policy decisions (who can revoke? under what conditions?)
- Current model focuses on mechanism (grant, delegate, drop)
- Future: explicit revocation API if needed, with clear policy hooks

**Future Real Kernel**:
- Authority table in kernel space (user cannot forge entries)
- Capability IDs cryptographically unforgeable (not just u64)
- Hardware memory protection prevents capability inspection/modification
- Same semantics as SimulatedKernel, proven by tests

### Phase 4: Interface Evolution Discipline

**Phase 4 (Current)**: Disciplined evolution model for IPC and storage schemas.

The system now includes:
- **IPC Schema Evolution Policy**: Clear rules for breaking vs non-breaking changes
- **Version Policy Enforcement**: Type-safe version checking with explicit errors
- **Service Contract Tests**: Golden tests that prevent accidental interface drift
- **Storage Schema Evolution**: Identity, versioning, and migration hooks for durable objects

**Philosophy**:
- **Explicit over implicit**: Version policies are written in code, not conventions
- **Testability first**: Contract tests catch breaking changes before deployment
- **Modularity first**: Services evolve independently within version contracts
- **Mechanism not policy**: Core provides versioning primitives, services define policies
- **No ossification**: Bounded compatibility prevents accumulating legacy baggage

**Evolution Without Legacy Thinking**:

Traditional systems struggle with evolution because backward compatibility becomes:
- A constraint that prevents improvement
- A source of complexity (compatibility layers, shims)
- A maintenance burden (supporting ancient versions forever)
- An accumulation of technical debt

PandaGen takes a different approach:
- **Bounded compatibility**: Support N and N-1, explicitly reject older versions
- **Explicit version checks**: No silent failures or undefined behavior
- **Contract testing**: Catch breaking changes in CI, not production
- **Graceful migration**: Clear error messages guide upgrades
- **Test-first evolution**: Version handling is tested like any other feature

**IPC Schema Evolution Model**:

1. **Schema Versioning**:
   ```rust
   pub struct SchemaVersion {
       pub major: u32,  // Breaking changes
       pub minor: u32,  // Backward-compatible additions
   }
   ```

2. **Version Policy**:
   ```rust
   let policy = VersionPolicy::current(3, 0)
       .with_min_major(2);  // Support v2.x and v3.x
   
   match policy.check_compatibility(&incoming_version) {
       Compatibility::Compatible => { /* handle */ }
       Compatibility::UpgradeRequired => { /* error */ }
       Compatibility::Unsupported => { /* error */ }
   }
   ```

3. **Explicit Error Handling**:
   - Schema mismatch returns detailed error with versions and service identity
   - Sender knows exactly what went wrong and how to fix it
   - No silent failures or mysterious deserialization errors

**Storage Schema Evolution Model**:

Storage objects evolve over time. Each object has:
- **Schema Identity**: What type of object (e.g., "user-profile", "audit-event")
- **Schema Version**: Which version of that schema
- **Migration Path**: How to transform old versions to new versions

```rust
pub struct ObjectSchemaId(String);
pub struct ObjectSchemaVersion(u32);

// Objects carry schema metadata
pub struct StoredObject {
    pub id: ObjectId,
    pub version: VersionId,
    pub schema_id: ObjectSchemaId,
    pub schema_version: ObjectSchemaVersion,
    pub data: Vec<u8>,
}

// Migration is a pure function
pub trait Migrator {
    fn migrate(
        &self,
        from_version: ObjectSchemaVersion,
        to_version: ObjectSchemaVersion,
        data: &[u8],
    ) -> Result<Vec<u8>, MigrationError>;
}
```

**Key Properties**:
- Migrations are deterministic (same input → same output)
- Migrations are testable (pure functions, no side effects)
- Old versions remain accessible (version immutability)
- Schema identity is explicit, not inferred from structure

**Service Contract Testing**:

Contract tests act as "golden" tests for service interfaces:
- Define canonical message structures for each service operation
- Fail CI if envelope fields change unexpectedly
- Fail CI if schema versions change without intentional update
- Fail CI if action identifiers drift

Example:
```rust
#[test]
fn test_registry_register_contract() {
    // This test ensures the "register" action contract stays stable
    let request = RegistryRegisterRequest {
        service_id: ServiceId::new(),
        channel: ChannelId::new(),
    };
    
    let envelope = MessageEnvelope::new(
        registry_service_id(),
        "registry.register".to_string(),
        SchemaVersion::new(1, 0),
        MessagePayload::new(&request).unwrap(),
    );
    
    // If these assertions fail, it's a breaking change
    assert_eq!(envelope.action, "registry.register");
    assert_eq!(envelope.schema_version.major, 1);
}
```

**Why This Matters**:

Evolution is a first-class design concern:
- Systems don't stay static - they grow, change, adapt
- Without discipline, evolution leads to fragmentation and breakage
- With discipline, evolution is controlled, testable, and safe

PandaGen proves that you can evolve without ossifying:
- Explicit version policies prevent surprise breakage
- Contract tests catch drift before it reaches production
- Bounded compatibility avoids legacy accumulation
- Clear errors make debugging straightforward

This is **evolution as a feature**, not evolution as technical debt.

### Performance

Currently optimized for clarity, not performance. Future work:
- Zero-copy message passing
- Lock-free data structures
- NUMA-aware scheduling

### Real Hardware

Simulated kernel proves the design. Next steps:
- Bootloader integration
- Real HAL implementations
- Interrupt handling
- DMA support

### Distributed

Message-passing design is inherently distributed-friendly:
- Kernel API over network
- Transparent remote services
- Capability delegation

## Conclusion

PandaGen is an experiment: **What if we designed an OS with modern software engineering principles?**

The answer:
- ✅ More testable
- ✅ More secure (capabilities)
- ✅ More modular
- ✅ Clearer interfaces
- ❌ No backward compatibility
- ❌ Not production-ready (yet)

This architecture proves that rejecting legacy constraints enables better design.
