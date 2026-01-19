//! Main editor implementation

use crate::commands::{Command, CommandError, CommandParser};
use crate::io::{DocumentHandle, IoError};
use crate::render::EditorView;
use crate::state::{EditorMode, EditorState, Position};
use input_types::{InputEvent, KeyCode, KeyEvent};
use services_storage::VersionId;
use thiserror::Error;

/// Editor error
#[derive(Debug, Error)]
pub enum EditorError {
    #[error("I/O error: {0}")]
    Io(#[from] IoError),

    #[error("Command error: {0}")]
    Command(#[from] CommandError),

    #[error("Not supported: {0}")]
    NotSupported(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),
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
    view: EditorView,
}

impl Editor {
    /// Create a new editor
    pub fn new() -> Self {
        Self {
            state: EditorState::new(),
            document: None,
            view: EditorView::default(),
        }
    }

    /// Create editor with specified viewport size
    pub fn with_viewport(viewport_lines: usize) -> Self {
        Self {
            state: EditorState::new(),
            document: None,
            view: EditorView::new(viewport_lines),
        }
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
                self.state.set_mode(EditorMode::Insert);
                self.state.set_status_message("");
                Ok(EditorAction::Continue)
            }

            // Delete character under cursor (only without modifiers)
            KeyCode::X if event.modifiers.is_empty() => {
                let pos = self.state.cursor().position();
                if self.state.buffer_mut().delete_char(pos) {
                    self.state.mark_dirty();
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

    /// Execute a parsed command
    fn execute_command(&mut self, cmd_str: &str) -> EditorResult<EditorAction> {
        let command = CommandParser::parse(cmd_str)?;

        match command {
            Command::Write => {
                // Simulated save for now
                let new_version = VersionId::new();
                self.state.set_dirty(false);
                self.state
                    .set_status_message(format!("Saved version {}", new_version));
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
                // Simulated save
                let new_version = VersionId::new();
                self.state.set_dirty(false);
                self.state
                    .set_status_message(format!("Saved version {}", new_version));
                Ok(EditorAction::Quit)
            }
        }
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
}
