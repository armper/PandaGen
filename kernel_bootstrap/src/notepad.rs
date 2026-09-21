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
/// Shift+arrow, Home and End, as the parser delivers them (GFX-054).
pub const KEY_SHIFT_UP: u8 = 0x88;
pub const KEY_SHIFT_DOWN: u8 = 0x89;
pub const KEY_SHIFT_LEFT: u8 = 0x8A;
pub const KEY_SHIFT_RIGHT: u8 = 0x8B;
pub const KEY_HOME: u8 = 0x8C;
pub const KEY_END: u8 = 0x8D;
pub const KEY_SHIFT_HOME: u8 = 0x8E;
pub const KEY_SHIFT_END: u8 = 0x8F;
pub const CTRL_A: u8 = 0x01;
pub const CTRL_C: u8 = 0x03;
pub const CTRL_F: u8 = 0x06;
pub const CTRL_V: u8 = 0x16;
pub const CTRL_X: u8 = 0x18;
pub const CTRL_N: u8 = 0x0E;
pub const CTRL_O: u8 = 0x0F;
pub const CTRL_S: u8 = 0x13;
/// Not a key the parser can produce -- the palette's "Save as..." row uses
/// it to ask for the prompt even when a name is already known.
pub const CTRL_SHIFT_S: u8 = 0x87;
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
    /// A name prompt opened: list the filesystem and call `set_file_names`,
    /// so the prompt can complete names as they are typed.
    ListFiles,
    /// Put this on the clipboard, which the desk owns so every card shares
    /// it (GFX-054).
    Copy(String),
}

