//! Command Palette Overlay
//!
//! Minimal overlay UI for the command palette that integrates with the workspace.

#[cfg(not(test))]
extern crate alloc;

#[cfg(not(test))]
use alloc::string::{String, ToString};
#[cfg(not(test))]
use alloc::vec::Vec;

#[cfg(test)]
use std::string::{String, ToString};
#[cfg(test)]
use std::vec::Vec;

use services_command_palette::{CommandDescriptor, CommandId, CommandPalette};

/// Maximum query length for palette search
const MAX_QUERY_LEN: usize = 128;

/// Maximum number of results to display
const MAX_DISPLAYED_RESULTS: usize = 10;

/// Focus target for restoring focus when palette closes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    /// No component focused
    None,
    /// Editor component
    Editor,
    /// CLI/Command component
    Cli,
}

/// State of the command palette overlay
pub struct PaletteOverlayState {
    /// Whether the palette is currently open
    pub open: bool,
    /// Current search query
    pub query: String,
    /// Filtered and sorted results
    pub results: Vec<CommandDescriptor>,
    /// Currently selected result index
    pub selection_index: usize,
    /// Focus target to restore when closing
    pub prev_focus: FocusTarget,
}

impl PaletteOverlayState {
    /// Creates a new palette overlay state
    pub fn new() -> Self {
        Self {
            open: false,
            query: String::new(),
            results: Vec::new(),
            selection_index: 0,
            prev_focus: FocusTarget::None,
        }
    }

    /// Opens the palette
    pub fn open(&mut self, prev_focus: FocusTarget) {
        self.open = true;
        self.query.clear();
        self.results.clear();
        self.selection_index = 0;
        self.prev_focus = prev_focus;
    }

    /// Closes the palette
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.results.clear();
        self.selection_index = 0;
    }

    /// Returns true if the palette is open
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Returns the previously focused target
    pub fn prev_focus(&self) -> FocusTarget {
        self.prev_focus
    }

    /// Updates the query and refreshes results
    pub fn update_query(&mut self, palette: &CommandPalette, query: String) {
        self.query = query;
        self.refresh_results(palette);
    }

    /// Appends a character to the query
    pub fn append_char(&mut self, palette: &CommandPalette, ch: char) {
        if self.query.len() < MAX_QUERY_LEN {
            self.query.push(ch);
            self.refresh_results(palette);
        }
    }

    /// Removes the last character from the query
    pub fn backspace(&mut self, palette: &CommandPalette) {
        if !self.query.is_empty() {
            self.query.pop();
            self.refresh_results(palette);
        }
    }

    /// Refreshes the results based on current query
    fn refresh_results(&mut self, palette: &CommandPalette) {
        if self.query.is_empty() {
            // Show all commands when query is empty
            self.results = palette.list_commands();
        } else {
            // Filter and sort by relevance
            self.results = palette.filter_commands(&self.query);
        }

        // Clamp selection index to valid range
        if !self.results.is_empty() {
            self.selection_index = self.selection_index.min(self.results.len() - 1);
        } else {
            self.selection_index = 0;
        }
    }

    /// Moves selection up
    pub fn move_selection_up(&mut self) {
        if self.selection_index > 0 {
            self.selection_index -= 1;
        }
    }

    /// Moves selection down
    pub fn move_selection_down(&mut self) {
        if !self.results.is_empty() && self.selection_index < self.results.len() - 1 {
            self.selection_index += 1;
        }
    }

    /// Gets the currently selected command ID, if any
    pub fn selected_command(&self) -> Option<&CommandId> {
        self.results.get(self.selection_index).map(|desc| &desc.id)
    }

    /// Gets the results to display (up to MAX_DISPLAYED_RESULTS)
    pub fn displayed_results(&self) -> &[CommandDescriptor] {
        let end = self.results.len().min(MAX_DISPLAYED_RESULTS);
        &self.results[..end]
    }

    /// Gets the current query
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Gets the selection index
    pub fn selection_index(&self) -> usize {
        self.selection_index
    }

    /// Gets the total number of results
    pub fn result_count(&self) -> usize {
        self.results.len()
    }
}

impl Default for PaletteOverlayState {
    fn default() -> Self {
        Self::new()
    }
}

/// Handles a key event for the palette overlay
///
/// Returns true if the event was consumed, false otherwise
pub fn handle_palette_key(
    state: &mut PaletteOverlayState,
    palette: &CommandPalette,
    byte: u8,
) -> PaletteKeyAction {
    match byte {
        0x1B => {
            // Escape - close palette
            PaletteKeyAction::Close
        }
        b'\n' | b'\r' => {
            // Enter - execute selected command
            if let Some(cmd_id) = state.selected_command() {
                PaletteKeyAction::Execute(cmd_id.clone())
            } else {
                PaletteKeyAction::None
            }
        }
        0x08 | 0x7F => {
            // Backspace
            state.backspace(palette);
            PaletteKeyAction::Consumed
        }
        0x1E => {
            // Up arrow (special handling needed - this is 'a' in current parser)
            // For now, we'll use a different approach
            // TODO: Properly handle arrow keys
            PaletteKeyAction::Consumed
        }
        0x1F => {
            // Down arrow (special handling needed)
            // TODO: Properly handle arrow keys
            PaletteKeyAction::Consumed
        }
        byte if byte >= 0x20 && byte < 0x7F => {
            // Printable character
            state.append_char(palette, byte as char);
            PaletteKeyAction::Consumed
        }
        _ => PaletteKeyAction::None,
    }
}

