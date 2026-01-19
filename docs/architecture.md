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

### Phase 7: Execution Identity, Supervision Trees, and Trust Boundaries

**Phase 7 (Current)**: Principled model of execution identity and supervision without POSIX concepts.

The system now includes:
- **Execution Identity Model**: ExecutionId, IdentityKind, IdentityMetadata
- **Identity Tracking in SimKernel**: Automatic identity creation and lifecycle management
- **Trust Boundaries**: Trust domain tags with cross-domain delegation auditing
- **Exit Notifications**: Structured termination reasons for supervision

**Philosophy**:
- **Identity is explicit and contextual, not global**: No POSIX users or UIDs
- **Authority comes from capabilities, not names**: Identity ≠ authority
- **Supervision is structure, not ad-hoc error handling**: Explicit relationships
- **Testability first**: All identity logic works under SimKernel

**Execution Identity Model**:

Every running task has an execution identity with:

Every running task has an execution identity with:
- `ExecutionId`: Unique identifier (never reused)
- `IdentityKind`: System | Service | Component | PipelineStage
- `TrustDomain`: "core" | "user" | "sandbox" | custom
- Parent/creator relationships for supervision
- Creation timestamp

```rust
// Create identity with full metadata
let metadata = IdentityMetadata::new(
    IdentityKind::Service,
    TrustDomain::core(),
    "storage-service",
    created_at_nanos,
)
.with_parent(supervisor_exec_id)
.with_task_id(task_id);

let exec_id = kernel.create_identity(metadata);
```

**Key Design Points**:

1. **Identity ≠ Authority**: Having an identity does NOT grant any privileges
   - Capabilities are the ONLY source of authority
   - Identity is for supervision and audit, not access control
   - No "run as user X" or privilege escalation concepts

2. **Explicit Parent-Child Relationships**:
   - Parent execution ID recorded at spawn time
   - Creator execution ID (who spawned this task)
   - Immutable after creation (no reparenting)

3. **Trust Domains**:
   - String-based tags: "core", "user", "sandbox", etc.
   - Cross-domain capability delegation is audited
   - NOT security enforcement (yet) - structural intent
   - Delegation within domain = normal
   - Delegation across domains = logged for review

4. **Exit Notifications**:
   ```rust
   pub enum ExitReason {
       Normal,                         // Successful completion
       Failure { error: String },      // Crashed or failed
       Cancelled { reason: String },   // Cancelled by supervisor
       Timeout,                        // Deadline exceeded
   }
   
   pub struct ExitNotification {
       pub execution_id: ExecutionId,
       pub task_id: Option<TaskId>,
       pub reason: ExitReason,
       pub terminated_at_nanos: u64,
   }
   ```

**SimKernel Integration**:

SimKernel automatically manages identity lifecycle:

```rust
// Spawn creates identity automatically
let handle = kernel.spawn_task(descriptor)?;
let exec_id = kernel.get_task_identity(handle.task_id)?;

// Or spawn with full identity control
let (handle, exec_id) = kernel.spawn_task_with_identity(
    descriptor,
    IdentityKind::Service,
    TrustDomain::core(),
    Some(parent_exec_id),
    Some(creator_exec_id),
)?;

// Terminate generates exit notification
kernel.terminate_task_with_reason(
    task_id,
    ExitReason::Failure { error: "crash".to_string() },
);

// Supervisor can check notifications
let notifications = kernel.get_exit_notifications();
for notif in notifications {
    match notif.reason {
        ExitReason::Normal => { /* child exited cleanly */ }
        ExitReason::Failure { .. } => { /* maybe restart */ }
        ExitReason::Cancelled { .. } => { /* intentional */ }
        ExitReason::Timeout => { /* took too long */ }
    }
}
kernel.clear_exit_notifications();
```

**Trust Boundary Auditing**:

Cross-domain delegation is tracked in capability audit log:

```rust
// Task A (core domain) delegates to Task B (user domain)
kernel.delegate_capability(cap_id, task_a, task_b)?;

// Audit log records:
CapabilityEvent::CrossDomainDelegation {
    cap_id,
    from_task,
    from_domain: "core",
    to_task,
    to_domain: "user",
}
```

Tests can verify trust boundary behavior:
```rust
let audit = kernel.audit_log();
assert!(audit.has_event(|e| matches!(
    e,
    CapabilityEvent::CrossDomainDelegation { .. }
)));
```

**Supervision Pattern** (future work in services_process_manager):

```rust
// Supervisor maintains child identity mapping
struct Supervisor {
    children: HashMap<ExecutionId, RestartPolicy>,
}

impl Supervisor {
    fn spawn_child(&mut self, kernel: &mut SimKernel) -> ExecutionId {
        let (handle, exec_id) = kernel.spawn_task_with_identity(
            descriptor,
            IdentityKind::Component,
            TrustDomain::user(),
            Some(self.exec_id),  // Parent
            Some(self.exec_id),  // Creator
        )?;
        
        self.children.insert(exec_id, RestartPolicy::OnFailure);
        exec_id
    }
    
    fn handle_notifications(&mut self, kernel: &mut SimKernel) {
        for notif in kernel.get_exit_notifications() {
            if let Some(policy) = self.children.get(&notif.execution_id) {
                // This is our child - apply restart policy
                match (notif.reason, policy) {
                    (ExitReason::Normal, _) => {
                        // Clean exit - don't restart
                        self.children.remove(&notif.execution_id);
                    }
                    (ExitReason::Failure { .. }, RestartPolicy::OnFailure) => {
                        // Restart the child
                        self.restart_child(kernel, notif.execution_id);
                    }
                    _ => { /* other policies */ }
                }
            }
        }
        kernel.clear_exit_notifications();
    }
}
```

**Design Rationale**:

**Why not POSIX users/groups?**
- POSIX UIDs are global numeric IDs (0-65535) with ambient authority
- PandaGen identities are contextual: parent/child relationships matter
- Authority comes from capabilities, not numeric identity
- No setuid, setgid, or privilege escalation complexity

**Why not authentication/crypto?**
- Phase 7 is about structure, not enforcement
- Trust domains are tags for supervision, not security boundaries (yet)
- Authentication requires key management, which is out of scope
- Focus: testable supervision patterns, not production security

**Why immutable identity metadata?**
- No reparenting or identity theft
- Clear audit trail (who created whom, when)
- Simpler to reason about (no state changes)
- Matches Rust ownership model (move, not mutate)

**Why exit notifications?**
- Supervisor needs structured information, not just "child died"
- Different exit reasons require different handling
- Timeout vs failure vs cancellation are distinct concepts
- Enables proper supervision without polling

**Testing Identity and Trust Boundaries**:

Tests validate:
- Identity creation and immutability
- Parent-child relationships
- Trust domain same/different detection
- Cross-domain delegation audit events
- Exit notification generation
- Identity retirement on termination

Example:
```rust
#[test]
fn test_trust_domain_delegation_cross_domain() {
    let mut kernel = SimulatedKernel::new();
    
    // Spawn in different trust domains
    let (task1_handle, _) = kernel.spawn_task_with_identity(
        descriptor1,
        IdentityKind::Component,
        TrustDomain::core(),
        None, None,
    )?;
    
    let (task2_handle, _) = kernel.spawn_task_with_identity(
        descriptor2,
        IdentityKind::Component,
        TrustDomain::user(),
        None, None,
    )?;
    
    // Grant and delegate across domains
    kernel.grant_capability(task1_handle.task_id, cap)?;
    kernel.delegate_capability(cap_id, task1_handle.task_id, task2_handle.task_id)?;
    
    // Verify cross-domain event recorded
    let audit = kernel.audit_log();
    assert!(audit.has_event(|e| matches!(
        e,
        CapabilityEvent::CrossDomainDelegation {
            from_domain, to_domain, ..
        } if from_domain == "core" && to_domain == "user"
    )));
}
```

**Integration with Previous Phases**:

Phase 7 builds on all previous phases:
- **Phase 1**: Uses TaskId, KernelApi, spawn semantics
- **Phase 2**: Works under fault injection
- **Phase 3**: Integrates with capability lifecycle (delegation, invalidation)
- **Phase 4**: Identity metadata is versioned/serializable if needed
- **Phase 5**: (Future) Pipeline stages have execution identities
- **Phase 6**: Exit notifications include Timeout and Cancelled reasons

All safety properties maintained:
- No capability leaks (identities retired, capabilities invalidated)
- No identity reuse (ExecutionId is UUID v4)
- No ambient authority (identity ≠ privilege)
- Deterministic testing (identity creation uses SimKernel time)

**Future Work**:

