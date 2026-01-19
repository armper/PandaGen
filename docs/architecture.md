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

### Phase 5: Typed Intent Pipelines, Composition Semantics, and Failure Propagation

**Phase 5 (Current)**: Safe composition of typed operations with explicit failure handling.

The system now includes:
- **Typed Pipeline System**: Compose handler stages with schema-validated input/output chaining
- **Explicit Capability Flow**: Track capabilities through pipeline stages without ambient authority
- **Bounded Failure Semantics**: Explicit retry policies with deterministic backoff (no infinite loops)
- **Execution Tracing**: Minimal, test-visible traces of stage execution and capability flow
- **Fail-Fast Behavior**: Pipelines stop at first non-retryable failure

**Philosophy**:
- **Explicit over implicit**: Failure modes are explicit, not hidden in abstraction
- **Testability first**: Pipelines work deterministically with SimKernel + fault injection
- **Modularity first**: Each stage is independent and composable
- **Mechanism not policy**: Kernel provides primitives, services orchestrate
- **Capabilities over ambient authority**: No capability leaks through composition
- **No legacy compatibility**: Not POSIX pipes, not shell pipelines, not stringly-typed

**Typed Intent Pipelines**:

Traditional shells compose commands via text pipes (`cmd1 | cmd2`). This is:
- Stringly-typed (all data becomes text)
- Error-prone (silent failures, no type checking)
- Ambient authority (commands inherit all privileges)
- No structured failure handling

PandaGen pipelines are fundamentally different:

1. **Typed Composition**: Each stage declares input/output schemas
   ```rust
   let stage1 = StageSpec::new(
       "CreateBlob",
       handler_service,
       "create",
       PayloadSchemaId::new("blob_params"),
       PayloadSchemaId::new("blob_capability"),
   );
   
   let stage2 = StageSpec::new(
       "TransformBlob",
       transformer_service,
       "transform",
       PayloadSchemaId::new("blob_capability"), // Must match stage1 output
       PayloadSchemaId::new("transformed_blob"),
   );
   ```

2. **Schema Validation**: Pipeline validates schema chaining at construction time
   - First stage input must match pipeline input
   - Each stage output must match next stage input
   - Last stage output must match pipeline output
   - Compilation-time and runtime validation

3. **Explicit Capability Flow**:
   ```rust
   stage2.with_capabilities(vec![cap_from_stage1]);
   // Stage 2 explicitly requires a capability produced by stage 1
   // Executor validates capability availability before execution
   ```

4. **Bounded Retry Policies**:
   ```rust
   stage.with_retry_policy(RetryPolicy::exponential_backoff(3, 100));
   // Max 3 retries, 100ms initial backoff, exponentially increasing
   // No infinite retries - ever
   ```

**Failure Semantics**:

Every stage returns one of three outcomes:
- `Success { output, capabilities }` - Stage succeeded, pipeline continues
- `Failure { error }` - Permanent failure, pipeline stops (fail-fast)
- `Retryable { error }` - Transient failure, retry with backoff

Pipeline execution rules:
- **Fail-Fast**: First permanent failure stops the entire pipeline
- **Bounded Retries**: Retryable stages retry up to `max_retries`, then convert to permanent failure
- **Deterministic Backoff**: Uses SimKernel time for reproducible retry timing
- **No Hidden State**: All failures are explicit in execution trace

**Execution Trace**:

Pipelines record a minimal trace for testing:
```rust
struct StageTraceEntry {
    stage_id: StageId,
    stage_name: String,
    start_time_ms: u64,      // Deterministic SimKernel time
    end_time_ms: u64,
    attempt: u32,            // 0 for first attempt, increments on retry
    result: StageExecutionResult,
    capabilities_in: Vec<u64>,  // Caps required by this stage
    capabilities_out: Vec<u64>, // Caps produced by this stage
}
```

This is NOT a production observability platform. It's:
- Minimal (stage boundaries, timestamps, cap IDs only)
- Test-visible (assertions can query trace)
- Deterministic (replay-able under SimKernel)

**Why This Matters**:

Composition is the heart of building complex systems:
- Without safe composition, systems become monolithic
- Without typed composition, systems become fragile
- Without explicit failure handling, systems become unpredictable

PandaGen proves that composition can be:
- **Type-safe**: Schemas validate at construction time
- **Capability-safe**: No authority leaks through stages
- **Failure-safe**: Bounded retries prevent infinite loops
- **Test-safe**: Deterministic execution under faults

This is **composition as a first-class feature**, not composition as an afterthought.

**Example: Three-Stage Blob Pipeline**:

