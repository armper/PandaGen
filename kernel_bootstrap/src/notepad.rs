//! Notepad (GFX-051): the desk's first app.
//!
//! Modeless. You type and it appears; there are no vi modes, no command
//! line, no leader key. Everything the app needs from the machine it asks
//! for through [`NotepadEffect`], so the whole thing is a pure state machine
//! that `cargo test` can drive on the host -- the desk performs the I/O and
//! reports back through [`Notepad::io_done`].
//!
//! It stands on `editor_core::TextBuffer`, which is the buffer this project
//! spent a round teaching not to eat trailing newlines, `\r`s or undo
//! history.
//!
//! Keys, as bytes from the kernel's scancode parser:
//!
//! | byte          | meaning                                  |
//! |---------------|------------------------------------------|
//! | printable     | insert                                   |
//! | `\n`          | new line                                 |
//! | `0x08`        | backspace                                |
//! | `0x80..=0x83` | arrows: up, down, left, right            |
//! | `0x84`        | delete                                   |
//! | `0x1A` Ctrl+Z | undo                                     |
//! | `0x13` Ctrl+S | save (asks for a name the first time)    |
//! | `0x0F` Ctrl+O | open by name                             |
//! | `0x0E` Ctrl+N | new document                             |
//! | `0x17` Ctrl+W | close (twice if unsaved)                 |
//! | `0x1B` Esc    | cancel a prompt                          |

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use editor_core::{Position, TextBuffer};

pub const KEY_UP: u8 = 0x80;
pub const KEY_DOWN: u8 = 0x81;
pub const KEY_LEFT: u8 = 0x82;
pub const KEY_RIGHT: u8 = 0x83;
pub const KEY_DELETE: u8 = 0x84;
pub const CTRL_N: u8 = 0x0E;
pub const CTRL_O: u8 = 0x0F;
pub const CTRL_S: u8 = 0x13;
pub const CTRL_T: u8 = 0x14;
pub const CTRL_W: u8 = 0x17;
pub const CTRL_Z: u8 = 0x1A;
pub const ESC: u8 = 0x1B;
pub const BACKSPACE: u8 = 0x08;

/// Bounded undo, like the editor's.
const MAX_UNDO: usize = 100;

/// What the app wants the desk to do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotepadEffect {
    /// Nothing changed on screen.
    None,
    /// Something changed on screen.
    Redraw,
    /// Write `content` to `path`, then call `io_done`.
    Save { path: String, content: String },
    /// Read `path`, then call `io_done`.
    Open { path: String },
    /// The user asked for the window to go.
    Close,
}

/// A one-line prompt in the footer, for a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Prompt {
    SaveAs(String),
    Open(String),
}

/// The Notepad's whole state.
#[derive(Debug, Clone)]
pub struct Notepad {
    buffer: TextBuffer,
    cursor: Position,
    undo: Vec<(TextBuffer, Position)>,
    dirty: bool,
    path: Option<String>,
    status: String,
    prompt: Option<Prompt>,
    /// First document line shown.
    scroll: usize,
    /// Set after the first Ctrl+W on a dirty document; a second closes.
    close_armed: bool,
    /// The wheel moved the view off the caret; cleared by the next edit or
    /// caret move so the view follows the caret again.
    scrolled_away: bool,
}

impl Default for Notepad {
    fn default() -> Self {
        Self::new()
    }
}

impl Notepad {
    pub fn new() -> Self {
        Self {
            buffer: TextBuffer::new(),
            cursor: Position::new(0, 0),
            undo: Vec::new(),
            dirty: false,
            path: None,
            status: String::new(),
            prompt: None,
            scroll: 0,
            close_armed: false,
            scrolled_away: false,
        }
    }

