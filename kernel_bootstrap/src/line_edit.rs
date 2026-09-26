//! The Terminal's input line (KBD-012): a caret you can move, history you
//! can walk, and Tab to finish a command's name.
//!
//! The workspace's prompt was an append-only buffer: arrows were dropped
//! before they reached it, Up and Down were "not implemented yet", and a
//! typo at the start of a long line meant backspacing over all of it. This
//! is the whole of a line editor, apart from the screen: bytes in, an
//! [`Edit`] out that says what the caller has to redraw. It knows nothing
//! of serial ports or the desk, so it runs under `cargo test`.

extern crate alloc;

use crate::notepad::{KEY_DELETE, KEY_DOWN, KEY_END, KEY_HOME, KEY_LEFT, KEY_RIGHT, KEY_UP};
use alloc::string::String;
use alloc::vec::Vec;

/// The longest line, in bytes.
pub const LINE_MAX: usize = 256;
/// How many lines the history keeps.
pub const HISTORY_MAX: usize = 64;
/// Ctrl+U: clear the line.
pub const CTRL_U: u8 = 0x15;

/// What a key did to the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Not a key for the line.
    Unhandled,
    /// A key for the line that changed nothing (Left at the start).
    Nothing,
    /// One byte added at the end: a terminal can just print it.
    Appended(u8),
    /// Anything else changed: the line has to be drawn again.
    Redraw,
    /// Enter: the line is done. It is still in the editor; `take` it.
    Submit,
    /// Tab found more than one way to finish the word: these.
    Choices(Vec<String>),
}

/// An input line with a caret and a history.
#[derive(Debug, Clone)]
pub struct LineEdit {
    text: Vec<u8>,
    cursor: usize,
    history: Vec<String>,
    /// Where Up has walked to, while walking; the line typed before the
    /// walk began is `draft`.
    browsing: Option<usize>,
    draft: Vec<u8>,
}

impl Default for LineEdit {
    fn default() -> Self {
        Self::new()
    }
}

impl LineEdit {
    pub fn new() -> Self {
        Self {
            text: Vec::new(),
            cursor: 0,
            history: Vec::new(),
            browsing: None,
            draft: Vec::new(),
        }
    }

    pub fn text(&self) -> &[u8] {
        &self.text
    }

