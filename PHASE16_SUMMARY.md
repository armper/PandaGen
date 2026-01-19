# Phase 16: Workspace Manager — Component Orchestration

## Overview

Phase 16 introduces the **Workspace Manager**, a user-facing component orchestrator that manages component lifecycle, focus, and interaction without being a POSIX shell or global stdin/stdout router.

## Philosophy

The Workspace Manager is **NOT**:
- A POSIX shell with job control
- A byte-stream interpreter
- A stdin/stdout multiplexer
- A monolithic "god shell"
- A terminal emulator

The Workspace Manager **IS**:
- A component orchestrator
- Focus-aware and capability-driven
- Observable and auditable
- Policy-enforced
- Budget-constrained

## Key Concepts

### Component-Based Model

Unlike traditional shells that manage processes and file descriptors, the Workspace Manager manages **components**:

- **ComponentId**: Unique identifier for each component instance
- **ComponentType**: Editor, CLI, PipelineExecutor, or Custom
- **ComponentState**: Running, Exited, Cancelled, or Failed
- **ComponentInfo**: Full metadata including identity, focus, and lifecycle

### Explicit Focus Management

Focus is never ambient. The Workspace Manager:
1. Coordinates with the Focus Manager service
2. Routes input only to the focused component
3. Tracks focus changes in audit trail
4. Enforces policy on focus grants

### Observable Lifecycle

Every component transition is recorded as a `WorkspaceEvent`:
- ComponentLaunched
- ComponentFocused
- ComponentUnfocused
- ComponentStateChanged
- ComponentTerminated

### Policy and Budget Enforcement

Components are subject to:
- **Identity**: Each component has an ExecutionId and IdentityMetadata
- **Trust domains**: Components inherit workspace's domain policies
- **Resource budgets**: CPU ticks, message counts, etc.
- **Policy evaluation**: Launch and focus requests may be denied

## Architecture

### Core Types

```rust
pub struct ComponentInfo {
    pub id: ComponentId,
    pub component_type: ComponentType,
    pub identity: IdentityMetadata,
    pub state: ComponentState,
    pub focusable: bool,
    pub subscription: Option<InputSubscriptionCap>,
    pub cancellation: CancellationSource,
    pub exit_reason: Option<ExitReason>,
    pub name: String,
    pub metadata: HashMap<String, String>,
}

pub struct WorkspaceManager {
    components: HashMap<ComponentId, ComponentInfo>,
    focus_manager: FocusManager,
    policy: Option<Box<dyn PolicyEngine>>,
    audit_trail: Vec<WorkspaceEvent>,
    workspace_identity: IdentityMetadata,
}
```

### Command Interface

The workspace provides a minimal command surface:

| Command | Description | Example |
|---------|-------------|---------|
| `open <type> [args...]` | Launch a component | `open editor notes.txt` |
| `list` | List all components | `list` |
| `focus <id>` | Focus specific component | `focus comp:abc123...` |
| `next` | Focus next component | `next` |
| `prev` | Focus previous component | `prev` |
| `close <id>` | Terminate component | `close comp:abc123...` |
| `status <id>` | Get component status | `status comp:abc123...` |

Commands do NOT:
- Manipulate files directly
- Pipe data between components
- Redirect I/O streams
- Implement job control

### Focus Integration

The workspace integrates tightly with `services_focus_manager`:

1. **On component launch**: If focusable, create InputSubscriptionCap
2. **On focus request**: Check policy, then grant via FocusManager
3. **On focus switch**: Release old focus, grant new focus
4. **On component exit**: Automatically remove from focus stack

### Policy Enforcement

Policy checks occur at:

1. **Launch time**: PolicyEvent::OnSpawn
   - May deny based on trust domain
   - May deny based on identity kind
   - May deny based on resource constraints

2. **Focus time**: PolicyEvent::OnCapabilityDelegate
   - Cross-domain focus may require approval
   - Sandbox components may have restrictions

Example with TrustDomainPolicy:
- Sandbox cannot spawn System services
- Sandbox cannot spawn Core domain services
- Cross-domain capability delegation requires approval

### Budget Enforcement

