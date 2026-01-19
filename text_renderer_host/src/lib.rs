//! # Text Renderer Host
//!
//! This crate provides a text-based renderer host for PandaGen OS.
//!
//! ## Philosophy
//!
//! - **Rendering is a host concern**, not a component concern
//! - **Components never print** - they publish views
//! - **Views are rendered, not streamed** - immutable frames
//! - **Renderer is dumb and replaceable** - no business logic
//! - **Renderer is NOT a terminal** - no ANSI, no cursor addressing, no terminal state
//!
//! ## Responsibilities
//!
//! The text renderer host:
//! - Subscribes to workspace view snapshots
//! - Renders focused component's views
//! - Renders status line consistently
//! - Redraws on revision change
//! - Stops rendering when cancelled (budget exhaustion)
//!
//! ## Non-Responsibilities
//!
//! The text renderer host does NOT:
//! - Emit ANSI escape codes
//! - Maintain terminal state
//! - Implement cursor addressing
//! - Mix rendering with workspace logic
//! - Generate input events
//!
//! This is presentation, not authority.

use view_types::{CursorPosition, ViewContent, ViewFrame};

/// Text renderer that converts ViewFrames to text output
pub struct TextRenderer {
    /// Last rendered revision (to detect changes)
    last_main_revision: Option<u64>,
    last_status_revision: Option<u64>,
}

impl TextRenderer {
    /// Creates a new text renderer
    pub fn new() -> Self {
        Self {
            last_main_revision: None,
            last_status_revision: None,
        }
    }

    /// Checks if a redraw is needed based on revision changes
    pub fn needs_redraw(
        &self,
        main_frame: Option<&ViewFrame>,
        status_frame: Option<&ViewFrame>,
    ) -> bool {
        let main_changed = main_frame.map(|f| f.revision) != self.last_main_revision;
        let status_changed = status_frame.map(|f| f.revision) != self.last_status_revision;
        main_changed || status_changed
    }

    /// Renders a workspace snapshot to text output
    ///
    /// Returns the rendered text as a String.
    /// Performs a full screen redraw if revision has changed.
    pub fn render_snapshot(
        &mut self,
        main_view: Option<&ViewFrame>,
        status_view: Option<&ViewFrame>,
    ) -> String {
        let mut output = String::new();

        // Full redraw - just render content (no ANSI codes)
        // Render main view (if present)
        if let Some(frame) = main_view {
            output.push_str(&self.render_view_frame(frame));
            self.last_main_revision = Some(frame.revision);
        } else {
            output.push_str("(no view)\n");
            self.last_main_revision = None;
        }

        // Separator line
        output.push('\n');
        output.push_str(&"─".repeat(80));
        output.push('\n');

        // Render status view (if present)
        if let Some(frame) = status_view {
            output.push_str(&self.render_status_line(frame));
            self.last_status_revision = Some(frame.revision);
        } else {
            output.push_str("(no status)\n");
            self.last_status_revision = None;
        }

        output
    }

    /// Renders a single view frame
    fn render_view_frame(&self, frame: &ViewFrame) -> String {
        match &frame.content {
            ViewContent::TextBuffer { lines } => {
                self.render_text_buffer(lines, frame.cursor.as_ref())
            }
            ViewContent::StatusLine { text } => format!("{}\n", text),
            ViewContent::Panel { metadata } => format!("[Panel: {}]\n", metadata),
        }
    }

    /// Renders a text buffer with optional cursor
    fn render_text_buffer(&self, lines: &[String], cursor: Option<&CursorPosition>) -> String {
        let mut output = String::new();

        for (line_idx, line) in lines.iter().enumerate() {
            if let Some(cursor_pos) = cursor {
                if cursor_pos.line == line_idx {
                    // Insert cursor marker at the correct column
                    let col = cursor_pos.column;
                    if col <= line.len() {
                        let (before, after) = line.split_at(col);
                        output.push_str(before);
                        output.push('|'); // Cursor marker
                        output.push_str(after);
                        output.push('\n');
                    } else {
                        // Cursor beyond line end
                        output.push_str(line);
                        output.push_str(&" ".repeat(col.saturating_sub(line.len())));
                        output.push('|');
                        output.push('\n');
                    }
                    continue;
                }
            }
            output.push_str(line);
            output.push('\n');
        }

        // If cursor is on a line beyond the buffer
        if let Some(cursor_pos) = cursor {
            if cursor_pos.line >= lines.len() {
                for _ in lines.len()..cursor_pos.line {
                    output.push('\n');
                }
                output.push_str(&" ".repeat(cursor_pos.column));
                output.push_str("|\n");
            }
        }

        output
    }

