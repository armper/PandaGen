//! Main editor implementation

use crate::commands::{Command, CommandError, CommandParser};
use crate::io::{DocumentHandle, EditorIo, IoError, OpenOptions};
use crate::render::EditorView;
use crate::state::{EditorMode, EditorState, Position};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use input_types::{InputEvent, KeyCode, KeyEvent};
use services_storage::{ObjectId, VersionId};
use services_view_host::{ViewHandleCap, ViewHost};
use view_types::{CursorPosition, ViewContent, ViewFrame};

/// Editor error
#[derive(Debug)]
pub enum EditorError {
    Io(IoError),
    Command(CommandError),
    NotSupported(String),
    InvalidState(String),
    ViewError(String),
}

impl fmt::Display for EditorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditorError::Io(e) => write!(f, "I/O error: {}", e),
            EditorError::Command(e) => write!(f, "Command error: {}", e),
            EditorError::NotSupported(s) => write!(f, "Not supported: {}", s),
            EditorError::InvalidState(s) => write!(f, "Invalid state: {}", s),
            EditorError::ViewError(s) => write!(f, "View error: {}", s),
        }
    }
}

impl From<IoError> for EditorError {
    fn from(e: IoError) -> Self {
        EditorError::Io(e)
    }
}

impl From<CommandError> for EditorError {
    fn from(e: CommandError) -> Self {
        EditorError::Command(e)
    }
}

/// Editor result
pub type EditorResult<T> = Result<T, EditorError>;

/// Editor action result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorAction {
    /// Continue editing
    Continue,
    /// Quit the editor
    Quit,
    /// Document was saved
    Saved(VersionId),
}

/// The vi-like editor
pub struct Editor {
    state: EditorState,
    document: Option<DocumentHandle>,
    io: Option<Box<dyn EditorIo>>,
    view: EditorView,
    /// View handles for publishing (optional)
    main_view_handle: Option<ViewHandleCap>,
    status_view_handle: Option<ViewHandleCap>,
    /// Current revision for view frames
    main_view_revision: u64,
    status_view_revision: u64,
}

impl Editor {
    /// Create a new editor
    pub fn new() -> Self {
        Self {
            state: EditorState::new(),
            document: None,
            io: None,
            view: EditorView::default(),
            main_view_handle: None,
            status_view_handle: None,
            main_view_revision: 1,
            status_view_revision: 1,
        }
    }

    /// Create editor with specified viewport size
    pub fn with_viewport(viewport_lines: usize) -> Self {
        Self {
            state: EditorState::new(),
            document: None,
            io: None,
            view: EditorView::new(viewport_lines),
            main_view_handle: None,
            status_view_handle: None,
            main_view_revision: 1,
            status_view_revision: 1,
        }
    }

    /// Set view handles for publishing
    pub fn set_view_handles(&mut self, main_view: ViewHandleCap, status_view: ViewHandleCap) {
        self.main_view_handle = Some(main_view);
        self.status_view_handle = Some(status_view);
    }

    /// Sets the editor I/O handler (storage/fs_view).
    pub fn set_io(&mut self, io: Box<dyn EditorIo>) {
        self.io = Some(io);
    }

    /// Get current editor state
    pub fn state(&self) -> &EditorState {
        &self.state
    }

    /// Get mutable editor state
    pub fn state_mut(&mut self) -> &mut EditorState {
        &mut self.state
    }

    /// Get current document handle
    pub fn document(&self) -> Option<&DocumentHandle> {
        self.document.as_ref()
    }

    /// Opens a document via the configured I/O handler.
    pub fn open_with(&mut self, options: OpenOptions) -> EditorResult<()> {
        let io = self
            .io
            .as_mut()
            .ok_or_else(|| EditorError::NotSupported("No I/O handler configured".to_string()))?;

        match io.open(options.clone()) {
            Ok(result) => {
                self.load_document(result.content, result.handle);
                Ok(())
            }
            Err(IoError::NotFound) => {
                // File not found - create empty buffer with path as target
                if let Some(path) = options.path {
                    self.state.set_document_label(Some(path.clone()));
                    self.state
                        .set_status_message(format!("[New File] {}", path));
                    // Don't set a document handle yet - will be created on save
                } else {
                    self.state.set_status_message("[New File]");
                }
                Ok(())
            }
            Err(IoError::PermissionDenied(reason)) => {
                self.state
                    .set_status_message(format!("Permission denied: {}", reason));
                Err(EditorError::Io(IoError::PermissionDenied(reason)))
            }
            Err(err) => {
                self.state.set_status_message(format!("Error: {}", err));
                Err(EditorError::Io(err))
            }
        }
    }

