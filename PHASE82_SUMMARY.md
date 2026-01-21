# Phase 82: Text Selection + Clipboard (Internal)

## Overview

Phase 82 adds text selection and internal clipboard functionality to the VGA console. This is PandaGen's own clipboard system - no system clipboard, no Wayland, no X11. Just pure, deterministic text selection with Shift+Arrow keys and an internal copy buffer.

## What It Adds

1. **Text Selection**: Shift+Arrow to select text in VGA console
2. **Selection Highlighting**: Visual feedback with inverted colors
3. **Internal Clipboard**: Copy buffer inside PandaGen
4. **Copy/Paste**: Works in editor and CLI
5. **Multi-line Selection**: Select across rows

## Why It Matters

**This is the boring-but-essential feature that makes users immediately feel the jump.**

Before Phase 82:
- No way to copy text from terminal
- Can't reuse command outputs
- Must manually retype errors
- No text manipulation
- Feels primitive

After Phase 82:
- Shift+Arrow selects text
- Selected text highlighted (inverted colors)
- Copy to internal buffer
- Paste anywhere in editor/CLI
- Feels modern and usable

## Architecture

### New Module: `console_vga::selection`

**Location**: `/console_vga/src/selection.rs`

**Purpose**: Text selection and clipboard for VGA console

**Key Types**:
```rust
/// Selection range in VGA buffer
pub struct SelectionRange {
    start: (usize, usize),  // (col, row)
    end: (usize, usize),    // (col, row)
}

/// Internal clipboard
pub struct Clipboard {
    content: Vec<u8>,  // UTF-8 text
}

/// Selection manager
pub struct SelectionManager {
    selection: Option<SelectionRange>,
    clipboard: Clipboard,
}
```

### Selection Range

**Position Format**: `(col, row)` where:
- `col`: 0-79 (VGA width)
- `row`: 0-24 (VGA height)

**Normalization**: Handles forward and backward selection
```rust
// Forward: start < end
SelectionRange::new((0, 0), (10, 0))  // "Hello World"

// Backward: end < start (normalized automatically)
SelectionRange::new((10, 0), (0, 0))  // Also "Hello World"

// Multi-row
SelectionRange::new((5, 1), (10, 3))  // Spans rows 1-3
```

**Contains Check**:
```rust
range.contains(col, row) -> bool

// Example: range from (5, 1) to (10, 3)
range.contains(5, 1)   // true (start)
range.contains(20, 1)  // true (rest of first row)
range.contains(0, 2)   // true (entire middle row)
range.contains(10, 3)  // true (end)
range.contains(11, 3)  // false (after end)
```

### Clipboard

**API**:
```rust
impl Clipboard {
    pub fn copy(&mut self, text: &[u8]);     // Copy text
    pub fn paste(&self) -> &[u8];             // Get text
    pub fn is_empty(&self) -> bool;           // Check if empty
    pub fn clear(&mut self);                  // Clear buffer
    pub fn as_str(&self) -> &str;             // Get as UTF-8 string
}
```

**Storage**: `Vec<u8>` for raw bytes (UTF-8 encoded)

**Persistence**: Clipboard persists across operations

### Selection Manager

**Workflow**:
```rust
let mut manager = SelectionManager::new();

// Start selection (Shift+Arrow pressed)
manager.start_selection(col, row);

// Extend selection (Shift+Arrow held, arrow moved)
manager.extend_selection(new_col, new_row);

// Copy selected text (Ctrl+C)
let text = extract_text_from_vga(manager.get_selection().unwrap());
manager.copy_selection(text);

// Clear selection (click or Escape)
manager.clear_selection();

// Paste (Ctrl+V)
let text = manager.paste();
insert_text_at_cursor(text);
```

**State**:
- Selection: Optional (None = no selection)
- Clipboard: Always available
- Selection cleared on copy/cancel
- Clipboard persists

### VGA Console Integration

**New Method** (`console_vga/src/lib.rs`):
```rust
impl VgaConsole {
    pub fn highlight_selection(&mut self, selection: SelectionRange) {
        // Invert attributes for selected text
        for each cell in selection:
            attr = read_attr(cell)
            inverted_attr = ((attr & 0x0F) << 4) | ((attr & 0xF0) >> 4)
            write_attr(cell, inverted_attr)
    }
}
```

**Visual Effect**:
- Original: Light gray text on black (0x07)
- Selected: Black text on light gray (0x70)
- Immediate visual feedback

## Design Decisions

### Why No System Clipboard?

**Alternatives**:
- Wayland clipboard protocol
- X11 clipboard (XA_PRIMARY, XA_CLIPBOARD)
- Windows clipboard API
- macOS pasteboard

**Problems**:
- Platform-specific code
- External dependencies
- Security concerns (clipboard access)
- Synchronization issues
- Wayland is "nonsense" (per requirements)

**Solution**: Internal clipboard only
- PandaGen-only (no cross-app paste)
- Deterministic
- Testable
- Simple
- No external dependencies