```rust
// Stage 1: Create a blob in storage
let create_stage = StageSpec::new(
    "CreateBlob",
    storage_service_id,
    "create_blob",
    PayloadSchemaId::new("create_blob_input"),
    PayloadSchemaId::new("create_blob_output"),
);

// Stage 2: Transform blob (e.g., uppercase)
// Requires capability from stage 1
let transform_stage = StageSpec::new(
    "TransformBlob",
    transformer_service_id,
    "transform",
    PayloadSchemaId::new("create_blob_output"),  // Chained from stage 1
    PayloadSchemaId::new("transform_blob_output"),
).with_capabilities(vec![blob_cap_id_from_stage1])
 .with_retry_policy(RetryPolicy::fixed_retries(2, 50));

// Stage 3: Annotate with metadata
// Requires capability from stage 2
let annotate_stage = StageSpec::new(
    "AnnotateMetadata",
    metadata_service_id,
    "annotate",
    PayloadSchemaId::new("transform_blob_output"), // Chained from stage 2
    PayloadSchemaId::new("annotate_metadata_output"),
).with_capabilities(vec![transformed_cap_id_from_stage2]);

// Compose into pipeline
let pipeline = PipelineSpec::new(
    "blob_processing_pipeline",
    PayloadSchemaId::new("create_blob_input"),
    PayloadSchemaId::new("annotate_metadata_output"),
)
.add_stage(create_stage)
.add_stage(transform_stage)
.add_stage(annotate_stage);

// Validate schema chaining
pipeline.validate()?;

// Execute
let executor = PipelineExecutor::new();
executor.add_capabilities(initial_caps);
let (final_output, trace) = executor.execute(&mut kernel, &pipeline, input)?;

// Verify execution
assert_eq!(trace.entries.len(), 3);
assert_eq!(trace.final_result, PipelineExecutionResult::Success);
```

**Integration with Prior Phases**:

Phase 5 builds on all previous phases:
- **Phase 1**: Uses KernelApi, IPC, and service framework
- **Phase 2**: Works with fault injection (message drop/delay/reorder)
- **Phase 3**: Enforces capability lifecycle (no leaks through stages)
- **Phase 4**: Respects schema versioning and migration rules

Pipelines maintain all safety properties even under faults:
- No capability use after transfer (move semantics)
- No capability leaks through dropped messages
- No double-commit in storage (fail-fast semantics)
- Storage immutability and lineage preserved

### Phase 6: Deterministic Cancellation, Timeouts, and Structured Lifecycle

**Phase 6 (Current)**: Explicit, testable cancellation and timeout primitives for controlled operation lifecycles.

The system now includes:
- **Lifecycle Crate**: CancellationToken, CancellationSource, Deadline, and Timeout types
- **Pipeline Cancellation**: Per-pipeline and per-stage timeout support with explicit cancellation checks
- **Intent Handler Pattern**: Documented pattern for handlers to check cancellation at safe points
- **Capability Safety**: No capability leaks on cancellation (capabilities only committed on success)
- **Deterministic Behavior**: All cancellation and timeout logic uses SimKernel time

**Philosophy**:
- **Explicit over implicit**: Cancellation requires explicit token, never automatic
- **Testability first**: Deterministic time, reproducible behavior, comprehensive tests
- **Mechanism not policy**: Kernel provides primitives, services decide when to cancel
- **Type safe**: Cancelled is distinct from Failed, compiler enforces handling
- **No POSIX concepts**: Not signals, not EINTR - structured, explicit cancellation

**Cancellation Model**:

Operations can be cancelled through:

1. **CancellationToken**: Cloneable handle for checking cancellation status
   ```rust
   let source = CancellationSource::new();
   let token = source.token();
   
   // Later, from any context:
   source.cancel(CancellationReason::UserCancel);
   
   // All tokens see the cancellation:
   assert!(token.is_cancelled());
   ```

2. **CancellationReason**: Explicit reason enum
   - `UserCancel`: User-initiated
   - `Timeout`: Deadline exceeded
   - `SupervisorCancel`: Orchestrator/parent cancelled
   - `DependencyFailed`: Cascade cancellation
   - `Custom(String)`: Domain-specific reasons

3. **Deadline/Timeout**: Deterministic time-based cancellation
   ```rust
   let timeout = Timeout::from_secs(5);
   let deadline = timeout.to_deadline(kernel.now());
   
   if deadline.has_passed(kernel.now()) {
       // Timeout occurred
   }
   ```

**Pipeline Integration**:

Pipelines support cancellation at multiple levels:

