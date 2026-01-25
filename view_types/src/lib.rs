#![no_std]

//! # View Types
//!
//! This crate defines stable, versionable view schemas for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Views, not streams**: Output is structured views, not byte streams
//! - **Immutable frames**: View frames are immutable; updates replace by revision
//! - **Capability-gated**: Views require capabilities to publish or subscribe
//! - **Testable**: Views are serializable and can be snapshot-tested
//!
//! ## Non-Goals
//!
//! This is NOT:
//! - ANSI/VT terminal emulation
//! - A graphics system
//! - A full UI toolkit
//! - stdout/stderr replacement

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
use core::fmt;
use uuid::Uuid;

/// Unique identifier for a view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewId(Uuid);

impl ViewId {
    /// Creates a new unique view ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Creates a ViewId from an existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Returns the inner UUID value
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ViewId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ViewId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "view:{}", self.0)
    }
}

/// Type of view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewKind {
    /// Text buffer view (main content)
    TextBuffer,
    /// Status line view (single line of status)
    StatusLine,
    /// Panel container (metadata only, no graphics)
    Panel,
}

impl fmt::Display for ViewKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViewKind::TextBuffer => write!(f, "TextBuffer"),
            ViewKind::StatusLine => write!(f, "StatusLine"),
            ViewKind::Panel => write!(f, "Panel"),
        }
    }
}

/// Cursor position in a view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorPosition {
    /// Line number (0-indexed)
    pub line: usize,
    /// Column number (0-indexed)
    pub column: usize,
}

impl CursorPosition {
    /// Creates a new cursor position
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    /// Creates a cursor at the origin (0, 0)
    pub fn origin() -> Self {
        Self { line: 0, column: 0 }
    }
}

impl fmt::Display for CursorPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// View frame - immutable snapshot of view state
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewFrame {
    /// Unique view identifier
    pub view_id: ViewId,
    /// Type of view
    pub kind: ViewKind,
    /// Monotonic revision number (must increase with each update)
    pub revision: u64,
    /// View content
    pub content: ViewContent,
    /// Optional cursor position
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<CursorPosition>,
    /// Optional title
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Component ID that owns this view
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    /// Timestamp when frame was created (simulation time in nanoseconds)
    pub timestamp_ns: u64,
}

impl ViewFrame {
    /// Creates a new view frame
    pub fn new(
        view_id: ViewId,
        kind: ViewKind,
        revision: u64,
        content: ViewContent,
        timestamp_ns: u64,
    ) -> Self {
        Self {
            view_id,
            kind,
            revision,
            content,
            cursor: None,
            title: None,
            component_id: None,
            timestamp_ns,
        }
    }

    /// Sets the cursor position
    pub fn with_cursor(mut self, cursor: CursorPosition) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Sets the title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the component ID
    pub fn with_component_id(mut self, component_id: impl Into<String>) -> Self {
        self.component_id = Some(component_id.into());
        self
    }

    /// Checks if this frame's revision is newer than another
    pub fn is_newer_than(&self, other: &ViewFrame) -> bool {
        self.view_id == other.view_id && self.revision > other.revision
    }

    /// Checks if this frame's revision is compatible (monotonic increase)
    pub fn is_valid_successor(&self, previous: &ViewFrame) -> bool {
        self.view_id == previous.view_id && self.revision > previous.revision
    }
}

/// Content of a view
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewContent {
    /// Text buffer content (vector of lines)
    TextBuffer { lines: Vec<String> },
    /// Status line content (single line)
    StatusLine { text: String },
    /// Panel metadata (no actual graphics)
    Panel { metadata: String },
}

impl ViewContent {
    /// Creates empty text buffer content
    pub fn empty_text_buffer() -> Self {
        ViewContent::TextBuffer { lines: Vec::new() }
    }

    /// Creates text buffer content from lines
    pub fn text_buffer(lines: Vec<String>) -> Self {
        ViewContent::TextBuffer { lines }
    }

    /// Creates status line content
    pub fn status_line(text: impl Into<String>) -> Self {
        ViewContent::StatusLine { text: text.into() }
    }

    /// Creates panel content
    pub fn panel(metadata: impl Into<String>) -> Self {
        ViewContent::Panel {
            metadata: metadata.into(),
        }
    }

    /// Returns the number of lines (for TextBuffer)
    pub fn line_count(&self) -> usize {
        match self {
            ViewContent::TextBuffer { lines } => lines.len(),
            ViewContent::StatusLine { .. } => 1,
            ViewContent::Panel { .. } => 0,
        }
    }