    /// Renders a status line
    fn render_status_line(&self, frame: &ViewFrame) -> String {
        match &frame.content {
            ViewContent::StatusLine { text } => format!("{}\n", text),
            _ => "(invalid status view)\n".to_string(),
        }
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use view_types::{ViewId, ViewKind};

    fn create_text_buffer_frame(
        lines: Vec<String>,
        cursor: Option<CursorPosition>,
        revision: u64,
    ) -> ViewFrame {
        let mut frame = ViewFrame::new(
            ViewId::new(),
            ViewKind::TextBuffer,
            revision,
            ViewContent::text_buffer(lines),
            0,
        );
        if let Some(cursor_pos) = cursor {
            frame = frame.with_cursor(cursor_pos);
        }
        frame
    }

    fn create_status_frame(text: String, revision: u64) -> ViewFrame {
        ViewFrame::new(
            ViewId::new(),
            ViewKind::StatusLine,
            revision,
            ViewContent::status_line(text),
            0,
        )
    }

    #[test]
    fn test_render_empty_snapshot() {
        let mut renderer = TextRenderer::new();
        let output = renderer.render_snapshot(None, None);
        assert!(output.contains("(no view)"));
        assert!(output.contains("(no status)"));
    }

    #[test]
    fn test_render_text_buffer_without_cursor() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hello".to_string(), "World".to_string()];
        let frame = create_text_buffer_frame(lines, None, 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("Hello"));
        assert!(output.contains("World"));
    }

    #[test]
    fn test_render_text_buffer_with_cursor() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hello".to_string(), "World".to_string()];
        let cursor = CursorPosition::new(0, 2); // At 'l' in "Hello"
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("He|llo")); // Cursor marker at position 2
    }

    #[test]
    fn test_render_cursor_at_line_end() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hi".to_string()];
        let cursor = CursorPosition::new(0, 2); // At end of "Hi"
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("Hi|"));
    }

    #[test]
    fn test_render_cursor_beyond_line() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Hi".to_string()];
        let cursor = CursorPosition::new(0, 5); // Beyond "Hi"
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("Hi   |")); // Spaces then cursor
    }

    #[test]
    fn test_render_cursor_on_empty_line() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["".to_string()];
        let cursor = CursorPosition::new(0, 0);
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        assert!(output.contains("|"));
    }

    #[test]
    fn test_render_cursor_beyond_buffer() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Line 1".to_string()];
        let cursor = CursorPosition::new(5, 3); // Line 5, column 3
        let frame = create_text_buffer_frame(lines, Some(cursor), 1);
        let output = renderer.render_snapshot(Some(&frame), None);
        // Should have empty lines and cursor on line 5
        let output_lines: Vec<&str> = output.lines().collect();
        assert!(output_lines.len() > 5);
    }

    #[test]
    fn test_render_status_line() {
        let mut renderer = TextRenderer::new();
        let status = create_status_frame("-- INSERT --".to_string(), 1);
        let output = renderer.render_snapshot(None, Some(&status));
        assert!(output.contains("-- INSERT --"));
    }

    #[test]
    fn test_render_with_both_views() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Content".to_string()];
        let main = create_text_buffer_frame(lines, None, 1);
        let status = create_status_frame("Status".to_string(), 1);
        let output = renderer.render_snapshot(Some(&main), Some(&status));
        assert!(output.contains("Content"));
        assert!(output.contains("Status"));
        assert!(output.contains("─")); // Separator
    }

    #[test]
    fn test_needs_redraw_on_revision_change() {
        let mut renderer = TextRenderer::new();
        let lines = vec!["Test".to_string()];
        let frame1 = create_text_buffer_frame(lines.clone(), None, 1);

        // First render
        renderer.render_snapshot(Some(&frame1), None);

        // Same revision - no redraw needed
        assert!(!renderer.needs_redraw(Some(&frame1), None));

        // New revision - redraw needed
        let frame2 = create_text_buffer_frame(lines, None, 2);
        assert!(renderer.needs_redraw(Some(&frame2), None));
    }

    #[test]
    fn test_needs_redraw_on_status_change() {
        let mut renderer = TextRenderer::new();
        let status1 = create_status_frame("Status 1".to_string(), 1);

        // First render
        renderer.render_snapshot(None, Some(&status1));

        // Same revision - no redraw needed
        assert!(!renderer.needs_redraw(None, Some(&status1)));

        // New revision - redraw needed
        let status2 = create_status_frame("Status 2".to_string(), 2);
        assert!(renderer.needs_redraw(None, Some(&status2)));
    }

    #[test]
    fn test_revision_tracking() {
        let mut renderer = TextRenderer::new();
        assert_eq!(renderer.last_main_revision, None);
        assert_eq!(renderer.last_status_revision, None);

        let lines = vec!["Test".to_string()];
        let main = create_text_buffer_frame(lines, None, 5);
        let status = create_status_frame("Status".to_string(), 10);

        renderer.render_snapshot(Some(&main), Some(&status));
        assert_eq!(renderer.last_main_revision, Some(5));
        assert_eq!(renderer.last_status_revision, Some(10));
    }
}
