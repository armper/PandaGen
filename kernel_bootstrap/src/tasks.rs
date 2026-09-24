//! Tasks (GFX-079): a to-do list that is a document.
//!
//! The list is kept as the document named `tasks`, one line per task,
//! `[x] done` or `[ ] not yet` -- readable in a Notepad, kept in versions
//! like everything else, and saved by the desk a second after it changes.
//! The card is the pleasant way to use it: a row is a task with a real
//! checkbox, Enter or a click ticks it, `a` asks for a new one in the
//! footer, Delete removes, and every action is a button too (GFX-083).

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use view_types::PixelRect;

use crate::widgets::{grid, rect, ButtonKind, Palette, Ui};

/// A row as a key byte (GFX-083): `ROW_KEY_FIRST` is the first task
/// shown; the desk sends it for a click on the row.
pub const ROW_KEY_FIRST: u8 = 0xC0;
pub const ROW_KEY_LAST: u8 = 0xFF;
/// A row's height in canvas pixels, and the buttons' height.
pub const ROW_H: u32 = 28;
pub const BUTTONS_H: u32 = 44;

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
    /// The first row shown, so the selection is always on screen.
    scroll: usize,
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
            scroll: 0,
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
            // A row's own key (GFX-083): the desk sends it for a click on
            // the row. The selected row ticks; another selects.
            key @ ROW_KEY_FIRST..=ROW_KEY_LAST => {
                let index = self.scroll + (key - ROW_KEY_FIRST) as usize;
                if index >= self.tasks.len() {
                    return TasksEffect::None;
                }
                if index == self.selection {
                    self.toggle();
                } else {
                    self.selection = index;
                }
            }
            CTRL_W | ESC => return TasksEffect::Close,
            _ => return TasksEffect::None,
        }
        TasksEffect::Redraw
    }

    /// How many rows fit above the buttons on a canvas `height` tall.
    pub fn rows_that_fit(height: u32) -> usize {
        (height.saturating_sub(BUTTONS_H + 12) / ROW_H).max(1) as usize
    }

    /// The buttons' rectangles for a canvas: Add, Done, Remove, Clear done.
    pub fn buttons(width: u32, height: u32) -> Vec<PixelRect> {
        grid(
            rect(0, height as i32 - BUTTONS_H as i32, width, BUTTONS_H),
            4,
            1,
            6,
        )
    }

    /// The card, drawn (GFX-083): a row a task with a checkbox, the
    /// selected row raised, the buttons under. Rows that do not fit
    /// scroll so the selection is always shown. `hover` is the pointer in
    /// canvas pixels.
    pub fn ui(
        &mut self,
        width: u32,
        height: u32,
        palette: Palette,
        hover: Option<(i32, i32)>,
    ) -> Ui {
        let mut ui = Ui::new(palette, hover);
        let p = *ui.palette();
        let rows = Self::rows_that_fit(height);
        if self.selection < self.scroll {
            self.scroll = self.selection;
        } else if self.selection >= self.scroll + rows {
            self.scroll = self.selection + 1 - rows;
        }
        if self.tasks.is_empty() {
            let note = if self.loaded {
                "Nothing to do. Add something."
            } else {
                "Reading the list..."
            };
            ui.text(6, 6, note, p.muted, 1);
        }
        for (i, task) in self.tasks.iter().skip(self.scroll).take(rows).enumerate() {
            let index = self.scroll + i;
            let row = rect(0, (i as u32 * ROW_H) as i32, width, ROW_H);
            if index == self.selection {
                ui.fill(row, p.raised, 6);
            }
            let check = rect(8, row.y as i32 + 5, 18, 18);
            if task.done {
                ui.fill(check, p.accent, 4);
                ui.text_centered(&check, "x", p.on_accent, 1);
            } else {
                ui.outline(check, p.muted, 4, 2);
            }
            let ink = if task.done { p.muted } else { p.text };
            ui.text(36, row.y as i32 + 6, &task.text, ink, 1);
            if ui.hovered(&row) && index != self.selection {
                ui.outline(row, p.raised, 6, 1);
            }
            if i < (ROW_KEY_LAST - ROW_KEY_FIRST) as usize {
                ui.hit_area(row, ROW_KEY_FIRST + i as u8);
            }
        }
        let buttons = Self::buttons(width, height);
        ui.button(buttons[0], "Add", b'a', ButtonKind::Primary);
        ui.button(buttons[1], "Done", b'\n', ButtonKind::Plain);
        ui.button(
            buttons[2],
            "Remove",
            crate::notepad::KEY_DELETE,
            ButtonKind::Quiet,
        );
        ui.button(buttons[3], "Clear done", b'c', ButtonKind::Quiet);
        ui
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
    use services_gui_host::Theme;
    use view_types::DrawOp;

    fn palette() -> Palette {
        Palette::from_theme(&Theme::DEFAULT)
    }

    fn texts(tasks: &mut TasksView) -> Vec<String> {
        tasks
            .ui(444, 340, palette(), None)
            .into_ops()
            .into_iter()
            .filter_map(|op| match op {
                DrawOp::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_list_is_a_document_and_round_trips() {
        let mut tasks = TasksView::new();
        assert!(texts(&mut tasks).contains(&"Reading the list...".to_string()));
        tasks.load("[x] Buy milk\n[ ] Write the summary\nno mark\n\n");
        assert_eq!(tasks.tasks().len(), 3);
        assert!(tasks.tasks()[0].done && !tasks.tasks()[2].done);
        assert_eq!(tasks.tasks()[2].text, "no mark");
        assert_eq!(
            tasks.content(),
            "[x] Buy milk\n[ ] Write the summary\n[ ] no mark\n"
        );
        let shown = texts(&mut tasks);
        assert!(shown.contains(&"Write the summary".to_string()));
        assert!(shown.contains(&"Clear done".to_string()));
        // One tick mark: the done one's checkbox.
        assert_eq!(shown.iter().filter(|t| *t == "x").count(), 1);
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
        // Clicks, by pixel: another row selects, the same row ticks,
        // buttons act.
        let row0 = rect(0, 0, 444, ROW_H);
        let key = tasks
            .ui(444, 340, palette(), None)
            .hit(row0.x as i32 + 100, row0.y as i32 + 10)
            .expect("a row there");
        assert_eq!(key, ROW_KEY_FIRST);
        assert_eq!(tasks.handle_byte(key), TasksEffect::Redraw);
        assert_eq!(tasks.selection, 0);
        tasks.handle_byte(key);
        assert!(tasks.tasks()[0].done);
        let buttons = TasksView::buttons(444, 340);
        let clear = buttons[3];
        let key = tasks
            .ui(444, 340, palette(), None)
            .hit(clear.x as i32 + 4, clear.y as i32 + 4)
            .unwrap();
        assert_eq!(key, b'c');
        assert_eq!(tasks.handle_byte(key), TasksEffect::Redraw, "Clear done");
        assert_eq!(tasks.tasks().len(), 1);
        assert_eq!(tasks.tasks()[0].text, "two");
        let add = buttons[0];
        let key = tasks
            .ui(444, 340, palette(), None)
            .hit(add.x as i32 + 4, add.y as i32 + 4)
            .unwrap();
        assert_eq!(key, b'a');
        tasks.handle_byte(key);
        assert!(tasks.prompt_open());
        assert_eq!(tasks.handle_byte(0x1B), TasksEffect::Redraw);
        assert!(!tasks.prompt_open());
        assert_eq!(
            tasks.handle_byte(ROW_KEY_FIRST + 5),
            TasksEffect::None,
            "no such row"
        );
        assert_eq!(tasks.handle_byte(0x1B), TasksEffect::Close);
        // An empty list says so once it has been read.
        tasks.load("");
        assert!(texts(&mut tasks).contains(&"Nothing to do. Add something.".to_string()));
        assert_eq!(tasks.footer(), "A adds a task");
        // A long list scrolls so the selection is shown.
        let many: String = (0..30).map(|i| alloc::format!("[ ] t{i}\n")).collect();
        tasks.load(&many);
        tasks.selection = 29;
        let shown = texts(&mut tasks);
        assert!(shown.contains(&"t29".to_string()));
        assert!(!shown.contains(&"t0".to_string()));
        assert_eq!(TasksView::rows_that_fit(340), 10);
    }
}