    /// Replace the document with `content` from `path`.
    pub fn load(&mut self, path: Option<String>, content: &str) {
        self.buffer = TextBuffer::from_string(content.to_string());
        self.cursor = Position::new(0, 0);
        self.undo.clear();
        self.dirty = false;
        self.path = path;
        self.prompt = None;
        self.scroll = 0;
        self.close_armed = false;
        self.scrolled_away = false;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn content(&self) -> String {
        self.buffer.as_string()
    }

    pub fn cursor(&self) -> Position {
        self.cursor
    }

    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// The header text: `name — Notepad`, with a mark when unsaved.
    pub fn title(&self) -> String {
        let name = self.path.as_deref().unwrap_or("Untitled");
        if self.dirty {
            alloc::format!("* {name} - Notepad")
        } else {
            alloc::format!("{name} - Notepad")
        }
    }

    /// The footer text: a prompt while one is open, otherwise position and
    /// saved state, with the last status message if there is one.
    pub fn footer(&self) -> String {
        match &self.prompt {
            Some(Prompt::SaveAs(name)) => {
                return alloc::format!("Save as: {name}_   (Enter to save, Esc to cancel)")
            }
            Some(Prompt::Open(name)) => {
                return alloc::format!("Open: {name}_   (Enter to open, Esc to cancel)")
            }
            None => {}
        }
        let col = self
            .buffer
            .line(self.cursor.row)
            .map(|line| line[..self.cursor.col.min(line.len())].chars().count())
            .unwrap_or(0);
        let mut text = alloc::format!(
            "Ln {}, Col {}   {} lines   {}",
            self.cursor.row + 1,
            col + 1,
            self.buffer.line_count(),
            if self.dirty { "Unsaved" } else { "Saved" }
        );
        if !self.status.is_empty() {
            text.push_str("   ");
            text.push_str(&self.status);
        }
        text
    }

    /// The lines the window shows, `rows` of them from `scroll`.
    pub fn viewport_lines(&mut self, rows: usize) -> Vec<String> {
        self.keep_cursor_visible(rows);
        (self.scroll..self.scroll + rows)
            .filter_map(|row| self.buffer.line(row).map(|l| l.to_string()))
            .collect()
    }

    /// The caret within the viewport, as `(line, character column)`.
    pub fn viewport_cursor(&self) -> Option<(usize, usize)> {
        let line = self.cursor.row.checked_sub(self.scroll)?;
        let col = self
            .buffer
            .line(self.cursor.row)
            .map(|text| text[..self.cursor.col.min(text.len())].chars().count())
            .unwrap_or(0);
        Some((line, col))
    }

    /// Put the caret where the pointer clicked: `view_line` is relative to
    /// the viewport and `column` counts characters, so both are mapped back
    /// to the document (GFX-052).
    pub fn place_cursor(&mut self, view_line: usize, column: usize) {
        let row = (self.scroll + view_line).min(self.buffer.line_count().saturating_sub(1));
        let line = self.buffer.line(row).unwrap_or("");
        let byte_col = line
            .char_indices()
            .map(|(index, _)| index)
            .nth(column)
            .unwrap_or(line.len());
        self.cursor = Position::new(row, byte_col);
        self.close_armed = false;
    }

    /// Scroll the view by `delta` lines (negative towards the top). The
    /// caret stays where it is; the next key brings the view back to it.
    pub fn scroll_by(&mut self, delta: i32) {
        let max = self.buffer.line_count().saturating_sub(1);
        let next = (self.scroll as i64 + delta as i64).clamp(0, max as i64) as usize;
        self.scroll = next;
        self.scrolled_away = true;
    }

    fn keep_cursor_visible(&mut self, rows: usize) {
        // After a wheel scroll the view is allowed to sit away from the
        // caret until something moves the caret.
        if self.scrolled_away {
            return;
        }
        if rows == 0 {
            return;
        }
        if self.cursor.row < self.scroll {
            self.scroll = self.cursor.row;
        } else if self.cursor.row >= self.scroll + rows {
            self.scroll = self.cursor.row + 1 - rows;
        }
    }

    /// The desk reports the outcome of a `Save` or `Open` here.
    pub fn io_done(&mut self, effect: &NotepadEffect, result: Result<Option<String>, String>) {
        match (effect, result) {
            (NotepadEffect::Save { path, .. }, Ok(_)) => {
                self.path = Some(path.clone());
                self.dirty = false;
                self.close_armed = false;
                self.status = alloc::format!("Saved {path}");
            }
            (NotepadEffect::Open { path }, Ok(Some(content))) => {
                self.load(Some(path.clone()), &content);
                self.status = alloc::format!("Opened {path}");
            }
            (NotepadEffect::Open { path }, Ok(None)) => {
                self.status = alloc::format!("Not found: {path}");
            }
            (_, Err(err)) => {
                self.status = alloc::format!("Error: {err}");
            }
            _ => {}
        }
    }

    /// One key.
    pub fn handle_byte(&mut self, byte: u8) -> NotepadEffect {
        self.status.clear();
        // Any key brings the view back to the caret.
        self.scrolled_away = false;
        if self.prompt.is_some() {
            return self.handle_prompt_byte(byte);
        }
        match byte {
            CTRL_S => {
                if let Some(path) = &self.path {
                    NotepadEffect::Save {
                        path: path.clone(),
                        content: self.content(),
                    }
                } else {
                    self.prompt = Some(Prompt::SaveAs(String::new()));
                    NotepadEffect::Redraw
                }
            }
            CTRL_O => {
                self.prompt = Some(Prompt::Open(String::new()));
                NotepadEffect::Redraw
            }
            CTRL_N => {
                if self.dirty && !self.close_armed {
                    self.close_armed = true;
                    self.status = "Unsaved changes: Ctrl+N again to discard".to_string();
                    return NotepadEffect::Redraw;
                }
                self.load(None, "");
                NotepadEffect::Redraw
            }
            CTRL_W => {
                if self.dirty && !self.close_armed {
                    self.close_armed = true;
                    self.status = "Unsaved changes: Ctrl+W again to close".to_string();
                    return NotepadEffect::Redraw;
                }
                NotepadEffect::Close
            }
            CTRL_Z => {
                if let Some((buffer, cursor)) = self.undo.pop() {
                    self.buffer = buffer;
                    self.cursor = cursor;
                    self.dirty = true;
                    NotepadEffect::Redraw
                } else {
                    self.status = "Nothing to undo".to_string();
                    NotepadEffect::Redraw
                }
            }
            KEY_UP => self.move_vertical(-1),
            KEY_DOWN => self.move_vertical(1),
            KEY_LEFT => {
                if self.cursor.col > 0 {
                    self.cursor.col =
                        floor_boundary(self.line(self.cursor.row), self.cursor.col - 1);
                } else if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                    self.cursor.col = self.buffer.line_length(self.cursor.row);
                }
                NotepadEffect::Redraw
            }
            KEY_RIGHT => {
                let len = self.buffer.line_length(self.cursor.row);
                if self.cursor.col < len {
                    self.cursor.col =
                        ceil_boundary(self.line(self.cursor.row), self.cursor.col + 1);
                } else if self.cursor.row + 1 < self.buffer.line_count() {
                    self.cursor.row += 1;
                    self.cursor.col = 0;
                }
                NotepadEffect::Redraw
            }
            BACKSPACE => {
                let before = self.snapshot();
                match self.buffer.backspace(self.cursor) {
                    Some(pos) => {
                        self.push_undo(before);
                        self.cursor = pos;
                        NotepadEffect::Redraw
                    }
                    None => NotepadEffect::None,
                }
            }
            KEY_DELETE => {
                let before = self.snapshot();
                if self.buffer.delete_char(self.cursor) {
                    self.push_undo(before);
                    NotepadEffect::Redraw
                } else {
                    NotepadEffect::None
                }
            }
            b'\n' | b'\r' => {
                let before = self.snapshot();
                if self.buffer.insert_newline(self.cursor) {
                    self.push_undo(before);
                    self.cursor.row += 1;
                    self.cursor.col = 0;
                    NotepadEffect::Redraw
                } else {
                    NotepadEffect::None
                }
            }
            ESC => {
                self.close_armed = false;
                NotepadEffect::Redraw
            }
            b'\t' => {
                // Four spaces: the one font has no tab stop, and a tab
                // character would render as nothing.
                let before = self.snapshot();
                let mut inserted = 0;
                for _ in 0..4 {
                    if self.buffer.insert_char(self.cursor, ' ') {
                        self.cursor.col += 1;
                        inserted += 1;
                    }
                }
                if inserted > 0 {
                    self.push_undo(before);
                    NotepadEffect::Redraw
                } else {
                    NotepadEffect::None
                }
            }
            0x20..=0x7E => {
                let before = self.snapshot();
                if self.buffer.insert_char(self.cursor, byte as char) {
                    self.push_undo(before);
                    self.cursor.col += 1;
                    NotepadEffect::Redraw
                } else {
                    NotepadEffect::None
                }
            }
            _ => NotepadEffect::None,
        }
    }

