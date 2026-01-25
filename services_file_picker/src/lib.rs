//! # File Picker Service
//!
//! A modern, text-mode file picker component for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Capability-based**: Only browse within provided DirCap
//! - **Deterministic**: Consistent ordering (directories first, then files, lexicographic)
//! - **No string paths**: Uses ObjectId and capabilities, not ambient filesystem access
//! - **Testable**: All navigation state is explicit and deterministic
//! - **Component-based**: Participates in normal focus routing
//!
//! ## Features
//!
//! - Navigate directories with arrow keys (↑/↓)
//! - Enter directories (Enter on directory)
//! - Select files (Enter on file)
//! - Go up one level (Esc or Back)
//! - Deterministic sorting (dirs before files, lexicographic within each)
//!
//! ## Example
//!
//! ```ignore
//! use services_file_picker::{FilePicker, FilePickerResult};
//! use fs_view::DirectoryView;
//!
//! let mut picker = FilePicker::new(root_directory);
//! 
//! // Process input
//! match picker.process_input(key_event) {
//!     FilePickerResult::FileSelected(object_id) => {
//!         // User selected a file
//!     }
//!     FilePickerResult::Cancelled => {
//!         // User cancelled
//!     }
//!     FilePickerResult::Continue => {
//!         // Still navigating
//!     }
//! }
//! ```

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use fs_view::{DirectoryEntry, DirectoryView};
use input_types::{InputEvent, KeyCode, KeyState};
use services_storage::ObjectId;
use services_storage::ObjectKind;
use thiserror::Error;

/// Result of file picker interaction
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePickerResult {
    /// User selected a file
    FileSelected {
        /// Object ID of the selected file
        object_id: ObjectId,
        /// Name of the selected file
        name: String,
    },
    /// User cancelled the picker
    Cancelled,
    /// Continue navigating (no action yet)
    Continue,
}

/// Error type for file picker operations
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FilePickerError {
    #[error("Permission denied: cannot access {0}")]
    PermissionDenied(String),
    
    #[error("Invalid directory: {0}")]
    InvalidDirectory(String),
    
    #[error("No entries in directory")]
    EmptyDirectory,
}

/// Entry in the file picker list (sorted and categorized)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerEntry {
    /// Entry name
    pub name: String,
    /// Object ID
    pub object_id: ObjectId,
    /// Object kind
    pub kind: ObjectKind,
    /// Whether this is a directory
    pub is_directory: bool,
}

impl PickerEntry {
    /// Creates a new picker entry from a directory entry
    fn from_directory_entry(entry: &DirectoryEntry) -> Self {
        Self {
            name: entry.name.clone(),
            object_id: entry.object_id,
            kind: entry.kind,
            is_directory: entry.kind == ObjectKind::Map,
        }
    }
}

/// File picker state
#[derive(Debug, Clone)]
pub struct FilePicker {
    /// Current directory being displayed
    current_directory: DirectoryView,
    /// Sorted list of entries (directories first, then files, lexicographic)
    entries: Vec<PickerEntry>,
    /// Currently selected index
    selected_index: usize,
    /// Directory navigation stack (for going back)
    directory_stack: Vec<DirectoryView>,
}

impl FilePicker {
    /// Creates a new file picker starting at the given directory
    pub fn new(root: DirectoryView) -> Self {
        let mut picker = Self {
            current_directory: root.clone(),
            entries: Vec::new(),
            selected_index: 0,
            directory_stack: Vec::new(),
        };
        picker.refresh_entries();
        picker
    }

