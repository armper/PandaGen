//! Timer (GFX-077): a stopwatch with laps, and a countdown that says so
//! when it is done.
//!
//! Time is the kernel's tick, 100 Hz, handed in by the desk: the card
//! never reads a clock itself, so `cargo test` can run a whole countdown
//! in no time at all. When a countdown reaches zero the desk raises a
//! notice -- through the notices centre like everything else the desk
//! says -- so the card need not be on screen, or even on this space, to
//! be heard.
//!
//! Every control is a real button (GFX-082) that stands for a key, so
//! the pointer and the keyboard reach the same code; the time is drawn
//! three times the font's size, and a countdown shows what is left as a
//! bar.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use view_types::PixelRect;

use crate::widgets::{grid, rect, ButtonKind, Palette, Ui};

/// Ticks per second.
pub const HZ: u64 = 100;
/// The countdown presets, in minutes, as the card offers them.
pub const PRESETS: [u64; 4] = [1, 5, 10, 25];
/// Where the card's parts go, top to bottom, in canvas pixels.
pub const TIME_TOP: i32 = 20;
pub const BAR_TOP: i32 = 76;
pub const CONTROLS_TOP: i32 = 96;
pub const PRESETS_TOP: i32 = 172;
pub const LAPS_TOP: i32 = 222;
const LAP_PITCH: i32 = 18;

/// The controls' and presets' rectangles for a canvas `width` wide
/// (GFX-082): what the drawing and the desk's hit test agree on.
#[derive(Debug, Clone)]
pub struct TimerLayout {
    pub controls: Vec<PixelRect>,
    pub presets: Vec<PixelRect>,
}

impl TimerLayout {
    pub fn new(width: u32) -> Self {
        Self {
            controls: grid(rect(0, CONTROLS_TOP, width, 44), 3, 1, 6),
            presets: grid(rect(0, PRESETS_TOP, width, 36), 4, 1, 6),
        }
    }
}
/// Laps kept.
pub const MAX_LAPS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Stopwatch,
    /// Counting down from this many ticks.
    Countdown(u64),
}

/// What a key or click asks the desk to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerEffect {
    None,
    Redraw,
    Close,
}

/// The timer's whole state.
#[derive(Debug, Clone)]
pub struct TimerView {
    pub mode: Mode,
    /// Ticks accumulated while running before the last pause.
    banked: u64,
    /// The tick the current run started at, while running.
    started: Option<u64>,
    /// The last tick the desk told us about.
    now: u64,
    laps: Vec<u64>,
    /// The countdown reached zero and the desk was told.
    fired: bool,
}

impl Default for TimerView {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerView {
    pub fn new() -> Self {
        Self {
            mode: Mode::Stopwatch,
            banked: 0,
            started: None,
            now: 0,
            laps: Vec::new(),
            fired: false,
        }
    }

    pub fn running(&self) -> bool {
        self.started.is_some()
    }

    /// Ticks run so far.
    pub fn elapsed(&self) -> u64 {
        self.banked
            + self
                .started
                .map(|s| self.now.saturating_sub(s))
                .unwrap_or(0)
    }

    /// What the big line shows: elapsed, or what is left.
    pub fn shown_ticks(&self) -> u64 {
        match self.mode {
            Mode::Stopwatch => self.elapsed(),
            Mode::Countdown(total) => total.saturating_sub(self.elapsed()),
        }
    }

    /// The desk's clock. Returns a message once when a countdown ends.
    pub fn poll(&mut self, now: u64) -> Option<String> {
        self.now = now;
        if let Mode::Countdown(total) = self.mode {
            if self.running() && self.elapsed() >= total && !self.fired {
                self.fired = true;
                self.banked = total;
                self.started = None;
                return Some(alloc::format!("Timer: {} up", describe(total)));
            }
        }
        None
    }

