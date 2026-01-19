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
├── identity/                # Execution identities & trust domains
├── policy/                  # Policy engine framework
├── resources/               # Resource budgets & enforcement
├── lifecycle/               # Task lifecycle management
├── pipeline/                # Pipeline execution primitives
├── services_registry/       # Service discovery
├── services_process_manager/# Service lifecycle management
├── services_logger/         # Structured logging
├── services_storage/        # Versioned object storage
├── services_fs_view/        # Filesystem view illusion
├── services_pipeline_executor/ # Pipeline execution service
├── services_input/          # Input subscription management (Phase 14)
├── services_focus_manager/  # Focus control & routing (Phase 14)
├── input_types/             # Input event types (Phase 14)
├── fs_view/                 # Filesystem view client library
├── intent_router/           # Typed command routing
└── cli_console/             # Demo bootstrap & interactive console
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

**Input System (Phase 14)**
- Explicit input subscriptions via capabilities
- Keyboard events are structured (KeyEvent), not byte streams
- Stack-based focus management
- No TTY/stdin/stdout emulation
- Fully testable via event injection

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

### Bare-Metal Track

- B1 bootable ISO pipeline is available. See `docs/qemu_boot.md`.

## 📖 Documentation

- [Architecture Overview](docs/architecture.md) - System design and principles
- [Interfaces](docs/interfaces.md) - API reference and contracts

### Quick Example: Interactive Input

```rust
use input_types::{InputEvent, KeyEvent, KeyCode, Modifiers};
use services_input::InputService;
use services_focus_manager::FocusManager;

// Create services
let mut input_service = InputService::new();
let mut focus_manager = FocusManager::new();

// Subscribe to keyboard input
let cap = input_service.subscribe_keyboard(task_id, channel)?;

// Request focus
focus_manager.request_focus(cap)?;

// Process keyboard events
let event = InputEvent::key(
    KeyEvent::pressed(KeyCode::A, Modifiers::CTRL)
);

if let Some(focused_cap) = focus_manager.route_event(&event)? {
    // Deliver event to focused component
    println!("Ctrl+A pressed!");
}
```

### Quick Example: Capability-Based Task Spawning

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

### ✅ Phase 1: Foundation (Complete)
- [x] Workspace structure
- [x] Core types (Cap, IDs, errors)
- [x] IPC primitives
- [x] Kernel API trait
- [x] Simulated kernel
- [x] HAL skeleton
- [x] Service scaffolding
- [x] Documentation
- [x] CI/CD

### ✅ Phase 2-13: Core Services (Complete)
- [x] Storage service (versioned objects)
- [x] Logger service (structured logging)
- [x] Process manager (lifecycle)
- [x] Service registry (discovery)
- [x] Identity system (trust domains)
- [x] Policy engine framework
- [x] Resource budgets & enforcement
- [x] Filesystem view illusion
- [x] Pipeline execution
- [x] Fault injection & resilience testing

### ✅ Phase 14: Input System (Complete)
- [x] Input types (KeyEvent, KeyCode, Modifiers)
- [x] Input service (subscription management)
- [x] Focus manager (stack-based focus control)
- [x] SimKernel event injection utilities
- [x] Interactive console demo
- [x] Full documentation & examples

### Phase 15+: Future Work
- [ ] Real kernel implementation (baremetal)
- [ ] Hardware drivers (keyboard, display, etc.)
- [ ] Pointer/touch input support
- [ ] Multi-core support
- [ ] Graphics/UI framework
- [ ] Network stack
- [ ] Bootloader integration

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
 
