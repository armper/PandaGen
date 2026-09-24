//! Calendar (GFX-076): a month at a glance, and a note for any day.
//!
//! The month is a grid of text cells, Monday first; today is marked, the
//! day under the caret is the card's selection, and a day that has a
//! note carries a dot. A day's note is an ordinary document named by the
//! day -- `2026-09-24` -- so it is in Files, keeps its versions, and opens
//! in a Notepad: the calendar does not have a text editor of its own,
//! the desk already has one. The desk tells the card which days have a
//! document through the same listing Files gets.
//!
//! Pure state: the desk feeds it today's date from the RTC and the file
//! names; it answers with lines, a selection span and what a key means.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use view_types::PixelRect;

use crate::rtc;
use crate::widgets::{grid, rect, ButtonKind, Palette, Ui};

/// A day of the month as a key byte (GFX-083): the desk's own range,
/// above anything the keyboard parser produces. `DAY_KEY_FIRST` is the
/// 1st.
pub const DAY_KEY_FIRST: u8 = 0xC1;
pub const DAY_KEY_LAST: u8 = DAY_KEY_FIRST + 30;
/// Where the card's parts go, in canvas pixels.
pub const HEADER_H: u32 = 32;
pub const WEEKDAYS_TOP: i32 = 40;
pub const DAYS_TOP: i32 = 60;
pub const DAYS_H: u32 = 236;
pub const TEXT_TOP: i32 = 304;

/// The header's month buttons and the six-by-seven day cells for a canvas
/// (GFX-083): what the drawing and the desk's hit test agree on.
#[derive(Debug, Clone)]
pub struct CalendarLayout {
    pub earlier: PixelRect,
    pub later: PixelRect,
    pub cells: Vec<PixelRect>,
}

impl CalendarLayout {
    pub fn new(width: u32) -> Self {
        Self {
            earlier: rect(0, 0, 36, HEADER_H),
            later: rect(width as i32 - 36, 0, 36, HEADER_H),
            cells: grid(rect(0, DAYS_TOP, width, DAYS_H), 7, 6, 4),
        }
    }
}

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub const fn new(year: u16, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    /// The document name for this day's note.
    pub fn name(&self) -> String {
        alloc::format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// A name that is a day, `YYYY-MM-DD`, or nothing.
    pub fn parse(name: &str) -> Option<Date> {
        let bytes = name.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let year: u16 = name[0..4].parse().ok()?;
        let month: u8 = name[5..7].parse().ok()?;
        let day: u8 = name[8..10].parse().ok()?;
        if !(1..=12).contains(&month) || day == 0 || day > rtc::days_in_month(year, month) {
            return None;
        }
        Some(Date::new(year, month, day))
    }

    /// Monday is 0.
    pub fn weekday(&self) -> usize {
        rtc::weekday(self.year, self.month, self.day)
    }

    fn plus_days(&self, delta: i64) -> Date {
        let days = rtc::days_from_civil(self.year as i64, self.month as i64, self.day as i64);
        let (y, m, d) = rtc::civil_from_days(days + delta);
        Date::new(y.clamp(1, 9999) as u16, m as u8, d as u8)
    }

    fn plus_months(&self, delta: i32) -> Date {
        let index = self.year as i32 * 12 + (self.month as i32 - 1) + delta;
        let year = (index.div_euclid(12)).clamp(1, 9999) as u16;
        let month = (index.rem_euclid(12) + 1) as u8;
        let day = self.day.min(rtc::days_in_month(year, month));
        Date::new(year, month, day)
    }

    /// `Thursday 24 September 2026`.
    pub fn long(&self) -> String {
        let weekday = match self.weekday() {
            0 => "Monday",
            1 => "Tuesday",
            2 => "Wednesday",
            3 => "Thursday",
            4 => "Friday",
            5 => "Saturday",
            _ => "Sunday",
        };
        alloc::format!(
            "{weekday} {} {} {}",
            self.day,
            MONTHS[self.month as usize - 1],
            self.year
        )
    }

    /// `Thu 24 Sep`.
    pub fn short(&self) -> String {
        alloc::format!(
            "{} {} {}",
            WEEKDAYS[self.weekday()],
            self.day,
            &MONTHS[self.month as usize - 1][..3]
        )
    }
}

/// What a key or click asks the desk to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarEffect {
    None,
    Redraw,
    /// Open the note for this day in a Notepad; `exists` says whether a
    /// document of that name is already there.
    OpenNote {
        name: String,
        exists: bool,
    },
    Close,
}

