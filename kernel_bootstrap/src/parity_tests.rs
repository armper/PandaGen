//! Parity tests that validate adapter correctness using editor_core snapshots
//!
//! These tests ensure that the minimal_editor adapter produces the same
//! core editor states as expected from direct editor_core usage.
//!
//! ## Philosophy
//!
//! - Validate that key translation + adapter produce correct core states
//! - Focus on adapter correctness, not core logic (already tested in editor_core)
//! - Use direct state comparison (mode, cursor, buffer) rather than hashing
//!   (snapshot hashing is cfg(test) only in editor_core)

#[cfg(test)]
mod parity_tests {
    use crate::minimal_editor::MinimalEditor;
    use editor_core::{EditorCore, Key};

    /// Trace 1: Simple insert and quit
    /// Input: i, "test", Escape
    #[test]
    fn test_parity_trace_insert_text() {
        // Reference: Run trace directly on EditorCore
        let mut core = EditorCore::new();
        core.apply_key(Key::from_ascii(b'i').unwrap()); // Enter insert
        core.apply_key(Key::from_ascii(b't').unwrap());
        core.apply_key(Key::from_ascii(b'e').unwrap());
        core.apply_key(Key::from_ascii(b's').unwrap());
        core.apply_key(Key::from_ascii(b't').unwrap());
        core.apply_key(Key::from_ascii(0x1B).unwrap()); // Escape

        // Adapter: Run same trace through MinimalEditor
        let mut adapter = MinimalEditor::new(24);
        adapter.process_byte(b'i');
        adapter.process_byte(b't');
        adapter.process_byte(b'e');
        adapter.process_byte(b's');
        adapter.process_byte(b't');
        adapter.process_byte(0x1B);

        // Both should produce same core state
        assert_eq!(adapter.mode(), core.mode(), "Mode mismatch");
        assert_eq!(adapter.cursor(), core.cursor(), "Cursor mismatch");
        assert_eq!(
            adapter.get_viewport_line(0).unwrap(),
            core.buffer().line(0).unwrap(),
            "Buffer content mismatch"
        );
    }

    /// Trace 2: Multiline edit
    /// Input: i, "line1", Enter, "line2", Escape
    #[test]
    fn test_parity_trace_multiline() {
        // Reference
        let mut core = EditorCore::new();
        core.apply_key(Key::from_ascii(b'i').unwrap());
        core.apply_key(Key::from_ascii(b'l').unwrap());
        core.apply_key(Key::from_ascii(b'i').unwrap());
        core.apply_key(Key::from_ascii(b'n').unwrap());
        core.apply_key(Key::from_ascii(b'e').unwrap());
        core.apply_key(Key::from_ascii(b'1').unwrap());
        core.apply_key(Key::from_ascii(b'\n').unwrap());
        core.apply_key(Key::from_ascii(b'l').unwrap());
        core.apply_key(Key::from_ascii(b'i').unwrap());
        core.apply_key(Key::from_ascii(b'n').unwrap());
        core.apply_key(Key::from_ascii(b'e').unwrap());
        core.apply_key(Key::from_ascii(b'2').unwrap());
        core.apply_key(Key::from_ascii(0x1B).unwrap());

        // Adapter
        let mut adapter = MinimalEditor::new(24);
        adapter.process_byte(b'i');
        adapter.process_byte(b'l');
        adapter.process_byte(b'i');
        adapter.process_byte(b'n');
        adapter.process_byte(b'e');
        adapter.process_byte(b'1');
        adapter.process_byte(b'\n');
        adapter.process_byte(b'l');
        adapter.process_byte(b'i');
        adapter.process_byte(b'n');
        adapter.process_byte(b'e');
        adapter.process_byte(b'2');
        adapter.process_byte(0x1B);

        // Validate parity
        assert_eq!(adapter.mode(), core.mode());
        assert_eq!(adapter.cursor(), core.cursor());
        assert_eq!(adapter.get_viewport_line(0).unwrap(), core.buffer().line(0).unwrap());
        assert_eq!(adapter.get_viewport_line(1).unwrap(), core.buffer().line(1).unwrap());
    }

    /// Trace 3: Movement in normal mode
    /// Input: i, "abc", Escape, h, h (move left twice)
    #[test]
    fn test_parity_trace_movement() {
        // Reference
        let mut core = EditorCore::new();
        core.apply_key(Key::from_ascii(b'i').unwrap());
        core.apply_key(Key::from_ascii(b'a').unwrap());
        core.apply_key(Key::from_ascii(b'b').unwrap());
        core.apply_key(Key::from_ascii(b'c').unwrap());
        core.apply_key(Key::from_ascii(0x1B).unwrap());
        core.apply_key(Key::from_ascii(b'h').unwrap());
        core.apply_key(Key::from_ascii(b'h').unwrap());

        // Adapter
        let mut adapter = MinimalEditor::new(24);
        adapter.process_byte(b'i');
        adapter.process_byte(b'a');
        adapter.process_byte(b'b');
        adapter.process_byte(b'c');
        adapter.process_byte(0x1B);
        adapter.process_byte(b'h');
        adapter.process_byte(b'h');

        // Validate
        assert_eq!(adapter.mode(), core.mode());
        assert_eq!(adapter.cursor(), core.cursor());
        assert_eq!(adapter.get_viewport_line(0).unwrap(), core.buffer().line(0).unwrap());
    }

