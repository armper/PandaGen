# PandaGen Operating System Roadmap

**Version**: 1.0  
**Status**: Living Document  
**Last Updated**: 2026-02-26

## Overview

This document outlines the high-impact feature prioritization and phased implementation strategy for evolving PandaGen from its current foundation into a complete, modern operating system. The roadmap emphasizes incremental delivery, testability, and architectural soundness over speed.

---

## Prioritized High-Impact Features

The following features are ordered by their impact on system completeness and capability, independent of implementation effort:

### 1. Process Model + Scheduler (Preemptive Multitasking) ✅ In Progress
**Impact**: Foundation for all concurrent execution  
**Status**: Phase 170 - Enhancement underway

**Why This Matters**:
- Enables true multitasking and isolation between services
- Critical foundation for everything else in the OS
- Allows fair resource allocation and responsiveness

**Current State**:
- ✅ Basic round-robin scheduler exists (Phase 23)
- ✅ Task spawning via `KernelApi::spawn_task()`
- ✅ Time-sliced preemption with configurable quantum
- ✅ Real-time EDF scheduling support
- 🔄 Needs: priority scheduling, enhanced lifecycle management, CPU affinity

**Next Steps**:
- Multi-priority scheduling policies
- Process groups and hierarchies
- CPU affinity and load balancing
- Advanced scheduler observability

---

### 2. Virtual Memory + User/Kernel Isolation
**Impact**: Security and stability foundation  
**Status**: Planned - Phase 2 of roadmap

**Why This Matters**:
- Prevents processes from interfering with each other
- Enables memory protection and fault isolation
- Foundation for security and stability guarantees

**Current State**:
- ❌ No virtual memory management
- ❌ No memory protection or isolation
- ✅ Capability-based security at service level
- ✅ Simulated kernel provides isolation in tests

**Next Steps**:
- Page table management for x86_64
- User/kernel mode separation
- Memory allocation with isolation
- Page fault handling
- Copy-on-write support

---

### 3. Syscall ABI + Minimal Userspace Runtime
**Impact**: Bridge between userspace and kernel  
**Status**: Partially Complete - Needs expansion

**Why This Matters**:
- Defines stable interface between user and kernel code
- Enables true privilege separation
- Foundation for running untrusted code safely

**Current State**:
- ✅ `KernelApi` trait defines interface
- ✅ Syscall gate implementation in sim_kernel
- ✅ Message-based syscall codec
- 🔄 Needs: real syscall handler for bare metal, userspace runtime library

**Next Steps**:
- x86_64 syscall instruction handler
- Userspace C runtime (or Rust no_std runtime)
- Context switching between user/kernel
- Syscall argument validation

---

### 4. VFS + One Concrete Filesystem
**Impact**: Persistent data and file-based workflows  
**Status**: Foundation exists - Needs file system layer

**Why This Matters**:
- Users need to store and retrieve data persistently
- Foundation for all file-based operations
- Enables traditional file-based workflows

**Current State**:
- ✅ `services_storage` provides versioned object storage
- ✅ `services_fs_view` provides path-based illusion
- ✅ Blob, Log, Map storage types
- 🔄 Needs: VFS abstraction, concrete block-based filesystem

**Next Steps**:
- VFS layer for filesystem abstraction
- Simple filesystem implementation (FAT32 or custom)
- Block device driver interface
- Mounting and unmounting
- File operations (read, write, create, delete)

---

### 5. Interrupt/Driver Framework + Key Drivers
**Impact**: Hardware interaction and device support  
**Status**: Partial - Needs framework and more drivers

**Why This Matters**:
- All hardware interaction goes through drivers
- Enables keyboard, display, storage, network
- Foundation for bare-metal functionality

**Current State**:
- ✅ HAL abstraction exists (`hal/`, `hal_x86_64/`)
- ✅ VGA text mode driver
- ✅ Framebuffer driver
- ✅ PS/2 keyboard stub
- ✅ Input HAL bridge (Phase 169)
- 🔄 Needs: interrupt framework, timer driver, storage driver

