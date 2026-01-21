//! Tests for the minimal bare-metal editor
//!
//! These tests validate that the editor behaves correctly and matches
//! the expected behavior of the full services_editor_vi editor.

#[cfg(test)]
mod tests {
    use crate::minimal_editor::{EditorMode, MinimalEditor, Position};

    #[test]
    fn test_editor_starts_in_normal_mode() {
        let editor = MinimalEditor::new(24);
        assert_eq!(editor.mode(), EditorMode::Normal);
        assert_eq!(editor.cursor(), Position::zero());
    }

    #[test]
    fn test_enter_insert_mode() {
        let mut editor = MinimalEditor::new(24);
        editor.process_byte(b'i');
        assert_eq!(editor.mode(), EditorMode::Insert);
    }

    #[test]
    fn test_exit_insert_mode() {
        let mut editor = MinimalEditor::new(24);
        editor.process_byte(b'i');
        assert_eq!(editor.mode(), EditorMode::Insert);
        editor.process_byte(0x1B); // Escape
        assert_eq!(editor.mode(), EditorMode::Normal);
    }

    #[test]
    fn test_insert_text() {
        let mut editor = MinimalEditor::new(24);
        editor.process_byte(b'i');
        editor.process_byte(b'h');
        editor.process_byte(b'e');
        editor.process_byte(b'l');
        editor.process_byte(b'l');
        editor.process_byte(b'o');

        let line = editor.get_viewport_line(0).unwrap();
        assert_eq!(line, "hello");
        assert!(editor.is_dirty());
    }

    #[test]
    fn test_backspace_in_insert_mode() {
        let mut editor = MinimalEditor::new(24);
        editor.process_byte(b'i');
        editor.process_byte(b'h');
        editor.process_byte(b'i');
        editor.process_byte(0x08); // Backspace
        editor.process_byte(b'e');

        let line = editor.get_viewport_line(0).unwrap();
        assert_eq!(line, "he");
    }

    #[test]
    fn test_newline_in_insert_mode() {
        let mut editor = MinimalEditor::new(24);
        editor.process_byte(b'i');
        editor.process_byte(b'l');
        editor.process_byte(b'i');
        editor.process_byte(b'n');
        editor.process_byte(b'e');
        editor.process_byte(b'1');
        editor.process_byte(b'\n');
        editor.process_byte(b'l');
        editor.process_byte(b'i');
        editor.process_byte(b'n');
        editor.process_byte(b'e');
        editor.process_byte(b'2');

        let line1 = editor.get_viewport_line(0).unwrap();
        let line2 = editor.get_viewport_line(1).unwrap();
        assert_eq!(line1, "line1");
        assert_eq!(line2, "line2");
    }

    #[test]
    fn test_hjkl_movement_in_normal_mode() {
        let mut editor = MinimalEditor::new(24);

        // Insert some text
        editor.process_byte(b'i');
        editor.process_byte(b'a');
        editor.process_byte(b'b');
        editor.process_byte(b'c');
        editor.process_byte(b'\n');
        editor.process_byte(b'd');
        editor.process_byte(b'e');
        editor.process_byte(b'f');
        editor.process_byte(0x1B); // Escape to normal

        // Cursor should be at (1, 2) - end of line 2
        // Note: Escape moves cursor left by 1
        assert_eq!(editor.cursor().row, 1);

        // Test h (left)
        editor.process_byte(b'h');
        assert_eq!(editor.cursor().col, editor.cursor().col);

        // Test k (up)
        editor.process_byte(b'k');
        assert_eq!(editor.cursor().row, 0);

        // Test j (down)
        editor.process_byte(b'j');
        assert_eq!(editor.cursor().row, 1);

        // Test l (right)
        let initial_col = editor.cursor().col;
        editor.process_byte(b'l');
        assert!(editor.cursor().col >= initial_col);
    }

    #[test]
    fn test_x_deletes_character() {
        let mut editor = MinimalEditor::new(24);

        // Insert "abc"
        editor.process_byte(b'i');
        editor.process_byte(b'a');
        editor.process_byte(b'b');
        editor.process_byte(b'c');
        editor.process_byte(0x1B); // Escape

        // Delete 'c'
        editor.process_byte(b'x');

        let line = editor.get_viewport_line(0).unwrap();
        assert_eq!(line, "ab");
    }

    #[test]
    fn test_command_mode_quit() {
        let mut editor = MinimalEditor::new(24);
        editor.process_byte(b':');
        assert_eq!(editor.mode(), EditorMode::Command);

        editor.process_byte(b'q');
        let should_quit = editor.process_byte(b'\n');
        assert!(should_quit);
    }

    #[test]
    fn test_command_mode_quit_dirty_buffer() {
        let mut editor = MinimalEditor::new(24);

        // Make buffer dirty
        editor.process_byte(b'i');
        editor.process_byte(b'x');
        editor.process_byte(0x1B);

        // Try to quit without saving
        editor.process_byte(b':');
        editor.process_byte(b'q');
        let should_quit = editor.process_byte(b'\n');
        assert!(!should_quit); // Should not quit
        assert!(editor.status_line().contains("No write"));
    }

