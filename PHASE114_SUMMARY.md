# Phase 114: Platform Adapter Traits for services_workspace_manager

**Date**: 2026-01-25  
**Status**: ✅ Complete  
**Goals**: Prepare `services_workspace_manager` for bare-metal integration by introducing platform adapter traits

---

## Problem Statement

The workspace manager was tightly coupled to simulation environments, with bare-metal systems using a separate implementation in `kernel_bootstrap/src/workspace.rs`. This created divergence and made it difficult to share workspace features between simulation and bare-metal environments.

**Key Issues**:
- No platform abstraction layer in `services_workspace_manager`
- Direct dependencies on `std::collections` preventing no_std usage
- No explicit entry point for platform integration
- Workspace logic mixed with simulation-specific I/O

---

## Solution Design

Introduce a **minimal platform abstraction layer** with these components:

### 1. Platform Traits (`platform` module)

Four small traits define the platform contract:

```rust
pub trait WorkspacePlatform {
    fn display(&mut self) -> &mut dyn WorkspaceDisplay;
    fn input(&mut self) -> &mut dyn WorkspaceInput;
    fn tick(&mut self) -> &mut dyn WorkspaceTick;
}

pub trait WorkspaceDisplay {
    fn render_main_view(&mut self, frame: &ViewFrame);
    fn render_status_view(&mut self, frame: &ViewFrame);
    fn render_status_strip(&mut self, content: &str);
    fn render_breadcrumbs(&mut self, content: &str);
    fn clear(&mut self);
    fn present(&mut self);
}

pub trait WorkspaceInput {
    fn poll_event(&mut self) -> Option<KeyEvent>;
    fn has_pending(&self) -> bool;
}

pub trait WorkspaceTick {
    fn advance(&mut self) -> u64;
    fn current(&self) -> u64;
}
```

**Design Principles**:
- **Minimal surface area**: Only abstract what's necessary
- **Explicit, not implicit**: All platform interactions go through traits
- **Deterministic**: No timers, no threads - caller-driven ticks
- **Capability-based**: Reuses existing `ViewFrame`, `KeyEvent` types

### 2. WorkspaceRuntime Public Entry Point

Single public API for driving the workspace:

```rust
pub struct WorkspaceRuntime<P: WorkspacePlatform> {
    platform: P,
    workspace: WorkspaceManager,
    tick_count: u64,
}

impl<P: WorkspacePlatform> WorkspaceRuntime<P> {
    pub fn new(platform: P, identity: IdentityMetadata, caps: WorkspaceCaps) -> Self;
    pub fn handle_input(&mut self);
    pub fn tick(&mut self);
    pub fn render(&mut self);
    pub fn workspace(&self) -> &WorkspaceManager;
    pub fn workspace_mut(&mut self) -> &mut WorkspaceManager;
}
```

**Benefits**:
- Clear contract for platform integrators
- Encapsulates the input → tick → render cycle
- Provides controlled access to underlying `WorkspaceManager`

### 3. no_std Compatibility (Feature-Gated)

Made the crate compatible with `no_std` environments:

```toml
[features]
default = ["std"]
std = []
```

**Implementation**:
- `#![cfg_attr(not(feature = "std"), no_std)]` at crate root
- Feature-gated imports:
  - `std::collections::HashMap` → `hashbrown::HashMap` (no_std)
  - `std::collections::VecDeque` → `alloc::collections::VecDeque` (no_std)
  - `println!` gated with `cfg(all(debug_assertions, feature = "std"))`
- Added `extern crate alloc` for heap allocations in no_std

---

## Implementation

### Files Changed

```
services_workspace_manager/
├── Cargo.toml                    # Added features, hashbrown dependency
├── src/
│   ├── lib.rs                    # Added no_std cfg, WorkspaceRuntime
│   ├── platform/
│   │   ├── mod.rs                # Platform traits
│   │   └── fake.rs               # FakePlatform for testing
│   ├── boot_profile.rs           # Feature-gated HashMap
│   ├── keybindings.rs            # Feature-gated HashMap
│   ├── workspace_status.rs       # Feature-gated VecDeque
│   └── help.rs                   # core::fmt instead of std::fmt
└── tests/
    └── runtime_tests.rs          # 8 new tests for WorkspaceRuntime
```

### Test Coverage

**New Tests** (8 added):
- `test_runtime_creation` - Basic construction
- `test_runtime_tick_advances` - Tick counter increments
- `test_runtime_handles_input` - Input routing to components
- `test_runtime_render_calls_platform` - Render invocation
- `test_runtime_workspace_access` - Accessor methods
- `test_runtime_with_storage_capability` - Capability injection
- `test_runtime_full_cycle` - Complete input→tick→render cycle
- `test_fake_platform_display_tracking` - Platform state tracking