    fn handle_prompt_byte(&mut self, byte: u8) -> NotepadEffect {
        let Some(prompt) = self.prompt.as_mut() else {
            return NotepadEffect::None;
        };
        let name = match prompt {
            Prompt::SaveAs(name) | Prompt::Open(name) => name,
        };
        match byte {
            ESC => {
                self.prompt = None;
                NotepadEffect::Redraw
            }
            BACKSPACE => {
                name.pop();
                NotepadEffect::Redraw
            }
            b'\n' | b'\r' => {
                let name = name.trim().to_string();
                let prompt = self.prompt.take();
                if name.is_empty() {
                    self.status = "A name is needed".to_string();
                    return NotepadEffect::Redraw;
                }
                match prompt {
                    Some(Prompt::SaveAs(_)) => NotepadEffect::Save {
                        path: name,
                        content: self.content(),
                    },
                    Some(Prompt::Open(_)) => NotepadEffect::Open { path: name },
                    None => NotepadEffect::None,
                }
            }
            0x20..=0x7E if name.len() < 64 => {
                name.push(byte as char);
                NotepadEffect::Redraw
            }
            _ => NotepadEffect::None,
        }
    }

    fn move_vertical(&mut self, delta: isize) -> NotepadEffect {
        let target = self.cursor.row as isize + delta;
        if target < 0 || target as usize >= self.buffer.line_count() {
            return NotepadEffect::None;
        }
        self.cursor.row = target as usize;
        let len = self.buffer.line_length(self.cursor.row);
        self.cursor.col = floor_boundary(self.line(self.cursor.row), self.cursor.col.min(len));
        NotepadEffect::Redraw
    }

