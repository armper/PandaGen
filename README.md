# PandaGen

**A modern operating system runtime designed from first principles**

PandaGen is an experimental OS-like runtime that intentionally rejects POSIX and legacy compatibility. It's a thought experiment: *What would an operating system look like if designed today using modern software engineering principles?*

## ⚠️ Project Status

This is a **research prototype** and **advanced foundation**. It is:
- ✅ Designed for testability and clarity
- ✅ Modular and evolvable
- ✅ Fully functional under `cargo test`
- ✅ Boots on x86_64 bare metal (via QEMU), on all CPUs
- ✅ Graphical desktop (own rasterizer and compositor) plus a text console
- ✅ Interactive text editor with workspace management
- ✅ Capability-based storage with permissions, persistent across reboots (virtio-blk)
- ✅ Networking: DHCP, ping, UDP, TCP, and an authenticated remote command channel
- ✅ Critical paths checked against executable models (bounded model checking)
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
├── core_types/                 # Fundamental types (Cap<T>, IDs)
├── graphics_rasterizer/        # no_std RGBA rasterizer, fonts, images
├── net_stack/                  # no_std Ethernet/ARP/IPv4/ICMP/UDP/TCP/DHCP
├── ipc/                        # Message passing primitives
├── kernel_api/                 # Kernel interface trait
├── sim_kernel/                 # Simulated kernel (for testing)
├── hal/                        # Hardware abstraction traits
├── hal_x86_64/                 # x86_64 HAL implementation
├── identity/                   # Execution identities & trust domains
├── policy/                     # Policy engine framework
├── resources/                  # Resource budgets & enforcement
├── lifecycle/                  # Task lifecycle management
├── pipeline/                   # Pipeline execution primitives
├── services_registry/          # Service discovery
├── services_process_manager/   # Service lifecycle management
├── services_logger/            # Structured logging
├── services_storage/           # Versioned object storage with permissions
├── services_fs_view/           # Filesystem view illusion
├── services_pipeline_executor/ # Pipeline execution service
├── services_input/             # Input subscription management
├── services_focus_manager/     # Focus control & routing
├── services_command_palette/   # Command palette with fuzzy search
├── services_view_host/         # View rendering coordination
├── services_gui_host/          # GUI composition and rendering
├── services_remote_ui_host/    # Remote UI over network
├── services_editor_vi/         # Vi-style text editor service
├── services_workspace_manager/ # Workspace and component management
├── services_file_picker/       # File selection interface
├── services_app_store/         # Application package management
├── services_network/           # Network stack primitives
├── services_notification/      # System notifications
├── services_job_scheduler/     # Job scheduling and execution
├── services_device_manager/    # Device enumeration and management
├── services_settings/          # Configuration management
├── input_types/                # Input event types
├── view_types/                 # View rendering types
├── editor_core/                # Reusable editor logic
├── text_renderer_host/         # Text rendering engine
├── console_vga/                # VGA console rendering
├── console_fb/                 # Framebuffer console
├── fs_view/                    # Filesystem view client library
├── intent_router/              # Typed command routing
├── packages/                   # Package metadata types
├── package_registry/           # Package registry service
├── remote_ipc/                 # Remote IPC with capabilities
├── distributed_storage/        # Distributed storage coordination
├── workspace_access/           # Workspace access control
├── developer_sdk/              # Developer tools and utilities
├── formal_verification/        # Formal verification tools
├── secure_boot/                # Secure boot infrastructure
├── kernel_bootstrap/           # Bare-metal kernel bootstrap
├── boot/                       # Boot loader integration
├── cli_console/                # Demo bootstrap & interactive console
├── tests_pipelines/            # Pipeline integration tests
├── tests_resilience/           # Resilience and fault injection tests
└── contract_tests/             # Contract testing infrastructure
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

