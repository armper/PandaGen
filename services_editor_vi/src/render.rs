//! Editor rendering and output

use alloc::string::{String, ToString};
use alloc::format;
use crate::state::{EditorMode, EditorState};

/// Editor view for rendering
///
/// Provides a simple text-based representation of the editor state
/// suitable for display in a console or test output.
pub struct EditorView {
    /// Number of lines to show in viewport
    viewport_lines: usize,
}

impl EditorView {
    pub fn new(viewport_lines: usize) -> Self {
        Self { viewport_lines }
    }

    /// Render the editor state to a string
    pub fn render(&self, state: &EditorState) -> String {
        let mut output = String::new();

        // Render viewport (buffer lines)
        let buffer = state.buffer();
        let cursor_pos = state.cursor().position();

        for row in 0..self.viewport_lines {
            if let Some(line) = buffer.line(row) {
                // Show cursor if on this line
                if row == cursor_pos.row {
                    output.push_str(&self.render_line_with_cursor(line, cursor_pos.col));
                } else {
                    output.push_str(line);
                }
            } else {
                output.push('~');
            }
            output.push('\n');
        }

        // Render status line
        output.push_str(&self.render_status_line(state));

        output
    }

    fn render_line_with_cursor(&self, line: &str, col: usize) -> String {
        let mut result = String::new();
        for (i, ch) in line.chars().enumerate() {
            if i == col {
                result.push_str(&format!("[{}]", ch));
            } else {
                result.push(ch);
            }
        }
        // Cursor at end of line
        if col == line.len() {
            result.push_str("[ ]");
        }
        result
    }

    fn render_status_line(&self, state: &EditorState) -> String {
        let mut status = String::new();

        // Mode
        status.push_str(state.mode().as_str());
        status.push(' ');

        // Document label with dirty indicator
        if let Some(label) = state.document_label() {
            status.push_str(label);
            if state.is_dirty() {
                status.push('*');
            }
            status.push(' ');
        } else if state.is_dirty() {
            // No filename but dirty
            status.push_str("[No Name]* ");
        }

        // Command buffer in command mode
        if state.mode() == EditorMode::Command {
            status.push(':');
            status.push_str(state.command_buffer());
        }

        // Search query in search mode
        if state.mode() == EditorMode::Search {
            status.push('/');
            status.push_str(state.search_query());
        }

        // Status message
        if !state.status_message().is_empty() {
            status.push_str(" | ");
            status.push_str(state.status_message());
        }

        status
    }

    /// Render just the status line
    pub fn render_status(&self, state: &EditorState) -> String {
        self.render_status_line(state)
    }
}

impl Default for EditorView {
    fn default() -> Self {
        Self::new(20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{EditorState, Position};
    use alloc::vec::Vec;

    #[test]
    fn test_render_empty_buffer() {
        let view = EditorView::new(3);
        let state = EditorState::new();

        let output = view.render(&state);
        let lines: Vec<&str> = output.lines().collect();

        assert_eq!(lines.len(), 4); // 3 viewport lines + 1 status line
        assert!(lines[0].contains("[ ]")); // Cursor at start
        assert_eq!(lines[1], "~");
        assert_eq!(lines[2], "~");
        assert!(lines[3].starts_with("NORMAL"));
    }

    #[test]
    fn test_render_with_content() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.load_content("hello\nworld".to_string());

        let output = view.render(&state);
        let lines: Vec<&str> = output.lines().collect();

        // First line contains hello and cursor at start
        assert!(lines[0].contains("hello") || lines[0].contains("[h]ello"));
        assert_eq!(lines[1], "world");
        assert_eq!(lines[2], "~");
    }

    #[test]
    fn test_render_cursor_position() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.load_content("hello".to_string());
        state.cursor_mut().set_position(Position::new(0, 2));

        let output = view.render(&state);
        let lines: Vec<&str> = output.lines().collect();

        assert!(lines[0].contains("[l]")); // Cursor on 'l'
    }

    #[test]
    fn test_render_status_normal_mode() {
        let view = EditorView::new(3);
        let state = EditorState::new();

        let status = view.render_status(&state);
        assert!(status.starts_with("NORMAL"));
    }

    #[test]
    fn test_render_status_insert_mode() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_mode(EditorMode::Insert);

        let status = view.render_status(&state);
        assert!(status.starts_with("INSERT"));
    }

    #[test]
    fn test_render_status_dirty_flag() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.mark_dirty();

        let status = view.render_status(&state);
        assert!(status.contains("[No Name]*"));
    }

    #[test]
    fn test_render_status_dirty_with_filename() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_document_label(Some("test.txt".to_string()));
        state.mark_dirty();

        let status = view.render_status(&state);
        assert!(status.contains("test.txt*"));
    }

    #[test]
    fn test_render_status_with_document_label() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_document_label(Some("test.txt".to_string()));

        let status = view.render_status(&state);
        assert!(status.contains("test.txt"));
    }

    #[test]
    fn test_render_status_command_mode() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_mode(EditorMode::Command);
        state.append_to_command('w');
        state.append_to_command('q');

        let status = view.render_status(&state);
        assert!(status.contains(":wq"));
    }

    #[test]
    fn test_render_status_with_message() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_status_message("Test message");

        let status = view.render_status(&state);
        assert!(status.contains("Test message"));
    }
}
