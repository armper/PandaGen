//! The Calendar (GFX-076; a program since PROC-009): a month at a
//! glance, and a note for any day.
//!
//! The month is a grid of cells, Monday first; today is marked, the day
//! under the caret is the selection, and a day that has a note carries a
//! dot. A day's note is an ordinary document named by the day --
//! `2026-09-24` -- so it is in Files, keeps its versions, and opens in a
//! Notepad: the Calendar has no text editor of its own, the desk already
//! has one. The program (`apps/calendar`) asks for exactly the documents
//! named like days (`####-##-##`) -- their names, to put the dots, and
//! the right to have one opened in a Notepad -- and nothing else a person
//! keeps.
//!
//! Pure state: it is given today's date and the day-named documents; it
//! answers with a view and what a key means.

#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use app_protocol::{Area, Kind, Op, Role, ViewWriter};

/// A day of the month as a key byte: the desk's own range, above
/// anything the keyboard parser produces. `DAY_KEY_FIRST` is the 1st.
pub const DAY_KEY_FIRST: u8 = 0xC1;
pub const DAY_KEY_LAST: u8 = DAY_KEY_FIRST + 30;
/// Keys (the desk's).
pub const KEY_UP: u8 = 0x80;
pub const KEY_DOWN: u8 = 0x81;
pub const KEY_LEFT: u8 = 0x82;
pub const KEY_RIGHT: u8 = 0x83;
pub const KEY_PAGE_UP: u8 = 0x90;
pub const KEY_PAGE_DOWN: u8 = 0x91;
pub const CTRL_W: u8 = 0x17;
pub const ESC: u8 = 0x1B;
/// Where the card's parts go, in canvas pixels.
pub const HEADER_H: u16 = 32;
pub const WEEKDAYS_TOP: u16 = 40;
pub const DAYS_TOP: u16 = 60;
pub const DAYS_H: u16 = 236;
pub const TEXT_TOP: u16 = 304;
/// The pattern of the documents it asks for.
pub const NOTE_PATTERN: &str = "####-##-##";

/// `area` cut into `cols` x `rows` cells with `gap` between them.
fn grid(area: Area, cols: u16, rows: u16, gap: u16) -> Vec<Area> {
    let w = area.w.saturating_sub(gap * (cols - 1)) / cols;
    let h = area.h.saturating_sub(gap * (rows - 1)) / rows;
    let mut cells = Vec::with_capacity((cols * rows) as usize);
    for r in 0..rows {
        for c in 0..cols {
            cells.push(Area::new(
                area.x + c * (w + gap),
                area.y + r * (h + gap),
                w,
                h,
            ));
        }
    }
    cells
}

/// The header's month buttons and the six-by-seven day cells.
#[derive(Debug, Clone)]
pub struct CalendarLayout {
    pub earlier: Area,
    pub later: Area,
    pub cells: Vec<Area>,
}

impl CalendarLayout {
    pub fn new(width: u16) -> Self {
        Self {
            earlier: Area::new(0, 0, 36, HEADER_H),
            later: Area::new(width - 36, 0, 36, HEADER_H),
            cells: grid(Area::new(0, DAYS_TOP, width, DAYS_H), 7, 6, 4),
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
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Date::new(year, month, day))
    }

    /// Monday is 0.
    pub fn weekday(&self) -> usize {
        weekday(self.year, self.month, self.day)
    }

    fn plus_days(&self, delta: i64) -> Date {
        let days = days_from_civil(self.year as i64, self.month as i64, self.day as i64);
        let (y, m, d) = civil_from_days(days + delta);
        Date::new(y.clamp(1, 9999) as u16, m as u8, d as u8)
    }

