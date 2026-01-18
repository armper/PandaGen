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
- Explicit grant/transfer semantics
- Type system enforces correctness

**Impact**:
- Least privilege by default
- Can't accidentally inherit dangerous capabilities
- Fine-grained security without complexity

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
