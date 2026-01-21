//! # Scrollback Buffer for VGA Text Console
//!
//! This module provides a scrollback buffer for storing terminal text history in VGA text mode.
//! Unlike the framebuffer version, this is no_std compatible and doesn't require heap allocation.
//!
//! ## Design
//!
//! - Fixed-width text grid (cols × rows)
//! - Ring buffer for efficient scrolling
//! - Viewport tracks visible portion
//! - No ANSI codes, just plain text with VGA attributes

use alloc::vec::Vec;

/// A line of text in the scrollback buffer with VGA attributes
#[derive(Clone, Debug)]
pub struct VgaLine {
    /// Text content (up to cols characters)
    pub text: Vec<u8>,
    /// VGA attribute for each character
    pub attrs: Vec<u8>,
}

impl VgaLine {
    /// Create a new empty line
    pub fn new(cols: usize, _default_attr: u8) -> Self {
        Self {
            text: Vec::with_capacity(cols),
            attrs: Vec::with_capacity(cols),
        }
    }

    /// Create a line from text with uniform attribute, truncating if needed
    pub fn from_text(text: &str, attr: u8, cols: usize) -> Self {
        let mut line_text = Vec::with_capacity(cols);
        let mut line_attrs = Vec::with_capacity(cols);
        
        for byte in text.bytes().take(cols) {
            line_text.push(byte);
            line_attrs.push(attr);
        }
        
        Self { 
            text: line_text,
            attrs: line_attrs,
        }
    }

    /// Get the text as a string slice (lossy conversion for non-UTF8)
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.text).unwrap_or("")
    }

    /// Get the length of the line
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Check if the line is empty
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Clear the line
    pub fn clear(&mut self) {
        self.text.clear();
        self.attrs.clear();
    }

    /// Append a character with attribute to the line (if space available)
    pub fn push(&mut self, ch: u8, attr: u8, max_cols: usize) -> bool {
        if self.text.len() < max_cols {
            self.text.push(ch);
            self.attrs.push(attr);
            true
        } else {
            false
        }
    }
}

/// Scrollback buffer for VGA terminal text
///
/// Stores a history of text lines with a fixed viewport.
pub struct VgaScrollback {
    /// Maximum number of lines to store
    max_lines: usize,
    /// Width in columns
    cols: usize,
    /// Height of visible viewport in rows
    viewport_rows: usize,
    /// Ring buffer of lines
    lines: Vec<VgaLine>,
    /// Current viewport position (0 = bottom, showing most recent lines)
    viewport_offset: usize,
}

impl VgaScrollback {
    /// Create a new scrollback buffer
    ///
    /// # Arguments
    /// * `cols` - Width in columns
    /// * `viewport_rows` - Height of visible viewport
    /// * `max_lines` - Maximum lines to store (older lines are dropped)
    /// * `default_attr` - Default VGA attribute byte (unused, kept for API compatibility)
    pub fn new(cols: usize, viewport_rows: usize, max_lines: usize, _default_attr: u8) -> Self {
        Self {
            max_lines,
            cols,
            viewport_rows,
            lines: Vec::new(),
            viewport_offset: 0,
        }
    }

    /// Get the number of columns
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Get the viewport height
    pub fn viewport_rows(&self) -> usize {
        self.viewport_rows
    }

    /// Get the total number of lines stored
    pub fn total_lines(&self) -> usize {
        self.lines.len()
    }

    /// Get the current viewport offset from the bottom
    pub fn viewport_offset(&self) -> usize {
        self.viewport_offset
    }

    /// Add a new line to the buffer
    pub fn push_line(&mut self, text: &str, attr: u8) {
        let line = VgaLine::from_text(text, attr, self.cols);

        // Add new line
        self.lines.push(line);

        // Remove old lines if we exceed max
        while self.lines.len() > self.max_lines {
            self.lines.remove(0);
        }

        // Reset viewport to bottom when new content is added
        self.viewport_offset = 0;
    }

    /// Add multiple lines from text (splitting on newlines)
    pub fn push_text(&mut self, text: &str, attr: u8) {
        for line in text.lines() {
            self.push_line(line, attr);
        }
    }

    /// Scroll viewport up (show older content) by one page
    ///
    /// Returns true if scrolling occurred
    pub fn page_up(&mut self) -> bool {
        self.scroll_up(self.viewport_rows)
    }

    /// Scroll viewport down (show newer content) by one page
    ///
    /// Returns true if scrolling occurred
    pub fn page_down(&mut self) -> bool {
        self.scroll_down(self.viewport_rows)
    }

    /// Scroll viewport up (show older content)
    ///
    /// Returns true if scrolling occurred
    pub fn scroll_up(&mut self, lines: usize) -> bool {
        let max_scroll = self.max_scroll_up();
        if max_scroll > 0 {
            let scroll_amount = lines.min(max_scroll);
            self.viewport_offset += scroll_amount;
            true
        } else {
            false
        }
    }

    /// Scroll viewport down (show newer content)
    ///
    /// Returns true if scrolling occurred
    pub fn scroll_down(&mut self, lines: usize) -> bool {
        if self.viewport_offset > 0 {
            self.viewport_offset = self.viewport_offset.saturating_sub(lines);
            true
        } else {
            false
        }
    }

    /// Scroll to bottom (show most recent content)
    pub fn scroll_to_bottom(&mut self) {
        self.viewport_offset = 0;
    }

