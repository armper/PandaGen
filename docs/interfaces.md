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

## Summary

PandaGen's interfaces are designed to be:
- **Clear**: Easy to understand and reason about
- **Safe**: Type system prevents misuse
- **Testable**: Can run under `cargo test`
- **Explicit**: No hidden behavior
- **Evolvable**: Versioning built-in

These contracts form the foundation for a system that is both powerful and maintainable.
