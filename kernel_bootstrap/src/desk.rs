//! The desk (GFX-050/052): a window manager for cards, a dock and a top bar.
//!
//! Apps own windows; the shell stays out of the way. This module holds the
//! pure part -- where the windows are, which one has focus, what is being
//! dragged or resized -- and turns pointer deliveries and key bytes into
//! changes to that state. It builds the `DesktopWindow` list the compositor
//! paints; the kernel loop does the painting, the presenting, the file I/O
//! and the console it asks for.
//!
//! Everything here is host-testable: nothing touches hardware.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use graphics_rasterizer::RasterRect;
use input_types::{PointerButton, PointerEventKind};
use services_gui_host::{
    Delivery, DesktopTab, DesktopWindow, DesktopWindowLayer, DesktopWindowRole, HitRegion,
    NoticeLevel, ShellNotice, SurfaceRect, Theme, Wallpaper, WindowStyle,
};

/// The default wallpaper (GFX-066): the person's own picture, 1280x800 in
/// 256 colours (one megabyte), built into the kernel so the first frame
/// already has it. The compositor samples it to whatever size the screen
/// is.
pub const WALLPAPER: Wallpaper = Wallpaper {
    width: 1280,
    height: 800,
    palette: include_bytes!("../assets/wallpaper.pal"),
    indices: include_bytes!("../assets/wallpaper.idx"),
};

/// What Look calls the two backgrounds.
pub const WALLPAPERS: [&str; 2] = ["Picture", "Gradient"];

/// How many spaces the desk has (GFX-067): four desks on one screen, each
/// with its own cards. Ctrl+1..4 switches, Ctrl+Shift+1..4 moves the
/// focused card, and the strip in the middle of the top bar shows where
/// you are and takes a click.
pub const SPACES: usize = 4;

/// The overview's mini cards (GFX-068): four to a row.
pub const OVERVIEW_COLUMNS: usize = 4;
pub const OVERVIEW_CARD: (usize, usize) = (280, 150);
pub const OVERVIEW_GAP: usize = 16;
use view_types::{CursorPosition, ViewContent, ViewFrame, ViewId, ViewKind};

use crate::calculator::Calculator;
use crate::calendar::{CalendarEffect, CalendarView, Date};
use crate::game::{Game, GameEffect};
use crate::notepad::{Notepad, NotepadEffect};
use crate::sketch::{SketchEffect, SketchView, SKETCH_FILE};
use crate::tasks::{TasksEffect, TasksView, TASKS_FILE};
use crate::timer::{TimerEffect, TimerView};

pub const TOP_BAR_HEIGHT: usize = 28;
pub const DOCK_HEIGHT: usize = 56;
pub const DOCK_MARGIN: usize = 12;
pub const NOTEPAD_SIZE: (usize, usize) = (720, 480);
pub const TERMINAL_SIZE: (usize, usize) = (800, 520);
/// Successive windows open offset by this much.
pub const CASCADE_STEP: usize = 32;
/// The smallest a card can be resized to.
pub const MIN_CARD_SIZE: (usize, usize) = (240, 140);
/// How close to an edge a drag must end to snap there.
pub const SNAP_MARGIN: usize = 6;
/// Ctrl+Tab, as the parser delivers it.
pub const KEY_CTRL_TAB: u8 = 0x85;
/// Ctrl+Space, as the parser delivers it: the palette.
pub const KEY_CTRL_SPACE: u8 = 0x86;
/// The palette card (GFX-053).
pub const PALETTE_WIDTH: usize = 560;
pub const PALETTE_MAX_ROWS: usize = 8;
/// Notice cards (GFX-053).
pub const NOTICE_WIDTH: usize = 340;
/// The one font's advance, for fitting text to a card's width.
pub const GLYPH_WIDTH: usize = 8;
pub const NOTICE_MARGIN: usize = 12;
/// How long a desk-raised notice stays, in ticks (100 Hz).
pub const NOTICE_TTL_TICKS: u64 = 400;
pub const FILES_SIZE: (usize, usize) = (680, 440);

/// The apps the dock offers, in dock order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeskApp {
    Notepad,
    /// The real filesystem, as a list: Enter or a click opens a file in a
    /// Notepad.
    Files,
    /// The machine's console -- the `WS >` prompt and everything it can do --
    /// as a card, so the desk never has to be left to reach it.
    Terminal,
    /// Themes and accents, previewed live and kept on disk (GFX-059).
    Look,
    /// The first-boot card: five lines on how the desk works (GFX-065).
    /// Not on the dock; opened once by the kernel, closed by a click.
    Welcome,
    /// Every notice the desk has shown, newest first (GFX-069). Not on the
    /// dock: the bar's "N new" and the palette open it.
    Notices,
    /// The machine, now: clock, uptime, memory, CPUs, what is open
    /// (GFX-072). A click on the clock opens it.
    Now,
    /// Every key the desk answers, in one card (GFX-072).
    Shortcuts,
    /// Exact decimal arithmetic with keys to click and a tape (GFX-075).
    Calculator,
    /// A month at a glance; a day's note is a document named by the day
    /// (GFX-076).
    Calendar,
    /// A stopwatch with laps and a countdown that says when it is up
    /// (GFX-077).
    Timer,
    /// The sliding-tiles game (GFX-078).
    Tiles,
    /// The to-do list, kept as the document `tasks` (GFX-079).
    Tasks,
    /// Draw with the pointer; kept as the document `sketch` (GFX-080).
    Sketch,
}

impl DeskApp {
    pub const ALL: [DeskApp; 10] = [
        DeskApp::Notepad,
        DeskApp::Files,
        DeskApp::Terminal,
        DeskApp::Look,
        DeskApp::Calculator,
        DeskApp::Calendar,
        DeskApp::Timer,
        DeskApp::Tiles,
        DeskApp::Tasks,
        DeskApp::Sketch,
    ];

    /// What the desk must ask the kernel for right after `app` opens in
    /// card `id`: the cards that show a document need it read.
    pub fn launch_request(self, id: ViewId) -> Option<DeskRequest> {
        match self {
            DeskApp::Files | DeskApp::Calendar => Some(DeskRequest::ListFiles { id }),
            DeskApp::Tasks => Some(DeskRequest::PreviewFile {
                id,
                name: TASKS_FILE.to_string(),
            }),
            DeskApp::Sketch => Some(DeskRequest::PreviewFile {
                id,
                name: SKETCH_FILE.to_string(),
            }),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            DeskApp::Notepad => "Notepad",
            DeskApp::Files => "Files",
            DeskApp::Terminal => "Terminal",
            DeskApp::Look => "Look",
            DeskApp::Welcome => "Welcome",
            DeskApp::Notices => "Notices",
            DeskApp::Now => "Now",
            DeskApp::Shortcuts => "Shortcuts",
            DeskApp::Calculator => "Calculator",
            DeskApp::Calendar => "Calendar",
            DeskApp::Timer => "Timer",
            DeskApp::Tiles => "Tiles",
            DeskApp::Tasks => "Tasks",
            DeskApp::Sketch => "Sketch",
        }
    }

    /// The dock tile's icon (GFX-084): sixteen rows of sixteen bits, drawn
    /// in the text colour, two pixels a bit. The system cards that are
    /// not on the dock have none.
    pub const fn icon(self) -> Option<[u16; 16]> {
        match self {
            DeskApp::Notepad => Some([
                0b0000000000000000,
                0b0011111111111000,
                0b0010000000001000,
                0b0010000000001000,
                0b0010111111101000,
                0b0010000000001000,
                0b0010111111101000,
                0b0010000000001000,
                0b0010111111101000,
                0b0010000000001000,
                0b0010111110001000,
                0b0010000000001000,
                0b0010000000001000,
                0b0011111111111000,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Files => Some([
                0b0000000000000000,
                0b0000000000000000,
                0b0000111111111100,
                0b0000100000000100,
                0b0011111111111100,
                0b0010000000000100,
                0b0111111111111010,
                0b0100000000001110,
                0b0100000000001000,
                0b0100000000001000,
                0b0100000000001000,
                0b0100000000001000,
                0b0100000000001000,
                0b0111111111111000,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Terminal => Some([
                0b0000000000000000,
                0b0000000000000000,
                0b0011000000000000,
                0b0001100000000000,
                0b0000110000000000,
                0b0000011000000000,
                0b0000001100000000,
                0b0000011000000000,
                0b0000110000000000,
                0b0001100000000000,
                0b0011000000000000,
                0b0000000000000000,
                0b0000000011111110,
                0b0000000011111110,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Look => Some([
                0b0000000000000000,
                0b0000011111100000,
                0b0001100000011000,
                0b0010011000000100,
                0b0100011000110010,
                0b0100000000110010,
                0b1000000000000001,
                0b1001100000000001,
                0b1001100000000001,
                0b0100000000000010,
                0b0100011000000010,
                0b0010011001000100,
                0b0001100001011000,
                0b0000011111100000,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Calculator => Some([
                0b0000000000000000,
                0b0111111111111000,
                0b0100000000001000,
                0b0101111111101000,
                0b0100000000001000,
                0b0111111111111000,
                0b0100000000001000,
                0b0101101101101000,
                0b0100000000001000,
                0b0101101101101000,
                0b0100000000001000,
                0b0101101101101000,
                0b0100000000001000,
                0b0111111111111000,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Calendar => Some([
                0b0000000000000000,
                0b0001000000100000,
                0b0111111111111000,
                0b0111111111111000,
                0b0111111111111000,
                0b0100000000001000,
                0b0101010101001000,
                0b0100000000001000,
                0b0101010101001000,
                0b0100000000001000,
                0b0101010100001000,
                0b0100000000001000,
                0b0111111111111000,
                0b0000000000000000,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Timer => Some([
                0b0000000000000000,
                0b0000001111000000,
                0b0000110000110000,
                0b0001000000001000,
                0b0010000100000100,
                0b0010000100000100,
                0b0100000100000010,
                0b0100000111000010,
                0b0100000000000010,
                0b0010000000000100,
                0b0010000000000100,
                0b0001000000001000,
                0b0000110000110000,
                0b0000001111000000,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Tiles => Some([
                0b0000000000000000,
                0b0111111011111100,
                0b0111111011111100,
                0b0111111011111100,
                0b0111111011111100,
                0b0111111011111100,
                0b0111111011111100,
                0b0000000000000000,
                0b0111111011111100,
                0b0111111011111100,
                0b0111111011111100,
                0b0111111011111100,
                0b0111111011111100,
                0b0111111011111100,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Tasks => Some([
                0b0000000000000000,
                0b0111110000000000,
                0b0100010111111110,
                0b0100010000000000,
                0b0100010111110000,
                0b0111110000000000,
                0b0000000000000000,
                0b0111110000000000,
                0b0100110111111110,
                0b0101010000000000,
                0b0110010111110000,
                0b0111110000000000,
                0b0000000000000000,
                0b0000000000000000,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            DeskApp::Sketch => Some([
                0b0000000000000000,
                0b0000000000001100,
                0b0000000000011110,
                0b0000000000110110,
                0b0000000001101100,
                0b0000000011011000,
                0b0000000110110000,
                0b0000001101100000,
                0b0000011011000000,
                0b0000110110000000,
                0b0001101100000000,
                0b0010011000000000,
                0b0010110000000000,
                0b0011110000000000,
                0b0000000000000000,
                0b0000000000000000,
            ]),
            _ => None,
        }
    }

    /// Two letters for the dock tile: what a tile shows when it has no
    /// icon, and what the bar says about the tile under the pointer.
    pub const fn monogram(self) -> &'static str {
        match self {
            DeskApp::Notepad => "Np",
            DeskApp::Files => "Fi",
            DeskApp::Terminal => "Tm",
            DeskApp::Look => "Lk",
            DeskApp::Welcome => "Hi",
            DeskApp::Notices => "Nt",
            DeskApp::Now => "Nw",
            DeskApp::Shortcuts => "Ky",
            DeskApp::Calculator => "Ca",
            DeskApp::Calendar => "Cl",
            DeskApp::Timer => "Ti",
            DeskApp::Tiles => "2k",
            DeskApp::Tasks => "Td",
            DeskApp::Sketch => "Sk",
        }
    }

    const fn size(self) -> (usize, usize) {
        match self {
            DeskApp::Notepad => NOTEPAD_SIZE,
            DeskApp::Files => FILES_SIZE,
            DeskApp::Terminal => TERMINAL_SIZE,
            DeskApp::Look => LOOK_SIZE,
            DeskApp::Welcome => WELCOME_SIZE,
            DeskApp::Notices => NOTICES_SIZE,
            DeskApp::Now => NOW_SIZE,
            DeskApp::Shortcuts => SHORTCUTS_SIZE,
            DeskApp::Calculator => CALCULATOR_SIZE,
            DeskApp::Calendar => CALENDAR_SIZE,
            DeskApp::Timer => TIMER_SIZE,
            DeskApp::Tiles => TILES_SIZE,
            DeskApp::Tasks => TASKS_SIZE,
            DeskApp::Sketch => SKETCH_SIZE,
        }
    }
}

/// The Sketch card (GFX-080): a canvas.
pub const SKETCH_SIZE: (usize, usize) = (520, 420);

/// The Tasks card (GFX-079).
pub const TASKS_SIZE: (usize, usize) = (460, 400);

/// The Tiles card (GFX-078): the board and its buttons.
pub const TILES_SIZE: (usize, usize) = (340, 400);

/// The Timer card (GFX-077): the controls, the presets, six laps.
pub const TIMER_SIZE: (usize, usize) = (400, 420);

/// The Calendar card (GFX-076): six week rows, today, the selected day.
pub const CALENDAR_SIZE: (usize, usize) = (420, 400);

/// The Calculator card (GFX-075, GFX-081): a display, real keys, a tape.
pub const CALCULATOR_SIZE: (usize, usize) = (300, 460);

/// The Now card (GFX-072).
pub const NOW_SIZE: (usize, usize) = (440, 260);
/// The shortcut sheet (GFX-072).
pub const SHORTCUTS_SIZE: (usize, usize) = (560, 520);
/// How often the Now card asks the kernel for fresh vitals, in ticks.
pub const VITALS_EVERY: u64 = 100;

/// What the kernel knows about the machine right now (GFX-072).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vitals {
    pub uptime_ticks: u64,
    pub heap_used_kib: usize,
    pub heap_total_kib: usize,
    pub cpus_online: usize,
    pub cpus_total: usize,
    pub files: usize,
}

/// The desk's own keys, for the sheet: the ones no palette row carries.
pub const DESK_KEYS: [(&str, &str); 9] = [
    ("Search files and actions", "type on the desk"),
    ("The palette", "Ctrl+Space, or click the bar"),
    ("Every card at once", "Ctrl+Tab (hold; let go to pick)"),
    ("Snap left / right", "Ctrl+Left / Ctrl+Right"),
    ("Fill the desk / put back", "Ctrl+Up / Ctrl+Down"),
    ("Go to a space", "Ctrl+1..4"),
    ("Move the card to a space", "Ctrl+Shift+1..4"),
    ("Close the card", "Ctrl+W"),
    ("Tuck a card", "drag it onto the dock"),
];

/// The Notices card (GFX-069).
pub const NOTICES_SIZE: (usize, usize) = (560, 400);
/// How many notices the log keeps.
pub const NOTICE_LOG: usize = 50;

/// The Welcome card (GFX-065): wide enough for its lines, low enough to
/// sit above the dock without covering the middle of the desk.
pub const WELCOME_SIZE: (usize, usize) = (440, 200);

/// What the Welcome card says. Short, and every line is a thing to try.
pub const WELCOME_LINES: [&str; 5] = [
    "Just start typing to search files and actions.",
    "Ctrl+Space or a click on the bar: the palette.",
    "Everything a card can do is in its header chips.",
    "Documents save themselves and keep every version.",
    "Look, on the dock, changes the theme as you point.",
];

/// The Look card (GFX-059).
pub const LOOK_SIZE: (usize, usize) = (420, 420);

/// What the desk looks like: a preset and an accent, by name (GFX-059).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookChoice {
    pub theme: String,
    pub accent: String,
    /// "Picture" or "Gradient" (GFX-066).
    pub wallpaper: String,
}

impl Default for LookChoice {
    fn default() -> Self {
        Self {
            theme: "Dusk".to_string(),
            accent: "Mint".to_string(),
            wallpaper: "Picture".to_string(),
        }
    }
}

impl LookChoice {
    pub fn theme(&self) -> Theme {
        let base = Theme::named(&self.theme).unwrap_or(Theme::DESK);
        match Theme::accent_named(&self.accent) {
            Some(accent) => base.with_accent(accent),
            None => base,
        }
    }

    /// The on-disk form: two lines, `theme=` and `accent=`.
    pub fn to_text(&self) -> String {
        alloc::format!(
            "theme={}\naccent={}\nwallpaper={}\n",
            self.theme,
            self.accent,
            self.wallpaper
        )
    }

    /// The picture, when the choice is the picture.
    pub fn wallpaper(&self) -> Option<Wallpaper> {
        self.wallpaper
            .eq_ignore_ascii_case("Picture")
            .then_some(WALLPAPER)
    }

    /// Parse the on-disk form; unknown names fall back to the defaults, so
    /// a hand-edited or damaged file cannot leave the desk unreadable.
    pub fn from_text(text: &str) -> Self {
        let mut choice = Self::default();
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("theme=") {
                if let Some((canonical, _)) = Theme::PRESETS
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
                {
                    choice.theme = canonical.to_string();
                }
            } else if let Some(name) = line.strip_prefix("wallpaper=") {
                if let Some(canonical) = WALLPAPERS
                    .iter()
                    .find(|n| n.eq_ignore_ascii_case(name.trim()))
                {
                    choice.wallpaper = canonical.to_string();
                }
            } else if let Some(name) = line.strip_prefix("accent=") {
                if let Some((canonical, _)) = Theme::ACCENTS
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
                {
                    choice.accent = canonical.to_string();
                }
            }
        }
        choice
    }
}

/// The Look app's state: which row is under the highlight. Rows are the
/// presets, then the accents; moving previews at once, Enter keeps.
#[derive(Debug, Clone, Default)]
pub struct LookView {
    pub row: usize,
}

impl LookView {
    const THEME_ROWS: usize = Theme::PRESETS.len();
    const ACCENT_ROWS: usize = Theme::PRESETS.len() + Theme::ACCENTS.len();
    const ROWS: usize = Self::ACCENT_ROWS + WALLPAPERS.len();

    /// The content lines: a heading, the presets, a gap, a heading, the
    /// accents. `line_of_row` maps a row to its line for the highlight.
    fn lines(kept: &LookChoice, preview: &LookChoice) -> Vec<String> {
        let mark = |name: &str, current: &str| {
            if name.eq_ignore_ascii_case(current) {
                alloc::format!("  * {name}")
            } else {
                alloc::format!("    {name}")
            }
        };
        let mut lines = alloc::vec!["Theme".to_string()];
        for (name, _) in Theme::PRESETS.iter() {
            let mut line = mark(name, &preview.theme);
            if name.eq_ignore_ascii_case(&kept.theme) && !name.eq_ignore_ascii_case(&preview.theme)
            {
                line.push_str("   (kept)");
            }
            lines.push(line);
        }
        lines.push(String::new());
        lines.push("Accent".to_string());
        for (name, _) in Theme::ACCENTS.iter() {
            let mut line = mark(name, &preview.accent);
            if name.eq_ignore_ascii_case(&kept.accent)
                && !name.eq_ignore_ascii_case(&preview.accent)
            {
                line.push_str("   (kept)");
            }
            lines.push(line);
        }
        lines.push(String::new());
        lines.push("Wallpaper".to_string());
        for name in WALLPAPERS.iter() {
            let mut line = mark(name, &preview.wallpaper);
            if name.eq_ignore_ascii_case(&kept.wallpaper)
                && !name.eq_ignore_ascii_case(&preview.wallpaper)
            {
                line.push_str("   (kept)");
            }
            lines.push(line);
        }
        lines
    }

    fn line_of_row(row: usize) -> usize {
        if row < Self::THEME_ROWS {
            1 + row
        } else if row < Self::ACCENT_ROWS {
            3 + row
        } else {
            5 + row
        }
    }

    /// The inverse of `line_of_row`: headings and the gap are no row.
    fn row_of_line(line: usize) -> Option<usize> {
        (0..Self::ROWS).find(|row| Self::line_of_row(*row) == line)
    }

    /// The choice the highlighted row stands for, given the current one.
    fn choice_at(&self, current: &LookChoice) -> LookChoice {
        let mut choice = current.clone();
        if self.row < Self::THEME_ROWS {
            choice.theme = Theme::PRESETS[self.row].0.to_string();
        } else if self.row < Self::ACCENT_ROWS {
            choice.accent = Theme::ACCENTS[self.row - Self::THEME_ROWS].0.to_string();
        } else {
            choice.wallpaper = WALLPAPERS[self.row - Self::ACCENT_ROWS].to_string();
        }
        choice
    }

    /// The row that stands for `choice`'s theme (so the card opens on it).
    fn row_of(choice: &LookChoice) -> usize {
        Theme::PRESETS
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(&choice.theme))
            .unwrap_or(0)
    }
}

impl DeskWindow {
    /// The header chips (GFX-056): each is a key the app already answers,
    /// so the pointer and the keyboard reach the same code. Files' depend
    /// on its state (GFX-057).
    pub fn actions(&self) -> Vec<(String, u8)> {
        match &self.state {
            AppState::Notepad(notepad) if notepad.browsing_history() => alloc::vec![
                ("Older".to_string(), crate::notepad::KEY_LEFT),
                ("Newer".to_string(), crate::notepad::KEY_RIGHT),
                ("Restore".to_string(), b'\n'),
                ("Back".to_string(), crate::notepad::ESC),
            ],
            AppState::Notepad(notepad) if notepad.prompt_open() == Some("find") => alloc::vec![
                ("Next".to_string(), b'\n'),
                ("Close".to_string(), crate::notepad::ESC),
            ],
            AppState::Notepad(notepad) if notepad.prompt_open().is_some() => {
                alloc::vec![("Cancel".to_string(), crate::notepad::ESC)]
            }
            AppState::Notepad(_) => alloc::vec![
                ("Save".to_string(), crate::notepad::CTRL_S),
                ("Save as".to_string(), crate::notepad::CTRL_SHIFT_S),
                ("Open".to_string(), crate::notepad::CTRL_O),
                ("Find".to_string(), crate::notepad::CTRL_F),
                ("History".to_string(), crate::notepad::CTRL_Y),
                ("Split".to_string(), crate::notepad::CTRL_D),
            ],
            AppState::Files(files) => files.actions(),
            AppState::Terminal => Vec::new(),
            AppState::Look(_) => alloc::vec![
                ("Keep".to_string(), b'\n'),
                ("Revert".to_string(), crate::notepad::ESC),
            ],
            AppState::Welcome => alloc::vec![("Got it".to_string(), crate::notepad::CTRL_W)],
            AppState::Notices => alloc::vec![
                ("Clear".to_string(), crate::notepad::KEY_DELETE),
                ("Close".to_string(), crate::notepad::CTRL_W),
            ],
            AppState::Now => alloc::vec![
                ("Notices".to_string(), b'n'),
                ("Look".to_string(), b'l'),
                ("Shortcuts".to_string(), b'k'),
                ("Close".to_string(), crate::notepad::CTRL_W),
            ],
            AppState::Shortcuts(_) => alloc::vec![("Close".to_string(), crate::notepad::CTRL_W)],
            AppState::Calculator(_) => alloc::vec![
                ("Clear".to_string(), crate::notepad::ESC),
                ("Close".to_string(), crate::notepad::CTRL_W),
            ],
            AppState::Calendar(_) => alloc::vec![
                ("Today".to_string(), b't'),
                ("Earlier".to_string(), crate::notepad::KEY_PAGE_UP),
                ("Later".to_string(), crate::notepad::KEY_PAGE_DOWN),
                ("Note".to_string(), b'\n'),
                ("Close".to_string(), crate::notepad::CTRL_W),
            ],
            AppState::Timer(timer) => alloc::vec![
                (
                    if timer.running() { "Pause" } else { "Start" }.to_string(),
                    b' ',
                ),
                ("Reset".to_string(), b'r'),
                ("Close".to_string(), crate::notepad::CTRL_W),
            ],
            AppState::Tiles(_) => alloc::vec![
                ("New".to_string(), b'n'),
                ("Close".to_string(), crate::notepad::CTRL_W),
            ],
            AppState::Tasks(tasks) if tasks.prompt_open() => alloc::vec![
                ("Add".to_string(), b'\n'),
                ("Cancel".to_string(), crate::notepad::ESC),
            ],
            AppState::Tasks(_) => alloc::vec![
                ("Add".to_string(), b'a'),
                ("Done".to_string(), b'\n'),
                ("Remove".to_string(), crate::notepad::KEY_DELETE),
                ("Close".to_string(), crate::notepad::CTRL_W),
            ],
            AppState::Sketch(_) => alloc::vec![
                ("Colour".to_string(), b'c'),
                ("Undo".to_string(), b'z'),
                ("Clear".to_string(), b'x'),
                ("Close".to_string(), crate::notepad::CTRL_W),
            ],
        }
    }
}

/// Ctrl+R: refresh the Files listing.
pub const CTRL_R: u8 = 0x12;

/// One entry of the root directory, as the kernel lists it (GFX-056).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    /// "file", "folder" or "object".
    pub kind: &'static str,
    /// Seconds since the epoch when last written; 0 when never stamped.
    pub modified_at: u64,
    /// What the content is, when the writer said (GFX-057).
    pub schema: Option<String>,
    pub tags: Vec<String>,
    /// In the bin.
    pub trashed: bool,
    /// How many earlier contents are kept.
    pub versions: usize,
}

impl FileEntry {
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_string(),
            kind: "file",
            ..Self::default()
        }
    }

    /// The type as a word: from the schema, never from the name.
    pub fn kind_label(&self) -> &'static str {
        match (self.kind, self.schema.as_deref()) {
            ("folder", _) => "Folder",
            (_, Some("text/plain")) => "Text",
            (_, Some(_)) => "Data",
            (_, None) => "File",
        }
    }
}

/// `12 B`, `1.5 KB`, `2.0 MB`.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        alloc::format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        alloc::format!("{}.{} KB", bytes / 1024, (bytes % 1024) * 10 / 1024)
    } else {
        let mb = bytes / (1024 * 1024);
        alloc::format!("{}.{} MB", mb, (bytes % (1024 * 1024)) * 10 / (1024 * 1024))
    }
}

/// A one-line prompt in the Files footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesPrompt {
    /// A name for a new, empty file.
    New(String),
    /// A new name for the selected file.
    Rename(String),
    /// Tags to add (and `-tag` to remove) on the selected file.
    Tag(String),
    /// Remove the selected file from the bin for good? Enter confirms.
    ConfirmPurge,
}

/// Ctrl+E: rename ("edit the name"). Ctrl+K: tag. Ctrl+B: the bin.
pub const CTRL_E: u8 = 0x05;
pub const CTRL_K: u8 = 0x0B;
pub const CTRL_B: u8 = 0x02;
/// Ctrl+U: the preview beside the list, on or off (GFX-071).
pub const CTRL_U: u8 = 0x15;
/// Ctrl+G: gather by the next tag (GFX-071).
pub const CTRL_G: u8 = 0x07;
/// How many lines a preview reads, and how wide it may be.
pub const PREVIEW_LINES: usize = 40;
/// A card narrower than this many cells shows the list alone.
pub const PREVIEW_MIN_COLUMNS: usize = 60;

/// The Files app's state (GFX-057): what the kernel listed, a filter typed
/// straight into the card, a sort, whether the bin is showing, the
/// selection within what is visible, and any prompt in the footer. There
/// are no folders: a file is found by name or tag, and by recency.
#[derive(Debug, Clone, Default)]
pub struct FilesView {
    pub entries: Vec<FileEntry>,
    /// Index into [`FilesView::visible`].
    pub selection: usize,
    pub loaded: bool,
    pub scroll: usize,
    pub prompt: Option<FilesPrompt>,
    /// Typed into the card: matches names and tags, case-insensitively.
    pub filter: String,
    /// Sorted by name rather than by recency.
    pub by_name: bool,
    /// Showing the bin instead of the files.
    pub bin: bool,
    /// The list alone; the default is the list with a preview (GFX-071).
    pub list_only: bool,
    /// The selected file's first lines, as the kernel read them: `(name,
    /// lines)`. Shown beside the list.
    pub preview: Option<(String, Vec<String>)>,
    /// The name a preview was asked for and has not arrived.
    pub preview_pending: Option<String>,
    /// Only files carrying this tag (GFX-071); a collection, not a folder.
    pub tag_filter: Option<String>,
}

impl FilesView {
    /// Indices into `entries` that the card shows, in display order.
    pub fn visible(&self) -> Vec<usize> {
        let filter = self.filter.to_ascii_lowercase();
        let mut shown: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.trashed == self.bin)
            // Names starting with '.' are the system's (the desk's look);
            // Files is for the person's files.
            .filter(|(_, e)| !e.name.starts_with('.'))
            .filter(|(_, e)| match &self.tag_filter {
                Some(tag) => e.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)),
                None => true,
            })
            .filter(|(_, e)| {
                filter.is_empty()
                    || e.name.to_ascii_lowercase().contains(&filter)
                    || e.tags
                        .iter()
                        .any(|t| t.to_ascii_lowercase().contains(&filter))
            })
            .map(|(i, _)| i)
            .collect();
        if self.by_name {
            shown.sort_by(|a, b| self.entries[*a].name.cmp(&self.entries[*b].name));
        } else {
            shown.sort_by(|a, b| {
                self.entries[*b]
                    .modified_at
                    .cmp(&self.entries[*a].modified_at)
                    .then_with(|| self.entries[*a].name.cmp(&self.entries[*b].name))
            });
        }
        shown
    }

    fn select(&mut self, delta: isize) {
        let count = self.visible().len();
        if count == 0 {
            self.selection = 0;
            return;
        }
        let last = count as isize - 1;
        self.selection = (self.selection as isize + delta).clamp(0, last) as usize;
    }

    pub fn selected(&self) -> Option<&FileEntry> {
        self.visible()
            .get(self.selection)
            .map(|index| &self.entries[*index])
    }

    /// The footer: a prompt while one is open, otherwise everything known
    /// about the selected entry -- the details live where the eye already
    /// is, not in a dialog.
    fn footer(&self) -> String {
        match &self.prompt {
            Some(FilesPrompt::New(name)) => {
                return alloc::format!("New: {name}_   a name, no suffix needed   (Enter, Esc)")
            }
            Some(FilesPrompt::Rename(name)) => {
                return alloc::format!("Rename to: {name}_   (Enter renames, Esc cancels)")
            }
            Some(FilesPrompt::Tag(tags)) => {
                return alloc::format!(
                    "Tags: {tags}_   space-separated, -tag removes   (Enter, Esc)"
                )
            }
            Some(FilesPrompt::ConfirmPurge) => {
                let name = self.selected().map(|e| e.name.as_str()).unwrap_or("");
                return alloc::format!("Remove {name} for good?   Enter removes, Esc keeps it");
            }
            None => {}
        }
        if !self.loaded {
            return "Reading the filesystem...".to_string();
        }
        let mut text = String::new();
        if let Some(tag) = &self.tag_filter {
            text.push_str(&alloc::format!("#{tag} only   "));
        }
        if !self.filter.is_empty() {
            text.push_str(&alloc::format!(
                "Filter: {}_   {} of {}   ",
                self.filter,
                self.visible().len(),
                self.entries
                    .iter()
                    .filter(|e| e.trashed == self.bin)
                    .count()
            ));
        }
        match self.selected() {
            Some(entry) => {
                text.push_str(&alloc::format!(
                    "{}   {}   {}",
                    entry.name,
                    entry.kind_label(),
                    format_size(entry.size)
                ));
                if !entry.tags.is_empty() {
                    text.push_str(&alloc::format!("   #{}", entry.tags.join(" #")));
                }
                text.push_str(&alloc::format!(
                    "   written {}",
                    crate::rtc::format_unix_minutes(entry.modified_at)
                ));
                if entry.versions > 0 {
                    text.push_str(&alloc::format!("   {} earlier", entry.versions));
                }
                text.push_str(if self.bin {
                    "   Enter restores"
                } else {
                    "   Enter opens"
                });
            }
            None if self.bin => text.push_str("The bin is empty"),
            None if !self.filter.is_empty() => text.push_str("Nothing matches"),
            None => {
                text.push_str("No files yet   Ctrl+N makes one, or just start typing to search")
            }
        }
        text
    }

    /// One row: name, tags, type, size and time in columns that fit
    /// `columns` character cells. The name column takes what the fixed
    /// columns leave; narrow cards lose the rightmost columns first.
    fn line(entry: &FileEntry, columns: usize) -> String {
        let when = crate::rtc::format_unix_minutes(entry.modified_at);
        let size = format_size(entry.size);
        let tags: String = entry.tags.iter().map(|t| alloc::format!("#{t} ")).collect();
        let tags: String = tags.trim_end().chars().take(14).collect();
        let clipped = |width: usize| -> String { entry.name.chars().take(width).collect() };
        // " {:<14} {:<6} {:>8}  {:16}" after the name.
        const TAIL_FULL: usize = 1 + 14 + 1 + 6 + 1 + 8 + 2 + 16;
        const TAIL_SHORT: usize = 1 + 8;
        if columns >= TAIL_FULL + 12 {
            let name_w = columns - TAIL_FULL;
            alloc::format!(
                "{:<name_w$} {:<14} {:<6} {:>8}  {}",
                clipped(name_w),
                tags,
                entry.kind_label(),
                size,
                when
            )
        } else if columns >= TAIL_SHORT + 8 {
            let name_w = columns - TAIL_SHORT;
            alloc::format!("{:<name_w$} {:>8}", clipped(name_w), size)
        } else {
            clipped(columns)
        }
    }

    /// Every tag on the files (not the bin), sorted, once each.
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .entries
            .iter()
            .filter(|e| !e.trashed && !e.name.starts_with('.'))
            .flat_map(|e| e.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Gather by the next tag: all -> first tag -> ... -> all (GFX-071).
    fn cycle_tag(&mut self) {
        let tags = self.all_tags();
        self.tag_filter = match &self.tag_filter {
            None => tags.first().cloned(),
            Some(current) => tags
                .iter()
                .position(|t| t == current)
                .and_then(|i| tags.get(i + 1))
                .cloned(),
        };
        self.selection = 0;
        self.scroll = 0;
    }

    /// The header chips for this state (GFX-057): the bin has its own.
    fn actions(&self) -> Vec<(String, u8)> {
        let sort = if self.by_name { "A-Z" } else { "Recent" };
        let gather = match &self.tag_filter {
            Some(tag) => alloc::format!("#{tag}"),
            None => "All".to_string(),
        };
        let preview = if self.list_only { "Preview" } else { "List" };
        if self.bin {
            alloc::vec![
                ("Restore".to_string(), b'\n'),
                ("Remove".to_string(), crate::notepad::KEY_DELETE),
                (sort.to_string(), crate::notepad::CTRL_S),
                ("Files".to_string(), CTRL_B),
            ]
        } else {
            alloc::vec![
                ("New".to_string(), crate::notepad::CTRL_N),
                ("Rename".to_string(), CTRL_E),
                ("Tag".to_string(), CTRL_K),
                ("Bin it".to_string(), crate::notepad::KEY_DELETE),
                (gather, CTRL_G),
                (sort.to_string(), crate::notepad::CTRL_S),
                (preview.to_string(), CTRL_U),
                ("Bin".to_string(), CTRL_B),
            ]
        }
    }
}

/// `work draft -old` -> add `work`, `draft`; remove `old`.
pub fn parse_tag_edit(text: &str) -> (Vec<String>, Vec<String>) {
    let mut add = Vec::new();
    let mut remove = Vec::new();
    for word in text.split_whitespace() {
        let word = word.trim_start_matches('#');
        if let Some(rest) = word.strip_prefix('-') {
            if !rest.is_empty() {
                remove.push(rest.to_ascii_lowercase());
            }
        } else if !word.is_empty() {
            add.push(word.to_ascii_lowercase());
        }
    }
    (add, remove)
}

/// Break `text` into lines of at most `columns` characters at spaces; a
/// word longer than a line is cut. A notice card is 340px wide and the
/// workspace's messages are longer than that -- the first build showed
/// "Unknown command: files. Type 'help' for h" and stopped.
pub fn wrap_words(text: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let mut word = word;
        while !word.is_empty() {
            let used = line.chars().count();
            let sep = usize::from(!line.is_empty());
            let len = word.chars().count();
            if used + sep + len <= columns {
                if sep == 1 {
                    line.push(' ');
                }
                line.push_str(word);
                break;
            }
            if used > 0 {
                lines.push(core::mem::take(&mut line));
                continue;
            }
            let cut = word
                .char_indices()
                .nth(columns)
                .map(|(i, _)| i)
                .unwrap_or(word.len());
            lines.push(word[..cut].to_string());
            word = &word[cut..];
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// What an open window holds.
#[derive(Debug, Clone)]
pub enum AppState {
    Notepad(Notepad),
    Files(FilesView),
    /// The console's state lives in the workspace; the card only shows it.
    Terminal,
    Look(LookView),
    Welcome,
    Notices,
    Now,
    /// The sheet, with how many rows it has scrolled.
    Shortcuts(usize),
    Calculator(Calculator),
    Calendar(CalendarView),
    Timer(TimerView),
    Tiles(Game),
    Tasks(TasksView),
    Sketch(SketchView),
}

/// Where screen pixel `(px, py)` falls in the canvas of a card with
/// `bounds`: the content area's own pixel space, the one the compositor
/// draws graphics in (GFX-080). `None` outside it.
fn canvas_point_in(bounds: RasterRect, px: usize, py: usize) -> Option<(i32, i32)> {
    let origin_x = bounds.x + services_gui_host::CARD_PADDING;
    let origin_y =
        bounds.y + services_gui_host::CARD_HEADER_HEIGHT + services_gui_host::CARD_PADDING;
    let right = bounds
        .right()
        .saturating_sub(services_gui_host::CARD_PADDING);
    let bottom = bounds
        .bottom()
        .saturating_sub(services_gui_host::CARD_PADDING + services_gui_host::CARD_FOOTER_HEIGHT);
    if px < origin_x || py < origin_y || px >= right || py >= bottom {
        return None;
    }
    Some(((px - origin_x) as i32, (py - origin_y) as i32))
}

/// One open window.
#[derive(Debug, Clone)]
pub struct DeskWindow {
    pub id: ViewId,
    pub app: DeskApp,
    pub bounds: RasterRect,
    pub z: usize,
    /// The bounds before a snap, so the next header drag un-snaps to them.
    pub restore: Option<RasterRect>,
    /// Tucked into the dock: not drawn, not focusable, waiting on its tile.
    pub tucked: bool,
    /// Which space the card is on (GFX-067).
    pub space: usize,
    pub state: AppState,
}

impl DeskWindow {
    pub fn notepad(&self) -> Option<&Notepad> {
        match &self.state {
            AppState::Notepad(notepad) => Some(notepad),
            _ => None,
        }
    }

    pub fn notepad_mut(&mut self) -> Option<&mut Notepad> {
        match &mut self.state {
            AppState::Notepad(notepad) => Some(notepad),
            _ => None,
        }
    }

    pub fn files(&self) -> Option<&FilesView> {
        match &self.state {
            AppState::Files(files) => Some(files),
            _ => None,
        }
    }

    pub fn look_mut(&mut self) -> Option<&mut LookView> {
        match &mut self.state {
            AppState::Look(look) => Some(look),
            _ => None,
        }
    }

    pub fn files_mut(&mut self) -> Option<&mut FilesView> {
        match &mut self.state {
            AppState::Files(files) => Some(files),
            _ => None,
        }
    }
}

/// A header drag in progress: which window, and where inside its header
/// the pointer grabbed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drag {
    id: ViewId,
    grab_x: usize,
    grab_y: usize,
}

/// A corner resize in progress: which window, and how far inside the
/// corner the pointer grabbed it, so the corner stays under the pointer
/// rather than jumping to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Resize {
    id: ViewId,
    grab_dx: usize,
    grab_dy: usize,
}

/// What the desk asks the kernel to do after handling input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeskRequest {
    /// Perform this file operation for the window with `id`, then call
    /// [`Desk::io_done`].
    Io { id: ViewId, effect: NotepadEffect },
    /// A key for the console, which the workspace owns.
    Terminal(u8),
    /// Draw the desk again: something on it moved on its own -- a running
    /// timer (GFX-077).
    Repaint,
    /// List the filesystem, then call [`Desk::files_listed`] for `id`.
    ListFiles { id: ViewId },
    /// Leave the desk for the text console.
    TextConsole,
    /// Scroll the console's view: positive notches towards older lines.
    TerminalScroll { notches: i32 },
    /// Create `name` empty, then list again for `id` and open it (GFX-056).
    CreateFile { id: ViewId, name: String },
    /// Give `from` the name `to`, then list again for `id`.
    RenameFile {
        id: ViewId,
        from: String,
        to: String,
    },
    /// Put `name` in the bin (or take it out), then list again (GFX-057).
    TrashFile {
        id: ViewId,
        name: String,
        trashed: bool,
    },
    /// Remove `name` for good, then list again.
    PurgeFile { id: ViewId, name: String },
    /// Add and remove tags on `name`, then list again.
    TagFile {
        id: ViewId,
        name: String,
        add: Vec<String>,
        remove: Vec<String>,
    },
    /// Read kept version `index` of `name` for the Notepad `id`, then call
    /// [`Desk::version_loaded`] (GFX-058).
    ReadVersion {
        id: ViewId,
        name: String,
        index: usize,
    },
    /// Write the desk's look to disk (GFX-059).
    SaveLook { text: String },
    /// Write the recently used files to disk (GFX-060); read back with
    /// the look.
    SaveRecent { text: String },
    /// Find lines containing `query` in the person's text files, then call
    /// [`Desk::search_results`] (GFX-061).
    SearchFiles { query: String },
    /// The Welcome card was closed: remember that on disk (GFX-065).
    Welcomed,
    /// Read the first lines of `name` for the Files card `id`, then call
    /// [`Desk::preview_loaded`] (GFX-071).
    PreviewFile { id: ViewId, name: String },
    /// Say how the machine is, then call [`Desk::vitals_loaded`] (GFX-072).
    Vitals,
    /// Read the desk's look from disk, then call [`Desk::apply_look`].
    LoadLook,
}

/// One row of the palette: what it does and how it is spelled (GFX-053).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    NewNotepad,
    NewTerminal,
    OpenFiles,
    Save,
    SaveAs,
    Open,
    CloseWindow,
    NextWindow,
    SnapLeft,
    SnapRight,
    Maximise,
    Tuck,
    TextConsole,
    SelectAll,
    Copy,
    Cut,
    Paste,
    Find,
    ToggleTheme,
    History,
    /// This document in a second card, side by side (GFX-074).
    Split,
    /// Go to space 1..4 (GFX-067).
    Space(usize),
    /// Move the focused card to space 1..4.
    MoveToSpace(usize),
    /// Every card at once (GFX-068).
    Overview,
    /// Every notice, newest first (GFX-069).
    Notices,
    /// The machine, now (GFX-072).
    Now,
    /// Every key (GFX-072).
    Shortcuts,
    /// The Calculator card (GFX-075).
    Calculator,
    /// The Calendar card (GFX-076).
    Calendar,
    /// The Timer card (GFX-077).
    Timer,
    /// The Tiles game (GFX-078).
    Tiles,
    /// The Tasks card (GFX-079).
    Tasks,
    /// The Sketch card (GFX-080).
    Sketch,
}

impl PaletteAction {
    pub const ALL: [PaletteAction; 39] = [
        PaletteAction::NewNotepad,
        PaletteAction::NewTerminal,
        PaletteAction::OpenFiles,
        PaletteAction::Save,
        PaletteAction::SaveAs,
        PaletteAction::Open,
        PaletteAction::CloseWindow,
        PaletteAction::NextWindow,
        PaletteAction::SnapLeft,
        PaletteAction::SnapRight,
        PaletteAction::Maximise,
        PaletteAction::Tuck,
        PaletteAction::TextConsole,
        PaletteAction::SelectAll,
        PaletteAction::Copy,
        PaletteAction::Cut,
        PaletteAction::Paste,
        PaletteAction::Find,
        PaletteAction::ToggleTheme,
        PaletteAction::History,
        PaletteAction::Split,
        PaletteAction::Space(0),
        PaletteAction::Space(1),
        PaletteAction::Space(2),
        PaletteAction::Space(3),
        PaletteAction::MoveToSpace(0),
        PaletteAction::MoveToSpace(1),
        PaletteAction::MoveToSpace(2),
        PaletteAction::MoveToSpace(3),
        PaletteAction::Overview,
        PaletteAction::Notices,
        PaletteAction::Now,
        PaletteAction::Shortcuts,
        PaletteAction::Calculator,
        PaletteAction::Calendar,
        PaletteAction::Timer,
        PaletteAction::Tiles,
        PaletteAction::Tasks,
        PaletteAction::Sketch,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            PaletteAction::NewNotepad => "New Notepad window",
            PaletteAction::NewTerminal => "New Terminal",
            PaletteAction::OpenFiles => "Files",
            PaletteAction::Save => "Save",
            PaletteAction::SaveAs => "Save as...",
            PaletteAction::Open => "Open file...",
            PaletteAction::CloseWindow => "Close window",
            PaletteAction::NextWindow => "Next window",
            PaletteAction::SnapLeft => "Snap left",
            PaletteAction::SnapRight => "Snap right",
            PaletteAction::Maximise => "Fill the desk",
            PaletteAction::Tuck => "Tuck into the dock",
            PaletteAction::TextConsole => "Switch to the text console",
            PaletteAction::SelectAll => "Select all",
            PaletteAction::Copy => "Copy",
            PaletteAction::Cut => "Cut",
            PaletteAction::Paste => "Paste",
            PaletteAction::Find => "Find...",
            PaletteAction::ToggleTheme => "Look: themes and accents",
            PaletteAction::History => "Earlier versions of this document",
            PaletteAction::Split => "Split: this document in a second card",
            PaletteAction::Space(0) => "Go to space 1",
            PaletteAction::Space(1) => "Go to space 2",
            PaletteAction::Space(2) => "Go to space 3",
            PaletteAction::Space(_) => "Go to space 4",
            PaletteAction::MoveToSpace(0) => "Move this card to space 1",
            PaletteAction::MoveToSpace(1) => "Move this card to space 2",
            PaletteAction::MoveToSpace(2) => "Move this card to space 3",
            PaletteAction::MoveToSpace(_) => "Move this card to space 4",
            PaletteAction::Overview => "Overview: every card at once",
            PaletteAction::Notices => "Notices: everything the desk has said",
            PaletteAction::Now => "Now: the machine, this second",
            PaletteAction::Shortcuts => "Shortcuts: every key the desk answers",
            PaletteAction::Calculator => "Calculator",
            PaletteAction::Calendar => "Calendar",
            PaletteAction::Timer => "Timer: stopwatch and countdown",
            PaletteAction::Tiles => "Tiles: the 2048 game",
            PaletteAction::Tasks => "Tasks: the to-do list",
            PaletteAction::Sketch => "Sketch: draw with the pointer",
        }
    }

    pub const fn shortcut(self) -> &'static str {
        match self {
            PaletteAction::NewNotepad => "Ctrl+N",
            PaletteAction::NewTerminal => "Ctrl+T",
            PaletteAction::OpenFiles => "",
            PaletteAction::Save => "Ctrl+S",
            PaletteAction::SaveAs => "",
            PaletteAction::Open => "Ctrl+O",
            PaletteAction::CloseWindow => "Ctrl+W",
            PaletteAction::NextWindow => "",
            PaletteAction::SnapLeft => "Ctrl+Left",
            PaletteAction::SnapRight => "Ctrl+Right",
            PaletteAction::Maximise => "Ctrl+Up",
            PaletteAction::Tuck => "drag onto the dock",
            PaletteAction::TextConsole => "",
            PaletteAction::SelectAll => "Ctrl+A",
            PaletteAction::Copy => "Ctrl+C",
            PaletteAction::Cut => "Ctrl+X",
            PaletteAction::Paste => "Ctrl+V",
            PaletteAction::Find => "Ctrl+F",
            PaletteAction::ToggleTheme => "",
            PaletteAction::History => "Ctrl+Y",
            PaletteAction::Split => "Ctrl+D",
            PaletteAction::Space(0) => "Ctrl+1",
            PaletteAction::Space(1) => "Ctrl+2",
            PaletteAction::Space(2) => "Ctrl+3",
            PaletteAction::Space(_) => "Ctrl+4",
            PaletteAction::MoveToSpace(0) => "Ctrl+Shift+1",
            PaletteAction::MoveToSpace(1) => "Ctrl+Shift+2",
            PaletteAction::MoveToSpace(2) => "Ctrl+Shift+3",
            PaletteAction::MoveToSpace(_) => "Ctrl+Shift+4",
            PaletteAction::Overview => "Ctrl+Tab",
            PaletteAction::Notices => "",
            PaletteAction::Now => "click the clock",
            PaletteAction::Shortcuts => "",
            PaletteAction::Calculator => "",
            PaletteAction::Calendar => "",
            PaletteAction::Timer => "",
            PaletteAction::Tiles => "",
            PaletteAction::Tasks => "",
            PaletteAction::Sketch => "",
        }
    }