    fn start_or_pause(&mut self) {
        if let Some(started) = self.started.take() {
            self.banked += self.now.saturating_sub(started);
        } else {
            if let Mode::Countdown(total) = self.mode {
                if self.banked >= total {
                    // Done: Start runs it again.
                    self.banked = 0;
                    self.fired = false;
                }
            }
            self.started = Some(self.now);
        }
    }

    fn reset(&mut self) {
        self.banked = 0;
        self.started = None;
        self.laps.clear();
        self.fired = false;
    }

    fn lap(&mut self) {
        if self.mode == Mode::Stopwatch && self.running() {
            self.laps.insert(0, self.elapsed());
            self.laps.truncate(MAX_LAPS);
        }
    }

    fn set_countdown(&mut self, minutes: u64) {
        self.mode = Mode::Countdown(minutes * 60 * HZ);
        self.reset();
        self.started = Some(self.now);
    }

    /// A key: Space starts or pauses, `l` laps, `r` resets, `s` is the
    /// stopwatch, `1`..`4` start a preset countdown.
    pub fn handle_byte(&mut self, byte: u8) -> TimerEffect {
        use crate::notepad::{CTRL_W, ESC};
        match byte {
            b' ' | b'\n' | b'\r' => self.start_or_pause(),
            b'l' | b'L' => self.lap(),
            b'r' | b'R' => self.reset(),
            b's' | b'S' => {
                self.mode = Mode::Stopwatch;
                self.reset();
            }
            b'1'..=b'4' => self.set_countdown(PRESETS[(byte - b'1') as usize]),
            CTRL_W | ESC => return TimerEffect::Close,
            _ => return TimerEffect::None,
        }
        TimerEffect::Redraw
    }

    /// The card, drawn (GFX-082): the mode, the time large, a bar of
    /// what is left while counting down, the controls, the presets, the
    /// laps. `hover` is the pointer in canvas pixels, if over the card.
    pub fn ui(&self, width: u32, height: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        let layout = TimerLayout::new(width);
        let mut ui = Ui::new(palette, hover);
        let p = *ui.palette();
        let heading = match self.mode {
            Mode::Stopwatch => "Stopwatch".to_string(),
            Mode::Countdown(total) => alloc::format!("Countdown  {}", describe(total)),
        };
        ui.text(0, 0, &heading, p.muted, 1);
        let time_ink = if self.fired { p.accent } else { p.text };
        ui.text_centered(
            &rect(0, TIME_TOP, width, 48),
            &format_ticks(self.shown_ticks()),
            time_ink,
            3,
        );
        if let Mode::Countdown(total) = self.mode {
            // What is left, as a bar.
            ui.fill(rect(0, BAR_TOP, width, 8), p.raised, 4);
            let left = self.shown_ticks().min(total);
            let filled = ((width as u64 * left) / total.max(1)) as u32;
            if filled > 0 {
                ui.fill(rect(0, BAR_TOP, filled, 8), p.accent, 4);
            }
        }
        let start = if self.running() { "Pause" } else { "Start" };
        let start_kind = if self.running() {
            ButtonKind::Plain
        } else {
            ButtonKind::Primary
        };
        ui.button(layout.controls[0], start, b' ', start_kind);
        if self.mode == Mode::Stopwatch {
            ui.button(layout.controls[1], "Lap", b'l', ButtonKind::Plain);
        } else {
            ui.button(layout.controls[1], "Stopwatch", b's', ButtonKind::Plain);
        }
        ui.button(layout.controls[2], "Reset", b'r', ButtonKind::Quiet);
        ui.text(0, PRESETS_TOP - 20, "Countdown", p.muted, 1);
        for (i, (cell, minutes)) in layout.presets.iter().zip(PRESETS.iter()).enumerate() {
            let label = alloc::format!("{minutes}m");
            ui.button(*cell, &label, b'1' + i as u8, ButtonKind::Accent);
        }
        let mut y = LAPS_TOP;
        if !self.laps.is_empty() {
            ui.text(0, y, "Laps", p.muted, 1);
            y += LAP_PITCH;
            let count = self.laps.len();
            for (i, lap) in self.laps.iter().enumerate() {
                if y + LAP_PITCH > height as i32 {
                    break;
                }
                let previous = self.laps.get(i + 1).copied().unwrap_or(0);
                let line = alloc::format!(
                    "{:>2}   {}   +{}",
                    count - i,
                    format_ticks(*lap),
                    format_ticks(lap - previous)
                );
                ui.text(8, y, &line, p.text, 1);
                y += LAP_PITCH;
            }
        } else if self.fired {
            ui.text(0, y, "Done", p.accent, 1);
        }
        ui
    }

