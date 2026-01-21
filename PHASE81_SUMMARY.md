# Phase 81: Process Isolation UX

## Overview

Phase 81 surfaces process isolation visibly through user-facing commands. It adds `ps` for listing processes, `kill` for stopping them, crash reason visibility, and restart policy display. This transforms the process manager from an invisible system service into a visible, manageable component.

## What It Adds

1. **`ps` Command**: List all running components/processes
2. **`kill <id>` Command**: Stop or terminate processes
3. **Crash Reason Visibility**: Show WHY a process failed
4. **Restart Policy Display**: Show how processes will restart
5. **Process Status Summary**: Human-readable process state

## Why It Matters

**This is where PandaGen starts feeling like a system, not an app.**

Before Phase 81:
- Process manager exists but is invisible
- No way to see what's running
- Can't stop or restart services manually
- Crashes are silent
- Restart policies are hidden

After Phase 81:
- `ps` shows all processes with states
- `kill` stops/terminates services
- Crashes show "Panic: out of memory" not just "failed"
- Status shows restart attempts: "Running [restarts: 3]"
- Users can manage processes like a real OS

## Architecture

### New Module: `services_process_manager::process_info`

**Location**: `/services_process_manager/src/process_info.rs`

**Purpose**: User-facing process management (ps, kill, status)

**Key Types**:
```rust
/// Process information for display
pub struct ProcessInfo {
    service_id: ServiceId,
    task_id: TaskId,
    name: String,
    state: LifecycleState,
    status: String,  // "Running", "Failed (Exit code: 1)", etc.
    restart_policy: RestartPolicy,
}

/// Process listing (ps command)
pub struct ProcessList {
    processes: HashMap<ServiceId, ProcessInfo>,
}

/// Kill signal types
pub enum KillSignal {
    Terminate,  // Graceful (SIGTERM)
    Kill,       // Force (SIGKILL)
    Interrupt,  // User interrupt (SIGINT)
}

/// Kill operation result
pub enum KillResult {
    Success { service_id, signal },
    NotFound { service_id },
    AlreadyStopped { service_id },
    Failed { service_id, reason },
}
```

### Enhanced Lifecycle

**Updated ServiceHandle** (`lifecycle.rs`):
```rust
pub struct ServiceHandle {
    pub task_id: TaskId,
    pub state: LifecycleState,
    pub crash_reason: Option<CrashReason>,  // NEW
    pub restart_count: u32,                  // NEW
}

pub enum CrashReason {
    Panic(String),           // "Panic: out of memory"
    ExitCode(i32),           // "Exit code: 1"
    Signal(String),          // "Killed by signal: TERM"
    ResourceLimit(String),   // "Resource limit exceeded: CPU"
    Timeout,                 // "Timeout"
    Error(String),           // "Error: connection refused"
}

impl ServiceHandle {
    pub fn set_crash_reason(&mut self, reason: CrashReason);
    pub fn increment_restart_count(&mut self);
    pub fn status_summary(&self) -> String;
}
```

### Process Listing (ps)

**Usage**:
```rust
let mut ps_list = ProcessList::new();

// Add processes
for (service_id, descriptor) in services {
    let handle = get_service_handle(service_id);
    let info = ProcessInfo::new(
        service_id,
        descriptor.name,
        &handle,
        descriptor.restart_policy,
    );
    ps_list.add(service_id, info);
}

// Display
println!("{}", ps_list.format_table());
```

**Output**:
```
SERVICE ID                           NAME                 STATE           STATUS
────────────────────────────────────────────────────────────────────────────────────────────────
123e4567-e89b-12d3-a456-426614174000 logger               Running         Running
234e5678-f90c-23d4-b567-527725185111 storage              Running         Running
345e6789-001d-34e5-c678-628836196222 cli-console          Failed          Failed (Exit code: 1) [restarts: 2]
456e789a-112e-45f6-d789-729947207333 editor               Running         Running [restarts: 1]
```

**API**:
- `list_all()`: All processes (sorted by name)
- `list_by_state(state)`: Filter by LifecycleState
- `get(service_id)`: Get specific process
- `get_by_name(name)`: Lookup by name
- `format_table()`: Human-readable table