    /// Whether the action needs a focused Notepad.
    const fn needs_notepad(self) -> bool {
        matches!(
            self,
            PaletteAction::Save
                | PaletteAction::SaveAs
                | PaletteAction::Open
                | PaletteAction::SelectAll
                | PaletteAction::Copy
                | PaletteAction::Cut
                | PaletteAction::Paste
                | PaletteAction::Find
                | PaletteAction::History
                | PaletteAction::Split
        )
    }

    /// Whether the action needs any focused window.
    const fn needs_window(self) -> bool {
        matches!(
            self,
            PaletteAction::CloseWindow
                | PaletteAction::SnapLeft
                | PaletteAction::SnapRight
                | PaletteAction::Maximise
                | PaletteAction::Tuck
                | PaletteAction::MoveToSpace(_)
        )
    }
}

/// The palette while it is open (GFX-053): a query and the actions that
/// match it, one selected. There are no menus anywhere on the desk; this is
/// the one place every action is listed, with its shortcut beside it.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub query: String,
    pub selection: usize,
    /// Lines inside files that contain the query, as the kernel found them
    /// for this query (GFX-061): `(file, line)`. Empty until it answers.
    pub hits: Vec<(String, String)>,
    /// The query the hits were found for; stale hits are not shown.
    pub hits_for: String,
}

/// Content search starts at this many characters, so one letter does not
/// read every file.
pub const SEARCH_MIN_CHARS: usize = 2;
/// How many lines a content search returns at most.
pub const SEARCH_MAX_HITS: usize = 6;

/// One row of the palette (GFX-060): an action, or a file opened lately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteRow {
    Action(PaletteAction),
    /// Open this file in a Notepad.
    Recent(String),
    /// A line inside a file that contains the query: open the file there.
    Hit {
        file: String,
        line: String,
    },
    /// Something copied lately: paste it into the focused Notepad (GFX-070).
    Paste(String),
}

/// How many copied texts the palette remembers.
pub const CLIPBOARD_HISTORY: usize = 10;

impl PaletteRow {
    pub fn label(&self) -> String {
        match self {
            PaletteRow::Action(action) => action.label().to_string(),
            PaletteRow::Recent(name) => alloc::format!("Open {name}"),
            PaletteRow::Hit { file, line } => {
                let line: String = line.trim().chars().take(40).collect();
                alloc::format!("{file}: {line}")
            }
            PaletteRow::Paste(text) => {
                let flat: String = text
                    .chars()
                    .map(|c| if c == '\n' { ' ' } else { c })
                    .take(40)
                    .collect();
                alloc::format!("Paste: {}", flat.trim())
            }
        }
    }

    pub fn shortcut(&self) -> &'static str {
        match self {
            PaletteRow::Action(action) => action.shortcut(),
            PaletteRow::Recent(_) => "recent",
            PaletteRow::Hit { .. } => "in file",
            PaletteRow::Paste(_) => "clipboard",
        }
    }
}

/// How many recently used files the palette offers.
pub const RECENT_FILES: usize = 5;

impl Palette {
    /// The rows that match the query, in order: the files used lately
    /// first, then the actions, filtered for the state of the desk so the
    /// list never offers something that would do nothing.
    pub fn matches(
        &self,
        has_window: bool,
        has_notepad: bool,
        recent: &[String],
        clips: &[String],
    ) -> Vec<PaletteRow> {
        let query = self.query.to_ascii_lowercase();
        let fits =
            |label: &str| query.is_empty() || label.to_ascii_lowercase().contains(query.as_str());
        let mut rows: Vec<PaletteRow> = recent
            .iter()
            .filter(|name| fits(&alloc::format!("Open {name}")))
            .map(|name| PaletteRow::Recent(name.clone()))
            .collect();
        if self.hits_for == self.query {
            rows.extend(self.hits.iter().map(|(file, line)| PaletteRow::Hit {
                file: file.clone(),
                line: line.clone(),
            }));
        }
        // Copied texts paste into a Notepad, so they are offered over one.
        if has_notepad {
            rows.extend(
                clips
                    .iter()
                    .map(|c| PaletteRow::Paste(c.clone()))
                    .filter(|row| fits(&row.label())),
            );
        }
        rows.extend(
            PaletteAction::ALL
                .iter()
                .copied()
                .filter(|action| !(action.needs_notepad() && !has_notepad))
                .filter(|action| !(action.needs_window() && !has_window))
                .filter(|action| fits(action.label()))
                .map(PaletteRow::Action),
        );
        rows
    }
}

/// A notice the desk raised itself (a save that landed), with its expiry.
#[derive(Debug, Clone)]
struct DeskNotice {
    notice: ShellNotice,
    expires_at: u64,
}

/// The console as the Terminal card shows it, built by the kernel from the
/// workspace: the lines that fit, and the caret on the prompt line.
#[derive(Debug, Clone, Default)]
pub struct TerminalView {
    pub lines: Vec<String>,
    /// `(line, character column)` of the caret within `lines`.
    pub cursor: Option<(usize, usize)>,
    pub status: String,
}

/// The whole desk.
#[derive(Debug, Clone)]
pub struct Desk {
    width: usize,
    height: usize,
    top_bar_id: ViewId,
    dock_id: ViewId,
    windows: Vec<DeskWindow>,
    focus: Option<ViewId>,
    drag: Option<Drag>,
    resize: Option<Resize>,
    next_z: usize,
    opened: usize,
    pointer: Option<(usize, usize)>,
    hovered_tile: Option<usize>,
    palette: Option<Palette>,
    palette_id: ViewId,
    /// One clipboard for every card (GFX-054)...
    clipboard: String,
    /// ...and what it held before, newest first (GFX-070).
    clipboard_history: Vec<String>,
    /// The look kept on disk (GFX-059)...
    look: LookChoice,
    /// ...and the one being previewed from the Look card, if any.
    look_preview: Option<LookChoice>,
    /// Files opened or saved lately, newest first (GFX-060).
    recent: Vec<String>,
    /// The Welcome card was closed this session; the kernel writes
    /// `.welcomed` and never shows it again (GFX-065).
    welcome_dismissed: bool,
    /// Notepads whose pending save is an autosave: no notice when it lands.
    quiet_saves: Vec<ViewId>,
    /// The space on screen (GFX-067).
    space: usize,
    /// The overview is open (GFX-068), with this entry of
    /// `overview_order` highlighted.
    overview: Option<usize>,
    /// A pointer drag selecting text in this card.
    text_select: Option<ViewId>,
    /// A stroke is being drawn in this Sketch card (GFX-080).
    sketching: Option<ViewId>,
    notices: Vec<DeskNotice>,
    notice_ids: Vec<ViewId>,
    /// Shell notices the person clicked away (GFX-063): hidden until the
    /// workspace's list changes.
    dismissed_shell: Vec<ShellNotice>,
    /// The workspace's notices as of the last frame, so a click can
    /// dismiss them.
    last_shell_notices: Vec<ShellNotice>,
    /// Every notice shown, newest first, with the tick it arrived
    /// (GFX-069); bounded by `NOTICE_LOG`.
    notice_log: Vec<(u64, ShellNotice)>,
    /// Notices logged since the Notices card was last looked at.
    unseen_notices: usize,
    /// The tick `windows_at` last saw, for "ago" in the Notices card.
    last_tick: u64,
    /// What the kernel last said about the machine (GFX-072), and when it
    /// was asked.
    vitals: Vitals,
    /// When vitals were last asked for, and whether the answer is still
    /// outstanding.
    vitals_asked_at: Option<u64>,
    vitals_pending: bool,
    /// The last file listing, for save-as and open autocomplete.
    file_names_cache: Vec<String>,
    /// The RTC's date, for the Calendar (GFX-076); `None` until it is read.
    today: Option<Date>,
    /// A document was saved since the Calendars last listed: their dots
    /// may be stale.
    notes_stale: bool,
    /// The tick a running timer was last drawn at (GFX-077).
    timer_drawn: u64,
}