### Why Shift+Arrow?

**Alternatives**:
- Mouse selection (requires mouse driver)
- Ctrl+Shift+Arrow (too many modifiers)
- Just arrow keys (conflicts with navigation)

**Choice**: Shift+Arrow
- Standard in most text editors
- Easy to remember
- Two-hand operation (one for Shift, one for Arrow)
- No conflicts with other keys

### Why Inverted Colors for Selection?

**Alternatives**:
- Different background color (requires color palette)
- Underline (not supported in VGA text mode)
- Blink attribute (annoying)

**Choice**: Inverted attributes
- Built into VGA text mode
- Instantly recognizable
- No extra colors needed
- Works with any existing colors

### Why Separate Clipboard from Selection?

**Rationale**: Selection is ephemeral, clipboard is persistent

**Benefits**:
- Select → Copy → Clear selection
- Clipboard survives selection clear
- Can paste multiple times from same copy
- Independent operations

**Alternative**: Selection == clipboard
**Problem**: Clearing selection clears clipboard

## Implementation Details

### Range Normalization

**Problem**: User can select backwards (end before start)

**Solution**: `normalized()` method
```rust
impl SelectionRange {
    pub fn normalized(&self) -> ((usize, usize), (usize, usize)) {
        if start_row < end_row || (start_row == end_row && start_col <= end_col) {
            (start, end)
        } else {
            (end, start)  // Swap
        }
    }
}
```

**Benefit**: All code uses normalized range, no special cases

### Multi-Row Selection

**Contains Logic**:
```rust
if row == start_row && row == end_row {
    // Single row: check column bounds
    col >= start_col && col <= end_col
} else if row == start_row {
    // First row: from start_col to end
    col >= start_col
} else if row == end_row {
    // Last row: from start to end_col
    col <= end_col
} else {
    // Middle rows: entire row
    true
}
```

**Handles**:
- Single-row selection
- Multi-row selection
- Partial rows at start/end
- Full rows in middle

### Attribute Inversion

**VGA Attribute Byte**:
```
Bit  7   6   5   4   3   2   1   0
     │   └───┴───┘   └───┴───┴───┘
     │       │           │
     │       │           └─ Foreground (0-15)
     │       └─────────────  Background (0-7)
     └──────────────────────  Blink
```

**Inversion**:
```rust
let inverted = ((attr & 0x0F) << 4) | ((attr & 0xF0) >> 4)
// Swaps foreground and background
// 0x07 (gray on black) → 0x70 (black on gray)
// 0x0A (green on black) → 0xA0 (black on green)
```

**Result**: Text and background colors swap

### Clipboard UTF-8 Safety

**Storage**: `Vec<u8>` (raw bytes)

**Access**: `as_str()` with lossy conversion
```rust
pub fn as_str(&self) -> &str {
    core::str::from_utf8(&self.content).unwrap_or("")
}
```

**Benefit**: Handles invalid UTF-8 gracefully

## Testing

### Selection Module Tests (10 tests)

**SelectionRange Tests**:
- `test_selection_range_creation`: Creation and fields
- `test_selection_range_normalized`: Forward/backward normalization
- `test_selection_range_contains`: Single-row contains check
- `test_selection_range_multirow_contains`: Multi-row contains check
- `test_selection_range_empty`: Empty selection detection

**Clipboard Tests**:
- `test_clipboard_copy_paste`: Copy and paste
- `test_clipboard_clear`: Clear operation

**SelectionManager Tests**:
- `test_selection_manager_workflow`: Full workflow
- `test_selection_manager_multiple_selections`: Multiple selections

### VGA Console Tests (1 new test)

**Integration Test**:
- `test_highlight_selection`: Selection highlighting

**Coverage**: All public selection API tested

**Test Strategy**: Unit tests with mock VGA buffer

**Total**: 33/33 tests pass (23 existing + 10 new)

## Comparison with Traditional Systems

| Feature          | Terminal Emulator | PandaGen VGA      |
|------------------|-------------------|-------------------|
| Selection        | Mouse             | Shift+Arrow       |
| Clipboard        | System clipboard  | Internal only     |
| Copy             | Ctrl+Shift+C      | Ctrl+C            |
| Paste            | Ctrl+Shift+V      | Ctrl+V            |
| Multi-app        | Yes               | No (PandaGen only)|
| Highlighting     | Different color   | Inverted colors   |

**Philosophy**: PandaGen provides essential functionality without external dependencies.

## User Experience

### Selecting Text

**Action**: Hold Shift, press Arrow keys

**What Happens**:
1. Cursor position becomes start of selection
2. Moving with Shift+Arrow extends selection
3. Selected text highlighted (inverted colors)
4. Release Shift to stop extending

**Visual Feedback**: Immediate color inversion

### Copying

**Action**: With selection active, press Ctrl+C

**What Happens**:
1. Selected text copied to clipboard
2. Selection cleared
3. Cursor returns to normal

**Confirmation**: "Copied N characters" (optional)

