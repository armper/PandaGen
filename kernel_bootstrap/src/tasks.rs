//! Tasks (GFX-079): a to-do list that is a document.
//!
//! The list is kept as the document named `tasks`, one line per task,
//! `[x] done` or `[ ] not yet` -- readable in a Notepad, kept in versions
//! like everything else, and saved by the desk a second after it changes.
//! The card is the pleasant way to use it: a row is a task, Enter or a
//! click ticks it, `a` asks for a new one in the footer, Delete removes,
//! and every action is a `[ button ]` too.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::timer::button_at;

/// The document the list lives in.
pub const TASKS_FILE: &str = "tasks";
/// How long the list sits changed before it saves, in ticks.
pub const SAVE_AFTER_TICKS: u64 = 100;
/// Most tasks a card shows before the rest scroll away.
pub const MAX_TASKS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub done: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TasksEffect {
    None,
    Redraw,
    Close,
}

#[derive(Debug, Clone)]
pub struct TasksView {
    tasks: Vec<Task>,
    pub selection: usize,
    /// A new task being typed in the footer.
    prompt: Option<String>,
    /// The document has arrived.
    pub loaded: bool,
    /// Changed since it was last saved, and when.
    changed_at: Option<u64>,
    now: u64,
}

impl Default for TasksView {
    fn default() -> Self {
        Self::new()
    }
}