1. **Overall Pipeline Timeout**:
   ```rust
   let pipeline = PipelineSpec::new(...)
       .add_stage(stage1)
       .add_stage(stage2)
       .with_timeout_ms(5000); // 5 second overall deadline
   ```

2. **Per-Stage Timeout**:
   ```rust
   let stage = StageSpec::new(...)
       .with_timeout_ms(1000); // 1 second for this stage
   ```

3. **Explicit Cancellation**:
   ```rust
   let source = CancellationSource::new();
   let token = source.token();
   
   // Start pipeline execution
   let result = executor.execute(&mut kernel, &pipeline, input, token);
   
   // Can cancel from another context:
   source.cancel(CancellationReason::SupervisorCancel);
   ```

**Cancellation Propagation**:

The pipeline executor checks cancellation at key points:
- Before pipeline starts
- Before each stage execution
- Before each retry attempt
- Against pipeline and stage deadlines

```rust
// Executor checks before each stage:
if cancellation_token.is_cancelled() {
    trace.set_final_result(PipelineExecutionResult::Cancelled {
        stage_name: stage.name.clone(),
        reason: cancellation_token.reason().to_string(),
    });
    return Err(...);
}
```

**Intent Handler Pattern**:

Handlers should check cancellation at safe points:

```rust
fn handle_storage_write(
    intent: &Intent,
    cancellation_token: &CancellationToken,
) -> Result<IntentResult, IntentError> {
    // Check cancellation before starting
    cancellation_token.throw_if_cancelled()?;
    
    // Do preparatory work
    let data = prepare_data(intent)?;
    
    // Check again before expensive operation
    cancellation_token.throw_if_cancelled()?;
    
    // Perform write
    write_to_storage(data)?;
    
    Ok(IntentResult::Success)
}
```

**Capability Safety on Cancellation**:

Capabilities produced by cancelled stages are NOT committed:
- Only successful stages add capabilities to the pipeline's pool
- Cancelled stages don't produce capabilities (not added to trace)
- No cleanup code needed - capabilities simply aren't propagated
- Integrates with Phase 3 capability lifecycle tracking

**Result Types**:

All result types include distinct Cancelled variant:

```rust
pub enum StageResult {
    Success { output, capabilities },
    Failure { error },
    Retryable { error },
    Cancelled { reason },  // New in Phase 6
}

pub enum PipelineExecutionResult {
    InProgress,
    Success,
    Failed { stage_name, error },
    Cancelled { stage_name, reason },  // New in Phase 6
}
```

**Timeout Semantics**:

Timeouts trigger cancellation with reason=Timeout:
- Pipeline timeout: overall deadline for entire pipeline
- Stage timeout: deadline for individual stage (including retries)
- Deterministic evaluation using SimKernel time
- No implicit retries after timeout (unless retry policy explicitly allows)

**Interaction with Retries**:

Cancellation and timeout interact with retry policies:
- Cancellation check happens before each retry attempt
- Stage timeout includes all retry attempts
- Max retries still enforced even if time remains
- Explicit cancellation takes precedence over retry policy

**Testing**:

Phase 6 includes comprehensive cancellation tests:
- Cancel before pipeline starts
- Cancel mid-stage (handler cooperates)
- Stage timeout configuration
- Pipeline timeout (mechanism tested)
- Cancellation propagation through stages

**Design Rationale**:

**Why explicit tokens?**
- Clear ownership: who can cancel?
- Type-safe: compiler enforces token handling
- Composable: tokens can be passed through layers
- Testable: deterministic behavior

**Why not async cancellation?**
- No async runtime required
- Works with existing single-threaded SimKernel
- Simpler to reason about
- Compatible with future async integration if needed

**Why distinct Cancelled result?**
- Not an error - cancellation is intentional
- Different handling: no retries, no error logging
- Clear semantics: operation was stopped, not failed
- Enables proper cleanup and resource management

**Integration with Previous Phases**:

Phase 6 builds on all previous phases:
- **Phase 1**: Uses KernelApi time primitives (Instant, Duration)
- **Phase 2**: Works under fault injection (deterministic timeouts)
- **Phase 3**: Respects capability lifecycle (no leaks on cancel)
- **Phase 4**: Cancelled status is version-compatible addition
- **Phase 5**: Seamless integration with typed pipelines and retry semantics

All safety properties maintained under cancellation:
- No capability leaks (Phase 3)
- No schema violations (Phase 4)
- No infinite loops (Phase 5 + timeouts)
- Deterministic behavior (Phase 2 + deterministic time)

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