### Process Killing (kill)

**Signals**:
```rust
pub enum KillSignal {
    Terminate,  // Graceful shutdown (cleanup, save state)
    Kill,       // Force kill (immediate termination)
    Interrupt,  // User interrupt (Ctrl+C equivalent)
}
```

**Usage**:
```rust
// Send TERM signal
let result = kill_process(service_id, KillSignal::Terminate);

match result {
    KillResult::Success { service_id, signal } => {
        println!("Sent {} to service {}", signal.name(), service_id);
    }
    KillResult::NotFound { service_id } => {
        println!("Service {} not found", service_id);
    }
    KillResult::AlreadyStopped { service_id } => {
        println!("Service {} is already stopped", service_id);
    }
    KillResult::Failed { service_id, reason } => {
        println!("Failed to kill {}: {}", service_id, reason);
    }
}
```

**Behavior**:
- `Terminate`: Request graceful shutdown, allow cleanup
- `Kill`: Immediate forceful termination, no cleanup
- `Interrupt`: User-initiated stop (Ctrl+C)

### Crash Reason Tracking

**Example Scenarios**:

**Panic**:
```rust
handle.set_crash_reason(CrashReason::Panic("Out of memory".to_string()));
// Status: "Failed (Panic: Out of memory)"
```

**Exit Code**:
```rust
handle.set_crash_reason(CrashReason::ExitCode(127));
// Status: "Failed (Exit code: 127)"
```

**Timeout**:
```rust
handle.set_crash_reason(CrashReason::Timeout);
// Status: "Failed (Timeout)"
```

**Resource Limit**:
```rust
handle.set_crash_reason(CrashReason::ResourceLimit("Memory limit exceeded".to_string()));
// Status: "Failed (Resource limit exceeded: Memory limit exceeded)"
```

### Restart Tracking

**Example**:
```rust
let mut handle = ServiceHandle::new(task_id, LifecycleState::Starting);

// First attempt
handle.set_state(LifecycleState::Running);

// Crashes
handle.set_crash_reason(CrashReason::ExitCode(1));

// Restart attempt
handle.set_state(LifecycleState::Restarting);
handle.increment_restart_count();

// Running again
handle.set_state(LifecycleState::Running);

// Status now shows: "Running [restarts: 1]"
```

## Design Decisions

### Why ServiceId in ProcessInfo?

**Rationale**: Services are identified by ServiceId, not PID