    /// The caret, in bytes from the start of the line.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Empty the line (history stays).
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.browsing = None;
    }

    /// Put `text` on the line, caret at the end.
    pub fn set(&mut self, text: &str) {
        self.text.clear();
        self.text.extend(
            text.bytes()
                .filter(|b| (0x20..0x7F).contains(b))
                .take(LINE_MAX),
        );
        self.cursor = self.text.len();
        self.browsing = None;
    }

    /// Take the line out, leaving it empty.
    pub fn take(&mut self) -> String {
        let line = String::from_utf8_lossy(&self.text).into_owned();
        self.clear();
        line
    }

    /// The lines run before, oldest first.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Remember a line that was run: not a blank one, not the one just
    /// before it again, and not one that carries a passphrase -- history
    /// is for walking back through, and a secret should not be there.
    pub fn remember(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty()
            || self.history.last().map(String::as_str) == Some(line)
            || crate::access_shell::secret_from(line).is_some()
        {
            return;
        }
        if self.history.len() == HISTORY_MAX {
            self.history.remove(0);
        }
        self.history.push(String::from(line));
    }

    /// One key. `words` are the commands Tab may finish.
    pub fn key(&mut self, byte: u8, words: &[&str]) -> Edit {
        match byte {
            b'\r' | b'\n' => Edit::Submit,
            0x08 | 0x7F => {
                if self.cursor == 0 {
                    return Edit::Nothing;
                }
                self.cursor -= 1;
                self.text.remove(self.cursor);
                self.edited();
                Edit::Redraw
            }
            KEY_DELETE => {
                if self.cursor == self.text.len() {
                    return Edit::Nothing;
                }
                self.text.remove(self.cursor);
                self.edited();
                Edit::Redraw
            }
            KEY_LEFT => self.move_to(self.cursor.saturating_sub(1)),
            KEY_RIGHT => self.move_to((self.cursor + 1).min(self.text.len())),
            KEY_HOME => self.move_to(0),
            KEY_END => self.move_to(self.text.len()),
            CTRL_U => {
                if self.text.is_empty() {
                    return Edit::Nothing;
                }
                self.text.clear();
                self.cursor = 0;
                self.edited();
                Edit::Redraw
            }
            KEY_UP => self.walk(-1),
            KEY_DOWN => self.walk(1),
            b'\t' => self.complete(words),
            0x20..=0x7E => {
                if self.text.len() >= LINE_MAX {
                    return Edit::Nothing;
                }
                self.text.insert(self.cursor, byte);
                self.cursor += 1;
                self.edited();
                if self.cursor == self.text.len() {
                    Edit::Appended(byte)
                } else {
                    Edit::Redraw
                }
            }
            _ => Edit::Unhandled,
        }
    }

    fn move_to(&mut self, at: usize) -> Edit {
        if at == self.cursor {
            return Edit::Nothing;
        }
        self.cursor = at;
        Edit::Redraw
    }

    /// Typing on a recalled line makes it the line being typed: Down no
    /// longer brings the draft back over it.
    fn edited(&mut self) {
        self.browsing = None;
    }

    /// Up (`-1`) and Down (`1`) through the history.
    fn walk(&mut self, step: i32) -> Edit {
        let len = self.history.len();
        let next = match (self.browsing, step < 0) {
            (None, true) if len > 0 => {
                self.draft = self.text.clone();
                Some(len - 1)
            }
            (None, _) => return Edit::Nothing,
            (Some(0), true) => return Edit::Nothing,
            (Some(at), true) => Some(at - 1),
            (Some(at), false) if at + 1 < len => Some(at + 1),
            // Down past the newest: the draft again.
            (Some(_), false) => None,
        };
        self.text = match next {
            Some(at) => self.history[at].as_bytes().to_vec(),
            None => core::mem::take(&mut self.draft),
        };
        self.cursor = self.text.len();
        self.browsing = next;
        Edit::Redraw
    }

    /// Tab: finish the command's name, if the caret is in it. One match
    /// completes it (and a space); several extend it as far as they agree
    /// and are handed back to be listed.
    fn complete(&mut self, words: &[&str]) -> Edit {
        let first_end = self
            .text
            .iter()
            .position(|b| *b == b' ')
            .unwrap_or(self.text.len());
        if self.cursor != first_end || first_end == 0 {
            return Edit::Nothing;
        }
        let typed = String::from_utf8_lossy(&self.text[..first_end]).into_owned();
        let mut found: Vec<&str> = words
            .iter()
            .copied()
            .filter(|w| w.starts_with(typed.as_str()))
            .collect();
        found.sort_unstable();
        found.dedup();
        match found.as_slice() {
            [] => Edit::Nothing,
            [one] => {
                let mut rest: Vec<u8> = one.as_bytes()[typed.len()..].to_vec();
                if self.text.get(first_end) != Some(&b' ') {
                    rest.push(b' ');
                }
                let rest: Vec<u8> = rest
                    .into_iter()
                    .take(LINE_MAX.saturating_sub(self.text.len()))
                    .collect();
                let n = rest.len();
                self.text.splice(first_end..first_end, rest);
                self.cursor = first_end + n;
                self.edited();
                Edit::Redraw
            }
            many => {
                let common = many.iter().skip(1).fold(many[0].len(), |n, w| {
                    many[0]
                        .bytes()
                        .zip(w.bytes())
                        .take(n)
                        .take_while(|(a, b)| a == b)
                        .count()
                });
                if common > typed.len() {
                    let rest = many[0].as_bytes()[typed.len()..common].to_vec();
                    let n = rest.len();
                    self.text.splice(first_end..first_end, rest);
                    self.cursor = first_end + n;
                    self.edited();
                }
                Edit::Choices(many.iter().map(|w| String::from(*w)).collect())
            }
        }
    }

    /// The line as a screen should show it: a passphrase being typed shows
    /// as stars.
    pub fn shown(&self) -> String {
        let line = String::from_utf8_lossy(&self.text).into_owned();
        mask(&line)
    }
}

