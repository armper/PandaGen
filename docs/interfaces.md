# Interface Reference

This document describes the key interfaces and contracts in PandaGen.

## Table of Contents

- [Kernel API](#kernel-api)
- [Capability System](#capability-system)
- [Message Passing](#message-passing)
- [Storage Interface](#storage-interface)
- [Service Lifecycle](#service-lifecycle)
- [Hardware Abstraction](#hardware-abstraction)

## Kernel API

The `KernelApi` trait defines the interface between user space and the kernel.

### Trait Definition

```rust
pub trait KernelApi {
    fn spawn_task(&mut self, descriptor: TaskDescriptor) -> Result<TaskHandle, KernelError>;
    fn create_channel(&mut self) -> Result<ChannelId, KernelError>;
    fn send_message(&mut self, channel: ChannelId, message: MessageEnvelope) -> Result<(), KernelError>;
    fn receive_message(&mut self, channel: ChannelId, timeout: Option<Duration>) -> Result<MessageEnvelope, KernelError>;
    fn now(&self) -> Instant;
    fn sleep(&mut self, duration: Duration) -> Result<(), KernelError>;
    fn grant_capability(&mut self, task: TaskId, capability: Cap<()>) -> Result<(), KernelError>;
    fn register_service(&mut self, service_id: ServiceId, channel: ChannelId) -> Result<(), KernelError>;
    fn lookup_service(&self, service_id: ServiceId) -> Result<ChannelId, KernelError>;
}
```

### Task Spawning

**Traditional**: `fork()` duplicates the entire process state.

**PandaGen**: Explicit construction with `TaskDescriptor`.

```rust
let descriptor = TaskDescriptor::new("my_service".to_string())
    .with_capability(some_capability);

let handle = kernel.spawn_task(descriptor)?;
```

**Contract**:
- Returns `TaskHandle` with unique `TaskId`
- Task has only explicitly granted capabilities
- No inherited state (no ambient authority)

### Channel Creation

**Traditional**: `pipe()` returns two file descriptors.

**PandaGen**: Typed channel with unique ID.

```rust
let channel_id = kernel.create_channel()?;
```

**Contract**:
- Returns unique `ChannelId`
- Channel is bidirectional
- Can carry structured messages
- Can transfer capabilities

### Message Passing

**Send** (non-blocking):
```rust
kernel.send_message(channel_id, message)?;
```

**Receive** (blocking with timeout):
```rust
let message = kernel.receive_message(channel_id, Some(timeout))?;
```

**Baseline Semantics**:
- Messages are ordered per channel (FIFO)
- Send never blocks in SimulatedKernel (may fail if channel doesn't exist)
- Receive blocks until message or timeout
- Messages are typed and versioned

**Delivery Guarantees**:
- **At-most-once delivery**: A message is delivered zero or one time, never duplicated
- **No guaranteed delivery**: Messages may be lost (especially under faults)
- **Ordering preserved per channel**: Messages sent on the same channel are received in order (unless reordered by fault injection)

**Fault Injection Semantics**:

When fault injection is enabled via `FaultPlan`, the following behaviors apply:

1. **Drop**: Message is silently discarded. Sender receives `Ok(())` but receiver never sees the message.
2. **Delay**: Message is held and delivered after simulated time advances. Maintains at-most-once semantics.
3. **Reorder**: Messages in queue are swapped deterministically. No duplication occurs.
4. **Crash on Send**: `send_message` returns `Err(KernelError::SendFailed)`. Message is not enqueued.
5. **Crash on Recv**: `receive_message` returns `Err(KernelError::ReceiveFailed)`. Message may or may not be consumed.

**Safety Properties Under Faults**:
- No message duplication (at-most-once is preserved)
- No undefined behavior or panics
- State remains consistent (no partial operations)
- Faults are deterministic and reproducible (given same FaultPlan)

**Testing Guidance**:
- Systems should be designed to tolerate message loss (at-most-once semantics)
- If at-least-once delivery is needed, implement explicit acknowledgment and retry at application level
- Use fault injection to validate resilience to dropped, delayed, and reordered messages

### Time Management

**Current Time**:
```rust
let now = kernel.now();
```

**Sleep**:
```rust
kernel.sleep(Duration::from_secs(1))?;
```

**Contract**:
- Time is explicit, not ambient
- In simulated kernel, time is controllable
- Sleep yields to scheduler
- No wall-clock dependency (testable)

### Capability Management

**Grant**:
```rust
kernel.grant_capability(target_task, capability)?;
```

**Contract**:
- Caller must have the capability
- Grant is explicit and auditable
- No implicit inheritance

### Service Discovery

**Register**:
```rust
kernel.register_service(service_id, channel_id)?;
```

**Lookup**:
```rust
let channel = kernel.lookup_service(service_id)?;
```

**Contract**:
- Services identified by `ServiceId`, not paths
- Registration requires capability
- Lookup returns communication channel
- No global namespace pollution

## Capability System

### Cap<T> Type

```rust
pub struct Cap<T> {
    id: u64,
    _phantom: PhantomData<T>,
}
```

**Properties**:
- **Unforgeable**: Can only be created by kernel
- **Typed**: `Cap<FileRead>` ≠ `Cap<FileWrite>`
- **Transferable**: Can be passed in messages
- **Traceable**: Unique ID for auditing
- **Move-only by default**: Capabilities use move semantics (no implicit cloning)

### Capability Lifecycle

PandaGen implements a rigorous capability lifecycle model with explicit operations and strong enforcement:

#### Lifecycle Operations

1. **Grant**: Initial capability issuance from kernel/authority to a task
   - Only the kernel or authorized services can grant capabilities
   - Creates an entry in the capability authority table
   - Recorded in audit log

2. **Delegate/Transfer**: Move ownership from one task to another
   - **Move semantics**: Original owner loses access after delegation
   - Validates that source task owns the capability
   - Updates ownership in authority table
   - Recorded in audit log

3. **Drop**: Explicit release of a capability
   - Owner voluntarily releases the capability
   - Capability becomes invalid
   - Recorded in audit log

4. **Invalidate**: Automatic invalidation on owner death
   - When a task terminates (crash or normal exit), all its capabilities are invalidated
   - Prevents use-after-free and dangling capability references
   - Recorded in audit log

#### Capability Semantics

**Move-Only Transfer** (default):
- When a capability is delegated from Task A to Task B, Task A can no longer use it
- No implicit cloning or duplication
- Clear ownership model: exactly one task owns a capability at any time
- Prevents confused deputy attacks and capability leaks

**Exception**: Some service capabilities (like Storage object capabilities) may be marked as "durable" and survive service restarts, but this is explicit and documented.

### Creating Capabilities

```rust
// Only trusted code (kernel) can do this
let cap: Cap<FileRead> = Cap::new(42);
```

### Granting Capabilities

```rust
// Kernel API
kernel.grant_capability(task_id, cap)?;

// This creates an authority table entry:
// - cap_id: 42
// - owner: task_id
// - status: Valid
```

### Delegating Capabilities (Move Semantics)

```rust
// Move ownership from task1 to task2
kernel.delegate_capability(cap_id, task1, task2)?;

// After delegation:
// - task1 can NO LONGER use cap_id
// - task2 is the new owner
// - Audit log records the delegation
```

**Enforcement**:
- `delegate_capability` validates that `task1` currently owns `cap_id`
- Returns error if task doesn't own the capability
- Returns error if target task doesn't exist

### Dropping Capabilities

```rust
// Explicitly release a capability
kernel.drop_capability(cap_id, task_id)?;

// Capability becomes invalid
// Cannot be used again
```

### Lifetime Rules

1. **Task-bound capabilities**: Most capabilities are bound to their owner task
   - When the task terminates, capabilities are automatically invalidated
   - No manual cleanup needed in most cases

2. **Durable capabilities**: Some capabilities survive owner death (e.g., Storage object capabilities)
   - Explicitly marked as durable in service design
   - Tied to service identity, not individual task
   - Must be explicitly documented why durability is needed

3. **Validation before use**: Every capability operation validates:
   - Capability exists in authority table
   - Capability status is Valid (not Transferred or Invalid)
   - Owner task is still alive
   - Requesting task is the current owner

### Audit Trail (Simulation/Test Mode)

SimulatedKernel maintains a capability audit log for testing:

```rust
// Access audit log
let audit = kernel.audit_log();

// Query events
let events = audit.get_events_for_cap(cap_id);
let grant_count = audit.count_events(|e| matches!(e, CapabilityEvent::Granted { .. }));

// Verify no leaks
assert!(!audit.has_event(|e| matches!(e, CapabilityEvent::InvalidUseAttempt { .. })));
```

**Audit Events**:
- `Granted`: Capability issued to a task
- `Delegated`: Capability transferred between tasks
- `Cloned`: Capability duplicated (rare, must be explicit)
- `Dropped`: Capability explicitly released
- `InvalidUseAttempt`: Failed attempt to use invalid capability
- `Invalidated`: Capability invalidated due to owner termination

### Security Properties

**Enforced by SimulatedKernel**:
1. No capability forgery (only kernel creates capabilities)
2. No capability use after transfer (move semantics)
3. No capability use after owner death (automatic invalidation)
4. No capability leak through message loss (fault injection tested)

**Future Real Kernel**:
- Will enforce same semantics at syscall boundary
- Capability table maintained in kernel space
- User space cannot manipulate capability ownership
- Hardware memory protection prevents capability forgery

### Transferring Capabilities (Legacy Documentation)

```rust
let transfer = CapabilityTransfer::new(cap, from_task, to_task);
let transferred = transfer.complete();
```

**Note**: This is a helper type for message-based transfers. The actual enforcement happens via `kernel.delegate_capability()`.

**Contract**:
- Type system prevents capability confusion
- Cannot cast `Cap<T>` to `Cap<U>`
- Compiler enforces correct usage
- Runtime enforces ownership and liveness

## Message Passing

### Message Envelope

```rust
pub struct MessageEnvelope {
    pub id: MessageId,
    pub destination: ServiceId,
    pub source: Option<TaskId>,
    pub action: String,
    pub schema_version: SchemaVersion,
    pub correlation_id: Option<MessageId>,
    pub payload: MessagePayload,
}
```

### Creating Messages

```rust
let message = MessageEnvelope::new(
    destination,
    "action.name".to_string(),
    SchemaVersion::new(1, 0),
    payload,
);
```

### Typed Payloads

```rust
#[derive(Serialize, Deserialize)]
struct MyPayload {
    field: String,
}

let payload = MessagePayload::new(&MyPayload {
    field: "value".to_string(),
})?;
```

### Schema Versioning

```rust
let v1_0 = SchemaVersion::new(1, 0);
let v1_1 = SchemaVersion::new(1, 1);

// Check compatibility
if v1_0.is_compatible_with(&v1_1) {
    // Same major version = compatible
}
```

**Contract**:
- Major version change = breaking
- Minor version change = backward compatible
- Receiver checks version before deserializing
- Mismatch is an error, not undefined behavior

### IPC Schema Evolution Policy

PandaGen implements a disciplined, testable evolution model for IPC message schemas.

#### Schema Version Semantics

Every `MessageEnvelope` contains a `schema_version` field with two components:
- **Major version**: Incremented for breaking changes
- **Minor version**: Incremented for backward-compatible changes

```rust
pub struct SchemaVersion {
    pub major: u32,  // Breaking changes
    pub minor: u32,  // Backward-compatible additions
}
```

#### Breaking vs Non-Breaking Changes

**NON-BREAKING Changes** (increment minor version only):
- Adding optional fields to message payloads
- Adding new action types (methods)
- Adding new error variants (as long as unknown errors are handled gracefully)
- Relaxing validation rules
- Adding new metadata fields to envelopes

Examples:
```rust
// v1.0: Original payload
struct RequestV1 {
    name: String,
}

// v1.1: Added optional field (non-breaking)
struct RequestV1_1 {
    name: String,
    #[serde(default)]
    timeout: Option<Duration>,
}
```

**BREAKING Changes** (increment major version):
- Removing fields from payloads
- Renaming fields (without backward-compatibility shims)
- Changing field types
- Changing field semantics (same name, different meaning)
- Removing action types (methods)
- Reordering required fields (if using positional encoding)
- Making optional fields required
- Tightening validation rules

Examples:
```rust
// v1.0: Original
struct RequestV1 {
    name: String,
    size: u32,  // in bytes
}

// v2.0: Changed semantics (breaking)
struct RequestV2 {
    name: String,
    size: u32,  // NOW in kilobytes - BREAKING!
}
```

#### Supported Version Window Policy

PandaGen uses a **"current + previous major version"** policy:
- Services MUST support the current major version
- Services SHOULD support the previous major version (N-1)
- Services MAY reject versions older than N-1
- All minor versions within a major version are compatible

Example:
- If current version is v3.x, service must support v3.x and should support v2.x
- Service may reject v1.x requests with explicit error

This policy:
- Avoids infinite backward compatibility (not a legacy system)
- Allows controlled evolution
- Provides migration window for upgrades
- Keeps implementation complexity bounded

#### Version Negotiation and Error Handling

When a service receives a message with an unsupported schema version:

1. **Check version compatibility**:
   ```rust
   let policy = VersionPolicy::new(current_major, current_minor);
   match policy.check_compatibility(&incoming_version) {
       Compatibility::Compatible => { /* process message */ }
       Compatibility::UpgradeRequired => {
           // Sender too old, return upgrade error
           return Err(SchemaMismatchError::upgrade_required(...));
       }
       Compatibility::Unsupported => {
           // Version too new or too old
           return Err(SchemaMismatchError::unsupported(...));
       }
   }
   ```

2. **Return explicit error**: Never fail silently or with generic errors
   - Error MUST include: expected version range, received version, service identity
   - Error SHOULD suggest remediation (upgrade sender, downgrade sender, wait for service update)

3. **Log the mismatch**: For debugging and monitoring
   - Track version mismatch patterns
   - Identify clients needing upgrades

#### Error Response Format

```rust
pub enum SchemaMismatchError {
    /// Sender is using too old a version
    UpgradeRequired {
        service: ServiceId,
        expected_min: SchemaVersion,
        received: SchemaVersion,
    },
    /// Version is not supported (too new or too old)
    Unsupported {
        service: ServiceId,
        supported_range: (SchemaVersion, SchemaVersion),
        received: SchemaVersion,
    },
}
```

#### Testing Schema Evolution

Contract tests MUST verify:
- Envelope structure remains stable across versions
- Schema version policy is enforced
- Version mismatch errors are explicit and actionable
- Services correctly reject unsupported versions

Example:
```rust
#[test]
fn test_reject_too_old_version() {
    let policy = VersionPolicy::current(3, 0).with_min_major(2);
    let old_version = SchemaVersion::new(1, 9);
    
    assert_eq!(
        policy.check_compatibility(&old_version),
        Compatibility::Unsupported
    );
}
```

#### Philosophy

- **Explicit over implicit**: Version checks are explicit in code, not magical
- **Testability first**: Version logic is pure functions, fully testable
- **Bounded compatibility**: No "forever support" - controlled evolution
- **Clear errors**: When versions mismatch, debugging is straightforward
- **No negotiation overhead**: Static policies enforced by tests, not runtime discovery

### Correlation IDs

```rust
// Request
let request = MessageEnvelope::new(...);
let request_id = request.id;

// Response
let response = MessageEnvelope::new(...)
    .with_correlation(request_id);
```

**Contract**:
- Response includes request's message ID
- Enables request/response matching
- Useful for RPC patterns

## Storage Interface

### Object Types

```rust
pub enum ObjectKind {
    Blob,  // Immutable bytes
    Log,   // Append-only
    Map,   // Key-value
}
```

### Object Identifiers

```rust
let object_id = ObjectId::new();
let version_id = VersionId::new();
```

**Contract**:
- IDs are unique, not paths
- Every modification creates new version
- Old versions remain accessible
- No implicit hierarchy

### Transactions

```rust
let mut tx = Transaction::new();
tx.modify(object_id)?;
tx.commit()?;
```

**States**:
- `Active`: Can perform operations
- `Committed`: Changes are permanent
- `RolledBack`: Changes are discarded

**Contract**:
- All modifications are atomic
- Can rollback before commit
- Cannot modify after finalization

### Transactional Storage Trait

```rust
pub trait TransactionalStorage {
    fn begin_transaction(&mut self) -> Result<Transaction, TransactionError>;
    fn read(&self, tx: &Transaction, object_id: ObjectId) -> Result<VersionId, TransactionError>;
    fn write(&mut self, tx: &mut Transaction, object_id: ObjectId, data: &[u8]) -> Result<VersionId, TransactionError>;
    fn commit(&mut self, tx: &mut Transaction) -> Result<(), TransactionError>;
    fn rollback(&mut self, tx: &mut Transaction) -> Result<(), TransactionError>;
}
```

## Service Lifecycle

### Service Descriptor

```rust
pub struct ServiceDescriptor {
    pub service_id: ServiceId,
    pub name: String,
    pub restart_policy: RestartPolicy,
    pub capabilities: Vec<Cap<()>>,
    pub dependencies: Vec<ServiceId>,
}
```

### Restart Policies

```rust
pub enum RestartPolicy {
    Never,
    Always,
    OnFailure,
    ExponentialBackoff { max_attempts: u32 },
}
```

**Contract**:
- Policy is explicit, not implicit
- Process manager enforces policy
- No shell scripts or external config

### Lifecycle States

```rust
pub enum LifecycleState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Restarting,
}
```

**Transitions**:
- `Starting` → `Running`: Startup successful
- `Running` → `Stopping`: Graceful shutdown initiated
- `Stopping` → `Stopped`: Shutdown complete
- `Running` → `Failed`: Unexpected failure
- `Failed` → `Restarting`: Policy triggers restart

### Service Handle

```rust
pub struct ServiceHandle {
    pub task_id: TaskId,
    pub state: LifecycleState,
}
```

## Hardware Abstraction

### CPU HAL

```rust
pub trait CpuHal {
    fn halt(&self);
    fn stack_pointer(&self) -> usize;
    fn instruction_pointer(&self) -> usize;
    fn cpu_id(&self) -> u32;
}
```

### Memory HAL

```rust
pub trait MemoryHal {
    fn allocate_page(&mut self) -> Result<usize, MemoryError>;
    fn free_page(&mut self, address: usize) -> Result<(), MemoryError>;
    fn map_page(&mut self, virtual_addr: usize, physical_addr: usize, 
                writable: bool, executable: bool) -> Result<(), MemoryError>;
    fn unmap_page(&mut self, virtual_addr: usize) -> Result<(), MemoryError>;
}
```

### Interrupt HAL

```rust
pub trait InterruptHal {
    fn enable_interrupts(&mut self);
    fn disable_interrupts(&mut self);
    fn interrupts_enabled(&self) -> bool;
    fn register_handler(&mut self, vector: u8, handler: fn());
}
```

**Contract**:
- Architecture-specific details hidden behind traits
- Core logic remains architecture-independent
- Can swap implementations (x86_64, ARM, RISC-V)

## Error Handling

### Kernel Errors

```rust
pub enum KernelError {
    SpawnFailed(String),
    ChannelError(String),
    SendFailed(String),
    ReceiveFailed(String),
    Timeout,
    ServiceNotFound(String),
    ServiceAlreadyRegistered(String),
    InsufficientAuthority,
    InvalidCapability,
    ResourceExhausted(String),
}
```

**Philosophy**:
- Errors are explicit, not error codes
- Descriptive messages for debugging
- Type-safe (using `thiserror`)

## Testing Contracts

### Unit Tests

Every crate must have unit tests demonstrating:
- Basic functionality
- Error conditions
- Edge cases
- Type safety

### Integration Tests

Services should have integration tests using `SimulatedKernel`:
- Service startup
- Message handling
- Capability usage
- State transitions

### Example

```rust
#[test]
fn test_service_communication() {
    let mut kernel = SimulatedKernel::new();
    
    // Setup
    let channel = kernel.create_channel()?;
    let service_id = ServiceId::new();
    kernel.register_service(service_id, channel)?;
    
    // Test
    let message = create_test_message(service_id);
    kernel.send_message(channel, message.clone())?;
    let received = kernel.receive_message(channel, None)?;
    
    // Assert
    assert_eq!(received.id, message.id);
}
```

## Design Guidelines

### For Interface Designers

1. **Make it trait-based**: Enable multiple implementations
2. **Make it testable**: Should work under `cargo test`
3. **Make it explicit**: No ambient authority or hidden state
4. **Make it typed**: Use the type system for safety
5. **Document the why**: Explain design decisions

### For Implementation

1. **Prefer composition over inheritance**
2. **Keep unsafe code minimal and isolated**
3. **Use type-state pattern for state machines**
4. **Make illegal states unrepresentable**
5. **Test everything**

### For Evolution

1. **Version all schemas**
2. **Maintain backward compatibility within major version**
3. **Document breaking changes**
4. **Provide migration paths**
5. **Consider testability**

## Pipeline Interface

### Typed Pipeline Composition

PandaGen provides a typed pipeline system for composing operations safely.

Unlike shell pipelines (`cmd1 | cmd2 | cmd3`), PandaGen pipelines are:
- **Typed**: Schema-validated input/output chaining
- **Capability-safe**: Explicit authority flow, no ambient privileges
- **Failure-explicit**: Bounded retry policies, no infinite loops
- **Deterministic**: Works with SimKernel for reproducible testing

#### Core Types

```rust
pub struct PipelineSpec {
    pub id: PipelineId,
    pub name: String,
    pub stages: Vec<StageSpec>,
    pub initial_input_schema: PayloadSchemaId,
    pub final_output_schema: PayloadSchemaId,
}

pub struct StageSpec {
    pub id: StageId,
    pub name: String,
    pub handler: ServiceId,
    pub action: String,
    pub input_schema: PayloadSchemaId,
    pub output_schema: PayloadSchemaId,
    pub retry_policy: RetryPolicy,
    pub required_capabilities: Vec<u64>,
}

pub enum StageResult {
    Success { output: TypedPayload, capabilities: Vec<u64> },
    Failure { error: String },
    Retryable { error: String },
}

pub struct RetryPolicy {
    pub max_retries: u32,            // 0 = no retries
    pub initial_backoff_ms: u64,
    pub backoff_multiplier: f64,     // For exponential backoff
}
```

#### Pipeline Validation

Pipelines validate schema chaining at construction:

```rust
// Schema chaining validation
pipeline.validate()?;

// Checks:
// 1. At least one stage
// 2. First stage input matches pipeline input
// 3. Each stage output matches next stage input
// 4. Last stage output matches pipeline output
```

**Contract**:
- Validation happens before execution
- Invalid pipelines return `PipelineError::SchemaMismatch`
- Schema IDs are string-based with version tags

#### Capability Flow

Stages explicitly declare required capabilities:

```rust
let stage2 = StageSpec::new(...)
    .with_capabilities(vec![cap_id_from_stage1]);
```

**Capability Rules**:
1. Stage cannot execute without required capabilities
2. Capabilities come from:
   - Initial pipeline inputs, OR
   - Output of previous stages
3. Missing capabilities cause immediate failure (fail-fast)
4. No capability forgery or ambient authority

**Enforcement**:
- Executor checks capability availability before stage execution
- Missing capability returns `ExecutorError::MissingCapability`
- Capability IDs tracked in execution trace

#### Failure Propagation

Every stage returns one of three outcomes:

**Success**:
```rust
StageResult::Success {
    output: TypedPayload,
    capabilities: Vec<u64>,
}
```
- Stage succeeded
- Pipeline continues to next stage
- Capabilities added to executor's pool

**Permanent Failure**:
```rust
StageResult::Failure {
    error: String,
}
```
- Stage failed permanently (non-recoverable)
- Pipeline stops immediately (fail-fast)
- No subsequent stages execute
- Trace records failure point

**Retryable Failure**:
```rust
StageResult::Retryable {
    error: String,
}
```
- Stage failed transiently (may succeed on retry)
- Retry according to stage's `RetryPolicy`
- If max retries exceeded, converts to permanent failure
- Backoff uses SimKernel time (deterministic)

#### Retry Policies

**No Retries** (default):
```rust
RetryPolicy::none()
// max_retries = 0
// Immediate failure on any error
```

**Fixed Retries**:
```rust
RetryPolicy::fixed_retries(3, 100)
// max_retries = 3
// initial_backoff_ms = 100
// backoff_multiplier = 1.0 (constant backoff)
```

**Exponential Backoff**:
```rust
RetryPolicy::exponential_backoff(3, 100)
// max_retries = 3
// initial_backoff_ms = 100
// backoff_multiplier = 2.0
// Backoff: 100ms, 200ms, 400ms
```

**Retry Rules**:
- `attempt = 0` is the first attempt (not a retry)
- `attempt = 1` is the first retry
- Max retries is inclusive (3 retries = 4 total attempts)
- Backoff happens BEFORE retry, not after failure
- Backoff uses `kernel.sleep(Duration)` (deterministic in tests)
- After max retries, stage returns permanent failure

**Backoff Calculation**:
```rust
backoff_duration(attempt) = initial_backoff_ms * (backoff_multiplier ^ attempt)
```

#### Execution Trace

Pipeline execution records a minimal trace:

```rust
pub struct ExecutionTrace {
    pub pipeline_id: PipelineId,
    pub entries: Vec<StageTraceEntry>,
    pub final_result: PipelineExecutionResult,
}

pub struct StageTraceEntry {
    pub stage_id: StageId,
    pub stage_name: String,
    pub start_time_ms: u64,      // SimKernel time
    pub end_time_ms: u64,
    pub attempt: u32,            // 0 = first attempt, 1+ = retries
    pub result: StageExecutionResult,
    pub capabilities_in: Vec<u64>,
    pub capabilities_out: Vec<u64>,
}
```

**Trace Properties**:
- One entry per execution attempt (including retries)
- Timestamps are deterministic (SimKernel time)
- Capability IDs recorded (not full capabilities)
- Minimal data (not a full observability platform)
- Test-visible for assertions

**Contract**:
- Trace is returned with pipeline result
- Successful pipelines have entries for all stages
- Failed pipelines have entries up to failure point
- Retried stages have multiple entries (one per attempt)

#### Example Usage

```rust
use pipeline::{PipelineSpec, StageSpec, RetryPolicy};
use services_pipeline_executor::PipelineExecutor;

// Define stages
let stage1 = StageSpec::new(
    "CreateBlob",
    storage_service_id,
    "create",
    PayloadSchemaId::new("blob_params"),
    PayloadSchemaId::new("blob_capability"),
);

let stage2 = StageSpec::new(
    "TransformBlob",
    transformer_service_id,
    "transform",
    PayloadSchemaId::new("blob_capability"),
    PayloadSchemaId::new("transformed_blob"),
)
.with_capabilities(vec![blob_cap_id])
.with_retry_policy(RetryPolicy::fixed_retries(2, 50));

// Build pipeline
let pipeline = PipelineSpec::new(
    "blob_pipeline",
    PayloadSchemaId::new("blob_params"),
    PayloadSchemaId::new("transformed_blob"),
)
.add_stage(stage1)
.add_stage(stage2);

// Validate
pipeline.validate()?;

// Execute
let mut executor = PipelineExecutor::new();
executor.add_capabilities(initial_caps);

let (output, trace) = executor.execute(
    &mut kernel,
    &pipeline,
    input_payload,
)?;

// Verify
assert_eq!(trace.final_result, PipelineExecutionResult::Success);
assert_eq!(trace.entries.len(), 2); // No retries
```

#### Safety Properties

Pipelines maintain these invariants:
1. **Schema Safety**: Type mismatches detected at validation
2. **Capability Safety**: No authority leaks through composition
3. **Bounded Execution**: No infinite retries or loops
4. **Fail-Fast**: First permanent failure stops pipeline
5. **Deterministic Timing**: Backoff uses kernel time (testable)
6. **Trace Completeness**: All executed stages recorded

#### Testing Guidelines

**Happy Path**:
- Verify all stages execute
- Check final output schema matches expected
- Validate capability flow (trace shows correct cap IDs)

**Failure Path**:
- Inject failure in middle stage
- Assert later stages don't execute
- Verify trace stops at failure point

**Retry Path**:
- Configure retry policy
- Inject transient failures
- Verify retry attempts match policy
- Check backoff timing in trace

**Fault Injection**:
- Use SimKernel fault injection (Phase 2)
- Test message drop/delay/reorder
- Verify pipelines remain safe under faults
- Assert no capability leaks

## Execution Identity and Supervision

### Execution Identity Types

PandaGen introduces explicit execution identity for supervision and audit.

```rust
/// Unique identifier for an execution context
pub struct ExecutionId(Uuid);

/// Type of execution context
pub enum IdentityKind {
    System,         // Core system component
    Service,        // User-space service
    Component,      // Application component
    PipelineStage,  // Pipeline stage execution
}

/// Trust domain tag
pub struct TrustDomain(String);

impl TrustDomain {
    pub fn core() -> Self;      // "core"
    pub fn user() -> Self;      // "user"
    pub fn sandbox() -> Self;   // "sandbox"
    pub fn new(name: impl Into<String>) -> Self;
}
```

### Identity Metadata

```rust
pub struct IdentityMetadata {
    pub execution_id: ExecutionId,
    pub kind: IdentityKind,
    pub task_id: Option<TaskId>,
    pub parent_id: Option<ExecutionId>,
    pub creator_id: Option<ExecutionId>,
    pub created_at_nanos: u64,
    pub trust_domain: TrustDomain,
    pub name: String,
}

impl IdentityMetadata {
    pub fn new(
        kind: IdentityKind,
        trust_domain: TrustDomain,
        name: impl Into<String>,
        created_at_nanos: u64,
    ) -> Self;
    
    pub fn with_task_id(self, task_id: TaskId) -> Self;
    pub fn with_parent(self, parent_id: ExecutionId) -> Self;
    pub fn with_creator(self, creator_id: ExecutionId) -> Self;
    
    pub fn same_domain(&self, other: &IdentityMetadata) -> bool;
    pub fn is_child_of(&self, parent_id: ExecutionId) -> bool;
}
```

**Contract**:
- Identity metadata is immutable after creation
- ExecutionId is never reused (even after termination)
- Identity does NOT grant authority (capabilities do)
- Parent/child relationships are structural, not access control

### Exit Notifications

```rust
pub enum ExitReason {
    Normal,
    Failure { error: String },
    Cancelled { reason: String },
    Timeout,
}

pub struct ExitNotification {
    pub execution_id: ExecutionId,
    pub task_id: Option<TaskId>,
    pub reason: ExitReason,
    pub terminated_at_nanos: u64,
}
```

**Contract**:
- Every task termination generates an ExitNotification
- Exit notifications are available to all (supervision checks parent-child)
- Exit reason is structural information, not enforcement
- Supervisor is responsible for interpreting and acting on notifications

### SimKernel Identity Operations

```rust
impl SimulatedKernel {
    /// Create identity with metadata
    pub fn create_identity(&mut self, metadata: IdentityMetadata) -> ExecutionId;
    
    /// Get identity metadata
    pub fn get_identity(&self, execution_id: ExecutionId) -> Option<&IdentityMetadata>;
    
    /// Get execution ID for a task
    pub fn get_task_identity(&self, task_id: TaskId) -> Option<ExecutionId>;
    
    /// Spawn task with full identity control
    pub fn spawn_task_with_identity(
        &mut self,
        descriptor: TaskDescriptor,
        kind: IdentityKind,
        trust_domain: TrustDomain,
        parent_id: Option<ExecutionId>,
        creator_id: Option<ExecutionId>,
    ) -> Result<(TaskHandle, ExecutionId), KernelError>;
    
    /// Terminate task with specific exit reason
    pub fn terminate_task_with_reason(&mut self, task_id: TaskId, reason: ExitReason);
    
    /// Get exit notifications (for supervision)
    pub fn get_exit_notifications(&self) -> &[ExitNotification];
    
    /// Clear exit notifications after processing
    pub fn clear_exit_notifications(&mut self);
}
```

**Usage Example**:

```rust
// Supervisor spawns child
let (child_handle, child_exec_id) = kernel.spawn_task_with_identity(
    TaskDescriptor::new("worker".to_string()),
    IdentityKind::Component,
    TrustDomain::user(),
    Some(supervisor_exec_id),  // Parent
    Some(supervisor_exec_id),  // Creator
)?;

// Later, check for child termination
let notifications = kernel.get_exit_notifications();
for notif in notifications {
    if notif.execution_id == child_exec_id {
        match notif.reason {
            ExitReason::Normal => {
                // Child completed successfully
            }
            ExitReason::Failure { error } => {
                // Child crashed - maybe restart
                eprintln!("Child failed: {}", error);
            }
            ExitReason::Timeout => {
                // Child took too long
            }
            ExitReason::Cancelled { reason } => {
                // Intentional cancellation
            }
        }
    }
}
kernel.clear_exit_notifications();
```

### Trust Boundaries

Trust boundaries are enforced through audit, not blocking:

```rust
// Same trust domain - delegation proceeds normally
let task1 = spawn_in_domain(TrustDomain::core());
let task2 = spawn_in_domain(TrustDomain::core());
kernel.delegate_capability(cap_id, task1, task2)?;
// No special audit event

// Cross trust domain - delegation proceeds but is audited
let task3 = spawn_in_domain(TrustDomain::user());
kernel.delegate_capability(cap_id, task1, task3)?;
// Audit log records CapabilityEvent::CrossDomainDelegation

// Tests can verify trust boundary behavior
let audit = kernel.audit_log();
assert!(audit.has_event(|e| matches!(
    e,
    CapabilityEvent::CrossDomainDelegation {
        from_domain, to_domain, ..
    } if from_domain == "core" && to_domain == "user"
)));
```

**Contract**:
- Trust domains are string-based tags, not enforcement boundaries
- Cross-domain delegation is allowed but audited
- Tests verify correct audit events are generated
- Future: explicit policies for blocking cross-domain delegation

### Supervision Rules

**Identity-based supervision** (future work in services_process_manager):

1. **Supervisor owns children**: Only the parent can restart/terminate direct children
2. **Exit notifications**: Supervisor receives structured exit information
3. **Restart policies**: Per-child restart policy (Never, Always, OnFailure, Backoff)
4. **No global control**: Cannot control unrelated identities

**Example supervision pattern**:

```rust
struct Supervisor {
    exec_id: ExecutionId,
    children: HashMap<ExecutionId, ChildInfo>,
}

struct ChildInfo {
    task_id: TaskId,
    restart_policy: RestartPolicy,
    restart_count: u32,
}

impl Supervisor {
    fn handle_exit_notifications(&mut self, kernel: &mut SimKernel) {
        for notif in kernel.get_exit_notifications() {
            if let Some(child) = self.children.get_mut(&notif.execution_id) {
                // This is our child - we own it
                match (notif.reason, &child.restart_policy) {
                    (ExitReason::Normal, _) => {
                        // Clean exit - don't restart
                        self.children.remove(&notif.execution_id);
                    }
                    (ExitReason::Failure { .. }, RestartPolicy::OnFailure) => {
                        // Restart with backoff
                        self.restart_child(kernel, notif.execution_id)?;
                    }
                    _ => {
                        // Other policy/reason combinations
                    }
                }
            }
            // Not our child - ignore
        }
        kernel.clear_exit_notifications();
    }
}
```

### Design Guidelines

**For Identity**:
1. Identity is for supervision and audit, not access control
2. Authority comes from capabilities, not identity
3. Use trust domains as structural hints, not security boundaries
4. Parent-child relationships are immutable (no reparenting)

**For Supervision**:
1. Only supervise your direct children (check parent_id)
2. Use exit notifications, don't poll task status
3. Restart policies should be explicit and bounded
4. Log supervision actions for audit

**For Trust Boundaries**:
1. Cross-domain delegation should be intentional
2. Test trust boundary behavior with audit assertions
3. Document why cross-domain delegation is needed
4. Future: explicit delegation policies per trust domain pair

## Policy Engine Interface

### Policy Engine Trait

Policy engines evaluate system operations and return decisions.

```rust
pub trait PolicyEngine: Send + Sync {
    /// Evaluates a policy for the given event and context
    ///
    /// Must be deterministic: same inputs always produce same outputs.
    /// Must be side-effect free: does not modify system state.
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision;

    /// Returns the name of this policy engine (for logging/audit)
    fn name(&self) -> &str;
}
```

**Contract**:
- Deterministic evaluation (same input → same output)
- Side-effect free (pure function)
- Thread-safe (Send + Sync)
- Returns explicit decision (Allow, Deny, or Require)

### Policy Decision Types

```rust
pub enum PolicyDecision {
    /// Operation is allowed to proceed
    Allow,
    /// Operation is denied with a specific reason
    Deny { reason: String },
    /// Operation requires additional action before proceeding
    Require { action: String },
}
```

**Semantics**:
- **Allow**: Operation may proceed without restrictions
- **Deny**: Operation is blocked; enforcement point returns error
- **Require**: Operation needs modification or approval before proceeding

### Policy Context

```rust
pub struct PolicyContext {
    /// Execution identity performing the operation
    pub actor_identity: IdentityMetadata,
    /// Target identity (if applicable)
    pub target_identity: Option<IdentityMetadata>,
    /// Capability involved (if any)
    pub capability_id: Option<u64>,
    /// Pipeline ID (if applicable)
    pub pipeline_id: Option<PipelineId>,
    /// Stage ID (if applicable)
    pub stage_id: Option<StageId>,
    /// Additional context-specific data
    pub metadata: Vec<(String, String)>,
}
```

**Usage Examples**:

```rust
// Context for spawn operation
let context = PolicyContext::for_spawn(
    creator_identity,
    new_task_identity,
);

// Context for capability delegation
let context = PolicyContext::for_capability_delegation(
    from_identity,
    to_identity,
    cap_id,
);

// Context for pipeline execution with metadata
let context = PolicyContext::for_pipeline(
    executor_identity,
    pipeline_id,
)
.with_metadata("timeout_ms", "5000")
.with_metadata("stage_count", "3");
```

### Policy Events

```rust
pub enum PolicyEvent {
    /// Task/service spawn
    OnSpawn,
    /// Task/service termination
    OnTerminate,
    /// Capability delegation between tasks
    OnCapabilityDelegate,
    /// Pipeline execution start
    OnPipelineStart,
    /// Pipeline stage start
    OnPipelineStageStart,
    /// Pipeline stage end
    OnPipelineStageEnd,
}
```

**When Each Event Triggers**:
- **OnSpawn**: Before creating new task/service execution
- **OnTerminate**: Before terminating a task (future use)
- **OnCapabilityDelegate**: Before transferring capability ownership
- **OnPipelineStart**: Before starting pipeline execution
- **OnPipelineStageStart**: Before executing each pipeline stage
- **OnPipelineStageEnd**: After each pipeline stage completes

### Enforcement Points

Policy enforcement is **optional** and **explicit**.

**1. Task Spawn (SimKernel)**:

```rust
// Policy is checked during spawn_task_with_identity
pub fn spawn_task_with_identity(
    &mut self,
    descriptor: TaskDescriptor,
    kind: IdentityKind,
    trust_domain: TrustDomain,
    parent_id: Option<ExecutionId>,
    creator_id: Option<ExecutionId>,
) -> Result<(TaskHandle, ExecutionId), KernelError>;
```

**Enforcement Behavior**:
- If no policy engine: operation proceeds
- If policy returns Allow: operation proceeds
- If policy returns Deny: returns `KernelError::InsufficientAuthority`
- If policy returns Require: returns `KernelError::InsufficientAuthority`

**2. Capability Delegation (SimKernel)**:

```rust
// Policy is checked during delegate_capability
pub fn delegate_capability(
    &mut self,
    cap_id: u64,
    from_task: TaskId,
    to_task: TaskId,
) -> Result<(), KernelError>;
```

**Enforcement Behavior**:
- Same as spawn: Deny/Require → error, Allow → proceed
- Also checks trust domain boundaries (logged in capability audit)

**3. Pipeline Execution (Future - services_pipeline_executor)**:

```rust
// Policy would be checked during pipeline execution start
pub fn execute(
    &mut self,
    kernel: &mut impl KernelApi,
    pipeline: &PipelineSpec,
    input: TypedPayload,
) -> Result<(TypedPayload, ExecutionTrace), ExecutorError>;
```

**Setting Policy Engine**:

```rust
// Create kernel with policy
let kernel = SimulatedKernel::new()
    .with_policy_engine(Box::new(TrustDomainPolicy));

// Or without policy (all operations allowed)
let kernel = SimulatedKernel::new();  // No policy = Allow all
```

### Reference Policy Implementations

**NoOpPolicy** (always allows):
```rust
pub struct NoOpPolicy;

impl PolicyEngine for NoOpPolicy {
    fn evaluate(&self, _event: PolicyEvent, _context: &PolicyContext) -> PolicyDecision {
        PolicyDecision::Allow
    }
    
    fn name(&self) -> &str {
        "NoOpPolicy"
    }
}
```

**TrustDomainPolicy** (sandbox restrictions):
```rust
pub struct TrustDomainPolicy;

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
    
    fn name(&self) -> &str {
        "TrustDomainPolicy"
    }
}
```

**PipelineSafetyPolicy** (timeout requirements):
```rust
pub struct PipelineSafetyPolicy {
    pub max_stages_unsupervised: usize,
}

impl PolicyEngine for PipelineSafetyPolicy {
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        match event {
            PolicyEvent::OnPipelineStart => {
                // User pipelines must have timeout
                if context.actor_identity.trust_domain == TrustDomain::user() {
                    if !context.metadata.iter().any(|(k, _)| k == "timeout_ms") {
                        return PolicyDecision::require("Pipelines in user domain must specify timeout");
                    }
                }
                
                // Check stage count
                if let Some((_, count_str)) = context.metadata.iter().find(|(k, _)| k == "stage_count") {
                    if let Ok(count) = count_str.parse::<usize>() {
                        if count > self.max_stages_unsupervised {
                            return PolicyDecision::require(
                                format!("Pipelines with {} stages require supervision", count)
                            );
                        }
                    }
                }
                
                PolicyDecision::Allow
            }
            _ => PolicyDecision::Allow,
        }
    }
    
    fn name(&self) -> &str {
        "PipelineSafetyPolicy"
    }
}
```

### Policy Composition

Multiple policies can be composed with precedence rules:

```rust
pub struct ComposedPolicy {
    policies: Vec<Box<dyn PolicyEngine>>,
}

impl ComposedPolicy {
    pub fn new() -> Self;
    pub fn add_policy(self, policy: Box<dyn PolicyEngine>) -> Self;
}
```

**Composition Rules**:
1. Policies are evaluated in order
2. First **Deny** wins (short-circuit)
3. All **Require** decisions are collected
4. **Allow** only if no Deny and no Require

**Example**:
```rust
let composed = ComposedPolicy::new()
    .add_policy(Box::new(NoOpPolicy))           // Always Allow
    .add_policy(Box::new(TrustDomainPolicy));   // May Deny or Require

let kernel = SimulatedKernel::new()
    .with_policy_engine(Box::new(composed));
```

### Policy Audit

Policy decisions are logged for testing and debugging:

```rust
// Access policy audit log
let audit = kernel.policy_audit();

// Query decisions
let deny_events = audit.find_events(|e| e.decision.is_deny());
let require_events = audit.find_events(|e| e.decision.is_require());

// Check for specific event
assert!(audit.has_event(|e| {
    matches!(e.event, PolicyEvent::OnSpawn) && e.decision.is_deny()
}));
```

**Audit Event Structure**:
```rust
pub struct PolicyAuditEvent {
    pub timestamp: Instant,              // Simulated time
    pub event: PolicyEvent,              // What operation was evaluated
    pub policy_name: String,             // Which policy made the decision
    pub decision: PolicyDecision,        // What was decided
    pub context_summary: String,         // Summary of context
}
```

### Design Philosophy

**What Policy Is**:
- Governance mechanism for system operations
- Explicit, testable, and pluggable
- Advisory with enforcement at specific points
- Context provider (uses identity, not grants authority)

**What Policy Is NOT**:
- Global permissions system (not POSIX)
- Authentication or cryptography
- Hard-coded rules engine
- Replacement for capabilities (policy is additive)

**Key Principles**:
1. **Mechanism not policy**: Kernel provides primitives
2. **Policy observes; it does not own**: Authority comes from capabilities
3. **Explicit over implicit**: All decisions are visible
4. **Testability first**: All policy logic works under SimKernel
5. **Pluggable**: Policies can be swapped, composed, or disabled

### Pipeline Policy Enforcement

**Phase 9**: Pipelines now integrate with the policy framework.

**Enforcement Points**:

1. **OnPipelineStart**: Evaluated before pipeline execution begins
   - Context includes: execution identity, trust domain, pipeline ID, timeout, stage count
   - Deny → pipeline fails immediately with explicit error
   - Require → pipeline fails with actionable message (e.g., "must specify timeout")
   - Allow → pipeline proceeds

2. **OnPipelineStageStart**: Evaluated before each stage execution
   - Context includes: execution identity, pipeline ID, stage ID, required capabilities, retry policy
   - Deny → pipeline fails at stage boundary with explicit error
   - Require → pipeline fails with actionable message
   - Allow → stage proceeds

3. **OnPipelineStageEnd**: Emitted after stage completion (audit only, not enforced)
   - Policy can observe stage completion
   - Decision is recorded but not acted upon

**Policy Context for Pipelines**:

```rust
// Context includes relevant metadata
let context = PolicyContext::for_pipeline(actor_identity, pipeline_id)
    .with_metadata("timeout_ms", "5000")
    .with_metadata("stage_count", "3");
```

**Error Reporting**:

When policy denies or requires action:

```rust
pub enum ExecutorError {
    PolicyDenied {
        policy: String,      // "PipelineSafetyPolicy"
        event: String,       // "OnPipelineStart"
        reason: String,      // "Sandbox cannot run pipelines"
        pipeline_id: Option<String>,
    },
    PolicyRequire {
        policy: String,      // "PipelineSafetyPolicy"
        event: String,       // "OnPipelineStart"
        action: String,      // "Pipelines in user domain must specify timeout"
        pipeline_id: Option<String>,
    },
    // ... other errors
}
```

**Explainable Policy Decisions**:

Policy engines can now produce detailed reports:

```rust
pub struct PolicyDecisionReport {
    /// Final aggregated decision
    pub decision: PolicyDecision,
    /// Individual policy evaluations
    pub evaluated_policies: Vec<PolicyEvaluation>,
    /// Final deny reason (if decision is Deny)
    pub deny_reason: Option<String>,
    /// Required actions (if decision is Require)
    pub required_actions: Vec<String>,
}

// Get detailed report from composed policy
let report = composed_policy.evaluate_with_report(event, &context);
for eval in &report.evaluated_policies {
    println!("{}: {:?}", eval.policy_name, eval.decision);
}
```

**Usage Example**:

```rust
use policy::PipelineSafetyPolicy;
use identity::{IdentityMetadata, IdentityKind, TrustDomain};

// Create executor with policy
let identity = IdentityMetadata::new(
    IdentityKind::Component,
    TrustDomain::user(),
    "my-pipeline",
    kernel.now().as_nanos(),
);

let executor = PipelineExecutor::new()
    .with_identity(identity)
    .with_policy_engine(Box::new(PipelineSafetyPolicy::new()));

// Execute pipeline - policy is checked automatically
let result = executor.execute(&mut kernel, &pipeline, input, token);

match result {
    Err(ExecutorError::PolicyRequire { policy, action, .. }) => {
        eprintln!("REQUIRES: {} (policy: {})", action, policy);
        // User can fix the issue and retry
    }
    Err(ExecutorError::PolicyDenied { policy, reason, .. }) => {
        eprintln!("DENIED by {}: {}", policy, reason);
        // Operation blocked by policy
    }
    Ok((output, trace)) => {
        // Pipeline executed successfully
    }
    Err(e) => {
        // Other errors
    }
}
```

**Safety Properties**:

- Policy checks are deterministic (same input → same output)
- Side-effect free (pure functions)
- Capability-safe (no partial leaks on denial)
- Cancellation-aware (policy only recorded for started stages)
- Preserve pre-Phase-9 behavior when policy is disabled (None)

**Testing**:

Integration tests validate:
- Require timeout: PipelineSafetyPolicy requires timeout for user domain pipelines
- Deny at pipeline start: Custom policies can deny pipeline execution
- Deny at stage start: Policies can deny individual stages
- Cancellation: Policy decisions remain coherent when pipeline is cancelled
- Fault injection: Policy checks occur deterministically under message delays/reorders

## Summary

PandaGen's interfaces are designed to be:
- **Clear**: Easy to understand and reason about
- **Safe**: Type system prevents misuse
- **Testable**: Can run under `cargo test`
- **Explicit**: No hidden behavior
- **Evolvable**: Versioning built-in

These contracts form the foundation for a system that is both powerful and maintainable.

## Phase 10: Policy-Driven Capability Derivation

**Phase 10** extends the policy framework with capability derivation, allowing policies to restrict (but not escalate) capabilities for pipeline execution and individual stages.

### Core Concepts

**Derived Authority**: A restricted version of the current authority, containing a subset of available capabilities.

**Capability Delta**: A structured explanation of what changed between the original and derived authority.

**Scope**: Capability restrictions can be:
- **Pipeline-scoped**: Applied at `OnPipelineStart`, affects entire pipeline
- **Stage-scoped**: Applied at `OnPipelineStageStart`, affects only that stage

### Type Definitions

```rust
/// Set of capabilities
pub struct CapabilitySet {
    pub capabilities: HashSet<u64>,
}

/// Derived (restricted) authority
pub struct DerivedAuthority {
    pub capabilities: CapabilitySet,
    pub constraints: Vec<String>,  // For future use
}

/// Explanation of capability changes
pub struct CapabilityDelta {
    pub removed: Vec<u64>,
    pub restricted: Vec<String>,  // For future use
    pub added: Vec<u64>,  // Should be empty (no escalation)
}

/// Policy decision with optional derived authority
pub enum PolicyDecision {
    Allow { derived: Option<DerivedAuthority> },
    Deny { reason: String },
    Require { action: String },
}
```

### Security Invariants

1. **No Authority Escalation**: Derived authority must be a subset of current authority
   ```rust
   assert!(derived.capabilities.is_subset_of(&current_capabilities));
   ```

2. **Deterministic**: Policy evaluation is pure and reproducible
   - Same inputs always produce same outputs
   - No randomness, timestamps, or side effects

3. **Scoped Isolation**: Stage-scoped derivations don't affect pipeline authority
   - Stage loses capability only for its duration
   - Next stage sees original pipeline authority

4. **Explainable**: Reports include capability delta
   ```rust
   let delta = CapabilityDelta::from(&before, &after);
   // Shows: removed: [3, 4], added: []
   ```

### Example: Read-Only Filesystem

Policy restricts write capability at pipeline start:

```rust
struct ReadOnlyFsPolicy;

impl PolicyEngine for ReadOnlyFsPolicy {
    fn evaluate(&self, event: PolicyEvent, _context: &PolicyContext) -> PolicyDecision {
        match event {
            PolicyEvent::OnPipelineStart => {
                // Remove write capability, keep read
                let derived = DerivedAuthority::from_capabilities(vec![CAP_FS_READ])
                    .with_constraint("read-only");
                PolicyDecision::allow_with_derived(derived)
            }
            _ => PolicyDecision::allow()
        }
    }
    fn name(&self) -> &str { "ReadOnlyFsPolicy" }
}
```

**Effect**:
- Pipeline starts with `[CAP_FS_READ, CAP_FS_WRITE]`
- Policy derives `[CAP_FS_READ]`
- All stages see only read capability
- Attempts to write fail: `Missing required capability: CAP_FS_WRITE`

### Example: Stage-Scoped Network Removal

Policy removes network capability for one stage:

```rust
struct NoNetworkStagePolicy;

impl PolicyEngine for NoNetworkStagePolicy {
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        match event {
            PolicyEvent::OnPipelineStageStart => {
                // Check if this is the restricted stage
                if context.stage_id == Some(SENSITIVE_STAGE_ID) {
                    // Remove network, keep FS
                    let derived = DerivedAuthority::from_capabilities(vec![CAP_FS_READ, CAP_FS_WRITE])
                        .with_constraint("no-network");
                    return PolicyDecision::allow_with_derived(derived);
                }
                PolicyDecision::allow()
            }
            _ => PolicyDecision::allow()
        }
    }
    fn name(&self) -> &str { "NoNetworkStagePolicy" }
}
```

**Effect**:
- Pipeline has `[CAP_FS_READ, CAP_FS_WRITE, CAP_NETWORK]`
- Stage 1 (normal): sees all capabilities
- Stage 2 (sensitive): sees `[CAP_FS_READ, CAP_FS_WRITE]` (no network)
- Stage 3 (normal): sees all capabilities again

### Enforcement Flow

1. **OnPipelineStart**:
   ```
   Evaluate policy → Check subset → Apply derived authority
   ```
   - If derived is not a subset → `PolicyDerivedAuthorityInvalid`
   - Otherwise: `execution_authority = derived`

2. **OnPipelineStageStart**:
   ```
   Evaluate policy → Check subset → Apply stage-scoped authority
   ```
   - Stage authority inherits pipeline authority
   - Policy can further restrict for this stage only
   - Validation: `stage_derived ⊆ execution_authority`

3. **Capability Check**:
   ```rust
   if !has_capability_with_authority(cap_id, &stage_authority) {
       return Err("Missing required capability");
   }
   ```

### Error Handling

**PolicyDerivedAuthorityInvalid**: Thrown when policy tries to grant capabilities not available

```rust
Err(ExecutorError::PolicyDerivedAuthorityInvalid {
    policy: "MaliciousPolicy",
    event: "OnPipelineStart",
    reason: "Derived authority grants more capabilities than available",
    delta: "removed: [], added: [999]",
    pipeline_id: Some("pipeline-123"),
})
```

### Capability Report

Extended `PolicyDecisionReport` includes:

```rust
pub struct PolicyDecisionReport {
    pub decision: PolicyDecision,
    pub evaluated_policies: Vec<PolicyEvaluation>,
    pub deny_reason: Option<String>,
    pub required_actions: Vec<String>,
    // Phase 10 additions:
    pub input_capabilities: Option<CapabilitySet>,
    pub output_capabilities: Option<CapabilitySet>,
    pub capability_delta: Option<CapabilityDelta>,
}
```

**Example output**:
```json
{
  "decision": { "Allow": { "derived": { "capabilities": [1, 2] } } },
  "input_capabilities": [1, 2, 3, 4],
  "output_capabilities": [1, 2],
  "capability_delta": {
    "removed": [3, 4],
    "restricted": [],
    "added": []
  }
}
```

### Backwards Compatibility

**No policy** (Phase pre-9/10):
```rust
let executor = PipelineExecutor::new();  // No policy engine
// Behaves exactly as before - no restrictions
```

**Policy without derivation** (Phase 9):
```rust
impl PolicyEngine for MyPolicy {
    fn evaluate(&self, event: PolicyEvent, _: &PolicyContext) -> PolicyDecision {
        PolicyDecision::allow()  // No derived authority
    }
}
// Behaves exactly as Phase 9 - policy checks but no derivation
```

### Testing

Integration tests verify:

1. **Pipeline-scoped derivation**: `test_policy_derives_readonly_fs_at_pipeline_start`
   - Policy restricts capabilities at start
   - Handler observes reduced capability set

2. **Stage-scoped derivation**: `test_policy_derives_no_network_at_stage_start`
   - One stage loses capability
   - Next stage regains it

3. **Subset enforcement**: `test_policy_derivation_is_subset_enforced`
   - Malicious policy tries to grant extra capability
   - Executor fails with `PolicyDerivedAuthorityInvalid`

4. **Explainability**: `test_policy_report_includes_capability_delta`
   - Report shows before/after and delta
   - Serializable and stable

5. **Cancellation coherence**: `test_policy_derivation_and_cancellation_coherent`
   - Derived authority applied only to started stages
   - Report remains consistent under cancellation

6. **Backwards compatibility**: `test_no_policy_behavior_unchanged`
   - Exact behavior preserved when policy=None

### Future Extensions

**Escalation path** (NOT implemented in Phase 10):
- Explicit "grant" policy with trusted signature
- Required for adding capabilities, not just restricting
- Must be auditable and explicit

**Fine-grained restrictions** (NOT implemented in Phase 10):
- Constraints beyond simple removal
- E.g., "read-only", "time-limited", "source-restricted"

### Summary

Phase 10 provides:
- **Secure capability restriction** without escalation
- **Scoped authority** (pipeline vs stage)
- **Deterministic and explainable** policy decisions
- **Backwards compatible** with existing code
- **Defense in depth** via subset validation

This enables least-privilege enforcement at the policy layer without modifying core capability semantics.

## Phase 11: Resource Budget Interface

### Resource Types

PandaGen provides five abstract resource types for deterministic accounting:

```rust
// All are strong newtypes with saturating arithmetic
pub struct CpuTicks(pub u64);        // Simulated execution steps
pub struct MemoryUnits(pub u64);     // Abstract memory units
pub struct MessageCount(pub u64);    // Number of messages
pub struct StorageOps(pub u64);      // Storage operations
pub struct PipelineStages(pub u64);  // Pipeline stages executed
```

**Common Operations**:
```rust
let ticks = CpuTicks::new(100);
let more = CpuTicks::new(50);

// Checked arithmetic (returns Option)
let sum = ticks.checked_add(more)?;  // Some(150)

// Saturating arithmetic (never panics)
let total = ticks.saturating_add(more);  // 150
```

### ResourceBudget

Immutable limits for resource consumption:

```rust
pub struct ResourceBudget {
    pub cpu_ticks: Option<CpuTicks>,
    pub memory_units: Option<MemoryUnits>,
    pub message_count: Option<MessageCount>,
    pub storage_ops: Option<StorageOps>,
    pub pipeline_stages: Option<PipelineStages>,
}
```

**Creation**:
```rust
// Unlimited budget (no constraints)
let unlimited = ResourceBudget::unlimited();

// Zero budget (all resources exhausted)
let zero = ResourceBudget::zero();

// Builder pattern
let budget = ResourceBudget::unlimited()
    .with_cpu_ticks(CpuTicks::new(1000))
    .with_message_count(MessageCount::new(50));
```

**Operations**:
```rust
// Check subset (child ≤ parent)
let is_valid = child_budget.is_subset_of(&parent_budget);

// Compute minimum (most restrictive)
let min = budget1.min(&budget2);
```

**Contract**:
- Once created, immutable
- Can only be replaced (not modified)
- `None` for a resource = unlimited
- Subset check validates inheritance

### ResourceUsage

Mutable tracker for current consumption:

```rust
pub struct ResourceUsage {
    pub cpu_ticks: CpuTicks,
    pub memory_units: MemoryUnits,
    pub message_count: MessageCount,
    pub storage_ops: StorageOps,
    pub pipeline_stages: PipelineStages,
}
```

**Operations**:
```rust
let mut usage = ResourceUsage::zero();

// Consume resources
usage.consume_cpu_ticks(CpuTicks::new(10));
usage.consume_message();
usage.consume_storage_op();
usage.consume_pipeline_stage();

// Check if exceeds budget
if let Some(exceeded) = usage.exceeds(&budget) {
    return Err(ResourceError::BudgetExceeded(exceeded));
}

// Compute remaining budget
let remaining = usage.remaining(&budget);
```

**Contract**:
- All values start at zero
- Saturating addition (never panics)
- Exceeds returns first violated limit
- Remaining uses saturating subtraction

### Budget Attachment to Identity

Identity metadata includes optional budget:

```rust
pub struct IdentityMetadata {
    // ... existing fields ...
    pub budget: Option<ResourceBudget>,
    pub usage: ResourceUsage,
}
```

**Usage**:
```rust
// Create identity with budget
let identity = IdentityMetadata::new(...)
    .with_budget(budget);

// Check if has budget
if identity.has_budget() {
    // Budget is present
}

// Validate inheritance
if child.budget_inherits_from(&parent) {
    // Child budget ≤ parent budget
}
```

**Contract**:
- Budget is optional (None = no limit)
- Usage always tracked (starts at zero)
- Inheritance validated at spawn time
- Budget scoped to identity lifetime

### Enforcement Points

SimKernel enforces budgets at deterministic points:

#### 1. Task Spawn
```rust
pub fn spawn_task_with_identity(
    &mut self,
    descriptor: TaskDescriptor,
    kind: IdentityKind,
    trust_domain: TrustDomain,
    parent_id: Option<ExecutionId>,
    creator_id: Option<ExecutionId>,
) -> Result<(TaskHandle, ExecutionId), KernelError>;
```

**Enforcement**:
- Validates budget inheritance (child ≤ parent)
- Returns error if violation detected
- Creates usage tracker for new identity

**Example**:
```rust
// Parent with budget
let parent_budget = ResourceBudget::unlimited()
    .with_cpu_ticks(CpuTicks::new(1000));

// Child with larger budget - fails
let child_budget = ResourceBudget::unlimited()
    .with_cpu_ticks(CpuTicks::new(2000));

let result = kernel.spawn_task_with_identity(...);
// Returns: Err(KernelError::InsufficientAuthority(
//   "Budget inheritance violation: child budget exceeds parent"
// ))
```

#### 2. Message Send/Receive
```rust
kernel.send_message(channel, message)?;
kernel.receive_message(channel, timeout)?;
```

**Enforcement** (planned):
- Check MessageCount budget before operation
- Increment usage after validation
- Return error if budget exceeded

**Example**:
```rust
// Task with MessageCount budget of 10
for i in 0..11 {
    let result = kernel.send_message(channel, msg);
    if i < 10 {
        assert!(result.is_ok());  // First 10 succeed
    } else {
        // 11th message fails
        assert!(matches!(result, Err(KernelError::ResourceExhausted(_))));
    }
}
```

#### 3. Simulated Execution Steps (future)
```rust
kernel.execute_steps(task_id, CpuTicks::new(100))?;
```

**Enforcement** (planned):
- Track computational work
- Increment CpuTicks usage
- Fail if budget exhausted

#### 4. Storage Operations (future)
```rust
storage.read(object_id)?;
storage.write(object_id, data)?;
```

**Enforcement** (planned):
- Track read/write operations
- Increment StorageOps usage
- Independent of data size

### Error Types

**ResourceBudgetExceeded**:
```rust
pub enum ResourceExceeded {
    CpuTicks { limit: CpuTicks, usage: CpuTicks },
    MemoryUnits { limit: MemoryUnits, usage: MemoryUnits },
    MessageCount { limit: MessageCount, usage: MessageCount },
    StorageOps { limit: StorageOps, usage: StorageOps },
    PipelineStages { limit: PipelineStages, usage: PipelineStages },
}

// Returns detailed information
let exceeded = ResourceExceeded::MessageCount {
    limit: MessageCount::new(10),
    usage: MessageCount::new(11),
};
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

**Contract**:
- All errors include resource type
- Limit and usage values provided
- Human-readable error messages
- Suitable for logging and debugging

### Budget Lifecycle

1. **Creation**:
   ```rust
   let budget = ResourceBudget::unlimited()
       .with_cpu_ticks(CpuTicks::new(1000));
   ```

2. **Attachment**:
   ```rust
   let identity = IdentityMetadata::new(...)
       .with_budget(budget);
   ```

3. **Validation**:
   ```rust
   // At spawn time
   if !child.budget_inherits_from(&parent) {
       return Err(...);
   }
   ```

4. **Enforcement**:
   ```rust
   // At each enforcement point
   usage.consume_message();
   if let Some(exceeded) = usage.exceeds(&budget) {
       return Err(...);
   }
   ```

5. **Termination**:
   ```rust
   // Budget automatically released
   kernel.terminate_task(task_id);
   // No cleanup needed
   ```

### Design Guidelines

**For Budget Users**:
1. Always check budget before operations
2. Use saturating arithmetic to prevent panics
3. Validate inheritance explicitly
4. Handle errors with clear messages
5. Test with deterministic scenarios

**For Policy Writers**:
1. Use budgets to limit resource consumption
2. Require budgets for untrusted domains
3. Derive restricted budgets (subset only)
4. Never escalate budget (no increase)
5. Explain budget decisions clearly

**For Kernel Implementers**:
1. Enforce budgets at deterministic points
2. Check before consuming (fail-fast)
3. Record budget events in audit log
4. Release budgets on termination
5. Test under fault injection

### Example: Budget-Limited Pipeline

```rust
use resources::{CpuTicks, MessageCount, ResourceBudget};

// Create budget for pipeline executor
let executor_budget = ResourceBudget::unlimited()
    .with_cpu_ticks(CpuTicks::new(5000))
    .with_message_count(MessageCount::new(100))
    .with_pipeline_stages(PipelineStages::new(10));

// Create executor identity with budget
let executor_identity = IdentityMetadata::new(
    IdentityKind::PipelineStage,
    TrustDomain::user(),
    "data-processor",
    kernel.now().as_nanos(),
)
.with_budget(executor_budget);

let exec_id = kernel.create_identity(executor_identity);

// Execute pipeline
// - Each stage consumes PipelineStages
// - Each message consumes MessageCount
// - Each computation consumes CpuTicks
// - Pipeline fails if any budget exceeded

let result = executor.execute(&mut kernel, &pipeline, input, token);

match result {
    Ok((output, trace)) => {
        // Success - budget not exhausted
        let final_usage = kernel.get_identity(exec_id).unwrap().usage;
        println!("Used: {}", final_usage);
    }
    Err(ExecutorError::ResourceExhausted(exceeded)) => {
        // Budget exhausted during execution
        eprintln!("Budget exceeded: {}", exceeded);
    }
    Err(e) => {
        // Other error
        eprintln!("Error: {}", e);
    }
}
```

### Integration with Previous Phases

Phase 11 builds on all previous phases:
- **Phase 1**: Uses KernelApi, TaskId, ExecutionId
- **Phase 2**: Deterministic under fault injection
- **Phase 3**: Budgets tied to identity lifetime (like capabilities)
- **Phase 4**: ResourceBudget is serializable/versioned
- **Phase 5**: Pipeline stages tracked as resource
- **Phase 6**: Budget exhaustion may trigger cancellation
- **Phase 7**: Budgets attached to ExecutionId
- **Phase 8**: Policies can require/deny based on budgets
- **Phase 9**: Budget enforcement integrated with pipelines
- **Phase 10**: Budget derivation follows capability derivation rules

All safety properties preserved:
- Deterministic: Same operations → same consumption
- No leaks: Budgets released on termination
- No escalation: Child ≤ parent enforced
- Explainable: Clear error messages with context

---

## Phase 12: Resource Enforcement Interface

### Enforcement Points

Phase 12 adds actual budget enforcement at execution boundaries.

#### Message Operations

**send_message** (MessageCount enforcement):
```rust
fn send_message(&mut self, channel: ChannelId, message: MessageEnvelope) 
    -> Result<(), KernelError>
```

Enforcement:
- Checks `message.source` TaskId (if present)
- Looks up ExecutionId for source task
- Checks MessageCount budget
- Consumes 1 MessageCount unit **before** sending
- Returns `ResourceBudgetExhausted` if limit reached
- Cancels identity on exhaustion

**receive_message** (MessageCount enforcement):
```rust
// Workaround for API limitation (no TaskId parameter)
kernel.set_receive_context(task_id);
let result = kernel.receive_message(channel, timeout)?;
kernel.clear_receive_context();
```

Enforcement:
- Uses `current_receive_task` context (if set)
- Looks up ExecutionId for receiver task
- Checks MessageCount budget
- Consumes 1 MessageCount unit **before** receiving
- Returns `ResourceBudgetExhausted` if limit reached

#### CPU Operations

**try_consume_cpu_ticks** (external enforcement):
```rust
pub fn try_consume_cpu_ticks(
    &mut self,
    execution_id: ExecutionId,
    amount: u64,
) -> Result<(), KernelError>
```

Enforcement:
- Checks if identity is cancelled
- Checks CpuTicks budget
- Consumes `amount` CpuTicks units
- Returns `ResourceBudgetExhausted` if would exceed limit
- Cancels identity on exhaustion

Usage:
- Called by pipeline executors
- Called by stage handlers
- Called for simulated work
- Deterministic consumption

#### Pipeline Stages

**try_consume_pipeline_stage** (external enforcement):
```rust
pub fn try_consume_pipeline_stage(
    &mut self,
    execution_id: ExecutionId,
    stage_name: String,
) -> Result<(), KernelError>
```

Enforcement:
- Checks if identity is cancelled
- Checks PipelineStages budget
- Consumes 1 PipelineStages unit
- Returns `ResourceBudgetExhausted` if limit reached
- Records stage name in audit
- Cancels identity on exhaustion

### Error Types

#### ResourceBudgetExhausted

```rust
KernelError::ResourceBudgetExhausted {
    resource_type: String,  // "MessageCount", "CpuTicks", etc.
    limit: u64,             // Budget limit
    usage: u64,             // Current usage at failure
    identity: String,       // ExecutionId that exhausted
    operation: String,      // Operation that failed
}
```

When to return:
- Budget limit reached
- Would exceed limit with this operation
- Identity already cancelled (resource_type contains "cancelled")

Error message format:
```
Resource budget exhausted: MessageCount limit=100, usage=100, 
identity=exec:a1b2c3d4..., operation=send_message
```

### Resource Audit Log

Phase 12 adds `ResourceAuditLog` for test visibility.

#### Accessing the Audit Log

```rust
// Get audit log reference
let audit = kernel.resource_audit();

// Count specific events
let message_count = audit.count_events(|e| matches!(
    e,
    ResourceEvent::MessageConsumed { .. }
));

// Check for exhaustion
assert!(audit.has_event(|e| matches!(
    e,
    ResourceEvent::BudgetExhausted { .. }
)));

// Query by execution ID
let entries = audit.entries_for_execution(execution_id);
```

#### Audit Events

**MessageConsumed**:
```rust
ResourceEvent::MessageConsumed {
    execution_id: ExecutionId,
    operation: MessageOperation::Send,  // or Receive
    before: u64,     // Usage before
    after: u64,      // Usage after
}
```

**CpuConsumed**:
```rust
ResourceEvent::CpuConsumed {
    execution_id: ExecutionId,
    amount: u64,     // Ticks consumed
    before: u64,     // Usage before
    after: u64,      // Usage after
}
```

**PipelineStageConsumed**:
```rust
ResourceEvent::PipelineStageConsumed {
    execution_id: ExecutionId,
    stage_name: String,  // Stage identifier
    before: u64,         // Usage before
    after: u64,          // Usage after
}
```

**BudgetExhausted**:
```rust
ResourceEvent::BudgetExhausted {
    execution_id: ExecutionId,
    resource_type: String,     // Which resource
    limit: u64,                // Budget limit
    attempted_usage: u64,      // What we tried to use
    operation: String,         // What operation failed
}
```

**CancelledDueToExhaustion**:
```rust
ResourceEvent::CancelledDueToExhaustion {
    execution_id: ExecutionId,
    resource_type: String,  // Which resource caused cancellation
}
```

### Cancellation Integration

Budget exhaustion triggers identity cancellation:

```rust
// After exhaustion, identity is cancelled
if kernel.is_identity_cancelled(execution_id) {
    // All further operations fail immediately
}
```

Cancelled identity behavior:
- All resource operations return `ResourceBudgetExhausted` with "cancelled" in type
- No further consumption recorded
- Deterministic (same exhaustion point every time)
- Audit log records cancellation event

### Testing Interface

For tests that need resource enforcement:

```rust
use resources::{ResourceBudget, MessageCount, CpuTicks, PipelineStages};
use sim_kernel::resource_audit;

#[test]
fn test_message_exhaustion() {
    let mut kernel = SimulatedKernel::new();
    
    // Create task with limited budget
    let budget = ResourceBudget::unlimited()
        .with_message_count(MessageCount::new(10));
    
    let descriptor = TaskDescriptor::new("limited".to_string());
    let (handle, exec_id) = kernel.spawn_task_with_identity(
        descriptor,
        IdentityKind::Component,
        TrustDomain::user(),
        None,
        None,
    )?;
    
    // Attach budget
    if let Some(identity) = kernel.get_identity_mut(exec_id) {
        *identity = identity.clone().with_budget(budget);
    }
    
    // Consume until exhausted
    let channel = kernel.create_channel()?;
    for i in 0..10 {
        let msg = create_message(handle.task_id);
        kernel.send_message(channel, msg)?;  // OK
    }
    
    // Next send fails
    let msg = create_message(handle.task_id);
    let result = kernel.send_message(channel, msg);
    assert!(matches!(result, Err(KernelError::ResourceBudgetExhausted { .. })));
    
    // Verify audit
    let audit = kernel.resource_audit();
    assert_eq!(audit.count_events(|e| matches!(
        e, resource_audit::ResourceEvent::BudgetExhausted { .. }
    )), 1);
}
```

### Design Guidelines

**For Enforcement Users (Tests)**:
1. Create identities with explicit budgets
2. Attach budgets via `get_identity_mut`
3. Consume resources through normal operations
4. Assert on `ResourceBudgetExhausted` errors
5. Verify audit log for deterministic consumption

**For External Consumers (Pipelines)**:
1. Use `try_consume_cpu_ticks` for simulated work
2. Use `try_consume_pipeline_stage` for stage entry
3. Handle exhaustion by aborting pipeline
4. Record exhaustion in execution trace
5. Don't retry after exhaustion (cancelled identity)

**For Kernel Implementers**:
1. Check budget **before** operation takes effect
2. Consume **exactly** the documented amount
3. Record audit events for test visibility
4. Cancel identity on exhaustion
5. Return detailed error with context

### Backwards Compatibility

Phase 12 maintains backwards compatibility:

- Identities without budgets: unlimited (no enforcement)
- Messages without source: no enforcement on send
- Receives without context: no enforcement on receive
- Old tests: continue to work (no budgets = no limits)

Enforcement is **opt-in** via explicit budget attachment.

### Integration Summary

Complete enforcement lifecycle:

```
1. Create identity with budget
   └─> ResourceBudget attached to IdentityMetadata

2. Operation consumes resource
   └─> Check budget → Consume → Record audit
   
3. Budget exhausted
   └─> Fail operation → Cancel identity → Record exhaustion
   
4. Further operations
   └─> Fail immediately (cancelled)
   
5. Verify in tests
   └─> Check audit log → Assert on errors
```

All operations deterministic, all events auditable, all failures explicit.

---

## Input System Interface

### Overview

The input system provides explicit, capability-based access to input events.

**Key Principles**:
- Input is explicit, not ambient
- Events are structured, not byte streams
- Focus is controlled, not implicit
- Everything is testable without hardware

### Input Event Schema

#### InputEvent

```rust
pub enum InputEvent {
    Key(KeyEvent),
    // Reserved for future: Pointer, Touch
}
```

#### KeyEvent

```rust
pub struct KeyEvent {
    pub code: KeyCode,        // Logical key
    pub modifiers: Modifiers, // Ctrl, Alt, Shift, Meta
    pub state: KeyState,      // Pressed, Released, Repeat
    pub text: Option<String>, // For IME (future)
}
```

**Examples**:
```rust
// Simple key press
KeyEvent::pressed(KeyCode::A, Modifiers::none())

// Ctrl+C
KeyEvent::pressed(KeyCode::C, Modifiers::CTRL)

// Shift+Enter
KeyEvent::pressed(KeyCode::Enter, Modifiers::SHIFT)
```

#### KeyCode

Logical key codes (not hardware scan codes):
- Letters: `A`-`Z`
- Numbers: `Num0`-`Num9`
- Function: `F1`-`F12`
- Special: `Enter`, `Backspace`, `Delete`, `Tab`, `Escape`
- Arrows: `Up`, `Down`, `Left`, `Right`
- Modifiers: `LeftCtrl`, `RightCtrl`, `LeftShift`, etc.
- Punctuation: `Comma`, `Period`, `Slash`, etc.

#### Modifiers

Bitflags for modifier keys:
```rust
Modifiers::CTRL   // Control key
Modifiers::ALT    // Alt key
Modifiers::SHIFT  // Shift key
Modifiers::META   // Meta/Super/Windows key

// Combine with .with()
Modifiers::CTRL.with(Modifiers::SHIFT)
```

#### KeyState

```rust
pub enum KeyState {
    Pressed,   // Key was pressed down
    Released,  // Key was released
    Repeat,    // Key is auto-repeating
}
```

### Input Service Interface

#### Subscribing to Input

```rust
pub fn subscribe_keyboard(
    &mut self,
    task_id: TaskId,
    channel: ChannelId,
) -> Result<InputSubscriptionCap, InputServiceError>
```

**Contract**:
- One subscription per task
- Returns capability representing subscription
- Events delivered via specified channel
- Delivery consumes MessageCount budget

**Example**:
```rust
let task_id = kernel.spawn_task(descriptor)?;
let channel = kernel.create_channel()?;
let cap = input_service.subscribe_keyboard(task_id, channel)?;
```

#### Revoking Subscription

```rust
pub fn revoke_subscription(
    &mut self,
    cap: &InputSubscriptionCap,
) -> Result<(), InputServiceError>
```

**Contract**:
- Deactivates subscription (doesn't remove)
- No more events delivered
- Subscription still exists but inactive

#### Unsubscribing

```rust
pub fn unsubscribe(
    &mut self,
    cap: &InputSubscriptionCap,
) -> Result<(), InputServiceError>
```

**Contract**:
- Completely removes subscription
- Releases resources
- Task can subscribe again later

### Focus Manager Interface

#### Requesting Focus

```rust
pub fn request_focus(
    &mut self,
    cap: InputSubscriptionCap,
) -> Result<(), FocusError>
```

**Contract**:
- Pushes subscription onto focus stack
- Top of stack has focus
- Previous focus loses focus
- Audit event recorded

**Example**:
```rust
let cap = input_service.subscribe_keyboard(task_id, channel)?;
focus_manager.request_focus(cap)?;
```

#### Releasing Focus

```rust
pub fn release_focus(&mut self) -> Result<InputSubscriptionCap, FocusError>
```

**Contract**:
- Pops top of focus stack
- Next subscription (if any) gains focus
- Returns released capability
- Audit event recorded

#### Routing Events

```rust
pub fn route_event(
    &self,
    event: &InputEvent,
) -> Result<Option<InputSubscriptionCap>, FocusError>
```

**Contract**:
- Returns focused subscription, if any
- Only focused subscription receives events
- Unfocused subscriptions receive nothing

**Example**:
```rust
let event = InputEvent::key(KeyEvent::pressed(KeyCode::A, Modifiers::none()));
if let Some(cap) = focus_manager.route_event(&event)? {
    // Deliver event to cap.channel
}
```

### Interactive Component Pattern

Complete flow for interactive component:

```rust
// 1. Create component
let task_id = TaskId::new();
let mut console = InteractiveConsole::new(task_id);

// 2. Subscribe to input
let channel = ChannelId::new();
console.subscribe(&mut input_service, channel)?;

// 3. Request focus
console.request_focus(&mut focus_manager)?;

// 4. Receive and process events
loop {
    // In real system, receive from channel
    let event = /* receive from channel */;
    
    if let Some(command) = console.process_event(event)? {
        // Execute command
        println!("Command: {}", command);
    }
}
```

### Testing with SimKernel

#### Injecting Events

```rust
use sim_kernel::test_utils::input_injection::InputEventQueue;

let mut queue = InputEventQueue::new();

// Inject events
queue.inject_event(InputEvent::key(
    KeyEvent::pressed(KeyCode::H, Modifiers::none())
));
queue.inject_event(InputEvent::key(
    KeyEvent::pressed(KeyCode::I, Modifiers::none())
));

// Process events
while let Some(event) = queue.next_event() {
    console.process_event(event)?;
}
```

#### Testing Focus Switching

```rust
let mut focus_manager = FocusManager::new();

// Component 1 gets focus
focus_manager.request_focus(cap1)?;
assert!(focus_manager.has_focus(&cap1));

// Component 2 takes focus
focus_manager.request_focus(cap2)?;
assert!(!focus_manager.has_focus(&cap1));
assert!(focus_manager.has_focus(&cap2));

// Events only go to cap2
let target = focus_manager.route_event(&event)?;
assert_eq!(target.unwrap().id, cap2.id);
```

### Policy Integration

Focus requests can be policy-gated:

```rust
pub enum PolicyEvent {
    // ... existing events
    OnInputFocusRequest,  // New: Focus request
}

impl PolicyEngine for CustomPolicy {
    fn evaluate(&self, event: PolicyEvent, context: &PolicyContext) -> PolicyDecision {
        match event {
            PolicyEvent::OnInputFocusRequest => {
                // Check if cross-domain
                if context.is_cross_domain() {
                    PolicyDecision::deny("Cross-domain focus requires approval")
                } else {
                    PolicyDecision::allow()
                }
            }
            _ => PolicyDecision::allow(),
        }
    }
}
```

### Audit Trail

All focus changes are recorded:

```rust
pub enum FocusEvent {
    Granted { subscription_id: u64, timestamp_ns: u64 },
    Transferred { from_subscription_id: u64, to_subscription_id: u64, timestamp_ns: u64 },
    Released { subscription_id: u64, timestamp_ns: u64 },
    Denied { subscription_id: u64, reason: String, timestamp_ns: u64 },
}

// Access audit trail
let trail = focus_manager.audit_trail();
for event in trail {
    match event {
        FocusEvent::Granted { subscription_id, .. } => {
            println!("Focus granted to {}", subscription_id);
        }
        _ => {}
    }
}
```

### Comparison with Traditional Models

| Aspect | Traditional (TTY/stdin) | PandaGen Input |
|--------|------------------------|----------------|
| **Authority** | Ambient (anyone can read) | Explicit (must subscribe) |
| **Data format** | Byte stream | Structured events |
| **Focus** | Implicit (race condition) | Explicit (stack-based) |
| **Testing** | Requires PTY or mocking | Direct injection |
| **Hardware** | Tightly coupled | Abstracted |
| **Concurrency** | Locks, buffers | Message passing |

### Example: Simple Interactive Session

```rust
// Setup
let mut kernel = SimulatedKernel::new();
let mut input_service = InputService::new();
let mut focus_manager = FocusManager::new();

let task_id = TaskId::new();
let channel = ChannelId::new();

// Subscribe and focus
let cap = input_service.subscribe_keyboard(task_id, channel)?;
focus_manager.request_focus(cap)?;

// Simulate typing "ls" + Enter
let mut console = InteractiveConsole::new(task_id);

console.process_event(InputEvent::key(
    KeyEvent::pressed(KeyCode::L, Modifiers::none())
))?;
console.process_event(InputEvent::key(
    KeyEvent::pressed(KeyCode::S, Modifiers::none())
))?;

let command = console.process_event(InputEvent::key(
    KeyEvent::pressed(KeyCode::Enter, Modifiers::none())
))?;

assert_eq!(command, Some("ls".to_string()));
```

### Summary

The input system provides:
- **Explicit subscriptions**: No ambient keyboard access
- **Structured events**: Typed, serializable, versionable
- **Focus control**: Stack-based, policy-gated
- **Full testability**: No hardware required
- **Budget enforcement**: Message delivery consumes resources
- **Audit trail**: All operations logged

This is the foundation for interactive components: CLI, editors, UI shells, debuggers.

## Editor Service (Phase 15)

### Interface: `services_editor_vi::Editor`

The editor service provides a modal text editor component with capability-based document access.

#### Creating an Editor

```rust
use services_editor_vi::Editor;

// Create with default viewport (20 lines)
let editor = Editor::new();

// Create with custom viewport
let editor = Editor::with_viewport(30);
```

#### Editor State

```rust
// Get current state
let state = editor.state();

// Check mode
match state.mode() {
    EditorMode::Normal => { /* navigation mode */ }
    EditorMode::Insert => { /* text entry mode */ }
    EditorMode::Command => { /* ex command mode */ }
}

// Check dirty flag
if state.is_dirty() {
    println!("Unsaved changes");
}

// Get cursor position
let pos = state.cursor().position();
println!("Row: {}, Col: {}", pos.row, pos.col);

// Get status message
println!("{}", state.status_message());
```

#### Processing Input

```rust
use input_types::{InputEvent, KeyCode, KeyEvent, Modifiers};

let event = InputEvent::key(KeyEvent::pressed(KeyCode::I, Modifiers::none()));

match editor.process_input(event)? {
    EditorAction::Continue => {
        // Keep editing
    }
    EditorAction::Saved(version_id) => {
        // Document saved, new version created
        println!("Saved version: {}", version_id);
    }
    EditorAction::Quit => {
        // Editor wants to quit
        break;
    }
}
```

#### Document Operations

```rust
use services_editor_vi::{DocumentHandle, OpenOptions};
use services_storage::{ObjectId, VersionId};

// Open new empty document
editor.new_document();

// Load document with capability
let handle = DocumentHandle::new(
    object_id,           // Object capability
    version_id,          // Current version
    Some("readme.txt"),  // Display label (optional)
    true                 // Can update directory link
);
editor.load_document(content, handle);

// Get current document
if let Some(handle) = editor.document() {
    println!("Editing: {:?}", handle.path_label);
}

// Get content
let content = editor.get_content();
```

#### Rendering

```rust
// Render full editor view (viewport + status)
let output = editor.render();
println!("{}", output);

// Example output:
// [h]ello world
// second line
// ~
// ~
// NORMAL readme.txt [+] | Saved v2
```

### Modal Input Processing

#### Normal Mode Commands

| Key | Action |
|-----|--------|
| `h` / Left | Move cursor left |
| `j` / Down | Move cursor down |
| `k` / Up | Move cursor up |
| `l` / Right | Move cursor right |
| `i` | Enter insert mode |
| `x` | Delete character under cursor |
| `:` | Enter command mode |

#### Insert Mode Commands

| Key | Action |
|-----|--------|
| Printable chars | Insert at cursor |
| Enter | Insert newline |
| Backspace | Delete previous character |
| Escape | Return to normal mode |

#### Command Mode Commands

| Command | Action |
|---------|--------|
| `:w` or `:write` | Save document (create new version) |
| `:q` or `:quit` | Quit (blocked if dirty) |
| `:q!` or `:quit!` | Force quit (discard changes) |
| `:wq` or `:x` | Save and quit |

### Document Handle

```rust
pub struct DocumentHandle {
    /// Object ID (capability)
    pub object_id: ObjectId,
    
    /// Current version ID
    pub version_id: VersionId,
    
    /// Optional path label (display only, NOT authority)
    pub path_label: Option<String>,
    
    /// Whether we can update directory link
    pub can_update_link: bool,
}
```

**Important**: `path_label` is for display only. Authority comes from `object_id` capability.

### Save Semantics

When saving:
1. New immutable version created in storage
2. New VersionId returned
3. Directory link updated **only if** `can_update_link == true`

```rust
// Save always creates new version
let result = save_document(&editor)?;

// Check if link was updated
if result.link_updated {
    println!("Saved and updated link");
} else {
    println!("Saved but link not updated (no permission)");
    // New version exists, but directory still points to old version
}
```

This separates content saves from directory updates:
- Content save: requires object write capability
- Link update: requires directory write capability
- These are independent authorities

### Open Options

```rust
use services_editor_vi::OpenOptions;

// Open by direct capability (preferred)
let opts = OpenOptions::new()
    .with_object(object_id);

// Open by path via fs_view (convenience)
let opts = OpenOptions::new()
    .with_path("/docs/readme.txt");
// Requires root capability for path resolution
// Path provides NO authority on its own
```

### Error Handling

```rust
use services_editor_vi::EditorError;

match editor.process_input(event) {
    Ok(action) => { /* handle action */ }
    Err(EditorError::Command(cmd_err)) => {
        // Command parse error (:w typo, etc.)
        println!("Command error: {}", cmd_err);
    }
    Err(EditorError::Io(io_err)) => {
        // I/O operation failed
        println!("I/O error: {}", io_err);
    }
    Err(e) => {
        println!("Error: {}", e);
    }
}
```

### Testing with SimKernel

```rust
#[test]
fn test_editor_workflow() {
    let mut editor = Editor::new();
    
    // Enter insert mode
    editor.process_input(press_key(KeyCode::I)).unwrap();
    
    // Type "hello"
    editor.process_input(press_key(KeyCode::H)).unwrap();
    editor.process_input(press_key(KeyCode::E)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::L)).unwrap();
    editor.process_input(press_key(KeyCode::O)).unwrap();
    
    // Exit insert mode
    editor.process_input(press_key(KeyCode::Escape)).unwrap();
    
    // Verify content
    assert_eq!(editor.get_content(), "hello");
    assert!(editor.state().is_dirty());
    
    // Save
    editor.process_input(press_key_shift(KeyCode::Semicolon)).unwrap(); // :
    editor.state_mut().append_to_command('w');
    let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();
    
    assert!(matches!(result, EditorAction::Saved(_)));
    assert!(!editor.state().is_dirty());
}

fn press_key(code: KeyCode) -> InputEvent {
    InputEvent::key(KeyEvent::pressed(code, Modifiers::none()))
}

fn press_key_shift(code: KeyCode) -> InputEvent {
    InputEvent::key(KeyEvent::pressed(code, Modifiers::SHIFT))
}
```

### Integration Example

```rust
use services_editor_vi::Editor;
use services_input::InputService;
use services_focus_manager::FocusManager;

// Create editor
let mut editor = Editor::new();

// Subscribe to keyboard input
let subscription = input_service.subscribe_keyboard(task_id, channel)?;

// Request focus
focus_manager.request_focus(subscription)?;

// Event loop
loop {
    // Receive keyboard event
    let event = receive_input_event()?;
    
    // Process in editor
    match editor.process_input(event)? {
        EditorAction::Continue => {
            // Render and continue
            println!("{}", editor.render());
        }
        EditorAction::Saved(version_id) => {
            // Handle save
            println!("Saved version: {}", version_id);
            println!("{}", editor.render());
        }
        EditorAction::Quit => {
            // Clean up and exit
            focus_manager.release_focus()?;
            break;
        }
    }
}
```

### Comparison with Traditional vi

| Feature | Traditional vi | PandaGen Editor |
|---------|----------------|----------------|
| Input | stdin (TTY) | InputEvent |
| Output | stdout (TTY) | String render |
| Files | Path strings | Capabilities |
| Save | Overwrite file | Create version |
| Open | `vi file.txt` | `editor.load_document(content, handle)` |
| Authority | Ambient (file paths) | Explicit (capabilities) |
| Testing | Requires PTY | cargo test |
| Embedding | Hard (process) | Easy (library) |

### Design Rationale

**Why modal?**
- Proven UI pattern (vi/vim)
- Keyboard-only workflow
- Clear state separation
- Easy to test

**Why capabilities?**
- No ambient file access
- Explicit authority
- Auditable operations
- Least privilege

**Why versioned?**
- Immutability preserved
- No data loss
- Easy rollback
- Natural fit for storage model

**Why component?**
- Easy to embed
- Easy to test
- Clean interfaces
- No process overhead

### Summary

The editor service provides:
- **Modal editing**: vi-like interface without TTY
- **Capability-based I/O**: Explicit document access
- **Versioned saves**: Immutable version creation
- **Testability**: Full coverage without hardware
- **Component model**: Library, not process

Usage pattern:
1. Create editor
2. Load document (via capability or path convenience)
3. Process input events
4. Handle actions (continue/saved/quit)
5. Render output