    /// Open a new empty document
    pub fn new_document(&mut self) {
        self.state = EditorState::new();
        self.document = None;
        self.state.set_status_message("New document");
    }

    /// Load document content (simulated for now, real I/O would use storage service)
    pub fn load_document(&mut self, content: String, handle: DocumentHandle) {
        let label = handle.path_label.clone();
        self.state.load_content(content);
        self.state.set_document_label(label);
        self.document = Some(handle);
        self.state.set_status_message("Document loaded");
    }

    /// Process an input event
    pub fn process_input(&mut self, event: InputEvent) -> EditorResult<EditorAction> {
        // Only process key press events
        let key_event = match event.as_key() {
            Some(ke) if ke.is_pressed() => ke,
            _ => return Ok(EditorAction::Continue),
        };

        match self.state.mode() {
            EditorMode::Normal => self.handle_normal_mode(key_event),
            EditorMode::Insert => self.handle_insert_mode(key_event),
            EditorMode::Command => self.handle_command_mode(key_event),
            EditorMode::Search => self.handle_search_mode(key_event),
        }
    }

    /// Handle normal mode key event
    fn handle_normal_mode(&mut self, event: &KeyEvent) -> EditorResult<EditorAction> {
        match event.code {
            // Navigation (only without modifiers)
            KeyCode::H | KeyCode::Left if event.modifiers.is_empty() => {
                self.state.move_cursor_left();
                Ok(EditorAction::Continue)
            }
            KeyCode::J | KeyCode::Down if event.modifiers.is_empty() => {
                self.state.move_cursor_down();
                Ok(EditorAction::Continue)
            }
            KeyCode::K | KeyCode::Up if event.modifiers.is_empty() => {
                self.state.move_cursor_up();
                Ok(EditorAction::Continue)
            }
            KeyCode::L | KeyCode::Right if event.modifiers.is_empty() => {
                self.state.move_cursor_right();
                Ok(EditorAction::Continue)
            }

            // Enter insert mode (only without modifiers)
            KeyCode::I if event.modifiers.is_empty() => {
                self.state.save_undo_snapshot();
                self.state.set_mode(EditorMode::Insert);
                self.state.set_status_message("");
                Ok(EditorAction::Continue)
            }

            // Delete character under cursor (only without modifiers)
            KeyCode::X if event.modifiers.is_empty() => {
                self.state.save_undo_snapshot();
                let pos = self.state.cursor().position();
                if self.state.buffer_mut().delete_char(pos) {
                    self.state.mark_dirty();
                    self.state.mark_line_dirty(pos.row);
                }
                Ok(EditorAction::Continue)
            }

            // Undo (only without modifiers)
            KeyCode::U if event.modifiers.is_empty() => {
                if self.state.undo() {
                    self.state.set_status_message("Undo");
                    // Mark all lines dirty on undo
                    self.state.mark_all_dirty(100);
                } else {
                    self.state.set_status_message("Already at oldest change");
                }
                Ok(EditorAction::Continue)
            }

            // Redo (Ctrl+R)
            KeyCode::R if event.modifiers.is_ctrl() => {
                if self.state.redo() {
                    self.state.set_status_message("Redo");
                    // Mark all lines dirty on redo
                    self.state.mark_all_dirty(100);
                } else {
                    self.state.set_status_message("Already at newest change");
                }
                Ok(EditorAction::Continue)
            }

            // Enter command mode
            KeyCode::Semicolon if event.modifiers.is_shift() => {
                // Shift+; = ':'
                self.state.set_mode(EditorMode::Command);
                self.state.set_status_message("");
                Ok(EditorAction::Continue)
            }

            // Enter search mode
            KeyCode::Slash if event.modifiers.is_empty() => {
                self.state.set_mode(EditorMode::Search);
                self.state.set_status_message("");
                Ok(EditorAction::Continue)
            }

            // Repeat last search
            KeyCode::N if event.modifiers.is_empty() => {
                if self.state.find_next(true) {
                    self.state.set_status_message("Next match");
                } else {
                    self.state.set_status_message("Pattern not found");
                }
                Ok(EditorAction::Continue)
            }

            _ => Ok(EditorAction::Continue),
        }
    }