/// The card's state: the month shown, the day selected, what is known.
#[derive(Debug, Clone)]
pub struct CalendarView {
    pub selected: Date,
    /// The RTC's date, when it is set.
    pub today: Option<Date>,
    /// Days that have a note document.
    noted: Vec<Date>,
    /// The listing has arrived.
    pub loaded: bool,
}

impl CalendarView {
    /// Open on `today`, or on a fixed day when the clock is not set, so
    /// the grid is still a grid.
    pub fn new(today: Option<Date>) -> Self {
        Self {
            selected: today.unwrap_or(Date::new(2026, 1, 1)),
            today,
            noted: Vec::new(),
            loaded: false,
        }
    }

    /// The desk's clock ticked over to another day.
    pub fn set_today(&mut self, today: Option<Date>) {
        self.today = today;
    }

    /// The filesystem's names: the ones that are days are notes.
    pub fn set_file_names(&mut self, names: &[String]) {
        self.noted = names.iter().filter_map(|n| Date::parse(n)).collect();
        self.noted.sort();
        self.loaded = true;
    }

    pub fn has_note(&self, date: Date) -> bool {
        self.noted.binary_search(&date).is_ok()
    }

    pub fn note_count_in_month(&self) -> usize {
        self.noted
            .iter()
            .filter(|d| d.year == self.selected.year && d.month == self.selected.month)
            .count()
    }

    fn open_selected(&self) -> CalendarEffect {
        CalendarEffect::OpenNote {
            name: self.selected.name(),
            exists: self.has_note(self.selected),
        }
    }

    /// One key: arrows move by a day or a week, PageUp/Down by a month,
    /// `t` goes to today, Enter opens the day's note.
    pub fn handle_byte(&mut self, byte: u8) -> CalendarEffect {
        use crate::notepad::{
            CTRL_W, ESC, KEY_DOWN, KEY_LEFT, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_RIGHT, KEY_UP,
        };
        match byte {
            KEY_LEFT => self.selected = self.selected.plus_days(-1),
            KEY_RIGHT => self.selected = self.selected.plus_days(1),
            KEY_UP => self.selected = self.selected.plus_days(-7),
            KEY_DOWN => self.selected = self.selected.plus_days(7),
            KEY_PAGE_UP => self.selected = self.selected.plus_months(-1),
            KEY_PAGE_DOWN => self.selected = self.selected.plus_months(1),
            b't' | b'T' => {
                if let Some(today) = self.today {
                    self.selected = today;
                }
            }
            b'\n' | b'\r' => return self.open_selected(),
            // A day's own key (GFX-083): the desk sends it for a click on
            // the day's cell. The selected day opens; another selects.
            key @ DAY_KEY_FIRST..=DAY_KEY_LAST => {
                let day = key - DAY_KEY_FIRST + 1;
                let (_, days) = self.shape();
                if day > days {
                    return CalendarEffect::None;
                }
                let date = Date::new(self.selected.year, self.selected.month, day);
                if date == self.selected {
                    return self.open_selected();
                }
                self.selected = date;
            }
            CTRL_W | ESC => return CalendarEffect::Close,
            _ => return CalendarEffect::None,
        }
        CalendarEffect::Redraw
    }

    /// The first week row's leading blanks and the month's length.
    fn shape(&self) -> (usize, u8) {
        let first = Date::new(self.selected.year, self.selected.month, 1);
        (first.weekday(), rtc::days_in_month(first.year, first.month))
    }

    /// The cell that shows `date` in the month on screen, if it is in it.
    pub fn cell_of(&self, layout: &CalendarLayout, date: Date) -> Option<PixelRect> {
        if date.year != self.selected.year || date.month != self.selected.month {
            return None;
        }
        let (lead, _) = self.shape();
        layout.cells.get(lead + date.day as usize - 1).copied()
    }

