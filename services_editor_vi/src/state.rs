//! Editor state and buffer management

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Editor mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorMode {
    /// Normal mode (navigation and commands)
    Normal,
    /// Insert mode (text entry)
    Insert,
    /// Command mode (ex commands)
    Command,
    /// Search mode (search prompt)
    Search,
}

impl EditorMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Command => "COMMAND",
            EditorMode::Search => "SEARCH",
        }
    }
}

/// Cursor position in the buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

impl Position {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    pub fn zero() -> Self {
        Self { row: 0, col: 0 }
    }
}

/// Cursor state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    position: Position,
}

impl Cursor {
    pub fn new() -> Self {
        Self {
            position: Position::zero(),
        }
    }

    pub fn position(&self) -> Position {
        self.position
    }

    pub fn set_position(&mut self, pos: Position) {
        self.position = pos;
    }

    pub fn move_up(&mut self, buffer: &TextBuffer) {
        if self.position.row > 0 {
            self.position.row -= 1;
            self.clamp_col(buffer);
        }
    }

    pub fn move_down(&mut self, buffer: &TextBuffer) {
        if self.position.row < buffer.line_count().saturating_sub(1) {
            self.position.row += 1;
            self.clamp_col(buffer);
        }
    }

    pub fn move_left(&mut self) {
        if self.position.col > 0 {
            self.position.col -= 1;
        }
    }

    pub fn move_right(&mut self, buffer: &TextBuffer) {
        let line_len = buffer.line_length(self.position.row);
        if self.position.col < line_len {
            self.position.col += 1;
        }
    }

    /// Clamp column to valid range for current line
    fn clamp_col(&mut self, buffer: &TextBuffer) {
        let line_len = buffer.line_length(self.position.row);
        if self.position.col > line_len {
            self.position.col = line_len;
        }
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

/// Text buffer
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBuffer {
    lines: Vec<String>,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
        }
    }

