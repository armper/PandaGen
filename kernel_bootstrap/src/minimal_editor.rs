//! Minimal editor for bare-metal execution
//!
//! This is a thin wrapper around editor_core that provides viewport management
//! and rendering logic for bare-metal VGA environment.

#[cfg(not(test))]
extern crate alloc;

#[cfg(not(test))]
use alloc::string::{String, ToString};

#[cfg(test)]
use std::string::String;

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
        // Note: In future capability model:
        // - handle.object_id would indicate existing file (FileCap for READ/WRITE)
        // - handle.path without object_id would indicate new file intent (DirCap for create)
        // - Neither would indicate empty buffer
        // Current simplified model uses ObjectId directly.
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

    /// Resize the viewport (e.g. to the graphical window height), keeping
    /// the cursor visible.
    pub fn set_viewport_rows(&mut self, rows: usize) {
        self.viewport_rows = rows.max(1);
        self.adjust_viewport();
    }

    /// Total lines in the document.
    pub fn line_count(&self) -> usize {
        self.core.buffer().line_count()
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
        // The desk's navigation bytes (KBD-012): arrows and Delete move
        // and edit in every mode, not only hjkl in normal mode.
        let key = match byte {
            crate::notepad::KEY_UP => Key::Up,
            crate::notepad::KEY_DOWN => Key::Down,
            crate::notepad::KEY_LEFT => Key::Left,
            crate::notepad::KEY_RIGHT => Key::Right,
            crate::notepad::KEY_DELETE => Key::Delete,
            _ => match Key::from_ascii(byte) {
                Some(k) => k,
                None => return false, // Unknown key, continue editing
            },
        };

        // Apply key to core
        let outcome = self.core.apply_key(key);

        // Handle outcome
        let should_quit = match outcome {
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
                                            self.status =
                                                String::from("Error: failed to save file");
                                            false
                                        }
                                    }
                                } else {
                                    self.status =
                                        String::from("Error: no file path (use :w <path>)");
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
                                // `:wq` used to throw the result away and
                                // exit regardless. With no path -- which is
                                // what `editor` with no argument gives you --
                                // the save always failed, so everything typed
                                // disappeared without even the message that
                                // plain `:w` prints. Quit only if it saved.
                                let Some(handle) = self.document.clone() else {
                                    self.status =
                                        String::from("Error: no file path (use :w <path>)");
                                    return false;
                                };
                                let content = self.core.buffer().as_string();
                                match io.save(&handle, &content) {
                                    Ok(msg) => {
                                        self.status = msg;
                                        self.core.mark_saved();
                                        true
                                    }
                                    Err(_) => {
                                        self.status = String::from("Error: failed to save file");
                                        false
                                    }
                                }
                            }
                        }
                    } else {
                        // No filesystem. Saying so and then marking the
                        // buffer clean told `:q` the work was safe, and the
                        // very next `:q` threw it away without complaint.
                        // The dirty flag is the only thing standing between
                        // `:q` and losing everything typed.
                        use editor_core::CoreIoRequest;
                        match io_req {
                            CoreIoRequest::Save | CoreIoRequest::SaveAs(_) => {
                                self.status = String::from("Filesystem unavailable");
                                false
                            }
                            CoreIoRequest::SaveAndQuit => {
                                self.status = String::from("Filesystem unavailable");
                                false
                            }
                        }
                    }
                }
                #[cfg(test)]
                {
                    // The same rule as the no-filesystem branch above: a
                    // save that did not happen must not mark the buffer
                    // clean, and must not let `:wq` exit.
                    use editor_core::CoreIoRequest;
                    match io_req {
                        CoreIoRequest::Save | CoreIoRequest::SaveAs(_) => {
                            self.status = String::from("Filesystem unavailable in test mode");
                            false
                        }
                        CoreIoRequest::SaveAndQuit => {
                            self.status = String::from("Filesystem unavailable in test mode");
                            false
                        }
                    }
                }
            }
        };

        if !should_quit {
            self.refresh_prompt_status();
        }

        should_quit
    }

    fn refresh_prompt_status(&mut self) {
        if !self.status.is_empty() {
            return;
        }

        match self.core.mode() {
            EditorMode::Command => {
                self.status.push(':');
                self.status.push_str(self.core.command_buffer());
            }
            EditorMode::Search => {
                self.status.push('/');
                self.status.push_str(self.core.search_query());
            }
            _ => {}
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
        if cursor.row >= self.scroll_offset && cursor.row < self.scroll_offset + self.viewport_rows
        {
            Some(Position::new(cursor.row - self.scroll_offset, cursor.col))
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
        assert_eq!(
            editor.mode(),
            EditorMode::Insert,
            "Should enter INSERT mode after 'i'"
        );

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
        assert_eq!(
            editor.mode(),
            EditorMode::Normal,
            "Should return to NORMAL mode after Escape"
        );

        // Check status line reflects mode (approximately, MinimalEditor logic might vary)
        // In Normal mode it should say "-- NORMAL --" or similar
        let status = editor.status_line();
        assert!(
            status.contains("NORMAL"),
            "Status line should indicate NORMAL mode"
        );
    }
}
