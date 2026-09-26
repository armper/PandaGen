//! # Scrollback Buffer
//!
//! This module provides a scrollback buffer for storing terminal text history.
//!
//! ## Design
//!
//! - Fixed-width text grid (cols × rows)
//! - Ring buffer for efficient scrolling
//! - Viewport tracks visible portion
//! - No ANSI codes, just plain text

use alloc::vec::Vec;

/// A line of text in the scrollback buffer
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// Text content (up to cols characters)
    pub text: Vec<u8>,
}

impl Line {
    /// Create a new empty line
    pub fn new(cols: usize) -> Self {
        Self {
            text: Vec::with_capacity(cols),
        }
    }

    /// Create a line from text, keeping the first `cols` characters.
    ///
    /// Two bugs lived here in turn, and the second was mine. First this
    /// truncated by raw bytes while `as_str` returned `""` for invalid
    /// UTF-8, so a line whose right margin fell inside a multi-byte
    /// character displayed as *nothing at all*. The fix cut on a character
    /// boundary -- and kept counting bytes, while `cols` means display
    /// columns everywhere else in this file, including the doc two fields
    /// up. So a 9-column line of two-byte characters kept four of them, and
    /// a line of box-drawing or CJK text showed a third of itself. "The
    /// whole line vanished" became "most of the line vanished".
    ///
    /// `cols` is a column count. One character, one column.
    pub fn from_text(text: &str, cols: usize) -> Self {
        Self {
            text: text
                .chars()
                .take(cols)
                .collect::<alloc::string::String>()
                .into_bytes(),
        }
    }

    /// Get the text as a string slice.
    ///
    /// Falls back to the longest valid prefix rather than to `""`. A line
    /// assembled byte by byte through `push` can still be cut mid-character,
    /// and losing the character is right where losing the line is not.
    pub fn as_str(&self) -> &str {
        match core::str::from_utf8(&self.text) {
            Ok(text) => text,
            Err(err) => {
                // `valid_up_to` is a character boundary by construction.
                core::str::from_utf8(&self.text[..err.valid_up_to()]).unwrap_or("")
            }
        }
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
    }

    /// Append one byte to the line, if there is room.
    ///
    /// Byte-level, unlike [`Self::from_text`], because the console feeds it
    /// one byte at a time from a serial-style stream and a multi-byte
    /// character arrives in pieces. `max_cols` is therefore a byte ceiling
    /// here, which for the ASCII path this is used on is the same number.
    /// [`Self::as_str`] copes with a sequence cut off part-way.
    pub fn push(&mut self, ch: u8, max_cols: usize) -> bool {
        if self.text.len() < max_cols {
            self.text.push(ch);
            true
        } else {
            false
        }
    }
}

/// Scrollback buffer for terminal text
///
/// Stores a history of text lines with a fixed viewport.
pub struct ScrollbackBuffer {
    /// Maximum number of lines to store
    max_lines: usize,
    /// Width in columns
    cols: usize,
    /// Height of visible viewport in rows
    viewport_rows: usize,
    /// The lines, oldest first, from `start` on. Lines before `start`
    /// have been dropped and are freed in bulk: dropping one line at a
    /// time with `remove(0)` shifted every line kept, on every line pushed
    /// once the buffer was full.
    lines: Vec<Line>,
    start: usize,
    /// Current viewport position (0 = bottom, showing most recent lines)
    viewport_offset: usize,
}

