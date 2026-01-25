//! Rendering logic for the file picker
//!
//! This module handles converting the file picker state into ViewFrames.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

use crate::{FilePicker, PickerEntry};

impl FilePicker {
    /// Renders the file picker as a text buffer view frame
    pub fn render_text_buffer(&self, view_id: ViewId, revision: u64, timestamp_ns: u64) -> ViewFrame {
        let mut lines = Vec::new();

        if self.entries.is_empty() {
            lines.push("(empty directory)".to_string());
        } else {
            for (index, entry) in self.entries.iter().enumerate() {
                let is_selected = index == self.selected_index;
                let line = format_entry(entry, is_selected);
                lines.push(line);
            }
        }

        ViewFrame::new(
            view_id,
            ViewKind::TextBuffer,
            revision,
            ViewContent::text_buffer(lines),
            timestamp_ns,
        )
        .with_cursor(CursorPosition::new(self.selected_index, 0))
    }

    /// Renders the status line view frame
    pub fn render_status_line(
        &self,
        view_id: ViewId,
        revision: u64,
        timestamp_ns: u64,
        breadcrumb: &str,
    ) -> ViewFrame {
        let entry_count = self.entry_count();
        let selected = self.selected_index + 1; // 1-indexed for display
        
        let status_text = if entry_count == 0 {
            format!("{} — Empty", breadcrumb)
        } else {
            format!("{} — {} of {} items", breadcrumb, selected, entry_count)
        };

        ViewFrame::new(
            view_id,
            ViewKind::StatusLine,
            revision,
            ViewContent::status_line(status_text),
            timestamp_ns,
        )
    }
}

/// Formats a single entry for display
fn format_entry(entry: &PickerEntry, is_selected: bool) -> String {
    let prefix = if is_selected { "> " } else { "  " };
    let type_marker = if entry.is_directory { "/" } else { " " };
    
    format!("{}{}{}", prefix, entry.name, type_marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_view::{DirectoryEntry, DirectoryView};
    use services_storage::{ObjectId, ObjectKind};

    fn create_test_directory() -> DirectoryView {
        let dir_id = ObjectId::new();
        let mut dir = DirectoryView::new(dir_id);

        dir.add_entry(DirectoryEntry::new(
            "src".to_string(),
            ObjectId::new(),
            ObjectKind::Map,
        ));
        dir.add_entry(DirectoryEntry::new(
            "README.md".to_string(),
            ObjectId::new(),
            ObjectKind::Blob,
        ));

        dir
    }

    #[test]
    fn test_format_entry_directory() {
        let entry = PickerEntry {
            name: "src".to_string(),
            object_id: ObjectId::new(),
            kind: ObjectKind::Map,
            is_directory: true,
        };

        let formatted = format_entry(&entry, false);
        assert_eq!(formatted, "  src/");

        let formatted_selected = format_entry(&entry, true);
        assert_eq!(formatted_selected, "> src/");
    }

    #[test]
    fn test_format_entry_file() {
        let entry = PickerEntry {
            name: "README.md".to_string(),
            object_id: ObjectId::new(),
            kind: ObjectKind::Blob,
            is_directory: false,
        };

        let formatted = format_entry(&entry, false);
        assert_eq!(formatted, "  README.md ");

        let formatted_selected = format_entry(&entry, true);
        assert_eq!(formatted_selected, "> README.md ");
    }

    #[test]
    fn test_render_text_buffer() {
        let dir = create_test_directory();
        let picker = FilePicker::new(dir);

        let view_id = ViewId::new();
        let frame = picker.render_text_buffer(view_id, 1, 0);

        assert_eq!(frame.view_id, view_id);
        assert_eq!(frame.kind, ViewKind::TextBuffer);
        assert_eq!(frame.revision, 1);

        // Check content
        match &frame.content {
            ViewContent::TextBuffer { lines } => {
                assert_eq!(lines.len(), 2);
                // First entry is selected (directory)
                assert_eq!(lines[0], "> src/");
                assert_eq!(lines[1], "  README.md ");
            }
            _ => panic!("Expected TextBuffer content"),
        }

        // Check cursor
        assert_eq!(frame.cursor, Some(CursorPosition::new(0, 0)));
    }

    #[test]
    fn test_render_status_line() {
        let dir = create_test_directory();
        let picker = FilePicker::new(dir);

        let view_id = ViewId::new();
        let frame = picker.render_status_line(view_id, 1, 0, "/home/user");

        assert_eq!(frame.view_id, view_id);
        assert_eq!(frame.kind, ViewKind::StatusLine);
        assert_eq!(frame.revision, 1);

        // Check content
        match &frame.content {
            ViewContent::StatusLine { text } => {
                assert_eq!(text, "/home/user — 1 of 2 items");
            }
            _ => panic!("Expected StatusLine content"),
        }
    }

    #[test]
    fn test_render_empty_directory() {
        let dir_id = ObjectId::new();
        let dir = DirectoryView::new(dir_id);
        let picker = FilePicker::new(dir);

        let view_id = ViewId::new();
        let frame = picker.render_text_buffer(view_id, 1, 0);

        match &frame.content {
            ViewContent::TextBuffer { lines } => {
                assert_eq!(lines.len(), 1);
                assert_eq!(lines[0], "(empty directory)");
            }
            _ => panic!("Expected TextBuffer content"),
        }
    }

    #[test]
    fn test_render_status_line_empty() {
        let dir_id = ObjectId::new();
        let dir = DirectoryView::new(dir_id);
        let picker = FilePicker::new(dir);

        let view_id = ViewId::new();
        let frame = picker.render_status_line(view_id, 1, 0, "/empty");

        match &frame.content {
            ViewContent::StatusLine { text } => {
                assert_eq!(text, "/empty — Empty");
            }
            _ => panic!("Expected StatusLine content"),
        }
    }

    #[test]
    fn test_cursor_follows_selection() {
        let dir = create_test_directory();
        let mut picker = FilePicker::new(dir);

        // Move selection down
        picker.move_selection_down();

        let view_id = ViewId::new();
        let frame = picker.render_text_buffer(view_id, 1, 0);

        // Cursor should be on second line
        assert_eq!(frame.cursor, Some(CursorPosition::new(1, 0)));
    }
}
