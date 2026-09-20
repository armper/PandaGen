//! Text buffer and position types

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[cfg(feature = "serde_support")]
use serde::{Deserialize, Serialize};

/// Cursor position in the buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde_support", derive(Serialize, Deserialize))]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

impl Position {
    pub const fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    pub const fn zero() -> Self {
        Self { row: 0, col: 0 }
    }
}

/// Text buffer with line-based storage
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBuffer {
    lines: Vec<String>,
    /// Whether the text this buffer was loaded from ended with a newline,
    /// so saving it back does not quietly remove one.
    trailing_newline: bool,
}

/// The largest character boundary at or before `col`.
///
/// The cursor column is counted in keystrokes but used as a byte index.
/// On any line holding a character outside ASCII the two disagree, and
/// `String::remove`, `insert` and `split_off` panic on an index inside a
/// character. The kernel aborts on panic, so that is the machine gone, not
/// just the editor. Snapping to a boundary makes the operation act on the
/// whole character, which is what the user meant.
pub(crate) fn floor_boundary(line: &str, col: usize) -> usize {
    let mut col = col.min(line.len());
    while col > 0 && !line.is_char_boundary(col) {
        col -= 1;
    }
    col
}

impl TextBuffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            trailing_newline: false,
        }
    }

    /// Load `content`, remembering how it ended.
    ///
    /// This used `str::lines()`, which discards the final newline that every
    /// text file ends with *and* strips a `\r` from the end of every line.
    /// So opening a file in the kernel's editor and saving it -- without
    /// typing anything -- wrote it back a byte shorter, and a CRLF file came
    /// back with every line ending rewritten.
    ///
    /// E8 fixed exactly this in `services_editor_vi::TextBuffer` and left the
    /// buffer next door, which is the one `kernel_bootstrap`'s editor writes
    /// to disk. Splitting on `\n` rather than using `lines()` keeps the
    /// `\r`; the flag keeps the last newline.
    pub fn from_string(content: String) -> Self {
        let trailing_newline = content.ends_with('\n');
        let lines = if content.is_empty() {
            vec![String::new()]
        } else {
            let body = if trailing_newline {
                &content[..content.len() - 1]
            } else {
                &content[..]
            };
            body.split('\n').map(String::from).collect()
        };
        Self {
            lines,
            trailing_newline,
        }
    }

    pub fn as_string(&self) -> String {
        let mut text = self.lines.join("\n");
        if self.trailing_newline {
            text.push('\n');
        }
        text
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, row: usize) -> Option<&str> {
        self.lines.get(row).map(|s| s.as_str())
    }

    pub fn line_length(&self, row: usize) -> usize {
        self.lines.get(row).map(|s| s.len()).unwrap_or(0)
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Insert a character at position
    pub fn insert_char(&mut self, pos: Position, ch: char) -> bool {
        if pos.row >= self.lines.len() {
            return false;
        }

        let line = &mut self.lines[pos.row];
        if pos.col > line.len() {
            return false;
        }

        let at = floor_boundary(line, pos.col);
        line.insert(at, ch);
        true
    }

    /// Insert a newline at position, splitting the line
    pub fn insert_newline(&mut self, pos: Position) -> bool {
        if pos.row >= self.lines.len() {
            return false;
        }

        let line = &mut self.lines[pos.row];
        if pos.col > line.len() {
            return false;
        }

        let at = floor_boundary(line, pos.col);
        let rest = line.split_off(at);
        self.lines.insert(pos.row + 1, rest);
        true
    }

    /// Delete character at position
    pub fn delete_char(&mut self, pos: Position) -> bool {
        if pos.row >= self.lines.len() {
            return false;
        }

        let line = &mut self.lines[pos.row];
        if pos.col >= line.len() {
            return false;
        }

        line.remove(floor_boundary(line, pos.col));
        true
    }

    /// Delete character before position (backspace)
    /// Returns new cursor position if successful
    pub fn backspace(&mut self, pos: Position) -> Option<Position> {
        if pos.col > 0 {
            // Delete character on same line
            let line = &mut self.lines[pos.row];
            let at = floor_boundary(line, pos.col);
            let previous = floor_boundary(line, at.saturating_sub(1));
            if previous == at {
                return Some(Position::new(pos.row, at));
            }
            line.remove(previous);
            Some(Position::new(pos.row, previous))
        } else if pos.row > 0 {
            // Join with previous line
            let current_line = self.lines.remove(pos.row);
            let prev_line = &mut self.lines[pos.row - 1];
            let new_col = prev_line.len();
            prev_line.push_str(&current_line);
            Some(Position::new(pos.row - 1, new_col))
        } else {
            None
        }
    }

    /// Delete entire line at row
    pub fn delete_line(&mut self, row: usize) -> bool {
        if row >= self.lines.len() {
            return false;
        }

        if self.lines.len() > 1 {
            self.lines.remove(row);
        } else {
            // Last line, just clear it
            self.lines[row].clear();
        }
        true
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use alloc::string::ToString;

    // The cursor column counts keystrokes but indexes bytes. On a line with
    // any character outside ASCII the two disagree, and `String::remove`,
    // `insert` and slicing panic on an index that is not a char boundary.
    // The kernel aborts on panic, so this is the machine, not the editor.

    #[test]
    fn deleting_inside_a_multi_byte_character_does_not_panic() {
        let mut buffer = TextBuffer::from_string("héllo".to_string());
        // Two presses of `l` from the start: col 2 is inside the `é`.
        assert!(buffer.delete_char(Position::new(0, 2)));
        assert_eq!(buffer.line(0), Some("hllo"), "the whole character goes");
    }

    #[test]
    fn inserting_inside_a_multi_byte_character_does_not_panic() {
        let mut buffer = TextBuffer::from_string("héllo".to_string());
        assert!(buffer.insert_char(Position::new(0, 2), 'x'));
        assert!(
            buffer.line(0).is_some_and(|l| l.contains('x')),
            "the character must land somewhere sensible"
        );
    }

    #[test]
    fn backspacing_inside_a_multi_byte_character_does_not_panic() {
        let mut buffer = TextBuffer::from_string("héllo".to_string());
        assert!(buffer.backspace(Position::new(0, 3)).is_some());
    }

    #[test]
    fn splitting_a_line_inside_a_multi_byte_character_does_not_panic() {
        let mut buffer = TextBuffer::from_string("héllo".to_string());
        assert!(buffer.insert_newline(Position::new(0, 2)));
        assert_eq!(buffer.line_count(), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_position() {
        let pos = Position::new(5, 10);
        assert_eq!(pos.row, 5);
        assert_eq!(pos.col, 10);

        let zero = Position::zero();
        assert_eq!(zero.row, 0);
        assert_eq!(zero.col, 0);
    }

    #[test]
    fn test_text_buffer_new() {
        let buffer = TextBuffer::new();
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.line(0), Some(""));
    }

    #[test]
    fn test_text_buffer_from_string() {
        let buffer = TextBuffer::from_string("hello\nworld".into());
        assert_eq!(buffer.line_count(), 2);
        assert_eq!(buffer.line(0), Some("hello"));
        assert_eq!(buffer.line(1), Some("world"));
    }

    #[test]
    fn test_text_buffer_to_string() {
        let mut buffer = TextBuffer::new();
        buffer.lines = vec!["hello".into(), "world".into()];
        assert_eq!(buffer.as_string(), "hello\nworld");
    }

    #[test]
    fn test_insert_char() {
        let mut buffer = TextBuffer::from_string("hello".into());
        assert!(buffer.insert_char(Position::new(0, 5), '!'));
        assert_eq!(buffer.line(0), Some("hello!"));
    }

    #[test]
    fn test_insert_newline() {
        let mut buffer = TextBuffer::from_string("hello".into());
        assert!(buffer.insert_newline(Position::new(0, 2)));
        assert_eq!(buffer.line_count(), 2);
        assert_eq!(buffer.line(0), Some("he"));
        assert_eq!(buffer.line(1), Some("llo"));
    }

    #[test]
    fn test_delete_char() {
        let mut buffer = TextBuffer::from_string("hello".into());
        assert!(buffer.delete_char(Position::new(0, 0)));
        assert_eq!(buffer.line(0), Some("ello"));
    }

    #[test]
    fn test_backspace() {
        let mut buffer = TextBuffer::from_string("hello".into());
        let new_pos = buffer.backspace(Position::new(0, 5));
        assert_eq!(new_pos, Some(Position::new(0, 4)));
        assert_eq!(buffer.line(0), Some("hell"));
    }

    #[test]
    fn test_backspace_line_join() {
        let mut buffer = TextBuffer::from_string("hello\nworld".into());
        let new_pos = buffer.backspace(Position::new(1, 0));
        assert_eq!(new_pos, Some(Position::new(0, 5)));
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.line(0), Some("helloworld"));
    }

    #[test]
    fn test_delete_line() {
        let mut buffer = TextBuffer::from_string("line1\nline2\nline3".into());
        assert!(buffer.delete_line(1));
        assert_eq!(buffer.line_count(), 2);
        assert_eq!(buffer.line(0), Some("line1"));
        assert_eq!(buffer.line(1), Some("line3"));
    }

    #[test]
    fn test_delete_last_line() {
        let mut buffer = TextBuffer::from_string("only".into());
        assert!(buffer.delete_line(0));
        assert_eq!(buffer.line_count(), 1);
        assert_eq!(buffer.line(0), Some(""));
    }
}
