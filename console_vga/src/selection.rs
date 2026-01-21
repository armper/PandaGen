//! # Text Selection and Clipboard for VGA Console
//!
//! This module provides text selection and internal clipboard functionality.
//! No system clipboard, no Wayland nonsense - just PandaGen's internal buffer.

use alloc::vec::Vec;

/// Selection range in the VGA buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRange {
    /// Start position (col, row)
    pub start: (usize, usize),
    /// End position (col, row)
    pub end: (usize, usize),
}

impl SelectionRange {
    /// Creates a new selection range
    pub fn new(start: (usize, usize), end: (usize, usize)) -> Self {
        Self { start, end }
    }

    /// Returns the normalized range (start <= end)
    pub fn normalized(&self) -> ((usize, usize), (usize, usize)) {
        let (start_col, start_row) = self.start;
        let (end_col, end_row) = self.end;

        // Compare row first, then column
        if start_row < end_row || (start_row == end_row && start_col <= end_col) {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    /// Checks if a position is within the selection
    pub fn contains(&self, col: usize, row: usize) -> bool {
        let ((start_col, start_row), (end_col, end_row)) = self.normalized();

        if row < start_row || row > end_row {
            return false;
        }

        if row == start_row && row == end_row {
            // Single row selection
            col >= start_col && col <= end_col
        } else if row == start_row {
            // First row of multi-row selection
            col >= start_col
        } else if row == end_row {
            // Last row of multi-row selection
            col <= end_col
        } else {
            // Middle rows - entire row selected
            true
        }
    }

    /// Checks if the selection is empty (start == end)
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Internal clipboard buffer
#[derive(Debug, Clone)]
pub struct Clipboard {
    /// Text content
    content: Vec<u8>,
}

impl Clipboard {
    /// Creates a new empty clipboard
    pub fn new() -> Self {
        Self {
            content: Vec::new(),
        }
    }

    /// Copies text to clipboard
    pub fn copy(&mut self, text: &[u8]) {
        self.content.clear();
        self.content.extend_from_slice(text);
    }

    /// Gets clipboard content
    pub fn paste(&self) -> &[u8] {
        &self.content
    }

    /// Checks if clipboard is empty
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Clears clipboard
    pub fn clear(&mut self) {
        self.content.clear();
    }

    /// Gets clipboard content as string (lossy UTF-8)
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.content).unwrap_or("")
    }
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new()
    }
}

/// Text selection manager
#[derive(Debug)]
pub struct SelectionManager {
    /// Current selection range
    selection: Option<SelectionRange>,
    /// Internal clipboard
    clipboard: Clipboard,
}

impl SelectionManager {
    /// Creates a new selection manager
    pub fn new() -> Self {
        Self {
            selection: None,
            clipboard: Clipboard::new(),
        }
    }

    /// Starts a new selection at the given position
    pub fn start_selection(&mut self, col: usize, row: usize) {
        self.selection = Some(SelectionRange::new((col, row), (col, row)));
    }

    /// Extends the current selection to the given position
    pub fn extend_selection(&mut self, col: usize, row: usize) {
        if let Some(ref mut selection) = self.selection {
            selection.end = (col, row);
        }
    }

    /// Gets the current selection range
    pub fn get_selection(&self) -> Option<SelectionRange> {
        self.selection
    }

    /// Clears the current selection
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Copies selected text to clipboard
    pub fn copy_selection(&mut self, text: &[u8]) {
        self.clipboard.copy(text);
    }

    /// Pastes clipboard content
    pub fn paste(&self) -> &[u8] {
        self.clipboard.paste()
    }

    /// Checks if there's an active selection
    pub fn has_selection(&self) -> bool {
        self.selection.is_some()
    }

    /// Checks if clipboard has content
    pub fn has_clipboard_content(&self) -> bool {
        !self.clipboard.is_empty()
    }
}