    /// Returns a specific line (for TextBuffer or StatusLine)
    pub fn get_line(&self, index: usize) -> Option<&str> {
        match self {
            ViewContent::TextBuffer { lines } => lines.get(index).map(|s| s.as_str()),
            ViewContent::StatusLine { text } if index == 0 => Some(text.as_str()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::format;

    #[test]
    fn test_view_id_creation() {
        let id1 = ViewId::new();
        let id2 = ViewId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_view_id_from_uuid() {
        let uuid = Uuid::new_v4();
        let id = ViewId::from_uuid(uuid);
        assert_eq!(id.as_uuid(), uuid);
    }

    #[test]
    fn test_view_id_display() {
        let id = ViewId::new();
        let display = format!("{}", id);
        assert!(display.starts_with("view:"));
    }

    #[test]
    fn test_view_kind_display() {
        assert_eq!(ViewKind::TextBuffer.to_string(), "TextBuffer");
        assert_eq!(ViewKind::StatusLine.to_string(), "StatusLine");
        assert_eq!(ViewKind::Panel.to_string(), "Panel");
    }

    #[test]
    fn test_cursor_position() {
        let cursor = CursorPosition::new(5, 10);
        assert_eq!(cursor.line, 5);
        assert_eq!(cursor.column, 10);
    }

    #[test]
    fn test_cursor_origin() {
        let cursor = CursorPosition::origin();
        assert_eq!(cursor.line, 0);
        assert_eq!(cursor.column, 0);
    }

    #[test]
    fn test_cursor_display() {
        let cursor = CursorPosition::new(5, 10);
        assert_eq!(format!("{}", cursor), "5:10");
    }

    #[test]
    fn test_view_frame_creation() {
        let view_id = ViewId::new();
        let content = ViewContent::empty_text_buffer();
        let frame = ViewFrame::new(view_id, ViewKind::TextBuffer, 1, content, 1000);

        assert_eq!(frame.view_id, view_id);
        assert_eq!(frame.kind, ViewKind::TextBuffer);
        assert_eq!(frame.revision, 1);
        assert_eq!(frame.timestamp_ns, 1000);
        assert!(frame.cursor.is_none());
        assert!(frame.title.is_none());
    }

    #[test]
    fn test_view_frame_with_cursor() {
        let view_id = ViewId::new();
        let content = ViewContent::empty_text_buffer();
        let cursor = CursorPosition::new(1, 2);
        let frame =
            ViewFrame::new(view_id, ViewKind::TextBuffer, 1, content, 1000).with_cursor(cursor);

        assert_eq!(frame.cursor, Some(cursor));
    }

    #[test]
    fn test_view_frame_with_title() {
        let view_id = ViewId::new();
        let content = ViewContent::empty_text_buffer();
        let frame =
            ViewFrame::new(view_id, ViewKind::TextBuffer, 1, content, 1000).with_title("My View");

        assert_eq!(frame.title, Some("My View".to_string()));
    }

    #[test]
    fn test_view_frame_with_component_id() {
        let view_id = ViewId::new();
        let content = ViewContent::empty_text_buffer();
        let frame = ViewFrame::new(view_id, ViewKind::TextBuffer, 1, content, 1000)
            .with_component_id("comp:123");

        assert_eq!(frame.component_id, Some("comp:123".to_string()));
    }

    #[test]
    fn test_view_frame_revision_ordering() {
        let view_id = ViewId::new();
        let content = ViewContent::empty_text_buffer();
        let frame1 = ViewFrame::new(view_id, ViewKind::TextBuffer, 1, content.clone(), 1000);
        let frame2 = ViewFrame::new(view_id, ViewKind::TextBuffer, 2, content, 2000);

        assert!(frame2.is_newer_than(&frame1));
        assert!(!frame1.is_newer_than(&frame2));
        assert!(frame2.is_valid_successor(&frame1));
        assert!(!frame1.is_valid_successor(&frame2));
    }

    #[test]
    fn test_view_frame_revision_different_views() {
        let view_id1 = ViewId::new();
        let view_id2 = ViewId::new();
        let content = ViewContent::empty_text_buffer();
        let frame1 = ViewFrame::new(view_id1, ViewKind::TextBuffer, 1, content.clone(), 1000);
        let frame2 = ViewFrame::new(view_id2, ViewKind::TextBuffer, 2, content, 2000);

        assert!(!frame2.is_newer_than(&frame1));
        assert!(!frame2.is_valid_successor(&frame1));
    }

    #[test]
    fn test_view_frame_revision_non_monotonic() {
        let view_id = ViewId::new();
        let content = ViewContent::empty_text_buffer();
        let frame1 = ViewFrame::new(view_id, ViewKind::TextBuffer, 5, content.clone(), 1000);
        let frame2 = ViewFrame::new(view_id, ViewKind::TextBuffer, 3, content, 2000);

        assert!(!frame2.is_valid_successor(&frame1));
    }

    #[test]
    fn test_text_buffer_content() {
        let lines = vec!["line1".to_string(), "line2".to_string()];
        let content = ViewContent::text_buffer(lines.clone());

        match content {
            ViewContent::TextBuffer { lines: l } => {
                assert_eq!(l, lines);
            }
            _ => panic!("Expected TextBuffer content"),
        }
    }

    #[test]
    fn test_empty_text_buffer() {
        let content = ViewContent::empty_text_buffer();
        assert_eq!(content.line_count(), 0);
    }

    #[test]
    fn test_status_line_content() {
        let content = ViewContent::status_line("Status text");

        match content {
            ViewContent::StatusLine { text } => {
                assert_eq!(text, "Status text");
            }
            _ => panic!("Expected StatusLine content"),
        }
    }

    #[test]
    fn test_panel_content() {
        let content = ViewContent::panel("Panel metadata");

        match content {
            ViewContent::Panel { metadata } => {
                assert_eq!(metadata, "Panel metadata");
            }
            _ => panic!("Expected Panel content"),
        }
    }

    #[test]
    fn test_content_line_count() {
        let text_buffer = ViewContent::text_buffer(vec!["a".to_string(), "b".to_string()]);
        let status_line = ViewContent::status_line("status");
        let panel = ViewContent::panel("metadata");

        assert_eq!(text_buffer.line_count(), 2);
        assert_eq!(status_line.line_count(), 1);
        assert_eq!(panel.line_count(), 0);
    }

    #[test]
    fn test_content_get_line() {
        let text_buffer = ViewContent::text_buffer(vec!["line1".to_string(), "line2".to_string()]);
        let status_line = ViewContent::status_line("status");

        assert_eq!(text_buffer.get_line(0), Some("line1"));
        assert_eq!(text_buffer.get_line(1), Some("line2"));
        assert_eq!(text_buffer.get_line(2), None);

        assert_eq!(status_line.get_line(0), Some("status"));
        assert_eq!(status_line.get_line(1), None);
    }

    #[test]
    fn test_view_frame_serialization() {
        let view_id = ViewId::new();
        let content = ViewContent::text_buffer(vec!["test".to_string()]);
        let frame = ViewFrame::new(view_id, ViewKind::TextBuffer, 1, content, 1000)
            .with_cursor(CursorPosition::new(0, 0))
            .with_title("Test");

        let json = serde_json::to_string(&frame).unwrap();
        let deserialized: ViewFrame = serde_json::from_str(&json).unwrap();

        assert_eq!(frame, deserialized);
    }

    #[test]
    fn test_view_content_serialization() {
        let text_buffer = ViewContent::text_buffer(vec!["line1".to_string()]);
        let status_line = ViewContent::status_line("status");
        let panel = ViewContent::panel("metadata");

        let json1 = serde_json::to_string(&text_buffer).unwrap();
        let json2 = serde_json::to_string(&status_line).unwrap();
        let json3 = serde_json::to_string(&panel).unwrap();

        let deserialized1: ViewContent = serde_json::from_str(&json1).unwrap();
        let deserialized2: ViewContent = serde_json::from_str(&json2).unwrap();
        let deserialized3: ViewContent = serde_json::from_str(&json3).unwrap();

        assert_eq!(text_buffer, deserialized1);
        assert_eq!(status_line, deserialized2);
        assert_eq!(panel, deserialized3);
    }

    #[test]
    fn test_cursor_position_serialization() {
        let cursor = CursorPosition::new(5, 10);
        let json = serde_json::to_string(&cursor).unwrap();
        let deserialized: CursorPosition = serde_json::from_str(&json).unwrap();

        assert_eq!(cursor, deserialized);
    }

    #[test]
    fn test_view_id_serialization() {
        let id = ViewId::new();
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: ViewId = serde_json::from_str(&json).unwrap();

        assert_eq!(id, deserialized);
    }
}