    /// Trace 4: Delete character with x
    /// Input: i, "test", Escape, 0 (move to start), x (delete)
    #[test]
    fn test_parity_trace_delete() {
        // Reference
        let mut core = EditorCore::new();
        core.apply_key(Key::from_ascii(b'i').unwrap());
        core.apply_key(Key::from_ascii(b't').unwrap());
        core.apply_key(Key::from_ascii(b'e').unwrap());
        core.apply_key(Key::from_ascii(b's').unwrap());
        core.apply_key(Key::from_ascii(b't').unwrap());
        core.apply_key(Key::from_ascii(0x1B).unwrap());
        core.apply_key(Key::from_ascii(b'0').unwrap());
        core.apply_key(Key::from_ascii(b'x').unwrap());

        // Adapter
        let mut adapter = MinimalEditor::new(24);
        adapter.process_byte(b'i');
        adapter.process_byte(b't');
        adapter.process_byte(b'e');
        adapter.process_byte(b's');
        adapter.process_byte(b't');
        adapter.process_byte(0x1B);
        adapter.process_byte(b'0');
        adapter.process_byte(b'x');

        // Validate
        assert_eq!(adapter.mode(), core.mode());
        assert_eq!(adapter.cursor(), core.cursor());
        assert_eq!(adapter.get_viewport_line(0).unwrap(), core.buffer().line(0).unwrap());
    }

    /// Trace 5: Backspace in insert mode
    /// Input: i, "abc", Backspace, Escape
    #[test]
    fn test_parity_trace_backspace() {
        // Reference
        let mut core = EditorCore::new();
        core.apply_key(Key::from_ascii(b'i').unwrap());
        core.apply_key(Key::from_ascii(b'a').unwrap());
        core.apply_key(Key::from_ascii(b'b').unwrap());
        core.apply_key(Key::from_ascii(b'c').unwrap());
        core.apply_key(Key::from_ascii(0x08).unwrap()); // Backspace
        core.apply_key(Key::from_ascii(0x1B).unwrap());

        // Adapter
        let mut adapter = MinimalEditor::new(24);
        adapter.process_byte(b'i');
        adapter.process_byte(b'a');
        adapter.process_byte(b'b');
        adapter.process_byte(b'c');
        adapter.process_byte(0x08);
        adapter.process_byte(0x1B);

        // Validate
        assert_eq!(adapter.mode(), core.mode());
        assert_eq!(adapter.cursor(), core.cursor());
        assert_eq!(adapter.get_viewport_line(0).unwrap(), core.buffer().line(0).unwrap());
    }

    /// Test that status line reflects mode correctly
    #[test]
    fn test_status_line_mode_display() {
        let mut adapter = MinimalEditor::new(24);

        // Normal mode
        assert!(adapter.status_line().contains("NORMAL"));

        // Insert mode
        adapter.process_byte(b'i');
        assert!(adapter.status_line().contains("INSERT"));

        // Back to normal
        adapter.process_byte(0x1B);
        assert!(adapter.status_line().contains("NORMAL"));

        // Command mode - status line shows command buffer content
        adapter.process_byte(b':');
        let status = adapter.status_line();
        // In command mode, status shows either COMMAND or the command buffer starting with ':'
        assert!(status.contains("COMMAND") || status.starts_with(':'), 
            "Expected COMMAND mode indicator or command buffer, got: {}", status);
    }

    /// Test that dirty flag is reflected in adapter
    #[test]
    fn test_dirty_flag_parity() {
        let mut core = EditorCore::new();
        let mut adapter = MinimalEditor::new(24);

        // Both should start clean
        assert_eq!(adapter.is_dirty(), core.dirty());

        // Entering insert mode alone doesn't make buffer dirty
        core.apply_key(Key::from_ascii(b'i').unwrap());
        adapter.process_byte(b'i');
        
        // Still clean (no content changes yet)
        assert_eq!(adapter.is_dirty(), core.dirty());
        assert!(!adapter.is_dirty());
        
        // Actually insert content to make dirty
        core.apply_key(Key::from_ascii(b'x').unwrap());
        adapter.process_byte(b'x');

        // Now both should be dirty
        assert!(core.dirty(), "Core should be dirty after inserting text");
        assert!(adapter.is_dirty(), "Adapter should be dirty after inserting text");
        assert_eq!(adapter.is_dirty(), core.dirty(), "Dirty flags should match");
    }
}