**Platform Tests** (4 in fake.rs):
- `test_fake_platform_creation`
- `test_fake_input_queue`
- `test_fake_display_operations`
- `test_fake_tick_advances`

**All Existing Tests Still Pass**: 135 lib + 22 integration + 8 runtime + 1 doctest = **166 tests total**

---

## FakePlatform for Testing

Provides deterministic, in-memory platform for unit tests:

```rust
let mut platform = FakePlatform::new();
platform.queue_input(KeyEvent::pressed(KeyCode::A, Modifiers::none()));

let runtime = WorkspaceRuntime::new(platform, identity, caps);
runtime.handle_input();  // Processes queued events
runtime.tick();          // Advances time
runtime.render();        // Renders to fake display
```

**Features**:
- Queue input events for replay
- Inspect rendered content (status strip, breadcrumbs)
- Track clear/present calls
- Deterministic tick counter

---

## Migration Path

### For Simulation (No Changes Required)

Existing code using `WorkspaceManager` directly continues to work:

```rust
let mut workspace = WorkspaceManager::new(identity);
workspace.launch_component(config)?;
workspace.route_input(&event);
let snapshot = workspace.render_snapshot();
```

### For Bare-Metal (Future Work)

Implement platform traits:

```rust
struct BareMetalPlatform {
    vga_display: VgaDisplay,
    keyboard: PS2Keyboard,
    tick: AtomicU64,
}

impl WorkspacePlatform for BareMetalPlatform { /* ... */ }

// In kernel_bootstrap:
let platform = BareMetalPlatform::new();
let mut runtime = WorkspaceRuntime::new(platform, identity, caps);

loop {
    runtime.handle_input();
    runtime.tick();
    runtime.render();
}
```

---

## Design Rationale

### Why Traits (Not Structs)?

- **Flexibility**: Simulation, bare-metal, and test implementations have different needs
- **Testability**: `FakePlatform` provides deterministic testing
- **No coupling**: Traits don't impose specific data structures

### Why WorkspaceRuntime Wrapper?

- **Clear API boundary**: Single entry point for platform integrators
- **Encapsulation**: Hides direct `WorkspaceManager` access unless needed
- **Coordination**: Manages input→tick→render cycle consistently

### Why Feature-Gated std (Not Pure no_std)?

- **Backward compatibility**: Existing simulation code uses std
- **Gradual migration**: Can build for no_std when ready
- **Testing**: Can test both configurations

---

## Philosophy Alignment

✅ **No POSIX assumptions**: Traits don't assume TTY, file descriptors, or fork/exec  
✅ **Testability first**: `FakePlatform` enables deterministic unit tests  
✅ **Modular and explicit**: Platform operations are explicit trait calls  
✅ **Mechanism over policy**: Traits provide mechanisms; platforms implement policy  
✅ **Human-readable**: Clear trait names and minimal surface area  

---

## Next Steps (Future MRs)

1. **Bare-Metal Integration**: Implement `BareMetalPlatform` in `kernel_bootstrap`
2. **QEMU Testing**: Wire up platform to actual VGA and PS/2 keyboard
3. **Consolidation**: Deprecate duplicate workspace logic in `kernel_bootstrap/src/workspace.rs`
4. **Feature Parity**: Ensure bare-metal gets command palette, breadcrumbs, etc.

---

## Metrics

- **Lines Changed**: ~800 lines added (traits, runtime, tests, feature gates)
- **Test Coverage**: +12 new tests (8 runtime + 4 platform)
- **Breaking Changes**: None (additive only)
- **Build Time Impact**: Negligible (hashbrown only in no_std builds)
- **Memory Impact**: Zero (no new runtime allocations)

---

## Review Checklist

- ✅ All tests pass (166/166)
- ✅ No breaking changes to existing API
- ✅ Documentation added for all public items
- ✅ Philosophy principles followed
- ✅ Feature gates tested (std and no_std)
- ✅ Platform traits have minimal surface area
- ✅ FakePlatform enables deterministic testing

---

## Retrospective

**What Went Well**:
- Traits cleanly separate platform concerns from workspace logic
- WorkspaceRuntime provides single integration point
- Feature-gated no_std preserves backward compatibility
- FakePlatform makes testing straightforward

**What Could Be Better**:
- Could add more sophisticated FakePlatform inspectors
- Display trait might need refinement for complex UIs
- Tick abstraction is simple but might need rate control later

**Lessons Learned**:
- Trait-based abstraction works well for platform-agnostic code
- Feature gates allow gradual no_std migration without breaking existing code
- Minimal trait surface area keeps implementation burden low