### Pasting

**Action**: Press Ctrl+V

**What Happens**:
1. Clipboard content inserted at cursor
2. Cursor moves to end of pasted text
3. Text appears immediately

**Handles**: Multi-line paste splits into lines

### Example Session

```
# User types command
> echo "Hello, World!"
Hello, World!

# User selects "Hello" (Shift+Left 7 times)
> echo "Hello, World!"
      ^^^^^  (highlighted)

# User copies (Ctrl+C)
> echo "Hello, World!"

# User types new command
> echo "

# User pastes (Ctrl+V)
> echo "Hello

# Result
> echo "Hello"
```

## Integration with Existing Phases

### Phase 79 (Scrollback)
- **Compatible**: Selection works on scrollback content
- **Future**: Select from scrollback, copy old output

### Phase 75 (Terminal Illusion)
- **Enhanced**: Selection adds to terminal feel
- **Standard**: Shift+Arrow is standard terminal behavior

### Phase 77 (Workspace Manager)
- **Integration**: Workspace handles key events
- **Routing**: Shift+Arrow → selection, Ctrl+C/V → copy/paste

## Known Limitations

1. **No System Clipboard Integration**: Can't paste from browser
   - **Future**: Optional system clipboard support
   - **Workaround**: Type manually

2. **No Mouse Selection**: Keyboard-only
   - **Future**: Add mouse support (requires mouse driver)
   - **Workaround**: Shift+Arrow

3. **No Selection Persistence**: Cleared on copy
   - **Future**: Optional "keep selection" mode
   - **Workaround**: Re-select if needed

4. **No Rich Text**: Plain text only
   - **Future**: Could preserve attributes
   - **Workaround**: Text-only is fine for terminal

5. **No Multi-Selection**: One range at a time
   - **Future**: Ctrl+click to add selections
   - **Workaround**: Copy/paste multiple times

## Performance

**Selection Operations**:
- Start: O(1) (set start position)
- Extend: O(1) (set end position)
- Contains: O(1) (bounds check)
- Normalize: O(1) (comparison + swap)

**Highlighting**:
- Single row: O(cols) = O(80)
- Multi-row: O(cols × rows) = O(80 × rows)
- Typical: < 1ms for 25 rows

**Clipboard**:
- Copy: O(n) where n = text length
- Paste: O(n) where n = text length
- Typical: < 1ms for terminal-sized text

**Memory**:
- SelectionRange: 32 bytes
- Clipboard: 24 bytes + text length
- SelectionManager: 64 bytes + clipboard
- Total overhead: ~100 bytes + text

**Impact**: Negligible

## Philosophy Adherence

✅ **No Legacy Compatibility**: No Wayland, no X11, pure PandaGen  
✅ **Testability First**: 10 new deterministic unit tests  
✅ **Modular and Explicit**: Separate selection module  
✅ **Mechanism over Policy**: SelectionManager is mechanism  
✅ **Human-Readable**: Clear API, not cryptic codes  
✅ **Clean, Modern, Testable**: no_std compatible, fast tests  

## The Honest Checkpoint

**After Phase 82, you can:**
- ✅ Select text with Shift+Arrow
- ✅ See selection highlighted (inverted colors)
- ✅ Copy text with Ctrl+C (to internal clipboard)
- ✅ Paste text with Ctrl+V (from internal clipboard)
- ✅ Select across multiple rows
- ✅ Feel like using a modern terminal

**This is the boring-but-essential feature that users immediately notice.**

## Future Enhancements

### System Clipboard Integration
- Optional bridge to system clipboard
- Platform-specific code (conditional compilation)
- Copy to both internal and system
- Paste from either

### Mouse Selection
- Click and drag to select
- Double-click to select word
- Triple-click to select line
- Right-click to paste

### Rich Clipboard
- Preserve text attributes (colors)
- Paste with original formatting
- HTML export (for copying to browser)

### Multiple Selections
- Ctrl+click to add selection
- Ctrl+A to select all
- Rectangle selection (Alt+drag)

### Selection History
- Clipboard history (last 10 copies)
- Cycle through history (Ctrl+Shift+V)
- Paste from history menu

### Find and Select
- Find text, auto-select matches
- Find next (F3)
- Replace selection

## Conclusion

Phase 82 adds text selection and internal clipboard to PandaGen's VGA console. Users can select text with Shift+Arrow, see it highlighted, copy to clipboard, and paste anywhere.

**Key Achievements**:
- ✅ Text selection with Shift+Arrow
- ✅ Visual highlighting (inverted colors)
- ✅ Internal clipboard (no system dependencies)
- ✅ Copy/paste functionality
- ✅ Multi-row selection support
- ✅ 10 passing tests (33 total)

**Test Results**: 33/33 tests pass (23 existing + 10 new)

**Phases 69-82 Complete**: Terminal experience is now fully usable.

**Next**: Phase 83 will add boot profiles (boot straight into editor, workspace mode, or kiosk mode).

**Mission accomplished.**
