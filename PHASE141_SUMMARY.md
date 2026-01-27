# Phase 141: CLI Component Implementation for Bare-Metal Workspace

## Overview
This phase implements a minimal but functional CLI component for the bare-metal workspace in `kernel_bootstrap`. Previously, the "Open CLI" action only showed a placeholder message. Now it provides a real interactive command-line interface suitable for bare-metal runs in QEMU.

## Motivation
The bare-metal kernel needed an interactive CLI mode that:
- Works without heavy service dependencies
- Reuses existing workspace command infrastructure
- Provides input line editing capabilities
- Maintains consistency with VGA and framebuffer rendering
- Follows the project's testability-first philosophy

## Changes Made

### 1. CLI State Management
Added CLI-specific state fields to `WorkspaceSession`:
```rust
cli_active: bool,          // CLI mode flag
cli_buffer: [u8; 64],      // CLI input buffer (COMMAND_MAX)
cli_len: usize,            // Buffer length
cli_cursor: usize,         // Cursor position
```

### 2. Helper Methods
- `set_cli_active(bool, serial)` - Activate/deactivate CLI mode with user notification
- `reset_cli_buffer()` - Clear CLI input buffer and cursor
- `is_cli_active()` - Public API to check CLI state

### 3. Command Execution Refactoring
Extracted command execution logic into a shared function:
```rust
fn run_command_line(&mut self, command: &str, ctx, serial)
```

This enables both the normal workspace prompt and CLI to execute commands through the same path, reducing code duplication and ensuring consistent behavior.

### 4. CLI Input Handling
Added dedicated CLI input processing in `process_input()`:
- **Printable characters**: Insert at cursor position with proper buffer shifting
- **Backspace**: Remove character before cursor
- **Enter**: Execute command and reset buffer
- **Escape**: Exit CLI mode and return to normal prompt
- **Special exit commands**: "exit" and "quit" also return to normal mode when in CLI

### 5. Rendering Updates
Updated prompt and cursor handling:
- `show_prompt()`: Displays "$ " for CLI mode, "> " for normal mode
- `get_command_text()`: Returns CLI buffer when active, command buffer otherwise
- `get_cursor_col()`: Tracks CLI cursor position separately from command buffer
- `emit_command_line()`: Uses correct prompt prefix based on mode

### 6. Command Palette Integration
- "Open CLI" action now properly activates CLI mode
- "open cli" command also activates CLI mode
- CLI displays entry message: "CLI mode: type commands, `exit` to leave"
- Clean exit restores normal prompt automatically

## Testing
Added comprehensive unit tests:
- `test_cli_state_initialization` - Verifies initial state
- `test_cli_buffer_management` - Tests buffer reset functionality
- `test_cli_prompt_display` - Validates prompt rendering
- `test_get_command_text_cli_vs_normal` - Ensures correct buffer selection
- `test_get_cursor_col_cli_vs_normal` - Verifies cursor position calculation

All tests pass:
```
test result: ok. 10 passed; 0 failed
```

No regressions in existing workspace, editor, or palette tests.

## Usage

### Activating CLI
1. From normal prompt: Type `open cli`
2. From command palette: Ctrl+P → type "cli" → Enter

### Using CLI
- Type commands and press Enter to execute
- Use backspace to edit input
- All workspace commands work: `help`, `ls`, `cat`, `mem`, `boot`, etc.

### Exiting CLI
- Press Escape key
- Type `exit` or `quit`

## Design Decisions

### Input Buffer Separation
CLI maintains a separate buffer from the normal command buffer to:
- Support independent cursor tracking for future enhancements
- Allow mode switching without losing state
- Enable different input processing rules if needed

### Shared Command Execution
Refactoring command execution into `run_command_line()`:
- Eliminates code duplication
- Ensures consistent command behavior
- Makes future command additions easier
- Improves testability

### Exit Command Handling
Special handling of "exit"/"quit" in CLI mode:
- These commands close components in normal mode
- In CLI mode, they exit the CLI itself
- Provides intuitive UX consistent with shell conventions

### Cursor Position Tracking
CLI tracks cursor separately to support:
- Future left/right arrow navigation
- Insert mode editing (already implemented)
- Command history navigation (future enhancement)

## Future Enhancements (Out of Scope)
- Command history (up/down arrow)
- Left/right arrow cursor movement (stub exists)
- Tab completion
- Advanced line editing (Ctrl+A, Ctrl+E, etc.)
- Integration with `services_workspace_manager` for networked shells
- Pipes and command chaining
- Scripting support

## Files Modified
- `kernel_bootstrap/src/workspace.rs` - Core implementation (147 insertions, 22 deletions)

## Alignment with Project Philosophy
✅ **Testability first**: All logic has deterministic unit tests  
✅ **Modular and explicit**: CLI state is explicit, not ambient  
✅ **Clean, modern code**: Readable implementation with clear intent  
✅ **No legacy POSIX assumptions**: Pure state machine, no fork/exec  
✅ **Mechanism over policy**: CLI provides primitives, commands define policy

## Verification Checklist
- [x] Code compiles without errors
- [x] All unit tests pass (10/10 workspace tests)
- [x] No regressions in editor tests
- [x] No regressions in palette tests
- [x] CLI activates from palette
- [x] CLI activates from command
- [x] CLI executes commands correctly
- [x] CLI exit paths work (Escape, exit, quit)
- [x] Prompt rendering distinguishes CLI mode
- [x] Documentation added (this file)