    fn plus_months(&self, delta: i32) -> Date {
        let index = self.year as i32 * 12 + (self.month as i32 - 1) + delta;
        let year = (index.div_euclid(12)).clamp(1, 9999) as u16;
        let month = (index.rem_euclid(12) + 1) as u8;
        let day = self.day.min(days_in_month(year, month));
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

/// What a key or click asks for.
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

    /// The clock ticked over to another day.
    pub fn set_today(&mut self, today: Option<Date>) {
        self.today = today;
    }

    /// The documents' names: the ones that are days are notes.
    pub fn set_file_names<S: AsRef<str>>(&mut self, names: &[S]) {
        self.noted = names
            .iter()
            .filter_map(|n| Date::parse(n.as_ref()))
            .collect();
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
            // A day's own key: a click on the day's cell. The selected day
            // opens; another selects.
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
        (first.weekday(), days_in_month(first.year, first.month))
    }

    /// The cell that shows `date` in the month on screen, if it is in it.
    pub fn cell_of(&self, layout: &CalendarLayout, date: Date) -> Option<Area> {
        if date.year != self.selected.year || date.month != self.selected.month {
            return None;
        }
        let (lead, _) = self.shape();
        layout.cells.get(lead + date.day as usize - 1).copied()
    }

    /// The card: the month with its turn buttons, the weekday names, a
    /// cell a day -- today outlined, the selected day filled, a dot on a
    /// day with a note -- then today and the selected day in words.
    pub fn draw(&self, width: u16, height: u16, view: &mut ViewWriter) {
        if width < 7 * 20 || height < TEXT_TOP + 36 {
            view.op(Op::Text {
                x: 0,
                y: 0,
                role: Role::Muted,
                scale: 1,
                text: "Make me bigger",
            });
            return;
        }
        let layout = CalendarLayout::new(width);
        view.op(Op::Button {
            area: layout.earlier,
            kind: Kind::Quiet,
            key: KEY_PAGE_UP,
            label: "<",
        });
        view.op(Op::Button {
            area: layout.later,
            kind: Kind::Quiet,
            key: KEY_PAGE_DOWN,
            label: ">",
        });
        let title = alloc::format!(
            "{} {}",
            MONTHS[self.selected.month as usize - 1],
            self.selected.year
        );
        view.op(Op::TextCentered {
            area: Area::new(36, 0, width - 72, HEADER_H),
            role: Role::Text,
            scale: 2,
            text: &title,
        });
        for (cell, name) in grid(Area::new(0, WEEKDAYS_TOP, width, 16), 7, 1, 4)
            .iter()
            .zip(WEEKDAYS.iter())
        {
            view.op(Op::TextCentered {
                area: *cell,
                role: Role::Muted,
                scale: 1,
                text: &name[..2],
            });
        }
        let (lead, days) = self.shape();
        for day in 1..=days {
            let Some(cell) = layout.cells.get(lead + day as usize - 1).copied() else {
                break;
            };
            let date = Date::new(self.selected.year, self.selected.month, day);
            let selected = date == self.selected;
            let (fill, ink) = if selected {
                (Role::Accent, Role::OnAccent)
            } else {
                (Role::Raised, Role::Text)
            };
            view.op(Op::Fill {
                area: cell,
                role: fill,
                radius: 6,
            });
            if self.today == Some(date) && !selected {
                view.op(Op::Outline {
                    area: cell,
                    role: Role::Accent,
                    radius: 6,
                    thickness: 2,
                });
            }
            let number = day.to_string();
            view.op(Op::TextCentered {
                area: cell,
                role: ink,
                scale: 1,
                text: &number,
            });
            if self.has_note(date) {
                view.op(Op::Fill {
                    area: Area::new(cell.x + cell.w - 11, cell.y + cell.h - 11, 6, 6),
                    role: if selected {
                        Role::OnAccent
                    } else {
                        Role::Accent
                    },
                    radius: 3,
                });
            }
            view.op(Op::Hit {
                area: cell,
                key: DAY_KEY_FIRST + day - 1,
            });
        }
        let today_line = match self.today {
            Some(today) => alloc::format!("Today  {}", today.long()),
            None => "The clock is not set".to_string(),
        };
        view.op(Op::Text {
            x: 0,
            y: TEXT_TOP,
            role: Role::Muted,
            scale: 1,
            text: &today_line,
        });
        // Today, as a button too: the pointer alone reaches everything.
        view.op(Op::Button {
            area: Area::new(width - 88, TEXT_TOP, 88, 34),
            kind: Kind::Quiet,
            key: b't',
            label: "Today",
        });
        let state = if self.has_note(self.selected) {
            alloc::format!("{}   a note is kept", self.selected.short())
        } else {
            alloc::format!("{}   no note yet", self.selected.short())
        };
        view.op(Op::Text {
            x: 0,
            y: TEXT_TOP + 18,
            role: Role::Text,
            scale: 1,
            text: &state,
        });
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

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The day of the week, Monday 0 to Sunday 6 (GFX-076). 1970-01-01 was
/// a Thursday.
pub fn weekday(year: u16, month: u8, day: u8) -> usize {
    (days_from_civil(year as i64, month as i64, day as i64) + 3).rem_euclid(7) as usize
}

/// How many days `month` of `year` has.
pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap =
                (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
            if leap {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use app_protocol::{OwnedOp, View};

    fn view_of(cal: &CalendarView) -> View {
        let mut buf = [0u8; app_protocol::VIEW_MAX];
        let mut view = ViewWriter::new(&mut buf, "Calendar", &cal.footer());
        cal.draw(404, 340, &mut view);
        View::decode(view.finish().unwrap(), 404, 340).expect("a view the desk takes")
    }

    fn texts(cal: &CalendarView) -> Vec<String> {
        view_of(cal)
            .ops
            .into_iter()
            .filter_map(|op| match op {
                OwnedOp::Text { text, .. } | OwnedOp::TextCentered { text, .. } => Some(text),
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
        let ops = view_of(&cal).ops;
        let accent_cells = ops
            .iter()
            .filter(|op| {
                matches!(op, OwnedOp::Fill { area, role: Role::Accent, radius: 6 }
                    if area.h == layout.cells[0].h)
            })
            .count();
        assert_eq!(accent_cells, 1);
        let dots = ops
            .iter()
            .filter(|op| matches!(op, OwnedOp::Fill { radius: 3, .. }))
            .count();
        assert_eq!(dots, 1);
        assert_eq!(
            cal.footer(),
            "1 note this month   Enter opens the day's note"
        );
    }

    #[test]
    fn names_that_are_not_days_are_not_notes() {
        let mut cal = CalendarView::new(Some(Date::new(2026, 9, 24)));
        cal.set_file_names(&["2026-09-24", "2026-13-01", "1999-02-29", "memo"]);
        assert!(cal.has_note(Date::new(2026, 9, 24)));
        assert_eq!(cal.note_count_in_month(), 1);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
    }

    #[test]
    fn keys_and_clicks_move_the_day_and_enter_opens_its_note() {
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
        // The key a click at a cell's corner is: the last button or hit
        // area drawn there, as the desk finds it.
        let hit = |cal: &CalendarView, cell: Area| {
            let (x, y) = (cell.x + 3, cell.y + 3);
            view_of(cal).ops.iter().rev().find_map(|op| match op {
                OwnedOp::Button { area, key, .. } | OwnedOp::Hit { area, key }
                    if x >= area.x && y >= area.y && x < area.x + area.w && y < area.y + area.h =>
                {
                    Some(*key)
                }
                _ => None,
            })
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
        assert!(!texts(&blind).is_empty());
        assert!(texts(&blind).contains(&"The clock is not set".to_string()));
        assert_eq!(blind.handle_byte(b't'), CalendarEffect::Redraw);
    }
}