    /// Refreshes the entry list from the current directory
    /// Applies deterministic sorting: directories first, then files, lexicographic within each group
    fn refresh_entries(&mut self) {
        let raw_entries = self.current_directory.list_entries();
        
        // Convert to picker entries
        let mut entries: Vec<PickerEntry> = raw_entries
            .iter()
            .map(|entry| PickerEntry::from_directory_entry(entry))
            .collect();
        
        // Sort deterministically:
        // 1. Directories first (is_directory = true comes before false)
        // 2. Within each group, lexicographic by name
        entries.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.cmp(&b.name))
        });
        
        self.entries = entries;
        
        // Reset selection if out of bounds
        if self.selected_index >= self.entries.len() && !self.entries.is_empty() {
            self.selected_index = 0;
        }
    }

    /// Processes an input event and returns the result
    pub fn process_input(&mut self, event: InputEvent) -> FilePickerResult {
        // Only handle key press events
        let key_event = match event {
            InputEvent::Key(ke) if ke.state == KeyState::Pressed => ke,
            _ => return FilePickerResult::Continue,
        };

        match key_event.code {
            KeyCode::Up => {
                self.move_selection_up();
                FilePickerResult::Continue
            }
            KeyCode::Down => {
                self.move_selection_down();
                FilePickerResult::Continue
            }
            KeyCode::Enter => self.handle_selection(),
            KeyCode::Escape => self.handle_back(),
            _ => FilePickerResult::Continue,
        }
    }

    /// Moves the selection up
    fn move_selection_up(&mut self) {
        if !self.entries.is_empty() {
            self.selected_index = if self.selected_index == 0 {
                self.entries.len() - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    /// Moves the selection down
    fn move_selection_down(&mut self) {
        if !self.entries.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.entries.len();
        }
    }

    /// Handles the Enter key (select file or enter directory)
    fn handle_selection(&mut self) -> FilePickerResult {
        if self.entries.is_empty() {
            return FilePickerResult::Continue;
        }

        let selected = &self.entries[self.selected_index];

        if selected.is_directory {
            // Enter the directory
            // TODO: Implement directory navigation when DirectoryView supports it
            // For now, just continue
            FilePickerResult::Continue
        } else {
            // Select the file
            FilePickerResult::FileSelected {
                object_id: selected.object_id,
                name: selected.name.clone(),
            }
        }
    }

    /// Handles the Escape key (go back or cancel)
    fn handle_back(&mut self) -> FilePickerResult {
        if let Some(parent_dir) = self.directory_stack.pop() {
            // Go back to parent directory
            self.current_directory = parent_dir;
            self.refresh_entries();
            FilePickerResult::Continue
        } else {
            // At root, cancel the picker
            FilePickerResult::Cancelled
        }
    }

    /// Returns the current directory
    pub fn current_directory(&self) -> &DirectoryView {
        &self.current_directory
    }

    /// Returns the list of entries
    pub fn entries(&self) -> &[PickerEntry] {
        &self.entries
    }

    /// Returns the currently selected index
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Returns the currently selected entry, if any
    pub fn selected_entry(&self) -> Option<&PickerEntry> {
        self.entries.get(self.selected_index)
    }

    /// Returns the number of entries
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use input_types::{KeyEvent, Modifiers};

    fn create_test_directory() -> DirectoryView {
        let dir_id = ObjectId::new();
        let mut dir = DirectoryView::new(dir_id);

        // Add some test entries (deliberately unsorted)
        dir.add_entry(DirectoryEntry::new(
            "zebra.txt".to_string(),
            ObjectId::new(),
            ObjectKind::Blob,
        ));
        dir.add_entry(DirectoryEntry::new(
            "apple.txt".to_string(),
            ObjectId::new(),
            ObjectKind::Blob,
        ));
        dir.add_entry(DirectoryEntry::new(
            "docs".to_string(),
            ObjectId::new(),
            ObjectKind::Map,
        ));
        dir.add_entry(DirectoryEntry::new(
            "src".to_string(),
            ObjectId::new(),
            ObjectKind::Map,
        ));

        dir
    }

    #[test]
    fn test_picker_creation() {
        let dir = create_test_directory();
        let picker = FilePicker::new(dir);

        assert_eq!(picker.entry_count(), 4);
        assert_eq!(picker.selected_index(), 0);
    }

    #[test]
    fn test_deterministic_sorting() {
        let dir = create_test_directory();
        let picker = FilePicker::new(dir);

        let entries = picker.entries();
        assert_eq!(entries.len(), 4);

        // First two should be directories (Map), sorted lexicographically
        assert!(entries[0].is_directory);
        assert_eq!(entries[0].name, "docs");
        assert!(entries[1].is_directory);
        assert_eq!(entries[1].name, "src");

        // Next two should be files (Blob), sorted lexicographically
        assert!(!entries[2].is_directory);
        assert_eq!(entries[2].name, "apple.txt");
        assert!(!entries[3].is_directory);
        assert_eq!(entries[3].name, "zebra.txt");
    }

    #[test]
    fn test_navigation_up() {
        let dir = create_test_directory();
        let mut picker = FilePicker::new(dir);

        // Start at index 0
        assert_eq!(picker.selected_index(), 0);

        // Move up wraps to last entry
        picker.move_selection_up();
        assert_eq!(picker.selected_index(), 3);

        // Move up again
        picker.move_selection_up();
        assert_eq!(picker.selected_index(), 2);
    }

    #[test]
    fn test_navigation_down() {
        let dir = create_test_directory();
        let mut picker = FilePicker::new(dir);

        // Start at index 0
        assert_eq!(picker.selected_index(), 0);

        // Move down
        picker.move_selection_down();
        assert_eq!(picker.selected_index(), 1);

        // Move to last, then wrap
        picker.move_selection_down();
        picker.move_selection_down();
        picker.move_selection_down();
        assert_eq!(picker.selected_index(), 0);
    }

    #[test]
    fn test_input_handling_up_down() {
        let dir = create_test_directory();
        let mut picker = FilePicker::new(dir);

        let down_event = InputEvent::Key(KeyEvent::pressed(
            KeyCode::Down,
            Modifiers::none(),
        ));

        let result = picker.process_input(down_event);
        assert_eq!(result, FilePickerResult::Continue);
        assert_eq!(picker.selected_index(), 1);

        let up_event = InputEvent::Key(KeyEvent::pressed(
            KeyCode::Up,
            Modifiers::none(),
        ));

        let result = picker.process_input(up_event);
        assert_eq!(result, FilePickerResult::Continue);
        assert_eq!(picker.selected_index(), 0);
    }

    #[test]
    fn test_file_selection() {
        let dir = create_test_directory();
        let mut picker = FilePicker::new(dir);

        // Move to a file entry (index 2 is "apple.txt")
        picker.selected_index = 2;

        let enter_event = InputEvent::Key(KeyEvent::pressed(
            KeyCode::Enter,
            Modifiers::none(),
        ));

        let result = picker.process_input(enter_event);
        match result {
            FilePickerResult::FileSelected { name, .. } => {
                assert_eq!(name, "apple.txt");
            }
            _ => panic!("Expected FileSelected result"),
        }
    }

    #[test]
    fn test_cancel_at_root() {
        let dir = create_test_directory();
        let mut picker = FilePicker::new(dir);

        let escape_event = InputEvent::Key(KeyEvent::pressed(
            KeyCode::Escape,
            Modifiers::none(),
        ));

        let result = picker.process_input(escape_event);
        assert_eq!(result, FilePickerResult::Cancelled);
    }

    #[test]
    fn test_empty_directory() {
        let dir_id = ObjectId::new();
        let dir = DirectoryView::new(dir_id);
        let picker = FilePicker::new(dir);

        assert_eq!(picker.entry_count(), 0);
        assert_eq!(picker.selected_index(), 0);
    }

    #[test]
    fn test_selected_entry() {
        let dir = create_test_directory();
        let picker = FilePicker::new(dir);

        let selected = picker.selected_entry();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().name, "docs");
    }

    #[test]
    fn test_ignore_key_release() {
        let dir = create_test_directory();
        let mut picker = FilePicker::new(dir);

        let release_event = InputEvent::Key(KeyEvent::released(
            KeyCode::Down,
            Modifiers::none(),
        ));

        let result = picker.process_input(release_event);
        assert_eq!(result, FilePickerResult::Continue);
        assert_eq!(picker.selected_index(), 0); // Should not move
    }
}
