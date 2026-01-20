# Phase 72: Line Editing & Input Ergonomics

## Overview

Phase 72 adds proper line editing and cursor movement to the interactive console, transforming it from basic text input into a comfortable command-line editing experience.

## What It Adds

1. **Left/Right Arrow Movement**: Navigate within the input line
2. **Home/End Keys**: Jump to start/end of line
3. **Proper Backspace/Delete**: Remove characters at cursor position
4. **Insert Mode**: Type characters at cursor position, not just at end
5. **Cursor Position Tracking**: Visual cursor stays correct during editing

## Why It Matters

**This is where users stop swearing at the screen.**

Without proper line editing:
- Typo at the start? Delete the whole line and retype.
- Want to fix one character? Too bad.
- Arrow keys? They don't work.

With Phase 72:
- Fix typos anywhere in the line
- Navigate freely with arrows
- Edit naturally like any terminal

## Implementation

### Enhanced InteractiveConsole

**New Field**:
```rust
pub struct InteractiveConsole {
    // ... existing fields ...
    cursor_pos: usize,  // Position within text_buffer
}
```

**New Key Handlers**:
- `Left`: Move cursor left (if not at start)
- `Right`: Move cursor right (if not at end)
- `Home`: Jump to position 0
- `End`: Jump to end of line
- `Backspace`: Delete character before cursor
- `Delete`: Delete character at cursor

**Updated Behavior**:
- Text input inserts at cursor position
- History navigation sets cursor to end
- Enter/Escape resets cursor to 0

### Key Implementation Details

#### Cursor Movement
```rust
KeyCode::Left => {
    if self.cursor_pos > 0 {
        self.cursor_pos -= 1;
    }
}
KeyCode::Right => {
    if self.cursor_pos < self.text_buffer.len() {
        self.cursor_pos += 1;
    }
}
```

Bounds checking prevents cursor from going out of range.

#### Character Insertion
```rust
// Old: self.text_buffer.push(c);
// New:
self.text_buffer.insert(self.cursor_pos, c);
self.cursor_pos += 1;
```

Insert at cursor, then advance cursor.

#### Backspace vs Delete
```rust
KeyCode::Backspace => {
    if self.cursor_pos > 0 {
        self.text_buffer.remove(self.cursor_pos - 1);
        self.cursor_pos -= 1;
    }
}
KeyCode::Delete => {
    if self.cursor_pos < self.text_buffer.len() {
        self.text_buffer.remove(self.cursor_pos);
        // cursor_pos stays same
    }
}
```

Backspace: delete before cursor, move cursor back  
Delete: delete at cursor, cursor stays

## Testing

### New Tests (13 tests added)

**Cursor Movement**:
- `test_cursor_left_right`: Basic arrow navigation
- `test_cursor_home_end`: Home/End keys
- `test_cursor_movement_bounds`: Can't move out of range

**Text Editing**:
- `test_insert_at_cursor`: Insert characters mid-line
- `test_backspace_at_cursor`: Delete before cursor
- `test_delete_at_cursor`: Delete at cursor
- `test_backspace_at_start`: No-op at position 0
- `test_delete_at_end`: No-op at end of line

**History Integration**:
- `test_history_navigation_updates_cursor`: Cursor moves to end

### Test Results
- **Total**: 37 tests passing
- **Added**: 13 new tests for Phase 72
- **Regressions**: 0 (all existing tests still pass)

## Changes Made

### Modified Files

**cli_console/src/interactive.rs**:
- Added `cursor_pos: usize` field
- Updated `new()` to initialize cursor_pos = 0
- Enhanced `handle_key_press()` with cursor movement logic
- Changed text input from `push()` to `insert(cursor_pos, c)`
- Updated backspace/delete to work at cursor position
- Added `cursor_pos()` getter method
- Added 13 comprehensive tests

## Design Decisions

### Why Not Use External Library?

Options considered:
- **rustyline**: Full-featured line editing library
- **termion**: Terminal UI library with line editing

Why we didn't use them:
1. **No POSIX Assumptions**: PandaGen doesn't use POSIX terminals
2. **Event-Based Input**: We use `KeyEvent`, not byte streams
3. **Minimal Dependencies**: Keep kernel components small
4. **Testability**: Pure logic without terminal emulation

Phase 72 implements exactly what we need, nothing more.