    pub fn footer(&self) -> String {
        match self.mode {
            Mode::Stopwatch => "Space starts and pauses   L laps   R resets".to_string(),
            Mode::Countdown(_) if self.fired => {
                "Done, said in Notices   Start runs it again".to_string()
            }
            Mode::Countdown(_) => "Space pauses   R resets   S stopwatch".to_string(),
        }
    }
}

/// Which `[ button ]` on `text` covers `column`, counting from 0.
pub fn button_at(text: &str, column: usize) -> Option<usize> {
    let mut index = 0;
    let mut open: Option<usize> = None;
    for (i, ch) in text.chars().enumerate() {
        match ch {
            '[' => open = Some(i),
            ']' => {
                if let Some(start) = open.take() {
                    if (start..=i).contains(&column) {
                        return Some(index);
                    }
                    index += 1;
                }
            }
            _ => {}
        }
    }
    None
}

/// `mm:ss.t`, or `h:mm:ss` past an hour.
pub fn format_ticks(ticks: u64) -> String {
    let tenths = (ticks / (HZ / 10)) % 10;
    let secs = ticks / HZ;
    if secs >= 3600 {
        alloc::format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
    } else {
        alloc::format!("{:02}:{:02}.{}", secs / 60, secs % 60, tenths)
    }
}

/// `5 minutes`, `1 minute`, `90 seconds`.
pub fn describe(ticks: u64) -> String {
    let secs = ticks / HZ;
    match secs {
        60 => "1 minute".to_string(),
        s if s % 60 == 0 => alloc::format!("{} minutes", s / 60),
        s => alloc::format!("{s} seconds"),
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

    fn texts(timer: &TimerView) -> Vec<String> {
        timer
            .ui(384, 360, palette(), None)
            .into_ops()
            .into_iter()
            .filter_map(|op| match op {
                DrawOp::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_stopwatch_runs_on_the_desks_ticks_pauses_and_laps() {
        let mut timer = TimerView::new();
        timer.poll(1_000);
        assert!(texts(&timer).contains(&"00:00.0".to_string()));
        assert_eq!(timer.handle_byte(b' '), TimerEffect::Redraw);
        timer.poll(1_230);
        assert!(texts(&timer).contains(&"00:02.3".to_string()));
        timer.handle_byte(b'l');
        timer.poll(1_500);
        timer.handle_byte(b'l');
        let shown = texts(&timer);
        assert!(shown.contains(&"Laps".to_string()));
        assert!(
            shown.contains(&" 2   00:05.0   +00:02.7".to_string()),
            "{shown:?}"
        );
        assert!(
            shown.contains(&" 1   00:02.3   +00:02.3".to_string()),
            "{shown:?}"
        );
        // Pause holds; resume continues from where it was.
        timer.handle_byte(b' ');
        timer.poll(9_000);
        assert_eq!(timer.elapsed(), 500);
        assert!(texts(&timer).contains(&"Start".to_string()));
        timer.handle_byte(b' ');
        timer.poll(9_100);
        assert_eq!(timer.elapsed(), 600);
        assert!(texts(&timer).contains(&"Pause".to_string()));
        assert_eq!(format_ticks(3_661 * HZ), "1:01:01");
        timer.handle_byte(b'r');
        assert_eq!(timer.elapsed(), 0);
        assert!(!texts(&timer).contains(&"Laps".to_string()));
    }

    #[test]
    fn a_countdown_says_so_once_when_it_is_up_and_buttons_are_hit_by_pixel() {
        let mut timer = TimerView::new();
        timer.poll(0);
        let layout = TimerLayout::new(384);
        assert_eq!(layout.controls.len(), 3);
        assert_eq!(layout.presets.len(), 4);
        // The second preset, clicked: five minutes, running at once.
        let cell = layout.presets[1];
        let key = timer
            .ui(384, 360, palette(), None)
            .hit(cell.x as i32 + 3, cell.y as i32 + 3)
            .expect("a preset there");
        assert_eq!(key, b'2');
        assert_eq!(
            timer
                .ui(384, 360, palette(), None)
                .hit(cell.x as i32 - 2, cell.y as i32 + 3),
            None,
            "the gap"
        );
        assert_eq!(timer.handle_byte(key), TimerEffect::Redraw);
        assert_eq!(timer.mode, Mode::Countdown(5 * 60 * HZ));
        assert!(timer.running());
        let shown = texts(&timer);
        assert!(shown.contains(&"Countdown  5 minutes".to_string()));
        assert!(shown.contains(&"05:00.0".to_string()));
        assert_eq!(timer.poll(100), None);
        assert!(texts(&timer).contains(&"04:59.0".to_string()));
        // The bar: a full-width track and an accent fill nearly as wide.
        let ops = timer.ui(384, 360, palette(), None).into_ops();
        let bars: Vec<u32> = ops
            .iter()
            .filter_map(|op| match op {
                DrawOp::RoundedFill { rect, .. } if rect.y == BAR_TOP as u32 => Some(rect.width),
                _ => None,
            })
            .collect();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0], 384);
        assert!((380..384).contains(&bars[1]), "{bars:?}");
        assert_eq!(
            timer.poll(5 * 60 * HZ + 7),
            Some("Timer: 5 minutes up".to_string())
        );
        assert_eq!(timer.poll(5 * 60 * HZ + 200), None, "said once");
        assert!(!timer.running());
        let shown = texts(&timer);
        assert!(shown.contains(&"00:00.0".to_string()));
        assert!(shown.contains(&"Done".to_string()));
        assert!(timer.footer().starts_with("Done"));
        // Reset, then Start runs it again from the top.
        let reset = layout.controls[2];
        let key = timer
            .ui(384, 360, palette(), None)
            .hit(reset.x as i32 + 3, reset.y as i32 + 3)
            .unwrap();
        assert_eq!(key, b'r');
        timer.handle_byte(key);
        assert_eq!(timer.elapsed(), 0);
        timer.handle_byte(b' ');
        timer.poll(5 * 60 * HZ + 300);
        assert!(texts(&timer).contains(&"04:59.0".to_string()));
        // The second control is "Stopwatch" while counting down.
        let second = layout.controls[1];
        assert!(texts(&timer).contains(&"Stopwatch".to_string()));
        let key = timer
            .ui(384, 360, palette(), None)
            .hit(second.x as i32 + 3, second.y as i32 + 3)
            .unwrap();
        assert_eq!(key, b's');
        timer.handle_byte(key);
        assert_eq!(timer.mode, Mode::Stopwatch);
        assert_eq!(timer.handle_byte(0x1B), TimerEffect::Close);
        assert_eq!(describe(90 * HZ), "90 seconds");
        assert_eq!(describe(60 * HZ), "1 minute");
    }

    #[test]
    fn bracketed_words_are_still_found_by_column() {
        assert_eq!(button_at("[ Add ] [ Done ]", 3), Some(0));
        assert_eq!(button_at("[ Add ] [ Done ]", 7), None);
        assert_eq!(button_at("[ Add ] [ Done ]", 12), Some(1));
    }
}