**Next Steps**:
- Interrupt descriptor table (IDT) setup
- IRQ routing and handling
- Timer driver (PIT/HPET)
- Storage driver (SATA/NVMe/VirtIO)
- Network driver (virtio-net or e1000)

---

### 6. Executable Loader (ELF) + Userspace Process Launch
**Impact**: Run arbitrary programs  
**Status**: Foundation exists - Needs loader

**Why This Matters**:
- Users need to run programs they build or download
- Enables dynamic application ecosystem
- Foundation for app distribution

**Current State**:
- ✅ Task spawning via `KernelApi`
- ✅ Capability-based task construction
- 🔄 Needs: ELF parser, program loader, dynamic linking support

**Next Steps**:
- ELF file format parser
- Program header loading
- Address space setup for loaded programs
- Dynamic linker (for shared libraries)
- Initial process setup (stack, args, env)

---

### 7. IPC Primitives
**Impact**: Inter-process communication  
**Status**: Strong foundation exists

**Why This Matters**:
- Processes need to communicate and coordinate
- Foundation for microkernel architecture
- Enables service-oriented design

**Current State**:
- ✅ Message passing via `ipc/` crate
- ✅ Typed messages with correlation IDs
- ✅ Channel-based communication
- ✅ Capability transfer support
- ✅ Remote IPC over network (Phase 90+)
- 🔄 Needs: shared memory IPC, synchronization primitives

**Next Steps**:
- Shared memory regions (with isolation)
- Synchronization primitives (semaphores, mutexes)
- Faster message passing for performance-critical paths
- Multi-cast/broadcast channels

---

### 8. Networking Baseline
**Impact**: Network connectivity  
**Status**: Service layer exists - Needs network stack

**Why This Matters**:
- Modern systems require network connectivity
- Enables distributed services and remote access
- Foundation for cloud/distributed features

**Current State**:
- ✅ `services_network` service scaffold
- ✅ Remote IPC/UI capability
- 🔄 Needs: TCP/IP stack, socket API, network drivers

**Next Steps**:
- TCP/IP stack implementation or integration (smoltcp?)
- Socket-like API for userspace
- DHCP client
- DNS resolver
- Network driver (virtio-net initially)

---

### 9. Observability (Logging/Tracing/Panic Reports/Perf Counters)
**Impact**: Debugging and performance analysis  
**Status**: Good foundation - Needs expansion

**Why This Matters**:
- Developers need visibility into system behavior
- Critical for debugging and performance tuning
- Foundation for production monitoring

**Current State**:
- ✅ `services_logger` structured logging
- ✅ Audit trails for scheduling, capabilities, etc.
- ✅ Test-time introspection
- 🔄 Needs: tracing, panic reports, performance counters

**Next Steps**:
- Distributed tracing infrastructure
- Panic/crash report collection
- Performance counter framework
- Real-time metrics dashboard
- Stack trace unwinding

---

### 10. Reliability/Security Foundations
**Impact**: Production readiness  
**Status**: Strong design foundation - Needs hardening

**Why This Matters**:
- Systems must be resilient to failures and attacks
- Foundation for trustworthy computing
- Required for any production use

**Current State**:
- ✅ Capability-based security model
- ✅ No ambient authority
- ✅ Fault injection testing framework
- ✅ Resource budgets and enforcement
- ✅ Secure boot infrastructure
- 🔄 Needs: formal verification, fault tolerance, security hardening

**Next Steps**:
- Formal verification of critical paths (Phase 110+)
- Byzantine fault tolerance
- Encrypted storage
- Secure update mechanism
- Security audit and penetration testing

---

## Phased Roadmap

This roadmap balances dependencies and risk to deliver value incrementally.

### Phase 1: Core Execution (Months 1-2)
**Goal**: Robust process execution and scheduling