Phase 8+ may add:
- Supervisor restart policies with exponential backoff
- Health checks and heartbeat monitoring
- Cascade termination (kill supervisor → kill children)
- Resource quotas per trust domain
- Enforcement of cross-domain delegation policies
- Identity-based audit queries (show all actions by exec_id)

### Phase 8: Pluggable Policy Engines (Explicit, Testable, Non-Invasive)

**Phase 8 (Current)**: Pluggable policy framework for governance without hard-coded rules.

The system now includes:
- **Policy Engine Abstraction**: PolicyEngine trait for evaluating operations
- **Policy Decisions**: Allow, Deny(reason), or Require(action)
- **Policy Context**: Structured information about operations for policy evaluation
- **Enforcement Points**: Spawn, capability delegation (with optional policy)
- **Reference Policies**: NoOpPolicy, TrustDomainPolicy, PipelineSafetyPolicy
- **Policy Composition**: Combine multiple policies with precedence rules
- **Policy Audit**: Test-visible logging of all policy decisions

**Philosophy**:
- **Mechanism not policy**: Kernel provides primitives, policies are pluggable
- **Policy observes; it does not own**: Authority comes from capabilities, not policy
- **Explicit and testable**: All policy logic works under SimKernel
- **Advisory + enforceable**: Policies make decisions, enforcement points apply them
- **Pluggable and removable**: System works without policy engines

**Policy Model**:

Policy engines evaluate operations and return decisions:

```rust
pub trait PolicyEngine {
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision;
    fn name(&self) -> &str;
}

pub enum PolicyDecision {
    Allow,                        // Operation may proceed
    Deny { reason: String },      // Operation is blocked
    Require { action: String },   // Additional action needed
}
```

**Key Design Points**:

1. **Policy Does NOT Replace Capabilities**: Policy is additive
   - Capabilities are the ONLY source of authority
   - Policy can deny operations, but cannot grant authority
   - Identity provides context, not permission

2. **Enforcement Points Are Explicit and Optional**:
   - Spawn: `SimKernel::spawn_task_with_identity` checks OnSpawn policy
   - Delegation: `SimKernel::delegate_capability` checks OnCapabilityDelegate policy
   - If no policy engine is set, all operations are allowed

3. **Policy Composition**:
   - Multiple policies can be active via `ComposedPolicy`
   - Decision precedence: Deny > Require > Allow
   - First Deny wins (short-circuit evaluation)
   - All Require decisions must be satisfied

4. **Trust Domain Policy Example**:
   ```rust
   impl PolicyEngine for TrustDomainPolicy {
       fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
           match event {
               PolicyEvent::OnSpawn => {
                   // Sandbox cannot spawn System services
                   if context.actor_identity.trust_domain == TrustDomain::sandbox()
                       && context.target_identity.kind == IdentityKind::System {
                       return PolicyDecision::deny("Sandbox cannot spawn System services");
                   }
                   PolicyDecision::Allow
               }
               PolicyEvent::OnCapabilityDelegate => {
                   // Cross-domain delegation requires approval
                   if context.is_cross_domain() {
                       return PolicyDecision::require("Cross-domain delegation needs approval");
                   }
                   PolicyDecision::Allow
               }
               _ => PolicyDecision::Allow,
           }
       }
   }
   ```

**Policy Audit**:

Policy decisions are logged for test verification:

```rust
// Set policy engine
let kernel = SimulatedKernel::new()
    .with_policy_engine(Box::new(TrustDomainPolicy));

// Perform operations...

// Verify policy decisions in tests
let audit = kernel.policy_audit();
assert!(audit.has_event(|e| {
    matches!(e.event, PolicyEvent::OnSpawn) && e.decision.is_deny()
}));
```

**Design Rationale**:

**Why pluggable policy?**
- Different deployments need different policies
- Policies evolve independently from mechanisms
- Easier to reason about (separation of concerns)
- Testable in isolation

**Why not bake policy into KernelApi?**
- Would violate "mechanism not policy" principle
- Would make kernel complex and opinionated
- Would prevent experimentation with different policies
- Would make testing harder

**Why Allow/Deny/Require?**
- Allow: Simple positive case
- Deny: Explicit blocking with reason (debuggable)
- Require: Allows conditional approval (e.g., "add timeout first")

**Integration with Previous Phases**:

Phase 8 builds on all previous phases:
- **Phase 1**: Uses KernelApi, TaskId, ServiceId
- **Phase 2**: Works under fault injection (deterministic policy evaluation)
- **Phase 3**: Policies observe capability operations, don't own them
- **Phase 4**: Policy decisions are versioned/serializable if needed
- **Phase 5**: (Future) Pipelines have policy enforcement points
- **Phase 6**: Policy can require timeouts on operations
- **Phase 7**: Policy uses identity and trust domains for context