    /// Handle insert mode key event
    fn handle_insert_mode(&mut self, event: &KeyEvent) -> EditorResult<EditorAction> {
        match event.code {
            // Exit insert mode
            KeyCode::Escape => {
                self.state.set_mode(EditorMode::Normal);
                Ok(EditorAction::Continue)
            }

            // Newline
            KeyCode::Enter => {
                let pos = self.state.cursor().position();
                if self.state.buffer_mut().insert_newline(pos) {
                    self.state.mark_dirty();
                    // Mark current line and all following lines dirty (due to line shift)
                    let viewport_end = pos.row + 20; // Mark next 20 lines
                    self.state.mark_lines_dirty(pos.row, viewport_end);
                    let new_pos = Position::new(pos.row + 1, 0);
                    self.state.cursor_mut().set_position(new_pos);
                }
                Ok(EditorAction::Continue)
            }

            // Backspace
            KeyCode::Backspace => {
                let pos = self.state.cursor().position();
                if let Some(new_pos) = self.state.buffer_mut().backspace(pos) {
                    self.state.mark_dirty();
                    // If we joined lines, mark current and following lines dirty
                    if new_pos.row < pos.row {
                        let viewport_end = new_pos.row + 20;
                        self.state.mark_lines_dirty(new_pos.row, viewport_end);
                    } else {
                        // Just backspaced on same line
                        self.state.mark_line_dirty(pos.row);
                    }
                    self.state.cursor_mut().set_position(new_pos);
                }
                Ok(EditorAction::Continue)
            }

            // Printable characters
            _ => {
                if let Some(ch) = self.key_to_char(event) {
                    let pos = self.state.cursor().position();
                    if self.state.buffer_mut().insert_char(pos, ch) {
                        self.state.mark_dirty();
                        // Mark current line dirty (text shifted to the right)
                        self.state.mark_line_dirty(pos.row);
                        let new_pos = Position::new(pos.row, pos.col + 1);
                        self.state.cursor_mut().set_position(new_pos);
                    }
                }
                Ok(EditorAction::Continue)
            }
        }
    }

    /// Handle command mode key event
    fn handle_command_mode(&mut self, event: &KeyEvent) -> EditorResult<EditorAction> {
        match event.code {
            // Exit command mode
            KeyCode::Escape => {
                self.state.set_mode(EditorMode::Normal);
                self.state.clear_command();
                Ok(EditorAction::Continue)
            }

            // Execute command
            KeyCode::Enter => {
                let cmd_str = self.state.command_buffer().to_string();
                self.state.clear_command();
                self.state.set_mode(EditorMode::Normal);
                self.execute_command(&cmd_str)
            }

            // Backspace
            KeyCode::Backspace => {
                self.state.backspace_command();
                Ok(EditorAction::Continue)
            }

            // Build command
            _ => {
                if let Some(ch) = self.key_to_char(event) {
                    self.state.append_to_command(ch);
                }
                Ok(EditorAction::Continue)
            }
        }
    }