impl TasksView {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            selection: 0,
            prompt: None,
            loaded: false,
            changed_at: None,
            now: 0,
        }
    }

    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// The document's text: one task per line.
    pub fn parse(text: &str) -> Vec<Task> {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let (done, rest) = if let Some(rest) = line.strip_prefix("[x] ") {
                    (true, rest)
                } else if let Some(rest) = line.strip_prefix("[ ] ") {
                    (false, rest)
                } else {
                    (false, line)
                };
                Task {
                    done,
                    text: rest.trim_end().to_string(),
                }
            })
            .take(MAX_TASKS)
            .collect()
    }

    pub fn content(&self) -> String {
        let mut text = String::new();
        for task in &self.tasks {
            text.push_str(if task.done { "[x] " } else { "[ ] " });
            text.push_str(&task.text);
            text.push('\n');
        }
        text
    }

    /// The desk read the document.
    pub fn load(&mut self, text: &str) {
        self.tasks = Self::parse(text);
        self.selection = self.selection.min(self.tasks.len().saturating_sub(1));
        self.loaded = true;
        self.changed_at = None;
    }

    /// The desk's clock; a changed list that has sat still asks to save.
    pub fn save_due(&mut self, now: u64) -> Option<String> {
        self.now = now;
        let since = self.changed_at?;
        if now.saturating_sub(since) < SAVE_AFTER_TICKS {
            return None;
        }
        self.changed_at = None;
        Some(self.content())
    }

    pub fn is_dirty(&self) -> bool {
        self.changed_at.is_some()
    }

    fn changed(&mut self) {
        self.changed_at = Some(self.now);
    }

    fn toggle(&mut self) {
        if let Some(task) = self.tasks.get_mut(self.selection) {
            task.done = !task.done;
            self.changed();
        }
    }

    fn remove(&mut self) {
        if self.selection < self.tasks.len() {
            self.tasks.remove(self.selection);
            self.selection = self.selection.min(self.tasks.len().saturating_sub(1));
            self.changed();
        }
    }

    fn clear_done(&mut self) {
        let before = self.tasks.len();
        self.tasks.retain(|t| !t.done);
        if self.tasks.len() != before {
            self.selection = self.selection.min(self.tasks.len().saturating_sub(1));
            self.changed();
        }
    }

    fn add(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.tasks.len() >= MAX_TASKS {
            return;
        }
        self.tasks.push(Task {
            done: false,
            text: text.to_string(),
        });
        self.selection = self.tasks.len() - 1;
        self.changed();
    }

    fn shift(&mut self, delta: isize) {
        let to = self.selection as isize + delta;
        if to < 0 || to as usize >= self.tasks.len() || self.tasks.is_empty() {
            return;
        }
        self.tasks.swap(self.selection, to as usize);
        self.selection = to as usize;
        self.changed();
    }

    pub fn prompt_open(&self) -> bool {
        self.prompt.is_some()
    }

    /// A key. While the footer asks for a new task, printable keys go
    /// there, Enter adds it and Esc drops it.
    pub fn handle_byte(&mut self, byte: u8) -> TasksEffect {
        use crate::notepad::{BACKSPACE, CTRL_W, ESC, KEY_DELETE, KEY_DOWN, KEY_UP};
        if let Some(prompt) = self.prompt.as_mut() {
            match byte {
                ESC => self.prompt = None,
                b'\n' | b'\r' => {
                    let text = self.prompt.take().unwrap_or_default();
                    self.add(&text);
                }
                BACKSPACE => {
                    prompt.pop();
                }
                0x20..=0x7E if prompt.len() < 80 => prompt.push(byte as char),
                _ => return TasksEffect::None,
            }
            return TasksEffect::Redraw;
        }
        match byte {
            KEY_UP => self.selection = self.selection.saturating_sub(1),
            KEY_DOWN => {
                self.selection = (self.selection + 1).min(self.tasks.len().saturating_sub(1))
            }
            b'\n' | b'\r' | b' ' => self.toggle(),
            b'a' | b'A' => self.prompt = Some(String::new()),
            KEY_DELETE | BACKSPACE => self.remove(),
            b'u' | b'U' => self.shift(-1),
            b'd' | b'D' => self.shift(1),
            b'c' | b'C' => self.clear_done(),
            CTRL_W | ESC => return TasksEffect::Close,
            _ => return TasksEffect::None,
        }
        TasksEffect::Redraw
    }

    /// The content line the buttons are on: after the tasks and a gap.
    pub fn buttons_line(&self) -> usize {
        self.tasks.len().max(1) + 1
    }

    /// A click: a row selects, the selected row ticks, a button acts.
    pub fn click(&mut self, line: usize, column: usize) -> TasksEffect {
        if line < self.tasks.len() {
            if line == self.selection {
                self.toggle();
            } else {
                self.selection = line;
            }
            return TasksEffect::Redraw;
        }
        if line == self.buttons_line() {
            let text = self.lines()[line].clone();
            return match button_at(&text, column) {
                Some(0) => self.handle_byte(b'a'),
                Some(1) => self.handle_byte(b'\n'),
                Some(2) => self.handle_byte(crate::notepad::KEY_DELETE),
                Some(3) => self.handle_byte(b'c'),
                _ => TasksEffect::None,
            };
        }
        TasksEffect::None
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .tasks
            .iter()
            .map(|t| alloc::format!("{} {}", if t.done { "[x]" } else { "[ ]" }, t.text))
            .collect();
        if lines.is_empty() {
            lines.push(if self.loaded {
                "Nothing to do. Add something.".to_string()
            } else {
                "Reading the list...".to_string()
            });
        }
        lines.push(String::new());
        lines.push("[ Add ] [ Done ] [ Remove ] [ Clear done ]".to_string());
        lines
    }

    /// The highlighted row, if there is a task under it.
    pub fn highlight(&self) -> Option<usize> {
        (!self.tasks.is_empty()).then_some(self.selection)
    }

    pub fn footer(&self) -> String {
        if let Some(prompt) = &self.prompt {
            return alloc::format!("New task: {prompt}_   (Enter adds, Esc cancels)");
        }
        let open = self.tasks.iter().filter(|t| !t.done).count();
        let state = if self.is_dirty() { "   saving..." } else { "" };
        match (self.tasks.len(), open) {
            (0, _) => "A adds a task".to_string(),
            (n, 0) => alloc::format!("All {n} done   C clears them{state}"),
            (n, open) => alloc::format!("{open} of {n} to do   Enter ticks   A adds{state}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notepad::{KEY_DELETE, KEY_DOWN, KEY_UP};

    #[test]
    fn the_list_is_a_document_and_round_trips() {
        let mut tasks = TasksView::new();
        assert_eq!(tasks.lines()[0], "Reading the list...");
        tasks.load("[x] Buy milk\n[ ] Write the summary\nno mark\n\n");
        assert_eq!(tasks.tasks().len(), 3);
        assert!(tasks.tasks()[0].done && !tasks.tasks()[2].done);
        assert_eq!(tasks.tasks()[2].text, "no mark");
        assert_eq!(
            tasks.content(),
            "[x] Buy milk\n[ ] Write the summary\n[ ] no mark\n"
        );
        assert_eq!(tasks.lines()[1], "[ ] Write the summary");
        assert_eq!(tasks.buttons_line(), 4);
        assert_eq!(tasks.footer(), "2 of 3 to do   Enter ticks   A adds");
        assert!(!tasks.is_dirty());
    }

    #[test]
    fn keys_and_clicks_tick_add_move_and_remove_then_it_saves_itself() {
        let mut tasks = TasksView::new();
        tasks.load("[ ] one\n[ ] two\n");
        tasks.save_due(10);
        // Enter ticks the selected one; the list is dirty, then saves.
        assert_eq!(tasks.handle_byte(b'\n'), TasksEffect::Redraw);
        assert!(tasks.tasks()[0].done && tasks.is_dirty());
        assert!(tasks.footer().ends_with("saving..."));
        assert_eq!(tasks.save_due(50), None);
        assert_eq!(
            tasks.save_due(10 + SAVE_AFTER_TICKS),
            Some("[x] one\n[ ] two\n".to_string())
        );
        assert!(!tasks.is_dirty());
        // A adds through the footer prompt.
        tasks.handle_byte(b'a');
        assert!(tasks.footer().starts_with("New task: _"));
        for b in b"three" {
            tasks.handle_byte(*b);
        }
        tasks.handle_byte(0x08);
        tasks.handle_byte(b'e');
        tasks.handle_byte(b'\n');
        assert_eq!(tasks.tasks()[2].text, "three");
        assert_eq!(tasks.selection, 2);
        // U moves it up; Delete removes; Up/Down select.
        tasks.handle_byte(b'u');
        assert_eq!(tasks.tasks()[1].text, "three");
        tasks.handle_byte(KEY_UP);
        tasks.handle_byte(KEY_DELETE);
        assert_eq!(tasks.tasks()[0].text, "three");
        tasks.handle_byte(KEY_DOWN);
        assert_eq!(tasks.selection, 1);
        // Clicks: another row selects, the same row ticks, buttons act.
        assert_eq!(tasks.click(0, 3), TasksEffect::Redraw);
        assert_eq!(tasks.selection, 0);
        tasks.click(0, 3);
        assert!(tasks.tasks()[0].done);
        let buttons = tasks.buttons_line();
        assert_eq!(
            tasks.lines()[buttons],
            "[ Add ] [ Done ] [ Remove ] [ Clear done ]"
        );
        assert_eq!(tasks.click(buttons, 31), TasksEffect::Redraw, "Clear done");
        assert_eq!(tasks.tasks().len(), 1);
        assert_eq!(tasks.tasks()[0].text, "two");
        let buttons = tasks.buttons_line();
        tasks.click(buttons, 2);
        assert!(tasks.prompt_open());
        assert_eq!(tasks.handle_byte(0x1B), TasksEffect::Redraw);
        assert!(!tasks.prompt_open());
        assert_eq!(tasks.handle_byte(0x1B), TasksEffect::Close);
        // An empty list says so once it has been read.
        tasks.load("");
        assert_eq!(tasks.lines()[0], "Nothing to do. Add something.");
        assert_eq!(tasks.footer(), "A adds a task");
    }
}