All safety properties maintained:
- No capability leaks (policy cannot grant authority)
- No ambient authority (policy observes, doesn't own)
- Deterministic testing (policy evaluation under SimKernel)
- Optional enforcement (system works without policies)

**Testing Policy Behavior**:

Tests validate:
- Individual policy engine logic
- Policy composition precedence
- Enforcement point integration
- Policy disabled (NoOpPolicy allows all)
- Audit trail completeness

Example:
```rust
#[test]
fn test_trust_domain_policy_denies_sandbox_spawn_system() {
    let mut kernel = SimulatedKernel::new()
        .with_policy_engine(Box::new(TrustDomainPolicy));
    
    // Sandbox task tries to spawn System service
    let result = kernel.spawn_task_with_identity(...);
    
    // Should be denied
    assert!(result.is_err());
    
    // Verify policy audit
    assert!(kernel.policy_audit().has_event(|e| e.decision.is_deny()));
}
```

**Future Work**:

Phase 9+ may add:
- ~~Policy enforcement in pipeline executor~~ (✅ Completed in Phase 9)
- Policy hot-reload (swap policies without restart)
- Policy decision caching for performance
- Policy composition DSL for complex rules
- Per-service policy overrides
- Policy-based resource quotas

### Phase 9: Pipeline Policy Enforcement + Policy Explain UX (Current)

**Phase 9**: Complete integration of policy framework with pipeline execution.

The system now includes:
- **Pipeline Policy Enforcement**: Pipelines check policy at start and stage boundaries
- **Explainable Decisions**: PolicyDecisionReport provides detailed evaluation information
- **Clear Error Messages**: Policy denials and requirements include actionable information
- **Deterministic Enforcement**: All policy checks work under SimKernel with fault injection

**Philosophy**:
- **Mechanism not policy**: Pipeline executor provides enforcement hooks, policies decide rules
- **Policy observes; it does not own**: Authority still comes from capabilities
- **Explicit over implicit**: Policy decisions are visible and testable
- **Preserve pre-Phase-9 behavior**: When policy is disabled (None), pipelines work exactly as before

**Policy Enforcement in Pipelines**:

Pipeline executor now checks policy at three points:

1. **OnPipelineStart**: Before pipeline execution begins
   ```rust
   let executor = PipelineExecutor::new()
       .with_identity(identity)
       .with_policy_engine(Box::new(PipelineSafetyPolicy::new()));
   
   let result = executor.execute(&mut kernel, &pipeline, input, token);
   // Policy checked before first stage runs
   ```

2. **OnPipelineStageStart**: Before each stage execution
   ```rust
   // Policy context includes:
   // - execution identity
   // - trust domain
   // - pipeline ID
   // - stage ID
   // - required capabilities
   // - timeout/retry metadata
   ```

3. **OnPipelineStageEnd**: After stage completion (audit only)
   - Policy can observe stage results
   - Decision recorded but not enforced

**Enforcement Rules**:

- **Deny** → Pipeline fails immediately with `ExecutorError::PolicyDenied`
- **Require** → Pipeline fails with `ExecutorError::PolicyRequire` and actionable message
- **Allow** → Pipeline continues execution

**Explainable Policy Decisions**:

```rust
pub struct PolicyDecisionReport {
    pub decision: PolicyDecision,
    pub evaluated_policies: Vec<PolicyEvaluation>,
    pub deny_reason: Option<String>,
    pub required_actions: Vec<String>,
}

// Composed policies produce full reports
let report = composed_policy.evaluate_with_report(event, &context);

// Shows which policies were evaluated and what they decided
for eval in &report.evaluated_policies {
    println!("{}: {:?}", eval.policy_name, eval.decision);
}
```

**Example Policy: PipelineSafetyPolicy**:

```rust
// Requires pipelines in user domain to have timeout
impl PolicyEngine for PipelineSafetyPolicy {
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        match event {
            PolicyEvent::OnPipelineStart => {
                if context.actor_identity.trust_domain == TrustDomain::user() {
                    let has_timeout = context.metadata.iter().any(|(k, _)| k == "timeout_ms");
                    if !has_timeout {
                        return PolicyDecision::require(
                            "Pipelines in user domain must specify a timeout"
                        );
                    }
                }
                PolicyDecision::Allow
            }
            _ => PolicyDecision::Allow,
        }
    }
}
```

**Error Reporting**:

Policy errors include all relevant context:

```rust
match result {
    Err(ExecutorError::PolicyRequire { policy, event, action, pipeline_id }) => {
        eprintln!("REQUIRES: {} (policy: {}, event: {})", action, policy, event);
        // e.g., "REQUIRES: Pipelines in user domain must specify timeout (policy: PipelineSafetyPolicy, event: OnPipelineStart)"
    }
    Err(ExecutorError::PolicyDenied { policy, event, reason, pipeline_id }) => {
        eprintln!("DENIED by {}: {} (event: {})", policy, reason, event);
        // e.g., "DENIED by DenySandboxPipelinePolicy: Sandbox cannot run pipelines (event: OnPipelineStart)"
    }
    Ok((output, trace)) => {
        // Success
    }
}
```

**Safety Properties**:

Enforcement maintains all Phase 1-8 safety properties:
- **Deterministic**: Same inputs → same policy decisions
- **Side-effect free**: Policy evaluation is pure
- **Capability-safe**: No authority leaks on denial (no partial operations)
- **Cancellation-aware**: Policy only recorded for started stages
- **Fault-tolerant**: Works correctly under message delay/reorder/drop

**Testing**:

Integration tests validate:

1. **Require Timeout**: PipelineSafetyPolicy requires timeout for user domain
   - Without timeout → PolicyRequire error
   - With timeout → success

2. **Deny at Pipeline Start**: Custom policy denies entire pipeline
   - Sandbox domain → denied before any stages run

3. **Deny at Stage Start**: Policy denies individual stages
   - Stage boundary → PolicyDenied error
   - No capability leaks

4. **Cancellation + Policy**: Mid-pipeline cancellation
   - Policy decisions only for started stages
   - Explain output remains coherent

**Design Rationale**:

**Why enforce at pipeline executor, not kernel?**
- Pipeline execution is orchestration, not kernel mechanism
- Keeps kernel API primitive and focused
- Policy is optional (pipelines work without it)
- Easier to test and compose policies

**Why Require in addition to Deny?**
- Deny is final: "you can't do this"
- Require is conditional: "you can do this IF you add X"
- Enables better UX: actionable error messages
- Example: "Add timeout to continue" vs "Denied: no reason given"

**Why PolicyDecisionReport?**
- Composed policies evaluate multiple engines
- Users need to know WHY a decision was made
- Testing needs to verify correct policy was applied
- Debugging requires visibility into policy logic

**Integration with Previous Phases**:

Phase 9 builds on all previous phases:
- **Phase 1**: Uses KernelApi, pipeline executor, IPC
- **Phase 2**: Works under fault injection (deterministic policy checks)
- **Phase 3**: Respects capability lifecycle (no leaks on denial)
- **Phase 4**: Policy decisions are versioned/serializable
- **Phase 5**: Integrates with typed pipelines and retry semantics
- **Phase 6**: Policy-aware of cancellation and timeouts
- **Phase 7**: Policy uses execution identity and trust domains for context
- **Phase 8**: Extends policy engine framework to pipelines

All safety properties preserved:
- No capability leaks (Phase 3)
- No schema violations (Phase 4)
- No infinite loops (Phase 5 + Phase 6 timeouts)
- Deterministic behavior (Phase 2 + deterministic policy evaluation)

**Future Work**:

Phase 10+ may add:
- Policy for pipeline composition (multi-pipeline orchestration)
- Resource quotas based on policy decisions
- Policy-driven retry strategies
- Cross-domain pipeline policies
- Policy decision caching for performance optimization

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

## Phase 10: Policy-Driven Capability Derivation

**Goals**: Extend policy from allow/deny/require into policy-driven capability derivation, enabling policies to restrict and/or grant scoped capabilities for a pipeline execution and its stages, without leaking authority and while preserving determinism.

### Security Boundary Feature

Phase 10 is not "just add a field" - it's a **security boundary**. The key challenge is preventing authority escalation while allowing restriction:

**Challenge**: How do we let policies restrict capabilities without:
1. Accidentally granting more authority than available?
2. Leaking capabilities across scope boundaries?
3. Breaking determinism?

**Solution**: Strict subset enforcement + scoped authority:

```
Invariant: derived_caps ⊆ current_caps

Enforcement:
- OnPipelineStart: execution_authority ⊆ initial_authority
- OnPipelineStageStart: stage_authority ⊆ execution_authority
- Error: PolicyDerivedAuthorityInvalid if subset check fails
```

### Determinism Requirements

**Hard Requirements**:

1. **Deterministic Evaluation**:
   - Policy evaluation is pure (no side effects)
   - Same inputs → same outputs
   - Serializable decisions
   - No timestamps, randomness, or external state

2. **No Authority Leaks**:
   - Derived authority ≤ current authority
   - No escalation without explicit "grant" path (not implemented)
   - Subset validation is mandatory, not optional

3. **Scoped**:
   - Pipeline-scoped: affects all stages
   - Stage-scoped: affects only that stage
   - Stage-scoped doesn't widen pipeline authority

4. **Explainable**:
   - PolicyDecisionReport shows:
     - input_capabilities
     - output_capabilities
     - delta (removed/restricted/added)
   - UX can show users exactly what changed and why

5. **Backwards Compatible**:
   - No policy → behavior identical to pre-Phase-9/10
   - Allow without derivation → behavior identical to Phase 9

### Domain Model

**CapabilitySet**:
```rust
pub struct CapabilitySet {
    pub capabilities: HashSet<u64>,
}

impl CapabilitySet {
    pub fn is_subset_of(&self, other: &CapabilitySet) -> bool;
    pub fn intersection(&self, other: &CapabilitySet) -> CapabilitySet;
    pub fn difference(&self, other: &CapabilitySet) -> CapabilitySet;
}
```

**DerivedAuthority**:
```rust
pub struct DerivedAuthority {
    pub capabilities: CapabilitySet,
    pub constraints: Vec<String>,  // Future use
}
```

**CapabilityDelta**:
```rust
pub struct CapabilityDelta {
    pub removed: Vec<u64>,
    pub restricted: Vec<String>,
    pub added: Vec<u64>,  // Should be empty (no escalation)
}

impl CapabilityDelta {
    pub fn from(before: &CapabilitySet, after: &CapabilitySet) -> Self;
}
```

**Extended PolicyDecision**:
```rust
pub enum PolicyDecision {
    Allow { derived: Option<DerivedAuthority> },
    Deny { reason: String },
    Require { action: String },
}
```

### Enforcement Points

**Pipeline Start** (`OnPipelineStart`):
1. Evaluate policy with pipeline context
2. If `Allow { derived: Some(auth) }`:
   - Validate: `auth.capabilities ⊆ current_capabilities`
   - If not subset → `PolicyDerivedAuthorityInvalid`
   - If valid → `execution_authority = auth.capabilities`
3. Continue with restricted authority

**Stage Start** (`OnPipelineStageStart`):
1. Evaluate policy with stage context
2. If `Allow { derived: Some(auth) }`:
   - Validate: `auth.capabilities ⊆ execution_authority`
   - If not subset → `PolicyDerivedAuthorityInvalid`
   - If valid → `stage_authority = auth.capabilities`
3. Stage runs with `stage_authority`
4. Next stage gets `execution_authority` (not stage_authority)

**Stage End** (`OnPipelineStageEnd`):
- Audit only, no mutation
- Policy evaluation recorded but doesn't affect authority

### Capability Checking

**Before Phase 10**:
```rust
if !has_capability(cap_id) {
    return Err("Missing required capability");
}
```

**After Phase 10**:
```rust
if !has_capability_with_authority(cap_id, &stage_authority) {
    return Err("Missing required capability");
}

fn has_capability_with_authority(
    cap_id: u64,
    authority: &Option<CapabilitySet>,
) -> bool {
    // Check if we have it
    if !self.has_capability(cap_id) {
        return false;
    }
    // Check against derived authority if present
    if let Some(auth) = authority {
        auth.capabilities.contains(&cap_id)
    } else {
        true
    }
}
```

### Error Handling

**New Error Type**:
```rust
PolicyDerivedAuthorityInvalid {
    policy: String,
    event: String,
    reason: String,
    delta: String,  // "removed: [1], added: [999]"
    pipeline_id: Option<String>,
}
```

**Integration with Existing Errors**:
- `PolicyDenied`: Policy says "no"
- `PolicyRequire`: Policy says "not yet, add X"
- `PolicyDerivedAuthorityInvalid`: Policy bug or malicious

### Testing Strategy

**Integration Tests** (6 minimum):

1. `test_policy_derives_readonly_fs_at_pipeline_start`:
   - Policy restricts FS to read-only
   - Handler observes reduced capability set
   - Validates pipeline-scoped derivation

2. `test_policy_derives_no_network_at_stage_start`:
   - Stage loses network capability
   - Next stage regains it
   - Validates stage-scoped isolation

3. `test_policy_derivation_is_subset_enforced`:
   - Malicious policy tries to grant extra capability
   - Executor fails with `PolicyDerivedAuthorityInvalid`
   - Validates defense against escalation

4. `test_policy_report_includes_capability_delta`:
   - Report includes input/output/delta
   - Validates explainability
   - Ensures serialization works

5. `test_policy_derivation_and_cancellation_coherent`:
   - Cancellation mid-execution
   - Derived authority only for started stages
   - Report remains consistent

6. `test_no_policy_behavior_unchanged`:
   - No policy engine set
   - Exact behavior as pre-Phase-9/10
   - Validates backwards compatibility

**Unit Tests** (policy crate):
- `CapabilitySet` operations (subset, intersection, difference)
- `CapabilityDelta::from` correctness
- Serialization/deserialization round-trips

### Design Rationale

**Why CapabilitySet in policy crate, not core_types?**
- Policy-specific abstraction
- Keeps core_types focused on kernel primitives
- Easier to evolve independently
- Policy needs set operations, kernel doesn't

**Why not allow "add" in CapabilityDelta?**
- No escalation without explicit grant path
- Grant path requires trusted policy signature
- Not implemented in Phase 10 - future work
- `added` field exists but should be empty

**Why stage-scoped authority doesn't affect pipeline authority?**
- Least surprise: stage restrictions are temporary
- Isolation: one stage can't widen authority for others
- Determinism: next stage sees same authority regardless of previous stage
- Exception: if desired, could add "tighten" mode in future

**Why mandatory subset validation?**
- Defense in depth
- Catches policy bugs
- Prevents accidental escalation
- Better error message than silent failure

**Why PolicyDerivedAuthorityInvalid vs PolicyDenied?**
- Different failure modes:
  - Denied: policy says "you can't do this"
  - Invalid: policy logic is buggy
- Invalid is more serious (policy implementation error)
- Helps debugging policy engines

### Integration with Previous Phases

Phase 10 builds on:
- **Phase 1**: Capability system, pipeline executor
- **Phase 2**: Deterministic execution (no randomness in policy)
- **Phase 3**: Capability lifecycle (no leaks on restriction)
- **Phase 4**: Versioned, serializable types
- **Phase 5**: Typed pipelines, stage boundaries
- **Phase 6**: Cancellation awareness
- **Phase 7**: Identity and trust domains for context
- **Phase 8**: Policy engine framework
- **Phase 9**: Policy enforcement at pipeline/stage boundaries

Phase 10 extends Phase 9's "allow/deny/require" into "allow with derived authority".

### Future Extensions

**Not Implemented in Phase 10**:

1. **Escalation Path**:
   - Explicit "grant" policy with signature
   - Required for adding capabilities
   - Must be auditable and explicit
   - E.g., `DerivedAuthority::from_grant(trusted_policy, new_caps)`

2. **Fine-Grained Restrictions**:
   - Beyond simple removal
   - Time-limited capabilities
   - Source-restricted capabilities
   - Rate-limited capabilities

3. **Cross-Pipeline Authority**:
   - Currently pipeline-local
   - Could extend to service-level authority
   - Would need global authority manager

4. **Dynamic Policy Update**:
   - Currently policy is fixed at pipeline start
   - Could allow mid-execution policy changes
   - Would need careful synchronization

### Summary

Phase 10 provides:
- **Secure**: No authority escalation, strict subset enforcement
- **Scoped**: Pipeline vs stage authority
- **Deterministic**: Pure, reproducible policy evaluation
- **Explainable**: Detailed capability deltas
- **Backwards compatible**: Works with or without policies
- **Defense in depth**: Multiple validation points

This enables least-privilege enforcement at the policy layer without compromising the core capability model. Policies can say "you have these capabilities, but you may only use these" without being able to grant capabilities they don't have.

## Phase 11: Resource Quotas, Budgets, and Deterministic Accounting

**Goals**: Introduce deterministic resource budgets enforced per identity and trust domain, driven by policy, fully testable under SimKernel.

### Resource Philosophy

Authority must be bounded. Even correct capabilities must have limits.

**Core Principles**:
- **Resources are finite and must be explicit**: No implicit unlimited resources
- **Budgets are enforced, not advisory**: Hard limits, not soft guidelines
- **Accounting is deterministic and testable**: Reproducible under SimKernel
- **Policy may require or limit resources, but does not implement accounting**: Separation of concerns
- **No POSIX concepts**: Not ulimits, not nice, not cgroups
- **No real hardware yet**: Simulation-first, hardware later

**Why No Throttling?**

Traditional systems use "nice" values, CPU shares, and best-effort resource management. PandaGen rejects this:
- **Throttling is unpredictable**: Cannot reason about timing or completion
- **Best-effort is not deterministic**: Cannot test reliably
- **Implicit limits are dangerous**: Resource exhaustion becomes surprise failure

Instead, PandaGen uses **deterministic hard limits**:
- Operations either succeed or fail explicitly
- No silent slowdown or starvation
- Testable under fault injection
- Clear error messages with resource type, limit, usage

**Budgeting vs Authority**

Resources and capabilities are orthogonal:
- **Capabilities**: What you may do (authority)
- **Budgets**: How much you may do (quota)

Both are required:
- Having a capability without budget: Cannot act (no quota)
- Having budget without capability: Cannot act (no authority)
- Having both: Can act until budget exhausted

Example:
```rust
// Task has FileWrite capability (authority)
// Task has StorageOps budget of 100 (quota)
// First 100 writes succeed
// 101st write fails with ResourceBudgetExceeded
```

### Resource Types

Phase 11 introduces five abstract resource types:

1. **CpuTicks**: Simulated execution steps
   - Not real CPU cycles
   - Deterministic consumption under SimKernel
   - Used for computational work tracking

2. **MemoryUnits**: Abstract memory allocation units
   - Not bytes or pages
   - Platform-independent
   - Used for memory quota enforcement

3. **MessageCount**: Number of messages sent/received
   - Explicit per-message accounting
   - Prevents message flooding
   - Deterministic under fault injection

4. **StorageOps**: Storage read/write operations
   - Not bytes or blocks
   - Operation-level tracking
   - Independent of storage size

5. **PipelineStages**: Number of pipeline stages executed
   - Limits pipeline complexity
   - Prevents runaway composition
   - Stage-level granularity

All types are:
- Strong newtypes (not raw integers)
- Saturating arithmetic (no panic on overflow)
- Serializable for persistence
- Testable with deterministic behavior

### Budget Structure

**ResourceBudget**: Immutable limits for resources
```rust
let budget = ResourceBudget::unlimited()
    .with_cpu_ticks(CpuTicks::new(1000))
    .with_message_count(MessageCount::new(50));
```

Properties:
- Immutable once created
- Can only be replaced by policy (with validation)
- Never grows unless explicitly derived
- Optional per resource (None = unlimited)

**ResourceUsage**: Current consumption tracking
```rust
let mut usage = ResourceUsage::zero();
usage.consume_cpu_ticks(CpuTicks::new(10));
usage.consume_message();
```

Properties:
- Mutable, updated as resources consumed
- Checked against budget at enforcement points
- Tracked per ExecutionId

**ResourceDelta**: Changes in consumption
```rust
let delta = ResourceDelta::from(&before, &after);
// Shows: cpu=+10, msg=+1, ...
```

### Budget Attachment to Identity

Every ExecutionId may have an optional ResourceBudget:

```rust
let identity = IdentityMetadata::new(...)
    .with_budget(budget);
```

**Inheritance Rules**:
- Child budget must be ≤ parent budget (subset check)
- Validation happens at spawn time
- Violation results in explicit error
- No implicit escalation

**Lifetime Scoping**:
- Budget scoped to identity lifetime
- Termination releases budget immediately
- No cleanup code needed (automatic)
- No dangling budget references

### Enforcement Points

SimKernel enforces budgets at specific points:

1. **Task Spawn** (initial budget assignment):
   - Validate budget inheritance
   - Create usage tracker
   - Fail if invalid

2. **Message Send/Receive** (MessageCount):
   - Check budget before operation
   - Consume one message unit
   - Fail with explicit error if exceeded

3. **Simulated Execution Steps** (CpuTicks):
   - Track computational work
   - Increment on kernel operations
   - Fail if budget exhausted

4. **Storage Operations** (StorageOps):
   - Track read/write operations
   - One unit per operation
   - Independent of data size

**Enforcement Behavior**:
- Exceeding budget results in deterministic failure
- Failure reason is explicit:
  - Which resource exceeded
  - Limit value
  - Current usage
  - Identity context
- No silent throttling or degradation
- No recovery without explicit budget increase

### Integration with Cancellation

Budget exhaustion may trigger cancellation:

```rust
if let Some(exceeded) = usage.exceeds(&budget) {
    // Option 1: Fail immediately
    return Err(ResourceBudgetExceeded(exceeded));
    
    // Option 2: Cancel with reason
    cancel_token.cancel(CancellationReason::Custom(
        format!("Budget exhausted: {}", exceeded)
    ));
}
```

Properties:
- Budget exhaustion is distinct from cancellation
- Cancellation may be triggered by budget
- Both recorded in audit log
- Both deterministic and testable

### Policy Integration

Policies can:
- **Require budgets**: "You must have MessageCount budget to proceed"
- **Deny if exceeds**: "Your budget is too large for sandbox"
- **Derive restricted budgets**: Subset enforcement

Example policies:
```rust
// Require budget for user domain
PolicyDecision::require("User tasks must specify MessageCount budget");

// Deny if budget too large
if budget.message_count > Some(MessageCount::new(100)) {
    PolicyDecision::deny("Sandbox limited to 100 messages");
}

// Derive restricted budget (subset only)
let derived = budget.min(&sandbox_limit);
PolicyDecision::allow_with_derived(DerivedAuthority::with_budget(derived));
```

Policy rules:
- Policy may deny if no budget present
- Policy may derive restricted budget (subset only)
- Policy cannot increase budget (no escalation)
- Budget derivation follows Phase 10 subset invariants

### Error Types

**ResourceBudgetExceeded**:
```rust
ResourceError::BudgetExceeded(ResourceExceeded::CpuTicks {
    limit: CpuTicks::new(1000),
    usage: CpuTicks::new(1001),
})
```

**ResourceBudgetMissing**:
```rust
ResourceError::BudgetMissing {
    operation: "send_message".to_string(),
}
```

**InvalidBudgetDerivation**:
```rust
ResourceError::InvalidBudgetDerivation {
    reason: "Child budget exceeds parent".to_string(),
}
```

All errors include:
- Resource type
- Limit and usage values
- Identity context
- Pipeline/stage context (if applicable)
- Human-readable explanation

### Testing Strategy

**Unit Tests** (resources crate):
- Arithmetic boundary conditions
- Budget subset validation
- Usage tracking
- Delta computation

**Integration Tests** (sim_kernel, pipelines):
- Budget exhaustion scenarios
- Inheritance validation
- Policy-required budgets
- Fault injection + budgets
- Cancellation interaction

**Properties Verified**:
- Deterministic: Same operations → same consumption
- No double-counting: Delayed messages counted once
- No leaks: Cancelled operations don't consume
- No escalation: Child ≤ parent always

### Design Rationale

**Why abstract resource types, not bytes/cycles?**
- Platform-independent
- Easier to test (no hardware needed)
- Simpler accounting (no conversion factors)
- Clear semantics (one message = one unit)

**Why immutable budgets?**
- Prevents accidental modification
- Clear audit trail (replace, don't mutate)
- Matches Rust ownership model
- Easier to reason about

**Why deterministic enforcement?**
- Testability is first-class constraint
- Reproducible behavior under faults
- No flaky tests from timing
- Clear semantics (succeed or fail)

**Why no global counters?**
- Per-identity tracking prevents interference
- No shared mutable state
- Easier to test in isolation
- Natural fit for distributed systems

**Why fail explicitly, not throttle?**
- Predictable behavior (no slowdown surprise)
- Testable outcomes (pass or fail)
- Clear error messages (know why it failed)
- No hidden performance degradation

### Future Extensions

**Not Implemented in Phase 11**:

1. **Real Hardware Integration**:
   - Map CpuTicks to real CPU cycles
   - Map MemoryUnits to bytes/pages
   - Hardware counters for enforcement
   - Preemption on budget exhaustion

2. **Dynamic Budget Adjustment**:
   - Increase budget at runtime (with policy approval)
   - Budget borrowing between siblings
   - Budget pooling for trust domains
   - Exponential backoff for exhaustion

3. **Budget Pooling**:
   - Shared budget across trust domain
   - Subtract from pool on allocation
   - Return to pool on termination
   - Prevents starvation in large systems

4. **Fine-Grained Storage Accounting**:
   - Track bytes written, not just operations
   - Separate read/write budgets
   - Storage quota per object
   - Garbage collection triggers

5. **Preemptive Scheduling**:
   - Budget-driven preemption
   - Fair share scheduling
   - Priority-based budget allocation
   - Work-conserving policies

### Summary

Phase 11 provides:
- **Deterministic resource limits**: No throttling, explicit failure
- **Per-identity budgets**: Scoped to execution lifetime
- **Inheritance validation**: Child ≤ parent enforced
- **Policy integration**: Budgets as first-class policy concern
- **Testable enforcement**: Works under SimKernel with fault injection
- **Explainable errors**: Clear resource type, limit, usage

This completes the authority model: capabilities (what) + budgets (how much) = controlled execution.

---

## Phase 12: Deterministic Resource Enforcement & Exhaustion Semantics

**Status**: Phase 12 (Current)

**Goal**: Turn resource budgets from models into enforced constraints with deterministic failure semantics.

### Philosophy: Budgets Must Bite

Phase 11 provided budget *models* and attachment to identities. Phase 12 makes limits **real**.

Core principles:
- **No throttling**: Never slow down on approaching limit
- **No fairness**: Not a scheduler, just enforcement
- **No retries**: Exhaustion fails immediately
- **Deterministic failure**: Same operations → same outcome
- **Explicit errors**: Know exactly what exhausted and why
- **Testability first**: All enforcement runs under SimKernel

### What We Don't Do

Phase 12 explicitly avoids:
- ❌ Scheduling or preemption
- ❌ Async runtimes or event loops
- ❌ Throttling or "slow down" behavior  
- ❌ POSIX-style soft/hard limits (ulimits)
- ❌ Best-effort survival (e.g., "try again later")
- ❌ Silent drops or degradation

If behavior feels like mercy or retry, it's wrong. This phase is about **limits**, not grace.

### Enforcement Points

Resources are consumed at real execution boundaries:

#### Message Operations
```rust
// Send: consumes MessageCount before sending
kernel.send_message(channel, message)?;
// If budget exhausted: Err(ResourceBudgetExhausted)
// Message source TaskId determines which identity

// Receive: consumes MessageCount before receiving
kernel.set_receive_context(task_id); // Phase 12 workaround
kernel.receive_message(channel, timeout)?;
// If budget exhausted: Err(ResourceBudgetExhausted)
```

Each send/receive consumes exactly **one** MessageCount unit.

#### CPU Operations
```rust
// External components (e.g., pipeline executor) consume CPU
kernel.try_consume_cpu_ticks(execution_id, amount)?;
// If budget exhausted: Err(ResourceBudgetExhausted)
```

Used for simulated execution steps, handler invocations, or stage processing.

#### Pipeline Stages
```rust
// Consume PipelineStages on stage entry
kernel.try_consume_pipeline_stage(execution_id, stage_name)?;
// If budget exhausted: Err(ResourceBudgetExhausted)
```

Each stage entry consumes exactly **one** PipelineStages unit.

### Exhaustion Semantics

When a budget limit is reached:

1. **Immediate Failure**: Operation aborts before taking effect
2. **No Partial Effects**: Transaction-like semantics
3. **Identity Cancellation**: Exhausted identity is marked cancelled
4. **Future Operations Rejected**: All subsequent operations fail instantly

```rust
// First exhaustion
let result = kernel.send_message(channel, msg);
assert!(matches!(result, Err(KernelError::ResourceBudgetExhausted { .. })));

// Subsequent operations fail immediately (cancelled)
let result2 = kernel.send_message(channel, msg2);
assert!(matches!(result2, Err(KernelError::ResourceBudgetExhausted { 
    resource_type, .. 
}) if resource_type.contains("cancelled")));
```

### Error Types

#### ResourceBudgetExhausted
```rust
KernelError::ResourceBudgetExhausted {
    resource_type: "MessageCount".to_string(),
    limit: 10,
    usage: 10,
    identity: "exec:a1b2c3...".to_string(),
    operation: "send_message".to_string(),
}
```

Includes:
- **resource_type**: Which resource (MessageCount, CpuTicks, etc.)
- **limit**: Budget limit that was exceeded
- **usage**: Current consumption at failure
- **identity**: Which ExecutionId exhausted
- **operation**: What operation failed

#### ResourceBudgetExceeded
Legacy variant for warnings (not currently used for hard enforcement).

### Cancellation Integration

Budget exhaustion **triggers cancellation**:

```rust
// Exhaustion cancels identity
assert!(kernel.is_identity_cancelled(execution_id));

// Cancelled identities consume no further resources
let result = kernel.try_consume_cpu_ticks(execution_id, 1);
assert!(result.is_err()); // "cancelled" in error message
```

Properties:
- Cancellation prevents further consumption
- No double-counting (consume once, fail once)
- Deterministic (same inputs → same cancellation point)
- Auditable (cancellation recorded in resource audit log)

### Audit & Observability

Phase 12 adds **ResourceAuditLog** (test-visible only):

```rust
// Check resource consumption
let audit = kernel.resource_audit();

// Count consumption events
audit.count_events(|e| matches!(e, ResourceEvent::MessageConsumed { .. }));

// Check exhaustion events
audit.has_event(|e| matches!(e, ResourceEvent::BudgetExhausted { .. }));

// Check cancellation
audit.has_event(|e| matches!(e, ResourceEvent::CancelledDueToExhaustion { .. }));

// Query by identity
let entries = audit.entries_for_execution(execution_id);
```

Audit events:
- **MessageConsumed**: Send/receive with before/after usage
- **CpuConsumed**: CPU consumption with amount
- **StorageOpConsumed**: Storage operation (future)
- **PipelineStageConsumed**: Stage entry
- **BudgetExhausted**: Limit reached, operation failed
- **CancelledDueToExhaustion**: Identity cancelled due to exhaustion

Audit properties:
- Does **not** affect correctness
- Deterministic (same order every run)
- Queryable in tests
- Test-only (not in production runtime)

### Fault Injection Interaction

Resource enforcement works correctly under fault injection:

**Delayed Messages**: Budget consumed at send time, not delivery time
```rust
// Send with delay fault - budget consumed immediately
kernel.send_message(channel, msg)?; // MessageCount consumed here
// Message delivered later (after delay)
// No additional consumption on delivery
```

**Message Drops**: Budget consumed even if dropped
```rust
// Send with drop fault
kernel.send_message(channel, msg)?; // MessageCount consumed
// Fault injector drops message
// Budget still consumed (operation succeeded from sender's view)
```

**Retries**: Each retry consumes resources
```rust
// Pipeline retry with 3 attempts
// Attempt 0: consume PipelineStages, CpuTicks
// Retry 1: consume CpuTicks again
// Retry 2: consume CpuTicks again
// Each attempt counts separately
```

Properties:
- Consumption is deterministic regardless of faults
- No double-counting under any fault
- Delay doesn't change consumption behavior
- Drop/reorder doesn't affect budget

### Testing Strategy

**Test Coverage** (tests_resilience/resource_enforcement.rs):

1. **Message Exhaustion**:
   - Send until exhausted
   - Receive until exhausted
   - Exact boundary conditions

2. **CPU Exhaustion**:
   - Consume ticks until exhausted
   - Verify exact tick counting
   - No double-consumption

3. **Pipeline Stage Exhaustion**:
   - Consume stages until exhausted
   - Verify per-stage tracking
   - Cancellation integration

4. **Cancellation Interaction**:
   - No consumption after cancellation
   - Cancellation error on retry
   - Audit log verification

5. **Fault Injection**:
   - Delayed messages consume deterministically
   - Dropped messages still consume
   - No timing-dependent behavior

**Properties Verified**:
- Deterministic: Same operations → same exhaustion point
- No double-counting: Resource consumed exactly once per operation
- No consumption after cancellation: Cancelled = dead
- Audit completeness: All events recorded
- Fault independence: Faults don't change consumption semantics

### Design Rationale

**Why fail immediately, not warn?**
- Predictable: Know exactly when failure occurs
- Testable: Deterministic outcomes
- Clear: No ambiguity between warning and error
- Safe: Prevents partial work

**Why cancel on exhaustion?**
- Prevents zombie execution (exhausted but still running)
- Clear lifecycle boundary (exhausted = terminated)
- Simplifies reasoning (dead is dead)
- Prevents resource leaks

**Why consume at call site, not completion?**
- Deterministic: Timing doesn't affect consumption
- Simple: No async tracking needed
- Fair: All operations cost the same
- Testable: Know consumption without waiting

**Why no recovery mechanism?**
- Forces explicit budget management
- Prevents accidental overuse
- Clear failure modes (fix budget or fail)
- Testability (no flaky recovery)

**Why separate exhaustion from cancellation?**
- Different causes: budget vs. external signal
- Different semantics: predictable vs. asynchronous
- Clear audit trail: why did it stop?
- Testable: verify exhaustion vs. cancellation separately

### Limitations & Future Work

**Not Implemented in Phase 12**:

1. **Storage Operations**: 
   - Complex integration with transaction semantics
   - Needs coordination with services_storage
   - Deferred to future phase

2. **Full Pipeline Integration**:
   - Pipeline executor has hooks but not fully integrated
   - Stage consumption needs pipeline refactoring
   - CPU per stage needs execution model changes

3. **KernelApi Redesign**:
   - send_message/receive_message don't pass TaskId
   - Workarounds used (message source, receive context)
   - Future: Add TaskId parameter to API

4. **Memory Accounting**:
   - MemoryUnits defined but not enforced
   - Needs allocator integration
   - Complex with Rust ownership

5. **Preemptive Enforcement**:
   - No preemption on budget exhaustion
   - No fair share scheduling
   - Still deterministic, single-threaded model

### Integration with Prior Phases

Phase 12 builds on:

- **Phase 3**: Capabilities track *what*, budgets track *how much*
- **Phase 6**: Exhaustion may trigger cancellation (lifecycle integration)
- **Phase 10**: Policy can derive restricted budgets (subset enforcement)
- **Phase 11**: Budgets attached to identities, inheritance validated

Complete authority model:
```
Authority = Capabilities (what you can do)
          + Budgets (how much you can do)
          + Policy (constraints on both)
          + Enforcement (make it real)
```

### Summary

Phase 12 provides:
- **Deterministic enforcement**: Budgets are hard limits, not suggestions
- **Explicit exhaustion**: Clear errors with context
- **Cancellation integration**: Exhausted = cancelled
- **Audit trail**: All consumption/exhaustion recorded
- **Fault-independent**: Works correctly under delay/drop/reorder
- **Test-driven**: All enforcement testable in SimKernel

This completes resource enforcement: budgets that **bite**.

---

## Phase 14: Input System Abstraction

### Philosophy: Input as Explicit Events

**Problem**: Traditional input models (stdin, TTY, global keyboard state) are:
- Ambient authority (anyone can read)
- Byte streams (unstructured, hard to test)
- Timing-dependent (race conditions)
- Hardware-coupled (can't test without devices)

**Solution**: Treat input as explicit, capability-gated events:
- **Events, not streams**: `InputEvent` structures (typed, serializable)
- **Explicit subscription**: Must request input via capability
- **Focus control**: Only focused component receives events
- **Test-first**: Inject events deterministically in SimKernel

### Input Model

```
┌─────────────────────────────────────────┐
│         Interactive Component           │
│     (CLI, Editor, UI Shell)             │
├─────────────────────────────────────────┤
│         Focus Manager Service           │
│     (maintains focus stack)             │
├─────────────────────────────────────────┤
│         Input Service                   │
│     (subscriptions + delivery)          │
├─────────────────────────────────────────┤
│         Input Types                     │
│     (KeyEvent, Modifiers, KeyCode)      │
└─────────────────────────────────────────┘
```

### Core Components

**1. Input Types (`input_types`)**
- `InputEvent`: Enum of input event types
  - `Key(KeyEvent)`: Keyboard events
  - (Pointer/Touch reserved for future)
- `KeyEvent`: Structured keyboard event
  - `code: KeyCode`: Logical key (A-Z, F1-F12, etc.)
  - `modifiers: Modifiers`: Ctrl, Alt, Shift, Meta
  - `state: KeyState`: Pressed, Released, Repeat
  - `text: Option<String>`: For IME support (future)
- No raw scan codes or hardware details

**2. Input Service (`services_input`)**
- Manages input subscriptions
- API:
  - `subscribe_keyboard(task_id, channel) -> InputSubscriptionCap`
  - `revoke_subscription(cap)`
  - `deliver_event(cap, event) -> bool`
- One subscription per task
- Delivery consumes MessageCount budget

**3. Focus Manager (`services_focus_manager`)**
- Maintains focus stack (LIFO)
- API:
  - `request_focus(cap)` - Push onto focus stack
  - `release_focus()` - Pop from focus stack
  - `route_event(event) -> Option<InputSubscriptionCap>`
- Only top of stack receives events
- Audit trail for all focus changes

**4. SimKernel Integration**
- Test utilities for event injection
- `InputEventQueue` for simulation
- Deterministic event ordering
- Budget consumption enforcement

### Why No TTY / stdin / stdout?

Traditional terminal model problems:
1. **Ambient authority**: Any process can read stdin
2. **Implicit focus**: "Whoever reads first" race condition
3. **Byte streams**: Raw bytes, not structured events
4. **Hardware coupling**: Assumes VT100-style terminal

PandaGen approach:
1. **Explicit capability**: Must request input subscription
2. **Explicit focus**: Focus manager controls routing
3. **Structured events**: Typed, serializable, testable
4. **Hardware abstraction**: Works in simulation, extensible to real hardware

### Interactive Component Pattern

```rust
// 1. Create component
let console = InteractiveConsole::new(task_id);

// 2. Subscribe to input
console.subscribe(&mut input_service, channel)?;

// 3. Request focus
console.request_focus(&mut focus_manager)?;

// 4. Process events
let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
if let Some(command) = console.process_event(event)? {
    // Execute command
}
```

### Future Evolution

When real hardware drivers are added:
1. HAL provides keyboard/pointer/touch drivers
2. Drivers inject events into input service
3. Everything else stays the same
4. Tests continue to work via injection

### Testing Strategy

All input behavior is testable:
- Unit tests: Each component in isolation
- Integration tests: Full input flow (subscribe → focus → event → delivery)
- Simulation tests: Multiple components competing for focus
- Budget tests: Message delivery consumes resources

### Summary

Phase 14 provides:
- **Explicit input ownership**: No ambient keyboard access
- **Event-driven**: Structured events, not byte streams
- **Focus control**: Explicit, policy-driven focus management
- **Testability**: Full input simulation without hardware
- **Extensibility**: Ready for pointer, touch, gamepad when needed

This is **not** a TTY. This is a modern input abstraction.

## Phase 15: Editor Component (Modal, Versioned, Capability-Safe)

### Philosophy: Components, Not Processes

Traditional Unix editors (vi, emacs, nano) are **processes** that:
- Run in a terminal (TTY)
- Read stdin, write stdout
- Access files via paths (ambient authority)
- Overwrite files on save

PandaGen's editor is a **component** that:
- Receives keyboard events (no TTY)
- Renders to a text surface (no stdout)
- Accesses documents via capabilities (no ambient paths)
- Creates new versions on save (immutability)

This is a fundamental shift: **editors as library components**, not standalone programs.

### Editor Model

**Modal Editing**:
```
Normal Mode ──i──> Insert Mode
     │              │
     │              └──Esc──> Normal Mode
     │
     └──:──> Command Mode
              │
              └──Enter/Esc──> Normal Mode
```

**Document Model**:
```
┌─────────────────┐
│  DocumentHandle │
├─────────────────┤
│ ObjectId        │ ← Capability to object
│ VersionId       │ ← Current version
│ path_label      │ ← Display only (not authority!)
│ can_update_link │ ← Write permission flag
└─────────────────┘
```

**Save Semantics**:
1. Save creates **new version** (immutable)
2. Return new VersionId capability
3. **Separately** update directory link (if permission exists)

This separates content versioning from directory management.

### Core Design

**State Machine**:
```rust
pub struct EditorState {
    mode: EditorMode,              // Normal | Insert | Command
    buffer: TextBuffer,            // Vec<String> (simple, testable)
    cursor: Cursor,                // Position with boundary checking
    dirty: bool,                   // Unsaved changes flag
    command_buffer: String,        // Command being typed
    status_message: String,        // Feedback to user
    document_label: Option<String>,// Display name (not authority)
}
```

**Operations**:
- Text insertion at cursor
- Character deletion (backspace, delete)
- Newline insertion with line splitting
- Line joining on backspace
- Cursor navigation with clamping

**Commands**:
- `:w` - Save (create new version)
- `:q` - Quit (blocked if dirty)
- `:q!` - Force quit (discard changes)
- `:wq` - Save and quit

### Capability-Based Document Access

**Opening a Document**:
```rust
// Option 1: Direct capability (preferred)
let handle = DocumentHandle::new(
    object_id,    // Capability to object
    version_id,   // Current version
    None,         // No path label
    false         // No link update permission
);

// Option 2: Via fs_view (convenience)
let options = OpenOptions::new()
    .with_path("/docs/readme.txt");
// fs_view resolves path → object capability
// Authority comes from root capability, not path
```

**Saving a Document**:
```rust
let save_result = editor.save()?;
// Returns:
// - new_version_id: VersionId   (always)
// - link_updated: bool           (only if can_update_link)
// - message: String              (status for user)

// If link_updated == false:
//   - New version created in storage
//   - Directory link still points to old version
//   - User notified: "Saved but no directory write permission"
```

This is crucial: **saving content and updating links are distinct operations**.

### Key Differences from Traditional Editors

| Traditional vi | PandaGen Editor |
|----------------|----------------|
| TTY-based | Event-based |
| stdin/stdout | InputEvent/Render |
| Path = authority | Path = label |
| File overwrite | Version creation |
| Global environment | Explicit capabilities |
| Hard to test | Fully testable |

### Implementation Highlights

**Modal Input Processing**:
```rust
match editor.state().mode() {
    EditorMode::Normal => {
        // h/j/k/l navigation
        // i enters insert
        // x deletes char
        // : enters command
    }
    EditorMode::Insert => {
        // printable chars insert
        // Enter inserts newline
        // Backspace deletes
        // Escape exits to normal
    }
    EditorMode::Command => {
        // build command string
        // Enter executes
        // Escape cancels
    }
}
```

**Character Translation**:
- Full A-Z (lowercase/uppercase with Shift)
- Numbers 0-9 (symbols with Shift)
- Punctuation (period, comma, space, etc.)
- No hardcoded ASCII assumptions
- Uses KeyCode enum, not scan codes

**Rendering**:
```
[h]ello world    ← Cursor on 'h'
second line
~                ← Empty line marker
~
NORMAL readme.txt | Saved v2
```

Status line shows: mode, dirty flag, label, messages

### Testing Strategy

**Unit Tests**: 51 tests
- State machine transitions
- Buffer operations
- Command parsing
- Cursor movement
- Rendering

**Integration Tests**: 11 tests
- Full edit sessions (insert → save → quit)
- Safety checks (quit blocked when dirty)
- Multi-line editing
- Mode switching
- Error handling

**All tests run under cargo test**:
- No terminal required
- Simulated KeyEvent injection
- Deterministic behavior
- Fast (< 1 second)

### Future Extensions

Ready for:
1. **Storage integration**: Real save/load via `services_storage`
2. **Path support**: Via `fs_view` for convenience
3. **Focus integration**: Via `services_focus_manager`
4. **Input subscription**: Via `services_input`
5. **Advanced features**:
   - Visual mode (selection)
   - Copy/paste
   - Undo/redo
   - Search/replace
   - Syntax highlighting

### Why This Matters

**Problem**: Traditional editors are hard to embed, hard to test, and tightly coupled to terminals.

**Solution**: Editor as a library component with:
- Explicit interfaces (not stdin/stdout)
- Capability-based I/O (not path-based)
- Versioned storage (not overwrite)
- Event-driven input (not byte streams)
- Full testability (no hardware)

**Impact**:
- Can embed editor in any application
- Can test editor without terminal
- Can version every document change
- Can enforce least-privilege access
- Can integrate with modern UIs

### Summary

Phase 15 provides:
- **Modal text editor**: vi-like interface without TTY coupling
- **Capability-based I/O**: Documents via capabilities, not paths
- **Versioned saves**: Immutable versions instead of overwrites
- **Component architecture**: Library, not standalone process
- **Full testability**: 62 tests without hardware

This demonstrates how classic Unix tools can be reimagined as modern, testable, capability-safe components.


---

## Phase 16: Workspace Manager — Component Orchestration

### Motivation

Traditional shells (bash, zsh) manage processes via stdin/stdout pipes and job control. This model has fundamental problems:

**Problems with POSIX Shells**:
1. **Ambient authority**: Any command inherits full shell environment
2. **Byte streams**: Unstructured stdin/stdout loses type information
3. **Implicit state**: Background jobs, environment variables, working directory
4. **Poor testability**: Hard to test shell scripts deterministically
5. **Tight coupling**: Components assume global I/O and terminal
6. **No lifecycle visibility**: Can't observe component internal state

### Solution: Workspace Manager

The Workspace Manager is **NOT** a shell. It's a **component orchestrator**:

- **Components not processes**: Manages high-level components (editor, CLI, pipeline)
- **Focus not I/O**: Routes input via explicit focus, not stdin
- **Observable lifecycle**: All component state changes are auditable
- **Capability-driven**: Components have explicit identities and policies
- **No ambient authority**: Components only have what they're explicitly granted

### Architecture

```
┌─────────────────────────────────────────────┐
│         Workspace Manager                    │
│  ┌────────────────────────────────────┐     │
│  │  Component Registry                │     │
│  │  - ComponentId → ComponentInfo     │     │
│  │  - State: Running/Exited/Cancelled │     │
│  │  - Identity & ExecutionId          │     │
│  └────────────────────────────────────┘     │
│                                              │
│  ┌────────────────────────────────────┐     │
│  │  Focus Manager Integration         │     │
│  │  - Grant/revoke focus              │     │
│  │  - Route input events              │     │
│  │  - Track focus changes             │     │
│  └────────────────────────────────────┘     │
│                                              │
│  ┌────────────────────────────────────┐     │
│  │  Policy & Budget Enforcement       │     │
│  │  - Launch policy checks            │     │
│  │  - Focus policy checks             │     │
│  │  - Budget exhaustion handling      │     │
│  └────────────────────────────────────┘     │
│                                              │
│  ┌────────────────────────────────────┐     │
│  │  Audit Trail                       │     │
│  │  - ComponentLaunched               │     │
│  │  - ComponentFocused                │     │
│  │  - ComponentTerminated             │     │
│  └────────────────────────────────────┘     │
└─────────────────────────────────────────────┘
           │
           ├─────► Editor Component
           ├─────► CLI Component
           └─────► Pipeline Executor
```

### Component Model

Each component in the workspace has:

```rust
pub struct ComponentInfo {
    pub id: ComponentId,              // Unique identifier
    pub component_type: ComponentType, // Editor, CLI, Pipeline, Custom
    pub identity: IdentityMetadata,    // ExecutionId, trust domain
    pub state: ComponentState,         // Running, Exited, Cancelled, Failed
    pub focusable: bool,               // Can receive input focus
    pub subscription: Option<InputSubscriptionCap>, // Focus capability
    pub cancellation: CancellationSource, // Lifecycle control
    pub exit_reason: Option<ExitReason>, // Why terminated
    pub name: String,                  // Human-readable name
    pub metadata: HashMap<String, String>, // Component-specific data
}
```

### Command Surface (Minimal)

The workspace provides a minimal command interface:

| Command | Example | Description |
|---------|---------|-------------|
| `open <type> [args]` | `open editor notes.txt` | Launch component |
| `list` | `list` | Show all components |
| `focus <id>` | `focus comp:abc123...` | Switch focus |
| `next` | `next` | Focus next component |
| `prev` | `prev` | Focus previous component |
| `close <id>` | `close comp:abc123...` | Terminate component |
| `status <id>` | `status comp:abc123...` | Get component info |

**What commands do NOT do**:
- Manipulate files (use `open editor` to launch editor)
- Pipe data (components use IPC)
- Redirect I/O (no stdin/stdout concept)
- Background jobs (components run independently)
- Variable expansion (pass metadata at launch)

### Focus Integration

The workspace integrates with `services_focus_manager`:

**On component launch**:
1. Create `InputSubscriptionCap` if focusable
2. Attach to component
3. Request focus (if no other component focused)

**On focus switch**:
1. Check policy (may deny cross-domain focus)
2. Release old focus
3. Grant new focus
4. Record in audit trail

**On component exit**:
1. Remove from focus stack
2. Cancel component via CancellationSource
3. Update state to Exited/Cancelled/Failed
4. Record termination in audit trail

### Policy Enforcement

Components are subject to policy at two points:

**1. Launch Time** (PolicyEvent::OnSpawn):
```rust
let context = PolicyContext::for_spawn(workspace_identity, component_identity);
let decision = policy.evaluate(PolicyEvent::OnSpawn, &context);

match decision {
    PolicyDecision::Allow { .. } => { /* launch */ },
    PolicyDecision::Deny { reason } => { /* reject */ },
    PolicyDecision::Require { action } => { /* prompt user */ },
}
```

**2. Focus Time** (PolicyEvent::OnCapabilityDelegate):
```rust
let context = PolicyContext::for_capability_delegation(
    workspace_identity, 
    component_identity, 
    subscription.id
);
let decision = policy.evaluate(PolicyEvent::OnCapabilityDelegate, &context);
```

Example policies:
- TrustDomainPolicy: Sandbox can't spawn System services
- PipelineSafetyPolicy: User pipelines must have timeouts
- Custom policies: Per-organization rules

### Budget Enforcement

Components can have resource budgets:

```rust
let budget = ResourceBudget::unlimited()
    .with_cpu_ticks(CpuTicks::new(1000))
    .with_message_count(MessageCount::new(100));

let config = LaunchConfig::new(ComponentType::Editor, "editor", ...)
    .with_budget(budget);
```

When budget exhausted:
1. Workspace calls `handle_budget_exhaustion(component_id)`
2. Component terminates with `ExitReason::Failure`
3. Focus automatically revoked
4. Audit trail records termination

### Observable Lifecycle

Every operation is recorded:

```rust
pub enum WorkspaceEvent {
    ComponentLaunched { component_id, component_type, execution_id, timestamp_ns },
    ComponentStateChanged { component_id, old_state, new_state, timestamp_ns },
    ComponentFocused { component_id, timestamp_ns },
    ComponentUnfocused { component_id, timestamp_ns },
    ComponentTerminated { component_id, reason, timestamp_ns },
}
```

Access via:
```rust
let events = workspace.audit_trail();
```

This enables:
- Debugging component failures
- Auditing policy decisions
- Understanding focus transitions
- Reconstructing workspace session history

### Comparison: Shell vs Workspace

| Aspect | Traditional Shell | PandaGen Workspace |
|--------|------------------|-------------------|
| **Abstraction** | Process + file descriptors | Component + capabilities |
| **Launch** | `command args` | `open type args` |
| **Communication** | Pipes: `cmd1 \| cmd2` | IPC channels |
| **Background** | `cmd &` (implicit) | All components independent |
| **Focus** | Terminal focus (implicit) | Explicit focus management |
| **Job control** | `fg`, `bg`, `jobs` | `focus`, `list` |
| **I/O** | stdin/stdout/stderr | Component-specific interfaces |
| **State** | Environment variables | Component metadata |
| **Policy** | None (ambient authority) | Policy engine evaluation |
| **Audit** | None | Full audit trail |
| **Testability** | Difficult | Deterministic under SimKernel |

### Testing Strategy

All tests are deterministic and run under cargo test:

**Unit Tests** (23 tests in lib.rs):
- Component creation and launch
- Focus management correctness
- State transitions (Running → Exited/Cancelled/Failed)
- Budget attachment and metadata
- Audit trail recording

**Integration Tests** (11 tests):
- Policy enforcement (allow/deny launch and focus)
- Budget exhaustion handling
- Command parsing and execution
- Multi-component focus switching
- Audit trail completeness
- Cross-domain scenarios

No hardware, no terminal, no timing issues—pure determinism.

### Why This Matters

**Problem**: POSIX shells were designed in the 1970s for byte stream processing. They don't fit modern needs:
- No type safety (everything is text)
- No lifecycle management
- No observability
- No policy enforcement
- Hard to test
- Ambient authority everywhere

**Solution**: Workspace Manager provides modern component orchestration:
- Type-safe component interfaces
- Observable lifecycle
- Auditable operations
- Policy-enforced actions
- Deterministically testable
- Explicit authority only

**Impact**:
- Users get a familiar command interface
- System gets observability and control
- Components remain independent
- Testing is trivial
- Security is enforceable
- No POSIX baggage

### Non-Goals (Enforced)

The Workspace Manager explicitly does NOT:

1. **Implement POSIX shell**: No $VAR, no &&/||, no wildcards
2. **Provide global I/O**: No stdin/stdout/stderr routing
3. **Support job control**: No suspend/resume, no &/%N
4. **Execute scripts**: No interpreter, no control flow
5. **Manage working directory**: Components use capabilities
6. **Provide environment variables**: Use component metadata
7. **Support pipes/redirects**: Components use IPC

This is intentional. The workspace orchestrates components—it doesn't interpret commands.

### Summary

Phase 16 provides:
- **Component orchestrator**: Manages components, not processes
- **Focus-driven input**: Explicit focus, not stdin
- **Observable lifecycle**: Full audit trail
- **Policy-enforced**: Launch and focus checks
- **Budget-aware**: Resource exhaustion handling
- **Testable**: 34 deterministic tests

This proves PandaGen can provide user-facing abstractions without recreating POSIX shells. The workspace is minimal, observable, and fits the PandaGen philosophy perfectly.