    /// Handle search mode key event
    fn handle_search_mode(&mut self, event: &KeyEvent) -> EditorResult<EditorAction> {
        match event.code {
            // Exit search mode
            KeyCode::Escape => {
                self.state.set_mode(EditorMode::Normal);
                self.state.clear_search();
                Ok(EditorAction::Continue)
            }

            // Execute search
            KeyCode::Enter => {
                // Execute search before changing mode (mode change clears search_query)
                let found = self.state.find_next(true);
                self.state.set_mode(EditorMode::Normal);
                if found {
                    self.state.set_status_message("Match found");
                } else {
                    self.state.set_status_message("Pattern not found");
                }
                Ok(EditorAction::Continue)
            }

            // Backspace
            KeyCode::Backspace => {
                self.state.backspace_search();
                Ok(EditorAction::Continue)
            }

            // Build search query
            _ => {
                if let Some(ch) = self.key_to_char(event) {
                    self.state.append_to_search(ch);
                }
                Ok(EditorAction::Continue)
            }
        }
    }

    /// Execute a parsed command
    fn execute_command(&mut self, cmd_str: &str) -> EditorResult<EditorAction> {
        let command = CommandParser::parse(cmd_str)?;

        match command {
            Command::Write => {
                let new_version = self.save_document()?;
                Ok(EditorAction::Saved(new_version))
            }

            Command::WriteAs { path } => {
                let new_version = self.save_document_as(&path)?;
                Ok(EditorAction::Saved(new_version))
            }

            Command::Quit => {
                if self.state.is_dirty() {
                    self.state
                        .set_status_message("No write since last change (use :q! to force)");
                    Ok(EditorAction::Continue)
                } else {
                    Ok(EditorAction::Quit)
                }
            }

            Command::ForceQuit => Ok(EditorAction::Quit),

            Command::WriteQuit => {
                let _ = self.save_document()?;
                Ok(EditorAction::Quit)
            }

            Command::Edit { path } => {
                // For now, set a status message that :e is not yet fully implemented
                // In a full implementation, this would:
                // 1. Check if current buffer is dirty and prompt if needed
                // 2. Request a file capability for the new path
                // 3. Load the new file content
                // 4. Replace the current buffer
                self.state.set_status_message(format!(
                    ":e not yet fully implemented (would open {})",
                    path
                ));
                Ok(EditorAction::Continue)
            }
        }
    }

    fn save_document(&mut self) -> EditorResult<VersionId> {
        if let (Some(io), Some(handle)) = (self.io.as_mut(), self.document.clone()) {
            let content = self.state.buffer().as_string();
            let result = io.save(&handle, &content)?;
            let new_handle = DocumentHandle::new(
                handle.object_id,
                result.new_version_id,
                handle.path_label.clone(),
                handle.can_update_link,
            );
            self.document = Some(new_handle);
            self.state.set_dirty(false);
            self.state.set_status_message(result.message);
            Ok(result.new_version_id)
        } else if self.document.is_none() && self.io.is_none() {
            // No document and no I/O - fallback to simple save (for tests)
            let new_version = VersionId::new();
            self.state.set_dirty(false);
            self.state
                .set_status_message(format!("Saved version {}", new_version));
            Ok(new_version)
        } else if self.io.is_some() {
            // Have I/O but no document - suggest Save As
            self.state
                .set_status_message("No file name (use :w <filename>)".to_string());
            Err(EditorError::InvalidState("No file name".to_string()))
        } else {
            // Have document but no I/O handler
            self.state
                .set_status_message("No I/O handler configured".to_string());
            Err(EditorError::NotSupported(
                "No I/O handler configured".to_string(),
            ))
        }
    }

    fn save_document_as(&mut self, path: &str) -> EditorResult<VersionId> {
        let io = self
            .io
            .as_mut()
            .ok_or_else(|| EditorError::NotSupported("No I/O handler configured".to_string()))?;

        let content = self.state.buffer().as_string();
        let result = io.save_as(path, &content)?;

        // Update document handle with new path
        let new_handle = DocumentHandle::new(
            ObjectId::new(), // New object created
            result.new_version_id,
            Some(path.to_string()),
            true, // Can update link since we just created it
        );
        self.document = Some(new_handle);
        self.state.set_dirty(false);
        self.state.set_status_message(result.message);
        Ok(result.new_version_id)
    }