/// Result of handling a palette key event
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteKeyAction {
    /// Event was not handled
    None,
    /// Event was consumed but no special action
    Consumed,
    /// Close the palette
    Close,
    /// Execute the given command
    Execute(CommandId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use services_command_palette::CommandDescriptor;

    fn create_test_palette() -> CommandPalette {
        let mut palette = CommandPalette::new();
        
        palette.register_command(
            CommandDescriptor::new(
                "open_editor",
                "Open Editor",
                "Opens a text editor",
                vec!["editor".to_string(), "edit".to_string()],
            ),
            Box::new(|_| Ok("Editor opened".to_string())),
        );

        palette.register_command(
            CommandDescriptor::new(
                "save_file",
                "Save File",
                "Saves the current file",
                vec!["file".to_string(), "save".to_string()],
            ),
            Box::new(|_| Ok("File saved".to_string())),
        );

        palette.register_command(
            CommandDescriptor::new(
                "quit",
                "Quit",
                "Quit the application",
                vec!["exit".to_string(), "close".to_string()],
            ),
            Box::new(|_| Ok("Quitting".to_string())),
        );

        palette
    }

    #[test]
    fn test_palette_open_close() {
        let mut state = PaletteOverlayState::new();
        
        assert!(!state.is_open());
        
        state.open(FocusTarget::Editor);
        assert!(state.is_open());
        assert_eq!(state.prev_focus(), FocusTarget::Editor);
        
        state.close();
        assert!(!state.is_open());
    }

    #[test]
    fn test_query_update() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.update_query(&palette, "edit".to_string());
        assert_eq!(state.query(), "edit");
        assert!(state.result_count() > 0);
    }

    #[test]
    fn test_append_char() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.append_char(&palette, 'e');
        state.append_char(&palette, 'd');
        state.append_char(&palette, 'i');
        state.append_char(&palette, 't');

        assert_eq!(state.query(), "edit");
    }

    #[test]
    fn test_backspace() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.append_char(&palette, 'a');
        state.append_char(&palette, 'b');
        assert_eq!(state.query(), "ab");

        state.backspace(&palette);
        assert_eq!(state.query(), "a");

        state.backspace(&palette);
        assert_eq!(state.query(), "");

        state.backspace(&palette); // Should not crash
        assert_eq!(state.query(), "");
    }

    #[test]
    fn test_selection_movement() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.update_query(&palette, "".to_string()); // Get all commands

        assert_eq!(state.selection_index(), 0);

        state.move_selection_down();
        assert_eq!(state.selection_index(), 1);

        state.move_selection_down();
        assert_eq!(state.selection_index(), 2);

        state.move_selection_up();
        assert_eq!(state.selection_index(), 1);

        state.move_selection_up();
        assert_eq!(state.selection_index(), 0);

        state.move_selection_up(); // Should stay at 0
        assert_eq!(state.selection_index(), 0);
    }

    #[test]
    fn test_selected_command() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.update_query(&palette, "edit".to_string());
        
        assert!(state.selected_command().is_some());
    }

    #[test]
    fn test_handle_escape() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        let action = handle_palette_key(&mut state, &palette, 0x1B);
        assert_eq!(action, PaletteKeyAction::Close);
    }

    #[test]
    fn test_handle_enter() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.update_query(&palette, "edit".to_string());

        let action = handle_palette_key(&mut state, &palette, b'\n');
        assert!(matches!(action, PaletteKeyAction::Execute(_)));
    }

    #[test]
    fn test_handle_printable() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        let action = handle_palette_key(&mut state, &palette, b'a');
        assert_eq!(action, PaletteKeyAction::Consumed);
        assert_eq!(state.query(), "a");
    }

    #[test]
    fn test_handle_backspace() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.append_char(&palette, 'a');
        state.append_char(&palette, 'b');

        let action = handle_palette_key(&mut state, &palette, 0x08);
        assert_eq!(action, PaletteKeyAction::Consumed);
        assert_eq!(state.query(), "a");
    }

    #[test]
    fn test_empty_query_shows_all() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.update_query(&palette, "".to_string());
        assert_eq!(state.result_count(), 3); // All three commands
    }

    #[test]
    fn test_filtered_results() {
        let palette = create_test_palette();
        let mut state = PaletteOverlayState::new();
        state.open(FocusTarget::None);

        state.update_query(&palette, "edit".to_string());
        assert!(state.result_count() > 0);
        
        // The "Open Editor" command should match
        let results = state.displayed_results();
        assert!(results.iter().any(|r| r.id.as_str() == "open_editor"));
    }
}