impl ScrollbackBuffer {
    /// Create a new scrollback buffer
    ///
    /// # Arguments
    /// * `cols` - Width in columns
    /// * `viewport_rows` - Height of visible viewport
    /// * `max_lines` - Maximum lines to store (older lines are dropped)
    pub fn new(cols: usize, viewport_rows: usize, max_lines: usize) -> Self {
        Self {
            max_lines,
            cols,
            viewport_rows,
            lines: Vec::new(),
            start: 0,
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
        self.live().len()
    }

    /// Get the current viewport offset from the bottom
    pub fn viewport_offset(&self) -> usize {
        self.viewport_offset
    }

    /// Add a new line to the buffer
    pub fn push_line(&mut self, text: &str) {
        let line = Line::from_text(text, self.cols);

        // Add new line
        self.lines.push(line);

        // Remove old lines if we exceed max
        if self.live().len() > self.max_lines {
            self.start += 1;
            // Free the dropped lines once they are as many as those kept:
            // one shift per `max_lines` pushes.
            if self.start >= self.max_lines.max(1) {
                self.lines.drain(..self.start);
                self.start = 0;
            }
        }

        // Reset viewport to bottom when new content is added
        self.viewport_offset = 0;
    }

    /// Add multiple lines from text (splitting on newlines)
    pub fn push_text(&mut self, text: &str) {
        for line in text.lines() {
            self.push_line(line);
        }
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
        if self.live().len() > self.viewport_rows {
            self.live().len() - self.viewport_rows
        } else {
            0
        }
    }

    /// Get the visible lines in the current viewport
    ///
    /// Returns a slice of lines that should be displayed
    pub fn visible_lines(&self) -> &[Line] {
        let total = self.live().len();
        if total == 0 {
            return &[];
        }

        // Calculate the start of the visible window
        let end = total - self.viewport_offset;
        let start = end.saturating_sub(self.viewport_rows);

        &self.live()[start..end]
    }

    /// The lines kept.
    fn live(&self) -> &[Line] {
        &self.lines[self.start..]
    }

    /// Clear all content
    pub fn clear(&mut self) {
        self.lines.clear();
        self.start = 0;
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
    fn test_line_creation() {
        let line = Line::new(80);
        assert_eq!(line.len(), 0);
        assert!(line.is_empty());
    }

    #[test]
    fn test_line_from_text() {
        let line = Line::from_text("Hello", 80);
        assert_eq!(line.as_str(), "Hello");
        assert_eq!(line.len(), 5);
    }

    #[test]
    fn test_line_truncation() {
        let line = Line::from_text("Hello World", 5);
        assert_eq!(line.as_str(), "Hello");
        assert_eq!(line.len(), 5);
    }

    #[test]
    fn test_line_push() {
        let mut line = Line::new(5);
        assert!(line.push(b'a', 5));
        assert!(line.push(b'b', 5));
        assert_eq!(line.as_str(), "ab");
    }

    #[test]
    fn test_line_push_at_capacity() {
        let mut line = Line::new(2);
        assert!(line.push(b'a', 2));
        assert!(line.push(b'b', 2));
        assert!(!line.push(b'c', 2)); // Should fail
        assert_eq!(line.as_str(), "ab");
    }

    #[test]
    fn test_scrollback_creation() {
        let buffer = ScrollbackBuffer::new(80, 25, 1000);
        assert_eq!(buffer.cols(), 80);
        assert_eq!(buffer.viewport_rows(), 25);
        assert_eq!(buffer.total_lines(), 0);
    }

    #[test]
    fn test_push_line() {
        let mut buffer = ScrollbackBuffer::new(80, 25, 1000);
        buffer.push_line("Line 1");
        buffer.push_line("Line 2");
        assert_eq!(buffer.total_lines(), 2);
    }

    #[test]
    fn test_push_text_multiple_lines() {
        let mut buffer = ScrollbackBuffer::new(80, 25, 1000);
        buffer.push_text("Line 1\nLine 2\nLine 3");
        assert_eq!(buffer.total_lines(), 3);
    }

    #[test]
    fn test_visible_lines_small_buffer() {
        let mut buffer = ScrollbackBuffer::new(80, 25, 1000);
        buffer.push_line("Line 1");
        buffer.push_line("Line 2");

        let visible = buffer.visible_lines();
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].as_str(), "Line 1");
        assert_eq!(visible[1].as_str(), "Line 2");
    }