    /// Convert key event to character (simple mapping)
    fn key_to_char(&self, event: &KeyEvent) -> Option<char> {
        let shift = event.modifiers.is_shift();

        match event.code {
            // Letters
            KeyCode::A => Some(if shift { 'A' } else { 'a' }),
            KeyCode::B => Some(if shift { 'B' } else { 'b' }),
            KeyCode::C => Some(if shift { 'C' } else { 'c' }),
            KeyCode::D => Some(if shift { 'D' } else { 'd' }),
            KeyCode::E => Some(if shift { 'E' } else { 'e' }),
            KeyCode::F => Some(if shift { 'F' } else { 'f' }),
            KeyCode::G => Some(if shift { 'G' } else { 'g' }),
            KeyCode::H => Some(if shift { 'H' } else { 'h' }),
            KeyCode::I => Some(if shift { 'I' } else { 'i' }),
            KeyCode::J => Some(if shift { 'J' } else { 'j' }),
            KeyCode::K => Some(if shift { 'K' } else { 'k' }),
            KeyCode::L => Some(if shift { 'L' } else { 'l' }),
            KeyCode::M => Some(if shift { 'M' } else { 'm' }),
            KeyCode::N => Some(if shift { 'N' } else { 'n' }),
            KeyCode::O => Some(if shift { 'O' } else { 'o' }),
            KeyCode::P => Some(if shift { 'P' } else { 'p' }),
            KeyCode::Q => Some(if shift { 'Q' } else { 'q' }),
            KeyCode::R => Some(if shift { 'R' } else { 'r' }),
            KeyCode::S => Some(if shift { 'S' } else { 's' }),
            KeyCode::T => Some(if shift { 'T' } else { 't' }),
            KeyCode::U => Some(if shift { 'U' } else { 'u' }),
            KeyCode::V => Some(if shift { 'V' } else { 'v' }),
            KeyCode::W => Some(if shift { 'W' } else { 'w' }),
            KeyCode::X => Some(if shift { 'X' } else { 'x' }),
            KeyCode::Y => Some(if shift { 'Y' } else { 'y' }),
            KeyCode::Z => Some(if shift { 'Z' } else { 'z' }),

            // Numbers
            KeyCode::Num0 => Some(if shift { ')' } else { '0' }),
            KeyCode::Num1 => Some(if shift { '!' } else { '1' }),
            KeyCode::Num2 => Some(if shift { '@' } else { '2' }),
            KeyCode::Num3 => Some(if shift { '#' } else { '3' }),
            KeyCode::Num4 => Some(if shift { '$' } else { '4' }),
            KeyCode::Num5 => Some(if shift { '%' } else { '5' }),
            KeyCode::Num6 => Some(if shift { '^' } else { '6' }),
            KeyCode::Num7 => Some(if shift { '&' } else { '7' }),
            KeyCode::Num8 => Some(if shift { '*' } else { '8' }),
            KeyCode::Num9 => Some(if shift { '(' } else { '9' }),

            // Space
            KeyCode::Space => Some(' '),

            // Punctuation
            KeyCode::Period => Some(if shift { '>' } else { '.' }),
            KeyCode::Comma => Some(if shift { '<' } else { ',' }),
            KeyCode::Slash => Some(if shift { '?' } else { '/' }),
            KeyCode::Semicolon => Some(if shift { ':' } else { ';' }),
            KeyCode::Quote => Some(if shift { '"' } else { '\'' }),
            KeyCode::LeftBracket => Some(if shift { '{' } else { '[' }),
            KeyCode::RightBracket => Some(if shift { '}' } else { ']' }),
            KeyCode::Backslash => Some(if shift { '|' } else { '\\' }),
            KeyCode::Minus => Some(if shift { '_' } else { '-' }),
            KeyCode::Equal => Some(if shift { '+' } else { '=' }),
            KeyCode::Grave => Some(if shift { '~' } else { '`' }),

            _ => None,
        }
    }

    /// Render the editor view
    pub fn render(&self) -> String {
        self.view.render(&self.state)
    }

    /// Get buffer content as string
    pub fn get_content(&self) -> String {
        self.state.buffer().as_string()
    }