    /// Scroll to top (show oldest content)
    pub fn scroll_to_top(&mut self) {
        self.viewport_offset = self.max_scroll_up();
    }

    /// Get maximum possible scroll up distance
    fn max_scroll_up(&self) -> usize {
        if self.lines.len() > self.viewport_rows {
            self.lines.len() - self.viewport_rows
        } else {
            0
        }
    }

    /// Get the visible lines in the current viewport
    ///
    /// Returns a slice of lines that should be displayed
    pub fn visible_lines(&self) -> &[VgaLine] {
        let total = self.lines.len();
        if total == 0 {
            return &[];
        }

        // Calculate the start of the visible window
        let end = total - self.viewport_offset;
        let start = if end > self.viewport_rows {
            end - self.viewport_rows
        } else {
            0
        };

        &self.lines[start..end]
    }

    /// Clear all content
    pub fn clear(&mut self) {
        self.lines.clear();
        self.viewport_offset = 0;
    }

    /// Check if at bottom of buffer
    pub fn at_bottom(&self) -> bool {
        self.viewport_offset == 0
    }

    /// Check if at top of buffer
    pub fn at_top(&self) -> bool {
        self.viewport_offset >= self.max_scroll_up()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vga_line_creation() {
        let line = VgaLine::new(80, 0x07);
        assert_eq!(line.len(), 0);
        assert!(line.is_empty());
    }

    #[test]
    fn test_vga_line_from_text() {
        let line = VgaLine::from_text("Hello", 0x07, 80);
        assert_eq!(line.as_str(), "Hello");
        assert_eq!(line.len(), 5);
        assert_eq!(line.attrs.len(), 5);
        assert!(line.attrs.iter().all(|&a| a == 0x07));
    }

    #[test]
    fn test_vga_line_truncation() {
        let line = VgaLine::from_text("Hello World", 0x0A, 5);
        assert_eq!(line.as_str(), "Hello");
        assert_eq!(line.len(), 5);
    }

    #[test]
    fn test_vga_line_push() {
        let mut line = VgaLine::new(5, 0x07);
        assert!(line.push(b'a', 0x07, 5));
        assert!(line.push(b'b', 0x0A, 5));
        assert_eq!(line.as_str(), "ab");
        assert_eq!(line.attrs[0], 0x07);
        assert_eq!(line.attrs[1], 0x0A);
    }

    #[test]
    fn test_vga_scrollback_creation() {
        let buffer = VgaScrollback::new(80, 25, 1000, 0x07);
        assert_eq!(buffer.cols(), 80);
        assert_eq!(buffer.viewport_rows(), 25);
        assert_eq!(buffer.total_lines(), 0);
    }

    #[test]
    fn test_push_line() {
        let mut buffer = VgaScrollback::new(80, 25, 1000, 0x07);
        buffer.push_line("Line 1", 0x07);
        buffer.push_line("Line 2", 0x0A);
        assert_eq!(buffer.total_lines(), 2);
    }

    #[test]
    fn test_visible_lines_small_buffer() {
        let mut buffer = VgaScrollback::new(80, 25, 1000, 0x07);
        buffer.push_line("Line 1", 0x07);
        buffer.push_line("Line 2", 0x0A);

        let visible = buffer.visible_lines();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].as_str(), "Line 1");
        assert_eq!(visible[1].as_str(), "Line 2");
    }

    #[test]
    fn test_scroll_up() {
        let mut buffer = VgaScrollback::new(80, 3, 1000, 0x07);
        for i in 1..=10 {
            buffer.push_line(&alloc::format!("Line {}", i), 0x07);
        }

        assert!(buffer.scroll_up(2));
        let visible = buffer.visible_lines();
        assert_eq!(visible[0].as_str(), "Line 6");
        assert_eq!(visible[2].as_str(), "Line 8");
    }

    #[test]
    fn test_page_up_down() {
        let mut buffer = VgaScrollback::new(80, 3, 1000, 0x07);
        for i in 1..=10 {
            buffer.push_line(&alloc::format!("Line {}", i), 0x07);
        }

        // PageUp scrolls up by viewport_rows (3)
        assert!(buffer.page_up());
        assert_eq!(buffer.viewport_offset(), 3);

        // PageDown scrolls down by viewport_rows (3)
        assert!(buffer.page_down());
        assert_eq!(buffer.viewport_offset(), 0);
    }

    #[test]
    fn test_at_bottom() {
        let mut buffer = VgaScrollback::new(80, 3, 1000, 0x07);
        for i in 1..=10 {
            buffer.push_line(&alloc::format!("Line {}", i), 0x07);
        }

        assert!(buffer.at_bottom());
        buffer.scroll_up(1);
        assert!(!buffer.at_bottom());
        buffer.scroll_to_bottom();
        assert!(buffer.at_bottom());
    }

    #[test]
    fn test_max_lines_limit() {
        let mut buffer = VgaScrollback::new(80, 3, 5, 0x07);
        for i in 1..=10 {
            buffer.push_line(&alloc::format!("Line {}", i), 0x07);
        }

        // Should only keep last 5 lines
        assert_eq!(buffer.total_lines(), 5);
        let visible = buffer.visible_lines();
        assert_eq!(visible[0].as_str(), "Line 8");
        assert_eq!(visible[2].as_str(), "Line 10");
    }
}
