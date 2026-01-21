//! Minimal editor for bare-metal execution
//!
//! This is a stripped-down version of services_editor_vi that works in bare-metal
//! environment without std dependencies. It shares the same modal editing philosophy
//! but renders directly to VGA without going through services_view_host.

#![cfg(not(test))]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// Editor mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    /// Normal mode (navigation and commands)
    Normal,
    /// Insert mode (text entry)
    Insert,
    /// Command mode (ex commands)
    Command,
}

impl EditorMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Command => "COMMAND",
        }
    }
}

/// Cursor position in the buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Text buffer
pub struct TextBuffer {
    lines: Vec<String>,
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            lines: {
                let mut v = Vec::new();
                v.push(String::new());
                v
            },
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, index: usize) -> Option<&str> {
        self.lines.get(index).map(|s| s.as_str())
    }

    pub fn line_length(&self, row: usize) -> usize {
        self.lines.get(row).map(|s| s.len()).unwrap_or(0)
    }

    pub fn insert_char(&mut self, pos: Position, ch: char) {
        if let Some(line) = self.lines.get_mut(pos.row) {
            if pos.col <= line.len() {
                line.insert(pos.col, ch);
            }
        }
    }

    pub fn delete_char(&mut self, pos: Position) {
        if let Some(line) = self.lines.get_mut(pos.row) {
            if pos.col < line.len() {
                line.remove(pos.col);
            }
        }
    }

    pub fn insert_newline(&mut self, pos: Position) {
        if pos.row < self.lines.len() {
            let line = &mut self.lines[pos.row];
            let new_line = line.split_off(pos.col);
            self.lines.insert(pos.row + 1, new_line);
        }
    }

    pub fn delete_line(&mut self, row: usize) {
        if row < self.lines.len() && self.lines.len() > 1 {
            self.lines.remove(row);
        } else if row < self.lines.len() {
            self.lines[row].clear();
        }
    }
}

/// Minimal editor state
pub struct MinimalEditor {
    mode: EditorMode,
    buffer: TextBuffer,
    cursor: Position,
    /// Command buffer for : commands
    command_buffer: String,
    /// Status message
    status: String,
    /// Dirty flag
    dirty: bool,
    /// Viewport size (rows that can be displayed)
    viewport_rows: usize,
    /// Scroll offset (first visible row)
    scroll_offset: usize,
}

impl MinimalEditor {
    pub fn new(viewport_rows: usize) -> Self {
        Self {
            mode: EditorMode::Normal,
            buffer: TextBuffer::new(),
            cursor: Position::zero(),
            command_buffer: String::new(),
            status: String::new(),
            dirty: false,
            viewport_rows,
            scroll_offset: 0,
        }
    }

    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    pub fn cursor(&self) -> Position {
        self.cursor
    }