    /// Publishes the current editor state to views
    ///
    /// Call this after processing input or state changes to update the views.
    pub fn publish_views(
        &mut self,
        view_host: &mut ViewHost,
        timestamp_ns: u64,
    ) -> Result<(), EditorError> {
        // Publish main view (buffer content)
        if let Some(handle) = &self.main_view_handle {
            let buffer = self.state.buffer();
            let lines: Vec<String> = (0..buffer.line_count())
                .filter_map(|i| buffer.line(i).map(|s| s.to_string()))
                .collect();

            let content = ViewContent::text_buffer(lines);
            let cursor_pos = self.state.cursor().position();
            let cursor = CursorPosition::new(cursor_pos.row, cursor_pos.col);

            let frame = ViewFrame::new(
                handle.view_id,
                view_types::ViewKind::TextBuffer,
                self.main_view_revision,
                content,
                timestamp_ns,
            )
            .with_cursor(cursor);

            view_host
                .publish_frame(handle, frame)
                .map_err(|e| EditorError::ViewError(e.to_string()))?;

            self.main_view_revision += 1;
        }

        // Publish status view
        if let Some(handle) = &self.status_view_handle {
            let status_text = self.view.render_status(&self.state);
            let content = ViewContent::status_line(status_text);

            let frame = ViewFrame::new(
                handle.view_id,
                view_types::ViewKind::StatusLine,
                self.status_view_revision,
                content,
                timestamp_ns,
            );

            view_host
                .publish_frame(handle, frame)
                .map_err(|e| EditorError::ViewError(e.to_string()))?;

            self.status_view_revision += 1;
        }

        Ok(())
    }