Components can have attached ResourceBudgets:
- CPU ticks
- Message count
- Memory allocations
- I/O operations

When budget exhausted:
1. Workspace calls `handle_budget_exhaustion(component_id)`
2. Component terminates with ExitReason::Failure
3. Focus revoked automatically
4. Audit trail records termination

## Integration Points

### With Focus Manager

```rust
// Create subscription for focusable component
let subscription = InputSubscriptionCap::new(id, task_id, channel_id);
component = component.with_subscription(subscription);

// Grant focus
focus_manager.request_focus(subscription)?;

// Remove on exit
focus_manager.remove_subscription(&subscription)?;
```

### With Policy Engine

```rust
// Check policy on launch
let context = PolicyContext::for_spawn(workspace_identity, component_identity);
let decision = policy.evaluate(PolicyEvent::OnSpawn, &context);

if let PolicyDecision::Deny { reason } = decision {
    return Err(WorkspaceError::LaunchDenied { reason });
}
```

### With Identity and Lifecycle

```rust
// Create component identity
let identity = IdentityMetadata::new(
    identity_kind,
    trust_domain,
    name,
    timestamp
).with_parent(workspace_identity.execution_id);

// Attach cancellation
let cancellation = CancellationSource::new();
component.cancellation_token(); // Can be passed to component
```

## Testing Strategy

### Unit Tests (23 tests in lib.rs)

- Component creation and lifecycle
- Focus management correctness
- State transitions
- Budget attachment
- Metadata preservation

### Integration Tests (11 tests)

- Policy enforcement (allow/deny)
- Budget exhaustion handling
- Command parsing and execution
- Multi-component focus switching
- Audit trail completeness

All tests are deterministic and run under SimKernel.

## Observability

### Audit Trail

Every workspace operation is logged:

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

### Component Inspection

```rust
// List all components
let components = workspace.list_components();

// Get specific component
let component = workspace.get_component(component_id)?;

// Get focused component
let focused_id = workspace.get_focused_component();
```

## Non-Goals (Enforced)

This phase explicitly does NOT:

1. **No POSIX shell**: No $VAR expansion, no pipes, no redirects
2. **No global I/O**: No stdin/stdout/stderr concepts
3. **No job control**: No background jobs, no fg/bg/jobs
4. **No implicit state**: Everything is explicit and observable
5. **No ambient authority**: All capabilities are explicit

## Future Work (Out of Scope)

Phase 16 is complete as-is. Future phases MAY add:

- Component templates for common use cases
- Workspace persistence (save/restore sessions)
- Multi-workspace support
- Component dependency graphs
- Advanced scheduling policies

These are intentionally deferred to avoid scope creep.

## Migration from Traditional Shells

| Traditional Shell | PandaGen Workspace |
|-------------------|-------------------|
| `vim file.txt` | `open editor file.txt` |
| `jobs` | `list` |
| `fg %1` | `focus <id>` |
| `kill %1` | `close <id>` |
| `echo "data" \| cmd` | Components communicate via IPC |
| `cmd &` | Components run independently, use `list` to see them |
| `export VAR=value` | Pass metadata at component launch |

## Quality Metrics

- **Lines of Code**: ~1,200 (including tests)
- **Test Coverage**: 34 tests, 100% pass
- **Clippy**: Zero warnings with `-D warnings`
- **Fmt**: All code formatted
- **Documentation**: Comprehensive inline and external docs

## Deliverables

1. ✅ `services_workspace_manager` crate
2. ✅ Component lifecycle management
3. ✅ Focus integration with `services_focus_manager`
4. ✅ Command interface with parsing
5. ✅ Policy enforcement integration
6. ✅ Budget exhaustion handling
7. ✅ Observable audit trail
8. ✅ Comprehensive test suite
9. ⏳ CLI integration (deferred - can be done in separate phase)
10. ✅ Documentation

## Conclusion

Phase 16 delivers a minimal, testable, policy-aware component orchestrator that proves PandaGen can provide user-facing abstractions without recreating POSIX. The workspace is observable, auditable, and deterministic—fitting the PandaGen philosophy perfectly.