    pub fn viewport_rows(&self) -> usize {
        self.viewport_rows
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn status_line(&self) -> &str {
        if !self.status.is_empty() {
            return &self.status;
        }
        match self.mode {
            EditorMode::Normal => "-- NORMAL --",
            EditorMode::Insert => "-- INSERT --",
            EditorMode::Command => {
                // Return command buffer with prompt
                return &self.command_buffer;
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Process a single byte of input
    /// Returns true if should quit
    pub fn process_byte(&mut self, byte: u8) -> bool {
        self.status.clear();

        match self.mode {
            EditorMode::Normal => self.handle_normal_mode(byte),
            EditorMode::Insert => self.handle_insert_mode(byte),
            EditorMode::Command => self.handle_command_mode(byte),
        }
    }

    fn handle_normal_mode(&mut self, byte: u8) -> bool {
        match byte {
            b'i' => {
                self.mode = EditorMode::Insert;
                false
            }
            b'a' => {
                self.mode = EditorMode::Insert;
                self.move_right();
                false
            }
            b'h' => {
                self.move_left();
                false
            }
            b'j' => {
                self.move_down();
                false
            }
            b'k' => {
                self.move_up();
                false
            }
            b'l' => {
                self.move_right();
                false
            }
            b'x' => {
                self.buffer.delete_char(self.cursor);
                self.dirty = true;
                false
            }
            b'd' => {
                // Simple dd - delete current line
                self.buffer.delete_line(self.cursor.row);
                self.dirty = true;
                if self.cursor.row >= self.buffer.line_count() {
                    self.cursor.row = self.buffer.line_count().saturating_sub(1);
                }
                self.clamp_cursor();
                false
            }
            b':' => {
                self.mode = EditorMode::Command;
                self.command_buffer.clear();
                self.command_buffer.push(':');
                false
            }
            _ => false,
        }
    }

    fn handle_insert_mode(&mut self, byte: u8) -> bool {
        match byte {
            0x1B => {
                // Escape key
                self.mode = EditorMode::Normal;
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                }
                self.clamp_cursor();
                false
            }
            b'\r' | b'\n' => {
                self.buffer.insert_newline(self.cursor);
                self.cursor.row += 1;
                self.cursor.col = 0;
                self.dirty = true;
                self.adjust_viewport();
                false
            }
            0x08 | 0x7F => {
                // Backspace
                if self.cursor.col > 0 {
                    self.cursor.col -= 1;
                    self.buffer.delete_char(self.cursor);
                    self.dirty = true;
                }
                false
            }
            ch if ch >= 0x20 && ch < 0x7F => {
                // Printable ASCII
                self.buffer.insert_char(self.cursor, ch as char);
                self.cursor.col += 1;
                self.dirty = true;
                false
            }
            _ => false,
        }
    }

    fn handle_command_mode(&mut self, byte: u8) -> bool {
        match byte {
            0x1B => {
                // Escape - cancel command
                self.mode = EditorMode::Normal;
                self.command_buffer.clear();
                false
            }
            b'\r' | b'\n' => {
                // Execute command
                let should_quit = self.execute_command();
                if !should_quit {
                    self.mode = EditorMode::Normal;
                }
                should_quit
            }
            0x08 | 0x7F => {
                // Backspace
                if self.command_buffer.len() > 1 {
                    // Keep the ':' prefix
                    self.command_buffer.pop();
                }
                false
            }
            ch if ch >= 0x20 && ch < 0x7F => {
                self.command_buffer.push(ch as char);
                false
            }
            _ => false,
        }
    }

    fn execute_command(&mut self) -> bool {
        let cmd = &self.command_buffer[1..]; // Skip ':' prefix
        match cmd.trim() {
            "q" => {
                if self.dirty {
                    self.status = String::from("No write since last change (use :q! to override)");
                    false
                } else {
                    true // Quit
                }
            }
            "q!" => true, // Force quit
            "w" => {
                self.status = String::from("Filesystem unavailable in bare-metal mode");
                self.dirty = false; // Pretend we saved
                false
            }
            "wq" => {
                self.status = String::from("Filesystem unavailable in bare-metal mode");
                true // Quit anyway
            }
            _ => {
                use core::fmt::Write;
                let mut msg = String::new();
                let _ = write!(msg, "Unknown command: {}", cmd);
                self.status = msg;
                false
            }
        }
    }

    fn move_up(&mut self) {
        if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.clamp_cursor();
            self.adjust_viewport();
        }
    }

    fn move_down(&mut self) {
        if self.cursor.row < self.buffer.line_count().saturating_sub(1) {
            self.cursor.row += 1;
            self.clamp_cursor();
            self.adjust_viewport();
        }
    }

    fn move_left(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        }
    }

    fn move_right(&mut self) {
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

    fn adjust_viewport(&mut self) {
        // Keep cursor in viewport
        if self.cursor.row < self.scroll_offset {
            self.scroll_offset = self.cursor.row;
        } else if self.cursor.row >= self.scroll_offset + self.viewport_rows {
            self.scroll_offset = self.cursor.row - self.viewport_rows + 1;
        }
    }

    /// Get a line from the visible viewport (0 = first visible line)
    pub fn get_viewport_line(&self, viewport_row: usize) -> Option<&str> {
        let buffer_row = self.scroll_offset + viewport_row;
        self.buffer.line(buffer_row)
    }

    /// Get cursor position relative to viewport
    pub fn get_viewport_cursor(&self) -> Option<Position> {
        if self.cursor.row >= self.scroll_offset
            && self.cursor.row < self.scroll_offset + self.viewport_rows
        {
            Some(Position::new(
                self.cursor.row - self.scroll_offset,
                self.cursor.col,
            ))
        } else {
            None
        }
    }
}