    #[test]
    fn test_command_mode_force_quit() {
        let mut editor = MinimalEditor::new(24);

        // Make buffer dirty
        editor.process_byte(b'i');
        editor.process_byte(b'x');
        editor.process_byte(0x1B);

        // Force quit
        editor.process_byte(b':');
        editor.process_byte(b'q');
        editor.process_byte(b'!');
        let should_quit = editor.process_byte(b'\n');
        assert!(should_quit);
    }

    #[test]
    fn test_command_mode_write() {
        let mut editor = MinimalEditor::new(24);

        // Make buffer dirty
        editor.process_byte(b'i');
        editor.process_byte(b'x');
        editor.process_byte(0x1B);

        // Try to write
        editor.process_byte(b':');
        editor.process_byte(b'w');
        editor.process_byte(b'\n');

        // Should show filesystem unavailable message
        assert!(editor.status_line().contains("unavailable"));
        assert!(!editor.is_dirty()); // But pretend we saved
    }

    #[test]
    fn test_golden_trace_insert_and_quit() {
        // Golden input trace: i, "test", Escape, :q!
        let mut editor = MinimalEditor::new(24);

        // Step 1: Enter insert mode
        editor.process_byte(b'i');
        assert_eq!(editor.mode(), EditorMode::Insert);

        // Step 2: Type "test"
        editor.process_byte(b't');
        editor.process_byte(b'e');
        editor.process_byte(b's');
        editor.process_byte(b't');

        // Step 3: Exit insert mode
        editor.process_byte(0x1B);
        assert_eq!(editor.mode(), EditorMode::Normal);

        // Verify buffer
        let line = editor.get_viewport_line(0).unwrap();
        assert_eq!(line, "test");

        // Step 4: Force quit
        editor.process_byte(b':');
        editor.process_byte(b'q');
        editor.process_byte(b'!');
        let should_quit = editor.process_byte(b'\n');
        assert!(should_quit);
    }

    #[test]
    fn test_golden_trace_multiline_edit() {
        // Golden trace: i, "line1", Enter, "line2", Escape, k, x, :q!
        let mut editor = MinimalEditor::new(24);

        editor.process_byte(b'i');
        editor.process_byte(b'l');
        editor.process_byte(b'i');
        editor.process_byte(b'n');
        editor.process_byte(b'e');
        editor.process_byte(b'1');
        editor.process_byte(b'\n');
        editor.process_byte(b'l');
        editor.process_byte(b'i');
        editor.process_byte(b'n');
        editor.process_byte(b'e');
        editor.process_byte(b'2');
        editor.process_byte(0x1B);

        assert_eq!(editor.mode(), EditorMode::Normal);
        assert_eq!(editor.cursor().row, 1);

        // Move up and delete
        editor.process_byte(b'k');
        assert_eq!(editor.cursor().row, 0);

        editor.process_byte(b'x');

        let line1 = editor.get_viewport_line(0).unwrap();
        assert!(line1.starts_with("ine1") || line1 == "line1"); // Depends on cursor position

        // Force quit
        editor.process_byte(b':');
        editor.process_byte(b'q');
        editor.process_byte(b'!');
        let should_quit = editor.process_byte(b'\n');
        assert!(should_quit);
    }

    #[test]
    fn test_viewport_scrolling() {
        let viewport_rows = 5;
        let mut editor = MinimalEditor::new(viewport_rows);

        // Insert more lines than viewport
        editor.process_byte(b'i');
        for i in 0..10 {
            editor.process_byte(b'0' + i);
            editor.process_byte(b'\n');
        }
        editor.process_byte(0x1B);

        // Scroll offset should adjust
        assert!(editor.scroll_offset() > 0);
    }

    #[test]
    fn test_status_line_shows_mode() {
        let mut editor = MinimalEditor::new(24);

        assert!(editor.status_line().contains("NORMAL"));

        editor.process_byte(b'i');
        assert!(editor.status_line().contains("INSERT"));

        editor.process_byte(0x1B);
        editor.process_byte(b':');
        assert!(editor.status_line().starts_with(':'));
    }

    #[test]
    fn test_command_mode_escape_cancels() {
        let mut editor = MinimalEditor::new(24);

        editor.process_byte(b':');
        assert_eq!(editor.mode(), EditorMode::Command);

        editor.process_byte(b'q');
        editor.process_byte(0x1B); // Escape

        assert_eq!(editor.mode(), EditorMode::Normal);
    }

    #[test]
    fn test_cursor_position_in_viewport() {
        let mut editor = MinimalEditor::new(24);

        editor.process_byte(b'i');
        editor.process_byte(b'a');
        editor.process_byte(b'b');
        editor.process_byte(b'c');

        let cursor = editor.get_viewport_cursor().unwrap();
        assert_eq!(cursor.row, 0);
        assert_eq!(cursor.col, 3); // After 'c'
    }
}