    /// The card, drawn (GFX-083): the month with its turn buttons, the
    /// weekday names, a cell a day -- today outlined, the selected day
    /// filled, a dot on a day with a note -- then today and the selected
    /// day in words. `hover` is the pointer in canvas pixels.
    pub fn ui(&self, width: u32, _height: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        use crate::notepad::{KEY_PAGE_DOWN, KEY_PAGE_UP};
        let layout = CalendarLayout::new(width);
        let mut ui = Ui::new(palette, hover);
        let p = *ui.palette();
        ui.button(layout.earlier, "<", KEY_PAGE_UP, ButtonKind::Quiet);
        ui.button(layout.later, ">", KEY_PAGE_DOWN, ButtonKind::Quiet);
        let title = alloc::format!(
            "{} {}",
            MONTHS[self.selected.month as usize - 1],
            self.selected.year
        );
        ui.text_centered(&rect(36, 0, width - 72, HEADER_H), &title, p.text, 2);
        for (cell, name) in grid(rect(0, WEEKDAYS_TOP, width, 16), 7, 1, 4)
            .iter()
            .zip(WEEKDAYS.iter())
        {
            ui.text_centered(cell, &name[..2], p.muted, 1);
        }
        let (lead, days) = self.shape();
        for day in 1..=days {
            let Some(cell) = layout.cells.get(lead + day as usize - 1).copied() else {
                break;
            };
            let date = Date::new(self.selected.year, self.selected.month, day);
            let selected = date == self.selected;
            let (fill, ink) = if selected {
                (p.accent, p.on_accent)
            } else {
                (p.raised, p.text)
            };
            ui.fill(cell, fill, 6);
            if self.today == Some(date) && !selected {
                ui.outline(cell, p.accent, 6, 2);
            }
            if ui.hovered(&cell) && !selected {
                ui.outline(cell, p.text, 6, 1);
            }
            ui.text_centered(&cell, &day.to_string(), ink, 1);
            if self.has_note(date) {
                let dot = rect(
                    (cell.x + cell.width) as i32 - 11,
                    (cell.y + cell.height) as i32 - 11,
                    6,
                    6,
                );
                ui.fill(dot, if selected { p.on_accent } else { p.accent }, 3);
            }
            ui.hit_area(cell, DAY_KEY_FIRST + day - 1);
        }
        let today_line = match self.today {
            Some(today) => alloc::format!("Today  {}", today.long()),
            None => "The clock is not set".to_string(),
        };
        ui.text(0, TEXT_TOP, &today_line, p.muted, 1);
        let state = if self.has_note(self.selected) {
            alloc::format!("{}   a note is kept", self.selected.short())
        } else {
            alloc::format!("{}   no note yet", self.selected.short())
        };
        ui.text(0, TEXT_TOP + 18, &state, p.text, 1);
        ui
    }

