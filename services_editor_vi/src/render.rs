//! Editor rendering and output

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
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

        // Status message or mode hint
        if !state.status_message().is_empty() {
            status.push_str(" | ");
            status.push_str(state.status_message());
        } else {
            // Show mode-specific hints when no status message
            status.push_str(" | ");
            status.push_str(&get_mode_hint(state.mode()));
            
            // Add command suggestions in Command mode
            if state.mode() == EditorMode::Command {
                let suggestions = get_command_suggestions(state.command_buffer());
                if !suggestions.is_empty() {
                    status.push_str(" | Suggestions: ");
                    status.push_str(&suggestions.join(" "));
                }
            }
        }

        status
    }

    /// Render just the status line
    pub fn render_status(&self, state: &EditorState) -> String {
        self.render_status_line(state)
    }
}

/// Get mode-specific hint text
fn get_mode_hint(mode: EditorMode) -> &'static str {
    match mode {
        EditorMode::Normal => "Normal — i=Insert :w=Save :q=Quit :help",
        EditorMode::Insert => "Insert — Esc=Normal",
        EditorMode::Command => "Command — Enter=Run Esc=Cancel :w :q :wq",
        EditorMode::Search => "Search — Enter=Find Esc=Cancel",
    }
}

/// Get command suggestions based on current command buffer
/// Returns up to 3 suggestions in deterministic order:
/// 1. Prefix matches first (command.starts_with(buffer))
/// 2. Then substring matches (command.contains(buffer))
/// 3. Lexicographic tie-break for stable ordering
fn get_command_suggestions(buffer: &str) -> Vec<&'static str> {
    use alloc::vec;
    const COMMANDS: &[&str] = &["e", "help", "q", "w", "wq"];
    
    if buffer.is_empty() {
        // Return top 3 commands lexicographically
        return vec!["e", "help", "q"];
    }
    
    let mut prefix_matches = Vec::new();
    let mut substring_matches = Vec::new();
    
    for &cmd in COMMANDS {
        if cmd.starts_with(buffer) {
            prefix_matches.push(cmd);
        } else if cmd.contains(buffer) {
            substring_matches.push(cmd);
        }
    }
    
    // Sort for deterministic ordering
    prefix_matches.sort();
    substring_matches.sort();
    
    // Combine: prefix matches first, then substring matches
    let mut result = prefix_matches;
    result.extend(substring_matches);
    
    // Return up to 3 suggestions
    result.truncate(3);
    result
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
    use alloc::vec;
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

    // Tests for mode hints
    #[test]
    fn test_mode_hint_normal() {
        let hint = get_mode_hint(EditorMode::Normal);
        assert_eq!(hint, "Normal — i=Insert :w=Save :q=Quit :help");
    }

    #[test]
    fn test_mode_hint_insert() {
        let hint = get_mode_hint(EditorMode::Insert);
        assert_eq!(hint, "Insert — Esc=Normal");
    }

    #[test]
    fn test_mode_hint_command() {
        let hint = get_mode_hint(EditorMode::Command);
        assert_eq!(hint, "Command — Enter=Run Esc=Cancel :w :q :wq");
    }

    #[test]
    fn test_mode_hint_search() {
        let hint = get_mode_hint(EditorMode::Search);
        assert_eq!(hint, "Search — Enter=Find Esc=Cancel");
    }

    // Tests for command suggestions
    #[test]
    fn test_suggestions_empty_buffer() {
        let suggestions = get_command_suggestions("");
        assert_eq!(suggestions, vec!["e", "help", "q"]);
    }

    #[test]
    fn test_suggestions_prefix_w() {
        let suggestions = get_command_suggestions("w");
        assert_eq!(suggestions, vec!["w", "wq"]);
    }

    #[test]
    fn test_suggestions_prefix_h() {
        let suggestions = get_command_suggestions("h");
        assert_eq!(suggestions, vec!["help"]);
    }

    #[test]
    fn test_suggestions_prefix_q() {
        let suggestions = get_command_suggestions("q");
        assert_eq!(suggestions, vec!["q", "wq"]); // "wq" contains "q" as substring
    }

    #[test]
    fn test_suggestions_prefix_e() {
        let suggestions = get_command_suggestions("e");
        assert_eq!(suggestions, vec!["e", "help"]); // "help" contains "e"
    }

    #[test]
    fn test_suggestions_unknown() {
        let suggestions = get_command_suggestions("xyz");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_suggestions_wq() {
        let suggestions = get_command_suggestions("wq");
        assert_eq!(suggestions, vec!["wq"]);
    }

    #[test]
    fn test_suggestions_deterministic_order() {
        // Test that suggestions are always returned in the same order
        let s1 = get_command_suggestions("e");
        let s2 = get_command_suggestions("e");
        assert_eq!(s1, s2);
    }

    // Integration tests for status line with hints
    #[test]
    fn test_status_line_with_normal_hint() {
        let view = EditorView::new(3);
        let state = EditorState::new();

        let status = view.render_status(&state);
        assert!(status.contains("Normal — i=Insert :w=Save :q=Quit :help"));
    }

    #[test]
    fn test_status_line_with_insert_hint() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_mode(EditorMode::Insert);

        let status = view.render_status(&state);
        assert!(status.contains("Insert — Esc=Normal"));
    }

    #[test]
    fn test_status_line_with_command_hint_and_suggestions() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_mode(EditorMode::Command);

        let status = view.render_status(&state);
        assert!(status.contains("Command — Enter=Run Esc=Cancel :w :q :wq"));
        assert!(status.contains("Suggestions: e help q"));
    }

    #[test]
    fn test_status_line_command_with_buffer() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_mode(EditorMode::Command);
        state.append_to_command('w');

        let status = view.render_status(&state);
        assert!(status.contains(":w"));
        assert!(status.contains("Suggestions: w wq"));
    }

    #[test]
    fn test_status_message_overrides_hint() {
        let view = EditorView::new(3);
        let mut state = EditorState::new();
        state.set_status_message("Custom message");

        let status = view.render_status(&state);
        assert!(status.contains("Custom message"));
        // Hint should not appear when status message is present
        assert!(!status.contains("Normal — i=Insert"));
    }
}
