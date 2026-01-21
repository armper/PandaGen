//! EditorCore state machine
//!
//! A complete, testable, no_std editor state machine following the modal
//! editing philosophy from services_editor_vi and kernel_bootstrap.

use alloc::string::String;
use alloc::vec::Vec;

use crate::{
    buffer::{Position, TextBuffer},
    command::{parse_command, Command},
    key::Key,
    mode::EditorMode,
    snapshot::EditorSnapshot,
};

/// Snapshot for undo/redo
#[derive(Debug, Clone)]
struct BufferSnapshot {
    buffer: TextBuffer,
    cursor: Position,
}

/// Outcome from applying a key to the editor
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreOutcome {
    /// Continue editing (no state change)
    Continue,
    /// State changed (buffer modified, mode changed, etc)
    Changed,
    /// Request to exit the editor
    RequestExit { forced: bool },
    /// Display a status message
    StatusMessage(String),
    /// Request IO operation from host
    RequestIo(CoreIoRequest),
}

/// IO request from editor core to host
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreIoRequest {
    /// Save to current file
    Save,
    /// Save to specified path
    SaveAs(String),
    /// Save to current file and quit
    SaveAndQuit,
}

/// Editor core state machine
pub struct EditorCore {
    mode: EditorMode,
    buffer: TextBuffer,
    cursor: Position,
    dirty: bool,
    command_buffer: String,
    search_query: String,
    last_search: Option<String>,
    status_message: String,
    undo_stack: Vec<BufferSnapshot>,
    redo_stack: Vec<BufferSnapshot>,
}

impl EditorCore {
    /// Create a new empty editor
    pub fn new() -> Self {
        Self {
            mode: EditorMode::Normal,
            buffer: TextBuffer::new(),
            cursor: Position::zero(),
            dirty: false,
            command_buffer: String::new(),
            search_query: String::new(),
            last_search: None,
            status_message: String::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Apply a key event and return the outcome
    pub fn apply_key(&mut self, key: Key) -> CoreOutcome {
        match self.mode {
            EditorMode::Normal => self.handle_normal_mode(key),
            EditorMode::Insert => self.handle_insert_mode(key),
            EditorMode::Command => self.handle_command_mode(key),
            EditorMode::Search => self.handle_search_mode(key),
        }
    }

    /// Get a complete snapshot of editor state (for parity testing)
    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            mode: self.mode,
            cursor: self.cursor,
            buffer_lines: self.buffer.lines().to_vec(),
            dirty: self.dirty,
            command_buffer: self.command_buffer.clone(),
            search_query: self.search_query.clone(),
            undo_depth: self.undo_stack.len(),
            redo_depth: self.redo_stack.len(),
        }
    }