/// `line` with any passphrase in it starred out (`login bea ****`).
pub fn mask(line: &str) -> String {
    match crate::access_shell::secret_from(line) {
        None => String::from(line),
        Some(at) => {
            let mut out = String::from(&line[..at]);
            out.extend(line[at..].chars().map(|c| if c == ' ' { ' ' } else { '*' }));
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(line: &mut LineEdit, s: &str) {
        for b in s.bytes() {
            line.key(b, &[]);
        }
    }

    #[test]
    fn test_line_edit_moves_the_caret_and_edits_in_the_middle() {
        let mut line = LineEdit::new();
        typed(&mut line, "ct notes");
        assert_eq!(line.key(KEY_HOME, &[]), Edit::Redraw);
        assert_eq!(line.key(KEY_RIGHT, &[]), Edit::Redraw);
        assert_eq!(line.key(b'a', &[]), Edit::Redraw, "not at the end: redraw");
        assert_eq!(line.text(), b"cat notes");
        assert_eq!(line.cursor(), 2);
        assert_eq!(line.key(KEY_DELETE, &[]), Edit::Redraw);
        assert_eq!(line.text(), b"ca notes");
        assert_eq!(line.key(0x08, &[]), Edit::Redraw);
        assert_eq!(line.text(), b"c notes");
        assert_eq!(line.key(KEY_END, &[]), Edit::Redraw);
        assert_eq!(line.key(KEY_RIGHT, &[]), Edit::Nothing);
        assert_eq!(line.key(b'!', &[]), Edit::Appended(b'!'));
        assert_eq!(line.key(CTRL_U, &[]), Edit::Redraw);
        assert!(line.is_empty());
        assert_eq!(line.key(0x08, &[]), Edit::Nothing);
        assert_eq!(line.key(0x9E, &[]), Edit::Unhandled);
    }

    #[test]
    fn test_line_edit_walks_history_and_keeps_the_draft() {
        let mut line = LineEdit::new();
        assert_eq!(line.key(KEY_UP, &[]), Edit::Nothing, "no history yet");
        for l in ["ls", "cat a", "cat a", "  ", "mem"] {
            line.remember(l);
        }
        assert_eq!(line.history(), ["ls", "cat a", "mem"]);
        typed(&mut line, "wri");
        assert_eq!(line.key(KEY_UP, &[]), Edit::Redraw);
        assert_eq!(line.text(), b"mem");
        line.key(KEY_UP, &[]);
        line.key(KEY_UP, &[]);
        assert_eq!(line.text(), b"ls");
        assert_eq!(line.key(KEY_UP, &[]), Edit::Nothing, "the oldest");
        line.key(KEY_DOWN, &[]);
        assert_eq!(line.text(), b"cat a");
        line.key(KEY_DOWN, &[]);
        line.key(KEY_DOWN, &[]);
        assert_eq!(line.text(), b"wri", "past the newest: the draft back");
        assert_eq!(line.key(KEY_DOWN, &[]), Edit::Nothing);
        // A recalled line, edited, is the line.
        line.key(KEY_UP, &[]);
        line.key(b'!', &[]);
        assert_eq!(line.key(KEY_DOWN, &[]), Edit::Nothing);
        assert_eq!(line.take(), "mem!");
        assert!(line.is_empty());
    }

    #[test]
    fn test_line_edit_forgets_passphrases_and_stars_them() {
        let mut line = LineEdit::new();
        line.remember("login bea hunter2");
        line.remember("passwd old new");
        line.remember("setpass bea s3cret");
        assert!(line.history().is_empty(), "{:?}", line.history());
        typed(&mut line, "login bea hunter2");
        assert_eq!(line.shown(), "login bea *******");
        assert_eq!(mask("passwd old new"), "passwd *** ***");
        assert_eq!(mask("setpass bea x"), "setpass bea *");
        assert_eq!(mask("login bea"), "login bea");
        assert_eq!(mask("ls"), "ls");
    }

    #[test]
    fn test_line_edit_tab_finishes_the_command() {
        let words = ["cat", "clear", "clearance", "cls", "ls"];
        let mut line = LineEdit::new();
        typed(&mut line, "ca");
        assert_eq!(line.key(b'\t', &words), Edit::Redraw);
        assert_eq!(line.text(), b"cat ");
        line.clear();
        typed(&mut line, "cle");
        assert_eq!(
            line.key(b'\t', &words),
            Edit::Choices(alloc::vec!["clear".into(), "clearance".into()])
        );
        assert_eq!(line.text(), b"clear", "extended as far as they agree");
        line.clear();
        typed(&mut line, "zz");
        assert_eq!(line.key(b'\t', &words), Edit::Nothing);
        line.clear();
        typed(&mut line, "cat x");
        assert_eq!(
            line.key(b'\t', &words),
            Edit::Nothing,
            "caret not in the word"
        );
    }
}