    #[test]
    fn test_visible_lines_large_buffer() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 1000);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        let visible = buffer.visible_lines();
        assert_eq!(visible.len(), 3);
        assert_eq!(visible[0].as_str(), "Line 8");
        assert_eq!(visible[1].as_str(), "Line 9");
        assert_eq!(visible[2].as_str(), "Line 10");
    }

    #[test]
    fn test_scroll_up() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 1000);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        assert!(buffer.scroll_up(2));
        let visible = buffer.visible_lines();
        assert_eq!(visible[0].as_str(), "Line 6");
        assert_eq!(visible[1].as_str(), "Line 7");
        assert_eq!(visible[2].as_str(), "Line 8");
    }

    #[test]
    fn test_scroll_down() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 1000);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        buffer.scroll_up(5);
        assert!(buffer.scroll_down(2));

        let visible = buffer.visible_lines();
        // After scrolling up 5, we're at offset 5, viewing lines 3-5
        // After scrolling down 2, we're at offset 3, viewing lines 5-7
        assert_eq!(visible[0].as_str(), "Line 5");
        assert_eq!(visible[1].as_str(), "Line 6");
        assert_eq!(visible[2].as_str(), "Line 7");
    }

    #[test]
    fn test_scroll_to_bottom() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 1000);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        buffer.scroll_up(5);
        buffer.scroll_to_bottom();

        let visible = buffer.visible_lines();
        assert_eq!(visible[0].as_str(), "Line 8");
        assert_eq!(visible[2].as_str(), "Line 10");
    }

    #[test]
    fn test_scroll_to_top() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 1000);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        buffer.scroll_to_top();

        let visible = buffer.visible_lines();
        assert_eq!(visible[0].as_str(), "Line 1");
        assert_eq!(visible[2].as_str(), "Line 3");
    }

    #[test]
    fn test_at_bottom() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 1000);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        assert!(buffer.at_bottom());
        buffer.scroll_up(1);
        assert!(!buffer.at_bottom());
        buffer.scroll_to_bottom();
        assert!(buffer.at_bottom());
    }

    #[test]
    fn test_at_top() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 1000);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        assert!(!buffer.at_top());
        buffer.scroll_to_top();
        assert!(buffer.at_top());
    }

    #[test]
    fn test_a_long_run_keeps_the_newest_and_frees_the_rest() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 5);
        for i in 1..=1000 {
            buffer.push_line(&format!("Line {}", i));
            assert!(buffer.lines.len() <= 10, "dropped lines are freed in bulk");
        }
        assert_eq!(buffer.total_lines(), 5);
        let visible = buffer.visible_lines();
        assert_eq!(visible[0].as_str(), "Line 998");
        assert_eq!(visible[2].as_str(), "Line 1000");
        buffer.clear();
        buffer.push_line("again");
        assert_eq!(buffer.visible_lines()[0].as_str(), "again");
    }

    #[test]
    fn test_max_lines_limit() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 5);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        // Should only keep last 5 lines
        assert_eq!(buffer.total_lines(), 5);
        let visible = buffer.visible_lines();
        assert_eq!(visible[0].as_str(), "Line 8");
        assert_eq!(visible[2].as_str(), "Line 10");
    }

    #[test]
    fn test_clear() {
        let mut buffer = ScrollbackBuffer::new(80, 25, 1000);
        buffer.push_line("Line 1");
        buffer.push_line("Line 2");
        buffer.clear();

        assert_eq!(buffer.total_lines(), 0);
        assert_eq!(buffer.visible_lines().len(), 0);
    }

    #[test]
    fn test_viewport_offset_reset_on_new_content() {
        let mut buffer = ScrollbackBuffer::new(80, 3, 1000);
        for i in 1..=10 {
            buffer.push_line(&format!("Line {}", i));
        }

        buffer.scroll_up(5);
        assert!(!buffer.at_bottom());

        // Adding new content should reset to bottom
        buffer.push_line("New Line");
        assert!(buffer.at_bottom());
    }
}
