# PandaGen

**A modern operating system runtime designed from first principles**

PandaGen is an experimental OS-like runtime that intentionally rejects POSIX and legacy compatibility. It's a thought experiment: *What would an operating system look like if designed today using modern software engineering principles?*

## ⚠️ Project Status

This is a **research prototype** and **foundation scaffold**. It is:
- ✅ Designed for testability and clarity
- ✅ Modular and evolvable
- ✅ Fully functional under `cargo test`
- ❌ Not a replacement for Linux/BSD/Windows
- ❌ Not production-ready
- ❌ Not POSIX-compatible (by design)

## 🎯 Philosophy

### Why This Exists

Legacy operating systems optimize for backward compatibility, not clarity. We believe:

1. **Testability is a first-class design constraint**
   - If something cannot be unit tested, its design is suspect
   - Most logic runs under `cargo test` on a normal host
   - Kernel code is minimal precisely because it's harder to test

2. **Modularity over convenience**
   - Everything is replaceable: storage, input, commands, UI, policies
   - No global namespaces
   - No hidden inheritance of state or privilege
   - Clear interfaces > clever shortcuts

3. **Explicit over implicit**
   - Capabilities instead of permissions
   - Construction instead of `fork()`
   - Message passing instead of shared mutable state
   - Typed interfaces instead of stringly-typed conventions

4. **Mechanism, not policy**
   - The kernel provides primitives, not opinions
   - Services implement policy in user space
   - Decisions are changeable without rewriting the system

5. **No legacy compatibility by design**
   - POSIX is not a goal
   - "Everything is a file" is not a goal
   - Shell pipelines, path-based filesystems, signals, fork/exec are not goals
   - Innovation is allowed because compatibility is explicitly rejected

6. **Humans should be able to reason about this system**
   - Clear naming
   - Small crates
   - Minimal unsafe code
   - Documentation that explains *why*, not just *what*

## 🏗️ Architecture

### Crate Structure

```
PandaGen/
├── core_types/              # Fundamental types (Cap<T>, IDs)
├── ipc/                     # Message passing primitives
├── kernel_api/              # Kernel interface trait
├── sim_kernel/              # Simulated kernel (for testing)
├── hal/                     # Hardware abstraction traits
├── hal_x86_64/              # x86_64 HAL implementation (skeleton)
├── services_registry/       # Service discovery
├── services_process_manager/# Service lifecycle management
├── services_logger/         # Structured logging
├── services_storage/        # Versioned object storage
├── intent_router/           # Typed command routing
└── cli_console/             # Demo bootstrap (not a shell)
```

### Key Design Decisions

**No POSIX**
- No `fork()`, `exec()`, `pipe()`, `signal()`
- Tasks are constructed explicitly with capabilities
- Communication is via typed messages, not file descriptors

**No Filesystem Paths**
- Objects have IDs, not paths
- Every modification creates a new version
- Storage types: Blob (immutable), Log (append-only), Map (key-value)

**Capability-Based Security**
- `Cap<T>` is a strongly-typed, unforgeable handle
- Authority is explicitly granted, never ambient
- Having a capability is the proof of authority

**Message Passing**
- All IPC is via structured messages
- Messages have schema versions for compatibility
- Correlation IDs for request/response matching

**Simulated Kernel**
- Full kernel API implementation that runs in-process
- Controlled time for deterministic testing
- Inspectable state for debugging

## 🚀 Getting Started

### Prerequisites

- Rust 1.70+ (2021 edition)
- Cargo

### Build

```bash
cargo build
```

### Test

```bash
cargo test
```

### Lint

```bash
cargo fmt --check
cargo clippy -- -D warnings
```

## 📖 Documentation

- [Architecture Overview](docs/architecture.md) - System design and principles
- [Interfaces](docs/interfaces.md) - API reference and contracts

### Quick Example

```rust
use sim_kernel::SimulatedKernel;
use kernel_api::{KernelApi, TaskDescriptor};

// Create a simulated kernel
let mut kernel = SimulatedKernel::new();

// Spawn a task (explicit construction, not fork)
let descriptor = TaskDescriptor::new("my_service".to_string());
let handle = kernel.spawn_task(descriptor)?;

// Create a communication channel
let channel = kernel.create_channel()?;

// Send a message
kernel.send_message(channel, message)?;
```

## 🧪 Testing Philosophy

**Everything is testable.** This is not negotiable.

- ✅ Core types have comprehensive unit tests
- ✅ Kernel API is fully mocked/simulated
- ✅ Time is controllable (no flaky tests)
- ✅ All tests run in milliseconds
- ✅ No external dependencies required

Run tests with:
```bash
cargo test --all
```

## 🛣️ Roadmap

### Phase 1: Foundation (Current)
- [x] Workspace structure
- [x] Core types (Cap, IDs, errors)
- [x] IPC primitives
- [x] Kernel API trait
- [x] Simulated kernel
- [x] HAL skeleton
- [x] Service scaffolding
- [x] Documentation
- [x] CI/CD

### Phase 2: Services (Next)
- [ ] Storage service implementation
- [ ] Logger service implementation
- [ ] Process manager implementation
- [ ] Intent router implementation
- [ ] Service registry implementation

### Phase 3: Advanced Features
- [ ] Real kernel implementation (baremetal)
- [ ] Hardware drivers
- [ ] Multi-core support
- [ ] Bootloader integration

### Phase 4: Ecosystem
- [ ] Development tools
- [ ] Application framework
- [ ] Examples and tutorials

## 🤝 Contributing

This is an experimental project. Contributions are welcome, but please:

1. Read the philosophy (this matters!)
2. Maintain testability
3. Keep abstractions clean
4. Document your reasoning

## 📜 License

MIT OR Apache-2.0

## 🙏 Acknowledgments

Inspired by:
- **seL4**: Formal verification and microkernel design
- **Fuchsia**: Capability-based security
- **Plan 9**: "Everything is a file" critique
- **Erlang**: Message passing and fault tolerance
- **Rust**: Type safety and zero-cost abstractions

---

**Remember:** This is not a Linux alternative. This is an exploration of what's possible when we reject backward compatibility and embrace modern software engineering.
 