    pub fn footer(&self) -> String {
        let notes = self.note_count_in_month();
        let kept = match notes {
            0 => String::new(),
            1 => "1 note this month   ".to_string(),
            n => alloc::format!("{n} notes this month   "),
        };
        alloc::format!("{kept}Enter opens the day's note")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use services_gui_host::Theme;
    use view_types::DrawOp;

    fn palette() -> Palette {
        Palette::from_theme(&Theme::DEFAULT)
    }

    fn texts(cal: &CalendarView) -> Vec<String> {
        cal.ui(404, 340, palette(), None)
            .into_ops()
            .into_iter()
            .filter_map(|op| match op {
                DrawOp::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn september_2026_starts_on_a_tuesday_and_the_grid_says_so() {
        let today = Date::new(2026, 9, 24);
        assert_eq!(today.weekday(), 3, "a Thursday");
        assert_eq!(today.long(), "Thursday 24 September 2026");
        assert_eq!(today.short(), "Thu 24 Sep");
        assert_eq!(today.name(), "2026-09-24");
        assert_eq!(Date::parse("2026-09-24"), Some(today));
        assert_eq!(Date::parse("2026-02-30"), None);
        assert_eq!(Date::parse("memo.txt"), None);
        let mut cal = CalendarView::new(Some(today));
        cal.set_file_names(&["2026-09-03".to_string(), "notes".to_string()]);
        let layout = CalendarLayout::new(404);
        assert_eq!(layout.cells.len(), 42);
        // The 1st sits in the second column (Tuesday); the 24th in row 3.
        assert_eq!(
            cal.cell_of(&layout, Date::new(2026, 9, 1)),
            Some(layout.cells[1])
        );
        assert_eq!(cal.cell_of(&layout, today), Some(layout.cells[24]));
        assert_eq!(cal.cell_of(&layout, Date::new(2026, 10, 1)), None);
        let shown = texts(&cal);
        assert!(shown.contains(&"September 2026".to_string()));
        assert!(shown.contains(&"Today  Thursday 24 September 2026".to_string()));
        assert!(shown.contains(&"Thu 24 Sep   no note yet".to_string()));
        // The selected day is the one accent fill among the cells; the
        // noted 3rd carries a dot; today (selected) has no outline.
        let ops = cal.ui(404, 340, palette(), None).into_ops();
        let accent_cells = ops
            .iter()
            .filter(|op| {
                matches!(op, DrawOp::RoundedFill { rect, color, radius: 6 }
                    if *color == palette().accent && rect.height == layout.cells[0].height)
            })
            .count();
        assert_eq!(accent_cells, 1);
        let dots = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::RoundedFill { radius: 3, .. }))
            .count();
        assert_eq!(dots, 1);
        assert_eq!(
            cal.footer(),
            "1 note this month   Enter opens the day's note"
        );
    }

    #[test]
    fn keys_and_clicks_move_the_day_and_enter_opens_its_note() {
        use crate::notepad::{KEY_DOWN, KEY_LEFT, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_UP};
        let today = Date::new(2026, 9, 24);
        let mut cal = CalendarView::new(Some(today));
        cal.set_file_names(&["2026-10-01".to_string()]);
        cal.handle_byte(KEY_DOWN);
        assert_eq!(
            cal.selected,
            Date::new(2026, 10, 1),
            "a week on crosses the month"
        );
        assert!(texts(&cal).contains(&"Thu 1 Oct   a note is kept".to_string()));
        cal.handle_byte(KEY_UP);
        cal.handle_byte(KEY_LEFT);
        assert_eq!(cal.selected, Date::new(2026, 9, 23));
        cal.handle_byte(KEY_PAGE_UP);
        assert_eq!(cal.selected, Date::new(2026, 8, 23));
        cal.handle_byte(KEY_PAGE_DOWN);
        cal.handle_byte(KEY_PAGE_DOWN);
        assert_eq!(cal.selected, Date::new(2026, 10, 23));
        // The 31st clamps to a shorter month.
        cal.selected = Date::new(2026, 10, 31);
        cal.handle_byte(KEY_PAGE_DOWN);
        assert_eq!(cal.selected, Date::new(2026, 11, 30));
        // Across the year.
        cal.handle_byte(KEY_PAGE_DOWN);
        cal.handle_byte(KEY_PAGE_DOWN);
        assert_eq!(cal.selected, Date::new(2027, 1, 30));
        assert_eq!(cal.handle_byte(b't'), CalendarEffect::Redraw);
        assert_eq!(cal.selected, today);
        // Clicks, by pixel: the header's buttons turn the month; a day
        // cell selects; the selected cell opens.
        let layout = CalendarLayout::new(404);
        let hit = |cal: &CalendarView, cell: PixelRect| {
            cal.ui(404, 340, palette(), None)
                .hit(cell.x as i32 + 3, cell.y as i32 + 3)
        };
        assert_eq!(hit(&cal, layout.later), Some(KEY_PAGE_DOWN));
        cal.handle_byte(KEY_PAGE_DOWN);
        assert_eq!(cal.selected.month, 10);
        assert_eq!(hit(&cal, layout.earlier), Some(KEY_PAGE_UP));
        cal.handle_byte(KEY_PAGE_UP);
        assert_eq!(cal.selected.month, 9);
        assert_eq!(hit(&cal, layout.cells[0]), None, "before the 1st");
        let ninth = cal.cell_of(&layout, Date::new(2026, 9, 9)).unwrap();
        let key = hit(&cal, ninth).expect("a day there");
        assert_eq!(key, DAY_KEY_FIRST + 8);
        assert_eq!(cal.handle_byte(key), CalendarEffect::Redraw);
        assert_eq!(cal.selected, Date::new(2026, 9, 9));
        assert_eq!(
            cal.handle_byte(key),
            CalendarEffect::OpenNote {
                name: "2026-09-09".to_string(),
                exists: false
            }
        );
        assert_eq!(
            cal.handle_byte(DAY_KEY_FIRST + 30),
            CalendarEffect::None,
            "no 31st"
        );
        cal.selected = Date::new(2026, 10, 1);
        assert_eq!(
            cal.handle_byte(b'\n'),
            CalendarEffect::OpenNote {
                name: "2026-10-01".to_string(),
                exists: true
            }
        );
        // No clock: a grid all the same, and it says so.
        let mut blind = CalendarView::new(None);
        assert!(texts(&blind).contains(&"The clock is not set".to_string()));
        assert_eq!(blind.handle_byte(b't'), CalendarEffect::Redraw);
    }
}
