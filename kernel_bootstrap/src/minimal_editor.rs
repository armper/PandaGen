//! Minimal editor for bare-metal execution
//!
//! This is a thin wrapper around editor_core that provides viewport management
//! and rendering logic for bare-metal VGA environment.

#[cfg(not(test))]
extern crate alloc;

#[cfg(not(test))]
use alloc::string::{String, ToString};

#[cfg(test)]
use std::string::{String, ToString};

use editor_core::{CoreOutcome, EditorCore, Key};

// Re-export types from editor_core for test compatibility
pub use editor_core::{EditorMode, Position};

#[cfg(not(test))]
use crate::bare_metal_editor_io::{BareMetalEditorIo, DocumentHandle};

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
    /// Optional EditorIo for file operations
    #[cfg(not(test))]
    pub(crate) editor_io: Option<BareMetalEditorIo>,
    /// Current document handle
    #[cfg(not(test))]
    pub(crate) document: Option<DocumentHandle>,
}

impl MinimalEditor {
    pub fn new(viewport_rows: usize) -> Self {
        Self {
            core: EditorCore::new(),
            viewport_rows,
            scroll_offset: 0,
            status: String::new(),
            #[cfg(not(test))]
            editor_io: None,
            #[cfg(not(test))]
            document: None,
        }
    }
    
    /// Set the EditorIo for this editor session
    #[cfg(not(test))]
    pub fn set_editor_io(&mut self, io: BareMetalEditorIo, handle: DocumentHandle) {
        self.editor_io = Some(io);
        self.document = Some(handle);
        // Note: handle.object_id indicates existing file (file_cap)
        // handle.path without object_id indicates new file intent (dir_cap)
        // Neither indicates empty buffer
    }

    
    /// Load content into the editor
    #[cfg(not(test))]
    pub fn load_content(&mut self, content: &str) {
        self.core.load_content(content.to_string());
        self.core.mark_saved();
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
                #[cfg(not(test))]
                {
                    // Handle filesystem operations if EditorIo is available
                    if let Some(ref mut io) = self.editor_io {
                        use editor_core::CoreIoRequest;
                        match io_req {
                            CoreIoRequest::Save => {
                                if let Some(ref handle) = self.document {
                                    let content = self.core.buffer().as_string();
                                    match io.save(handle, &content) {
                                        Ok(msg) => {
                                            self.status = msg;
                                            self.core.mark_saved();
                                            false
                                        }
                                        Err(_) => {
                                            self.status = String::from("Error: failed to save file");
                                            false
                                        }
                                    }
                                } else {
                                    self.status = String::from("Error: no file path (use :w <path>)");
                                    false
                                }
                            }
                            CoreIoRequest::SaveAs(path) => {
                                let content = self.core.buffer().as_string();
                                match io.save_as(&path, &content) {
                                    Ok((msg, handle)) => {
                                        self.status = msg;
                                        self.document = Some(handle);
                                        self.core.mark_saved();
                                        false
                                    }
                                    Err(_) => {
                                        self.status = String::from("Error: failed to save file");
                                        false
                                    }
                                }
                            }
                            CoreIoRequest::SaveAndQuit => {
                                if let Some(ref handle) = self.document {
                                    let content = self.core.buffer().as_string();
                                    let _ = io.save(handle, &content);
                                }
                                true // Quit anyway
                            }
                        }
                    } else {
                        // No filesystem available - use old behavior
                        use editor_core::CoreIoRequest;
                        match io_req {
                            CoreIoRequest::Save | CoreIoRequest::SaveAs(_) => {
                                self.status = String::from("Filesystem unavailable");
                                self.core.mark_saved();
                                false
                            }
                            CoreIoRequest::SaveAndQuit => {
                                self.status = String::from("Filesystem unavailable");
                                true // Quit anyway
                            }
                        }
                    }
                }
                #[cfg(test)]
                {
                    // In test mode, simulate IO operations
                    use editor_core::CoreIoRequest;
                    match io_req {
                        CoreIoRequest::Save | CoreIoRequest::SaveAs(_) => {
                            self.status = String::from("Filesystem unavailable in test mode");
                            self.core.mark_saved();
                            false
                        }
                        CoreIoRequest::SaveAndQuit => {
                            self.status = String::from("Filesystem unavailable in test mode");
                            true // Quit anyway
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_editor_input_flow() {
        // Create editor with 10 rows viewport
        let mut editor = MinimalEditor::new(10);
        
        // Initial state
        assert_eq!(editor.mode(), EditorMode::Normal);
        assert_eq!(editor.cursor().row, 0);
        assert_eq!(editor.cursor().col, 0);
        
        // Press 'i' to enter insert mode
        editor.process_byte(b'i');
        assert_eq!(editor.mode(), EditorMode::Insert, "Should enter INSERT mode after 'i'");
        
        // Press 'a' to insert character
        editor.process_byte(b'a');
        editor.process_byte(b' ');
        editor.process_byte(b'j');
        
        // Verify content
        // Note: MinimalEditor wraps EditorCore but doesn't expose get_text() directly in pub API.
        // We can check viewport line 0
        let line = editor.get_viewport_line(0);
        assert!(line.is_some(), "Line 0 should exist");
        assert_eq!(line.unwrap(), "a j", "Line 0 should contain 'a j'");
        
        // Verify cursor moved
        assert_eq!(editor.cursor().col, 3, "Cursor should move after typing");
        
        // Press Escape to exit insert mode
        editor.process_byte(0x1B); // Escape
        assert_eq!(editor.mode(), EditorMode::Normal, "Should return to NORMAL mode after Escape");
        
        // Check status line reflects mode (approximately, MinimalEditor logic might vary)
        // In Normal mode it should say "-- NORMAL --" or similar
        let status = editor.status_line();
        assert!(status.contains("NORMAL"), "Status line should indicate NORMAL mode");
    }
}