    pub fn from_string(content: String) -> Self {
        let lines = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(|s| s.to_string()).collect()
        };
        Self { lines }
    }

    pub fn as_string(&self) -> String {
        self.lines.join("\n")
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, row: usize) -> Option<&str> {
        self.lines.get(row).map(|s| s.as_str())
    }

    pub fn line_length(&self, row: usize) -> usize {
        self.lines.get(row).map(|s| s.len()).unwrap_or(0)
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Insert a character at position
    pub fn insert_char(&mut self, pos: Position, ch: char) -> bool {
        if pos.row >= self.lines.len() {
            return false;
        }

        let line = &mut self.lines[pos.row];
        if pos.col > line.len() {
            return false;
        }

        line.insert(pos.col, ch);
        true
    }

    /// Insert a newline at position
    pub fn insert_newline(&mut self, pos: Position) -> bool {
        if pos.row >= self.lines.len() {
            return false;
        }

        let line = &mut self.lines[pos.row];
        if pos.col > line.len() {
            return false;
        }

        let rest = line.split_off(pos.col);
        self.lines.insert(pos.row + 1, rest);
        true
    }

    /// Delete character at position
    pub fn delete_char(&mut self, pos: Position) -> bool {
        if pos.row >= self.lines.len() {
            return false;
        }

        let line = &mut self.lines[pos.row];
        if pos.col >= line.len() {
            return false;
        }

        line.remove(pos.col);
        true
    }

    /// Delete character before position (backspace)
    pub fn backspace(&mut self, pos: Position) -> Option<Position> {
        if pos.col > 0 {
            // Delete character on same line
            let line = &mut self.lines[pos.row];
            line.remove(pos.col - 1);
            return Some(Position::new(pos.row, pos.col - 1));
        } else if pos.row > 0 {
            // Join with previous line
            let current_line = self.lines.remove(pos.row);
            let prev_line = &mut self.lines[pos.row - 1];
            let new_col = prev_line.len();
            prev_line.push_str(&current_line);
            return Some(Position::new(pos.row - 1, new_col));
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TextBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

/// Editor state snapshot for undo/redo
#[derive(Debug, Clone)]
struct EditorSnapshot {
    buffer: TextBuffer,
    cursor: Cursor,
}

/// Editor state
#[derive(Debug, Clone)]
pub struct EditorState {
    mode: EditorMode,
    buffer: TextBuffer,
    cursor: Cursor,
    dirty: bool,
    command_buffer: String,
    status_message: String,
    document_label: Option<String>,
    /// Undo history (stack of previous states)
    undo_stack: Vec<EditorSnapshot>,
    /// Redo history (stack of undone states)
    redo_stack: Vec<EditorSnapshot>,
    /// Current search query
    search_query: String,
    /// Last search query (for 'n' repeat search)
    last_search: Option<String>,
    /// Dirty lines tracking (line indices that have changed since last render)
    dirty_lines: BTreeSet<usize>,
    /// Cursor position changed flag
    cursor_dirty: bool,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            mode: EditorMode::Normal,
            buffer: TextBuffer::new(),
            cursor: Cursor::new(),
            dirty: false,
            command_buffer: String::new(),
            status_message: String::new(),
            document_label: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            search_query: String::new(),
            last_search: None,
            dirty_lines: BTreeSet::new(),
            cursor_dirty: false,
        }
    }

    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
        if mode != EditorMode::Command {
            self.command_buffer.clear();
        }
        if mode != EditorMode::Search {
            self.search_query.clear();
        }
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut TextBuffer {
        &mut self.buffer
    }

    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    pub fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.cursor
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn command_buffer(&self) -> &str {
        &self.command_buffer
    }

    pub fn append_to_command(&mut self, ch: char) {
        self.command_buffer.push(ch);
    }

    pub fn backspace_command(&mut self) {
        self.command_buffer.pop();
    }

    pub fn clear_command(&mut self) {
        self.command_buffer.clear();
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    pub fn set_status_message(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
    }

    pub fn move_cursor_up(&mut self) {
        if self.cursor.position.row > 0 {
            self.cursor.position.row -= 1;
            self.clamp_cursor_col();
            self.cursor_dirty = true;
        }
    }

    pub fn move_cursor_down(&mut self) {
        if self.cursor.position.row < self.buffer.line_count().saturating_sub(1) {
            self.cursor.position.row += 1;
            self.clamp_cursor_col();
            self.cursor_dirty = true;
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor.position.col > 0 {
            self.cursor.position.col -= 1;
            self.cursor_dirty = true;
        }
    }

    pub fn move_cursor_right(&mut self) {
        let line_len = self.buffer.line_length(self.cursor.position.row);
        if self.cursor.position.col < line_len {
            self.cursor.position.col += 1;
            self.cursor_dirty = true;
        }
    }

    fn clamp_cursor_col(&mut self) {
        let line_len = self.buffer.line_length(self.cursor.position.row);
        if self.cursor.position.col > line_len {
            self.cursor.position.col = line_len;
        }
    }

    pub fn document_label(&self) -> Option<&str> {
        self.document_label.as_deref()
    }

    pub fn set_document_label(&mut self, label: Option<String>) {
        self.document_label = label;
    }

    pub fn load_content(&mut self, content: String) {
        self.buffer = TextBuffer::from_string(content);
        self.cursor = Cursor::new();
        self.dirty = false;
    }

    /// Get current search query
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Append character to search query
    pub fn append_to_search(&mut self, ch: char) {
        self.search_query.push(ch);
    }

    /// Backspace in search query
    pub fn backspace_search(&mut self) {
        self.search_query.pop();
    }

    /// Clear search query
    pub fn clear_search(&mut self) {
        self.search_query.clear();
    }

    /// Execute search: find next occurrence of query from current position
    pub fn find_next(&mut self, forward: bool) -> bool {
        let query = if !self.search_query.is_empty() {
            self.last_search = Some(self.search_query.clone());
            &self.search_query
        } else if let Some(ref last) = self.last_search {
            last
        } else {
            return false;
        };

        if query.is_empty() {
            return false;
        }

        let start_pos = self.cursor.position();
        let line_count = self.buffer.line_count();

        // For first search (when search_query not empty), start from current position
        // For repeat search (n), start from next position
        let start_col = if !self.search_query.is_empty() {
            start_pos.col
        } else {
            start_pos.col + 1
        };

        // Search from start position
        if forward {
            let mut row = start_pos.row;
            let mut col = start_col;

            while row < line_count {
                if let Some(line) = self.buffer.line(row) {
                    if col < line.len() {
                        if let Some(pos) = line[col..].find(query) {
                            self.cursor.set_position(Position::new(row, col + pos));
                            return true;
                        }
                    }
                }
                row += 1;
                col = 0;
            }

            // Wrap around to beginning
            for row in 0..start_pos.row {
                if let Some(line) = self.buffer.line(row) {
                    if let Some(pos) = line.find(query) {
                        self.cursor.set_position(Position::new(row, pos));
                        return true;
                    }
                }
            }

            // Check start row up to start column (only for repeat search)
            if self.search_query.is_empty() && start_pos.row < line_count {
                if let Some(line) = self.buffer.line(start_pos.row) {
                    let end_col = start_pos.col.min(line.len());
                    if let Some(pos) = line[..end_col].find(query) {
                        self.cursor.set_position(Position::new(start_pos.row, pos));
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Save current state for undo
    pub fn save_undo_snapshot(&mut self) {
        let snapshot = EditorSnapshot {
            buffer: self.buffer.clone(),
            cursor: self.cursor.clone(),
        };
        self.undo_stack.push(snapshot);
        // Clear redo stack when making a new edit
        self.redo_stack.clear();

        // Limit undo stack size to prevent unbounded growth
        const MAX_UNDO_STACK: usize = 100;
        if self.undo_stack.len() > MAX_UNDO_STACK {
            self.undo_stack.remove(0);
        }
    }

    /// Undo last edit
    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo_stack.pop() {
            // Save current state to redo stack
            let current = EditorSnapshot {
                buffer: self.buffer.clone(),
                cursor: self.cursor.clone(),
            };
            self.redo_stack.push(current);

            // Restore snapshot
            self.buffer = snapshot.buffer;
            self.cursor = snapshot.cursor;
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    /// Redo previously undone edit
    pub fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.redo_stack.pop() {
            // Save current state to undo stack
            let current = EditorSnapshot {
                buffer: self.buffer.clone(),
                cursor: self.cursor.clone(),
            };
            self.undo_stack.push(current);

            // Restore snapshot
            self.buffer = snapshot.buffer;
            self.cursor = snapshot.cursor;
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    /// Mark a specific line as dirty (needs re-rendering)
    pub fn mark_line_dirty(&mut self, line: usize) {
        self.dirty_lines.insert(line);
    }

    /// Mark a range of lines as dirty
    pub fn mark_lines_dirty(&mut self, start: usize, end: usize) {
        for line in start..=end.min(self.buffer.line_count().saturating_sub(1)) {
            self.dirty_lines.insert(line);
        }
    }

    /// Mark the cursor as dirty (moved without content change)
    pub fn mark_cursor_dirty(&mut self) {
        self.cursor_dirty = true;
    }

    /// Get dirty lines and clear the dirty set
    pub fn take_dirty_lines(&mut self) -> Vec<usize> {
        let lines: Vec<usize> = self.dirty_lines.iter().copied().collect();
        self.dirty_lines.clear();
        lines
    }

    /// Check if cursor is dirty and clear the flag
    pub fn take_cursor_dirty(&mut self) -> bool {
        let dirty = self.cursor_dirty;
        self.cursor_dirty = false;
        dirty
    }

    /// Get dirty lines without clearing
    pub fn get_dirty_lines(&self) -> Vec<usize> {
        self.dirty_lines.iter().copied().collect()
    }

    /// Force mark all visible lines as dirty
    pub fn mark_all_dirty(&mut self, viewport_lines: usize) {
        for line in 0..viewport_lines.min(self.buffer.line_count()) {
            self.dirty_lines.insert(line);
        }
        self.cursor_dirty = true;
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_mode() {
        assert_eq!(EditorMode::Normal.as_str(), "NORMAL");
        assert_eq!(EditorMode::Insert.as_str(), "INSERT");
        assert_eq!(EditorMode::Command.as_str(), "COMMAND");
    }

    #[test]
    fn test_position() {
        let pos = Position::new(5, 10);
        assert_eq!(pos.row, 5);
        assert_eq!(pos.col, 10);

        let zero = Position::zero();
        assert_eq!(zero.row, 0);
        assert_eq!(zero.col, 0);
    }

    #[test]
    fn test_text_buffer_new() {
        let buffer = TextBuffer::new();
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.line(0), Some(""));
    }

    #[test]
    fn test_text_buffer_from_string() {
        let buffer = TextBuffer::from_string("hello\nworld".to_string());
        assert_eq!(buffer.line_count(), 2);
        assert_eq!(buffer.line(0), Some("hello"));
        assert_eq!(buffer.line(1), Some("world"));
    }

    #[test]
    fn test_text_buffer_to_string() {
        let mut buffer = TextBuffer::new();
        buffer.lines = vec!["hello".to_string(), "world".to_string()];
        assert_eq!(buffer.as_string(), "hello\nworld");
    }

    #[test]
    fn test_text_buffer_insert_char() {
        let mut buffer = TextBuffer::from_string("hello".to_string());
        assert!(buffer.insert_char(Position::new(0, 5), '!'));
        assert_eq!(buffer.line(0), Some("hello!"));
    }

    #[test]
    fn test_text_buffer_insert_newline() {
        let mut buffer = TextBuffer::from_string("hello".to_string());
        assert!(buffer.insert_newline(Position::new(0, 2)));
        assert_eq!(buffer.line_count(), 2);
        assert_eq!(buffer.line(0), Some("he"));
        assert_eq!(buffer.line(1), Some("llo"));
    }

    #[test]
    fn test_text_buffer_delete_char() {
        let mut buffer = TextBuffer::from_string("hello".to_string());
        assert!(buffer.delete_char(Position::new(0, 0)));
        assert_eq!(buffer.line(0), Some("ello"));
    }

    #[test]
    fn test_text_buffer_backspace() {
        let mut buffer = TextBuffer::from_string("hello".to_string());
        let new_pos = buffer.backspace(Position::new(0, 5));
        assert_eq!(new_pos, Some(Position::new(0, 4)));
        assert_eq!(buffer.line(0), Some("hell"));
    }

    #[test]
    fn test_text_buffer_backspace_line_join() {
        let mut buffer = TextBuffer::from_string("hello\nworld".to_string());
        let new_pos = buffer.backspace(Position::new(1, 0));
        assert_eq!(new_pos, Some(Position::new(0, 5)));
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.line(0), Some("helloworld"));
    }

    #[test]
    fn test_cursor_movement() {
        let mut state = EditorState::new();
        state.load_content("hello\nworld".to_string());

        state.move_cursor_down();
        assert_eq!(state.cursor().position().row, 1);

        state.move_cursor_right();
        assert_eq!(state.cursor().position().col, 1);

        state.move_cursor_up();
        assert_eq!(state.cursor().position().row, 0);

        state.move_cursor_left();
        assert_eq!(state.cursor().position().col, 0);
    }

    #[test]
    fn test_editor_state_modes() {
        let mut state = EditorState::new();
        assert_eq!(state.mode(), EditorMode::Normal);

        state.set_mode(EditorMode::Insert);
        assert_eq!(state.mode(), EditorMode::Insert);

        state.set_mode(EditorMode::Command);
        assert_eq!(state.mode(), EditorMode::Command);
    }

    #[test]
    fn test_editor_state_dirty_flag() {
        let mut state = EditorState::new();
        assert!(!state.is_dirty());

        state.mark_dirty();
        assert!(state.is_dirty());

        state.set_dirty(false);
        assert!(!state.is_dirty());
    }

    #[test]
    fn test_editor_state_command_buffer() {
        let mut state = EditorState::new();
        state.set_mode(EditorMode::Command);

        state.append_to_command('w');
        assert_eq!(state.command_buffer(), "w");

        state.append_to_command('q');
        assert_eq!(state.command_buffer(), "wq");

        state.backspace_command();
        assert_eq!(state.command_buffer(), "w");

        state.clear_command();
        assert_eq!(state.command_buffer(), "");
    }

    #[test]
    fn test_editor_state_status_message() {
        let mut state = EditorState::new();
        state.set_status_message("Test message");
        assert_eq!(state.status_message(), "Test message");
    }

    #[test]
    fn test_editor_state_load_content() {
        let mut state = EditorState::new();
        state.mark_dirty();

        state.load_content("test\ncontent".to_string());

        assert_eq!(state.buffer().line_count(), 2);
        assert_eq!(state.buffer().line(0), Some("test"));
        assert!(!state.is_dirty());
    }

    #[test]
    fn test_dirty_tracking_insert_char() {
        let mut state = EditorState::new();
        state.mark_line_dirty(5);

        let dirty = state.get_dirty_lines();
        assert_eq!(dirty, vec![5]);

        // Take should clear
        let taken = state.take_dirty_lines();
        assert_eq!(taken, vec![5]);
        assert!(state.get_dirty_lines().is_empty());
    }

    #[test]
    fn test_dirty_tracking_multiple_lines() {
        let mut state = EditorState::new();
        state.load_content("line1\nline2\nline3\nline4\nline5\nline6".to_string());
        state.mark_lines_dirty(2, 5);

        let dirty = state.get_dirty_lines();
        assert_eq!(dirty.len(), 4); // lines 2, 3, 4, 5
        assert!(dirty.contains(&2));
        assert!(dirty.contains(&3));
        assert!(dirty.contains(&4));
        assert!(dirty.contains(&5));
    }

    #[test]
    fn test_cursor_dirty_tracking() {
        let mut state = EditorState::new();
        assert!(!state.take_cursor_dirty());

        state.mark_cursor_dirty();
        assert!(state.take_cursor_dirty());

        // Should be cleared after take
        assert!(!state.take_cursor_dirty());
    }

    #[test]
    fn test_cursor_movement_marks_dirty() {
        let mut state = EditorState::new();
        state.load_content("hello\nworld".to_string());

        // Clear any dirty flags from load
        state.take_cursor_dirty();

        state.move_cursor_right();
        assert!(state.take_cursor_dirty());

        state.move_cursor_down();
        assert!(state.take_cursor_dirty());
    }

    #[test]
    fn test_mark_all_dirty() {
        let mut state = EditorState::new();
        state.load_content("line1\nline2\nline3\nline4\nline5".to_string());

        state.mark_all_dirty(3);
        let dirty = state.get_dirty_lines();

        assert!(dirty.len() >= 3);
        assert!(state.take_cursor_dirty());
    }
}
