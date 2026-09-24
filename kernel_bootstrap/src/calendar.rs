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

use crate::rtc;

/// The grid's width in characters: seven cells of four.
pub const WIDTH: usize = 28;
/// The content line of the first week row: the month title and the
/// weekday header come first.
pub const FIRST_WEEK_LINE: usize = 2;
const CELL: usize = 4;

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

    /// The day drawn at content `line`, `column`, if any.
    pub fn day_at(&self, line: usize, column: usize) -> Option<Date> {
        let row = line.checked_sub(FIRST_WEEK_LINE)?;
        let (lead, days) = self.shape();
        let index = row * 7 + column / CELL;
        let day = (index + 1).checked_sub(lead)?;
        if day == 0 || day > days as usize {
            return None;
        }
        Some(Date::new(
            self.selected.year,
            self.selected.month,
            day as u8,
        ))
    }

    /// A click: the title's ends turn the month, a day selects it, and
    /// the selected day opens its note.
    pub fn click(&mut self, line: usize, column: usize) -> CalendarEffect {
        if line == 0 {
            if column < 2 {
                self.selected = self.selected.plus_months(-1);
                return CalendarEffect::Redraw;
            }
            if column + 2 >= WIDTH {
                self.selected = self.selected.plus_months(1);
                return CalendarEffect::Redraw;
            }
            return CalendarEffect::None;
        }
        match self.day_at(line, column) {
            Some(day) if day == self.selected => self.open_selected(),
            Some(day) => {
                self.selected = day;
                CalendarEffect::Redraw
            }
            None => CalendarEffect::None,
        }
    }

    /// The selected day's cell, for the card's selection fill: `(line,
    /// first column, end column)`.
    pub fn selection(&self) -> Vec<(usize, usize, usize)> {
        let (lead, _) = self.shape();
        let index = lead + self.selected.day as usize - 1;
        let line = FIRST_WEEK_LINE + index / 7;
        let column = (index % 7) * CELL;
        alloc::vec![(line, column, column + CELL - 1)]
    }

    pub fn lines(&self) -> Vec<String> {
        let title = alloc::format!(
            "{} {}",
            MONTHS[self.selected.month as usize - 1],
            self.selected.year
        );
        let mut lines = alloc::vec![
            alloc::format!("<{:^26}>", title),
            WEEKDAYS
                .iter()
                .map(|d| alloc::format!(" {} ", &d[..2]))
                .collect::<Vec<_>>()
                .join(""),
        ];
        let (lead, days) = self.shape();
        let mut row = String::new();
        for _ in 0..lead {
            row.push_str("    ");
        }
        for day in 1..=days {
            let date = Date::new(self.selected.year, self.selected.month, day);
            let mark = if self.today == Some(date) { '*' } else { ' ' };
            let dot = if self.has_note(date) { '.' } else { ' ' };
            row.push_str(&alloc::format!("{mark}{day:>2}{dot}"));
            if row.len() >= WIDTH {
                lines.push(core::mem::take(&mut row));
            }
        }
        if !row.is_empty() {
            lines.push(row);
        }
        while lines.len() < FIRST_WEEK_LINE + 6 {
            lines.push(String::new());
        }
        lines.push(String::new());
        lines.push(match self.today {
            Some(today) => alloc::format!("Today  {}", today.long()),
            None => "The clock is not set".to_string(),
        });
        lines.push(if self.has_note(self.selected) {
            alloc::format!("{}   a note is kept", self.selected.short())
        } else {
            alloc::format!("{}   no note yet", self.selected.short())
        });
        lines
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
        let lines = cal.lines();
        assert_eq!(lines[0], "<      September 2026      >");
        assert_eq!(lines[1], " Mo  Tu  We  Th  Fr  Sa  Su ");
        assert_eq!(lines[2], "      1   2   3.  4   5   6 ");
        assert!(lines[5].contains("*24 "), "{:?}", lines[5]);
        assert_eq!(lines[6], " 28  29  30 ");
        assert_eq!(lines[9], "Today  Thursday 24 September 2026");
        assert_eq!(lines[10], "Thu 24 Sep   no note yet");
        // The selection sits on the 24th: row 3 of the grid, Thursday.
        assert_eq!(cal.selection(), alloc::vec![(5, 12, 15)]);
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
        assert!(cal.lines()[10].ends_with("a note is kept"));
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
        // Clicks: the title's ends turn the month; a day selects; again opens.
        assert_eq!(cal.click(0, 27), CalendarEffect::Redraw);
        assert_eq!(cal.selected.month, 10);
        assert_eq!(cal.click(0, 0), CalendarEffect::Redraw);
        assert_eq!(cal.selected.month, 9);
        assert_eq!(cal.day_at(2, 0), None, "before the 1st");
        assert_eq!(cal.day_at(2, 5), Some(Date::new(2026, 9, 1)));
        assert_eq!(cal.day_at(6, 12), None, "after the 30th");
        assert_eq!(cal.click(3, 9), CalendarEffect::Redraw);
        assert_eq!(cal.selected, Date::new(2026, 9, 9));
        assert_eq!(
            cal.click(3, 9),
            CalendarEffect::OpenNote {
                name: "2026-09-09".to_string(),
                exists: false
            }
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
        assert_eq!(blind.lines()[9], "The clock is not set");
        assert_eq!(blind.handle_byte(b't'), CalendarEffect::Redraw);
    }
}
