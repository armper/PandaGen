//! Apps (GFX-089): every app on the desk as a grid of icons.
//!
//! The dock shows icons without names; the palette shows names without
//! pictures. This card shows both, large, and filters as you type: a
//! person who does not know what "Ti" or the clock-face tile is finds it
//! here by looking or by typing the first letters. Enter or a click opens
//! the app the way its dock tile would, and the card goes away.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use view_types::{DrawOp, PixelRect};

use crate::desk::DeskApp;
use crate::widgets::{grid, rect, Palette, Ui};

/// A cell's key byte: `CELL_KEY_FIRST + n` is the n-th cell shown.
pub const CELL_KEY_FIRST: u8 = 0xC0;
/// Five across; the grid starts under the search line.
pub const COLUMNS: u32 = 5;
pub const GRID_TOP: i32 = 32;
pub const CELL_H: u32 = 104;

/// What a key or a click asks of the desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherEffect {
    None,
    Redraw,
    /// Open `DeskApp::ALL[index]`, as its dock tile would.
    Launch(usize),
    Close,
}

#[derive(Debug, Clone, Default)]
pub struct LauncherView {
    pub query: String,
    pub selection: usize,
}

impl LauncherView {
    /// The apps that match what was typed, as indices into `DeskApp::ALL`:
    /// a name that starts with it first, then a name that contains it.
    pub fn matches(&self) -> Vec<usize> {
        let query = self.query.to_ascii_lowercase();
        let name = |i: usize| DeskApp::ALL[i].name().to_ascii_lowercase();
        let all = 0..DeskApp::ALL.len();
        let mut found: Vec<usize> = all
            .clone()
            .filter(|i| name(*i).starts_with(&query))
            .collect();
        found.extend(all.filter(|i| !name(*i).starts_with(&query) && name(*i).contains(&query)));
        found
    }

    pub fn handle_byte(&mut self, byte: u8) -> LauncherEffect {
        use crate::notepad::{BACKSPACE, CTRL_W, ESC, KEY_DOWN, KEY_LEFT, KEY_RIGHT, KEY_UP};
        let shown = self.matches();
        let last = shown.len().saturating_sub(1);
        match byte {
            b'\n' | b'\r' => {
                return match shown.get(self.selection) {
                    Some(index) => LauncherEffect::Launch(*index),
                    None => LauncherEffect::None,
                }
            }
            key if key >= CELL_KEY_FIRST => {
                return match shown.get((key - CELL_KEY_FIRST) as usize) {
                    Some(index) => LauncherEffect::Launch(*index),
                    None => LauncherEffect::None,
                }
            }
            ESC if !self.query.is_empty() => {
                self.query.clear();
                self.selection = 0;
            }
            ESC | CTRL_W => return LauncherEffect::Close,
            BACKSPACE => {
                self.query.pop();
                self.selection = 0;
            }
            KEY_LEFT => self.selection = self.selection.saturating_sub(1),
            KEY_RIGHT => self.selection = (self.selection + 1).min(last),
            KEY_UP => self.selection = self.selection.saturating_sub(COLUMNS as usize),
            KEY_DOWN => self.selection = (self.selection + COLUMNS as usize).min(last),
            0x21..=0x7E if self.query.len() < 24 => {
                self.query.push(byte as char);
                self.selection = 0;
            }
            _ => return LauncherEffect::None,
        }
        LauncherEffect::Redraw
    }

    /// The cells for a canvas `width` wide: two rows of five.
    pub fn cells(width: u32) -> Vec<PixelRect> {
        grid(rect(0, GRID_TOP, width, CELL_H * 2 + 8), COLUMNS, 2, 8)
    }