    // Public accessors for rendering/testing
    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    pub fn cursor(&self) -> Position {
        self.cursor
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn command_buffer(&self) -> &str {
        &self.command_buffer
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn status_message(&self) -> &str {
        &self.status_message
    }

    // Private mode handlers

    /// Helper to insert a character in insert mode
    fn insert_char_in_insert_mode(&mut self, ch: char) -> CoreOutcome {
        self.save_undo_snapshot();
        if self.buffer.insert_char(self.cursor, ch) {
            self.cursor.col += 1;
            self.dirty = true;
            CoreOutcome::Changed
        } else {
            CoreOutcome::Continue
        }
    }

    fn handle_normal_mode(&mut self, key: Key) -> CoreOutcome {
        match key {
            // Enter insert mode
            Key::I => {
                self.mode = EditorMode::Insert;
                CoreOutcome::Changed
            }
            Key::A => {
                self.mode = EditorMode::Insert;
                // Move cursor right (even to line_len for append)
                let line_len = self.buffer.line_length(self.cursor.row);
                if self.cursor.col < line_len {
                    self.cursor.col += 1;
                }
                CoreOutcome::Changed
            }

            // Navigation
            Key::H | Key::Left => {
                self.move_cursor_left();
                CoreOutcome::Changed
            }
            Key::J | Key::Down => {
                self.move_cursor_down();
                CoreOutcome::Changed
            }
            Key::K | Key::Up => {
                self.move_cursor_up();
                CoreOutcome::Changed
            }
            Key::L | Key::Right => {
                self.move_cursor_right();
                CoreOutcome::Changed
            }

            // Editing
            Key::X => {
                self.save_undo_snapshot();
                if self.buffer.delete_char(self.cursor) {
                    self.dirty = true;
                    CoreOutcome::Changed
                } else {
                    CoreOutcome::Continue
                }
            }
            Key::D => {
                self.save_undo_snapshot();
                if self.buffer.delete_line(self.cursor.row) {
                    self.dirty = true;
                    if self.cursor.row >= self.buffer.line_count() {
                        self.cursor.row = self.buffer.line_count().saturating_sub(1);
                    }
                    self.clamp_cursor();
                    CoreOutcome::Changed
                } else {
                    CoreOutcome::Continue
                }
            }

            // Undo/redo
            Key::U => {
                if self.undo() {
                    CoreOutcome::StatusMessage("Undo".into())
                } else {
                    CoreOutcome::StatusMessage("Already at oldest change".into())
                }
            }
            Key::CtrlR => {
                if self.redo() {
                    CoreOutcome::StatusMessage("Redo".into())
                } else {
                    CoreOutcome::StatusMessage("Already at newest change".into())
                }
            }

            // Enter command mode
            Key::Colon => {
                self.mode = EditorMode::Command;
                self.command_buffer.clear();
                CoreOutcome::Changed
            }

            // Enter search mode
            Key::Slash => {
                self.mode = EditorMode::Search;
                self.search_query.clear();
                CoreOutcome::Changed
            }

            // Repeat last search
            Key::N => {
                if self.find_next() {
                    CoreOutcome::Changed
                } else {
                    CoreOutcome::StatusMessage("Pattern not found".into())
                }
            }

            _ => CoreOutcome::Continue,
        }
    }

    fn handle_insert_mode(&mut self, key: Key) -> CoreOutcome {
        match key {
            Key::Escape => {
                // Exit insert mode
                self.mode = EditorMode::Normal;
                // Move cursor back if possible (vi behavior)
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
                self.clamp_cursor();
                CoreOutcome::Changed
            }
            Key::Char(ch) => self.insert_char_in_insert_mode(ch),
            // Handle dedicated key variants as their character equivalents in insert mode
            // This allows typing 'i', 'a', 'h', etc. in insert mode
            Key::I => self.insert_char_in_insert_mode('i'),
            Key::A => self.insert_char_in_insert_mode('a'),
            Key::H => self.insert_char_in_insert_mode('h'),
            Key::J => self.insert_char_in_insert_mode('j'),
            Key::K => self.insert_char_in_insert_mode('k'),
            Key::L => self.insert_char_in_insert_mode('l'),
            Key::X => self.insert_char_in_insert_mode('x'),
            Key::D => self.insert_char_in_insert_mode('d'),
            Key::U => self.insert_char_in_insert_mode('u'),
            Key::N => self.insert_char_in_insert_mode('n'),
            Key::Space => self.insert_char_in_insert_mode(' '),
            Key::Enter => {
                self.save_undo_snapshot();
                if self.buffer.insert_newline(self.cursor) {
                    self.cursor.row += 1;
                    self.cursor.col = 0;
                    self.dirty = true;
                    CoreOutcome::Changed
                } else {
                    CoreOutcome::Continue
                }
            }
            Key::Backspace => {
                self.save_undo_snapshot();
                if let Some(new_pos) = self.buffer.backspace(self.cursor) {
                    self.cursor = new_pos;
                    self.dirty = true;
                    CoreOutcome::Changed
                } else {
                    CoreOutcome::Continue
                }
            }
            _ => CoreOutcome::Continue,
        }
    }

    fn handle_command_mode(&mut self, key: Key) -> CoreOutcome {
        match key {
            Key::Escape => {
                // Cancel command
                self.mode = EditorMode::Normal;
                self.command_buffer.clear();
                CoreOutcome::Changed
            }
            Key::Enter => {
                // Execute command
                let outcome = self.execute_command();
                if !matches!(outcome, CoreOutcome::RequestExit { .. }) {
                    self.mode = EditorMode::Normal;
                }
                outcome
            }
            Key::Backspace => {
                if !self.command_buffer.is_empty() {
                    self.command_buffer.pop();
                }
                CoreOutcome::Changed
            }
            Key::Char(ch) => {
                self.command_buffer.push(ch);
                CoreOutcome::Changed
            }
            Key::Space => {
                self.command_buffer.push(' ');
                CoreOutcome::Changed
            }
            _ => CoreOutcome::Continue,
        }
    }

    fn handle_search_mode(&mut self, key: Key) -> CoreOutcome {
        match key {
            Key::Escape => {
                // Cancel search
                self.mode = EditorMode::Normal;
                self.search_query.clear();
                CoreOutcome::Changed
            }
            Key::Enter => {
                // Execute search and exit search mode
                let outcome = if self.find_next() {
                    CoreOutcome::Changed
                } else {
                    CoreOutcome::StatusMessage("Pattern not found".into())
                };
                self.mode = EditorMode::Normal;
                self.search_query.clear(); // Clear so 'n' knows it's a repeat
                outcome
            }
            Key::Backspace => {
                if !self.search_query.is_empty() {
                    self.search_query.pop();
                }
                CoreOutcome::Changed
            }
            Key::Char(ch) => {
                self.search_query.push(ch);
                CoreOutcome::Changed
            }
            Key::Space => {
                self.search_query.push(' ');
                CoreOutcome::Changed
            }
            _ => CoreOutcome::Continue,
        }
    }

    fn execute_command(&mut self) -> CoreOutcome {
        let cmd = parse_command(&self.command_buffer);

        match cmd {
            Command::Quit { force } => {
                if self.dirty && !force {
                    CoreOutcome::StatusMessage(
                        "No write since last change (use :q! to override)".into(),
                    )
                } else {
                    CoreOutcome::RequestExit { forced: force }
                }
            }
            Command::Write => CoreOutcome::RequestIo(CoreIoRequest::Save),
            Command::WriteAs(path) => CoreOutcome::RequestIo(CoreIoRequest::SaveAs(path)),
            Command::WriteQuit => {
                if self.dirty {
                    CoreOutcome::RequestIo(CoreIoRequest::SaveAndQuit)
                } else {
                    CoreOutcome::RequestExit { forced: false }
                }
            }
            Command::Unknown(cmd_str) => {
                CoreOutcome::StatusMessage(alloc::format!("Unknown command: {}", cmd_str))
            }
        }
    }

    // Undo/redo implementation

    fn save_undo_snapshot(&mut self) {
        let snapshot = BufferSnapshot {
            buffer: self.buffer.clone(),
            cursor: self.cursor,
        };
        self.undo_stack.push(snapshot);
        // Clear redo stack on new edit
        self.redo_stack.clear();

        // Limit stack size
        const MAX_UNDO_STACK: usize = 100;
        if self.undo_stack.len() > MAX_UNDO_STACK {
            self.undo_stack.remove(0);
        }
    }

    fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo_stack.pop() {
            // Save current state to redo
            let current = BufferSnapshot {
                buffer: self.buffer.clone(),
                cursor: self.cursor,
            };
            self.redo_stack.push(current);

            // Restore snapshot
            self.buffer = snapshot.buffer;
            self.cursor = snapshot.cursor;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.redo_stack.pop() {
            // Save current state to undo
            let current = BufferSnapshot {
                buffer: self.buffer.clone(),
                cursor: self.cursor,
            };
            self.undo_stack.push(current);

            // Restore snapshot
            self.buffer = snapshot.buffer;
            self.cursor = snapshot.cursor;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    // Search implementation

    fn find_next(&mut self) -> bool {
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

        let start_pos = self.cursor;
        let line_count = self.buffer.line_count();

        // Start from next position (for repeat search with 'n')
        let start_col = if !self.search_query.is_empty() {
            start_pos.col
        } else {
            start_pos.col + 1
        };

        // Search from start position forward
        let mut row = start_pos.row;
        let mut col = start_col;

        while row < line_count {
            if let Some(line) = self.buffer.line(row) {
                if col < line.len() {
                    if let Some(pos) = line[col..].find(query.as_str()) {
                        self.cursor = Position::new(row, col + pos);
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
                if let Some(pos) = line.find(query.as_str()) {
                    self.cursor = Position::new(row, pos);
                    return true;
                }
            }
        }

        // Check start row up to start column (only for repeat search)
        if self.search_query.is_empty() && start_pos.row < line_count {
            if let Some(line) = self.buffer.line(start_pos.row) {
                let end_col = start_pos.col.min(line.len());
                if let Some(pos) = line[..end_col].find(query.as_str()) {
                    self.cursor = Position::new(start_pos.row, pos);
                    return true;
                }
            }
        }

        false
    }

    // Cursor movement

    fn move_cursor_up(&mut self) {
        if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.clamp_cursor();
        }
    }

    fn move_cursor_down(&mut self) {
        if self.cursor.row < self.buffer.line_count().saturating_sub(1) {
            self.cursor.row += 1;
            self.clamp_cursor();
        }
    }

    fn move_cursor_left(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        }
    }

    fn move_cursor_right(&mut self) {
        let line_len = self.buffer.line_length(self.cursor.row);
        if self.cursor.col < line_len {
            self.cursor.col += 1;
        }
    }

    fn clamp_cursor(&mut self) {
        let line_len = self.buffer.line_length(self.cursor.row);
        if self.cursor.col > line_len {
            self.cursor.col = line_len;
        }
    }

    // Public API for loading content
    pub fn load_content(&mut self, content: String) {
        self.buffer = TextBuffer::from_string(content);
        self.cursor = Position::zero();
        self.dirty = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }
}

impl Default for EditorCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_editor() {
        let editor = EditorCore::new();
        assert_eq!(editor.mode(), EditorMode::Normal);
        assert_eq!(editor.cursor(), Position::zero());
        assert!(!editor.dirty());
        assert_eq!(editor.buffer().line_count(), 1);
    }

    #[test]
    fn test_enter_insert_mode() {
        let mut editor = EditorCore::new();
        let outcome = editor.apply_key(Key::I);
        assert_eq!(outcome, CoreOutcome::Changed);
        assert_eq!(editor.mode(), EditorMode::Insert);
    }

    #[test]
    fn test_insert_char() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I); // Enter insert mode
        let outcome = editor.apply_key(Key::Char('h'));
        assert_eq!(outcome, CoreOutcome::Changed);
        assert!(editor.dirty());
        assert_eq!(editor.buffer().line(0), Some("h"));
        assert_eq!(editor.cursor(), Position::new(0, 1));
    }

    #[test]
    fn test_insert_vi_command_letters_in_insert_mode() {
        // Bug fix test: Ensure that vi command letters (i, a, h, j, k, l, etc.)
        // can be typed normally in INSERT mode
        let mut editor = EditorCore::new();
        
        // Enter insert mode
        editor.apply_key(Key::I);
        assert_eq!(editor.mode(), EditorMode::Insert);
        
        // Type the letters that are also vi commands
        editor.apply_key(Key::I); // Should type 'i', not enter insert mode again
        editor.apply_key(Key::A); // Should type 'a', not append mode
        editor.apply_key(Key::H); // Should type 'h', not move left
        editor.apply_key(Key::J); // Should type 'j', not move down
        editor.apply_key(Key::K); // Should type 'k', not move up
        editor.apply_key(Key::L); // Should type 'l', not move right
        editor.apply_key(Key::X); // Should type 'x', not delete
        editor.apply_key(Key::D); // Should type 'd', not delete line
        editor.apply_key(Key::U); // Should type 'u', not undo
        editor.apply_key(Key::N); // Should type 'n', not repeat search
        
        // Verify all letters were typed
        assert_eq!(editor.buffer().as_string(), "iahjklxdun");
        assert_eq!(editor.mode(), EditorMode::Insert);
    }

    #[test]
    fn test_escape_from_insert() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('h'));
        editor.apply_key(Key::Char('i'));
        let outcome = editor.apply_key(Key::Escape);
        assert_eq!(outcome, CoreOutcome::Changed);
        assert_eq!(editor.mode(), EditorMode::Normal);
        // Cursor should move back one position
        assert_eq!(editor.cursor(), Position::new(0, 1));
    }

    #[test]
    fn test_insert_newline() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('h'));
        editor.apply_key(Key::Char('i'));
        editor.apply_key(Key::Enter);
        assert_eq!(editor.buffer().line_count(), 2);
        assert_eq!(editor.cursor(), Position::new(1, 0));
    }

    #[test]
    fn test_backspace() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('h'));
        editor.apply_key(Key::Char('i'));
        editor.apply_key(Key::Backspace);
        assert_eq!(editor.buffer().line(0), Some("h"));
        assert_eq!(editor.cursor(), Position::new(0, 1));
    }

    #[test]
    fn test_navigation_normal_mode() {
        let mut editor = EditorCore::new();
        editor.load_content("hello\nworld".into());

        editor.apply_key(Key::J); // Move down
        assert_eq!(editor.cursor().row, 1);

        editor.apply_key(Key::L); // Move right
        assert_eq!(editor.cursor().col, 1);

        editor.apply_key(Key::K); // Move up
        assert_eq!(editor.cursor().row, 0);

        editor.apply_key(Key::H); // Move left
        assert_eq!(editor.cursor().col, 0);
    }

    #[test]
    fn test_delete_char() {
        let mut editor = EditorCore::new();
        editor.load_content("hello".into());
        editor.apply_key(Key::X); // Delete char
        assert_eq!(editor.buffer().line(0), Some("ello"));
        assert!(editor.dirty());
    }

    #[test]
    fn test_delete_line() {
        let mut editor = EditorCore::new();
        editor.load_content("line1\nline2\nline3".into());
        editor.apply_key(Key::J); // Move to line 2
        editor.apply_key(Key::D); // Delete line
        assert_eq!(editor.buffer().line_count(), 2);
        assert_eq!(editor.buffer().line(0), Some("line1"));
        assert_eq!(editor.buffer().line(1), Some("line3"));
    }

    #[test]
    fn test_undo_redo() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('h'));
        editor.apply_key(Key::Escape);

        // Undo
        let outcome = editor.apply_key(Key::U);
        assert!(matches!(outcome, CoreOutcome::StatusMessage(_)));
        assert_eq!(editor.buffer().line(0), Some(""));

        // Redo
        let outcome = editor.apply_key(Key::CtrlR);
        assert!(matches!(outcome, CoreOutcome::StatusMessage(_)));
        assert_eq!(editor.buffer().line(0), Some("h"));
    }

    #[test]
    fn test_command_mode() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::Colon);
        assert_eq!(editor.mode(), EditorMode::Command);

        editor.apply_key(Key::Char('q'));
        assert_eq!(editor.command_buffer(), "q");

        editor.apply_key(Key::Backspace);
        assert_eq!(editor.command_buffer(), "");
    }

    #[test]
    fn test_quit_command() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::Colon);
        editor.apply_key(Key::Char('q'));
        let outcome = editor.apply_key(Key::Enter);
        assert_eq!(outcome, CoreOutcome::RequestExit { forced: false });
    }

    #[test]
    fn test_quit_dirty_fails() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('x'));
        editor.apply_key(Key::Escape);

        editor.apply_key(Key::Colon);
        editor.apply_key(Key::Char('q'));
        let outcome = editor.apply_key(Key::Enter);
        assert!(matches!(outcome, CoreOutcome::StatusMessage(_)));
    }

    #[test]
    fn test_force_quit() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('x'));
        editor.apply_key(Key::Escape);

        editor.apply_key(Key::Colon);
        editor.apply_key(Key::Char('q'));
        editor.apply_key(Key::Char('!'));
        let outcome = editor.apply_key(Key::Enter);
        assert_eq!(outcome, CoreOutcome::RequestExit { forced: true });
    }

    #[test]
    fn test_write_command() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::Colon);
        editor.apply_key(Key::Char('w'));
        let outcome = editor.apply_key(Key::Enter);
        assert_eq!(outcome, CoreOutcome::RequestIo(CoreIoRequest::Save));
    }

    #[test]
    fn test_write_quit_command() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('x'));
        editor.apply_key(Key::Escape);

        editor.apply_key(Key::Colon);
        editor.apply_key(Key::Char('w'));
        editor.apply_key(Key::Char('q'));
        let outcome = editor.apply_key(Key::Enter);
        assert_eq!(
            outcome,
            CoreOutcome::RequestIo(CoreIoRequest::SaveAndQuit)
        );
    }

    #[test]
    fn test_search_mode() {
        let mut editor = EditorCore::new();
        editor.load_content("hello world\nfoo bar".into());

        editor.apply_key(Key::Slash);
        assert_eq!(editor.mode(), EditorMode::Search);

        editor.apply_key(Key::Char('w'));
        editor.apply_key(Key::Char('o'));
        assert_eq!(editor.search_query(), "wo");

        let outcome = editor.apply_key(Key::Enter);
        assert_eq!(outcome, CoreOutcome::Changed);
        assert_eq!(editor.cursor(), Position::new(0, 6)); // "world" starts at col 6
    }

    #[test]
    fn test_search_not_found() {
        let mut editor = EditorCore::new();
        editor.load_content("hello world".into());

        editor.apply_key(Key::Slash);
        editor.apply_key(Key::Char('z'));
        let outcome = editor.apply_key(Key::Enter);
        assert!(matches!(outcome, CoreOutcome::StatusMessage(_)));
    }

    #[test]
    fn test_repeat_search() {
        let mut editor = EditorCore::new();
        editor.load_content("foo bar foo baz".into());

        // First search
        editor.apply_key(Key::Slash);
        editor.apply_key(Key::Char('f'));
        editor.apply_key(Key::Char('o'));
        editor.apply_key(Key::Char('o'));
        editor.apply_key(Key::Enter);
        assert_eq!(editor.cursor().col, 0); // First "foo"

        // Repeat search with 'n'
        editor.apply_key(Key::N);
        assert_eq!(editor.cursor().col, 8); // Second "foo"
    }

    #[test]
    fn test_search_wraps() {
        let mut editor = EditorCore::new();
        editor.load_content("foo bar\nbaz foo".into());

        // Move past the last "foo"
        editor.apply_key(Key::J); // Move to line 1
        // Move to end of line 1
        for _ in 0..7 {
            editor.apply_key(Key::L);
        }
        // Now at (1, 6) or (1, 7), past "baz foo"

        // Search should wrap to beginning
        editor.apply_key(Key::Slash);
        editor.apply_key(Key::Char('f'));
        editor.apply_key(Key::Char('o'));
        editor.apply_key(Key::Char('o'));
        editor.apply_key(Key::Enter);
        assert_eq!(editor.cursor(), Position::new(0, 0)); // Wrapped to first "foo"
    }

    #[test]
    fn test_snapshot() {
        let mut editor = EditorCore::new();
        editor.load_content("test".into());
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('!'));

        let snapshot = editor.snapshot();
        assert_eq!(snapshot.mode, EditorMode::Insert);
        assert_eq!(snapshot.cursor, Position::new(0, 1));
        assert_eq!(snapshot.buffer_lines.len(), 1);
        assert!(snapshot.dirty);
    }

    #[test]
    fn test_load_content_clears_undo() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('x'));
        editor.apply_key(Key::Escape);

        assert_eq!(editor.undo_stack.len(), 1);

        editor.load_content("new content".into());
        assert_eq!(editor.undo_stack.len(), 0);
        assert!(!editor.dirty());
    }

    #[test]
    fn test_mark_saved() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);
        editor.apply_key(Key::Char('x'));
        assert!(editor.dirty());

        editor.mark_saved();
        assert!(!editor.dirty());
    }

    #[test]
    fn test_append_mode() {
        let mut editor = EditorCore::new();
        editor.load_content("hi".into());
        editor.apply_key(Key::A); // Append
        assert_eq!(editor.mode(), EditorMode::Insert);
        assert_eq!(editor.cursor(), Position::new(0, 1)); // After current char
    }

    #[test]
    fn test_escape_cancels_command() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::Colon);
        editor.apply_key(Key::Char('q'));
        editor.apply_key(Key::Escape);
        assert_eq!(editor.mode(), EditorMode::Normal);
        assert_eq!(editor.command_buffer(), "");
    }

    #[test]
    fn test_escape_cancels_search() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::Slash);
        editor.apply_key(Key::Char('x'));
        editor.apply_key(Key::Escape);
        assert_eq!(editor.mode(), EditorMode::Normal);
        assert_eq!(editor.search_query(), "");
    }

    #[test]
    fn test_undo_limit() {
        let mut editor = EditorCore::new();
        editor.apply_key(Key::I);

        // Add more than MAX_UNDO_STACK edits
        for _ in 0..150 {
            editor.apply_key(Key::Char('x'));
        }

        // Stack should be limited
        assert!(editor.undo_stack.len() <= 100);
    }
}