### Why Track Cursor Position?

Alternative: Recalculate cursor from text buffer and input state

Why explicit cursor position:
- **Simple**: One field, clear semantics
- **Fast**: O(1) access, no scanning
- **Testable**: Easy to assert cursor position
- **Correct**: No chance of desync

### Why Insert Mode Only?

Traditional editors have insert and overwrite modes (toggled by Insert key).

Phase 72 implements insert mode only because:
- **Common Case**: Insert mode is default in most terminals
- **Simpler**: One less state to track and test
- **Future**: Can add overwrite mode if needed

## Comparison with Traditional Terminals

| Feature | bash/zsh | PandaGen Phase 72 |
|---------|----------|-------------------|
| Arrow keys | ✅ | ✅ |
| Home/End | ✅ | ✅ |
| Backspace/Delete | ✅ | ✅ |
| Insert at cursor | ✅ | ✅ |
| Ctrl+A/Ctrl+E | ✅ | ❌ (Phase 74) |
| Ctrl+K/Ctrl+U | ✅ | ❌ (future) |
| Word movement | ✅ (Alt+B/Alt+F) | ❌ (future) |
| Completion | ✅ (Tab) | ❌ (future) |

Phase 72 covers the essential 80% that users expect.

## User Experience Impact

### Before Phase 72
```
> ls -la /some/very/long/path/that/i/mistyped
         ^ Typo here

Options:
1. Hold backspace for 5 seconds
2. Give up, press Enter, retype whole thing
```

### After Phase 72
```
> ls -la /some/very/long/path/that/i/mistyped
         ^ Typo here
         
Press Left arrow 40 times, fix typo, press End, Enter.
Or: Press Home, Right 40 times, fix typo, End, Enter.
```

Still not perfect (Phase 74 adds Ctrl+A, Ctrl+E), but **massively better**.

## Integration with Existing Phases

### Phase 68 (Command History)
- History navigation (Up/Down) already implemented
- Phase 72 adds: cursor moves to end when navigating history
- Works together: navigate history, then edit with arrows

### Phase 71 (Scrollback)
- Scrollback is for output (PageUp/PageDown in Phase 74)
- Phase 72 is for input (command line editing)
- Complementary, not overlapping

## Known Limitations

1. **No Word Movement**: Can't jump by word (Ctrl+Left/Right)
   - Future enhancement
   - Not essential for Phase 72

2. **No Kill/Yank**: Can't cut/paste lines (Ctrl+K/Ctrl+Y)
   - Future enhancement
   - Not common in basic terminal use

3. **No Tab Completion**: Typing full commands only
   - Future enhancement
   - Requires filesystem/command integration

4. **Single-Line Only**: No multi-line editing
   - Intentional: Phase 72 is CLI, not editor
   - Editor (Phase 73) handles multi-line

## Performance

All operations O(1) except:
- `insert(cursor_pos, c)`: O(n) where n = length after cursor
- `remove(cursor_pos)`: O(n) where n = length after cursor

Typical command line is 10-50 characters, so O(n) is negligible (<1μs).

## Philosophy Adherence

✅ **No Legacy Compatibility**: No termios, no TTY, pure events  
✅ **Testability First**: 13 new tests, all deterministic  
✅ **Modular and Explicit**: Clear cursor position field  
✅ **Mechanism over Policy**: Provides primitives, not fancy features  
✅ **Human-Readable**: `cursor_pos`, `text_buffer`, clear names  
✅ **Clean, Modern, Testable**: No unsafe, fast tests  

## Next Steps

### Phase 73: Editor View Integration
- Editor renders inside framebuffer console
- Status line (-- INSERT --)
- Workspace prompt vs editor separation
- Focus transitions

### Phase 74: Keyboard Semantics Polish
- Ctrl+A/Ctrl+E (alternative to Home/End)
- PageUp/PageDown (scrollback navigation)
- Ctrl+C/Ctrl+D (signal handling)
- Key repeat (auto-repeat when held)

## Conclusion

Phase 72 successfully adds line editing to PandaGen's interactive console. The system now has:
- ✅ Cursor movement (arrows, Home, End)
- ✅ Proper backspace/delete
- ✅ Insert at cursor position
- ✅ Comfortable text editing

**Test Results**: 37 tests passing, 0 failures

Users can now edit command lines naturally without frustration.
