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

### Creating Capabilities

```rust
// Only trusted code (kernel) can do this
let cap: Cap<FileRead> = Cap::new(42);
```

### Granting Capabilities

```rust
let grant = CapabilityGrant::new(cap, Some(grantor_id));
// Include in message or pass to kernel
```

### Transferring Capabilities

```rust
let transfer = CapabilityTransfer::new(cap, from_task, to_task);
let transferred = transfer.complete();
```

**Contract**:
- Type system prevents capability confusion
- Cannot cast `Cap<T>` to `Cap<U>`
- Compiler enforces correct usage

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

## Summary

PandaGen's interfaces are designed to be:
- **Clear**: Easy to understand and reason about
- **Safe**: Type system prevents misuse
- **Testable**: Can run under `cargo test`
- **Explicit**: No hidden behavior
- **Evolvable**: Versioning built-in

These contracts form the foundation for a system that is both powerful and maintainable.