    /// The card, drawn: the search line, then a cell per app with its
    /// icon twice the dock's size and its name under it.
    pub fn ui(&self, width: u32, _height: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        let mut ui = Ui::new(palette, hover);
        let p = *ui.palette();
        let search = if self.query.is_empty() {
            String::from("Type to find an app")
        } else {
            alloc::format!("{}_", self.query)
        };
        ui.search_field(rect(0, 0, width, 26), &search, self.query.is_empty());
        let shown = self.matches();
        if shown.is_empty() {
            ui.text(4, GRID_TOP + 12, "No app has that name", p.muted, 1);
        }
        for (n, (cell, index)) in Self::cells(width).iter().zip(shown.iter()).enumerate() {
            let app = DeskApp::ALL[*index];
            ui.fill(*cell, p.raised, 10);
            if n == self.selection {
                ui.outline(*cell, p.accent, 10, 2);
            } else if ui.hovered(cell) {
                ui.outline(*cell, p.text, 10, 1);
            }
            if let Some(id) = app.picture(64) {
                // The colour icon (GFX-094).
                ui.push(DrawOp::Picture {
                    x: cell.x + cell.width.saturating_sub(64) / 2,
                    y: cell.y + 8,
                    id,
                });
            } else if let Some(bits) = app.icon() {
                // Twice the dock's: sixteen bits, four pixels each.
                let x = cell.x + cell.width.saturating_sub(64) / 2;
                ui.push(DrawOp::Icon {
                    x,
                    y: cell.y + 8,
                    scale: 4,
                    bits,
                    color: None,
                });
            }
            ui.text_centered(
                &rect(cell.x as i32, (cell.y + 76) as i32, cell.width, 20),
                app.name(),
                p.text,
                1,
            );
            ui.hit_area(*cell, CELL_KEY_FIRST + n as u8);
        }
        ui
    }

    pub fn footer(&self) -> String {
        String::from("Enter or a click opens   arrows move   Esc closes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use services_gui_host::Theme;

    #[test]
    fn typing_filters_by_name_and_enter_or_a_click_launches() {
        let mut view = LauncherView::default();
        assert_eq!(view.matches().len(), DeskApp::ALL.len());
        for b in b"ca" {
            view.handle_byte(*b);
        }
        let names: Vec<&str> = view
            .matches()
            .iter()
            .map(|i| DeskApp::ALL[*i].name())
            .collect();
        assert_eq!(names, alloc::vec!["Calculator", "Calendar"]);
        view.handle_byte(crate::notepad::KEY_RIGHT);
        let calendar = DeskApp::ALL
            .iter()
            .position(|a| *a == DeskApp::Calendar)
            .unwrap();
        assert_eq!(view.handle_byte(b'\n'), LauncherEffect::Launch(calendar));
        // "et" is inside Sketch; nothing starts with it.
        view.handle_byte(crate::notepad::ESC);
        assert!(view.query.is_empty(), "the first Esc clears");
        for b in b"et" {
            view.handle_byte(*b);
        }
        let names: Vec<&str> = view
            .matches()
            .iter()
            .map(|i| DeskApp::ALL[*i].name())
            .collect();
        assert_eq!(names, alloc::vec!["Sketch"]);
        view.handle_byte(crate::notepad::ESC);
        // A click, by pixel, on the third cell.
        let palette = Palette::from_theme(&Theme::DEFAULT);
        let cell = LauncherView::cells(504)[2];
        let key = view
            .ui(504, 260, palette, None)
            .hit(cell.x as i32 + 10, cell.y as i32 + 10)
            .unwrap();
        assert_eq!(view.handle_byte(key), LauncherEffect::Launch(2));
        // Drawn: an icon and a name a cell, the first cell ringed.
        let ops = view.ui(504, 260, palette, None).into_ops();
        let icons = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::Picture { .. }))
            .count();
        assert_eq!(icons, DeskApp::ALL.len());
        assert!(ops
            .iter()
            .any(|op| matches!(op, DrawOp::Text { text, .. } if text == "Calculator")));
        assert_eq!(view.handle_byte(crate::notepad::ESC), LauncherEffect::Close);
        for b in b"zzz" {
            view.handle_byte(*b);
        }
        assert_eq!(view.handle_byte(b'\n'), LauncherEffect::None);
    }
}