impl Default for SelectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_range_creation() {
        let range = SelectionRange::new((0, 0), (10, 0));
        assert_eq!(range.start, (0, 0));
        assert_eq!(range.end, (10, 0));
    }

    #[test]
    fn test_selection_range_normalized() {
        // Forward selection
        let range = SelectionRange::new((0, 0), (10, 0));
        let (start, end) = range.normalized();
        assert_eq!(start, (0, 0));
        assert_eq!(end, (10, 0));

        // Backward selection
        let range = SelectionRange::new((10, 0), (0, 0));
        let (start, end) = range.normalized();
        assert_eq!(start, (0, 0));
        assert_eq!(end, (10, 0));

        // Multi-row backward selection
        let range = SelectionRange::new((10, 2), (5, 1));
        let (start, end) = range.normalized();
        assert_eq!(start, (5, 1));
        assert_eq!(end, (10, 2));
    }

    #[test]
    fn test_selection_range_contains() {
        let range = SelectionRange::new((5, 1), (15, 1));
        
        assert!(range.contains(5, 1));   // Start
        assert!(range.contains(10, 1));  // Middle
        assert!(range.contains(15, 1));  // End
        assert!(!range.contains(4, 1));  // Before start
        assert!(!range.contains(16, 1)); // After end
        assert!(!range.contains(10, 0)); // Wrong row
    }

    #[test]
    fn test_selection_range_multirow_contains() {
        let range = SelectionRange::new((5, 1), (10, 3));
        
        assert!(range.contains(5, 1));   // Start of first row
        assert!(range.contains(20, 1));  // Rest of first row
        assert!(range.contains(0, 2));   // Entire middle row
        assert!(range.contains(50, 2));  // Entire middle row
        assert!(range.contains(0, 3));   // Start of last row
        assert!(range.contains(10, 3));  // End of last row
        assert!(!range.contains(11, 3)); // After end
        assert!(!range.contains(0, 0));  // Before start row
        assert!(!range.contains(0, 4));  // After end row
    }

    #[test]
    fn test_selection_range_empty() {
        let range = SelectionRange::new((5, 1), (5, 1));
        assert!(range.is_empty());

        let range = SelectionRange::new((5, 1), (6, 1));
        assert!(!range.is_empty());
    }

    #[test]
    fn test_clipboard_copy_paste() {
        let mut clipboard = Clipboard::new();
        assert!(clipboard.is_empty());

        clipboard.copy(b"Hello, World!");
        assert!(!clipboard.is_empty());
        assert_eq!(clipboard.paste(), b"Hello, World!");
        assert_eq!(clipboard.as_str(), "Hello, World!");
    }

    #[test]
    fn test_clipboard_clear() {
        let mut clipboard = Clipboard::new();
        clipboard.copy(b"Test");
        assert!(!clipboard.is_empty());

        clipboard.clear();
        assert!(clipboard.is_empty());
        assert_eq!(clipboard.paste(), b"");
    }

    #[test]
    fn test_selection_manager_workflow() {
        let mut manager = SelectionManager::new();
        
        // No selection initially
        assert!(!manager.has_selection());
        
        // Start selection
        manager.start_selection(0, 0);
        assert!(manager.has_selection());
        
        let selection = manager.get_selection().unwrap();
        assert_eq!(selection.start, (0, 0));
        assert_eq!(selection.end, (0, 0));
        
        // Extend selection
        manager.extend_selection(10, 0);
        let selection = manager.get_selection().unwrap();
        assert_eq!(selection.end, (10, 0));
        
        // Copy text
        manager.copy_selection(b"Selected text");
        assert!(manager.has_clipboard_content());
        
        // Paste
        assert_eq!(manager.paste(), b"Selected text");
        
        // Clear selection
        manager.clear_selection();
        assert!(!manager.has_selection());
        
        // Clipboard persists after selection cleared
        assert!(manager.has_clipboard_content());
        assert_eq!(manager.paste(), b"Selected text");
    }

    #[test]
    fn test_selection_manager_multiple_selections() {
        let mut manager = SelectionManager::new();
        
        // First selection
        manager.start_selection(0, 0);
        manager.extend_selection(10, 0);
        manager.copy_selection(b"First");
        
        // Second selection (replaces first)
        manager.start_selection(5, 1);
        manager.extend_selection(15, 1);
        manager.copy_selection(b"Second");
        
        // Second text in clipboard
        assert_eq!(manager.paste(), b"Second");
    }
}