PandaGen boots on x86_64 hardware (QEMU is the reference machine):
- ✅ Bootable ISO via Limine; `cargo xtask iso` then `cargo xtask qemu`
- ✅ Framebuffer text console and a graphical desktop (`display graphics`)
- ✅ PS/2 keyboard and mouse
- ✅ Persistent storage on virtio-blk over PCI; files survive reboots
- ✅ SMP: every CPU online with its own GDT/TSS and LAPIC timer; idle CPUs poll kernel tasks and share the desktop present (3x faster)
- ✅ CPU exceptions print a register dump instead of triple-faulting
- ✅ virtio-net over PCI with DHCP, ARP, ICMP ping, UDP echo, and a TCP server
- ✅ Remote read-only commands over UDP (`remote_ipc` envelopes) and TCP (signed lines), HMAC-authenticated with replay protection

See `docs/qemu_boot.md` for build and boot instructions and `docs/next_steps.md` for what is in progress.

### Talking To A Running Kernel

With `cargo xtask qemu` running (it forwards the kernel's ports to localhost):

```bash
cargo xtask remote cpus          # remote_ipc over UDP 7778
cargo xtask remote-tcp net       # signed line protocol over TCP 7780
```

Scripted, headless verification drives keystrokes, mouse, screenshots, and host-side network steps in one run:

```bash
cargo xtask qemu-script --keys "sleep:4,udp:hello,remote-tcp:cpus;online=4,tcp:echo,shot:desk" --expect-serial "SMP: 4 of 4 CPUs online"
```

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

### ✅ Phase 14-56: Input & Bare-Metal Foundation (Complete)
- [x] Input types (KeyEvent, KeyCode, Modifiers)
- [x] Input service (subscription management)
- [x] Focus manager (stack-based focus control)
- [x] SimKernel event injection utilities
- [x] Interactive console demo
- [x] Bare-metal boot proof (Phase 56)
- [x] x86_64 HAL implementation

### ✅ Phase 57-90: Views, Rendering & Editor (Complete)
- [x] View host & snapshot rendering (Phase 60)
- [x] Framebuffer console support
- [x] VGA text mode console
- [x] Filesystem permissions & ownership (Phase 80)
- [x] Unified editor architecture (Phase 90)
- [x] Core editor extraction
- [x] Text rendering engine

### ✅ Phase 91-117: Workspace, UI & Performance (Complete)
- [x] Fast framebuffer editor rendering (Phase 100)
- [x] File persistence and UX improvements
- [x] Workspace modernization (Phase 110)
- [x] Command palette with fuzzy search
- [x] Workspace manager service
- [x] Vi-style editor service
- [x] Bare-metal workspace platform adapter (Phase 115)
- [x] High-impact performance optimizations (Phase 117)

### ✅ Phase 213-251: Graphics (Complete)
- [x] `graphics_rasterizer`: no_std RGBA rasterizer, 8x16 font, clipping, images
- [x] `services_gui_host`: compositor, damage tracking, theme, animation, telemetry
- [x] Graphical shell: launcher, command palette, editor, file picker, notifications
- [x] Bare-metal desktop mode with pointer, present pacing, memory pressure policy

### ✅ Phase 252-273: Storage, SMP, Networking, Verification (Complete)
- [x] Persistent virtio-blk storage over PCI, mounted across reboots (252-253)
- [x] SMP bring-up: spinlocks, CPU registry, LAPIC, IPIs, job queue, parallel present, per-CPU timers, GDT/TSS, exception diagnostics, tasks on any CPU (254-256, 261-263)
- [x] virtio-net, ARP/IPv4/ICMP/UDP, DHCP, TCP (257-258, 268, 272)
- [x] Authenticated, replay-protected remote commands over UDP and TCP (259-260, 267, 273)
- [x] Model-based checkers for capabilities, scheduling (round-robin and EDF), IPC access, message budgets, and the kernel heap; nine `sim_kernel` defects found and fixed (264-271)

### 🔄 Open Work (see `docs/next_steps.md`)
- [x] Parallel task execution across CPUs (Phase 275); preemptive tasks with context switching remain open
- [ ] Per-caller keys for remote commands; DHCP lease renewal
- [ ] Out-of-order TCP reassembly and congestion control

Each phase is recorded in a `PHASE<n>_SUMMARY.md` at the repository root.

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
 