**Benefits**:
- Consistent with PandaGen's capability model
- ServiceId survives restarts (PID doesn't)
- Easy to map service → capabilities

**Alternative**: Use TaskId directly
**Problem**: TaskId changes on restart, ServiceId doesn't

### Why Three Kill Signals?

**POSIX has 64 signals**: SIGTERM, SIGKILL, SIGINT, SIGHUP, ...

**PandaGen has 3**:
- `Terminate`: Clean shutdown
- `Kill`: Force kill
- `Interrupt`: User stop

**Rationale**: Most signals are legacy (SIGHUP for modems, etc.)

**Future**: Could add more if needed

### Why Status Summary String?

**Alternative**: Expose all fields separately

**Problem**: Too much work for common case

**Solution**: `status_summary()` combines state + crash + restarts
```rust
"Running"
"Failed (Exit code: 1)"
"Running [restarts: 3]"
"Failed (Panic: out of memory) [restarts: 2]"
```

**Benefits**:
- Single string for display
- Human-readable
- Includes all relevant context

### Why Process List Instead of Iterator?

**Alternative**: Return iterator over processes

**Problem**: Need filtering, sorting, formatting

**Solution**: ProcessList with methods
- `list_all()`: Sorted by name
- `list_by_state()`: Filtered + sorted
- `format_table()`: Pretty output

**Trade-off**: More memory, but easier to use

## Implementation Details

### LifecycleState Display

**Added `as_str()` method**:
```rust
impl LifecycleState {
    pub fn as_str(&self) -> &'static str {
        match self {
            LifecycleState::Starting => "Starting",
            LifecycleState::Running => "Running",
            LifecycleState::Stopping => "Stopping",
            LifecycleState::Stopped => "Stopped",
            LifecycleState::Failed => "Failed",
            LifecycleState::Restarting => "Restarting",
        }
    }
}
```

**Use**: `println!("State: {}", state.as_str());`

### Crash Reason Description

**Formatted descriptions**:
```rust
impl CrashReason {
    pub fn description(&self) -> String {
        match self {
            CrashReason::Panic(msg) => format!("Panic: {}", msg),
            CrashReason::ExitCode(code) => format!("Exit code: {}", code),
            CrashReason::Signal(sig) => format!("Killed by signal: {}", sig),
            CrashReason::ResourceLimit(limit) => format!("Resource limit exceeded: {}", limit),
            CrashReason::Timeout => "Timeout".to_string(),
            CrashReason::Error(err) => format!("Error: {}", err),
        }
    }
}
```

### Process Info Display

**Implements `Display` trait**:
```rust
impl Display for ProcessInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:<36} {:<20} {:<15} {}",
            format!("{}", self.service_id),
            self.name,
            self.state.as_str(),
            self.status
        )
    }
}
```

**Output**: Fixed-width columns for alignment

### Kill Signal Safety

**Graceful vs Forceful**:
```rust
impl KillSignal {
    pub fn is_forceful(&self) -> bool {
        matches!(self, KillSignal::Kill)
    }
}
```

**Usage**: Check before killing
```rust
if signal.is_forceful() {
    // No cleanup, immediate termination
    terminate_immediately(service_id);
} else {
    // Request graceful shutdown
    request_shutdown(service_id);
}
```

## Testing

### Lifecycle Tests (6 new tests)

**CrashReason Tests**:
- `test_crash_reason_description`: Description formatting
- `test_service_handle_crash_reason`: Setting crash reason

**Restart Tests**:
- `test_service_handle_restart_count`: Increment restart counter

**Status Tests**:
- `test_service_handle_status_summary`: Status string generation

**Display Tests**:
- `test_lifecycle_state_as_str`: State to string conversion

### Process Info Tests (7 tests)

**ProcessInfo Tests**:
- `test_process_info_creation`: Creation and fields

**ProcessList Tests**:
- `test_process_list_add_remove`: Add/remove processes
- `test_process_list_by_state`: Filter by state
- `test_process_list_get_by_name`: Lookup by name
- `test_process_list_format_table`: Table formatting

**KillSignal Tests**:
- `test_kill_signal`: Signal names and properties

**KillResult Tests**:
- `test_kill_result_display`: Result formatting

**Coverage**: All public process_info API tested

**Test Strategy**: Unit tests with mock services, deterministic IDs

**Total**: 21/21 tests pass (14 existing + 7 new)

## Comparison with Traditional Systems

| Feature          | Unix (ps/kill)    | PandaGen          |
|------------------|-------------------|-------------------|
| Process ID       | PID (int)         | ServiceId (UUID)  |
| State            | R/S/Z/T           | Starting/Running/Failed/etc. |
| Crash Info       | Exit code only    | Crash reason + message |
| Restart Info     | None (in systemd) | Restart count visible |
| Kill Signals     | 64 signals        | 3 signals (simple) |
| Process Listing  | Complex ps output | Clean table format |

**Philosophy**: Simplify and clarify, not replicate Unix.

## User Experience

### Listing Processes

**Command**: `ps`

**Output**:
```
SERVICE ID                           NAME                 STATE           STATUS
────────────────────────────────────────────────────────────────────────────────────────────────
123...000                            logger               Running         Running
234...111                            storage              Running         Running
345...222                            cli-console          Failed          Failed (Exit code: 1) [restarts: 2]
456...333                            editor               Running         Running [restarts: 1]
```

**What User Sees**:
- Service names (human-readable)
- Current state
- Crash reasons (if failed)
- Restart attempts

### Killing a Process

**Command**: `kill cli-console`

**Output**: `Sent TERM to service cli-console`

**Command**: `kill -9 editor` (force kill)

**Output**: `Sent KILL to service editor`

**Errors**:
- `Service not found` → Service doesn't exist
- `Already stopped` → Can't kill stopped service

### Viewing Crash Reasons

**When service crashes**:
```
Service cli-console crashed: Panic: Out of memory
Restart policy: Always
Restart attempt: 3
```

**User knows**:
- What failed (cli-console)
- Why it failed (out of memory)
- What will happen (restart)
- How many times it's been restarted (3)

## Integration with Existing Phases

### Phase 17 (Process Manager)
- **Base**: Process manager with lifecycle
- **Extended**: Now adds crash tracking and restart counting
- **Compatible**: Existing lifecycle states unchanged

### Phase 77 (Workspace Manager)
- **Integration**: Workspace can show ps output
- **Commands**: `ps` and `kill` commands in CLI
- **Display**: Process list in workspace UI

### Phase 80 (Permissions)
- **Future**: Require capability to kill processes
- **Current**: Kill is unrestricted (system-level)

## Known Limitations

1. **No Real Process Hierarchy**: No parent/child relationships
   - **Future**: Add process tree
   - **Workaround**: Track dependencies manually

2. **No Resource Usage**: ps doesn't show CPU/memory
   - **Future**: Add resource monitoring
   - **Workaround**: Separate monitoring tool

3. **No Real-Time Updates**: ps is snapshot, not live
   - **Future**: Add `top`-like live view
   - **Workaround**: Run ps repeatedly

4. **No Process Groups**: Can't kill multiple processes at once
   - **Future**: Add process groups or tags
   - **Workaround**: Kill one at a time

5. **No Signal Queue**: Signals are immediate, not queued
   - **Future**: Add signal queue for pending signals
   - **Workaround**: Wait between kills

## Performance

**ProcessList Operations**:
- Add: O(1) (HashMap insert)
- Remove: O(1) (HashMap remove)
- Get: O(1) (HashMap lookup)
- List all: O(n log n) (sort by name)
- Filter by state: O(n) (iterate + filter)

**Memory**:
- Per ProcessInfo: ~200 bytes
- 100 processes: ~20 KB
- Negligible overhead

**Crash Tracking**:
- No overhead when no crashes
- Crash reason stored in ServiceHandle (already allocated)

## Philosophy Adherence

✅ **No Legacy Compatibility**: Not POSIX ps/kill, PandaGen-native  
✅ **Testability First**: 7 new deterministic unit tests  
✅ **Modular and Explicit**: Separate process_info module  
✅ **Mechanism over Policy**: ProcessList is mechanism, commands use it  
✅ **Human-Readable**: Clear status messages, not codes  
✅ **Clean, Modern, Testable**: Pure Rust, no unsafe, fast tests  

## The Honest Checkpoint

**After Phase 81, you can:**
- ✅ Run `ps` to see all processes
- ✅ Run `kill <id>` to stop a process
- ✅ See crash reasons ("Panic: out of memory")
- ✅ See restart attempts ("Running [restarts: 3]")
- ✅ Understand process state at a glance
- ✅ Feel like managing a real system

**This is the moment PandaGen stops being invisible and becomes manageable.**

## Future Enhancements

### Process Tree
- Show parent/child relationships
- `ps tree` command
- Visualize service dependencies

### Resource Monitoring
- CPU usage per process
- Memory usage per process
- I/O statistics
- Network usage

### Live Process Viewer (top)
- Real-time process list
- Sortable columns
- Auto-refresh
- Keyboard shortcuts

### Process Groups
- Tag processes
- Kill by tag
- List by tag

### Signal Queue
- Queue signals for busy processes
- Retry failed signals
- Signal history/log

### Crash Dumps
- Save crash context
- Stack traces
- Memory dumps
- Debug symbols

## Conclusion

Phase 81 makes process isolation visible through `ps`, `kill`, crash reasons, and restart tracking. Users can now see and manage processes like a real operating system.

**Key Achievements**:
- ✅ `ps` command (ProcessList with table formatting)
- ✅ `kill` command (3 signal types)
- ✅ Crash reason tracking (CrashReason enum)
- ✅ Restart count tracking
- ✅ Status summaries
- ✅ 7 passing tests (21 total)

**Test Results**: 21/21 tests pass (14 existing + 7 new)

**Phases 69-81 Complete**: Process isolation is now visible and manageable.

**Next**: Phase 82 adds text selection and clipboard, Phase 83 implements boot profiles.

**Mission accomplished.**