impl Desk {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            top_bar_id: ViewId::new(),
            dock_id: ViewId::new(),
            windows: Vec::new(),
            focus: None,
            drag: None,
            resize: None,
            next_z: 1,
            opened: 0,
            pointer: None,
            hovered_tile: None,
            palette: None,
            palette_id: ViewId::new(),
            clipboard: String::new(),
            clipboard_history: Vec::new(),
            look: LookChoice::default(),
            look_preview: None,
            recent: Vec::new(),
            welcome_dismissed: false,
            quiet_saves: Vec::new(),
            space: 0,
            overview: None,
            text_select: None,
            sketching: None,
            notices: Vec::new(),
            notice_ids: (0..4).map(|_| ViewId::new()).collect(),
            dismissed_shell: Vec::new(),
            last_shell_notices: Vec::new(),
            notice_log: Vec::new(),
            unseen_notices: 0,
            last_tick: 0,
            vitals: Vitals::default(),
            vitals_asked_at: None,
            vitals_pending: false,
            file_names_cache: Vec::new(),
            today: None,
            notes_stale: false,
            timer_drawn: 0,
        }
    }

    /// The kernel read the clock: today's date, or `None` when it is not
    /// set. Every Calendar card marks it (GFX-076).
    pub fn set_today(&mut self, today: Option<Date>) {
        if self.today == today {
            return;
        }
        self.today = today;
        for window in &mut self.windows {
            if let AppState::Calendar(calendar) = &mut window.state {
                calendar.set_today(today);
            }
        }
    }

    /// Where screen pixel `(px, py)` falls in card `id`'s canvas -- the
    /// content area's own pixel space, the one the compositor draws
    /// graphics in (GFX-080). `None` outside it.
    fn canvas_point(&self, id: ViewId, px: usize, py: usize) -> Option<(i32, i32)> {
        canvas_point_in(self.window(id)?.bounds, px, py)
    }

    /// The canvas's size for a card with `bounds`: the content area
    /// between the header, the padding and the footer (GFX-081).
    fn canvas_size(bounds: RasterRect) -> (u32, u32) {
        let width = bounds
            .width
            .saturating_sub(services_gui_host::CARD_PADDING * 2);
        let height = bounds.height.saturating_sub(
            services_gui_host::CARD_HEADER_HEIGHT
                + services_gui_host::CARD_PADDING * 2
                + services_gui_host::CARD_FOOTER_HEIGHT,
        );
        (width as u32, height as u32)
    }

    /// Open a day's note (GFX-076): a Notepad on the document named by
    /// the day, read from disk when it exists, otherwise new with that
    /// name so it saves itself once something is typed.
    fn open_note(&mut self, name: String, exists: bool) -> Option<DeskRequest> {
        let id = self.launch(DeskApp::Notepad);
        if exists {
            return Some(DeskRequest::Io {
                id,
                effect: NotepadEffect::Open { path: name },
            });
        }
        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
            notepad.load(Some(name), "");
            notepad.set_status("A new note for the day: it saves itself as you type");
        }
        None
    }

    pub fn palette_open(&self) -> bool {
        self.palette.is_some()
    }

    /// Raise a notice card for a few seconds (GFX-053).
    pub fn notify(&mut self, level: NoticeLevel, text: impl Into<String>, now: u64) {
        let notice = ShellNotice::new(level, text);
        self.log_notice(now, notice.clone());
        self.notices.push(DeskNotice {
            notice,
            expires_at: now.saturating_add(NOTICE_TTL_TICKS),
        });
        let max = self.notice_ids.len();
        if self.notices.len() > max {
            let overflow = self.notices.len() - max;
            self.notices.drain(..overflow);
        }
    }

    /// The kernel answers a `ListFiles` request.
    pub fn files_listed(&mut self, id: ViewId, entries: Vec<FileEntry>) {
        let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
        self.file_names_cache = names.clone();
        if let Some(files) = self.window_mut(id).and_then(|w| w.files_mut()) {
            // Keep the selection on the same name across a refresh.
            let keep = files.selected().map(|e| e.name.clone());
            files.entries = entries;
            files.loaded = true;
            let visible = files.visible();
            files.selection = keep
                .and_then(|name| visible.iter().position(|i| files.entries[*i].name == name))
                .unwrap_or(0);
            files.select(0);
        }
        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
            notepad.set_file_names(names.clone());
        }
        if let Some(AppState::Calendar(calendar)) = self.window_mut(id).map(|w| &mut w.state) {
            calendar.set_file_names(&names);
        }
    }

    /// A header chip was pressed: the app's key for it (GFX-056).
    fn card_action(&mut self, id: ViewId, index: usize) -> (Option<DeskRequest>, bool) {
        let Some(actions) = self.window(id).map(|w| w.actions()) else {
            return (None, false);
        };
        let Some((_, byte)) = actions.get(index) else {
            return (None, false);
        };
        self.handle_app_key(id, *byte)
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// The theme the desk is drawn with: the preview while one is open,
    /// otherwise what is kept.
    pub fn theme(&self) -> Theme {
        self.look_preview.as_ref().unwrap_or(&self.look).theme()
    }

    /// A text was copied: to the front of the history, once, bounded.
    fn remember_copy(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        self.clipboard_history.retain(|t| *t != text);
        self.clipboard_history.insert(0, text);
        self.clipboard_history.truncate(CLIPBOARD_HISTORY);
    }

    /// What has been copied, newest first.
    pub fn clipboard_history(&self) -> &[String] {
        &self.clipboard_history
    }

    /// The kept look, by name.
    pub fn look(&self) -> &LookChoice {
        &self.look
    }

    /// The wallpaper behind the cards: the preview's while one is open,
    /// otherwise the kept choice's; `None` is the theme's gradient.
    pub fn wallpaper(&self) -> Option<Wallpaper> {
        self.look_preview.as_ref().unwrap_or(&self.look).wallpaper()
    }

    /// The kernel read the look file (GFX-059); an unreadable or missing
    /// file leaves the defaults.
    pub fn apply_look(&mut self, text: Option<&str>) {
        if let Some(text) = text {
            self.look = LookChoice::from_text(text);
        }
        self.look_preview = None;
    }

    /// The kernel read the recent-files list (GFX-060): one name a line.
    pub fn apply_recent(&mut self, text: Option<&str>) {
        if let Some(text) = text {
            self.recent = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(RECENT_FILES)
                .map(String::from)
                .collect();
        }
    }

    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// `name` was just used: to the front of the list, bounded. Returns the
    /// request that keeps the list on disk when it changed.
    fn touch_recent(&mut self, name: &str) -> Option<DeskRequest> {
        if self.recent.first().map(String::as_str) == Some(name) {
            return None;
        }
        self.recent.retain(|n| n != name);
        self.recent.insert(0, name.to_string());
        self.recent.truncate(RECENT_FILES);
        let mut text = self.recent.join("\n");
        text.push('\n');
        Some(DeskRequest::SaveRecent { text })
    }

    /// The kernel answers `Vitals` (GFX-072).
    pub fn vitals_loaded(&mut self, vitals: Vitals) {
        self.vitals = vitals;
        self.vitals_pending = false;
    }

    /// `1h 02m` / `3m 07s` from ticks at 100 Hz.
    pub fn uptime(ticks: u64) -> String {
        let secs = ticks / 100;
        if secs >= 3600 {
            alloc::format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
        } else {
            alloc::format!("{}m {:02}s", secs / 60, secs % 60)
        }
    }

    /// The Now card's lines (GFX-072).
    fn now_lines(&self, clock: &str) -> Vec<String> {
        let v = &self.vitals;
        let cards = self
            .windows
            .iter()
            .filter(|w| w.app != DeskApp::Welcome)
            .count();
        let spaces_used = self.space_counts().iter().filter(|c| **c > 0).count();
        let mut lines = alloc::vec![
            alloc::format!("Clock        {clock}"),
            alloc::format!("Up           {}", Self::uptime(v.uptime_ticks)),
        ];
        if v.heap_total_kib > 0 {
            lines.push(alloc::format!(
                "Memory       {} MiB of {} MiB in use",
                v.heap_used_kib / 1024,
                v.heap_total_kib / 1024
            ));
        }
        lines.push(alloc::format!(
            "CPUs         {} online of {}",
            v.cpus_online,
            v.cpus_total.max(v.cpus_online)
        ));
        lines.push(alloc::format!("Files        {}", v.files));
        lines.push(alloc::format!(
            "Cards        {cards} open on {spaces_used} space{}",
            if spaces_used == 1 { "" } else { "s" }
        ));
        lines.push(alloc::format!(
            "Look         {} + {}",
            self.look.theme,
            self.look.accent
        ));
        lines.push(alloc::format!(
            "Notices      {} kept",
            self.notice_log.len()
        ));
        lines
    }

    /// The shortcut sheet's lines (GFX-072): the desk's keys, then every
    /// palette row that has one -- generated, so it cannot go stale.
    fn shortcut_lines() -> Vec<String> {
        let mut lines: Vec<String> = DESK_KEYS
            .iter()
            .map(|(what, key)| alloc::format!("{what:<34} {key}"))
            .collect();
        lines.push(String::new());
        for action in PaletteAction::ALL.iter() {
            if !action.shortcut().is_empty() {
                lines.push(alloc::format!(
                    "{:<34} {}",
                    action.label(),
                    action.shortcut()
                ));
            }
        }
        lines
    }

    /// The kernel read `name` for the Files card `id` (GFX-071).
    pub fn preview_loaded(&mut self, id: ViewId, name: &str, text: &str) {
        if let Some(files) = self.window_mut(id).and_then(|w| w.files_mut()) {
            let lines: Vec<String> = text
                .lines()
                .take(PREVIEW_LINES)
                .map(|l| l.chars().take(120).collect())
                .collect();
            files.preview = Some((name.to_string(), lines));
            if files.preview_pending.as_deref() == Some(name) {
                files.preview_pending = None;
            }
        }
        // The Tasks card reads its whole document the same way (GFX-079).
        if let Some(AppState::Tasks(tasks)) = self.window_mut(id).map(|w| &mut w.state) {
            if name == TASKS_FILE {
                tasks.load(text);
            }
        }
        // And the Sketch its strokes (GFX-080).
        if let Some(AppState::Sketch(sketch)) = self.window_mut(id).map(|w| &mut w.state) {
            if name == SKETCH_FILE {
                sketch.load(text);
            }
        }
    }

    /// Every tick (GFX-060): documents that have sat still save
    /// themselves. The saves are quiet -- no notice card -- because a card
    /// for something the person did not ask for is noise. Files cards ask
    /// for the selected file's first lines when the selection has moved on
    /// from the preview they hold (GFX-071).
    pub fn tick(&mut self, now: u64) -> Vec<DeskRequest> {
        let mut requests = Vec::new();
        // Timers run on the desk's ticks; a countdown that is up is said
        // through a notice, so it is heard from any space (GFX-077).
        let mut done = Vec::new();
        for window in &mut self.windows {
            if let AppState::Timer(timer) = &mut window.state {
                if let Some(message) = timer.poll(now) {
                    done.push(message);
                }
            }
        }
        for message in done {
            self.notify(NoticeLevel::Info, message, now);
        }
        // A running timer is drawn ten times a second.
        let running = self
            .windows
            .iter()
            .any(|w| matches!(&w.state, AppState::Timer(t) if t.running()));
        if running && now.saturating_sub(self.timer_drawn) >= crate::timer::HZ / 10 {
            self.timer_drawn = now;
            requests.push(DeskRequest::Repaint);
        }
        // A save may have made a day's note: the Calendars list again.
        if self.notes_stale {
            self.notes_stale = false;
            for window in &self.windows {
                if matches!(window.state, AppState::Calendar(_)) {
                    requests.push(DeskRequest::ListFiles { id: window.id });
                }
            }
        }
        // A Now card asks once a second (GFX-072), and never twice at once.
        if self.windows.iter().any(|w| w.app == DeskApp::Now) {
            let due = !self.vitals_pending
                && self
                    .vitals_asked_at
                    .map(|asked| now.saturating_sub(asked) >= VITALS_EVERY)
                    .unwrap_or(true);
            if due {
                self.vitals_asked_at = Some(now);
                self.vitals_pending = true;
                requests.push(DeskRequest::Vitals);
            }
        }
        for window in &mut self.windows {
            if let AppState::Files(files) = &mut window.state {
                if files.list_only || files.bin {
                    continue;
                }
                let Some(want) = files.selected().map(|e| e.name.clone()) else {
                    continue;
                };
                let have = files.preview.as_ref().map(|(n, _)| n.as_str());
                if have != Some(want.as_str()) && files.preview_pending.as_deref() != Some(&want) {
                    files.preview_pending = Some(want.clone());
                    requests.push(DeskRequest::PreviewFile {
                        id: window.id,
                        name: want,
                    });
                }
            }
        }
        if self.welcome_dismissed {
            self.welcome_dismissed = false;
            requests.push(DeskRequest::Welcomed);
        }
        for window in &mut self.windows {
            if let AppState::Notepad(notepad) = &mut window.state {
                if let Some(effect) = notepad.autosave_due(now) {
                    requests.push(DeskRequest::Io {
                        id: window.id,
                        effect,
                    });
                    if !self.quiet_saves.contains(&window.id) {
                        self.quiet_saves.push(window.id);
                    }
                }
            }
            // The task list saves itself the same way (GFX-079).
            if let AppState::Tasks(tasks) = &mut window.state {
                if let Some(content) = tasks.save_due(now) {
                    requests.push(DeskRequest::Io {
                        id: window.id,
                        effect: NotepadEffect::Save {
                            path: TASKS_FILE.to_string(),
                            content,
                        },
                    });
                    if !self.quiet_saves.contains(&window.id) {
                        self.quiet_saves.push(window.id);
                    }
                }
            }
            // And the drawing (GFX-080).
            if let AppState::Sketch(sketch) = &mut window.state {
                if let Some(content) = sketch.save_due(now) {
                    requests.push(DeskRequest::Io {
                        id: window.id,
                        effect: NotepadEffect::Save {
                            path: SKETCH_FILE.to_string(),
                            content,
                        },
                    });
                    if !self.quiet_saves.contains(&window.id) {
                        self.quiet_saves.push(window.id);
                    }
                }
            }
        }
        requests
    }

    pub fn focus(&self) -> Option<ViewId> {
        self.focus
    }

    pub fn window(&self, id: ViewId) -> Option<&DeskWindow> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_mut(&mut self, id: ViewId) -> Option<&mut DeskWindow> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn focused_window(&self) -> Option<&DeskWindow> {
        self.focus.and_then(|id| self.window(id))
    }

    /// Whether a key typed now belongs to an app rather than the shell.
    /// Whether the console has a card open, and if so how many content rows
    /// it shows -- so the kernel can build a [`TerminalView`] that fits.
    pub fn terminal_rows(&self) -> Option<usize> {
        let window = self.windows.iter().find(|w| w.app == DeskApp::Terminal)?;
        Some(Self::card_rows(window.bounds))
    }

    pub fn set_pointer(&mut self, position: Option<(usize, usize)>) {
        self.pointer = position;
    }

    /// The area cards may occupy: below the top bar, above the dock.
    fn work_area(&self) -> RasterRect {
        let top = TOP_BAR_HEIGHT;
        let bottom = self.height.saturating_sub(DOCK_HEIGHT + DOCK_MARGIN * 2);
        RasterRect::new(0, top, self.width, bottom.saturating_sub(top))
    }

    fn card_rows(bounds: RasterRect) -> usize {
        DesktopWindow::card(
            ViewFrame::new(
                ViewId::new(),
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(Vec::new()),
                0,
            ),
            bounds,
        )
        .with_footer(Some(String::new()))
        .content_rows()
    }

    /// Open `app` in a new card, cascaded from the last, and focus it.
    pub fn launch(&mut self, app: DeskApp) -> ViewId {
        let (w, h) = app.size();
        let area = self.work_area();
        let w = w.min(area.width);
        let h = h.min(area.height);
        let step = (self.opened % 6) * CASCADE_STEP;
        let x =
            (area.x + area.width.saturating_sub(w) / 2 + step).min(area.right().saturating_sub(w));
        let y = (area.y + 16 + step).min(area.bottom().saturating_sub(h).max(area.y));
        let id = ViewId::new();
        let z = self.next_z;
        self.next_z += 1;
        self.opened += 1;
        self.windows.push(DeskWindow {
            id,
            app,
            bounds: RasterRect::new(x, y, w, h),
            z,
            restore: None,
            tucked: false,
            space: self.space,
            state: match app {
                DeskApp::Notepad => AppState::Notepad(Notepad::new()),
                DeskApp::Files => AppState::Files(FilesView::default()),
                DeskApp::Terminal => AppState::Terminal,
                DeskApp::Look => AppState::Look(LookView {
                    row: LookView::row_of(&self.look),
                }),
                DeskApp::Welcome => AppState::Welcome,
                DeskApp::Notices => AppState::Notices,
                DeskApp::Now => AppState::Now,
                DeskApp::Shortcuts => AppState::Shortcuts(0),
                DeskApp::Calculator => AppState::Calculator(Calculator::new()),
                DeskApp::Calendar => AppState::Calendar(CalendarView::new(self.today)),
                DeskApp::Timer => AppState::Timer(TimerView::new()),
                // Seeded from the clock and the launch count: no two games
                // alike, and the same game under test.
                DeskApp::Tiles => AppState::Tiles(Game::new(
                    self.last_tick.wrapping_mul(6_364_136_223_846_793_005) ^ self.opened as u64,
                )),
                DeskApp::Tasks => AppState::Tasks(TasksView::new()),
                DeskApp::Sketch => AppState::Sketch(SketchView::new()),
            },
        });
        self.focus = Some(id);
        id
    }

    /// The first boot (GFX-065): a Welcome card at the bottom left, above
    /// the dock, that does not take focus -- typing on the bare desk is
    /// still a search -- and does not use a cascade slot, so the first
    /// Notepad opens where it always does.
    pub fn show_welcome(&mut self) -> ViewId {
        if let Some(existing) = self.windows.iter().find(|w| w.app == DeskApp::Welcome) {
            return existing.id;
        }
        let focus_before = self.focus;
        let opened_before = self.opened;
        let id = self.launch(DeskApp::Welcome);
        self.opened = opened_before;
        self.focus = focus_before;
        let area = self.work_area();
        let (w, h) = WELCOME_SIZE;
        if let Some(window) = self.window_mut(id) {
            window.bounds = RasterRect::new(
                area.x + 24,
                area.bottom().saturating_sub(h + 12),
                w.min(area.width),
                h.min(area.height),
            );
        }
        id
    }

    /// Tuck `id` into the dock: hidden until its tile is clicked.
    pub fn tuck(&mut self, id: ViewId) {
        if let Some(window) = self.window_mut(id) {
            window.tucked = true;
        }
        if self.focus == Some(id) {
            self.focus = self.topmost_on_stage();
        }
    }

    /// Whether a card is drawn and takes focus: on this space, not tucked.
    fn on_stage(&self, window: &DeskWindow) -> bool {
        !window.tucked && window.space == self.space
    }

    /// The highest card on this space, for focus to fall back to.
    fn topmost_on_stage(&self) -> Option<ViewId> {
        self.windows
            .iter()
            .filter(|w| self.on_stage(w))
            .max_by_key(|w| w.z)
            .map(|w| w.id)
    }

    /// The space on screen.
    pub fn space(&self) -> usize {
        self.space
    }

    /// Remember a notice (GFX-069), newest first, bounded.
    fn log_notice(&mut self, now: u64, notice: ShellNotice) {
        self.notice_log.insert(0, (now, notice));
        self.notice_log.truncate(NOTICE_LOG);
        self.unseen_notices += 1;
    }

    /// The log, newest first, as `(tick, notice)`.
    pub fn notice_log(&self) -> &[(u64, ShellNotice)] {
        &self.notice_log
    }

    /// Notices that arrived since the Notices card was last looked at.
    pub fn unseen_notices(&self) -> usize {
        self.unseen_notices
    }

    /// `12s ago`, `3m ago`, `2h ago`, at 100 ticks a second.
    pub fn ago(now: u64, then: u64) -> String {
        let secs = now.saturating_sub(then) / 100;
        if secs < 60 {
            alloc::format!("{secs}s ago")
        } else if secs < 3600 {
            alloc::format!("{}m ago", secs / 60)
        } else {
            alloc::format!("{}h ago", secs / 3600)
        }
    }

    /// The bar's right-hand text: "3 new   12:34", or just the clock.
    fn bar_right(&self, clock: &str) -> String {
        if self.unseen_notices > 0 {
            alloc::format!("{} new   {clock}", self.unseen_notices)
        } else {
            clock.to_string()
        }
    }

    /// Whether top-bar text cell `column` is on the "N new" indicator.
    fn notices_at_column(&self, column: usize, clock_len: usize) -> bool {
        if self.unseen_notices == 0 {
            return false;
        }
        // The same right-alignment the compositor paints with: a 12px
        // margin, then the text; the indicator is its first word.
        let clock: String = core::iter::repeat('0').take(clock_len).collect();
        let total = self.bar_right(&clock).chars().count();
        let start = self.width.saturating_sub(12 + total * GLYPH_WIDTH) / GLYPH_WIDTH;
        let indicator = alloc::format!("{} new", self.unseen_notices)
            .chars()
            .count();
        (start..start + indicator).contains(&column)
    }

    /// Whether top-bar text cell `column` is on the clock, the last
    /// `clock_len` cells of the right-hand text.
    fn clock_at_column(&self, column: usize, clock_len: usize) -> bool {
        let clock: String = core::iter::repeat('0').take(clock_len).collect();
        let total = self.bar_right(&clock).chars().count();
        let start = self.width.saturating_sub(12 + total * GLYPH_WIDTH) / GLYPH_WIDTH;
        (start + total - clock_len..start + total).contains(&column)
    }

    /// Whether the overview is open.
    pub fn overview_open(&self) -> bool {
        self.overview.is_some()
    }

    /// Every card, in the order the overview shows them: this space first,
    /// highest on top, then the other spaces in order. Tucked cards are
    /// here too -- the overview is where a tucked card is found without
    /// remembering which tile it went into. The Welcome card is not a
    /// card to switch to.
    fn overview_order(&self) -> Vec<ViewId> {
        let mut cards: Vec<&DeskWindow> = self
            .windows
            .iter()
            .filter(|w| w.app != DeskApp::Welcome)
            .collect();
        let current = self.space;
        cards.sort_by(|a, b| {
            let space = |w: &DeskWindow| (w.space != current, w.space);
            space(a).cmp(&space(b)).then_with(|| b.z.cmp(&a.z))
        });
        cards.iter().map(|w| w.id).collect()
    }

    /// Open the overview on the next card (GFX-068). Nothing to show with
    /// no cards.
    pub fn open_overview(&mut self) -> bool {
        let count = self.overview_order().len();
        if count == 0 {
            return false;
        }
        self.palette = None;
        self.overview = Some(if count > 1 { 1 } else { 0 });
        true
    }

    /// Move the highlight by `delta`, wrapping.
    fn overview_step(&mut self, delta: isize) {
        let count = self.overview_order().len() as isize;
        if let (Some(index), true) = (self.overview, count > 0) {
            let next = (index as isize + delta).rem_euclid(count) as usize;
            self.overview = Some(next);
        }
    }

    /// Take the highlighted card: raised, untucked, its space shown.
    fn overview_pick(&mut self) -> bool {
        let Some(index) = self.overview.take() else {
            return false;
        };
        match self.overview_order().get(index).copied() {
            Some(id) => {
                self.raise(id);
                true
            }
            None => true,
        }
    }

    /// The overview's mini card for a window: the title, a few lines of
    /// what is in it, and which space it is on.
    fn overview_card(
        &self,
        window: &DeskWindow,
        bounds: RasterRect,
        highlighted: bool,
    ) -> DesktopWindow {
        let (title, lines) = match &window.state {
            AppState::Notepad(notepad) => (
                notepad.title(),
                notepad
                    .content()
                    .lines()
                    .take(3)
                    .map(|l| l.chars().take(32).collect::<String>())
                    .collect::<Vec<_>>(),
            ),
            AppState::Files(files) => (
                "Files".to_string(),
                alloc::vec![alloc::format!("{} files", files.entries.len())],
            ),
            AppState::Terminal => (
                "Terminal".to_string(),
                alloc::vec!["the console".to_string()],
            ),
            AppState::Look(_) => (
                "Look".to_string(),
                alloc::vec!["themes and accents".to_string()],
            ),
            AppState::Welcome => ("Welcome".to_string(), Vec::new()),
            AppState::Notices => (
                "Notices".to_string(),
                alloc::vec![alloc::format!("{} kept", self.notice_log.len())],
            ),
            AppState::Now => ("Now".to_string(), alloc::vec!["the machine".to_string()]),
            AppState::Shortcuts(_) => (
                "Shortcuts".to_string(),
                alloc::vec!["every key".to_string()],
            ),
            AppState::Calculator(calc) => ("Calculator".to_string(), alloc::vec![calc.shown()]),
            AppState::Calendar(calendar) => (
                "Calendar".to_string(),
                alloc::vec![calendar.selected.short()],
            ),
            AppState::Timer(timer) => (
                "Timer".to_string(),
                alloc::vec![crate::timer::format_ticks(timer.shown_ticks())],
            ),
            AppState::Tiles(game) => (
                "Tiles".to_string(),
                alloc::vec![alloc::format!("score {}", game.score())],
            ),
            AppState::Tasks(tasks) => (
                "Tasks".to_string(),
                alloc::vec![alloc::format!(
                    "{} to do",
                    tasks.tasks().iter().filter(|t| !t.done).count()
                )],
            ),
            AppState::Sketch(sketch) => (
                "Sketch".to_string(),
                alloc::vec![alloc::format!("{} strokes", sketch.strokes().len())],
            ),
        };
        let mut frame = ViewFrame::new(
            window.id,
            ViewKind::TextBuffer,
            0,
            ViewContent::text_buffer(lines),
            0,
        );
        frame.title = Some(title);
        let footer = alloc::format!(
            "space {}{}",
            window.space + 1,
            if window.tucked { "   tucked" } else { "" }
        );
        let mut card = DesktopWindow::card(frame, bounds)
            .with_z_index(usize::MAX / 2 - 2)
            .with_footer(Some(footer));
        card.closable = false;
        if highlighted {
            card = card.focused();
        }
        card
    }

    /// Show space `index` (GFX-067): focus goes to its highest card, or to
    /// nothing, and the palette closes. Returns whether anything changed.
    pub fn go_to_space(&mut self, index: usize) -> bool {
        if index >= SPACES || index == self.space {
            return false;
        }
        self.space = index;
        self.focus = self.topmost_on_stage();
        self.palette = None;
        self.drag = None;
        self.resize = None;
        self.text_select = None;
        true
    }

    /// Move the focused card to space `index` and follow it there.
    pub fn move_focused_to_space(&mut self, index: usize) -> bool {
        if index >= SPACES {
            return false;
        }
        let Some(id) = self.focus else {
            return false;
        };
        if let Some(window) = self.window_mut(id) {
            window.space = index;
        }
        self.space = index;
        self.raise(id);
        true
    }

    /// How many cards each space holds, for the strip.
    fn space_counts(&self) -> [usize; SPACES] {
        let mut counts = [0; SPACES];
        for window in &self.windows {
            if window.app != DeskApp::Welcome {
                counts[window.space.min(SPACES - 1)] += 1;
            }
        }
        counts
    }

    /// The top bar's centre text: `[1] 2 3 4`, with a dot after a space
    /// that holds cards, so the strip says where things are.
    fn space_strip(&self) -> String {
        let counts = self.space_counts();
        let mut strip = String::new();
        for index in 0..SPACES {
            if index > 0 {
                strip.push_str("  ");
            }
            let mark = if counts[index] > 0 { "." } else { " " };
            if index == self.space {
                strip.push_str(&alloc::format!("[{}]{mark}", index + 1));
            } else {
                strip.push_str(&alloc::format!(" {} {mark}", index + 1));
            }
        }
        strip
    }

    /// Which space a click on top-bar text cell `column` means, if any.
    fn space_at_column(&self, column: usize) -> Option<usize> {
        let strip = self.space_strip();
        let start = services_gui_host::top_bar_centre_column(self.width, strip.chars().count());
        let offset = column.checked_sub(start)?;
        // Each space is 4 cells wide, with two between.
        let cell = offset % 6;
        let index = offset / 6;
        (cell < 4 && index < SPACES).then_some(index)
    }

    fn untuck(&mut self, id: ViewId) {
        if let Some(window) = self.window_mut(id) {
            window.tucked = false;
        }
        self.raise(id);
    }

    /// Back to the size a window had before it was snapped, if it was.
    fn unsnap(&mut self, id: ViewId) -> bool {
        let area = self.work_area();
        if let Some(window) = self.window_mut(id) {
            if let Some(restore) = window.restore.take() {
                window.bounds = RasterRect::new(
                    restore.x.min(area.right().saturating_sub(restore.width)),
                    restore.y.max(area.y),
                    restore.width.min(area.width),
                    restore.height.min(area.height),
                );
                return true;
            }
        }
        false
    }

    /// Snap the focused window from the palette.
    fn snap_focused(&mut self, how: PaletteAction) {
        let area = self.work_area();
        let Some(id) = self.focus else {
            return;
        };
        let target = match how {
            PaletteAction::SnapLeft => RasterRect::new(area.x, area.y, area.width / 2, area.height),
            PaletteAction::SnapRight => RasterRect::new(
                area.x + area.width / 2,
                area.y,
                area.width - area.width / 2,
                area.height,
            ),
            _ => area,
        };
        if let Some(window) = self.window_mut(id) {
            if window.restore.is_none() {
                window.restore = Some(window.bounds);
            }
            window.bounds = target;
        }
    }

    /// The document in card `id` in a second card as well (GFX-074): the
    /// new card is another view of the same text -- its own caret, the
    /// same document -- and the two are snapped side by side, the original
    /// left, the new one right and focused. Returns whether it happened.
    fn split(&mut self, id: ViewId) -> bool {
        let Some(view) = self.window(id).and_then(|w| w.notepad()).map(|n| n.share()) else {
            return false;
        };
        let twin = self.launch(DeskApp::Notepad);
        if let Some(window) = self.window_mut(twin) {
            window.state = AppState::Notepad(view);
        }
        self.focus = Some(id);
        self.snap_focused(PaletteAction::SnapLeft);
        self.focus = Some(twin);
        self.snap_focused(PaletteAction::SnapRight);
        true
    }

    /// Run a palette action. Returns what the kernel must do, if anything.
    fn run_action(&mut self, action: PaletteAction) -> Option<DeskRequest> {
        match action {
            PaletteAction::NewNotepad => {
                self.launch(DeskApp::Notepad);
                None
            }
            PaletteAction::NewTerminal => {
                self.launch(DeskApp::Terminal);
                None
            }
            PaletteAction::OpenFiles => {
                let id = self.launch(DeskApp::Files);
                Some(DeskRequest::ListFiles { id })
            }
            PaletteAction::Save => self.forward_to_notepad(crate::notepad::CTRL_S),
            PaletteAction::SaveAs => self.forward_to_notepad(crate::notepad::CTRL_SHIFT_S),
            PaletteAction::Open => self.forward_to_notepad(crate::notepad::CTRL_O),
            PaletteAction::SelectAll => self.forward_to_notepad(crate::notepad::CTRL_A),
            PaletteAction::Copy => self.forward_to_notepad(crate::notepad::CTRL_C),
            PaletteAction::Cut => self.forward_to_notepad(crate::notepad::CTRL_X),
            PaletteAction::Paste => self.forward_to_notepad(crate::notepad::CTRL_V),
            PaletteAction::Find => self.forward_to_notepad(crate::notepad::CTRL_F),
            PaletteAction::History => self.forward_to_notepad(crate::notepad::CTRL_Y),
            PaletteAction::Split => {
                if let Some(id) = self.focus {
                    self.split(id);
                }
                None
            }
            PaletteAction::Space(index) => {
                self.go_to_space(index);
                None
            }
            PaletteAction::MoveToSpace(index) => {
                self.move_focused_to_space(index);
                None
            }
            PaletteAction::Overview => {
                self.open_overview();
                None
            }
            PaletteAction::Calculator => {
                self.open_or_raise(DeskApp::Calculator);
                None
            }
            PaletteAction::Timer => {
                self.open_or_raise(DeskApp::Timer);
                None
            }
            PaletteAction::Tiles => {
                self.open_or_raise(DeskApp::Tiles);
                None
            }
            PaletteAction::Tasks => {
                let had = self.windows.iter().any(|w| w.app == DeskApp::Tasks);
                let id = self.open_or_raise(DeskApp::Tasks);
                if had {
                    None
                } else {
                    DeskApp::Tasks.launch_request(id)
                }
            }
            PaletteAction::Sketch => {
                let had = self.windows.iter().any(|w| w.app == DeskApp::Sketch);
                let id = self.open_or_raise(DeskApp::Sketch);
                if had {
                    None
                } else {
                    DeskApp::Sketch.launch_request(id)
                }
            }
            PaletteAction::Calendar => {
                let had = self.windows.iter().any(|w| w.app == DeskApp::Calendar);
                let id = self.open_or_raise(DeskApp::Calendar);
                (!had).then_some(DeskRequest::ListFiles { id })
            }
            PaletteAction::Notices => {
                self.open_or_raise(DeskApp::Notices);
                None
            }
            PaletteAction::Now => {
                self.open_or_raise(DeskApp::Now);
                None
            }
            PaletteAction::Shortcuts => {
                self.open_or_raise(DeskApp::Shortcuts);
                None
            }
            PaletteAction::CloseWindow => {
                if let Some(id) = self.focus {
                    self.close(id);
                }
                None
            }
            PaletteAction::NextWindow => {
                self.cycle_focus();
                None
            }
            PaletteAction::SnapLeft | PaletteAction::SnapRight | PaletteAction::Maximise => {
                self.snap_focused(action);
                None
            }
            PaletteAction::Tuck => {
                if let Some(id) = self.focus {
                    self.tuck(id);
                }
                None
            }
            PaletteAction::TextConsole => Some(DeskRequest::TextConsole),
            PaletteAction::ToggleTheme => {
                self.open_or_raise(DeskApp::Look);
                None
            }
        }
    }

    fn forward_to_notepad(&mut self, byte: u8) -> Option<DeskRequest> {
        let id = self.focus?;
        let (request, _) = self.handle_app_key(id, byte);
        request
    }

    /// A content search for the palette's query, when it is long enough.
    fn search_request(palette: &Palette) -> Option<DeskRequest> {
        (palette.query.trim().len() >= SEARCH_MIN_CHARS).then(|| DeskRequest::SearchFiles {
            query: palette.query.trim().to_string(),
        })
    }

    /// The kernel found these lines for `query` (GFX-061). Shown only while
    /// the palette still asks the same thing.
    pub fn search_results(&mut self, query: &str, hits: Vec<(String, String)>) {
        if let Some(palette) = self.palette.as_mut() {
            palette.hits = hits;
            palette.hits_for = query.to_string();
        }
    }

    /// A key while the palette is open.
    fn handle_palette_key(&mut self, byte: u8) -> (Option<DeskRequest>, bool) {
        let has_window = self.focused_window().is_some();
        let has_notepad = self.focused_window().and_then(|w| w.notepad()).is_some();
        let recent = self.recent.clone();
        let clips = self.clipboard_history.clone();
        let query = self
            .palette
            .as_ref()
            .map(|p| p.query.clone())
            .unwrap_or_default();
        let Some(palette) = self.palette.as_mut() else {
            return (None, false);
        };
        match byte {
            crate::notepad::ESC | KEY_CTRL_SPACE | 0x10 => {
                self.palette = None;
                (None, true)
            }
            crate::notepad::KEY_UP => {
                palette.selection = palette.selection.saturating_sub(1);
                (None, true)
            }
            crate::notepad::KEY_DOWN => {
                let count = palette
                    .matches(has_window, has_notepad, &recent, &clips)
                    .len();
                palette.selection = (palette.selection + 1).min(count.saturating_sub(1));
                (None, true)
            }
            crate::notepad::BACKSPACE => {
                palette.query.pop();
                palette.selection = 0;
                (Self::search_request(palette), true)
            }
            b'\n' | b'\r' => {
                let matches = palette.matches(has_window, has_notepad, &recent, &clips);
                let chosen = matches.get(palette.selection).cloned();
                self.palette = None;
                match chosen {
                    Some(PaletteRow::Action(action)) => (self.run_action(action), true),
                    Some(PaletteRow::Recent(name)) => {
                        let id = self.launch(DeskApp::Notepad);
                        (
                            Some(DeskRequest::Io {
                                id,
                                effect: NotepadEffect::Open { path: name },
                            }),
                            true,
                        )
                    }
                    Some(PaletteRow::Paste(text)) => {
                        // Paste it: it becomes the clipboard, then Ctrl+V.
                        self.remember_copy(text.clone());
                        self.clipboard = text;
                        match self.focus {
                            Some(id) => self.handle_app_key(id, crate::notepad::CTRL_V),
                            None => (None, true),
                        }
                    }
                    Some(PaletteRow::Hit { file, .. }) => {
                        // Open the file and land on the line: the Notepad
                        // finds the query once the content arrives.
                        let id = self.launch(DeskApp::Notepad);
                        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
                            notepad.find_on_open(&query);
                        }
                        (
                            Some(DeskRequest::Io {
                                id,
                                effect: NotepadEffect::Open { path: file },
                            }),
                            true,
                        )
                    }
                    None => (None, true),
                }
            }
            0x20..=0x7E => {
                if palette.query.len() < 40 {
                    palette.query.push(byte as char);
                    palette.selection = 0;
                }
                (Self::search_request(palette), true)
            }
            _ => (None, false),
        }
    }

    /// Bring `id` to the front and give it focus.
    pub fn raise(&mut self, id: ViewId) {
        let z = self.next_z;
        let Some(window) = self.window_mut(id) else {
            return;
        };
        window.z = z;
        window.tucked = false;
        let space = window.space;
        self.next_z += 1;
        self.focus = Some(id);
        // A card on another space brings you to it (GFX-067): the dock
        // tile, a palette row, Ctrl+Tab never leave you looking at
        // nothing.
        if space != self.space {
            self.space = space;
            self.palette = None;
        }
    }

    pub fn close(&mut self, id: ViewId) {
        if self.window(id).map(|w| w.app) == Some(DeskApp::Look) {
            self.look_preview = None;
        }
        if self.window(id).map(|w| w.app) == Some(DeskApp::Welcome) {
            self.welcome_dismissed = true;
        }
        self.windows.retain(|w| w.id != id);
        if self.drag.map(|d| d.id) == Some(id) {
            self.drag = None;
        }
        if self.resize.map(|r| r.id) == Some(id) {
            self.resize = None;
        }
        if self.focus == Some(id) {
            // The top-most remaining window takes focus.
            self.focus = self.topmost_on_stage();
        }
    }

    /// Focus the next window in z order (Ctrl+Tab). Tucked windows are
    /// skipped; the dock is how they come back.
    pub fn cycle_focus(&mut self) -> bool {
        if self.windows.iter().filter(|w| self.on_stage(w)).count() < 2 {
            return false;
        }
        // The lowest window comes to the top, so repeated presses walk the
        // whole stack.
        let lowest = self
            .windows
            .iter()
            .filter(|w| self.on_stage(w))
            .min_by_key(|w| w.z)
            .map(|w| w.id);
        if let Some(id) = lowest {
            self.raise(id);
        }
        true
    }

    /// A dock tile was pressed: focus the app's window if it has one, else
    /// launch it.
    /// Bring an app's window to the front, or open one.
    pub fn open_or_raise(&mut self, app: DeskApp) -> ViewId {
        let existing = self
            .windows
            .iter()
            .filter(|w| w.app == app)
            .max_by_key(|w| w.z)
            .map(|w| w.id);
        match existing {
            Some(id) => {
                self.raise(id);
                id
            }
            None => self.launch(app),
        }
    }

    pub fn activate_dock_tile(&mut self, index: usize) -> Option<DeskRequest> {
        let app = DeskApp::ALL.get(index).copied()?;
        // A tucked window of this app comes back first.
        let tucked = self
            .windows
            .iter()
            .filter(|w| w.app == app && w.tucked)
            .max_by_key(|w| w.z)
            .map(|w| w.id);
        if let Some(id) = tucked {
            self.untuck(id);
            return None;
        }
        let existing = self
            .windows
            .iter()
            .filter(|w| w.app == app)
            .max_by_key(|w| w.z)
            .map(|w| w.id);
        match existing {
            Some(id) => {
                self.raise(id);
                None
            }
            None => {
                let id = self.launch(app);
                app.launch_request(id)
            }
        }
    }

    /// Snap a window whose drag ended at the desk's edge (GFX-052): left or
    /// right half, or the whole work area from the top edge.
    fn snap_if_at_edge(&mut self, id: ViewId, px: usize, py: usize) -> bool {
        let area = self.work_area();
        let target = if px < SNAP_MARGIN {
            Some(RasterRect::new(area.x, area.y, area.width / 2, area.height))
        } else if px + SNAP_MARGIN >= self.width {
            Some(RasterRect::new(
                area.x + area.width / 2,
                area.y,
                area.width - area.width / 2,
                area.height,
            ))
        } else if py <= area.y + SNAP_MARGIN {
            Some(area)
        } else {
            None
        };
        let Some(target) = target else {
            return false;
        };
        if let Some(window) = self.window_mut(id) {
            if window.restore.is_none() {
                window.restore = Some(window.bounds);
            }
            window.bounds = target;
            return true;
        }
        false
    }

    /// Apply routed pointer deliveries. Returns whether the screen changed.
    pub fn handle_deliveries(&mut self, deliveries: &[Delivery]) -> bool {
        let (_, changed) = self.handle_deliveries_with_requests(deliveries);
        changed
    }

    /// As `handle_deliveries`, also returning what the kernel must do.
    pub fn handle_deliveries_with_requests(
        &mut self,
        deliveries: &[Delivery],
    ) -> (Vec<DeskRequest>, bool) {
        let mut requests = Vec::new();
        let mut changed = false;
        for delivery in deliveries {
            match delivery {
                Delivery::FocusChanged { current, .. } => {
                    if let Some(id) = current {
                        if self.window(*id).is_some() {
                            self.raise(*id);
                            changed = true;
                        }
                    }
                }
                Delivery::Leave { target } if *target == self.dock_id => {
                    if self.hovered_tile.take().is_some() {
                        changed = true;
                    }
                }
                Delivery::Pointer { target, event, hit } => {
                    let press = event.is_press(PointerButton::Primary);
                    let release = event.is_release(PointerButton::Primary);
                    let region = hit.map(|h| h.region);
                    let px = event.position.x.max(0) as usize;
                    let py = event.position.y.max(0) as usize;

                    if self.overview.is_some() && press {
                        // A mini card is the window it stands for.
                        let order = self.overview_order();
                        match order.iter().position(|id| id == target) {
                            Some(index) => {
                                self.overview = Some(index);
                                self.overview_pick();
                            }
                            None => self.overview = None,
                        }
                        changed = true;
                        continue;
                    }
                    if *target == self.dock_id {
                        let tile = match region {
                            Some(HitRegion::DockTile { index }) => Some(index),
                            _ => None,
                        };
                        if self.hovered_tile != tile {
                            self.hovered_tile = tile;
                            changed = true;
                        }
                        if let (true, Some(index)) = (press, tile) {
                            if let Some(request) = self.activate_dock_tile(index) {
                                requests.push(request);
                            }
                            changed = true;
                        }
                        continue;
                    }
                    // A row of the palette runs on a click (GFX-063).
                    if press && *target == self.palette_id {
                        if let (Some(HitRegion::Content { line, .. }), Some(palette)) =
                            (region, self.palette.as_mut())
                        {
                            palette.selection = line;
                            let (request, _) = self.handle_palette_key(b'\n');
                            requests.extend(request);
                        }
                        changed = true;
                        continue;
                    }
                    // A click anywhere while the palette is open closes it.
                    if press && self.palette.is_some() && *target != self.palette_id {
                        self.palette = None;
                        changed = true;
                    }
                    // The bar is the desk's own handle: a click opens the
                    // palette, the same as Ctrl+Space (GFX-063).
                    if press && *target == self.top_bar_id {
                        let column = match region {
                            Some(HitRegion::Content { column, .. }) => Some(column),
                            _ => None,
                        };
                        // The clock is five cells wide ("12:34"); the indicator
                        // sits just before it.
                        if column
                            .map(|c| self.notices_at_column(c, 5))
                            .unwrap_or(false)
                        {
                            self.open_or_raise(DeskApp::Notices);
                        } else if column.map(|c| self.clock_at_column(c, 5)).unwrap_or(false) {
                            // The clock is the way into Now (GFX-072).
                            self.open_or_raise(DeskApp::Now);
                        } else {
                            match column.and_then(|c| self.space_at_column(c)) {
                                Some(space) => {
                                    self.go_to_space(space);
                                }
                                None => self.palette = Some(Palette::default()),
                            }
                        }
                        changed = true;
                        continue;
                    }
                    // A notice goes away when clicked (GFX-063).
                    if press && self.notice_ids.contains(target) {
                        self.notices.clear();
                        let shell = core::mem::take(&mut self.last_shell_notices);
                        self.dismissed_shell = shell;
                        changed = true;
                        continue;
                    }
                    if self.window(*target).is_none() {
                        continue;
                    }

                    match (press, release, region, &event.kind) {
                        (true, _, Some(HitRegion::Close), _) => {
                            self.close(*target);
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Action { index }), _) => {
                            self.raise(*target);
                            self.focus = Some(*target);
                            let (request, _) = self.card_action(*target, index);
                            requests.extend(request);
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Resize), _) => {
                            self.raise(*target);
                            if let Some(window) = self.window(*target) {
                                self.resize = Some(Resize {
                                    id: *target,
                                    grab_dx: window.bounds.right().saturating_sub(px),
                                    grab_dy: window.bounds.bottom().saturating_sub(py),
                                });
                            }
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Header), _) => {
                            self.raise(*target);
                            if let Some(window) = self.window_mut(*target) {
                                // A snapped card un-snaps under the pointer.
                                if let Some(restore) = window.restore.take() {
                                    let half = restore.width / 2;
                                    window.bounds = RasterRect::new(
                                        px.saturating_sub(half),
                                        window.bounds.y,
                                        restore.width,
                                        restore.height,
                                    );
                                }
                                self.drag = Some(Drag {
                                    id: *target,
                                    grab_x: px.saturating_sub(window.bounds.x),
                                    grab_y: py.saturating_sub(window.bounds.y),
                                });
                            }
                            changed = true;
                        }
                        (true, _, Some(HitRegion::Content { line, column }), _) => {
                            self.raise(*target);
                            if let Some(notepad) =
                                self.window_mut(*target).and_then(|w| w.notepad_mut())
                            {
                                notepad.place_cursor(line, column);
                                self.text_select = Some(*target);
                            }
                            let mut open = false;
                            if let Some(files) =
                                self.window_mut(*target).and_then(|w| w.files_mut())
                            {
                                let row = files.scroll + line;
                                if row < files.visible().len() {
                                    open = files.selection == row;
                                    files.selection = row;
                                }
                            }
                            if open {
                                requests.extend(self.open_files_selection(*target));
                            }
                            // Calculator: the keys are in the content (GFX-075).
                            // Calculator: real keys, hit by pixel (GFX-081).
                            if let Some((x, y)) = self.canvas_point(*target, px, py) {
                                let bounds = self.window(*target).map(|w| w.bounds);
                                let palette = crate::widgets::Palette::from_theme(&self.theme());
                                if let (Some(bounds), Some(AppState::Calculator(calc))) =
                                    (bounds, self.window_mut(*target).map(|w| &mut w.state))
                                {
                                    let (w, h) = Self::canvas_size(bounds);
                                    if let Some(key) = calc.ui(w, h, palette, None).hit(x, y) {
                                        calc.press(key as char);
                                    }
                                }
                            }
                            // Calendar: a day cell or a month button, by
                            // pixel (GFX-083); the hit is a key the card answers.
                            let mut effect = CalendarEffect::None;
                            if let Some((x, y)) = self.canvas_point(*target, px, py) {
                                let bounds = self.window(*target).map(|w| w.bounds);
                                let palette = crate::widgets::Palette::from_theme(&self.theme());
                                if let (Some(bounds), Some(AppState::Calendar(calendar))) =
                                    (bounds, self.window_mut(*target).map(|w| &mut w.state))
                                {
                                    let (w, h) = Self::canvas_size(bounds);
                                    if let Some(key) = calendar.ui(w, h, palette, None).hit(x, y) {
                                        effect = calendar.handle_byte(key);
                                    }
                                }
                            }
                            if let CalendarEffect::OpenNote { name, exists } = effect {
                                requests.extend(self.open_note(name, exists));
                            }
                            // Timer and Tiles: real buttons, hit by pixel
                            // (GFX-082); the hit is the key the button is.
                            if let Some((x, y)) = self.canvas_point(*target, px, py) {
                                let bounds = self.window(*target).map(|w| w.bounds);
                                let palette = crate::widgets::Palette::from_theme(&self.theme());
                                if let Some(bounds) = bounds {
                                    let (w, h) = Self::canvas_size(bounds);
                                    match self.window_mut(*target).map(|w| &mut w.state) {
                                        Some(AppState::Timer(timer)) => {
                                            if let Some(key) =
                                                timer.ui(w, h, palette, None).hit(x, y)
                                            {
                                                timer.handle_byte(key);
                                            }
                                        }
                                        Some(AppState::Tiles(game)) => {
                                            if let Some(key) =
                                                game.ui(w, h, palette, None).hit(x, y)
                                            {
                                                game.handle_byte(key);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            // Tasks: a row or a button, by pixel (GFX-083).
                            if let Some((x, y)) = self.canvas_point(*target, px, py) {
                                let bounds = self.window(*target).map(|w| w.bounds);
                                let palette = crate::widgets::Palette::from_theme(&self.theme());
                                if let (Some(bounds), Some(AppState::Tasks(tasks))) =
                                    (bounds, self.window_mut(*target).map(|w| &mut w.state))
                                {
                                    let (w, h) = Self::canvas_size(bounds);
                                    if let Some(key) = tasks.ui(w, h, palette, None).hit(x, y) {
                                        tasks.handle_byte(key);
                                    }
                                }
                            }
                            // Sketch: a stroke begins where the pointer
                            // pressed, in the canvas's own pixels (GFX-080).
                            if let Some((x, y)) = self.canvas_point(*target, px, py) {
                                if let Some(AppState::Sketch(sketch)) =
                                    self.window_mut(*target).map(|w| &mut w.state)
                                {
                                    sketch.begin(x, y);
                                    self.sketching = Some(*target);
                                }
                            }
                            // Look: a click on a row previews it; a click on
                            // the row already under the highlight keeps it.
                            let look_row = self
                                .window(*target)
                                .filter(|w| w.app == DeskApp::Look)
                                .and_then(|_| LookView::row_of_line(line));
                            if let Some(row) = look_row {
                                let current = self
                                    .look_preview
                                    .clone()
                                    .unwrap_or_else(|| self.look.clone());
                                let mut keep = false;
                                if let Some(look) =
                                    self.window_mut(*target).and_then(|w| w.look_mut())
                                {
                                    keep = look.row == row;
                                    look.row = row;
                                    if !keep {
                                        self.look_preview = Some(look.choice_at(&current));
                                    }
                                }
                                if keep {
                                    let (request, _) = self.handle_app_key(*target, b'\n');
                                    requests.extend(request);
                                }
                            }
                            changed = true;
                        }
                        (true, _, _, _) => {
                            self.raise(*target);
                            changed = true;
                        }
                        (_, _, _, PointerEventKind::Wheel { dy, .. }) => {
                            if let Some(notepad) =
                                self.window_mut(*target).and_then(|w| w.notepad_mut())
                            {
                                notepad.scroll_by(-(*dy) * 3);
                                changed = true;
                            } else if self.window(*target).map(|w| w.app) == Some(DeskApp::Terminal)
                            {
                                // The console's scrollback is the
                                // workspace's; ask for it.
                                requests.push(DeskRequest::TerminalScroll { notches: *dy * 3 });
                                changed = true;
                            } else if let Some(AppState::Shortcuts(scroll)) =
                                self.window_mut(*target).map(|w| &mut w.state)
                            {
                                let max = Self::shortcut_lines().len().saturating_sub(1);
                                *scroll =
                                    (*scroll as i64 - *dy as i64 * 3).clamp(0, max as i64) as usize;
                                changed = true;
                            }
                        }
                        (_, _, _, PointerEventKind::Move { .. }) => {
                            if self.sketching == Some(*target) {
                                if let Some((x, y)) = self.canvas_point(*target, px, py) {
                                    if let Some(AppState::Sketch(sketch)) =
                                        self.window_mut(*target).map(|w| &mut w.state)
                                    {
                                        changed |= sketch.extend(x, y);
                                    }
                                }
                            } else if self.text_select == Some(*target) {
                                if let Some(HitRegion::Content { line, column }) = region {
                                    if let Some(notepad) =
                                        self.window_mut(*target).and_then(|w| w.notepad_mut())
                                    {
                                        notepad.extend_selection_to(line, column);
                                        changed = true;
                                    }
                                }
                            } else if let Some(drag) = self.drag.filter(|d| d.id == *target) {
                                let (width, height) = (self.width, self.height);
                                if let Some(window) = self.window_mut(*target) {
                                    let max_x = width.saturating_sub(window.bounds.width);
                                    let max_y = height.saturating_sub(window.bounds.height);
                                    window.bounds.x = px.saturating_sub(drag.grab_x).min(max_x);
                                    window.bounds.y = py
                                        .saturating_sub(drag.grab_y)
                                        .max(TOP_BAR_HEIGHT)
                                        .min(max_y);
                                    changed = true;
                                }
                            } else if let Some(resize) = self.resize.filter(|r| r.id == *target) {
                                let (width, height) = (self.width, self.height);
                                if let Some(window) = self.window_mut(*target) {
                                    let (min_w, min_h) = MIN_CARD_SIZE;
                                    window.restore = None;
                                    window.bounds.width = (px + resize.grab_dx)
                                        .saturating_sub(window.bounds.x)
                                        .max(min_w)
                                        .min(width.saturating_sub(window.bounds.x));
                                    window.bounds.height = (py + resize.grab_dy)
                                        .saturating_sub(window.bounds.y)
                                        .max(min_h)
                                        .min(height.saturating_sub(window.bounds.y));
                                    changed = true;
                                }
                            }
                        }
                        (_, true, _, _) => {
                            if self.drag.map(|d| d.id) == Some(*target) {
                                self.drag = None;
                                // Dropped on the dock: tucked away (GFX-053).
                                let dock_top =
                                    self.height.saturating_sub(DOCK_HEIGHT + DOCK_MARGIN);
                                if py >= dock_top {
                                    self.tuck(*target);
                                    changed = true;
                                } else {
                                    changed |= self.snap_if_at_edge(*target, px, py);
                                }
                            }
                            if self.resize.map(|r| r.id) == Some(*target) {
                                self.resize = None;
                            }
                            if self.text_select == Some(*target) {
                                self.text_select = None;
                            }
                            if self.sketching == Some(*target) {
                                self.sketching = None;
                                if let Some(AppState::Sketch(sketch)) =
                                    self.window_mut(*target).map(|w| &mut w.state)
                                {
                                    sketch.end();
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Delivery::Capture { transition, .. } => {
                    if matches!(transition, input_types::PointerCapture::Lost) {
                        self.drag = None;
                        self.resize = None;
                    }
                }
                _ => {}
            }
        }
        (requests, changed)
    }

    /// A key for the focused app. Returns what the kernel must do, if
    /// anything, and whether the screen changed.
    pub fn handle_key(&mut self, byte: u8) -> (Option<DeskRequest>, bool) {
        // The palette first: it is modal while open, and Ctrl+Space or
        // Ctrl+P opens it from anywhere.
        if self.palette.is_some() {
            return self.handle_palette_key(byte);
        }
        if byte == KEY_CTRL_SPACE || byte == 0x10 {
            self.palette = Some(Palette::default());
            return (None, true);
        }
        // Launching is global. The first build only answered Ctrl+T with
        // nothing focused, so with a Notepad open the keystroke fell into
        // the Notepad and did nothing -- a shortcut that only works when
        // there is nothing to use it on. Ctrl+N stays the Notepad's own
        // "new document" while a Notepad is focused; from a Terminal, or
        // from the bare desk, it opens one.
        // The overview (GFX-068) owns the keys while it is open: Ctrl+Tab
        // or Right/Down highlights the next card, Left/Up the previous,
        // letting go of Ctrl or Enter takes it, Esc leaves things as they
        // were. Anything else is swallowed -- there is no card under the
        // keys to type into.
        if self.overview.is_some() {
            return match byte {
                KEY_CTRL_TAB | crate::notepad::KEY_RIGHT | crate::notepad::KEY_DOWN => {
                    self.overview_step(1);
                    (None, true)
                }
                crate::notepad::KEY_LEFT | crate::notepad::KEY_UP => {
                    self.overview_step(-1);
                    (None, true)
                }
                crate::notepad::KEY_CTRL_RELEASED | b'\n' | b'\r' => (None, self.overview_pick()),
                crate::notepad::ESC => {
                    self.overview = None;
                    (None, true)
                }
                _ => (None, false),
            };
        }
        if byte == crate::notepad::KEY_CTRL_RELEASED {
            return (None, false);
        }
        if byte == KEY_CTRL_TAB {
            return (None, self.open_overview());
        }
        // The spaces (GFX-067).
        if (crate::notepad::KEY_CTRL_1..crate::notepad::KEY_CTRL_1 + SPACES as u8).contains(&byte) {
            return (
                None,
                self.go_to_space((byte - crate::notepad::KEY_CTRL_1) as usize),
            );
        }
        if (crate::notepad::KEY_CTRL_SHIFT_1..crate::notepad::KEY_CTRL_SHIFT_1 + SPACES as u8)
            .contains(&byte)
        {
            return (
                None,
                self.move_focused_to_space((byte - crate::notepad::KEY_CTRL_SHIFT_1) as usize),
            );
        }
        // The window keys (GFX-062): Ctrl+Left/Right snap to a half,
        // Ctrl+Up fills the desk, Ctrl+Down goes back to the size before
        // any of those -- the keyboard's version of dragging to an edge.
        if let Some(how) = match byte {
            crate::notepad::KEY_CTRL_LEFT => Some(Some(PaletteAction::SnapLeft)),
            crate::notepad::KEY_CTRL_RIGHT => Some(Some(PaletteAction::SnapRight)),
            crate::notepad::KEY_CTRL_UP => Some(Some(PaletteAction::Maximise)),
            crate::notepad::KEY_CTRL_DOWN => Some(None),
            _ => None,
        } {
            let Some(id) = self.focus else {
                return (None, false);
            };
            let changed = match how {
                Some(how) => {
                    self.snap_focused(how);
                    true
                }
                None => self.unsnap(id),
            };
            return (None, changed);
        }
        if byte == crate::notepad::CTRL_T {
            self.launch(DeskApp::Terminal);
            return (None, true);
        }
        let Some(id) = self.focus else {
            // The bare desk. Ctrl+N opens a Notepad, and a printable key
            // opens the palette with that key already typed: the desk with
            // nothing on it is a search box, not a place keys vanish into.
            // (They used to: the kernel sent bare-desk keys through a second
            // entry point that knew three shortcuts and nothing else, so the
            // first Ctrl+Space of a session went to the invisible console.)
            if byte == crate::notepad::CTRL_N {
                self.launch(DeskApp::Notepad);
                return (None, true);
            }
            if (0x20..0x7f).contains(&byte) {
                self.palette = Some(Palette {
                    query: String::from(byte as char),
                    ..Palette::default()
                });
                return (None, true);
            }
            return (None, false);
        };
        if byte == crate::notepad::CTRL_N
            && self.window(id).map(|w| w.app) == Some(DeskApp::Terminal)
        {
            self.launch(DeskApp::Notepad);
            return (None, true);
        }
        self.handle_app_key(id, byte)
    }

    /// Open the Files card's selected entry in a fresh Notepad; the kernel
    /// reads the file and reports back to that window. Enter does this, and
    /// so does a click on the row that is already selected -- one click
    /// selects, the next opens, and there is no double-click clock to beat.
    fn open_files_selection(&mut self, id: ViewId) -> Option<DeskRequest> {
        let files = self.window_mut(id).and_then(|w| w.files_mut())?;
        let name = files.selected().map(|e| e.name.clone())?;
        let notepad_id = self.launch(DeskApp::Notepad);
        Some(DeskRequest::Io {
            id: notepad_id,
            effect: NotepadEffect::Open { path: name },
        })
    }

    /// A key for window `id`'s app.
    fn handle_app_key(&mut self, id: ViewId, byte: u8) -> (Option<DeskRequest>, bool) {
        let clipboard = self.clipboard.clone();
        let current_look = self
            .look_preview
            .clone()
            .unwrap_or_else(|| self.look.clone());
        let kept_row = LookView::row_of(&self.look);
        // Split is the desk's, not the Notepad's: it makes a card.
        if byte == crate::notepad::CTRL_D && self.window(id).and_then(|w| w.notepad()).is_some() {
            return (None, self.split(id));
        }
        let Some(window) = self.window_mut(id) else {
            return (None, false);
        };
        match &mut window.state {
            AppState::Terminal => {
                if byte == crate::notepad::CTRL_W {
                    self.close(id);
                    return (None, true);
                }
                if byte == crate::notepad::KEY_PAGE_UP {
                    return (Some(DeskRequest::TerminalScroll { notches: 10 }), true);
                }
                if byte == crate::notepad::KEY_PAGE_DOWN {
                    return (Some(DeskRequest::TerminalScroll { notches: -10 }), true);
                }
                (Some(DeskRequest::Terminal(byte)), true)
            }
            AppState::Files(files) => {
                // A prompt owns the keys while it is open.
                if let Some(prompt) = files.prompt.clone() {
                    return Self::files_prompt_key(files, id, prompt, byte);
                }
                let in_bin = files.bin;
                match byte {
                    crate::notepad::KEY_UP => {
                        files.select(-1);
                        (None, true)
                    }
                    crate::notepad::KEY_DOWN => {
                        files.select(1);
                        (None, true)
                    }
                    crate::notepad::KEY_PAGE_UP => {
                        files.select(-10);
                        (None, true)
                    }
                    crate::notepad::KEY_PAGE_DOWN => {
                        files.select(10);
                        (None, true)
                    }
                    crate::notepad::KEY_HOME => {
                        files.selection = 0;
                        (None, true)
                    }
                    crate::notepad::KEY_END => {
                        files.selection = files.visible().len().saturating_sub(1);
                        (None, true)
                    }
                    CTRL_R => (Some(DeskRequest::ListFiles { id }), true),
                    CTRL_U => {
                        files.list_only = !files.list_only;
                        (None, true)
                    }
                    CTRL_G => {
                        files.cycle_tag();
                        (None, true)
                    }
                    crate::notepad::CTRL_S => {
                        files.by_name = !files.by_name;
                        files.selection = 0;
                        (None, true)
                    }
                    CTRL_B => {
                        files.bin = !files.bin;
                        files.selection = 0;
                        files.scroll = 0;
                        (None, true)
                    }
                    crate::notepad::CTRL_N if !in_bin => {
                        files.prompt = Some(FilesPrompt::New(String::new()));
                        (None, true)
                    }
                    CTRL_E if !in_bin => match files.selected() {
                        Some(entry) => {
                            files.prompt = Some(FilesPrompt::Rename(entry.name.clone()));
                            (None, true)
                        }
                        None => (None, false),
                    },
                    CTRL_K if !in_bin => match files.selected() {
                        Some(_) => {
                            files.prompt = Some(FilesPrompt::Tag(String::new()));
                            (None, true)
                        }
                        None => (None, false),
                    },
                    crate::notepad::KEY_DELETE => match files.selected() {
                        Some(entry) if in_bin => {
                            let _ = entry;
                            files.prompt = Some(FilesPrompt::ConfirmPurge);
                            (None, true)
                        }
                        Some(entry) => (
                            Some(DeskRequest::TrashFile {
                                id,
                                name: entry.name.clone(),
                                trashed: true,
                            }),
                            true,
                        ),
                        None => (None, false),
                    },
                    crate::notepad::BACKSPACE => {
                        if files.filter.pop().is_some() {
                            files.selection = 0;
                            (None, true)
                        } else {
                            (None, false)
                        }
                    }
                    crate::notepad::ESC => {
                        if files.filter.is_empty() {
                            (None, false)
                        } else {
                            files.filter.clear();
                            files.selection = 0;
                            (None, true)
                        }
                    }
                    crate::notepad::CTRL_W => {
                        self.close(id);
                        (None, true)
                    }
                    b'\n' | b'\r' if in_bin => match files.selected() {
                        Some(entry) => (
                            Some(DeskRequest::TrashFile {
                                id,
                                name: entry.name.clone(),
                                trashed: false,
                            }),
                            true,
                        ),
                        None => (None, false),
                    },
                    b'\n' | b'\r' => match self.open_files_selection(id) {
                        Some(request) => (Some(request), true),
                        None => (None, false),
                    },
                    // Typing is searching: the filter is the card's search
                    // line, and it matches tags as well as names.
                    0x20..=0x7E if files.filter.len() < 40 => {
                        files.filter.push(byte as char);
                        files.selection = 0;
                        files.scroll = 0;
                        (None, true)
                    }
                    _ => (None, false),
                }
            }
            AppState::Welcome => match byte {
                crate::notepad::CTRL_W | crate::notepad::ESC | b'\n' | b'\r' => {
                    self.close(id);
                    (None, true)
                }
                _ => (None, false),
            },
            AppState::Now => match byte {
                crate::notepad::CTRL_W | crate::notepad::ESC => {
                    self.close(id);
                    (None, true)
                }
                b'n' | b'N' => {
                    self.open_or_raise(DeskApp::Notices);
                    (None, true)
                }
                b'l' | b'L' => {
                    self.open_or_raise(DeskApp::Look);
                    (None, true)
                }
                b'k' | b'K' => {
                    self.open_or_raise(DeskApp::Shortcuts);
                    (None, true)
                }
                _ => (None, false),
            },
            AppState::Calculator(calc) => {
                if byte == crate::notepad::CTRL_W {
                    self.close(id);
                    return (None, true);
                }
                (None, calc.handle_byte(byte))
            }
            AppState::Calendar(calendar) => match calendar.handle_byte(byte) {
                CalendarEffect::None => (None, false),
                CalendarEffect::Redraw => (None, true),
                CalendarEffect::Close => {
                    self.close(id);
                    (None, true)
                }
                CalendarEffect::OpenNote { name, exists } => (self.open_note(name, exists), true),
            },
            AppState::Timer(timer) => match timer.handle_byte(byte) {
                TimerEffect::None => (None, false),
                TimerEffect::Redraw => (None, true),
                TimerEffect::Close => {
                    self.close(id);
                    (None, true)
                }
            },
            AppState::Tiles(game) => match game.handle_byte(byte) {
                GameEffect::None => (None, false),
                GameEffect::Redraw => (None, true),
                GameEffect::Close => {
                    self.close(id);
                    (None, true)
                }
            },
            AppState::Tasks(tasks) => match tasks.handle_byte(byte) {
                TasksEffect::None => (None, false),
                TasksEffect::Redraw => (None, true),
                TasksEffect::Close => {
                    self.close(id);
                    (None, true)
                }
            },
            AppState::Sketch(sketch) => match sketch.handle_byte(byte) {
                SketchEffect::None => (None, false),
                SketchEffect::Redraw => (None, true),
                SketchEffect::Close => {
                    self.close(id);
                    (None, true)
                }
            },
            AppState::Shortcuts(scroll) => match byte {
                crate::notepad::CTRL_W | crate::notepad::ESC => {
                    self.close(id);
                    (None, true)
                }
                // The sheet is longer than a card: it scrolls (GFX-072).
                crate::notepad::KEY_DOWN | crate::notepad::KEY_PAGE_DOWN => {
                    let step = if byte == crate::notepad::KEY_DOWN {
                        1
                    } else {
                        10
                    };
                    let max = Self::shortcut_lines().len().saturating_sub(1);
                    *scroll = (*scroll + step).min(max);
                    (None, true)
                }
                crate::notepad::KEY_UP | crate::notepad::KEY_PAGE_UP => {
                    let step = if byte == crate::notepad::KEY_UP {
                        1
                    } else {
                        10
                    };
                    *scroll = scroll.saturating_sub(step);
                    (None, true)
                }
                _ => (None, false),
            },
            AppState::Notices => match byte {
                crate::notepad::CTRL_W | crate::notepad::ESC => {
                    self.close(id);
                    (None, true)
                }
                crate::notepad::KEY_DELETE => {
                    self.notice_log.clear();
                    self.unseen_notices = 0;
                    (None, true)
                }
                _ => (None, false),
            },
            AppState::Look(look) => {
                let current = current_look;
                match byte {
                    crate::notepad::KEY_UP | crate::notepad::KEY_DOWN => {
                        let row = if byte == crate::notepad::KEY_UP {
                            look.row.saturating_sub(1)
                        } else {
                            (look.row + 1).min(LookView::ROWS - 1)
                        };
                        look.row = row;
                        let preview = look.choice_at(&current);
                        self.look_preview = Some(preview);
                        (None, true)
                    }
                    b'\n' | b'\r' => {
                        // Keep: what is previewed becomes the look, on disk.
                        let chosen = self.look_preview.take().unwrap_or(current);
                        let text = chosen.to_text();
                        self.look = chosen;
                        (Some(DeskRequest::SaveLook { text }), true)
                    }
                    crate::notepad::ESC => {
                        look.row = kept_row;
                        self.look_preview = None;
                        (None, true)
                    }
                    crate::notepad::CTRL_W => {
                        self.look_preview = None;
                        self.close(id);
                        (None, true)
                    }
                    _ => (None, false),
                }
            }
            AppState::Notepad(notepad) => match if byte == crate::notepad::CTRL_V {
                notepad.paste(&clipboard)
            } else {
                notepad.handle_byte(byte)
            } {
                NotepadEffect::None => (None, false),
                NotepadEffect::Redraw => (None, true),
                NotepadEffect::ListFiles => (Some(DeskRequest::ListFiles { id }), true),
                NotepadEffect::Copy(text) => {
                    self.remember_copy(text.clone());
                    self.clipboard = text;
                    (None, true)
                }
                NotepadEffect::Version { path, index } => (
                    Some(DeskRequest::ReadVersion {
                        id,
                        name: path,
                        index,
                    }),
                    true,
                ),
                NotepadEffect::Close => {
                    self.close(id);
                    (None, true)
                }
                effect @ (NotepadEffect::Save { .. } | NotepadEffect::Open { .. }) => {
                    (Some(DeskRequest::Io { id, effect }), true)
                }
            },
        }
    }

    /// A key while a Files prompt is open.
    fn files_prompt_key(
        files: &mut FilesView,
        id: ViewId,
        prompt: FilesPrompt,
        byte: u8,
    ) -> (Option<DeskRequest>, bool) {
        let selected = files.selected().map(|e| e.name.clone());
        match (prompt, byte) {
            (_, crate::notepad::ESC) => {
                files.prompt = None;
                (None, true)
            }
            (FilesPrompt::ConfirmPurge, b'\n' | b'\r') => {
                files.prompt = None;
                match selected {
                    Some(name) => (Some(DeskRequest::PurgeFile { id, name }), true),
                    None => (None, true),
                }
            }
            (FilesPrompt::ConfirmPurge, _) => (None, false),
            (FilesPrompt::Tag(text), b'\n' | b'\r') => {
                files.prompt = None;
                let (add, remove) = parse_tag_edit(&text);
                match selected {
                    Some(name) if !(add.is_empty() && remove.is_empty()) => (
                        Some(DeskRequest::TagFile {
                            id,
                            name,
                            add,
                            remove,
                        }),
                        true,
                    ),
                    _ => (None, true),
                }
            }
            (FilesPrompt::New(name) | FilesPrompt::Rename(name), b'\n' | b'\r') => {
                let name = name.trim().to_string();
                let renaming = matches!(files.prompt, Some(FilesPrompt::Rename(_)));
                if name.is_empty() {
                    return (None, false);
                }
                files.prompt = None;
                if renaming {
                    let from = selected.unwrap_or_default();
                    if from == name {
                        return (None, true);
                    }
                    (Some(DeskRequest::RenameFile { id, from, to: name }), true)
                } else {
                    (Some(DeskRequest::CreateFile { id, name }), true)
                }
            }
            (
                FilesPrompt::New(mut text)
                | FilesPrompt::Rename(mut text)
                | FilesPrompt::Tag(mut text),
                byte,
            ) => {
                let kind = files.prompt.clone();
                match byte {
                    crate::notepad::BACKSPACE => {
                        text.pop();
                    }
                    0x20..=0x7E if text.len() < 64 => text.push(byte as char),
                    _ => return (None, false),
                }
                files.prompt = Some(match kind {
                    Some(FilesPrompt::Rename(_)) => FilesPrompt::Rename(text),
                    Some(FilesPrompt::Tag(_)) => FilesPrompt::Tag(text),
                    _ => FilesPrompt::New(text),
                });
                (None, true)
            }
        }
    }

    /// The kernel delivers a kept version to the Notepad that asked.
    pub fn version_loaded(
        &mut self,
        id: ViewId,
        index: usize,
        total: usize,
        content: Option<&str>,
        when: u64,
    ) {
        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
            notepad.show_version(index, total, content, when);
        }
    }

    /// The kernel reports a file operation's outcome, and the desk says so
    /// with a notice card.
    pub fn io_done(
        &mut self,
        id: ViewId,
        effect: &NotepadEffect,
        result: Result<Option<String>, String>,
        now: u64,
    ) -> Option<DeskRequest> {
        let quiet = if let Some(at) = self.quiet_saves.iter().position(|q| *q == id) {
            self.quiet_saves.remove(at);
            matches!(effect, NotepadEffect::Save { .. })
        } else {
            false
        };
        let follow_up = match (effect, &result) {
            (NotepadEffect::Save { path, .. }, Ok(_))
            | (NotepadEffect::Open { path }, Ok(Some(_))) => self.touch_recent(path),
            _ => None,
        };
        let (level, text) = match (effect, &result) {
            (NotepadEffect::Save { path, .. }, Ok(_)) => {
                self.notes_stale = true;
                (NoticeLevel::Info, alloc::format!("Saved {path}"))
            }
            (NotepadEffect::Open { path }, Ok(Some(_))) => {
                (NoticeLevel::Info, alloc::format!("Opened {path}"))
            }
            (NotepadEffect::Open { path }, Ok(None)) => {
                (NoticeLevel::Warning, alloc::format!("Not found: {path}"))
            }
            (_, Err(err)) => (NoticeLevel::Error, alloc::format!("{err}")),
            _ => (NoticeLevel::Info, String::new()),
        };
        if let Some(notepad) = self.window_mut(id).and_then(|w| w.notepad_mut()) {
            notepad.io_done(effect, result);
            if quiet {
                notepad.set_status("Saved itself");
            }
        }
        if !text.is_empty() && !quiet {
            self.notify(level, text, now);
        }
        follow_up
    }

    /// The window list for the compositor, top bar and dock included.
    ///
    /// `clock` is the top bar's right-hand text; `terminal` is the console
    /// as the kernel built it for the Terminal card, if one is open.
    pub fn windows(
        &mut self,
        clock: &str,
        caret_visible: bool,
        terminal: Option<&TerminalView>,
    ) -> Vec<DesktopWindow> {
        self.windows_at(clock, caret_visible, terminal, u64::MAX / 2, &[])
    }

    /// As `windows`, with the time (for notice expiry) and the workspace's
    /// own notices to show as cards.
    pub fn windows_at(
        &mut self,
        clock: &str,
        caret_visible: bool,
        terminal: Option<&TerminalView>,
        now: u64,
        shell_notices: &[ShellNotice],
    ) -> Vec<DesktopWindow> {
        self.notices.retain(|n| n.expires_at > now);
        self.last_tick = now;
        // The workspace's notices that are new this frame go in the log.
        // "New" means not in the previous frame's list and not the same
        // notice logged in the last half minute -- a frame built without
        // the workspace's list (the pointer path does that) must not make
        // every notice new again.
        let fresh: Vec<ShellNotice> = shell_notices
            .iter()
            .filter(|n| !self.last_shell_notices.contains(n))
            .filter(|n| {
                !self
                    .notice_log
                    .iter()
                    .any(|(then, logged)| logged == *n && now.saturating_sub(*then) < 3_000)
            })
            .cloned()
            .collect();
        for notice in fresh {
            self.log_notice(now, notice);
        }
        self.last_shell_notices = shell_notices.to_vec();
        if self
            .focused_window()
            .map(|w| w.app == DeskApp::Notices)
            .unwrap_or(false)
        {
            self.unseen_notices = 0;
        }
        let mut out = Vec::with_capacity(self.windows.len() + 8);

        // Cards.
        let focus = self.focus;
        let kept_look = self.look.clone();
        let look_preview = self.look_preview.clone();
        let notice_log = self.notice_log.clone();
        let now_tick = self.last_tick;
        let now_lines = self.now_lines(clock);
        let space = self.space;
        let overview_open = self.overview.is_some();
        if let Some(highlight) = self.overview {
            // The overview (GFX-068): every card as a small card, in a
            // grid, the highlighted one ringed. Real cards stay hidden.
            let order = self.overview_order();
            let area = self.work_area();
            let (w, h) = OVERVIEW_CARD;
            let step_x = w + OVERVIEW_GAP;
            let step_y = h + OVERVIEW_GAP + 10;
            let total_w = OVERVIEW_COLUMNS.min(order.len().max(1)) * step_x - OVERVIEW_GAP;
            let x0 = area.x + area.width.saturating_sub(total_w) / 2;
            let y0 = area.y + 40;
            for (index, id) in order.iter().enumerate() {
                let Some(window) = self.windows.iter().find(|w| w.id == *id) else {
                    continue;
                };
                let bounds = RasterRect::new(
                    x0 + (index % OVERVIEW_COLUMNS) * step_x,
                    y0 + (index / OVERVIEW_COLUMNS) * step_y,
                    w,
                    h,
                );
                out.push(self.overview_card(window, bounds, index == highlight));
            }
        }
        let pointer = self.pointer;
        let palette = crate::widgets::Palette::from_theme(&self.theme());
        for window in &mut self.windows {
            if window.tucked || window.space != space || overview_open {
                continue;
            }
            // The pointer in this card's canvas, for hover (GFX-081).
            let hover = pointer.and_then(|(px, py)| canvas_point_in(window.bounds, px, py));
            let rows = Self::card_rows(window.bounds);
            let focused = focus == Some(window.id);
            let mut highlight = None;
            let mut selection = Vec::new();
            let mut styles = Vec::new();
            let mut graphics: Option<Vec<view_types::DrawOp>> = None;
            let (lines, title, footer, cursor) = match &mut window.state {
                AppState::Notepad(notepad) => {
                    // Long lines wrap to the card (GFX-064); the width is
                    // whatever the card is this frame, so a resize reflows.
                    let columns = window
                        .bounds
                        .width
                        .saturating_sub(services_gui_host::CARD_PADDING * 2)
                        / GLYPH_WIDTH;
                    notepad.set_wrap(columns.saturating_sub(1));
                    let lines = notepad.viewport_lines(rows);
                    let cursor = notepad.viewport_cursor();
                    selection = notepad.viewport_selection(rows);
                    styles = notepad.viewport_styles(rows);
                    (lines, notepad.title(), notepad.footer(), cursor)
                }
                AppState::Files(files) => {
                    let visible = files.visible();
                    files.selection = files.selection.min(visible.len().saturating_sub(1));
                    if files.selection < files.scroll {
                        files.scroll = files.selection;
                    } else if rows > 0 && files.selection >= files.scroll + rows {
                        files.scroll = files.selection + 1 - rows;
                    }
                    let columns = window
                        .bounds
                        .width
                        .saturating_sub(services_gui_host::CARD_PADDING * 2)
                        / GLYPH_WIDTH;
                    // The preview (GFX-071) takes the right two fifths when
                    // the card is wide enough and the list is not asked for
                    // alone; the list keeps the left.
                    let selected_name = files.selected().map(|e| e.name.clone());
                    let with_preview =
                        !files.list_only && !files.bin && columns >= PREVIEW_MIN_COLUMNS;
                    let list_w = if with_preview {
                        columns * 11 / 20
                    } else {
                        columns
                    };
                    let preview_w = columns.saturating_sub(list_w + 3);
                    let preview_lines: Vec<String> = match (&files.preview, &selected_name) {
                        (Some((name, lines)), Some(selected)) if name == selected => lines.clone(),
                        (_, Some(_)) => alloc::vec!["...".to_string()],
                        (_, None) => Vec::new(),
                    };
                    let lines: Vec<String> = (0..rows)
                        .filter(|i| {
                            files.scroll + i < visible.len()
                                || (with_preview && *i < preview_lines.len())
                        })
                        .map(|i| {
                            let left = visible
                                .get(files.scroll + i)
                                .map(|index| FilesView::line(&files.entries[*index], list_w))
                                .unwrap_or_default();
                            if !with_preview {
                                return left;
                            }
                            let right: String = preview_lines
                                .get(i)
                                .map(|l| l.chars().take(preview_w).collect())
                                .unwrap_or_default();
                            alloc::format!("{:<list_w$} | {}", left, right)
                        })
                        .collect();
                    if !visible.is_empty() {
                        highlight = Some(files.selection - files.scroll);
                    }
                    let title = if files.bin { "Files - Bin" } else { "Files" };
                    (lines, title.to_string(), files.footer(), None)
                }
                AppState::Terminal => {
                    let view = terminal.cloned().unwrap_or_default();
                    (view.lines, "Terminal".to_string(), view.status, view.cursor)
                }
                AppState::Welcome => (
                    WELCOME_LINES.iter().map(|l| l.to_string()).collect(),
                    "Welcome to PandaGen".to_string(),
                    "Close this card and it stays closed".to_string(),
                    None,
                ),
                AppState::Now => (
                    now_lines.clone(),
                    "Now".to_string(),
                    "The machine, this second; refreshed every second".to_string(),
                    None,
                ),
                AppState::Calculator(calc) => {
                    let (w, h) = Self::canvas_size(window.bounds);
                    graphics = Some(calc.ui(w, h, palette, hover).into_ops());
                    (Vec::new(), "Calculator".to_string(), calc.footer(), None)
                }
                AppState::Calendar(calendar) => {
                    let (w, h) = Self::canvas_size(window.bounds);
                    graphics = Some(calendar.ui(w, h, palette, hover).into_ops());
                    (Vec::new(), "Calendar".to_string(), calendar.footer(), None)
                }
                AppState::Timer(timer) => {
                    let title = if timer.running() {
                        alloc::format!(
                            "Timer - {}",
                            crate::timer::format_ticks(timer.shown_ticks())
                        )
                    } else {
                        "Timer".to_string()
                    };
                    let (w, h) = Self::canvas_size(window.bounds);
                    graphics = Some(timer.ui(w, h, palette, hover).into_ops());
                    (Vec::new(), title, timer.footer(), None)
                }
                AppState::Tiles(game) => {
                    let (w, h) = Self::canvas_size(window.bounds);
                    graphics = Some(game.ui(w, h, palette, hover).into_ops());
                    (Vec::new(), "Tiles".to_string(), game.footer(), None)
                }
                AppState::Tasks(tasks) => {
                    let (w, h) = Self::canvas_size(window.bounds);
                    graphics = Some(tasks.ui(w, h, palette, hover).into_ops());
                    (Vec::new(), "Tasks".to_string(), tasks.footer(), None)
                }
                AppState::Sketch(sketch) => {
                    let canvas_width = window
                        .bounds
                        .width
                        .saturating_sub(services_gui_host::CARD_PADDING * 2)
                        as u32;
                    graphics = Some(sketch.ops(canvas_width));
                    (Vec::new(), "Sketch".to_string(), sketch.footer(), None)
                }
                AppState::Shortcuts(scroll) => {
                    let all = Self::shortcut_lines();
                    let shown: Vec<String> = all.iter().skip(*scroll).cloned().collect();
                    let footer = if all.len() > rows {
                        alloc::format!(
                            "{} of {} keys   arrows and the wheel scroll",
                            (*scroll + rows).min(all.len()),
                            all.len()
                        )
                    } else {
                        "Every key the desk answers; the palette lists the rest".to_string()
                    };
                    (shown, "Shortcuts".to_string(), footer, None)
                }
                AppState::Notices => {
                    let lines: Vec<String> = notice_log
                        .iter()
                        .take(rows)
                        .map(|(then, notice)| {
                            alloc::format!(
                                "{:>8}   {:<5}  {}",
                                Desk::ago(now_tick, *then),
                                notice.card_title(),
                                notice.text
                            )
                        })
                        .collect();
                    let footer = if notice_log.is_empty() {
                        "Nothing yet: saves, opens and the console's warnings land here".to_string()
                    } else {
                        alloc::format!("{} kept, newest first   Delete clears", notice_log.len())
                    };
                    (lines, "Notices".to_string(), footer, None)
                }
                AppState::Look(look) => {
                    let preview = look_preview.clone().unwrap_or_else(|| kept_look.clone());
                    let lines = LookView::lines(&kept_look, &preview);
                    highlight = Some(LookView::line_of_row(look.row));
                    let footer = if look_preview.is_some() {
                        alloc::format!(
                            "Previewing {} + {}   Enter keeps it   Esc goes back",
                            preview.theme,
                            preview.accent
                        )
                    } else {
                        alloc::format!(
                            "{} + {}   arrows preview, and it changes as you go",
                            preview.theme,
                            preview.accent
                        )
                    };
                    (lines, "Look".to_string(), footer, None)
                }
            };
            let mut frame = ViewFrame::new(
                window.id,
                ViewKind::TextBuffer,
                0,
                ViewContent::text_buffer(lines),
                0,
            );
            frame.title = Some(title);
            if let Some(ops) = graphics.take() {
                // A canvas, not text: the card draws these in its content
                // area's own pixel space (GFX-080).
                frame.content = ViewContent::graphics(ops);
            }
            if caret_visible && focused {
                if let Some((line, column)) = cursor {
                    frame.cursor = Some(CursorPosition::new(line, column));
                }
            }
            let mut card = DesktopWindow::card(frame, window.bounds)
                .with_z_index(window.z)
                .with_footer(Some(footer))
                .with_highlight(highlight)
                .with_selection(selection)
                .with_line_styles(styles)
                .with_actions(
                    window
                        .actions()
                        .into_iter()
                        .map(|(label, _)| label)
                        .collect(),
                );
            if focused {
                card = card.focused();
            }
            out.push(card);
        }

        // The palette: a card near the top, above everything, without a
        // close glyph -- Esc or a click elsewhere closes it.
        if let Some(palette) = &self.palette {
            let has_window = focus.is_some();
            let has_notepad = self
                .windows
                .iter()
                .any(|w| focus == Some(w.id) && w.notepad().is_some());
            let matches = palette.matches(
                has_window,
                has_notepad,
                &self.recent,
                &self.clipboard_history,
            );
            let rows = matches.len().min(PALETTE_MAX_ROWS);
            let mut lines = Vec::with_capacity(rows + 1);
            let inner = PALETTE_WIDTH.saturating_sub(services_gui_host::CARD_PADDING * 2)
                / services_gui_host::RASTER_CELL_WIDTH;
            for action in matches.iter().take(rows) {
                let label = action.label();
                let shortcut = action.shortcut();
                let pad =
                    inner.saturating_sub(label.chars().count() + shortcut.chars().count() + 2);
                let mut line = label.clone();
                for _ in 0..pad {
                    line.push(' ');
                }
                line.push_str("  ");
                line.push_str(shortcut);
                lines.push(line);
            }
            if matches.is_empty() {
                lines.push("No actions match".to_string());
            }
            let height = services_gui_host::CARD_HEADER_HEIGHT
                + services_gui_host::CARD_PADDING * 2
                + lines.len() * services_gui_host::CARD_LINE_HEIGHT
                + services_gui_host::CARD_FOOTER_HEIGHT;
            let mut frame = ViewFrame::new(
                self.palette_id,
                ViewKind::Panel,
                0,
                ViewContent::text_buffer(lines),
                0,
            );
            frame.title = Some(alloc::format!("> {}_", palette.query));
            let mut card = DesktopWindow::card(
                frame,
                RasterRect::new(
                    self.width.saturating_sub(PALETTE_WIDTH) / 2,
                    TOP_BAR_HEIGHT + 40,
                    PALETTE_WIDTH,
                    height,
                ),
            )
            .with_role(DesktopWindowRole::Palette)
            .with_z_index(usize::MAX / 2)
            .with_footer(Some("Enter runs   Esc closes   type to filter".to_string()))
            .with_highlight(
                (!matches.is_empty()).then_some(palette.selection.min(rows.saturating_sub(1))),
            );
            card.closable = false;
            card.focused = true;
            out.push(card);
        }

        // Notices: small cards at the top right, newest highest. The
        // workspace's own (a display switch, an error) and the desk's (a
        // save that landed) share one look.
        let mut all: Vec<ShellNotice> = shell_notices
            .iter()
            .filter(|n| !self.dismissed_shell.contains(n))
            .cloned()
            .collect();
        all.extend(self.notices.iter().map(|n| n.notice.clone()));
        let mut y = TOP_BAR_HEIGHT + NOTICE_MARGIN;
        let columns = (NOTICE_WIDTH - services_gui_host::CARD_PADDING * 2) / GLYPH_WIDTH;
        for (index, notice) in all.iter().rev().take(self.notice_ids.len()).enumerate() {
            let lines = wrap_words(&notice.text, columns);
            let height = services_gui_host::CARD_HEADER_HEIGHT
                + services_gui_host::CARD_PADDING * 2
                + services_gui_host::CARD_LINE_HEIGHT * lines.len().max(1);
            let mut frame = ViewFrame::new(
                self.notice_ids[index],
                ViewKind::Panel,
                0,
                ViewContent::text_buffer(lines),
                0,
            );
            frame.title = Some(notice.card_title());
            let mut card = DesktopWindow::card(
                frame,
                RasterRect::new(
                    self.width.saturating_sub(NOTICE_WIDTH + NOTICE_MARGIN),
                    y,
                    NOTICE_WIDTH,
                    height,
                ),
            )
            .with_role(DesktopWindowRole::Notification)
            .with_z_index(usize::MAX / 2 - 1);
            card.closable = false;
            out.push(card);
            y += height + NOTICE_MARGIN;
        }

        // Top bar: the focused app's name on the left, the clock on the right.
        // With nothing open the bar says how to begin, once; a desk with
        // no hint on it is a desk the first visitor stares at.
        let left = match (
            self.hovered_tile.and_then(|i| DeskApp::ALL.get(i)),
            self.focused_window(),
        ) {
            _ if self.overview.is_some() => {
                "Overview   -   Ctrl+Tab again, let go to pick, Esc to stay".to_string()
            }
            // The dock has no labels; the bar says what the tile under the
            // pointer is, and whether it is running (GFX-062).
            (Some(app), _) => {
                let running = self.windows.iter().filter(|w| w.app == *app).count();
                match running {
                    0 => alloc::format!("{}   -   click to open", app.name()),
                    1 => alloc::format!("{}   -   open", app.name()),
                    n => alloc::format!("{}   -   {n} open", app.name()),
                }
            }
            (None, Some(w)) => w.app.name().to_string(),
            (None, None) if !self.windows.iter().any(|w| self.on_stage(w)) => {
                "PandaGen   -   type to search, Ctrl+Space for everything".to_string()
            }
            (None, None) => "PandaGen".to_string(),
        };
        let mut bar_frame = ViewFrame::new(
            self.top_bar_id,
            ViewKind::StatusLine,
            0,
            ViewContent::text_buffer(alloc::vec![self.bar_right(clock), self.space_strip()]),
            0,
        );
        bar_frame.title = Some(left);
        out.push(
            DesktopWindow::new(bar_frame, SurfaceRect::new(0, 0, 0, 0))
                .with_role(DesktopWindowRole::Status)
                .with_layer(DesktopWindowLayer::System)
                .with_style(WindowStyle::TopBar)
                .with_pixel_rect(RasterRect::new(0, 0, self.width, TOP_BAR_HEIGHT)),
        );

        // Dock: one tile per app, the running ones marked, the hovered one lit.
        let tabs: Vec<DesktopTab> = DeskApp::ALL
            .iter()
            .filter(|app| {
                !matches!(
                    app,
                    DeskApp::Welcome | DeskApp::Notices | DeskApp::Now | DeskApp::Shortcuts
                )
            })
            .enumerate()
            .map(|(index, app)| {
                let mut tab =
                    DesktopTab::new(app.monogram(), self.windows.iter().any(|w| w.app == *app))
                        .with_icon(app.icon());
                tab.hovered = self.hovered_tile == Some(index);
                tab.tucked = self.windows.iter().any(|w| w.app == *app && w.tucked);
                tab
            })
            .collect();
        let count = tabs.len();
        let pill_width = count * services_gui_host::DOCK_TILE
            + count.saturating_sub(1) * services_gui_host::DOCK_TILE_GAP
            + 32;
        let dock_frame = ViewFrame::new(
            self.dock_id,
            ViewKind::Panel,
            0,
            ViewContent::text_buffer(Vec::new()),
            0,
        );
        out.push(
            DesktopWindow::new(dock_frame, SurfaceRect::new(0, 0, 0, 0))
                .with_role(DesktopWindowRole::Launcher)
                .with_layer(DesktopWindowLayer::System)
                .with_style(WindowStyle::Dock)
                .with_tabs(tabs)
                .with_pixel_rect(RasterRect::new(
                    self.width.saturating_sub(pill_width) / 2,
                    self.height.saturating_sub(DOCK_HEIGHT + DOCK_MARGIN),
                    pill_width,
                    DOCK_HEIGHT,
                )),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dock's tile row: every tile and the gaps between, so a test
    /// finds the first tile whatever the number of apps.
    fn dock_row_width() -> usize {
        let n = DeskApp::ALL.len();
        n * services_gui_host::DOCK_TILE + (n - 1) * services_gui_host::DOCK_TILE_GAP
    }
    use input_types::{ButtonState, Modifiers, PointerButtons, PointerEvent, PointerPosition};
    use services_gui_host::{Compositor, DesktopInputRouter};

    fn pointer(kind: PointerEventKind, x: i32, y: i32, buttons: PointerButtons) -> PointerEvent {
        PointerEvent::new(kind, PointerPosition::new(x, y), buttons, Modifiers::NONE)
    }

    fn press(x: i32, y: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Button {
                button: PointerButton::Primary,
                state: ButtonState::Pressed,
            },
            x,
            y,
            PointerButtons::PRIMARY,
        )
    }

    fn release(x: i32, y: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Button {
                button: PointerButton::Primary,
                state: ButtonState::Released,
            },
            x,
            y,
            PointerButtons::none(),
        )
    }

    fn moved(x: i32, y: i32, buttons: PointerButtons) -> PointerEvent {
        pointer(
            PointerEventKind::Move {
                delta: input_types::PointerDelta::new(0, 0),
            },
            x,
            y,
            buttons,
        )
    }

    fn wheel(x: i32, y: i32, dy: i32) -> PointerEvent {
        pointer(
            PointerEventKind::Wheel { dx: 0, dy },
            x,
            y,
            PointerButtons::none(),
        )
    }

    /// Drive one event through the real router against the desk's own
    /// window list, then apply the deliveries.
    fn route(desk: &mut Desk, router: &mut DesktopInputRouter, event: PointerEvent) -> bool {
        let compositor = Compositor::new();
        let windows = desk.windows("00:00", true, None);
        let deliveries = router.route(&compositor, &windows, event);
        desk.handle_deliveries(&deliveries)
    }

    fn drag(desk: &mut Desk, router: &mut DesktopInputRouter, from: (i32, i32), to: (i32, i32)) {
        route(desk, router, press(from.0, from.1));
        route(desk, router, moved(to.0, to.1, PointerButtons::PRIMARY));
        route(desk, router, release(to.0, to.1));
    }

    #[test]
    fn ctrl_n_and_ctrl_t_on_the_bare_desk_open_apps() {
        let mut desk = Desk::new(1280, 800);
        assert!(desk.handle_key(crate::notepad::CTRL_N).1);
        assert!(desk.handle_key(crate::notepad::CTRL_T).1);
        assert_eq!(desk.window_count(), 2);
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );
        assert!(desk.terminal_rows().is_some());
    }

    /// The kernel used to route bare-desk keys through a second entry point
    /// that knew three shortcuts, so the first Ctrl+Space of a session --
    /// before any card had focus -- went to the invisible console, and so
    /// did everything typed after it. There is one entry point now, and a
    /// printable key on the bare desk is the start of a palette search.
    #[test]
    fn typing_on_the_bare_desk_opens_the_palette_with_the_key_typed() {
        let mut desk = Desk::new(1280, 800);
        assert!(!desk.palette_open());
        let (request, changed) = desk.handle_key(b't');
        assert!(request.is_none() && changed && desk.palette_open());
        for byte in b"erm" {
            desk.handle_key(*byte);
        }
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        assert!(palette.frame.title.as_deref().unwrap().contains("term"));
        desk.handle_key(b'\n');
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // A control byte the bare desk has no use for is not taken.
        let mut desk = Desk::new(1280, 800);
        assert_eq!(desk.handle_key(0x01), (None, false));
        assert!(!desk.palette_open());
    }

    /// The first build answered the launch shortcuts only with nothing
    /// focused, so with a Notepad open Ctrl+T fell into the Notepad and did
    /// nothing. Launching is global; Ctrl+N is the Notepad's own inside one.
    #[test]
    fn launch_shortcuts_work_whatever_is_focused() {
        let mut desk = Desk::new(1280, 800);
        desk.handle_key(crate::notepad::CTRL_N);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // Ctrl+T from inside the Notepad opens a Terminal.
        let (_, changed) = desk.handle_key(crate::notepad::CTRL_T);
        assert!(changed);
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // Ctrl+N from the Terminal opens a second Notepad...
        desk.handle_key(crate::notepad::CTRL_N);
        assert_eq!(desk.window_count(), 3);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // ...while inside a Notepad it is "new document", not a new window.
        for byte in b"draft" {
            desk.handle_key(*byte);
        }
        desk.handle_key(crate::notepad::CTRL_N);
        desk.handle_key(crate::notepad::CTRL_N); // confirm the discard
        assert_eq!(desk.window_count(), 3);
        assert_eq!(
            desk.focused_window()
                .and_then(|w| w.notepad())
                .map(|n| n.content()),
            Some(String::new())
        );
    }

    #[test]
    fn the_palette_lists_filters_and_runs_actions() {
        let mut desk = Desk::new(1280, 800);
        // Ctrl+Space opens it from the bare desk; it is a card.
        let (_, changed) = desk.handle_key(KEY_CTRL_SPACE);
        assert!(changed && desk.palette_open());
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        assert_eq!(palette.style, WindowStyle::Card);
        // With nothing open, window-only actions are not offered.
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(listed.iter().any(|l| l.starts_with("New Notepad window")));
        assert!(!listed.iter().any(|l| l.starts_with("Close window")));

        // Typing filters; Enter runs the selected row.
        for byte in b"term" {
            desk.handle_key(*byte);
        }
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        assert!(palette.frame.title.as_deref().unwrap().contains("term"));
        let (request, _) = desk.handle_key(b'\n');
        assert!(request.is_none());
        assert!(!desk.palette_open());
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // Esc closes without running.
        desk.handle_key(0x10);
        assert!(desk.palette_open());
        desk.handle_key(crate::notepad::ESC);
        assert!(!desk.palette_open());

        // "Switch to the text console" is a request to the kernel.
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"text" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(request, Some(DeskRequest::TextConsole));
    }

    #[test]
    fn dropping_a_card_on_the_dock_tucks_it_and_the_tile_brings_it_back() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let bounds = desk.window(id).unwrap().bounds;
        drag(
            &mut desk,
            &mut router,
            ((bounds.x + 100) as i32, (bounds.y + 10) as i32),
            (640, 790),
        );
        assert!(desk.window(id).unwrap().tucked);
        assert_eq!(desk.focus(), None);
        let windows = desk.windows("", true, None);
        assert!(
            !windows.iter().any(|w| w.frame.view_id == id),
            "a tucked card is still drawn"
        );
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(dock.tabs[0].active && dock.tabs[0].tucked);

        // Ctrl+Tab skips it; the dock tile brings it back.
        assert!(!desk.cycle_focus());
        let pill = dock.bounds();
        let first_x = (pill.x + (pill.width - dock_row_width()) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;
        route(&mut desk, &mut router, press(first_x, y));
        assert!(!desk.window(id).unwrap().tucked);
        assert_eq!(desk.focus(), Some(id));
    }

    #[test]
    fn files_lists_the_filesystem_and_enter_opens_in_a_notepad() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let windows = desk.windows("", true, None);
        let pill = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap()
            .bounds();
        let files_x = (pill.x + (pill.width - dock_row_width()) / 2 + 20 + 48) as i32;
        let y = (pill.y + pill.height / 2) as i32;
        let compositor = Compositor::new();
        let deliveries = router.route(&compositor, &windows, press(files_x, y));
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        let id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(requests, alloc::vec![DeskRequest::ListFiles { id }]);

        desk.files_listed(
            id,
            alloc::vec![FileEntry::named("a.txt"), FileEntry::named("b.txt")],
        );
        desk.handle_key(crate::notepad::KEY_DOWN);
        let (request, _) = desk.handle_key(b'\n');
        let notepad_id = desk.focused_window().map(|w| w.id).unwrap();
        assert_ne!(notepad_id, id);
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id: notepad_id,
                effect: NotepadEffect::Open {
                    path: "b.txt".to_string()
                }
            })
        );
        assert_eq!(desk.window_count(), 2);
    }

    #[test]
    fn notices_wrap_to_the_card_instead_of_running_off_it() {
        assert_eq!(
            wrap_words("Saved n.txt", 40),
            alloc::vec!["Saved n.txt".to_string()]
        );
        assert_eq!(
            wrap_words("Unknown command: files. Type 'help' for help.", 20),
            alloc::vec![
                "Unknown command:".to_string(),
                "files. Type 'help'".to_string(),
                "for help.".to_string()
            ]
        );
        assert_eq!(
            wrap_words("abcdefghij", 4),
            alloc::vec!["abcd".to_string(), "efgh".to_string(), "ij".to_string()]
        );
        assert_eq!(wrap_words("", 4), alloc::vec![String::new()]);

        let mut desk = Desk::new(1280, 800);
        let shell = alloc::vec![ShellNotice::new(
            NoticeLevel::Warning,
            "Unknown command: files. Type 'help' for help.",
        )];
        let windows = desk.windows_at("", true, None, 5000, &shell);
        let notice = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Notification)
            .unwrap();
        let lines = match &notice.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(lines.len() >= 2, "{lines:?}");
        assert!(lines
            .iter()
            .all(|l| l.len() * GLYPH_WIDTH <= NOTICE_WIDTH - 16));
        assert_eq!(
            notice.bounds().height,
            services_gui_host::CARD_HEADER_HEIGHT
                + 16
                + services_gui_host::CARD_LINE_HEIGHT * lines.len()
        );
    }

    #[test]
    fn in_files_one_click_selects_and_a_click_on_the_selection_opens() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Files);
        desk.files_listed(
            id,
            alloc::vec![FileEntry::named("a.txt"), FileEntry::named("b.txt")],
        );
        let bounds = desk.window(id).unwrap().bounds;
        let text_x = (bounds.x + services_gui_host::CARD_PADDING + 4) as i32;
        let row_y = |row: usize| {
            (bounds.y
                + services_gui_host::CARD_HEADER_HEIGHT
                + services_gui_host::CARD_PADDING
                + services_gui_host::CARD_LINE_HEIGHT * row
                + 4) as i32
        };
        let compositor = Compositor::new();
        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, press(text_x, row_y(1)));
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        assert!(requests.is_empty(), "the first click only selects");
        route(&mut desk, &mut router, release(text_x, row_y(1)));
        assert_eq!(
            desk.window(id).unwrap().files().map(|f| f.selection),
            Some(1)
        );

        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, press(text_x, row_y(1)));
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        let notepad_id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(
            requests,
            alloc::vec![DeskRequest::Io {
                id: notepad_id,
                effect: NotepadEffect::Open {
                    path: "b.txt".to_string()
                }
            }]
        );
    }

    #[test]
    fn the_clipboard_is_the_desks_and_every_notepad_shares_it() {
        let mut desk = Desk::new(1280, 800);
        let first = desk.launch(DeskApp::Notepad);
        for byte in b"copy me" {
            desk.handle_key(*byte);
        }
        desk.handle_key(crate::notepad::CTRL_A);
        let (request, changed) = desk.handle_key(crate::notepad::CTRL_C);
        assert!(request.is_none() && changed);
        assert_eq!(desk.clipboard, "copy me");

        let second = desk.launch(DeskApp::Notepad);
        assert_ne!(first, second);
        desk.handle_key(crate::notepad::CTRL_V);
        assert_eq!(
            desk.window(second).unwrap().notepad().unwrap().content(),
            "copy me"
        );

        // The selection is drawn: the card carries spans for the compositor.
        desk.handle_key(crate::notepad::CTRL_A);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == second).unwrap();
        assert_eq!(card.selection_spans, alloc::vec![(0, 0, 7)]);
        let other = windows.iter().find(|w| w.frame.view_id == first).unwrap();
        assert!(
            other.selection_spans.is_empty() || other.selection_spans == alloc::vec![(0, 0, 7)]
        );
    }

    #[test]
    fn dragging_across_a_notepads_text_selects_it() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        for byte in b"hello world" {
            desk.handle_key(*byte);
        }
        let bounds = desk.window(id).unwrap().bounds;
        let text_x = |col: usize| {
            (bounds.x
                + services_gui_host::CARD_PADDING
                + col * services_gui_host::RASTER_CELL_WIDTH
                + 2) as i32
        };
        let y = (bounds.y
            + services_gui_host::CARD_HEADER_HEIGHT
            + services_gui_host::CARD_PADDING
            + 4) as i32;
        drag(&mut desk, &mut router, (text_x(6), y), (text_x(11), y));
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(notepad.selected_text().as_deref(), Some("world"));

        // After the release, moving the pointer does not grow it.
        route(
            &mut desk,
            &mut router,
            moved(text_x(0), y, PointerButtons::none()),
        );
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(notepad.selected_text().as_deref(), Some("world"));

        // Editing actions are palette rows only over a Notepad.
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"find" {
            desk.handle_key(*byte);
        }
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(
            listed.iter().any(|l| l.starts_with("Find...")),
            "{listed:?}"
        );
        desk.handle_key(b'\n');
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert!(
            notepad.footer().starts_with("Find: world_"),
            "{}",
            notepad.footer()
        );
    }

    #[test]
    fn the_terminal_card_scrolls_the_workspaces_scrollback_by_wheel_and_page_keys() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Terminal);
        let bounds = desk.window(id).unwrap().bounds;
        let (x, y) = ((bounds.x + 100) as i32, (bounds.y + 100) as i32);
        let compositor = Compositor::new();
        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, wheel(x, y, 2));
        let (requests, changed) = desk.handle_deliveries_with_requests(&deliveries);
        assert!(changed);
        assert_eq!(
            requests,
            alloc::vec![DeskRequest::TerminalScroll { notches: 6 }]
        );
        assert_eq!(
            desk.handle_key(crate::notepad::KEY_PAGE_UP).0,
            Some(DeskRequest::TerminalScroll { notches: 10 })
        );
        assert_eq!(
            desk.handle_key(crate::notepad::KEY_PAGE_DOWN).0,
            Some(DeskRequest::TerminalScroll { notches: -10 })
        );
    }

    #[test]
    fn the_look_card_previews_as_you_move_keeps_on_enter_and_reverts_on_esc() {
        let mut desk = Desk::new(1280, 800);
        assert_eq!(desk.theme(), Theme::DESK);
        let windows = desk.windows("", true, None);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert!(bar.frame.title.as_deref().unwrap().contains("Ctrl+Space"));

        // The palette opens Look.
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"look" {
            desk.handle_key(*byte);
        }
        desk.handle_key(b'\n');
        let id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(desk.window(id).unwrap().app, DeskApp::Look);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.actions, alloc::vec!["Keep", "Revert"]);
        assert_eq!(
            card.highlight_line,
            Some(1),
            "opens on the kept theme, Dusk"
        );

        // Down twice: Ember, previewed at once, not kept.
        desk.handle_key(crate::notepad::KEY_DOWN);
        desk.handle_key(crate::notepad::KEY_DOWN);
        assert_eq!(desk.theme(), Theme::EMBER.with_accent(Theme::ACCENTS[0].1));
        assert_eq!(desk.look().theme, "Dusk");
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert!(card
            .footer
            .as_deref()
            .unwrap()
            .starts_with("Previewing Ember + Mint"));
        // Esc reverts.
        desk.handle_key(crate::notepad::ESC);
        assert_eq!(desk.theme(), Theme::DESK);

        // Down to Ember again, then on to the accents: Sky. Enter keeps and
        // asks the kernel to write it.
        for _ in 0..(LookView::THEME_ROWS + 1) {
            desk.handle_key(crate::notepad::KEY_DOWN);
        }
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::SaveLook {
                text: "theme=Mono\naccent=Sky\nwallpaper=Picture\n".to_string()
            })
        );
        // The third section: the picture is on by default; Gradient turns
        // it off, previewed like everything else.
        assert!(desk.wallpaper().is_some());
        for _ in 0..(LookView::ROWS) {
            desk.handle_key(crate::notepad::KEY_DOWN);
        }
        assert!(desk.wallpaper().is_none(), "the last row is Gradient");
        desk.handle_key(crate::notepad::ESC);
        assert!(desk.wallpaper().is_some());
        let mut parsed = Desk::new(1280, 800);
        parsed.apply_look(Some("wallpaper=gradient\n"));
        assert!(parsed.wallpaper().is_none());
        assert_eq!(parsed.look().theme, "Dusk");
        assert_eq!(WALLPAPER.indices.len(), WALLPAPER.width * WALLPAPER.height);
        assert_eq!(WALLPAPER.palette.len(), 256 * 3);
        assert_eq!(desk.look().theme, "Mono");
        assert_eq!(desk.theme(), Theme::MONO.with_accent(Theme::ACCENTS[1].1));

        // What was written reads back; nonsense reads as the defaults.
        let mut fresh = Desk::new(1280, 800);
        fresh.apply_look(Some("theme=mono\naccent=SKY\n"));
        assert_eq!(fresh.look(), desk.look());
        fresh.apply_look(Some("theme=plaid\n"));
        assert_eq!(fresh.look(), &LookChoice::default());
        fresh.apply_look(None);
        assert_eq!(fresh.look(), &LookChoice::default());

        // Closing the card drops any preview.
        desk.handle_key(crate::notepad::KEY_UP);
        assert_ne!(desk.theme(), desk.look().theme());
        desk.close(id);
        assert_eq!(desk.theme(), desk.look().theme());

        // A card with focus names itself in the bar.
        desk.launch(DeskApp::Notepad);
        let windows = desk.windows("", true, None);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert_eq!(bar.frame.title.as_deref(), Some("Notepad"));
    }

    #[test]
    fn a_still_document_saves_itself_quietly_and_the_palette_remembers_it() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        desk.window_mut(id)
            .unwrap()
            .notepad_mut()
            .unwrap()
            .load(Some("memo".to_string()), "");
        desk.handle_key(b'h');
        assert!(
            desk.tick(1_000).is_empty(),
            "the first tick only starts the clock"
        );
        assert!(desk
            .tick(1_000 + crate::notepad::AUTOSAVE_IDLE_TICKS - 1)
            .is_empty());
        let requests = desk.tick(1_000 + crate::notepad::AUTOSAVE_IDLE_TICKS);
        let effect = NotepadEffect::Save {
            path: "memo".to_string(),
            content: "h".to_string(),
        };
        assert_eq!(
            requests,
            alloc::vec![DeskRequest::Io {
                id,
                effect: effect.clone()
            }]
        );
        // It lands quietly: no notice, a status, and the file is recent.
        let follow_up = desk.io_done(id, &effect, Ok(None), 2_000);
        assert_eq!(
            follow_up,
            Some(DeskRequest::SaveRecent {
                text: "memo\n".to_string()
            })
        );
        let windows = desk.windows_at("", true, None, 2_000, &[]);
        assert!(!windows
            .iter()
            .any(|w| w.role == DesktopWindowRole::Notification));
        assert!(desk
            .window(id)
            .unwrap()
            .notepad()
            .unwrap()
            .footer()
            .contains("Saved itself"));
        assert_eq!(desk.recent(), &["memo".to_string()]);

        // A save the person asked for still says so.
        let follow_up = desk.io_done(id, &effect, Ok(None), 3_000);
        assert_eq!(follow_up, None, "already at the front");
        let windows = desk.windows_at("", true, None, 3_000, &[]);
        assert!(windows
            .iter()
            .any(|w| w.role == DesktopWindowRole::Notification));

        // The palette lists it first, and Enter opens it in a new Notepad.
        desk.handle_key(KEY_CTRL_SPACE);
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(listed[0].starts_with("Open memo"), "{listed:?}");
        let (request, _) = desk.handle_key(b'\n');
        let new_id = desk.focused_window().map(|w| w.id).unwrap();
        assert_ne!(new_id, id);
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id: new_id,
                effect: NotepadEffect::Open {
                    path: "memo".to_string()
                }
            })
        );

        // The list reads back, bounded and newest first.
        let mut fresh = Desk::new(1280, 800);
        fresh.apply_recent(Some("a\nb\nc\nd\ne\nf\ng\n"));
        assert_eq!(fresh.recent().len(), RECENT_FILES);
        assert_eq!(fresh.recent()[0], "a");
    }

    #[test]
    fn the_palette_searches_inside_files_and_enter_opens_the_file_on_the_line() {
        let mut desk = Desk::new(1280, 800);
        desk.handle_key(KEY_CTRL_SPACE);
        // One letter is not a search; two are.
        assert_eq!(desk.handle_key(b'q').0, None);
        assert_eq!(
            desk.handle_key(b'u').0,
            Some(DeskRequest::SearchFiles {
                query: "qu".to_string()
            })
        );
        desk.search_results(
            "qu",
            alloc::vec![("memo".to_string(), "  the quick fox".to_string())],
        );
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(listed[0].starts_with("memo: the quick fox"), "{listed:?}");
        assert!(listed[0].trim_end().ends_with("in file"), "{listed:?}");

        // A stale answer is not shown once the query moves on.
        desk.handle_key(b'i');
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(!listed.iter().any(|l| l.starts_with("memo:")), "{listed:?}");
        desk.search_results(
            "qui",
            alloc::vec![("memo".to_string(), "the quick fox".to_string())],
        );

        // Enter opens the file in a Notepad that will find the query.
        let (request, _) = desk.handle_key(b'\n');
        let id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id,
                effect: NotepadEffect::Open {
                    path: "memo".to_string()
                }
            })
        );
        desk.io_done(
            id,
            &NotepadEffect::Open {
                path: "memo".to_string(),
            },
            Ok(Some("slow\nthe quick fox\n".to_string())),
            100,
        );
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(notepad.selected_text().as_deref(), Some("qui"));
        assert_eq!(notepad.cursor().row, 1);
    }

    #[test]
    fn ctrl_arrows_snap_fill_and_put_a_window_back() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;
        let area = desk.work_area();
        assert!(desk.handle_key(crate::notepad::KEY_CTRL_LEFT).1);
        let bounds = desk.window(id).unwrap().bounds;
        assert_eq!((bounds.x, bounds.width), (area.x, area.width / 2));
        desk.handle_key(crate::notepad::KEY_CTRL_RIGHT);
        let bounds = desk.window(id).unwrap().bounds;
        assert_eq!(bounds.x, area.x + area.width / 2);
        desk.handle_key(crate::notepad::KEY_CTRL_UP);
        assert_eq!(desk.window(id).unwrap().bounds, area);
        // Ctrl+Down: the size before the first snap, not the last one.
        desk.handle_key(crate::notepad::KEY_CTRL_DOWN);
        assert_eq!(desk.window(id).unwrap().bounds, before);
        // Nothing snapped, nothing to put back.
        assert!(!desk.handle_key(crate::notepad::KEY_CTRL_DOWN).1);
        // With nothing focused the keys do nothing.
        desk.close(id);
        assert_eq!(
            desk.handle_key(crate::notepad::KEY_CTRL_LEFT),
            (None, false)
        );
    }

    #[test]
    fn the_bar_names_the_dock_tile_under_the_pointer() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        desk.launch(DeskApp::Notepad);
        let windows = desk.windows("", true, None);
        let pill = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap()
            .bounds();
        let first_x = (pill.x + (pill.width - dock_row_width()) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;
        route(
            &mut desk,
            &mut router,
            moved(first_x, y, PointerButtons::none()),
        );
        let bar_title = |desk: &mut Desk| {
            desk.windows("", true, None)
                .iter()
                .find(|w| w.style == WindowStyle::TopBar)
                .and_then(|w| w.frame.title.clone())
                .unwrap()
        };
        assert_eq!(bar_title(&mut desk), "Notepad   -   open");
        route(
            &mut desk,
            &mut router,
            moved(first_x + 48, y, PointerButtons::none()),
        );
        assert_eq!(bar_title(&mut desk), "Files   -   click to open");
        // Off the dock: the focused card's name again.
        route(
            &mut desk,
            &mut router,
            moved(640, 400, PointerButtons::none()),
        );
        assert_eq!(bar_title(&mut desk), "Notepad");
    }

    #[test]
    fn the_pointer_alone_runs_the_palette_dismisses_notices_and_picks_a_look() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let compositor = Compositor::new();
        // The bar opens the palette.
        route(&mut desk, &mut router, press(300, 14));
        route(&mut desk, &mut router, release(300, 14));
        assert!(desk.palette_open());
        // Row 1 is "New Terminal" on the bare desk; a click runs it.
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let (ox, oy, pitch) = palette.card_text_origin();
        let deliveries = router.route(
            &compositor,
            &windows,
            press((ox + 4) as i32, (oy + pitch + 4) as i32),
        );
        desk.handle_deliveries_with_requests(&deliveries);
        route(
            &mut desk,
            &mut router,
            release((ox + 4) as i32, (oy + pitch + 4) as i32),
        );
        assert!(!desk.palette_open());
        assert_eq!(
            desk.focused_window().map(|w| w.app),
            Some(DeskApp::Terminal)
        );

        // A notice goes when clicked.
        desk.notify(NoticeLevel::Info, "hello", 100);
        let windows = desk.windows_at("", true, None, 100, &[]);
        let notice = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Notification)
            .unwrap();
        let b = notice.bounds();
        let deliveries = router.route(
            &compositor,
            &windows,
            press((b.x + 20) as i32, (b.y + 40) as i32),
        );
        desk.handle_deliveries_with_requests(&deliveries);
        route(
            &mut desk,
            &mut router,
            release((b.x + 20) as i32, (b.y + 40) as i32),
        );
        let windows = desk.windows_at("", true, None, 101, &[]);
        assert!(!windows
            .iter()
            .any(|w| w.role == DesktopWindowRole::Notification));

        // Look: click a theme row to preview, click it again to keep.
        let id = desk.launch(DeskApp::Look);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let (ox, oy, pitch) = card.card_text_origin();
        let ember_line = LookView::line_of_row(2);
        let (cx, cy) = ((ox + 40) as i32, (oy + pitch * ember_line + 4) as i32);
        let click = press(cx, cy);
        let deliveries = router.route(&compositor, &windows, click);
        desk.handle_deliveries_with_requests(&deliveries);
        route(&mut desk, &mut router, release(cx, cy));
        assert_eq!(desk.theme(), Theme::EMBER.with_accent(Theme::ACCENTS[0].1));
        assert_eq!(desk.look().theme, "Dusk");
        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, click);
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        assert_eq!(desk.look().theme, "Ember");
        assert!(matches!(
            requests.first(),
            Some(DeskRequest::SaveLook { .. })
        ));
        // A heading is no row.
        let heading = press((ox + 4) as i32, (oy + 4) as i32);
        let windows = desk.windows("", true, None);
        let deliveries = router.route(&compositor, &windows, heading);
        let (requests, _) = desk.handle_deliveries_with_requests(&deliveries);
        assert!(requests.is_empty());
        assert_eq!(desk.look().theme, "Ember");
    }

    #[test]
    fn notepad_chips_follow_its_mode() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        desk.handle_key(crate::notepad::CTRL_F);
        assert_eq!(desk.window(id).unwrap().actions()[0].0, "Next");
        desk.handle_key(crate::notepad::ESC);
        desk.handle_key(crate::notepad::CTRL_O);
        assert_eq!(desk.window(id).unwrap().actions()[0].0, "Cancel");
        desk.handle_key(crate::notepad::ESC);
        desk.window_mut(id)
            .unwrap()
            .notepad_mut()
            .unwrap()
            .load(Some("m".to_string()), "x");
        desk.handle_key(crate::notepad::CTRL_Y);
        desk.version_loaded(id, 0, 1, Some("old"), 0);
        let labels: Vec<String> = desk
            .window(id)
            .unwrap()
            .actions()
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        assert_eq!(labels, alloc::vec!["Older", "Newer", "Restore", "Back"]);
        // The Back chip is Esc.
        let (_, byte) = desk.window(id).unwrap().actions()[3].clone();
        desk.handle_app_key(id, byte);
        assert!(!desk
            .window(id)
            .unwrap()
            .notepad()
            .unwrap()
            .browsing_history());
    }

    #[test]
    fn the_welcome_card_sits_out_of_the_way_and_closes_for_good() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.show_welcome();
        assert_eq!(desk.show_welcome(), id, "only one");
        // No focus taken: typing is still a search.
        assert_eq!(desk.focus(), None);
        assert!(desk.handle_key(b't').1 && desk.palette_open());
        desk.handle_key(crate::notepad::ESC);
        // Not a cascade slot: the first Notepad opens where it always does.
        let fresh = Desk::new(1280, 800).launch(DeskApp::Notepad);
        let _ = fresh;
        let mut plain = Desk::new(1280, 800);
        let plain_id = plain.launch(DeskApp::Notepad);
        let notepad = desk.launch(DeskApp::Notepad);
        assert_eq!(
            desk.window(notepad).unwrap().bounds,
            plain.window(plain_id).unwrap().bounds
        );
        // Bottom left, above the dock; not on the dock.
        let bounds = desk.window(id).unwrap().bounds;
        let area = desk.work_area();
        assert!(bounds.bottom() <= area.bottom() && bounds.x < 100);
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert_eq!(
            dock.tabs.len(),
            DeskApp::ALL.len(),
            "Welcome is not in ALL, so not a tile"
        );
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.actions, alloc::vec!["Got it"]);
        assert_eq!(card.frame.title.as_deref(), Some("Welcome to PandaGen"));
        // The chip closes it and the kernel is told once.
        desk.raise(id);
        let (_, byte) = desk.window(id).unwrap().actions()[0].clone();
        desk.handle_app_key(id, byte);
        assert!(desk.window(id).is_none());
        assert_eq!(desk.tick(0), alloc::vec![DeskRequest::Welcomed]);
        assert!(desk.tick(1).is_empty());
    }

    #[test]
    fn spaces_hold_their_own_cards_and_the_strip_says_where_things_are() {
        let mut desk = Desk::new(1280, 800);
        let a = desk.launch(DeskApp::Notepad);
        assert_eq!(desk.space(), 0);
        // Ctrl+2: an empty space; the card is not drawn, focus is nothing,
        // the bar says so.
        assert!(desk.handle_key(crate::notepad::KEY_CTRL_1 + 1).1);
        assert_eq!(desk.space(), 1);
        assert_eq!(desk.focus(), None);
        let windows = desk.windows("", true, None);
        assert!(!windows.iter().any(|w| w.frame.view_id == a));
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        let lines = match &bar.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert_eq!(
            lines[1], " 1 .  [2]    3     4  ",
            "4 cells a space, 2 between"
        );
        assert!(bar.frame.title.as_deref().unwrap().contains("Ctrl+Space"));
        // A card opened here is here; Ctrl+Tab stays on this space.
        let b = desk.launch(DeskApp::Terminal);
        assert!(!desk.cycle_focus(), "one card on this space");
        // Ctrl+Shift+1 moves it to space 1 and follows.
        assert!(desk.handle_key(crate::notepad::KEY_CTRL_SHIFT_1).1);
        assert_eq!(desk.space(), 0);
        assert_eq!(desk.window(b).unwrap().space, 0);
        assert_eq!(desk.focus(), Some(b));
        assert!(desk.cycle_focus(), "two cards here now");
        // Raising a card on another space follows it there.
        desk.handle_key(crate::notepad::KEY_CTRL_SHIFT_1 + 3);
        assert_eq!(desk.space(), 3);
        desk.go_to_space(0);
        desk.raise(a);
        assert_eq!(desk.space(), 3, "raising a card on space 4 goes there");
        // Out of range and same-space are no-ops.
        assert!(!desk.go_to_space(SPACES));
        assert!(!desk.go_to_space(3));
        // The strip takes a click: cell 0 of the centred text is space 1.
        let strip = desk.space_strip();
        let start = services_gui_host::top_bar_centre_column(1280, strip.chars().count());
        assert_eq!(desk.space_at_column(start + 1), Some(0));
        assert_eq!(desk.space_at_column(start + 6 + 1), Some(1));
        assert_eq!(desk.space_at_column(start + 4), None, "the gap");
        assert_eq!(desk.space_at_column(0), None);
        let mut router = DesktopInputRouter::new();
        let x = ((start + 1) * 8 + 2) as i32;
        route(&mut desk, &mut router, press(x, 14));
        route(&mut desk, &mut router, release(x, 14));
        assert_eq!(desk.space(), 0);
        assert_eq!(desk.focus(), Some(b), "b is what is left on space 1");
        // The rest of the bar still opens the palette.
        route(&mut desk, &mut router, press(300, 14));
        route(&mut desk, &mut router, release(300, 14));
        assert!(desk.palette_open());
    }

    #[test]
    fn every_notice_is_kept_and_the_bar_counts_the_unseen_ones() {
        let mut desk = Desk::new(1280, 800);
        desk.notify(NoticeLevel::Info, "Saved memo", 1_000);
        let shell = alloc::vec![ShellNotice::new(NoticeLevel::Warning, "Unknown command: x")];
        let windows = desk.windows_at("12:34", true, None, 7_000, &shell);
        // The same shell notice next frame is not logged twice.
        let _ = desk.windows_at("12:34", true, None, 7_100, &shell);
        assert_eq!(desk.notice_log().len(), 2);
        assert_eq!(
            desk.notice_log()[0].1.text,
            "Unknown command: x",
            "newest first"
        );
        assert_eq!(desk.unseen_notices(), 2);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        let lines = match &bar.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert_eq!(lines[0], "2 new   12:34");
        assert_eq!(Desk::ago(7_000, 1_000), "1m ago");
        assert_eq!(Desk::ago(6_900, 1_000), "59s ago");
        assert_eq!(Desk::ago(100_000, 1_000), "16m ago");
        assert_eq!(Desk::ago(1_000_000, 0), "2h ago");

        // A click on "2 new" opens the card; looking at it clears the count.
        let mut router = DesktopInputRouter::new();
        let start = (1280 - 12 - "2 new   12:34".len() * 8) / 8;
        let x = (start * 8 + 4) as i32;
        route(&mut desk, &mut router, press(x, 14));
        route(&mut desk, &mut router, release(x, 14));
        let id = desk.focused_window().map(|w| w.id).expect("Notices opened");
        assert_eq!(desk.window(id).unwrap().app, DeskApp::Notices);
        let windows = desk.windows_at("12:34", true, None, 8_000, &shell);
        assert_eq!(desk.unseen_notices(), 0);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let rows = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(
            rows[0].contains("WARN") && rows[0].ends_with("Unknown command: x"),
            "{rows:?}"
        );
        assert!(
            rows[1].starts_with("  1m ago") && rows[1].contains("Saved memo"),
            "{rows:?}"
        );
        assert_eq!(card.actions, alloc::vec!["Clear", "Close"]);
        // Delete clears; Esc closes; the bar is just the clock again.
        desk.handle_key(crate::notepad::KEY_DELETE);
        assert!(desk.notice_log().is_empty());
        desk.handle_key(crate::notepad::ESC);
        assert!(desk.window(id).is_none());
        let windows = desk.windows_at("12:34", true, None, 9_000, &shell);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        let lines = match &bar.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert_eq!(lines[0], "12:34");
        // Not a dock tile; bounded.
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert_eq!(dock.tabs.len(), DeskApp::ALL.len());
        for i in 0..(NOTICE_LOG + 10) {
            desk.notify(NoticeLevel::Info, alloc::format!("n{i}"), 10_000 + i as u64);
        }
        assert_eq!(desk.notice_log().len(), NOTICE_LOG);
    }

    #[test]
    fn the_palette_offers_what_was_copied_before() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        for word in ["one", "two", "one"] {
            for byte in word.bytes() {
                desk.handle_key(byte);
            }
            desk.handle_key(crate::notepad::CTRL_A);
            desk.handle_key(crate::notepad::CTRL_C);
            desk.handle_key(crate::notepad::KEY_DELETE);
        }
        // "one" copied again moves to the front; no duplicate.
        assert_eq!(
            desk.clipboard_history(),
            &["one".to_string(), "two".to_string()]
        );
        // Over a Notepad the palette lists them; "two" pastes.
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"two" {
            desk.handle_key(*byte);
        }
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(listed[0].starts_with("Paste: two"), "{listed:?}");
        desk.handle_key(b'\n');
        assert_eq!(desk.window(id).unwrap().notepad().unwrap().content(), "two");
        assert_eq!(
            desk.clipboard_history()[0],
            "two",
            "pasting makes it the latest"
        );
        // Not offered with no Notepad to paste into.
        desk.close(id);
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"paste" {
            desk.handle_key(*byte);
        }
        let windows = desk.windows("", true, None);
        let palette = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Palette)
            .unwrap();
        let listed = match &palette.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(
            !listed.iter().any(|l| l.starts_with("Paste:")),
            "{listed:?}"
        );
    }

    #[test]
    fn files_previews_the_selected_file_beside_the_list_and_gathers_by_tag() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Files);
        let mut a = FileEntry::named("alpha");
        a.tags = alloc::vec!["work".to_string()];
        let mut b = FileEntry::named("beta");
        b.tags = alloc::vec!["home".to_string(), "work".to_string()];
        let c = FileEntry::named("gamma");
        desk.files_listed(id, alloc::vec![a, b, c]);
        // The tick asks for the selected file once, not every tick.
        let requests = desk.tick(0);
        assert_eq!(
            requests,
            alloc::vec![DeskRequest::PreviewFile {
                id,
                name: "alpha".to_string()
            }]
        );
        assert!(desk.tick(1).is_empty(), "already asked");
        desk.preview_loaded(id, "alpha", "first line\nsecond line\n");
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let rows = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(
            rows[0].starts_with("alpha") && rows[0].contains("| first line"),
            "{rows:?}"
        );
        assert!(
            rows[1].starts_with("beta") && rows[1].contains("| second line"),
            "{rows:?}"
        );
        assert!(
            rows[2].starts_with("gamma") && rows[2].trim_end().ends_with('|'),
            "{rows:?}"
        );
        // Moving the selection asks for the next file; until it arrives
        // the preview says so.
        desk.handle_key(crate::notepad::KEY_DOWN);
        assert_eq!(
            desk.tick(2),
            alloc::vec![DeskRequest::PreviewFile {
                id,
                name: "beta".to_string()
            }]
        );
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let rows = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(rows[0].contains("| ..."), "{rows:?}");
        // The list alone, on request.
        desk.handle_key(CTRL_U);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let rows = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(!rows[0].contains('|'), "{rows:?}");
        assert!(
            desk.tick(3).is_empty(),
            "no preview asked for while the list is alone"
        );
        assert!(card.actions.iter().any(|a| a == "Preview"));
        desk.handle_key(CTRL_U);
        // Gather by tag: All -> #home -> #work -> All.
        let files = desk.window(id).unwrap().files().unwrap();
        assert_eq!(
            files.all_tags(),
            alloc::vec!["home".to_string(), "work".to_string()]
        );
        desk.handle_key(CTRL_G);
        let files = desk.window(id).unwrap().files().unwrap();
        assert_eq!(files.tag_filter.as_deref(), Some("home"));
        assert_eq!(files.visible().len(), 1);
        assert!(files.footer().starts_with("#home only"));
        desk.handle_key(CTRL_G);
        let files = desk.window(id).unwrap().files().unwrap();
        assert_eq!(files.visible().len(), 2);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert!(
            card.actions.iter().any(|a| a == "#work"),
            "{:?}",
            card.actions
        );
        desk.handle_key(CTRL_G);
        let files = desk.window(id).unwrap().files().unwrap();
        assert_eq!(files.tag_filter, None);
        assert_eq!(files.visible().len(), 3);
    }

    #[test]
    fn the_clock_opens_now_which_asks_the_kernel_once_a_second() {
        let mut desk = Desk::new(1280, 800);
        // No Now card: no asking.
        assert!(desk.tick(0).is_empty());
        // Click the clock.
        let mut router = DesktopInputRouter::new();
        let start = (1280 - 12 - 5 * 8) / 8;
        let x = (start * 8 + 4) as i32;
        route(&mut desk, &mut router, press(x, 14));
        route(&mut desk, &mut router, release(x, 14));
        let id = desk.focused_window().map(|w| w.id).expect("Now opened");
        assert_eq!(desk.window(id).unwrap().app, DeskApp::Now);
        // It asks, and not again until a second has passed.
        assert_eq!(desk.tick(500), alloc::vec![DeskRequest::Vitals]);
        assert!(desk.tick(550).is_empty());
        desk.vitals_loaded(Vitals {
            uptime_ticks: 372_500,
            heap_used_kib: 12 * 1024,
            heap_total_kib: 32 * 1024,
            cpus_online: 4,
            cpus_total: 4,
            files: 6,
        });
        assert_eq!(desk.tick(600), alloc::vec![DeskRequest::Vitals]);
        let windows = desk.windows_at("12:34", true, None, 600, &[]);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let rows = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(rows[0].ends_with("12:34"), "{rows:?}");
        assert!(rows[1].ends_with("1h 02m"), "{rows:?}");
        assert!(rows[2].contains("12 MiB of 32 MiB"), "{rows:?}");
        assert!(rows[3].contains("4 online of 4"), "{rows:?}");
        assert!(rows[4].ends_with("6"), "{rows:?}");
        assert!(rows[5].contains("1 open on 1 space"), "{rows:?}");
        assert_eq!(Desk::uptime(6_100), "1m 01s");
        // Its chips open the other system cards.
        assert_eq!(card.actions[2], "Shortcuts");
        desk.handle_key(b'k');
        let (sheet_id, sheet_app) = desk.focused_window().map(|w| (w.id, w.app)).unwrap();
        assert_eq!(sheet_app, DeskApp::Shortcuts);
        let windows = desk.windows("", true, None);
        let card = windows
            .iter()
            .find(|w| w.frame.view_id == sheet_id)
            .unwrap();
        let rows = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert!(rows[0].contains("type on the desk"), "{rows:?}");
        assert!(
            rows.iter()
                .any(|r| r.starts_with("Save") && r.ends_with("Ctrl+S")),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|r| r.contains("Ctrl+Shift+1..4")),
            "{rows:?}"
        );
        // The sheet scrolls: Down moves the first row on; PageUp comes back.
        desk.handle_key(crate::notepad::KEY_DOWN);
        let windows = desk.windows("", true, None);
        let card = windows
            .iter()
            .find(|w| w.frame.view_id == sheet_id)
            .unwrap();
        let scrolled = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert_eq!(scrolled[0], rows[1]);
        desk.handle_key(crate::notepad::KEY_PAGE_UP);
        let windows = desk.windows("", true, None);
        let card = windows
            .iter()
            .find(|w| w.frame.view_id == sheet_id)
            .unwrap();
        let back = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines.clone(),
            _ => panic!(),
        };
        assert_eq!(back[0], rows[0]);
        // Neither is a dock tile.
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert_eq!(dock.tabs.len(), DeskApp::ALL.len());
    }

    #[test]
    fn files_hides_the_systems_dot_names() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Files);
        desk.files_listed(
            id,
            alloc::vec![FileEntry::named(".look"), FileEntry::named("memo")],
        );
        let files = desk.window(id).unwrap().files().unwrap();
        assert_eq!(files.visible().len(), 1);
        assert_eq!(files.selected().map(|e| e.name.as_str()), Some("memo"));
    }

    #[test]
    fn files_shows_tags_type_size_and_time_and_narrows_to_the_name() {
        let entry = FileEntry {
            name: "notes".to_string(),
            size: 1536,
            kind: "file",
            modified_at: 1_789_946_225,
            schema: Some("text/plain".to_string()),
            tags: alloc::vec!["work".to_string()],
            trashed: false,
            versions: 2,
        };
        let line = FilesView::line(&entry, 90);
        assert!(line.starts_with("notes"), "{line}");
        assert!(line.contains("#work") && line.contains("Text"), "{line}");
        assert!(
            line.contains("1.5 KB") && line.contains("2026-09-20 23:17"),
            "{line}"
        );
        let narrow = FilesView::line(&entry, 40);
        assert!(
            narrow.contains("1.5 KB") && !narrow.contains("2026"),
            "{narrow}"
        );
        assert_eq!(FilesView::line(&entry, 3), "not");
        assert_eq!(format_size(12), "12 B");
        assert_eq!(FileEntry::named("x").kind_label(), "File");

        let mut files = FilesView::default();
        files.entries = alloc::vec![entry];
        files.loaded = true;
        let footer = files.footer();
        assert!(
            footer.contains("Text") && footer.contains("#work"),
            "{footer}"
        );
        assert!(
            footer.contains("2 earlier") && footer.contains("written 2026"),
            "{footer}"
        );
        files.entries.clear();
        assert!(files.footer().contains("start typing"));
    }

    #[test]
    fn typing_in_files_filters_by_name_or_tag_and_recency_sorts_first() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Files);
        let mut old = FileEntry::named("alpha");
        old.modified_at = 100;
        let mut new = FileEntry::named("zeta");
        new.modified_at = 900;
        new.tags = alloc::vec!["work".to_string()];
        let mut binned = FileEntry::named("gone");
        binned.trashed = true;
        desk.files_listed(id, alloc::vec![old, new, binned]);
        let names = |desk: &Desk| -> Vec<String> {
            let files = desk.window(id).unwrap().files().unwrap();
            files
                .visible()
                .iter()
                .map(|i| files.entries[*i].name.clone())
                .collect()
        };
        // Recent first, the bin hidden.
        assert_eq!(
            names(&desk),
            alloc::vec!["zeta".to_string(), "alpha".to_string()]
        );
        // Ctrl+S: by name.
        desk.handle_key(crate::notepad::CTRL_S);
        assert_eq!(
            names(&desk),
            alloc::vec!["alpha".to_string(), "zeta".to_string()]
        );
        // Typing filters by tag too; Backspace and Esc undo it.
        for byte in b"wor" {
            desk.handle_key(*byte);
        }
        assert_eq!(names(&desk), alloc::vec!["zeta".to_string()]);
        let footer = desk.window(id).unwrap().files().unwrap().footer();
        assert!(footer.starts_with("Filter: wor_   1 of 2"), "{footer}");
        desk.handle_key(crate::notepad::ESC);
        assert_eq!(names(&desk).len(), 2);
        // The bin shows only what is in it, with its own chips.
        desk.handle_key(CTRL_B);
        assert_eq!(names(&desk), alloc::vec!["gone".to_string()]);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.frame.title.as_deref(), Some("Files - Bin"));
        assert_eq!(
            card.actions,
            alloc::vec!["Restore", "Remove", "A-Z", "Files"]
        );
        // Enter restores; Delete asks, then removes for good.
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::TrashFile {
                id,
                name: "gone".to_string(),
                trashed: false
            })
        );
        desk.handle_key(crate::notepad::KEY_DELETE);
        assert!(desk
            .window(id)
            .unwrap()
            .files()
            .unwrap()
            .footer()
            .starts_with("Remove gone for good?"));
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::PurgeFile {
                id,
                name: "gone".to_string()
            })
        );
    }

    #[test]
    fn files_creates_renames_tags_and_bins_through_footer_prompts() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Files);
        desk.files_listed(
            id,
            alloc::vec![FileEntry::named("a"), FileEntry::named("b")],
        );
        // Ctrl+N: a name, Enter. No suffix.
        desk.handle_key(crate::notepad::CTRL_N);
        for byte in b"memo" {
            desk.handle_key(*byte);
        }
        let footer = desk.window(id).unwrap().files().unwrap().footer();
        assert!(footer.starts_with("New: memo_"), "{footer}");
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::CreateFile {
                id,
                name: "memo".to_string()
            })
        );
        // Delete bins without asking: it is reversible.
        desk.handle_key(crate::notepad::KEY_DOWN);
        let (request, _) = desk.handle_key(crate::notepad::KEY_DELETE);
        assert_eq!(
            request,
            Some(DeskRequest::TrashFile {
                id,
                name: "b".to_string(),
                trashed: true
            })
        );
        // Rename starts from the current name; an unchanged name is a no-op.
        desk.handle_key(CTRL_E);
        assert!(desk
            .window(id)
            .unwrap()
            .files()
            .unwrap()
            .footer()
            .starts_with("Rename to: b_"));
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(request, None);
        desk.handle_key(CTRL_E);
        desk.handle_key(crate::notepad::BACKSPACE);
        desk.handle_key(b'c');
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::RenameFile {
                id,
                from: "b".to_string(),
                to: "c".to_string()
            })
        );
        // Tags: words add, -word removes, # is optional.
        desk.handle_key(CTRL_K);
        for byte in b"Work #draft -old" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::TagFile {
                id,
                name: "b".to_string(),
                add: alloc::vec!["work".to_string(), "draft".to_string()],
                remove: alloc::vec!["old".to_string()],
            })
        );
        // Ctrl+R lists again; a refresh keeps the selection by name.
        assert_eq!(
            desk.handle_key(CTRL_R).0,
            Some(DeskRequest::ListFiles { id })
        );
        desk.files_listed(
            id,
            alloc::vec![
                FileEntry::named("a"),
                FileEntry::named("b"),
                FileEntry::named("memo")
            ],
        );
        assert_eq!(
            desk.window(id)
                .unwrap()
                .files()
                .unwrap()
                .selected()
                .map(|e| e.name.as_str()),
            Some("b")
        );
    }

    #[test]
    fn header_chips_run_the_apps_own_keys() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        desk.handle_key(b'x');
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(
            card.actions,
            alloc::vec!["Save", "Save as", "Open", "Find", "History", "Split"]
        );
        let save = card.action_rects()[0].expect("the Save chip is placed");
        let compositor = Compositor::new();
        let deliveries = router.route(
            &compositor,
            &windows,
            press((save.x + 3) as i32, (save.y + 3) as i32),
        );
        let (requests, changed) = desk.handle_deliveries_with_requests(&deliveries);
        assert!(changed);
        // An unnamed document: Save opens the name prompt, which asks for
        // the listing to complete against.
        assert_eq!(requests, alloc::vec![DeskRequest::ListFiles { id }]);
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert!(
            notepad.footer().starts_with("Save as:"),
            "{}",
            notepad.footer()
        );

        let files = desk.launch(DeskApp::Files);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == files).unwrap();
        assert_eq!(
            card.actions,
            alloc::vec!["New", "Rename", "Tag", "Bin it", "All", "Recent", "List", "Bin"]
        );
        let terminal = desk.launch(DeskApp::Terminal);
        let windows = desk.windows("", true, None);
        let card = windows
            .iter()
            .find(|w| w.frame.view_id == terminal)
            .unwrap();
        assert!(card.actions.is_empty());
    }

    /// Split (GFX-074): the chip, Ctrl+D or the palette row puts the
    /// focused Notepad's document in a second card, snapped beside the
    /// first; typing in either shows in both; closing one asks nothing.
    #[test]
    fn split_puts_one_document_in_two_cards_side_by_side() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        desk.handle_key(b'h');
        let chips = desk.window(id).unwrap().actions();
        assert!(chips
            .iter()
            .any(|(label, key)| label == "Split" && *key == crate::notepad::CTRL_D));
        assert_eq!(desk.handle_key(crate::notepad::CTRL_D), (None, true));
        let pads: Vec<ViewId> = desk
            .windows
            .iter()
            .filter(|w| w.app == DeskApp::Notepad)
            .map(|w| w.id)
            .collect();
        assert_eq!(pads.len(), 2);
        let twin = pads.into_iter().find(|p| *p != id).unwrap();
        assert_eq!(desk.focus(), Some(twin));
        let area = desk.work_area();
        let left = desk.window(id).unwrap().bounds;
        let right = desk.window(twin).unwrap().bounds;
        assert_eq!((left.x, left.width), (area.x, area.width / 2));
        assert_eq!(right.x, area.x + area.width / 2);
        assert!(desk
            .window(id)
            .unwrap()
            .notepad()
            .unwrap()
            .shares_with(desk.window(twin).unwrap().notepad().unwrap()));
        // Typed in the new card, shown in both.
        desk.handle_key(b'i');
        assert_eq!(desk.window(id).unwrap().notepad().unwrap().content(), "hi");
        assert_eq!(
            desk.window(twin).unwrap().notepad().unwrap().content(),
            "hi"
        );
        // Unsaved, but shared: Ctrl+W closes at once and the text stays.
        desk.handle_key(crate::notepad::CTRL_W);
        assert!(desk.window(twin).is_none());
        let kept = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(kept.content(), "hi");
        assert!(!kept.shared());
        // Ctrl+D on a Terminal is the Terminal's.
        desk.launch(DeskApp::Terminal);
        assert!(matches!(
            desk.handle_key(crate::notepad::CTRL_D),
            (Some(DeskRequest::Terminal(0x04)), true)
        ));
    }

    /// The Calculator (GFX-075): on the dock and in the palette; its keys
    /// are clicked in the content and typed on the keyboard, and the card
    /// shows the expression, the result and the tape.
    #[test]
    fn the_calculator_takes_clicks_on_its_keys_and_typed_keys_alike() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let compositor = Compositor::new();
        assert!(DeskApp::ALL.contains(&DeskApp::Calculator));
        assert!(PaletteAction::ALL.contains(&PaletteAction::Calculator));
        let id = desk.launch(DeskApp::Calculator);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.actions, alloc::vec!["Clear", "Close"]);
        assert!(matches!(card.frame.content, ViewContent::Graphics { .. }));
        let (ox, oy, _) = card.card_text_origin();
        let bounds = desk.window(id).unwrap().bounds;
        let (w, h) = Desk::canvas_size(bounds);
        let layout = crate::calculator::Layout::new(w, h);
        // Click the centres of "7", "+", "8", "=" as the layout places them.
        let click = |desk: &mut Desk, router: &mut DesktopInputRouter, key: char| {
            let windows = desk.windows("", true, None);
            let cell = layout.key_rect(key).expect("a key on the grid");
            let x = (ox as u32 + cell.x + cell.width / 2) as i32;
            let y = (oy as u32 + cell.y + cell.height / 2) as i32;
            let deliveries = router.route(&compositor, &windows, press(x, y));
            desk.handle_deliveries_with_requests(&deliveries);
            let deliveries = router.route(&compositor, &windows, release(x, y));
            desk.handle_deliveries_with_requests(&deliveries);
        };
        for key in ['7', '+', '8', '='] {
            click(&mut desk, &mut router, key);
        }
        let calc = match &desk.window(id).unwrap().state {
            AppState::Calculator(calc) => calc.clone(),
            _ => panic!(),
        };
        assert_eq!(calc.shown(), "15");
        assert_eq!(
            calc.tape().first().map(|(e, r)| (e.as_str(), r.as_str())),
            Some(("7+8", "15"))
        );
        // The pointer over "=" outlines it: one more op than at rest.
        let cell = layout.key_rect('=').unwrap();
        let at_rest = calc
            .ui(
                w,
                h,
                crate::widgets::Palette::from_theme(&Theme::DEFAULT),
                None,
            )
            .into_ops()
            .len();
        let hovered = calc
            .ui(
                w,
                h,
                crate::widgets::Palette::from_theme(&Theme::DEFAULT),
                Some((cell.x as i32 + 2, cell.y as i32 + 2)),
            )
            .into_ops()
            .len();
        assert_eq!(hovered, at_rest + 1);
        // Typed: the result carries on. Esc clears rather than closing.
        desk.handle_key(b'*');
        desk.handle_key(b'2');
        desk.handle_key(b'\n');
        let calc = match &desk.window(id).unwrap().state {
            AppState::Calculator(calc) => calc.clone(),
            _ => panic!(),
        };
        assert_eq!(calc.shown(), "30");
        desk.handle_key(crate::notepad::ESC);
        assert!(desk.window(id).is_some());
        desk.handle_key(crate::notepad::CTRL_W);
        assert!(desk.window(id).is_none());
    }

    /// The Calendar (GFX-076): opened from the palette it asks for the
    /// listing, marks the days that have a note, opens a day's note in a
    /// Notepad named by the day, and lists again after a save.
    #[test]
    fn the_calendar_marks_noted_days_and_opens_a_day_as_a_document() {
        let mut desk = Desk::new(1280, 800);
        desk.set_today(Some(Date::new(2026, 9, 24)));
        assert!(DeskApp::ALL.contains(&DeskApp::Calendar));
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"calen" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        let id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(desk.window(id).unwrap().app, DeskApp::Calendar);
        assert_eq!(request, Some(DeskRequest::ListFiles { id }));
        desk.files_listed(
            id,
            alloc::vec![FileEntry::named("2026-09-03"), FileEntry::named("memo")],
        );
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let texts: Vec<String> = match &card.frame.content {
            ViewContent::Graphics { ops } => ops
                .iter()
                .filter_map(|op| match op {
                    view_types::DrawOp::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect(),
            _ => panic!("a drawn card"),
        };
        assert!(texts.contains(&"September 2026".to_string()), "{texts:?}");
        assert!(texts.contains(&"Today  Thursday 24 September 2026".to_string()));
        assert_eq!(card.actions[0], "Today");
        // Enter on today: a new Notepad named by the day, nothing to read.
        let (request, changed) = desk.handle_key(b'\n');
        assert!(changed && request.is_none());
        let pad_id = desk.focused_window().map(|w| w.id).unwrap();
        let pad = desk.window(pad_id).unwrap().notepad().unwrap();
        assert_eq!(pad.path().as_deref(), Some("2026-09-24"));
        assert!(!pad.is_dirty());
        // Its save makes the Calendar list again, once.
        let effect = NotepadEffect::Save {
            path: "2026-09-24".to_string(),
            content: "x".to_string(),
        };
        desk.io_done(pad_id, &effect, Ok(None), 10);
        assert_eq!(desk.tick(11), alloc::vec![DeskRequest::ListFiles { id }]);
        assert!(desk.tick(12).is_empty());
        // A day with a note opens by reading it.
        desk.raise(id);
        desk.focus = Some(id);
        for _ in 0..3 {
            assert!(desk.handle_key(crate::notepad::KEY_UP).1);
        }
        let (request, _) = desk.handle_key(b'\n');
        let opened = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id: opened,
                effect: NotepadEffect::Open {
                    path: "2026-09-03".to_string()
                }
            })
        );
    }

    /// The Timer (GFX-077): it runs on `tick`, its title shows the time
    /// while running, and a countdown that is up becomes a notice in the
    /// log -- once -- whether or not the card is on this space.
    #[test]
    fn a_countdown_that_is_up_becomes_a_notice_from_any_space() {
        let mut desk = Desk::new(1280, 800);
        assert!(DeskApp::ALL.contains(&DeskApp::Timer));
        let id = desk.launch(DeskApp::Timer);
        desk.tick(1_000);
        // Preset 1: one minute, running at once; the chip says Pause.
        assert!(desk.handle_key(b'1').1);
        assert_eq!(desk.window(id).unwrap().actions()[0].0, "Pause");
        assert_eq!(desk.tick(1_100), alloc::vec![DeskRequest::Repaint]);
        assert!(desk.tick(1_105).is_empty(), "ten a second, not a hundred");
        let windows = desk.windows_at("", true, None, 1_100, &[]);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.frame.title.as_deref(), Some("Timer - 00:58.9"));
        // Move to another space; the countdown still runs and is heard.
        desk.go_to_space(2);
        let before = desk.notice_log.len();
        desk.tick(1_000 + 60 * crate::timer::HZ + 5);
        assert_eq!(desk.notice_log.len(), before + 1);
        assert_eq!(desk.notice_log[0].1.text, "Timer: 1 minute up");
        desk.tick(1_000 + 60 * crate::timer::HZ + 500);
        assert_eq!(desk.notice_log.len(), before + 1, "said once");
        // Back on space 1, the card says Done and Start would run it again.
        desk.go_to_space(0);
        assert_eq!(desk.window(id).unwrap().actions()[0].0, "Start");
        desk.handle_key(crate::notepad::CTRL_W);
        assert!(desk.window(id).is_none());
    }

    /// Tiles (GFX-078): on the dock and in the palette; arrows and the
    /// buttons both play, and a click on a button is a move.
    #[test]
    fn tiles_plays_by_arrow_and_by_button() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let compositor = Compositor::new();
        assert!(DeskApp::ALL.contains(&DeskApp::Tiles));
        let id = desk.launch(DeskApp::Tiles);
        let count = |desk: &Desk| match &desk.window(id).unwrap().state {
            AppState::Tiles(game) => game.cells().iter().flatten().filter(|v| **v != 0).count(),
            _ => panic!(),
        };
        assert_eq!(count(&desk), 2);
        let board = |desk: &Desk| match &desk.window(id).unwrap().state {
            AppState::Tiles(game) => *game.cells(),
            _ => panic!(),
        };
        let before = board(&desk);
        // An arrow that moves something changes the board (two equal
        // tiles may merge, so the count is not the measure).
        let mut moved = false;
        for key in [
            crate::notepad::KEY_LEFT,
            crate::notepad::KEY_UP,
            crate::notepad::KEY_RIGHT,
            crate::notepad::KEY_DOWN,
        ] {
            if desk.handle_key(key).1 {
                moved = true;
                break;
            }
        }
        assert!(moved);
        assert_ne!(board(&desk), before);
        // The New button, clicked: two tiles again.
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.actions, alloc::vec!["New", "Close"]);
        assert!(matches!(card.frame.content, ViewContent::Graphics { .. }));
        let (ox, oy, _) = card.card_text_origin();
        let (w, h) = Desk::canvas_size(desk.window(id).unwrap().bounds);
        let new = crate::game::GameLayout::new(w, h).buttons[4];
        let x = (ox as u32 + new.x + new.width / 2) as i32;
        let y = (oy as u32 + new.y + new.height / 2) as i32;
        let deliveries = router.route(&compositor, &windows, press(x, y));
        desk.handle_deliveries_with_requests(&deliveries);
        let deliveries = router.route(&compositor, &windows, release(x, y));
        desk.handle_deliveries_with_requests(&deliveries);
        assert_eq!(count(&desk), 2);
        desk.handle_key(crate::notepad::ESC);
        assert!(desk.window(id).is_none());
    }

    /// Tasks (GFX-079): opening asks for the `tasks` document, the card
    /// shows it with the selected row highlighted, a change saves itself
    /// quietly a second later, and the chips follow the footer prompt.
    #[test]
    fn tasks_is_a_document_that_saves_itself_quietly() {
        let mut desk = Desk::new(1280, 800);
        assert!(DeskApp::ALL.contains(&DeskApp::Tasks));
        desk.handle_key(KEY_CTRL_SPACE);
        for byte in b"tasks" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        let id = desk.focused_window().map(|w| w.id).unwrap();
        assert_eq!(
            request,
            Some(DeskRequest::PreviewFile {
                id,
                name: "tasks".to_string()
            })
        );
        desk.preview_loaded(id, "tasks", "[ ] one\n[x] two\n");
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert!(matches!(card.frame.content, ViewContent::Graphics { .. }));
        assert_eq!(card.actions, alloc::vec!["Add", "Done", "Remove", "Close"]);
        // Add through the footer: the chips become Add / Cancel.
        desk.tick(10);
        desk.handle_key(b'a');
        assert_eq!(desk.window(id).unwrap().actions()[1].0, "Cancel");
        for byte in b"three" {
            desk.handle_key(*byte);
        }
        desk.handle_key(b'\n');
        // Nothing yet; a second later, a quiet save of the whole document.
        assert!(desk.tick(50).is_empty());
        let requests = desk.tick(10 + crate::tasks::SAVE_AFTER_TICKS);
        assert_eq!(
            requests,
            alloc::vec![DeskRequest::Io {
                id,
                effect: NotepadEffect::Save {
                    path: "tasks".to_string(),
                    content: "[ ] one\n[x] two\n[ ] three\n".to_string()
                }
            }]
        );
        let before = desk.notice_log.len();
        let effect = match &requests[0] {
            DeskRequest::Io { effect, .. } => effect.clone(),
            _ => panic!(),
        };
        desk.io_done(id, &effect, Ok(None), 200);
        assert_eq!(desk.notice_log.len(), before, "a quiet save");
        // The dock tile asks for the document too.
        desk.handle_key(crate::notepad::CTRL_W);
        assert!(desk.window(id).is_none());
        let (request, _) = desk.handle_key(KEY_CTRL_SPACE);
        assert!(request.is_none());
        desk.handle_key(crate::notepad::ESC);
        let fresh = desk.launch(DeskApp::Tasks);
        assert!(matches!(
            DeskApp::Tasks.launch_request(fresh),
            Some(DeskRequest::PreviewFile { .. })
        ));
    }

    /// Sketch (GFX-080): press, move, release draws a stroke in the
    /// canvas's own pixels; the card's content is graphics; the drawing
    /// saves itself as a document a second later.
    #[test]
    fn sketch_draws_a_stroke_with_the_pointer_and_keeps_it_as_a_document() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        assert!(DeskApp::ALL.contains(&DeskApp::Sketch));
        let id = desk.launch(DeskApp::Sketch);
        assert!(matches!(
            DeskApp::Sketch.launch_request(id),
            Some(DeskRequest::PreviewFile { name, .. }) if name == "sketch"
        ));
        desk.preview_loaded(id, "sketch", "2: 1,1 2,2\n");
        desk.tick(10);
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert!(matches!(card.frame.content, ViewContent::Graphics { .. }));
        assert_eq!(
            card.actions,
            alloc::vec!["Colour", "Undo", "Clear", "Close"]
        );
        let (ox, oy, _) = card.card_text_origin();
        let (x0, y0) = ((ox + 20) as i32, (oy + 30) as i32);
        route(&mut desk, &mut router, press(x0, y0));
        route(
            &mut desk,
            &mut router,
            moved(x0 + 40, y0 + 10, PointerButtons::PRIMARY),
        );
        route(
            &mut desk,
            &mut router,
            moved(x0 + 80, y0 + 10, PointerButtons::PRIMARY),
        );
        route(&mut desk, &mut router, release(x0 + 80, y0 + 10));
        let strokes = match &desk.window(id).unwrap().state {
            AppState::Sketch(sketch) => sketch.strokes().to_vec(),
            _ => panic!(),
        };
        assert_eq!(strokes.len(), 2, "the loaded one and the drawn one");
        assert_eq!(
            strokes[1].points,
            alloc::vec![(20, 30), (60, 40), (100, 40)]
        );
        // Moving with the button up draws nothing more.
        route(
            &mut desk,
            &mut router,
            moved(x0 + 90, y0 + 50, PointerButtons::none()),
        );
        let strokes = match &desk.window(id).unwrap().state {
            AppState::Sketch(sketch) => sketch.strokes().to_vec(),
            _ => panic!(),
        };
        assert_eq!(strokes[1].points.len(), 3);
        // The drawing saves itself, quietly, as the `sketch` document.
        let requests = desk.tick(10 + crate::sketch::SAVE_AFTER_TICKS + 1);
        assert!(matches!(
            requests.as_slice(),
            [DeskRequest::Io { effect: NotepadEffect::Save { path, content }, .. }]
                if path == "sketch" && content.starts_with("2: 1,1 2,2\n0: 20,30 60,40 100,40\n")
        ));
        // Colour, undo, and the chips.
        desk.handle_key(b'c');
        desk.handle_key(b'z');
        let strokes = match &desk.window(id).unwrap().state {
            AppState::Sketch(sketch) => sketch.strokes().to_vec(),
            _ => panic!(),
        };
        assert_eq!(strokes.len(), 1);
        desk.handle_key(crate::notepad::CTRL_W);
        assert!(desk.window(id).is_none());
    }

    /// Every dock app has an icon, and the dock's tiles carry them (GFX-084).
    #[test]
    fn every_dock_tile_carries_its_apps_icon() {
        let mut desk = Desk::new(1280, 800);
        for app in DeskApp::ALL {
            let icon = app.icon().expect("an icon for a dock app");
            assert!(icon.iter().any(|row| *row != 0), "{app:?} is blank");
        }
        assert_eq!(DeskApp::Welcome.icon(), None);
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(dock.tabs.iter().all(|t| t.icon.is_some()));
        assert_eq!(dock.tabs[0].icon, DeskApp::Notepad.icon());
    }

    #[test]
    fn the_history_chip_asks_the_kernel_and_the_answer_reaches_the_notepad() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        desk.window_mut(id)
            .unwrap()
            .notepad_mut()
            .unwrap()
            .load(Some("memo".to_string()), "now");
        let (request, _) = desk.handle_key(crate::notepad::CTRL_Y);
        assert_eq!(
            request,
            Some(DeskRequest::ReadVersion {
                id,
                name: "memo".to_string(),
                index: 0
            })
        );
        desk.version_loaded(id, 0, 1, Some("before"), 0);
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert!(notepad.browsing_history());
        assert_eq!(notepad.content(), "before");
        // Enter restores: a Save request for what is shown.
        let (request, _) = desk.handle_key(b'\n');
        assert_eq!(
            request,
            Some(DeskRequest::Io {
                id,
                effect: NotepadEffect::Save {
                    path: "memo".to_string(),
                    content: "before".to_string()
                }
            })
        );
    }

    #[test]
    fn a_landed_save_raises_a_notice_that_expires() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        let effect = NotepadEffect::Save {
            path: "n.txt".to_string(),
            content: String::new(),
        };
        desk.io_done(id, &effect, Ok(None), 1000);
        let windows = desk.windows_at("", true, None, 1000, &[]);
        let notice = windows
            .iter()
            .find(|w| w.role == DesktopWindowRole::Notification)
            .expect("a notice card");
        assert!(
            matches!(&notice.frame.content, ViewContent::TextBuffer { lines } if lines[0] == "Saved n.txt")
        );
        assert!(!notice.closable);

        let windows = desk.windows_at("", true, None, 1000 + NOTICE_TTL_TICKS + 1, &[]);
        assert!(
            !windows
                .iter()
                .any(|w| w.role == DesktopWindowRole::Notification),
            "the notice did not expire"
        );

        // The workspace's own notices show the same way.
        let shell = alloc::vec![ShellNotice::new(
            NoticeLevel::Info,
            "Switching display to desk."
        )];
        let windows = desk.windows_at("", true, None, 5000, &shell);
        assert!(windows
            .iter()
            .any(|w| w.role == DesktopWindowRole::Notification));
    }

    #[test]
    fn the_empty_desk_has_a_top_bar_and_a_dock_and_nothing_else() {
        let mut desk = Desk::new(1280, 800);
        let windows = desk.windows("12:34", true, None);
        assert_eq!(windows.len(), 2);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert!(
            matches!(&bar.frame.content, ViewContent::TextBuffer { lines } if lines[0] == "12:34")
        );
        assert!(windows.iter().any(|w| w.style == WindowStyle::Dock));
    }

    #[test]
    fn clicking_the_dock_tile_launches_notepad_once_and_then_focuses_it() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        // The tile row is centred in the pill; the first tile's centre is
        // 20px into that row.
        let pill = dock.bounds();
        let first_x = (pill.x + (pill.width - dock_row_width()) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;

        assert!(route(&mut desk, &mut router, press(first_x, y)));
        route(&mut desk, &mut router, release(first_x, y));
        assert_eq!(desk.window_count(), 1);
        assert_eq!(desk.focused_window().map(|w| w.app), Some(DeskApp::Notepad));

        // A second click does not open a second Notepad.
        route(&mut desk, &mut router, press(first_x, y));
        route(&mut desk, &mut router, release(first_x, y));
        assert_eq!(desk.window_count(), 1);
    }

    #[test]
    fn hovering_a_dock_tile_lights_it_and_leaving_the_dock_clears_it() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let windows = desk.windows("", true, None);
        let pill = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap()
            .bounds();
        let first_x = (pill.x + (pill.width - dock_row_width()) / 2 + 20) as i32;
        let y = (pill.y + pill.height / 2) as i32;

        assert!(route(
            &mut desk,
            &mut router,
            moved(first_x, y, PointerButtons::none())
        ));
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(dock.tabs[0].hovered && !dock.tabs[1].hovered);

        route(
            &mut desk,
            &mut router,
            moved(10, 300, PointerButtons::none()),
        );
        let windows = desk.windows("", true, None);
        let dock = windows
            .iter()
            .find(|w| w.style == WindowStyle::Dock)
            .unwrap();
        assert!(!dock.tabs[0].hovered);
    }

    #[test]
    fn a_card_drags_by_its_header_and_stays_on_the_desk() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;

        let grab = ((before.x + 100) as i32, (before.y + 10) as i32);
        drag(&mut desk, &mut router, grab, (grab.0 + 50, grab.1 + 40));
        let after = desk.window(id).unwrap().bounds;
        assert_eq!((after.x, after.y), (before.x + 50, before.y + 40));

        // Dragging far off-screen clamps to the desk.
        drag(
            &mut desk,
            &mut router,
            (after.x as i32 + 100, after.y as i32 + 10),
            (1200, 700),
        );
        let clamped = desk.window(id).unwrap().bounds;
        assert!(clamped.right() <= 1280 && clamped.bottom() <= 800);
    }

    #[test]
    fn a_drag_to_the_edge_snaps_and_the_next_drag_unsnaps() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;
        let area = desk.work_area();

        // To the left edge: the left half.
        drag(
            &mut desk,
            &mut router,
            ((before.x + 100) as i32, (before.y + 10) as i32),
            (2, 300),
        );
        let snapped = desk.window(id).unwrap().bounds;
        assert_eq!(
            (snapped.x, snapped.width),
            (0, area.width / 2),
            "not the left half"
        );
        assert_eq!(snapped.height, area.height);

        // To the top: the whole work area.
        drag(
            &mut desk,
            &mut router,
            (100, (snapped.y + 10) as i32),
            (600, TOP_BAR_HEIGHT as i32 + 2),
        );
        let maxed = desk.window(id).unwrap().bounds;
        assert_eq!(maxed, area, "not maximised");

        // Dragging it away restores the original size under the pointer.
        drag(
            &mut desk,
            &mut router,
            (600, (maxed.y + 10) as i32),
            (640, 400),
        );
        let restored = desk.window(id).unwrap().bounds;
        assert_eq!(
            (restored.width, restored.height),
            (before.width, before.height)
        );
    }

    #[test]
    fn the_corner_grip_resizes_within_limits() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        let before = desk.window(id).unwrap().bounds;
        let grip = ((before.right() - 4) as i32, (before.bottom() - 4) as i32);

        drag(&mut desk, &mut router, grip, (grip.0 - 100, grip.1 - 60));
        let smaller = desk.window(id).unwrap().bounds;
        assert_eq!(
            (smaller.width, smaller.height),
            (before.width - 100, before.height - 60)
        );

        // Never below the minimum, whatever the pointer does.
        let grip = ((smaller.right() - 4) as i32, (smaller.bottom() - 4) as i32);
        drag(&mut desk, &mut router, grip, (10, 10));
        let floor = desk.window(id).unwrap().bounds;
        assert_eq!((floor.width, floor.height), MIN_CARD_SIZE);
    }

    #[test]
    fn clicking_in_the_text_places_the_caret_and_the_wheel_scrolls() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let id = desk.launch(DeskApp::Notepad);
        for i in 0..60 {
            for byte in alloc::format!("line {i}").bytes() {
                desk.handle_key(byte);
            }
            desk.handle_key(b'\n');
        }
        let bounds = desk.window(id).unwrap().bounds;
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let (ox, oy, pitch) = card.card_text_origin();

        // Wheel up scrolls the view back towards the top.
        let rows = Desk::card_rows(bounds);
        route(
            &mut desk,
            &mut router,
            wheel((ox + 40) as i32, (oy + 40) as i32, 10),
        );
        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        let first_shown = match &card.frame.content {
            ViewContent::TextBuffer { lines } => lines[0].clone(),
            _ => panic!(),
        };
        assert_ne!(
            first_shown,
            alloc::format!("line {}", 61 - rows),
            "the wheel did not scroll"
        );

        // Click on the third visible line, column 3.
        let (x, y) = ((ox + 8 * 3 + 2) as i32, (oy + pitch * 2 + 5) as i32);
        route(&mut desk, &mut router, press(x, y));
        route(&mut desk, &mut router, release(x, y));
        let notepad = desk.window(id).unwrap().notepad().unwrap();
        assert_eq!(
            notepad.viewport_cursor(),
            Some((2, 3)),
            "the caret did not follow the click"
        );
    }

    #[test]
    fn the_close_glyph_closes_and_focus_falls_to_the_next_card() {
        let mut desk = Desk::new(1280, 800);
        let mut router = DesktopInputRouter::new();
        let first = desk.launch(DeskApp::Notepad);
        let second = desk.launch(DeskApp::Notepad);
        assert_eq!(desk.focus(), Some(second));

        let windows = desk.windows("", true, None);
        let card = windows.iter().find(|w| w.frame.view_id == second).unwrap();
        let close = card.close_rect().unwrap();
        let (x, y) = (
            (close.x + close.width / 2) as i32,
            (close.y + close.height / 2) as i32,
        );
        route(&mut desk, &mut router, press(x, y));
        assert_eq!(desk.window_count(), 1);
        assert_eq!(desk.focus(), Some(first));
    }

    #[test]
    fn ctrl_tab_shows_the_overview_and_letting_go_picks() {
        let mut desk = Desk::new(1280, 800);
        let a = desk.launch(DeskApp::Notepad);
        let b = desk.launch(DeskApp::Terminal);
        let c = desk.launch(DeskApp::Notepad);
        assert_eq!(desk.focus(), Some(c));
        // Ctrl+Tab: the overview, with the next card (b, just under c)
        // highlighted; the real cards are hidden, the mini cards are drawn
        // with the windows' own ids.
        assert!(desk.handle_key(KEY_CTRL_TAB).1);
        assert!(desk.overview_open());
        assert_eq!(desk.focus(), Some(c), "nothing picked yet");
        let windows = desk.windows("", true, None);
        let minis: Vec<&DesktopWindow> = windows
            .iter()
            .filter(|w| w.style == WindowStyle::Card && w.role == DesktopWindowRole::Main)
            .collect();
        assert_eq!(minis.len(), 3);
        assert!(minis.iter().all(|w| w.bounds().width == OVERVIEW_CARD.0));
        let ringed: Vec<ViewId> = minis
            .iter()
            .filter(|w| w.focused)
            .map(|w| w.frame.view_id)
            .collect();
        assert_eq!(ringed, alloc::vec![b]);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert!(bar.frame.title.as_deref().unwrap().starts_with("Overview"));
        // Typing is swallowed; Ctrl+Tab again moves on to a; release picks.
        assert_eq!(desk.handle_key(b'x'), (None, false));
        desk.handle_key(KEY_CTRL_TAB);
        assert!(desk.handle_key(crate::notepad::KEY_CTRL_RELEASED).1);
        assert!(!desk.overview_open());
        assert_eq!(desk.focus(), Some(a));
        // Esc leaves things as they were.
        desk.handle_key(KEY_CTRL_TAB);
        desk.handle_key(crate::notepad::ESC);
        assert_eq!(desk.focus(), Some(a));
        // A tucked card on another space is in the overview and comes
        // back on its own space when picked.
        desk.tuck(a);
        desk.go_to_space(2);
        assert!(desk.handle_key(KEY_CTRL_TAB).1);
        let order = desk.overview_order();
        assert_eq!(order.len(), 3, "tucked cards are in the overview");
        // Nothing is on space 3, so every card is "elsewhere", by height:
        // a was raised last, so it is first even though it is tucked.
        assert_eq!(order[0], a);
        let target = order.iter().position(|id| *id == a).unwrap();
        while desk.overview != Some(target) {
            desk.handle_key(KEY_CTRL_TAB);
        }
        desk.handle_key(crate::notepad::KEY_CTRL_RELEASED);
        assert_eq!(desk.focus(), Some(a));
        assert_eq!(desk.space(), 0);
        assert!(!desk.window(a).unwrap().tucked);
        // A click on a mini card picks it.
        desk.handle_key(KEY_CTRL_TAB);
        let windows = desk.windows("", true, None);
        let mini_c = windows
            .iter()
            .find(|w| w.frame.view_id == c)
            .unwrap()
            .bounds();
        let mut router = DesktopInputRouter::new();
        let (x, y) = ((mini_c.x + 40) as i32, (mini_c.y + 60) as i32);
        route(&mut desk, &mut router, press(x, y));
        route(&mut desk, &mut router, release(x, y));
        assert!(!desk.overview_open());
        assert_eq!(desk.focus(), Some(c));
        // With nothing open there is no overview.
        let mut empty = Desk::new(1280, 800);
        assert!(!empty.handle_key(KEY_CTRL_TAB).1);
        // Letting go of Ctrl with no overview is nothing.
        assert_eq!(
            empty.handle_key(crate::notepad::KEY_CTRL_RELEASED),
            (None, false)
        );
    }

    #[test]
    fn terminal_keys_go_to_the_console_and_its_view_fills_the_card() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Terminal);
        let (request, changed) = desk.handle_key(b'h');
        assert_eq!(request, Some(DeskRequest::Terminal(b'h')));
        assert!(changed);

        let view = TerminalView {
            lines: alloc::vec!["PandaGen Workspace".to_string(), "WS > h".to_string()],
            cursor: Some((1, 6)),
            status: "WS".to_string(),
        };
        let windows = desk.windows("", true, Some(&view));
        let card = windows.iter().find(|w| w.frame.view_id == id).unwrap();
        assert_eq!(card.frame.title.as_deref(), Some("Terminal"));
        assert_eq!(card.frame.cursor.map(|c| (c.line, c.column)), Some((1, 6)));

        // Ctrl+W closes the console card rather than reaching the console.
        let (request, _) = desk.handle_key(crate::notepad::CTRL_W);
        assert_eq!(request, None);
        assert_eq!(desk.window_count(), 0);
    }

    #[test]
    fn keys_go_to_the_focused_notepad_and_io_comes_back_as_a_request() {
        let mut desk = Desk::new(1280, 800);
        let id = desk.launch(DeskApp::Notepad);
        for byte in b"hi" {
            let (request, changed) = desk.handle_key(*byte);
            assert!(request.is_none() && changed);
        }
        let (request, _) = desk.handle_key(crate::notepad::CTRL_S);
        assert_eq!(
            request,
            Some(DeskRequest::ListFiles { id }),
            "the first save asks for a name, and for the names to complete against"
        );
        for byte in b"n.txt" {
            desk.handle_key(*byte);
        }
        let (request, _) = desk.handle_key(b'\n');
        match request {
            Some(DeskRequest::Io { id: got, effect }) => {
                assert_eq!(got, id);
                assert_eq!(
                    effect,
                    NotepadEffect::Save {
                        path: "n.txt".to_string(),
                        content: "hi".to_string()
                    }
                );
                desk.io_done(id, &effect, Ok(None), 0);
            }
            other => panic!("expected a save request, got {other:?}"),
        }
        assert!(!desk.window(id).unwrap().notepad().unwrap().is_dirty());
        let windows = desk.windows("09:41", true, None);
        let bar = windows
            .iter()
            .find(|w| w.style == WindowStyle::TopBar)
            .unwrap();
        assert_eq!(bar.frame.title.as_deref(), Some("Notepad"));
    }
}