/// A one-line prompt in the footer, for a file name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Prompt {
    SaveAs(String),
    Open(String),
    /// Find: the query, and how many places match it.
    Find(String),
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
    /// The other end of the selection; the caret is the moving end. `None`
    /// when nothing is selected (GFX-054).
    anchor: Option<Position>,
    /// Names on the filesystem, for the prompt's completions.
    file_names: Vec<String>,
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
            file_names: Vec::new(),
            anchor: None,
        }
    }

    /// The selection as `(start, end)` in document order, if any.
    pub fn selection(&self) -> Option<(Position, Position)> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            return None;
        }
        let before = (anchor.row, anchor.col) < (self.cursor.row, self.cursor.col);
        Some(if before {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        })
    }

    /// The selected text, with `\n` between lines.
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        let mut text = String::new();
        for row in start.row..=end.row {
            let line = self.line(row);
            let from = if row == start.row {
                start.col.min(line.len())
            } else {
                0
            };
            let to = if row == end.row {
                end.col.min(line.len())
            } else {
                line.len()
            };
            text.push_str(&line[from..to]);
            if row != end.row {
                text.push('\n');
            }
        }
        Some(text)
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// The selection within the viewport, as `(view line, first column,
    /// end column)` in characters, one span per line. A selected line
    /// break shows as one cell past the line's end, so an empty selected
    /// line is still visibly selected.
    pub fn viewport_selection(&self, rows: usize) -> Vec<(usize, usize, usize)> {
        let Some((start, end)) = self.selection() else {
            return Vec::new();
        };
        let chars = |row: usize, col: usize| {
            let line = self.line(row);
            line[..col.min(line.len())].chars().count()
        };
        let mut spans = Vec::new();
        for row in start.row..=end.row {
            if row < self.scroll || row >= self.scroll + rows {
                continue;
            }
            let from = if row == start.row {
                chars(row, start.col)
            } else {
                0
            };
            let to = if row == end.row {
                chars(row, end.col)
            } else {
                chars(row, self.line(row).len()) + 1
            };
            spans.push((row - self.scroll, from, to));
        }
        spans
    }

    /// The pointer moved with the button down: the selection runs from
    /// where it pressed to here.
    pub fn extend_selection_to(&mut self, view_line: usize, column: usize) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.place_cursor(view_line, column);
        self.anchor = Some(anchor);
    }

    /// Paste `text` at the caret, over the selection if there is one. One
    /// undo step.
    pub fn paste(&mut self, text: &str) -> NotepadEffect {
        self.status.clear();
        self.scrolled_away = false;
        if self.prompt.is_some() {
            // Into the prompt's name, one line of it.
            let line: String = text.chars().take_while(|c| *c != '\n').collect();
            for byte in line.bytes() {
                self.handle_prompt_byte(byte);
            }
            return NotepadEffect::Redraw;
        }
        if text.is_empty() {
            return NotepadEffect::None;
        }
        let before = self.snapshot();
        self.delete_selection();
        self.insert_text(text);
        self.push_undo(before);
        NotepadEffect::Redraw
    }

    /// Remove the selected text; the caret lands where it began. Returns
    /// whether there was one. Does not push undo: callers group it with
    /// whatever follows.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let mut lines: Vec<String> = self.buffer.lines().to_vec();
        let trailing = self.buffer.as_string().ends_with('\n');
        let head = lines[start.row][..start.col.min(lines[start.row].len())].to_string();
        let tail = lines[end.row][end.col.min(lines[end.row].len())..].to_string();
        lines[start.row] = head + &tail;
        lines.drain(start.row + 1..=end.row);
        let mut content = lines.join("\n");
        if trailing {
            content.push('\n');
        }
        self.buffer = TextBuffer::from_string(content);
        self.cursor = start;
        self.anchor = None;
        true
    }

    fn insert_text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' {
                if self.buffer.insert_newline(self.cursor) {
                    self.cursor.row += 1;
                    self.cursor.col = 0;
                }
            } else if ch != '\r' && self.buffer.insert_char(self.cursor, ch) {
                self.cursor.col += ch.len_utf8();
            }
        }
    }

    /// Where the selection would start if a key moved the caret with Shift
    /// held: the anchor stays, or is planted, before the move.
    fn hold_anchor(&mut self) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
    }

    fn move_left(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col = floor_boundary(self.line(self.cursor.row), self.cursor.col - 1);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.col = self.buffer.line_length(self.cursor.row);
        }
    }

    fn move_right(&mut self) {
        let len = self.buffer.line_length(self.cursor.row);
        if self.cursor.col < len {
            self.cursor.col = ceil_boundary(self.line(self.cursor.row), self.cursor.col + 1);
        } else if self.cursor.row + 1 < self.buffer.line_count() {
            self.cursor.row += 1;
            self.cursor.col = 0;
        }
    }

    /// Find the next `query` after the selection's start (or the caret),
    /// wrapping round; select it. Matches are within one line.
    fn find_next(&mut self, query: &str) -> bool {
        if query.is_empty() {
            return false;
        }
        let from = self.selection().map(|(s, _)| s).unwrap_or(self.cursor);
        let count = self.buffer.line_count();
        for step in 0..=count {
            let row = (from.row + step) % count;
            let line = self.line(row);
            let start_at = if step == 0 {
                ceil_boundary(line, (from.col + 1).min(line.len()))
            } else {
                0
            };
            let hit = if step == count {
                // Back on the starting line, before where we began.
                line[..from.col.min(line.len())].find(query)
            } else {
                line[start_at..].find(query).map(|i| i + start_at)
            };
            if let Some(col) = hit {
                self.anchor = Some(Position::new(row, col));
                self.cursor = Position::new(row, col + query.len());
                self.scrolled_away = false;
                return true;
            }
        }
        false
    }

    fn match_count(&self, query: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        self.buffer
            .lines()
            .iter()
            .map(|l| l.matches(query).count())
            .sum()
    }

    /// The names a prompt completes against.
    pub fn set_file_names(&mut self, names: Vec<String>) {
        self.file_names = names;
    }

    /// Names that start with what has been typed into the prompt, in order.
    fn completions(&self, typed: &str) -> Vec<&str> {
        self.file_names
            .iter()
            .filter(|name| name.starts_with(typed))
            .map(|name| name.as_str())
            .collect()
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
            Some(Prompt::Find(query)) => {
                let count = self.match_count(query);
                let found = match (query.is_empty(), count) {
                    (true, _) => String::new(),
                    (false, 0) => "   no matches".to_string(),
                    (false, n) => alloc::format!("   {n} found"),
                };
                return alloc::format!("Find: {query}_{found}   (Enter next, Esc closes)");
            }
            Some(Prompt::SaveAs(name)) | Some(Prompt::Open(name)) => {
                let verb = if matches!(self.prompt, Some(Prompt::SaveAs(_))) {
                    "Save as"
                } else {
                    "Open"
                };
                let completions = self.completions(name);
                let mut text = alloc::format!("{verb}: {name}_");
                if !completions.is_empty() {
                    text.push_str("   -> ");
                    let shown: Vec<&str> = completions.iter().copied().take(3).collect();
                    text.push_str(&shown.join(", "));
                    if completions.len() > 3 {
                        text.push_str(", ...");
                    }
                    text.push_str("   (Tab completes)");
                } else {
                    text.push_str("   (Enter confirms, Esc cancels)");
                }
                return text;
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
        if let Some(selected) = self.selected_text() {
            text.push_str(&alloc::format!("   {} selected", selected.chars().count()));
        }
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
        self.anchor = None;
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
        // Selection first (GFX-054): Shift+movement grows it, plain
        // movement drops it, and an edit replaces it.
        match byte {
            KEY_SHIFT_LEFT | KEY_SHIFT_RIGHT | KEY_SHIFT_UP | KEY_SHIFT_DOWN | KEY_SHIFT_HOME
            | KEY_SHIFT_END => {
                self.hold_anchor();
                match byte {
                    KEY_SHIFT_LEFT => self.move_left(),
                    KEY_SHIFT_RIGHT => self.move_right(),
                    KEY_SHIFT_UP => {
                        self.move_vertical(-1);
                    }
                    KEY_SHIFT_DOWN => {
                        self.move_vertical(1);
                    }
                    KEY_SHIFT_HOME => self.cursor.col = 0,
                    _ => self.cursor.col = self.buffer.line_length(self.cursor.row),
                }
                return NotepadEffect::Redraw;
            }
            KEY_HOME => {
                self.anchor = None;
                self.cursor.col = 0;
                return NotepadEffect::Redraw;
            }
            KEY_END => {
                self.anchor = None;
                self.cursor.col = self.buffer.line_length(self.cursor.row);
                return NotepadEffect::Redraw;
            }
            CTRL_A => {
                let last = self.buffer.line_count().saturating_sub(1);
                self.anchor = Some(Position::new(0, 0));
                self.cursor = Position::new(last, self.buffer.line_length(last));
                return NotepadEffect::Redraw;
            }
            CTRL_C => {
                return match self.selected_text() {
                    Some(text) => NotepadEffect::Copy(text),
                    None => {
                        self.status = "Nothing selected".to_string();
                        NotepadEffect::Redraw
                    }
                };
            }
            CTRL_X => {
                let Some(text) = self.selected_text() else {
                    self.status = "Nothing selected".to_string();
                    return NotepadEffect::Redraw;
                };
                let before = self.snapshot();
                self.delete_selection();
                self.push_undo(before);
                return NotepadEffect::Copy(text);
            }
            CTRL_F => {
                // A one-line selection is the query to start from.
                let seed = self
                    .selected_text()
                    .filter(|t| !t.contains('\n'))
                    .unwrap_or_default();
                self.prompt = Some(Prompt::Find(seed));
                return NotepadEffect::Redraw;
            }
            KEY_UP | KEY_DOWN | KEY_LEFT | KEY_RIGHT => self.anchor = None,
            BACKSPACE | KEY_DELETE if self.selection().is_some() => {
                let before = self.snapshot();
                self.delete_selection();
                self.push_undo(before);
                return NotepadEffect::Redraw;
            }
            b'\n' | b'\r' | b'\t' | 0x20..=0x7E if self.selection().is_some() => {
                let before = self.snapshot();
                self.delete_selection();
                let text = match byte {
                    b'\n' | b'\r' => "\n".to_string(),
                    b'\t' => "    ".to_string(),
                    _ => (byte as char).to_string(),
                };
                self.insert_text(&text);
                self.push_undo(before);
                return NotepadEffect::Redraw;
            }
            _ => {}
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
                    NotepadEffect::ListFiles
                }
            }
            CTRL_SHIFT_S => {
                self.prompt = Some(Prompt::SaveAs(String::new()));
                NotepadEffect::ListFiles
            }
            CTRL_O => {
                self.prompt = Some(Prompt::Open(String::new()));
                NotepadEffect::ListFiles
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
                    self.anchor = None;
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
                self.move_left();
                NotepadEffect::Redraw
            }
            KEY_RIGHT => {
                self.move_right();
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
                self.anchor = None;
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
        if let Some(Prompt::Find(query)) = &self.prompt {
            let mut query = query.clone();
            let effect = match byte {
                ESC => {
                    self.prompt = None;
                    return NotepadEffect::Redraw;
                }
                b'\n' | b'\r' => {
                    if !self.find_next(&query) && !query.is_empty() {
                        self.status = "No matches".to_string();
                    }
                    NotepadEffect::Redraw
                }
                BACKSPACE => {
                    query.pop();
                    NotepadEffect::Redraw
                }
                0x20..=0x7E if query.len() < 64 => {
                    query.push(byte as char);
                    // Incremental: the first match from the selection's
                    // start, which is where the last one landed. If the
                    // longer query matches nowhere, the caret stays put.
                    let saved = (self.cursor, self.anchor);
                    if let Some((start, _)) = self.selection() {
                        self.cursor = start;
                        self.anchor = None;
                        // Search from one before, so the same place can
                        // match the longer query.
                        if self.cursor.col > 0 {
                            self.cursor.col -= 1;
                        } else if self.cursor.row > 0 {
                            self.cursor.row -= 1;
                            self.cursor.col = self.buffer.line_length(self.cursor.row);
                        } else {
                            // Line 0, column 0: the search starts after the
                            // caret, so step to the very end and wrap.
                            let last = self.buffer.line_count() - 1;
                            self.cursor = Position::new(last, self.buffer.line_length(last));
                        }
                    }
                    if !self.find_next(&query) {
                        (self.cursor, self.anchor) = saved;
                    }
                    NotepadEffect::Redraw
                }
                _ => NotepadEffect::None,
            };
            self.prompt = Some(Prompt::Find(query));
            return effect;
        }
        let Some(prompt) = self.prompt.as_mut() else {
            return NotepadEffect::None;
        };
        let name = match prompt {
            Prompt::SaveAs(name) | Prompt::Open(name) => name,
            Prompt::Find(_) => return NotepadEffect::None,
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
            b'\t' => {
                // Complete to the first matching name.
                let typed = name.clone();
                let first = self
                    .file_names
                    .iter()
                    .find(|candidate| candidate.starts_with(&typed))
                    .cloned();
                if let (Some(first), Some(prompt)) = (first, self.prompt.as_mut()) {
                    match prompt {
                        Prompt::SaveAs(n) | Prompt::Open(n) => *n = first,
                        Prompt::Find(_) => {}
                    }
                }
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
                    Some(Prompt::Find(_)) | None => NotepadEffect::None,
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
        // The prompt asks for the listing so it can complete names.
        assert_eq!(pad.handle_byte(CTRL_S), NotepadEffect::ListFiles);
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
    fn shift_arrows_select_and_typing_replaces_the_selection() {
        let mut pad = Notepad::new();
        type_str(&mut pad, "hello world");
        assert!(pad.selection().is_none());
        for _ in 0..5 {
            pad.handle_byte(KEY_SHIFT_LEFT);
        }
        assert_eq!(pad.selected_text().as_deref(), Some("world"));
        assert_eq!(pad.viewport_selection(10), alloc::vec![(0, 6, 11)]);
        assert!(pad.footer().contains("5 selected"), "{}", pad.footer());

        // A plain arrow drops it without moving the text.
        pad.handle_byte(KEY_LEFT);
        assert!(pad.selection().is_none());
        assert_eq!(pad.content(), "hello world");

        // Home, Shift+End: the whole line; typing replaces it in one step.
        pad.handle_byte(KEY_HOME);
        pad.handle_byte(KEY_SHIFT_END);
        assert_eq!(pad.selected_text().as_deref(), Some("hello world"));
        pad.handle_byte(b'x');
        assert_eq!(pad.content(), "x");
        pad.handle_byte(CTRL_Z);
        assert_eq!(pad.content(), "hello world");
        assert!(
            pad.selection().is_none(),
            "undo does not resurrect a selection"
        );
    }

    #[test]
    fn a_selection_across_lines_deletes_and_copies_as_one() {
        let mut pad = Notepad::new();
        pad.load(None, "one\ntwo\nthree\n");
        // From the "e" of "one" to the "r" of "three".
        pad.handle_byte(KEY_RIGHT);
        pad.handle_byte(KEY_RIGHT);
        pad.handle_byte(KEY_SHIFT_DOWN);
        pad.handle_byte(KEY_SHIFT_DOWN);
        assert_eq!(pad.selected_text().as_deref(), Some("e\ntwo\nth"));
        assert_eq!(
            pad.viewport_selection(10),
            alloc::vec![(0, 2, 4), (1, 0, 4), (2, 0, 2)]
        );
        assert_eq!(
            pad.handle_byte(CTRL_C),
            NotepadEffect::Copy("e\ntwo\nth".to_string())
        );
        assert_eq!(pad.content(), "one\ntwo\nthree\n", "copy does not edit");
        assert_eq!(
            pad.handle_byte(CTRL_X),
            NotepadEffect::Copy("e\ntwo\nth".to_string())
        );
        assert_eq!(pad.content(), "onree\n", "cut keeps the trailing newline");
        assert_eq!(pad.cursor(), Position::new(0, 2));
        assert_eq!(pad.handle_byte(KEY_DELETE), NotepadEffect::Redraw);
        assert_eq!(pad.content(), "onee\n");
        assert_eq!(pad.handle_byte(CTRL_Z), NotepadEffect::Redraw);
        assert_eq!(pad.handle_byte(CTRL_Z), NotepadEffect::Redraw);
        assert_eq!(pad.content(), "one\ntwo\nthree\n");
    }

    #[test]
    fn paste_lands_at_the_caret_or_over_the_selection_as_one_undo_step() {
        let mut pad = Notepad::new();
        type_str(&mut pad, "ab");
        pad.handle_byte(KEY_LEFT);
        assert_eq!(pad.paste("1\n2"), NotepadEffect::Redraw);
        assert_eq!(pad.content(), "a1\n2b");
        assert_eq!(pad.cursor(), Position::new(1, 1));
        pad.handle_byte(CTRL_A);
        assert_eq!(pad.selected_text().as_deref(), Some("a1\n2b"));
        pad.paste("z");
        assert_eq!(pad.content(), "z");
        pad.handle_byte(CTRL_Z);
        assert_eq!(pad.content(), "a1\n2b");
        assert_eq!(pad.paste(""), NotepadEffect::None);
        assert_eq!(
            pad.handle_byte(CTRL_C),
            NotepadEffect::Redraw,
            "nothing selected"
        );
        assert!(pad.footer().contains("Nothing selected"));
    }

    #[test]
    fn find_selects_the_next_match_incrementally_and_wraps() {
        let mut pad = Notepad::new();
        pad.load(None, "cat\ndog cat\ncatalogue");
        assert_eq!(pad.handle_byte(CTRL_F), NotepadEffect::Redraw);
        assert!(pad.footer().starts_with("Find: _"), "{}", pad.footer());
        type_str(&mut pad, "ca");
        // Incremental: from the caret at (0,0), the first "ca" after it is
        // on line 1.
        assert_eq!(
            pad.selection(),
            Some((Position::new(1, 4), Position::new(1, 6)))
        );
        assert!(pad.footer().contains("3 found"), "{}", pad.footer());
        type_str(&mut pad, "t");
        assert_eq!(
            pad.selection(),
            Some((Position::new(1, 4), Position::new(1, 7)))
        );
        pad.handle_byte(b'\n');
        assert_eq!(
            pad.selection(),
            Some((Position::new(2, 0), Position::new(2, 3)))
        );
        pad.handle_byte(b'\n');
        assert_eq!(
            pad.selection(),
            Some((Position::new(0, 0), Position::new(0, 3))),
            "wraps to the top"
        );
        type_str(&mut pad, "z");
        assert!(pad.footer().contains("no matches"), "{}", pad.footer());
        pad.handle_byte(ESC);
        assert!(pad.footer().starts_with("Ln "));
        assert_eq!(pad.content(), "cat\ndog cat\ncatalogue", "find never edits");

        // A one-line selection seeds the query.
        pad.handle_byte(KEY_HOME);
        pad.handle_byte(KEY_SHIFT_END);
        pad.handle_byte(CTRL_F);
        assert!(pad.footer().starts_with("Find: cat_"), "{}", pad.footer());
    }

    #[test]
    fn a_pointer_drag_extends_the_selection_from_the_press() {
        let mut pad = Notepad::new();
        pad.load(None, "hello world");
        pad.place_cursor(0, 6);
        pad.extend_selection_to(0, 11);
        assert_eq!(pad.selected_text().as_deref(), Some("world"));
        pad.extend_selection_to(0, 2);
        assert_eq!(pad.selected_text().as_deref(), Some("llo "));
        pad.place_cursor(0, 0);
        assert!(pad.selection().is_none(), "a click drops the selection");
    }

    #[test]
    fn a_prompt_asks_for_the_listing_then_completes_on_tab() {
        let mut pad = Notepad::new();
        type_str(&mut pad, "x");
        assert_eq!(pad.handle_byte(CTRL_O), NotepadEffect::ListFiles);
        pad.set_file_names(alloc::vec![
            "notes.txt".to_string(),
            "notes2.txt".to_string(),
            "readme".to_string(),
        ]);
        type_str(&mut pad, "no");
        assert!(
            pad.footer().contains("notes.txt, notes2.txt"),
            "{}",
            pad.footer()
        );
        pad.handle_byte(b'\t');
        assert!(
            pad.footer().starts_with("Open: notes.txt_"),
            "{}",
            pad.footer()
        );
        assert_eq!(
            pad.handle_byte(b'\n'),
            NotepadEffect::Open {
                path: "notes.txt".to_string()
            }
        );
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