**Features**:
- ✅ Basic scheduler (Complete - Phase 23)
- 🔄 **Enhanced scheduler with priorities** (In Progress - Phase 170)
- 📋 Syscall ABI refinement for bare metal
- 📋 ELF loader implementation
- 📋 Basic process lifecycle management

**Deliverables**:
- Multi-priority preemptive scheduler
- Load and execute simple ELF binaries
- Basic userspace runtime library
- Context switching between tasks

**Success Criteria**:
- Can spawn, schedule, and run multiple user processes
- Processes can make syscalls
- Tests demonstrate correct scheduling behavior

---

### Phase 2: Isolation & Storage (Months 3-4)
**Goal**: Memory isolation and persistent storage

**Features**:
- 📋 Virtual memory management
- 📋 User/kernel isolation
- 📋 VFS layer
- 📋 Simple filesystem (FAT32 or custom)
- 📋 Block device driver

**Deliverables**:
- Page table management for x86_64
- Memory protection between processes
- VFS abstraction layer
- Working filesystem with persistent storage
- Basic block device driver (VirtIO or SATA)

**Success Criteria**:
- Processes are isolated from each other
- Files can be created, written, read, and deleted
- Data persists across reboots
- Tests demonstrate isolation guarantees

---

### Phase 3: Hardware & Networking (Months 5-6)
**Goal**: Real hardware interaction

**Features**:
- 📋 Interrupt framework
- 📋 Timer driver
- 📋 Storage driver (SATA/NVMe)
- 📋 Network driver (virtio-net)
- 📋 TCP/IP stack integration

**Deliverables**:
- Interrupt descriptor table and IRQ routing
- Working timer for scheduling
- Real storage driver (not RAM disk)
- Network connectivity with TCP/IP
- Basic socket API

**Success Criteria**:
- System responds to timer interrupts for preemption
- Can read/write to real storage device
- Can send/receive network packets
- Basic network services work (ping, simple HTTP)

---

### Phase 4: Robustness & Polish (Months 7-8)
**Goal**: Production-ready foundation

**Features**:
- 📋 Enhanced IPC (shared memory, sync primitives)
- 📋 Advanced observability (tracing, metrics)
- 📋 Security hardening
- 📋 Reliability improvements
- 📋 Documentation and examples

**Deliverables**:
- Shared memory IPC
- Synchronization primitives (mutexes, semaphores)
- Distributed tracing
- Performance monitoring dashboard
- Comprehensive documentation
- Example applications

**Success Criteria**:
- System is stable under stress testing
- Performance is acceptable for intended use cases
- Security model is validated
- Documentation enables third-party development

---

## Implementation Principles

Throughout all phases, we maintain PandaGen's core principles:

1. **Testability First**: Everything must be testable under `cargo test`
2. **No Legacy Compatibility**: We're not POSIX and we're proud of it
3. **Explicit Over Implicit**: No ambient authority, no hidden state
4. **Mechanism Over Policy**: Kernel provides primitives, services implement policy
5. **Modular Design**: Small, focused crates with clear interfaces
6. **Human-Readable**: Code should be understandable, not clever

---

## Success Metrics

We'll measure progress against these metrics:

- **Test Coverage**: >80% for all core crates
- **Build Time**: <2 minutes for full workspace
- **Test Time**: <10 seconds for full test suite
- **Boot Time**: <1 second to interactive prompt
- **Documentation**: Every public API documented with examples
- **Lines of Unsafe**: <5% of total codebase

---

## Future Considerations (Beyond Phase 4)

- Multi-core scheduling and synchronization
- Advanced graphics/GUI frameworks
- Container/sandbox support
- Distributed consensus and replication
- Real-time guarantees for critical tasks
- Formal verification of security properties
- Performance optimization and tuning

---

## Notes

- This roadmap is a living document and will evolve based on feedback and discoveries
- Actual implementation timelines depend on team size and priorities
- Each phase includes comprehensive testing and documentation
- Security and reliability are considered at every step, not bolt-on additions

---

**Status Legend**:
- ✅ Complete
- 🔄 In Progress
- 📋 Planned
- ❌ Not Started
