//! Minimal editor for bare-metal execution
//!
//! This is a thin wrapper around editor_core that provides viewport management
//! and rendering logic for bare-metal VGA environment.

#[cfg(not(test))]
extern crate alloc;

#[cfg(not(test))]
use alloc::string::String;

#[cfg(test)]
use std::string::String;

use editor_core::{CoreOutcome, EditorCore, Key};

// Re-export types from editor_core for test compatibility
pub use editor_core::{EditorMode, Position};

/// Minimal editor - thin wrapper around EditorCore with viewport management
pub struct MinimalEditor {
    /// Core editor state machine
    core: EditorCore,
    /// Viewport size (rows that can be displayed)
    viewport_rows: usize,
    /// Scroll offset (first visible row)
    scroll_offset: usize,
    /// Status message (separate from core for rendering)
    status: String,
}

impl MinimalEditor {
    pub fn new(viewport_rows: usize) -> Self {
        Self {
            core: EditorCore::new(),
            viewport_rows,
            scroll_offset: 0,
            status: String::new(),
        }
    }

    /// Get current editor mode
    pub fn mode(&self) -> EditorMode {
        self.core.mode()
    }

    /// Get current cursor position
    pub fn cursor(&self) -> Position {
        self.core.cursor()
    }

    /// Get viewport height
    pub fn viewport_rows(&self) -> usize {
        self.viewport_rows
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Get status line for rendering
    pub fn status_line(&self) -> &str {
        // Priority: local status > core status > command/search buffer > mode display
        if !self.status.is_empty() {
            return &self.status;
        }

        let core_status = self.core.status_message();
        if !core_status.is_empty() {
            return core_status;
        }

        // Show mode-specific status
        match self.core.mode() {
            EditorMode::Normal => "-- NORMAL --",
            EditorMode::Insert => "-- INSERT --",
            EditorMode::Command => "-- COMMAND --",
            EditorMode::Search => "-- SEARCH --",
        }
    }

    /// Check if buffer has unsaved changes
    pub fn is_dirty(&self) -> bool {
        self.core.dirty()
    }

    /// Process a single byte of input
    /// Returns true if should quit
    pub fn process_byte(&mut self, byte: u8) -> bool {
        // Clear local status
        self.status.clear();

        // Convert byte to Key
        let key = match Key::from_ascii(byte) {
            Some(k) => k,
            None => return false, // Unknown key, continue editing
        };

        // Apply key to core
        let outcome = self.core.apply_key(key);

        // Handle outcome
        match outcome {
            CoreOutcome::Continue => false,
            CoreOutcome::Changed => {
                self.adjust_viewport();
                false
            }
            CoreOutcome::RequestExit { .. } => {
                // In bare-metal mode, always quit (no filesystem operations)
                true
            }
            CoreOutcome::StatusMessage(msg) => {
                self.status = msg;
                false
            }
            CoreOutcome::RequestIo(io_req) => {
                // In bare-metal mode, simulate IO operations
                use editor_core::CoreIoRequest;
                match io_req {
                    CoreIoRequest::Save | CoreIoRequest::SaveAs(_) => {
                        self.status = String::from("Filesystem unavailable in bare-metal mode");
                        self.core.mark_saved();
                        false
                    }
                    CoreIoRequest::SaveAndQuit => {
                        self.status = String::from("Filesystem unavailable in bare-metal mode");
                        true // Quit anyway
                    }
                }
            }
        }
    }

    /// Adjust viewport to keep cursor visible
    fn adjust_viewport(&mut self) {
        let cursor_row = self.core.cursor().row;

        // Keep cursor in viewport
        if cursor_row < self.scroll_offset {
            self.scroll_offset = cursor_row;
        } else if cursor_row >= self.scroll_offset + self.viewport_rows {
            self.scroll_offset = cursor_row.saturating_sub(self.viewport_rows - 1);
        }
    }

    /// Get a line from the visible viewport (0 = first visible line)
    pub fn get_viewport_line(&self, viewport_row: usize) -> Option<&str> {
        let buffer_row = self.scroll_offset + viewport_row;
        self.core.buffer().line(buffer_row)
    }

    /// Get cursor position relative to viewport
    pub fn get_viewport_cursor(&self) -> Option<Position> {
        let cursor = self.core.cursor();
        if cursor.row >= self.scroll_offset
            && cursor.row < self.scroll_offset + self.viewport_rows
        {
            Some(Position::new(
                cursor.row - self.scroll_offset,
                cursor.col,
            ))
        } else {
            None
        }
    }
}