    fn line(&self, row: usize) -> &str {
        self.buffer.line(row).unwrap_or("")
    }

    fn snapshot(&self) -> (TextBuffer, Position) {
        (self.buffer.clone(), self.cursor)
    }

    /// Push only after an edit landed -- E7's lesson.
    fn push_undo(&mut self, before: (TextBuffer, Position)) {
        self.undo.push(before);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.dirty = true;
        self.close_armed = false;
    }
}

/// The largest character boundary at or before `col` -- E3's lesson.
fn floor_boundary(line: &str, col: usize) -> usize {
    let mut col = col.min(line.len());
    while col > 0 && !line.is_char_boundary(col) {
        col -= 1;
    }
    col
}

/// The smallest character boundary at or after `col`.
fn ceil_boundary(line: &str, col: usize) -> usize {
    let mut col = col.min(line.len());
    while col < line.len() && !line.is_char_boundary(col) {
        col += 1;
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(pad: &mut Notepad, text: &str) {
        for byte in text.bytes() {
            pad.handle_byte(byte);
        }
    }

    #[test]
    fn typing_appears_and_marks_the_document_unsaved() {
        let mut pad = Notepad::new();
        type_str(&mut pad, "hello");
        pad.handle_byte(b'\n');
        type_str(&mut pad, "world");
        assert_eq!(pad.content(), "hello\nworld");
        assert!(pad.is_dirty());
        assert!(pad.title().starts_with("* Untitled"));
        assert!(pad.footer().contains("Ln 2, Col 6"));
    }

    #[test]
    fn the_first_save_asks_for_a_name_and_the_second_does_not() {
        let mut pad = Notepad::new();
        type_str(&mut pad, "note");
        assert_eq!(pad.handle_byte(CTRL_S), NotepadEffect::Redraw);
        assert!(pad.footer().starts_with("Save as:"));
        type_str(&mut pad, "a.txt");
        let effect = pad.handle_byte(b'\n');
        assert_eq!(
            effect,
            NotepadEffect::Save {
                path: "a.txt".to_string(),
                content: "note".to_string()
            }
        );
        pad.io_done(&effect, Ok(None));
        assert!(!pad.is_dirty());
        assert_eq!(pad.path(), Some("a.txt"));

        type_str(&mut pad, "!");
        assert!(
            matches!(pad.handle_byte(CTRL_S), NotepadEffect::Save { path, .. } if path == "a.txt")
        );
    }

    #[test]
    fn undo_restores_what_the_last_edit_changed_and_no_ops_do_not_count() {
        let mut pad = Notepad::new();
        type_str(&mut pad, "ab");
        // Backspace at the start of an empty second line is not an edit.
        for _ in 0..50 {
            pad.handle_byte(KEY_LEFT);
        }
        for _ in 0..50 {
            assert_eq!(pad.handle_byte(BACKSPACE), NotepadEffect::None);
        }
        pad.handle_byte(CTRL_Z);
        assert_eq!(
            pad.content(),
            "a",
            "one undo must remove exactly one insert"
        );
        pad.handle_byte(CTRL_Z);
        assert_eq!(pad.content(), "");
    }

    #[test]
    fn arrows_move_by_characters_not_bytes() {
        let mut pad = Notepad::new();
        pad.load(None, "héllo");
        for _ in 0..3 {
            pad.handle_byte(KEY_RIGHT);
        }
        // Three characters in is byte 4; the caret column reports characters.
        assert_eq!(pad.cursor().col, 4);
        assert_eq!(pad.viewport_cursor(), Some((0, 3)));
        pad.handle_byte(KEY_LEFT);
        assert_eq!(pad.cursor().col, 3);
    }

    #[test]
    fn closing_an_unsaved_document_takes_two_asks() {
        let mut pad = Notepad::new();
        type_str(&mut pad, "x");
        assert_eq!(pad.handle_byte(CTRL_W), NotepadEffect::Redraw);
        assert!(pad.footer().contains("Ctrl+W again"));
        assert_eq!(pad.handle_byte(CTRL_W), NotepadEffect::Close);

        let mut clean = Notepad::new();
        assert_eq!(clean.handle_byte(CTRL_W), NotepadEffect::Close);
    }

    #[test]
    fn the_viewport_follows_the_cursor() {
        let mut pad = Notepad::new();
        for i in 0..30 {
            type_str(&mut pad, &alloc::format!("line {i}"));
            pad.handle_byte(b'\n');
        }
        let lines = pad.viewport_lines(10);
        assert_eq!(lines.len(), 10);
        assert_eq!(pad.viewport_cursor().map(|(l, _)| l), Some(9));
        for _ in 0..40 {
            pad.handle_byte(KEY_UP);
        }
        let lines = pad.viewport_lines(10);
        assert_eq!(lines[0], "line 0");
        assert_eq!(pad.viewport_cursor(), Some((0, 0)));
    }

    #[test]
    fn a_click_places_the_caret_by_characters_and_the_wheel_scrolls_the_view() {
        // Loaded, not typed: the key path is one byte at a time and only
        // takes ASCII, so a typed 'é' would arrive as two bytes it ignores.
        let text: String = (0..40).map(|i| alloc::format!("héllo {i}\n")).collect();
        let mut pad = Notepad::new();
        pad.load(None, &text);
        for _ in 0..45 {
            pad.handle_byte(KEY_DOWN);
        }
        // Forty lines, caret on the last: the view shows 30..39.
        let _ = pad.viewport_lines(10);
        pad.scroll_by(-20);
        let lines = pad.viewport_lines(10);
        assert_eq!(lines[0], "héllo 10", "the wheel did not move the view");
        // The caret has not moved and the view stays where the wheel put it.
        assert_eq!(pad.viewport_lines(10)[0], "héllo 10");

        // Click on the second visible line, after 'héll' (4 characters).
        pad.place_cursor(1, 4);
        assert_eq!(
            pad.cursor(),
            Position::new(11, 5),
            "4 characters is 5 bytes here"
        );
        assert_eq!(pad.viewport_cursor(), Some((1, 4)));

        // The next key brings the view back to the caret's neighbourhood.
        pad.handle_byte(b'!');
        assert!(pad.viewport_cursor().is_some());
    }

    #[test]
    fn tab_is_four_spaces_and_one_undo() {
        let mut pad = Notepad::new();
        pad.handle_byte(b'\t');
        assert_eq!(pad.content(), "    ");
        pad.handle_byte(CTRL_Z);
        assert_eq!(pad.content(), "");
    }

    #[test]
    fn a_round_trip_through_load_and_content_keeps_the_file_intact() {
        let mut pad = Notepad::new();
        pad.load(Some("n.txt".to_string()), "one\r\ntwo\n");
        assert_eq!(pad.content(), "one\r\ntwo\n");
        assert!(!pad.is_dirty());
        assert_eq!(pad.title(), "n.txt - Notepad");
    }
}