    /// Convenience method to process input and publish views
    pub fn process_input_and_publish(
        &mut self,
        event: InputEvent,
        view_host: &mut ViewHost,
        timestamp_ns: u64,
    ) -> EditorResult<EditorAction> {
        let action = self.process_input(event)?;
        self.publish_views(view_host, timestamp_ns)?;
        Ok(action)
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::Modifiers;
    use services_storage::ObjectId;

    fn press_key(code: KeyCode) -> InputEvent {
        InputEvent::key(KeyEvent::pressed(code, Modifiers::none()))
    }

    fn press_key_shift(code: KeyCode) -> InputEvent {
        InputEvent::key(KeyEvent::pressed(code, Modifiers::SHIFT))
    }

    #[test]
    fn test_editor_new() {
        let editor = Editor::new();
        assert_eq!(editor.state().mode(), EditorMode::Normal);
        assert!(!editor.state().is_dirty());
    }

    #[test]
    fn test_new_document() {
        let mut editor = Editor::new();
        editor.state_mut().mark_dirty();

        editor.new_document();

        assert!(!editor.state().is_dirty());
        assert!(editor.state().status_message().contains("New"));
    }

    #[test]
    fn test_enter_insert_mode() {
        let mut editor = Editor::new();

        let result = editor.process_input(press_key(KeyCode::I));
        assert!(result.is_ok());
        assert_eq!(editor.state().mode(), EditorMode::Insert);
    }

    #[test]
    fn test_exit_insert_mode() {
        let mut editor = Editor::new();
        editor.state_mut().set_mode(EditorMode::Insert);

        let result = editor.process_input(press_key(KeyCode::Escape));
        assert!(result.is_ok());
        assert_eq!(editor.state().mode(), EditorMode::Normal);
    }

    #[test]
    fn test_insert_characters() {
        let mut editor = Editor::new();
        editor.state_mut().set_mode(EditorMode::Insert);

        editor.process_input(press_key(KeyCode::H)).unwrap();
        editor.process_input(press_key(KeyCode::E)).unwrap();
        editor.process_input(press_key(KeyCode::L)).unwrap();
        editor.process_input(press_key(KeyCode::L)).unwrap();
        editor.process_input(press_key(KeyCode::O)).unwrap();

        assert_eq!(editor.get_content(), "hello");
        assert!(editor.state().is_dirty());
    }

    #[test]
    fn test_insert_with_shift() {
        let mut editor = Editor::new();
        editor.state_mut().set_mode(EditorMode::Insert);

        editor.process_input(press_key_shift(KeyCode::H)).unwrap();
        editor.process_input(press_key(KeyCode::I)).unwrap();

        assert_eq!(editor.get_content(), "Hi");
    }

    #[test]
    fn test_insert_newline() {
        let mut editor = Editor::new();
        editor.state_mut().set_mode(EditorMode::Insert);

        editor.process_input(press_key(KeyCode::H)).unwrap();
        editor.process_input(press_key(KeyCode::I)).unwrap();
        editor.process_input(press_key(KeyCode::Enter)).unwrap();
        editor.process_input(press_key(KeyCode::B)).unwrap();
        editor.process_input(press_key(KeyCode::Y)).unwrap();
        editor.process_input(press_key(KeyCode::E)).unwrap();

        assert_eq!(editor.get_content(), "hi\nbye");
    }

    #[test]
    fn test_backspace_in_insert_mode() {
        let mut editor = Editor::new();
        editor.state_mut().set_mode(EditorMode::Insert);

        editor.process_input(press_key(KeyCode::H)).unwrap();
        editor.process_input(press_key(KeyCode::E)).unwrap();
        editor.process_input(press_key(KeyCode::L)).unwrap();
        editor.process_input(press_key(KeyCode::Backspace)).unwrap();
        editor.process_input(press_key(KeyCode::Y)).unwrap();

        assert_eq!(editor.get_content(), "hey");
    }

    #[test]
    fn test_navigation_hjkl() {
        let mut editor = Editor::new();
        editor.load_document(
            "hello\nworld".to_string(),
            DocumentHandle::new(ObjectId::new(), VersionId::new(), None, false),
        );

        // Move down
        editor.process_input(press_key(KeyCode::J)).unwrap();
        assert_eq!(editor.state().cursor().position().row, 1);

        // Move right
        editor.process_input(press_key(KeyCode::L)).unwrap();
        assert_eq!(editor.state().cursor().position().col, 1);

        // Move up
        editor.process_input(press_key(KeyCode::K)).unwrap();
        assert_eq!(editor.state().cursor().position().row, 0);

        // Move left
        editor.process_input(press_key(KeyCode::H)).unwrap();
        assert_eq!(editor.state().cursor().position().col, 0);
    }

    #[test]
    fn test_delete_char_normal_mode() {
        let mut editor = Editor::new();
        editor.load_document(
            "hello".to_string(),
            DocumentHandle::new(ObjectId::new(), VersionId::new(), None, false),
        );

        editor.process_input(press_key(KeyCode::X)).unwrap();

        assert_eq!(editor.get_content(), "ello");
        assert!(editor.state().is_dirty());
    }

    #[test]
    fn test_enter_command_mode() {
        let mut editor = Editor::new();

        // Shift+; = ':'
        let result = editor.process_input(press_key_shift(KeyCode::Semicolon));
        assert!(result.is_ok());
        assert_eq!(editor.state().mode(), EditorMode::Command);
    }

    #[test]
    fn test_command_write() {
        let mut editor = Editor::new();
        editor.state_mut().set_mode(EditorMode::Insert);
        editor.process_input(press_key(KeyCode::H)).unwrap();
        editor.process_input(press_key(KeyCode::I)).unwrap();
        editor.state_mut().set_mode(EditorMode::Command);

        editor.state_mut().append_to_command('w');
        let result = editor.process_input(press_key(KeyCode::Enter));

        assert!(matches!(result, Ok(EditorAction::Saved(_))));
        assert!(!editor.state().is_dirty());
    }

    #[test]
    fn test_command_quit_with_changes() {
        let mut editor = Editor::new();
        editor.state_mut().mark_dirty();
        editor.state_mut().set_mode(EditorMode::Command);

        editor.state_mut().append_to_command('q');
        let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

        // Should refuse to quit
        assert_eq!(result, EditorAction::Continue);
        assert!(editor.state().status_message().contains("No write"));
    }

    #[test]
    fn test_command_force_quit() {
        let mut editor = Editor::new();
        editor.state_mut().mark_dirty();
        editor.state_mut().set_mode(EditorMode::Command);

        editor.state_mut().append_to_command('q');
        editor.state_mut().append_to_command('!');
        let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

        assert_eq!(result, EditorAction::Quit);
    }

    #[test]
    fn test_command_write_quit() {
        let mut editor = Editor::new();
        editor.state_mut().mark_dirty();
        editor.state_mut().set_mode(EditorMode::Command);

        editor.state_mut().append_to_command('w');
        editor.state_mut().append_to_command('q');
        let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

        assert_eq!(result, EditorAction::Quit);
        assert!(!editor.state().is_dirty());
    }

    #[test]
    fn test_full_edit_session() {
        let mut editor = Editor::new();

        // Enter insert mode
        editor.process_input(press_key(KeyCode::I)).unwrap();

        // Type "hello"
        editor.process_input(press_key(KeyCode::H)).unwrap();
        editor.process_input(press_key(KeyCode::E)).unwrap();
        editor.process_input(press_key(KeyCode::L)).unwrap();
        editor.process_input(press_key(KeyCode::L)).unwrap();
        editor.process_input(press_key(KeyCode::O)).unwrap();

        // Exit insert mode
        editor.process_input(press_key(KeyCode::Escape)).unwrap();

        assert_eq!(editor.state().mode(), EditorMode::Normal);
        assert_eq!(editor.get_content(), "hello");
        assert!(editor.state().is_dirty());

        // Enter command mode and save
        editor
            .process_input(press_key_shift(KeyCode::Semicolon))
            .unwrap();
        editor.state_mut().append_to_command('w');
        let result = editor.process_input(press_key(KeyCode::Enter)).unwrap();

        assert!(matches!(result, EditorAction::Saved(_)));
        assert!(!editor.state().is_dirty());
    }

    #[test]
    fn test_editor_publish_views() {
        use core_types::TaskId;
        use services_view_host::ViewHost;
        use view_types::ViewKind;

        let mut editor = Editor::new();
        let mut view_host = ViewHost::new();

        // Create views for the editor
        let task_id = TaskId::new();
        let main_view = view_host
            .create_view(
                ViewKind::TextBuffer,
                Some("test-editor".to_string()),
                task_id,
                ipc::ChannelId::new(),
            )
            .unwrap();
        let status_view = view_host
            .create_view(
                ViewKind::StatusLine,
                Some("test-editor-status".to_string()),
                task_id,
                ipc::ChannelId::new(),
            )
            .unwrap();

        editor.set_view_handles(main_view, status_view);

        // Enter insert mode and type
        editor.process_input(press_key(KeyCode::I)).unwrap();
        editor.process_input(press_key(KeyCode::H)).unwrap();
        editor.process_input(press_key(KeyCode::I)).unwrap();

        // Publish views
        editor.publish_views(&mut view_host, 1000).unwrap();

        // Verify main view was published
        let main_frame = view_host.get_latest(main_view.view_id).unwrap();
        assert!(main_frame.is_some());
        let frame = main_frame.unwrap();
        assert_eq!(frame.revision, 1);

        // Verify status view was published
        let status_frame = view_host.get_latest(status_view.view_id).unwrap();
        assert!(status_frame.is_some());
    }

    #[test]
    fn test_editor_view_revision_increments() {
        use core_types::TaskId;
        use services_view_host::ViewHost;
        use view_types::ViewKind;

        let mut editor = Editor::new();
        let mut view_host = ViewHost::new();

        let task_id = TaskId::new();
        let main_view = view_host
            .create_view(
                ViewKind::TextBuffer,
                Some("test".to_string()),
                task_id,
                ipc::ChannelId::new(),
            )
            .unwrap();
        let status_view = view_host
            .create_view(
                ViewKind::StatusLine,
                Some("test-status".to_string()),
                task_id,
                ipc::ChannelId::new(),
            )
            .unwrap();

        editor.set_view_handles(main_view, status_view);

        // Publish multiple times
        editor.publish_views(&mut view_host, 1000).unwrap();
        editor.publish_views(&mut view_host, 2000).unwrap();
        editor.publish_views(&mut view_host, 3000).unwrap();

        // Verify revision increments
        let frame = view_host.get_latest(main_view.view_id).unwrap().unwrap();
        assert_eq!(frame.revision, 3);
    }
}
